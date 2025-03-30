use std::{fmt, net::SocketAddr, sync::Arc};

use app::AppConnection;
use bytes::Bytes;
use kanal::AsyncReceiver;
use mrial_proto::EPacketType;
use tokio::sync::RwLock;
use web::WebConnection;

pub mod app;
pub mod web;

pub type PacketTypeVariant = u8;

#[derive(Debug)]
pub enum BroadcastTaskError {
    TransferFailed(String),
    EncryptionFailed(String),
    TaskNotRunning,
}

impl fmt::Display for BroadcastTaskError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BroadcastTaskError::TaskNotRunning => write!(f, "Broadcast Task is not running"),
            BroadcastTaskError::EncryptionFailed(msg) => write!(f, "Encryption Failed: {}", msg),
            BroadcastTaskError::TransferFailed(msg) => write!(f, "Transfer Failed: {}", msg),
        }
    }
}

impl std::error::Error for BroadcastTaskError {}

pub trait Client {
    fn is_alive(&self) -> bool;
}

#[derive(Debug, Clone)]
pub struct ServerMeta {
    pub width: usize,
    pub height: usize,
    pub opus: bool
}

impl Default for ServerMeta {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            opus: true
        }
    }
}

pub struct ConnectionManager {
    web: WebConnection,
    app: AppConnection,
    meta: Arc<RwLock<ServerMeta>>,
}

impl ConnectionManager {
    pub async fn new() -> Self {
        Self {
            web: WebConnection::new(),
            app: AppConnection::new().await,
            meta: Arc::new(RwLock::new(ServerMeta::default())),
        }
    }

    pub fn get_web(&self) -> WebConnection {
        self.web.clone()
    }

    pub fn get_app(&self) -> AppConnection {
        self.app.clone()
    }

    #[inline]
    pub async fn is_opus(&self) -> bool {
        let meta = self.meta.read().await;
        meta.opus
    }

    pub async fn get_meta(&self) -> ServerMeta {
        let meta = self.meta.read().await;
        meta.clone()
    }

    pub async fn set_meta(&self, meta: ServerMeta) -> bool {
        let mut stream_restart_flag = false;

        let mut current_meta = self.meta.write().await;

        if current_meta.width != meta.width
            || current_meta.height != meta.height
        {
            stream_restart_flag = true;
        }

        current_meta.width = meta.width;
        current_meta.height = meta.height;
        current_meta.opus = meta.opus;

        stream_restart_flag
    }

    pub async fn set_dimensions(&self, width: usize, height: usize) {
        let mut meta = self.meta.write().await;

        meta.width = width;
        meta.height = height;
    }

    #[inline]
    pub async fn has_web_clients(&self) -> bool {
        self.web.has_clients().await
    }

    #[inline]
    pub async fn has_app_clients(&self) -> bool {
        self.app.has_clients().await
    }

    #[inline]
    pub fn web_receiver(&self) -> AsyncReceiver<Bytes> {
        self.web.receiver()
    }

    #[inline]
    pub fn web_encrypted_broadcast(
        &self,
        packet_type: EPacketType,
        buf: &[u8],
    ) -> Result<(), BroadcastTaskError> {
        self.web.broadcast_frame(packet_type, buf)
    }

    #[inline]
    pub async fn app_retransmit_frame(
        &self,
        src: SocketAddr,
        frame_id: u8,
        real_packet_size: u32,
        subpacket_ids: Vec<u16>,
    ) {
        self.app.retransmit_frame(src, frame_id, real_packet_size, subpacket_ids).await
    }
    
    pub async fn app_drain_subpacket_cache(&self) {
        self.app.drain_subpacket_cache().await
    }

    #[inline]
    pub async fn app_encrypted_broadcast(
        &self,
        packet_type: EPacketType,
        packet_type_variant: PacketTypeVariant,
        buf: &[u8],
    ) -> Result<(), BroadcastTaskError> {
        self.app.broadcast_encrypted_frame(packet_type, packet_type_variant, buf).await
    }

    #[inline]
    pub async fn app_recv_from(
        &self,
        buf: &mut [u8],
    ) -> Result<(usize, SocketAddr), std::io::Error> {
        self.app.recv_from(buf).await
    }

    #[inline]
    pub async fn filter_clients(&self) {
        tokio::join! {
            self.web.filter_clients(),
            self.app.filter_clients(),
        };
    }

    #[inline]
    pub async fn has_clients(&self) -> bool {
        let (has_web_clients, has_app_clients) = tokio::join! {
            self.has_web_clients(),
            self.has_app_clients(),
        };

        has_web_clients || has_app_clients
    }
}

impl Clone for ConnectionManager {
    fn clone(&self) -> Self {
        Self {
            web: self.web.clone(),
            app: self.app.clone(),
            meta: self.meta.clone(),
        }
    }
}
