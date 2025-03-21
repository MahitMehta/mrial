use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use chacha20poly1305::{aead::KeyInit, ChaCha20Poly1305};
use kanal::{AsyncReceiver, Sender};
use log::{debug, error};
use mrial_fs::{storage::StorageMultiType, Users};
use rand::thread_rng;
use rsa::{pkcs1::EncodeRsaPublicKey, RsaPrivateKey, RsaPublicKey};
use std::{collections::HashMap, fmt, net::SocketAddr, sync::Arc, time::SystemTime};
use tokio::{net::UdpSocket, runtime::Handle, sync::RwLock, task::JoinHandle};

use mrial_proto::{
    deploy::{Broadcaster, PacketDeployer}, packet::*, ClientShakeAE, ClientStatePayload, JSONPayloadAE, JSONPayloadSE, JSONPayloadUE, ServerShookSE, ServerShookUE, ServerStatePayload, SERVER_PING_TOLERANCE
};

#[cfg(target_os = "linux")]
use crate::video::display::DisplayMeta;

use super::{BroadcastTaskError, Client};

const SERVER_DEFAULT_PORT: u16 = 8554;
const RSA_PRIVATE_KEY_BIT_SIZE: usize = 2048;

pub struct AppClient {
    last_ping: SystemTime,
    src: SocketAddr,
    muted: bool,
    connected: bool,
    priv_key: Option<RsaPrivateKey>,
    sym_key: Arc<RwLock<Option<ChaCha20Poly1305>>>,
}

impl AppClient {
    pub fn new(src: SocketAddr, priv_key: RsaPrivateKey) -> Self {
        Self {
            src,
            priv_key: Some(priv_key),
            muted: false,
            connected: false,
            last_ping: SystemTime::now(),
            sym_key: Arc::new(RwLock::new(None)),
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
            AppConnectionError::InvalidCredentials => {
                write!(f, "User Not Found, Failed to Authenticate")
            }
            AppConnectionError::ShakeAEDecryptionFailed => {
                write!(f, "Failed to Decrypt Client Shake AE Payload")
            }
            AppConnectionError::FailedToLoadUsers => {
                write!(f, "Failed to load Users, Failed to Authenticate")
            }
            AppConnectionError::ClientPrivateKeyNotFound => {
                write!(f, "Client Private Key Not Found")
            }
            AppConnectionError::Unexpected(e) => write!(f, "Unexpected Error: {}", e),
        }
    }
}

type BroadcastPayload = (EPacketType, Vec<u8>);

struct AppBroadcastTask {
    receiver: AsyncReceiver<BroadcastPayload>,

    audio_deployer: PacketDeployer,
    audio_broadcaster: AppAudioBroadcaster,
    video_deployer: PacketDeployer,
    video_broadcaster: AppVideoBroadcaster,
}

struct AppVideoBroadcaster {
    socket: Arc<UdpSocket>,
    clients: Arc<RwLock<HashMap<String, AppClient>>>,
}

impl Broadcaster for AppVideoBroadcaster {
    async fn broadcast(&self, bytes: &[u8]) {
        let clients = self.clients.read().await;

        for client in clients.values() {
            if let Err(e) = self.socket.send_to(bytes, client.src).await {
                debug!("Failed to Broadcast Video to Client (Disconnecting): {}", e);
                
                let src_str: String = client.src.to_string();
                self.clients.write().await.remove(&src_str);
            }
        }
    }
}

struct AppAudioBroadcaster {
    socket: Arc<UdpSocket>,
    clients: Arc<RwLock<HashMap<String, AppClient>>>,
}

impl Broadcaster for AppAudioBroadcaster {
    async fn broadcast(&self, bytes: &[u8]) {
        let clients = self.clients.read().await;

        for client in clients.values() {
            if client.muted {
                continue;
            }
            if let Err(e) = self.socket.send_to(bytes, client.src).await {
                debug!("Failed to Broadcast Audio to Client (Disconnecting): {}", e);
                
                let src_str: String = client.src.to_string();
                self.clients.write().await.remove(&src_str);
            }
        }
    }
}

impl AppBroadcastTask {
    #[inline]
    async fn broadcast(&mut self, payload: BroadcastPayload) {
       let (packet_type, buf) = payload;

       match packet_type {
            EPacketType::NAL => {
                self.video_deployer.slice_and_send(&buf, &self.video_broadcaster).await;
            }
            EPacketType::Audio => {
                self.audio_deployer.slice_and_send(&buf, &self.audio_broadcaster).await;
            }
            _ => {
                error!("Unsupported Packet Type (Dropping): {:?}", packet_type);
            }
       }
    }

    async fn broadcast_loop(&mut self) {
        while let Ok(payload) = self.receiver.recv().await {
            self.broadcast(payload).await;
        }
    }

    pub fn run(
        tokio_handle: Handle,
        socket: Arc<UdpSocket>,
        clients: Arc<RwLock<HashMap<String, AppClient>>>,
        receiver: AsyncReceiver<BroadcastPayload>,
    ) -> JoinHandle<()> {
        tokio_handle.spawn(async move {
            let mut thread = Self { 
                receiver,
                audio_deployer: PacketDeployer::new(EPacketType::Audio, false),
                video_deployer: PacketDeployer::new(EPacketType::NAL, false),
                video_broadcaster: AppVideoBroadcaster {
                    socket: socket.clone(),
                    clients: clients.clone()
                },
                audio_broadcaster: AppAudioBroadcaster {
                    socket,
                    clients
                }
            };

            thread.broadcast_loop().await;
        })
    }
}

pub struct AppConnection {
    socket: Arc<UdpSocket>,
    clients: Arc<RwLock<HashMap<String, AppClient>>>,
    users: Users,

    broadcast_sender: Sender<BroadcastPayload>,
    broadcast_receiver: AsyncReceiver<BroadcastPayload>,
    broadcast_task: Arc<std::sync::RwLock<Option<JoinHandle<()>>>>,
}

impl AppConnection {
    pub async fn new() -> Self {
        let server_address = SocketAddr::from(([0, 0, 0, 0], SERVER_DEFAULT_PORT));
        let socket = UdpSocket::bind(server_address).await.expect(&format!(
            "Failed to Bind UDP Socket at Port:{}",
            SERVER_DEFAULT_PORT
        ));
        let users = Users::new();

        let (broadcast_sender, broadcast_receiver) = kanal::unbounded();

        Self {
            broadcast_sender,
            broadcast_receiver: broadcast_receiver.clone_async(),
            broadcast_task: Arc::new(std::sync::RwLock::new(None)),

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

            if let Some(client) = self.clients.write().await.get_mut(&src_str) {
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

    pub async fn get_sym_key(&self) -> Option<ChaCha20Poly1305> {
        if let Some(client) = self
            .clients
            .read()
            .await
            .values()
            .find(|client| client.is_connected())
        {
            return client.sym_key.read().await.clone();
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
            *client.sym_key.write().await = Some(sym_key.clone());

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

            let wrapped_sym_key = client.sym_key.read().await.clone();
            let payload_len = match ServerShookSE::write_payload(
                &mut buf[HEADER..],
                wrapped_sym_key,
                &ServerShookSE {
                    server_state: ServerStatePayload {
                        widths,
                        heights,
                        width: 0,
                        height: 0,
                        header: Some(Vec::new()),
                    },
                },
            ) {
                Ok(len) => len,
                Err(_) => {
                    return Err(AppConnectionError::Unexpected(
                        "Failed to Write Server Shook SE Payload".to_string(),
                    ));
                }
            };
            debug!("Server Shook SE Payload Len: {}", payload_len);

            if let Err(e) = self.socket
                .send_to(&buf[..HEADER + payload_len], &src)
                .await 
            {
                return Err(AppConnectionError::Unexpected(e.to_string()));
            }

            // TODO: Send NAL Header
            // let header_bytes = match headers.lock() {
            //     Ok(headers) => headers.clone(),
            //     Err(_) => None,
            // };

            return Ok(payload.client_state);
        }

        return Err(AppConnectionError::Unexpected(
            "Client Not Found in Clients HashMap".to_string(),
        ));
    }

    pub async fn initialize_client(&self, src: SocketAddr) -> Result<(), std::io::Error> {
        let src_str = src.to_string();
        debug!("Initial Shake UE With Client: {}", src_str);

        let priv_key = RsaPrivateKey::new(&mut thread_rng(), RSA_PRIVATE_KEY_BIT_SIZE)
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

    /// This function starts the broadcast spawns tokio async task
    /// that listens for app broadcast messages and sends them to all.
    /// Note: This function should be called from the main thread from inside the video server.
    pub fn start_broadcast_async_task(&self) {
        if let Ok(mut thread) = self.broadcast_task.write() {
            if thread.is_none() {
                *thread = Some(AppBroadcastTask::run(
                    Handle::current(),
                    self.socket.clone(),
                    self.clients.clone(),
                    self.broadcast_receiver.clone(),
                ));
            }
        }
    }

    #[inline]
    pub async fn broadcast_encrypted_frame(&self, packet_type: EPacketType, buf: &[u8]) -> Result<(), BroadcastTaskError> {
        let sym_key = match self.get_sym_key().await {
            Some(sym_key) => sym_key,
            None => {
                return Err(BroadcastTaskError::EncryptionFailed("Symmetric Key Not Found".to_string()));
            },
        };

        match encrypt_frame(&sym_key, buf) {
            Ok(encrypted_frame) => {
                if let Ok(task) = self.broadcast_task.read() {
                    if task.is_none() {
                        return Err(BroadcastTaskError::TaskNotRunning);
                    }
                }
        
                if let Err(e) = self.broadcast_sender.send((packet_type, encrypted_frame)) {
                    return Err(BroadcastTaskError::TransferFailed(e.to_string()));
                }

                return Ok(());
            }
            Err(e) => {
                return Err(BroadcastTaskError::EncryptionFailed(e.to_string()));
            }
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub async fn raw_broadcast(&self, buf: &[u8]) {
        let clients = self.clients.read().await;

        for client in clients.values() {
            if let Err(e) = self.socket.send_to(buf, client.src).await {
                debug!("Failed to Broadcast to Client: {}", e);
                self.remove_client(client.src).await;
            }
        }
    }

    #[inline]
    pub async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), std::io::Error> {
        self.socket.recv_from(buf).await
    }
}

impl Clone for AppConnection {
    fn clone(&self) -> Self {
        Self {
            broadcast_sender: self.broadcast_sender.clone(),
            broadcast_receiver: self.broadcast_receiver.clone(),
            broadcast_task: self.broadcast_task.clone(),

            socket: self.socket.clone(),
            clients: self.clients.clone(),
            users: self.users.clone(),
        }
    }
}
