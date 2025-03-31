use std::{
    net::{SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use chacha20poly1305::{aead::KeyInit, ChaCha20Poly1305};
use kanal::Sender;
use log::{debug, info};
use mrial_fs::Server;
use mrial_proto::{video::EColorSpace, *};
use rsa::{pkcs1::DecodeRsaPublicKey, RsaPublicKey};

use crate::{ClientState, ConnectionAction};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

impl Default for ClientMetaData {
    fn default() -> Self {
        ClientMetaData {
            width: 0,
            height: 0,
            widths: vec![],
            heights: vec![],
            muted: false,
            opus: true,
            colorspace: EColorSpace::YUV444,
            server: Server::default(),
        }
    }
}

#[derive(Debug)]
pub struct ClientMetaData {
    pub width: usize,
    pub height: usize,
    pub widths: Vec<u16>,
    pub heights: Vec<u16>,
    pub muted: bool,
    pub opus: bool,
    pub colorspace: EColorSpace,
    pub server: Server,
}

pub struct Client {
    socket_address: String,
    socket: Option<UdpSocket>,
    state: ConnectionState,
    meta: Arc<RwLock<ClientMetaData>>,
    sym_key: Arc<RwLock<Option<ChaCha20Poly1305>>>,
    conn_sender: Sender<ConnectionAction>,
}

impl Client {
    pub fn new(meta: ClientMetaData, conn_sender: Sender<ConnectionAction>) -> Client {
        Client {
            socket_address: String::new(),
            socket: None,
            state: ConnectionState::Disconnected,
            meta: Arc::new(RwLock::new(meta)),
            sym_key: Arc::new(RwLock::new(None)),
            conn_sender,
        }
    }

    pub fn get_meta_clone(&self) -> Arc<RwLock<ClientMetaData>> {
        self.meta.clone()
    }

    pub fn set_meta_via_state(&mut self, state: &ClientState) {
        if let Ok(mut meta_handle) = self.meta.write() {
            meta_handle.opus = state.opus;
            meta_handle.muted = state.muted;
        }
    }

    pub fn get_meta(&self) -> std::sync::RwLockReadGuard<ClientMetaData> {
        self.meta.read().unwrap()
    }

    pub fn set_socket_address(&mut self, ip_addr: &String, port: u16) {
        self.socket_address = format!("{}:{}", ip_addr, port);
    }

    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    pub fn connect(&mut self) {
        if !self.socket_connected() && self.state == ConnectionState::Connecting {
            let client_address = SocketAddr::from(([0, 0, 0, 0], 0));
            let socket = UdpSocket::bind(client_address).expect("Failed to Bind to Local Port");
            match socket.connect(&self.socket_address) {
                Ok(_) => self.socket = Some(socket),
                Err(_e) => {
                    println!("Failed to Connect to Server: {}", &self.socket_address);
                    thread::sleep(Duration::from_millis(1000));
                    return;
                }
            }
        }

        self.send_handshake();
    }

    /// Retransmit a packet
    /// # Arguments
    /// * `frame_id` - The ID of the frame to retransmit
    /// * `real_packet_size` - The size of the packet
    /// * `subpacket_ids` - The IDs of the packets to be retransmitted
    pub fn retransmit(
        &self, 
        frame_id: u8, 
        real_packet_size: u32, 
        subpacket_ids: Vec<u16>
    ) -> Result<usize, std::io::Error> {
        let mut buf = [0u8; MTU];

        // Send multiple retransmit packets if there are too many subpacket IDs
        let body_len = write_retransmit_body(
            frame_id, real_packet_size, subpacket_ids, &mut buf[HEADER..],);
        write_header(
            EPacketType::Retransmit, 
            0, 
            body_len as u32, 
            frame_id, 
            &mut buf);

        if let Some(socket) = &self.socket {
            return Ok(socket.send(&buf[0..HEADER + body_len])?);
        }

        debug!("Socket Not Initialized");
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Socket Not Initialized",
        ))
    }

    pub fn disconnect(&mut self) {
        if !self.socket_connected() {
            return;
        }

        let mut buf = [0u8; HEADER];
        write_header(
            EPacketType::Disconnect,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut buf,
        );
        let _ = self.socket.as_ref().unwrap().send(&buf);
        info!("Sent Disconnection Packet");

        self.socket = None;
        self.state = ConnectionState::Disconnected;
    }

    #[inline]
    pub fn connection_state(&self) -> &ConnectionState {
        &self.state
    }

    #[inline]
    pub fn socket_connected(&self) -> bool {
        self.socket.is_some()
    }

    #[inline]
    pub fn connected(&self) -> bool {
        self.socket_connected() && self.state == ConnectionState::Connected
    }

    #[inline]
    pub fn get_sym_key(&self) -> Arc<RwLock<Option<ChaCha20Poly1305>>> {
        self.sym_key.clone()
    }

    pub fn clone(&self) -> Client {
        if let Some(socket) = &self.socket {
            let socket = socket.try_clone().unwrap();
            return Client {
                socket_address: self.socket_address.clone(),
                socket: Some(socket),
                state: self.state,
                meta: self.meta.clone(),
                sym_key: self.sym_key.clone(),
                conn_sender: self.conn_sender.clone(),
            };
        }

        Client {
            socket_address: self.socket_address.clone(),
            socket: None,
            sym_key: self.sym_key.clone(),
            state: ConnectionState::Disconnected,
            meta: self.meta.clone(),
            conn_sender: self.conn_sender.clone(),
        }
    }

    #[inline]
    pub fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> Result<(usize, std::net::SocketAddr), std::io::Error> {
        match &self.socket {
            None => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Socket Not Initialized",
            )),
            Some(socket) => {
                let (amt, src) = socket.recv_from(buf)?;
                Ok((amt, src))
            }
        }
    }

    #[inline]
    pub fn send(&self, buf: &[u8]) -> Result<usize, std::io::Error> {
        match &self.socket {
            None => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Socket Not Initialized",
            )),
            Some(socket) => {
                let amt = socket.send(buf)?;
                Ok(amt)
            }
        }
    }

    fn update_client_conn_state(&self, payload: ServerStatePayload) {
        if let Ok(mut meta_handle) = self.meta.write() {
            meta_handle.widths = payload.widths;
            meta_handle.heights = payload.heights;

            let _ = &self.conn_sender.send(ConnectionAction::UpdateState);
        }
    }

    pub fn send_handshake(&mut self) {
        if let Some(socket) = &self.socket {
            let _ = socket
                .set_read_timeout(Some(Duration::from_millis(1000)))
                .expect("Failed to Set Timeout");
            let mut buf = [0u8; MTU];

            write_header(EPacketType::ShakeUE, 0, HEADER as u32, 0, &mut buf);

            let _ = socket.send(&buf[0..HEADER]);
            debug!("Sent Initial Shake UE Packet");

            let (amt, _src) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(_e) => return,
            };

            if parse_packet_type(&buf) == EPacketType::ShookUE {
                debug!("Received Initial Shook UE Packet");

                if let Ok(shookue_payload) = ServerShookUE::from_payload(&mut buf[HEADER..amt]) {
                    if let Ok(pub_key) = RsaPublicKey::from_pkcs1_pem(&shookue_payload.pub_key) {
                        debug!("Valid Public Key Received");

                        let client_state = match self.meta.read() {
                            Ok(meta) => ClientStatePayload {
                                width: meta.width as u16,
                                height: meta.height as u16,
                                muted: meta.muted,
                                opus: meta.opus,
                                csp: meta.colorspace
                            },
                            Err(_e) => {
                                debug!("Failed to Read Client Meta Data");
                                return;
                            }
                        };

                        let mut rng = rand::thread_rng();
                        let key = ChaCha20Poly1305::generate_key(&mut rng);
                        let cipher = ChaCha20Poly1305::new(&key);
                        *self.sym_key.write().unwrap() = Some(cipher);
                        let key_vec = key.to_vec();
                        let key_base64 = STANDARD_NO_PAD.encode(&key_vec);

                        let payload_len = ClientShakeAE::write_payload(
                            &mut buf[HEADER..],
                            &mut rng,
                            pub_key,
                            &ClientShakeAE {
                                username: self.meta.read().unwrap().server.username.clone(),
                                pass: self.meta.read().unwrap().server.pass.clone(),
                                sym_key: key_base64,
                                state: client_state,
                            },
                        );

                        write_header(
                            EPacketType::ShakeAE,
                            0,
                            (HEADER + payload_len) as u32,
                            0,
                            &mut buf,
                        );
                        let _ = socket.send(&buf[0..HEADER + payload_len]);
                        debug!("Sent Shake AE Packet");

                        // Wait for Shook SE Packet by Waiting at most a 100 Packets
                        for _ in 0u8..100 {
                            let (amt, _src) = match socket.recv_from(&mut buf) {
                                Ok(v) => v,
                                Err(_e) => return,
                            };

                            if parse_packet_type(&buf) == EPacketType::ShookSE {
                                let _ = socket
                                    .set_read_timeout(Some(Duration::from_millis(5000)))
                                    .expect("Failed to Set Timeout");
                                if let Ok(payload) = ServerShookSE::from_payload(
                                    &mut buf[HEADER..amt],
                                    self.sym_key.read().unwrap().clone().as_mut().unwrap(),
                                ) {
                                    debug!("Received Valid Shook SE Packet");
                                    self.update_client_conn_state(payload.server_state);

                                    // TODO: Validate if this is in the correct place
                                    self.state = ConnectionState::Connected;
                                    break;
                                };
                            }
                        }
                    }
                }
            }
        }
    }
}
