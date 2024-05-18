use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use chacha20poly1305::{aead::KeyInit, ChaCha20Poly1305};
use log::debug;
use rsa::{pkcs1::EncodeRsaPublicKey, RsaPrivateKey, RsaPublicKey};
use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    time::SystemTime,
};

use mrial_proto::{
    packet::*, ClientShakeAE, ClientStatePayload, JSONPayloadAE, JSONPayloadSE, JSONPayloadUE,
    ServerShookSE, ServerShookUE, ServerStatePayload, SERVER_PING_TOLERANCE, SERVER_STATE_PAYLOAD,
};

use crate::video::display::DisplayMeta;

const SERVER_DEFAULT_PORT: u16 = 8554;

pub struct Client {
    last_ping: SystemTime,
    src: SocketAddr,
    muted: bool,
    connected: bool,
    priv_key: RsaPrivateKey,
    sym_key: Option<ChaCha20Poly1305>,
}

impl Client {
    pub fn new(src: SocketAddr, priv_key: RsaPrivateKey) -> Self {
        Self {
            src,
            priv_key,
            muted: false,
            connected: false,
            last_ping: SystemTime::now(),
            sym_key: None,
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Client is connected with an encrypted tunnel and is authenticated
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn is_alive(&self) -> bool {
        self.last_ping.elapsed().unwrap().as_secs() < SERVER_PING_TOLERANCE
    }
}

pub struct ServerMetaData {
    pub width: usize,
    pub height: usize,
}

pub struct Connection {
    clients: Arc<RwLock<HashMap<String, Client>>>,
    meta: Arc<RwLock<ServerMetaData>>,
    socket: UdpSocket,
    // audio_deployer: PacketDeployer,
    // video_deployer: PacketDeployer,
}

impl Connection {
    pub fn new() -> Self {
        let server_address = SocketAddr::from(([0, 0, 0, 0], SERVER_DEFAULT_PORT));
        let socket = UdpSocket::bind(server_address).expect(&format!(
            "Failed to Bind UDP Socket at Port:{}",
            SERVER_DEFAULT_PORT
        ));

        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            meta: Arc::new(RwLock::new(ServerMetaData {
                width: 0,
                height: 0,
            })),
            socket: socket.try_clone().unwrap(),
            // audio_deployer: PacketDeployer::new(socket.try_clone().unwrap(), EPacketType::Audio),
            // video_deployer: PacketDeployer::new(socket, EPacketType::NAL),
        }
    }

    pub fn get_meta(&self) -> std::sync::RwLockReadGuard<'_, ServerMetaData> {
        self.meta.read().unwrap()
    }

    pub fn set_dimensions(&self, width: usize, height: usize) {
        self.meta.write().unwrap().width = width;
        self.meta.write().unwrap().height = height;
    }

    pub fn send_alive(&self, src: SocketAddr) {
        let mut buf = [0u8; HEADER];
        write_header(
            EPacketType::Alive, 
            0, 
            HEADER.try_into().unwrap(),
            0, 
            &mut buf
        );
        self.socket.send_to(&buf, src).unwrap();
    }

    #[inline]
    pub fn received_ping(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        if self.clients.read().unwrap().contains_key(&src_str) {
            let current = SystemTime::now();

            self.clients
                .write()
                .unwrap()
                .get_mut(&src_str)
                .unwrap()
                .last_ping = current;
        }
    }

    pub fn mute_client(&self, src: SocketAddr, muted: bool) {
        let src_str = src.to_string();

        if let Some(client) = self.clients.write().unwrap().get_mut(&src_str) {
            client.set_muted(muted)
        }
    }

    #[inline]
    pub fn filter_clients(&self) {
        let mut clients = self.clients.write().unwrap();
        clients.retain(|_, client| client.is_alive());
    }

    #[inline]
    pub fn has_clients(&self) -> bool {
        self.clients
            .read()
            .unwrap()
            .values()
            .find(|client| client.is_connected())
            .is_some()
    }

    pub fn remove_client(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        self.clients.write().unwrap().remove(&src_str);
    }

    fn get_client_priv_key(&self, src_str: &String) -> Option<RsaPrivateKey> {
        if let Some(client) = self.clients.read().unwrap().get(src_str) {
            return Some(client.priv_key.clone());
        }

        return None;
    }

    pub fn get_sym_key(&self) -> Option<ChaCha20Poly1305> {
        if let Some(client) = self.clients
            .read()
            .unwrap()
            .values()
            .find(|client| client.is_connected() && client.sym_key.is_some()) {
                return client.sym_key.clone();
            }

        None
    }

    pub fn connect_client(
        &self,
        src: SocketAddr,
        encypyted_payload: &[u8],
        headers: &[u8],
    ) -> Option<ClientStatePayload> {
        let src_str = src.to_string();

        if let Some(priv_key) = self.get_client_priv_key(&src_str) {
            let payload = ClientShakeAE::from_payload(encypyted_payload, priv_key).unwrap();

            // TODO: Validate User Credentials
            debug!("Client Shake AE by User: {:?}", payload.username);

            if let Some(client) = self.clients.write().unwrap().get_mut(&src_str) {
                let sym_key_vec = STANDARD_NO_PAD.decode(&payload.sym_key).unwrap();
                let sym_key = ChaCha20Poly1305::new_from_slice(&sym_key_vec).unwrap();

                client.connected = true;
                client.sym_key = Some(sym_key.clone());

                let mut buf = [0u8; MTU];
                write_header(
                    EPacketType::ShookSE,
                    0,
                    HEADER.try_into().unwrap(),
                    0,
                    &mut buf,
                );

                let mut widths = vec![0u16; 0];
                let mut heights = vec![0u16; 0];

                // TODO: Windows implementation needed
                #[cfg(target_os = "linux")]
                if let Ok((w, h)) = DisplayMeta::get_display_resolutions() {
                    widths = w;
                    heights = h;
                }

                let mut rng = rand::thread_rng();
                let payload_len = ServerShookSE::write_payload(
                    &mut buf[HEADER..],
                    &mut rng,
                    &mut client.sym_key.clone().unwrap(),
                    &ServerShookSE {
                        server_state: ServerStatePayload {
                            widths,
                            heights,
                            width: 0,
                            height: 0,
                        },
                    },
                );
                self.socket
                    .send_to(&buf[..HEADER + payload_len], &src)
                    .unwrap();

                // let mut buf = [0u8; MTU];
                // write_header(EPacketType::NAL, 0, HEADER.try_into().unwrap(), 0, &mut buf);
                // buf[HEADER..HEADER + headers.len()].copy_from_slice(headers);
                // self.socket
                //     .send_to(&buf[0..HEADER + headers.len()], src)
                //     .unwrap();

                return Some(payload.client_state);
            }
        }

        None
    }

    pub fn initialize_client(&self, src: SocketAddr) {
        let src_str = src.to_string();
        debug!("Initial Shake UE With Client: {}", src_str);

        let mut rng = rand::thread_rng();
        let bits = 2048;
        let priv_key = RsaPrivateKey::new(&mut rng, bits).expect("Failed to Generate RSA Key Pair");
        let pub_key = RsaPublicKey::from(&priv_key);
        let pub_key_str = pub_key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();

        self.clients
            .write()
            .unwrap()
            .insert(src_str.clone(), Client::new(src, priv_key));

        let mut buf = [0u8; MTU];
        write_header(
            EPacketType::ShookUE,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut buf,
        );

        let mut amt = HEADER;

        amt += ServerShookUE::write_payload(
            &mut buf[HEADER..],
            &ServerShookUE {
                pub_key: pub_key_str,
            },
        );

        self.socket.send_to(&buf[..amt], src).unwrap();
        debug!("Sent Shook UE Packet to Client: {}", src_str);
    }

    #[inline]
    pub fn broadcast(&self, buf: &[u8]) {
        for client in self.clients.read().unwrap().values() {
            self.socket.send_to(buf, client.src).unwrap();
        }
    }

    #[inline]
    pub fn broadcast_audio(&self, buf: &[u8]) {
        for client in self.clients.read().unwrap().values() {
            if client.muted {
                continue;
            }
            self.socket.send_to(buf, client.src).unwrap();
        }
    }

    #[inline]
    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), std::io::Error> {
        self.socket.recv_from(buf)
    }

    pub fn clone(&self) -> Self {
        Self {
            clients: self.clients.clone(),
            meta: self.meta.clone(),
            socket: self.socket.try_clone().unwrap(),
            // audio_deployer: self.audio_deployer.try_clone().unwrap(),
            // video_deployer: self.video_deployer.try_clone().unwrap()
        }
    }
}
