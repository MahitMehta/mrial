use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use app::AppConnection;
use web::WebConnection;

pub mod app;
pub mod web;

pub trait Client {
    fn is_alive(&self) -> bool;
}

pub trait Connection {
    fn filter_clients(&self);

    fn has_clients(&self) -> bool;
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
    pub fn new() -> Self {
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
    pub fn has_web_clients(&self) -> bool {
        self.web.has_clients()
    }

    #[inline]
    pub fn has_app_clients(&self) -> bool {
        self.app.has_clients()
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
}

impl Connection for ConnectionManager {
    fn filter_clients(&self) {
        self.web.filter_clients();
        self.app.filter_clients();
    }

    fn has_clients(&self) -> bool {
        self.web.has_clients() || self.app.has_clients()
    }
}
