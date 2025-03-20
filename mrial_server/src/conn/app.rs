use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use chacha20poly1305::{aead::KeyInit, ChaCha20Poly1305};
use log::debug;
use mrial_fs::{storage::StorageMultiType, Users};
use rsa::{pkcs1::EncodeRsaPublicKey, RsaPrivateKey, RsaPublicKey};
use std::{
    collections::HashMap, fmt, net::SocketAddr, sync::Arc, time::SystemTime
};
use tokio::{net::UdpSocket, sync::{Mutex, RwLock}};

use mrial_proto::{
    packet::*, ClientShakeAE, ClientStatePayload, JSONPayloadAE, JSONPayloadSE, JSONPayloadUE,
    ServerShookSE, ServerShookUE, ServerStatePayload, SERVER_PING_TOLERANCE,
};

#[cfg(target_os = "linux")]
use crate::video::display::DisplayMeta;

use super::Client;

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

#[derive(Debug)]
pub enum AppConnectionError {
    InvalidCredentials,
    ShakeAEDecryptionFailed,
    FailedToLoadUsers,
    ClientPrivateKeyNotFound,
    Unexpected(String),
}

impl fmt::Display for AppConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AppConnectionError::InvalidCredentials => 
                write!(f, "User Not Found, Failed to Authenticate"),
            AppConnectionError::ShakeAEDecryptionFailed => 
                write!(f, "Failed to Decrypt Client Shake AE Payload"),
            AppConnectionError::FailedToLoadUsers =>
                write!(f, "Failed to load Users, Failed to Authenticate"),
            AppConnectionError::ClientPrivateKeyNotFound =>
                write!(f, "Client Private Key Not Found"),
            AppConnectionError::Unexpected(e) =>
                write!(f, "Unexpected Error: {}", e),
        }
    }
}

pub struct AppConnection {
    socket: Arc<UdpSocket>,
    clients: Arc<RwLock<HashMap<String, AppClient>>>,
    users: Users,
}

impl AppConnection {
    pub async fn new() -> Self {
        let server_address = SocketAddr::from(([0, 0, 0, 0], SERVER_DEFAULT_PORT));
        let socket = UdpSocket::bind(server_address).await.expect(&format!(
            "Failed to Bind UDP Socket at Port:{}",
            SERVER_DEFAULT_PORT
        ));
        let users = Users::new();

        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            socket: Arc::new(socket),
            users,
        }
    }

    #[inline]
    pub async fn filter_clients(&self) {
        let mut clients = self.clients.write().await;
        clients.retain(|_, client| client.is_alive());
    }

    #[inline]
    pub async fn has_clients(&self) -> bool {
        let clients = self.clients.read().await;

        clients
            .values()
            .find(|client| client.is_connected())
            .is_some()
    }

    #[inline]
    pub async fn send_alive(&self, src: SocketAddr) -> Result<usize, std::io::Error> {
        let mut buf = [0u8; HEADER];
        write_header(
            EPacketType::Alive,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut buf,
        );

        Ok(self.socket.send_to(&buf, src).await?)
    }

    #[inline]
    pub async fn received_ping(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        if self.clients.read().await.contains_key(&src_str) {
            let current = SystemTime::now();

            if let Some(client) = self.clients
                .write()
                .await
                .get_mut(&src_str) 
            {
                client.last_ping = current;
            }
        }
    }

    pub async fn mute_client(&self, src: SocketAddr, muted: bool) {
        let src_str = src.to_string();

        if let Some(client) = self.clients.write().await.get_mut(&src_str) {
            client.set_muted(muted)
        }
    }

    pub async fn remove_client(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        self.clients.write().await.remove(&src_str);
    }

    async fn get_client_priv_key(&self, src_str: &String) -> Option<RsaPrivateKey> {
        let clients = self.clients.read().await;

        if let Some(client) = clients.get(src_str) {
            return client.priv_key.clone();
        }

        return None;
    }

    pub fn get_sym_key_blocking(&self) -> Option<ChaCha20Poly1305> {
        if let Some(client) = self
            .clients
            .blocking_read()
            .values()
            .find(|client| client.is_connected() && client.sym_key.is_some())
        {
            return client.sym_key.clone();
        }

    None
    }

    pub async fn get_sym_key(&self) -> Option<ChaCha20Poly1305> {
        if let Some(client) = self
            .clients
            .read()
            .await
            .values()
            .find(|client| client.is_connected() && client.sym_key.is_some())
        {
            return client.sym_key.clone();
        }

        None
    }

    pub async fn connect_client(
        &mut self,
        src: SocketAddr,
        encypyted_payload: &[u8],
        _headers: Option<Vec<u8>>,
    ) -> Result<ClientStatePayload, AppConnectionError> {
        let src_str = src.to_string();

        let priv_key = match self.get_client_priv_key(&src_str).await {
            Some(priv_key) => priv_key,
            None => {
                return Err(AppConnectionError::ClientPrivateKeyNotFound);
            }
        };

        let payload = match ClientShakeAE::from_payload(encypyted_payload, priv_key) {
            Ok(payload) => payload,
            Err(_) => {
                return Err(AppConnectionError::ShakeAEDecryptionFailed);
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
                    return Err(AppConnectionError::InvalidCredentials);
                }
            }
            Err(_) => {
                return Err(AppConnectionError::FailedToLoadUsers);
            }
        }
        debug!("User Found, Authenticated");

        let mut clients = self.clients.write().await;
        if let Some(client) = clients.get_mut(&src_str) {
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
    
            #[cfg(target_os = "linux")]
            let mut widths = vec![0u16; 0];
            #[cfg(target_os = "linux")]
            let mut heights = vec![0u16; 0];
    
            #[cfg(not(target_os = "linux"))]
            let widths = vec![0u16; 0];
            #[cfg(not(target_os = "linux"))]
            let heights = vec![0u16; 0];
    
            // TODO: Windows and MacOS implementation needed
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
                        header: Some(Vec::new())
                    },
                },
            );
            println!("Server Shook SE Payload Len: {}", payload_len);
            self.socket
                .send_to(&buf[..HEADER + payload_len], &src)
                .await;
    
            // TODO: Send NAL Header
            // let header_bytes = match headers.lock() {
            //     Ok(headers) => headers.clone(),
            //     Err(_) => None,
            // };
    
            return Ok(payload.client_state)
        }

        return Err(AppConnectionError::Unexpected(
            "Client Not Found in Clients HashMap".to_string(),
        ));
    }

    pub async fn initialize_client(&self, src: SocketAddr) -> Result<(), std::io::Error> {
        let src_str = src.to_string();
        debug!("Initial Shake UE With Client: {}", src_str);

        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, RSA_PRIVATE_KEY_BIT_SIZE)
            .expect("Failed to Generate RSA Key Pair");
        let pub_key = RsaPublicKey::from(&priv_key);
        let pub_key_str = pub_key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();

        self.clients
            .write()
            .await
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

        self.socket.send_to(&buf[..amt], src).await?;
        debug!("Sent Shook UE Packet to Client: {}", src_str);

        Ok(())
    }

    #[inline]
    pub async fn broadcast(&self, buf: &[u8]) {
        let clients = self.clients.read().await;
        
        for client in clients.values() {
            if let Err(e) = self.socket.send_to(buf, client.src).await {
                debug!("Failed to Broadcast to Client: {}", e);
                self.remove_client(client.src).await;
            }
        }
    }

    #[inline]
    pub async fn broadcast_audio(&self, buf: &[u8]) {
        let clients = self.clients.read().await;

        for client in clients.values() {
            if client.muted {
                continue;
            }
            if let Err(e) = self.socket.send_to(buf, client.src).await {
                debug!("Failed to Broadcast Audio to Client: {}", e);
                self.remove_client(client.src).await;
            }
        }
    }

    #[inline]
    pub fn try_recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), std::io::Error> {
        self.socket.try_recv_from(buf)
    }
}

impl Clone for AppConnection {
    fn clone(&self) -> Self {
        Self {
            socket: self.socket.clone(),
            clients: self.clients.clone(),
            users: self.users.clone(),
        }
    }
}
