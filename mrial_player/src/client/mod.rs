use std::{
    fmt,
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

#[derive(Debug, PartialEq)]
pub enum HandshakeError {
    VersionMismatch(String, String),
    FailedToReceiveShookSE(String),
    FailedToSendShakeUE(String),
    FailedToSetTimeout(String),
    InvalidPublicKey(String),
    Other(String),
    FailedToReceiveShakeUE(String),
    SocketNotInitialized,
    FailedToReceiveShookUE(String),
    InvalidShakeUEPayload(String),
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HandshakeError::InvalidShakeUEPayload(err) => {
                write!(f, "Invalid Shake UE Payload: {err}")
            }
            HandshakeError::FailedToReceiveShookUE(err) => {
                write!(f, "Failed to Receive Shook UE Packet: {err}")
            }
            HandshakeError::VersionMismatch(server_version, client_version) => {
                write!(
                    f,
                    "Version Mismatch: {server_version} (Server) != {client_version} (Client)",
                )
            }
            HandshakeError::FailedToReceiveShookSE(err) => {
                write!(f, "Failed to Receive Shook SE Packet: {err}")
            }
            HandshakeError::FailedToSendShakeUE(err) => {
                write!(f, "Failed to Send Shake UE Packet: {err}")
            }
            HandshakeError::FailedToSetTimeout(err) => {
                write!(f, "Failed to Set Timeout: {err}")
            }
            HandshakeError::SocketNotInitialized => {
                write!(f, "Socket Not Initialized")
            }
            HandshakeError::FailedToReceiveShakeUE(err) => {
                write!(f, "Failed to Receive Initial Shake UE Packet: {err}")
            }
            HandshakeError::InvalidPublicKey(err) => {
                write!(f, "Invalid Public Key: {err}")
            }
            HandshakeError::Other(err) => {
                write!(f, "{err}")
            }
        }
    }
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
            meta_handle.colorspace = match state.colorspace.as_str() {
                "limited" => EColorSpace::YUV420,
                "full" => EColorSpace::YUV444,
                _ => EColorSpace::YUV444,
            }
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

    pub fn connect(&mut self) -> Result<(), HandshakeError> {
        if !self.socket_connected() && self.state == ConnectionState::Connecting {
            let client_address = SocketAddr::from(([0, 0, 0, 0], 0));
            let socket = match UdpSocket::bind(client_address) {
                Ok(socket) => socket,
                Err(e) => {
                    return Err(HandshakeError::Other(format!(
                        "Failed to Bind Socket: {}",
                        e
                    )));
                }
            };
            match socket.connect(&self.socket_address) {
                Ok(_) => self.socket = Some(socket),
                Err(_e) => {
                    thread::sleep(Duration::from_millis(1000));
                    return Err(HandshakeError::Other(format!(
                        "Socket Failed to Connect to Server: {}",
                        &self.socket_address
                    )));
                }
            }
        }

        self.send_handshake()
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
        subpacket_ids: Vec<u16>,
    ) -> Result<usize, std::io::Error> {
        let mut buf = [0u8; MTU];

        // Send multiple retransmit packets if there are too many subpacket IDs
        let body_len = write_retransmit_body(
            frame_id,
            real_packet_size,
            subpacket_ids,
            &mut buf[HEADER..],
        );
        write_header(
            EPacketType::Retransmit,
            0,
            body_len as u32,
            frame_id,
            &mut buf,
        );

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

    pub fn send_handshake(&mut self) -> Result<(), HandshakeError> {
        let socket = match &self.socket {
            Some(socket) => socket,
            None => return Err(HandshakeError::SocketNotInitialized),
        };

        if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(1000))) {
            return Err(HandshakeError::FailedToSetTimeout(e.to_string()));
        }

        let mut buf = [0u8; MTU];
        write_header(EPacketType::ShakeUE, 0, 0, 0, &mut buf);

        if let Err(e) = socket.send(&buf[0..HEADER]) {
            return Err(HandshakeError::FailedToSendShakeUE(e.to_string()));
        };
        debug!("Sent Initial Shake UE Packet");

        let (amt, _src) = match socket.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) => return Err(HandshakeError::FailedToReceiveShakeUE(e.to_string())),
        };

        match parse_packet_type(&buf) {
            EPacketType::ShookUE => {}
            _other => {
                return Err(HandshakeError::FailedToReceiveShookUE(format!(
                    "Instead got {:?}",
                    _other
                )))
            }
        }

        debug!("Received Initial Shook UE Packet");

        let shookue_payload = match ServerShookUE::from_payload(&mut buf[HEADER..amt]) {
            Ok(payload) => payload,
            Err(e) => return Err(HandshakeError::InvalidShakeUEPayload(e.to_string())),
        };

        let pub_key = match RsaPublicKey::from_pkcs1_pem(&shookue_payload.pub_key) {
            Ok(key) => key,
            Err(e) => {
                return Err(HandshakeError::InvalidPublicKey(e.to_string()));
            }
        };

        debug!("Valid Public Key Received");

        let client_state = match self.meta.read() {
            Ok(meta) => ClientStatePayload {
                width: meta.width as u16,
                height: meta.height as u16,
                muted: meta.muted,
                opus: meta.opus,
                csp: meta.colorspace,
            },
            Err(e) => {
                return Err(HandshakeError::Other(format!(
                    "Failed to Read Local Client Meta: {e}"
                )));
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

        // Wait for Shook SE Packet by waiting at most a 100 Packets
        for _ in 0..100 {
            let (amt, _src) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) => {
                    return Err(HandshakeError::FailedToReceiveShookSE(e.to_string()));
                }
            };

            if parse_packet_type(&buf) != EPacketType::ShookSE {
                continue;
            }

            if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(5000))) {
                return Err(HandshakeError::FailedToSetTimeout(e.to_string()));
            };

            if let Ok(payload) = ServerShookSE::from_payload(
                &mut buf[HEADER..amt],
                self.sym_key.read().unwrap().clone().as_mut().unwrap(),
            ) {
                debug!("Received Valid Shook SE Packet");

                if payload.server_state.version != env!("CARGO_PKG_VERSION").to_string() {
                    return Err(HandshakeError::VersionMismatch(
                        payload.server_state.version,
                        env!("CARGO_PKG_VERSION").to_string(),
                    ));
                }

                self.update_client_conn_state(payload.server_state);
                self.state = ConnectionState::Connected;
                return Ok(());
            };
        }

        Err(HandshakeError::FailedToReceiveShookSE(
            "Exhausted Retries".to_string(),
        ))
    }
}
