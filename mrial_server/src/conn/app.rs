use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use chacha20poly1305::{aead::KeyInit, ChaCha20Poly1305};
use log::debug;
use mrial_fs::{storage::StorageMultiType, Users};
use rsa::{pkcs1::EncodeRsaPublicKey, RsaPrivateKey, RsaPublicKey};
use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    time::SystemTime,
};

use mrial_proto::{
    packet::*, ClientShakeAE, ClientStatePayload, JSONPayloadAE, JSONPayloadSE, JSONPayloadUE,
    ServerShookSE, ServerShookUE, ServerStatePayload, SERVER_PING_TOLERANCE,
};

#[cfg(target_os = "linux")]
use crate::video::display::DisplayMeta;

use super::{Client, Connection};

const SERVER_DEFAULT_PORT: u16 = 8554;
const RSA_PRIVATE_KEY_BIT_SIZE: usize = 2048;

pub struct AppClient {
    last_ping: SystemTime,
    src: SocketAddr,
    muted: bool,
    connected: bool,
    priv_key: Option<RsaPrivateKey>,
    sym_key: Option<ChaCha20Poly1305>,
}

impl AppClient {
    pub fn new(src: SocketAddr, priv_key: RsaPrivateKey) -> Self {
        Self {
            src,
            priv_key: Some(priv_key),
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
}

impl Client for AppClient {
    fn is_alive(&self) -> bool {
        let alive = self.last_ping.elapsed().unwrap().as_secs() < SERVER_PING_TOLERANCE;

        if !alive {
            debug!("Client: {} is Dead", self.src);
        }

        alive
    }
}

impl Connection for AppConnection {
    #[inline]
    fn filter_clients(&self) {
        if let Ok(mut clients) = self.clients.write() {
            clients.retain(|_, client| client.is_alive());
        }
    }

    #[inline]
    fn has_clients(&self) -> bool {
        if let Ok(clients) = self.clients.read() {
            return clients
                .values()
                .find(|client| client.is_connected())
                .is_some();
        }

        false
    }
}

pub struct AppConnection {
    socket: UdpSocket,
    clients: Arc<RwLock<HashMap<String, AppClient>>>,
    users: Users,
}

impl AppConnection {
    pub fn new() -> Self {
        let server_address = SocketAddr::from(([0, 0, 0, 0], SERVER_DEFAULT_PORT));
        let socket = UdpSocket::bind(server_address).expect(&format!(
            "Failed to Bind UDP Socket at Port:{}",
            SERVER_DEFAULT_PORT
        ));
        let users = Users::new();

        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            socket: socket.try_clone().unwrap(),
            users,
        }
    }

    pub fn send_alive(&self, src: SocketAddr) {
        let mut buf = [0u8; HEADER];
        write_header(
            EPacketType::Alive,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut buf,
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

    pub fn remove_client(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        self.clients.write().unwrap().remove(&src_str);
    }

    fn get_client_priv_key(&self, src_str: &String) -> Option<RsaPrivateKey> {
        if let Some(client) = self.clients.read().unwrap().get(src_str) {
            return client.priv_key.clone();
        }

        return None;
    }

    pub fn get_sym_key(&self) -> Option<ChaCha20Poly1305> {
        if let Some(client) = self
            .clients
            .read()
            .unwrap()
            .values()
            .find(|client| client.is_connected() && client.sym_key.is_some())
        {
            return client.sym_key.clone();
        }

        None
    }

    pub fn connect_client(
        &mut self,
        src: SocketAddr,
        encypyted_payload: &[u8],
        _headers: &[u8],
    ) -> Option<ClientStatePayload> {
        let src_str = src.to_string();

        if let Some(priv_key) = self.get_client_priv_key(&src_str) {
            let payload = match ClientShakeAE::from_payload(encypyted_payload, priv_key) {
                Ok(payload) => payload,
                Err(_) => {
                    debug!("Failed to Decrypt Client Shake AE Payload");
                    return None;
                }
            };

            debug!("Client Shake AE by User: {:?}", payload.username);
            match self.users.load() {
                Ok(_) => {
                    if self
                        .users
                        .find_user_by_credentials(&payload.username, &payload.pass)
                        .is_none()
                    {
                        debug!("User Not Found, Failed to Authenticate");
                        return None;
                    }
                }
                Err(_) => {
                    debug!("Failed to Reload Users, Failed to Authenticate");
                    return None;
                }
            }
            debug!("User Found, Authenticated");

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

                // TODO: Send NAL Header
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
        let priv_key = RsaPrivateKey::new(
            &mut rng, RSA_PRIVATE_KEY_BIT_SIZE)
            .expect("Failed to Generate RSA Key Pair");
        let pub_key = RsaPublicKey::from(&priv_key);
        let pub_key_str = pub_key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();

        self.clients
            .write()
            .unwrap()
            .insert(src_str.clone(), AppClient::new(src, priv_key));

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
        if let Ok(clients) = self.clients.read() {
            for client in clients.values() {
                if let Err(e) = self.socket.send_to(buf, client.src) {
                    debug!("Failed to Broadcast to Client: {}", e);
                    self.remove_client(client.src);
                }
            }
        }
    }

    #[inline]
    pub fn broadcast_audio(&self, buf: &[u8]) {
        if let Ok(clients) = self.clients.read() {
            for client in clients.values() {
                if client.muted {
                    continue;
                }
                if let Err(e) = self.socket.send_to(buf, client.src) {
                    debug!("Failed to Broadcast Audio to Client: {}", e);
                    self.remove_client(client.src);
                }
            }
        }
    }

    #[inline]
    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), std::io::Error> {
        self.socket.recv_from(buf)
    }

    pub fn try_clone(&self) -> Result<Self, std::io::Error> {
        let socket = self.socket.try_clone()?;

        Ok(Self {
            clients: self.clients.clone(),
            socket,
            users: self.users.clone(),
        })
    }
}
