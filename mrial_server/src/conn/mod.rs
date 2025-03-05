use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use app::AppConnection;
use bytes::Bytes;
use tokio::runtime::Handle;
use web::{BroadcastTaskError, WebConnection};

pub mod app;
pub mod web;

pub trait Client {
    fn is_alive(&self) -> bool;
}

#[derive(Debug, Clone)]
pub struct ServerMeta {
    pub width: usize,
    pub height: usize,
}

pub struct ConnectionManager {
    web: WebConnection,
    app: AppConnection,
    meta: Arc<RwLock<ServerMeta>>,
}

impl ConnectionManager {
    pub fn new(_tokio_handle: Handle) -> Self {
        Self {
            web: WebConnection::new(),
            app: AppConnection::new(),
            meta: Arc::new(RwLock::new(ServerMeta {
                width: 0,
                height: 0,
            })),
        }
    }

    pub fn get_web(&self) -> Result<WebConnection, ()> {
        Ok(self.web.clone())
    }

    pub fn get_app(&self) -> Result<AppConnection, std::io::Error> {
        Ok(self.app.try_clone()?)
    }

    pub fn get_meta(&self) -> Option<ServerMeta> {
        if let Ok(meta) = self.meta.read() {
            return Some(meta.clone());
        }

        None
    }

    pub fn set_dimensions(&self, width: usize, height: usize) {
        if let Ok(mut meta) = self.meta.write() {
            meta.width = width;
            meta.height = height;
        }
    }

    #[inline]
    pub async fn has_web_clients(&self) -> bool {
        self.web.has_clients().await
    }

    #[inline]
    pub fn has_app_clients(&self) -> bool {
        self.app.has_clients()
    }

    #[inline]
    pub fn web_broadcast(&self, buf: &[u8]) -> Result<(), BroadcastTaskError> {
        let bytes = Bytes::copy_from_slice(buf);
        self.web.broadcast(bytes)
    }

    #[inline]
    pub fn app_broadcast(&self, buf: &[u8]) {
        self.app.broadcast(buf);
    }

    #[inline]
    pub fn app_recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), std::io::Error> {
        self.app.recv_from(buf)
    }

    #[inline]
    pub fn app_broadcast_audio(&self, buf: &[u8]) {
        self.app.broadcast_audio(buf);
    }

    pub fn try_clone(&self) -> Result<Self, std::io::Error> {
        let app = self.app.try_clone()?;

        Ok(Self {
            app,
            web: self.web.clone(),
            meta: self.meta.clone(),
        })
    }

    #[inline]
    pub async fn filter_clients(&self) {
        self.web.filter_clients().await;
        self.app.filter_clients();
    }

    #[inline]
    pub async fn has_clients(&self) -> bool {
        self.app.has_clients() || self.web.has_clients().await
    }

    #[inline]
    pub fn has_clients_blocking(&self) -> bool {
        self.app.has_clients() || self.web.has_clients_blocking()
    }

    #[inline]
    pub fn has_web_clients_blocking(&self) -> bool {
        self.web.has_clients_blocking()
    }
}
