use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    time::SystemTime,
};

use mrial_proto::{packet::*, write_state_payload, ServerStatePayload, SERVER_STATE_PAYLOAD, CLIENT_STATE_PAYLOAD, SERVER_PING_TOLERANCE};

use crate::video::display::DisplayMeta;

const SERVER_DEFAULT_PORT: u16 = 8554;

pub struct Client {
    last_ping: SystemTime,
    src: SocketAddr,
}

impl Client {
    pub fn new(src: SocketAddr) -> Self {
        Self {
            src,
            last_ping: SystemTime::now(),
        }
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
    socket: UdpSocket
}

impl Connection {
    pub fn new() -> Self {
        let server_address = SocketAddr::from(([0, 0, 0, 0], SERVER_DEFAULT_PORT));
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            meta: Arc::new(RwLock::new(ServerMetaData {
                width: 0,
                height: 0,
            })),
            socket: UdpSocket::bind(server_address).expect("Failed to Establish UdpSocket"),
        }
    }
    
    pub fn get_meta(&self) -> std::sync::RwLockReadGuard<'_, ServerMetaData> {
        self.meta.read().unwrap()
    }

    pub fn set_dimensions(&self, width: usize, height: usize) {
        self.meta.write().unwrap().width = width;
        self.meta.write().unwrap().height = height;
    }

    #[inline]
    pub fn client_pinged(&self, src: SocketAddr) {
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

    #[inline]
    pub fn filter_clients(&self) {
        let mut clients = self.clients.write().unwrap();
        clients.retain(|_, client| client.is_alive());
    }

    #[inline]
    pub fn has_clients(&self) -> bool {
        self.clients.read().unwrap().len() > 0
    }

    pub fn remove_client(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        self.clients.write().unwrap().remove(&src_str);
    }

    pub fn add_client(&mut self, src: SocketAddr, headers: &[u8]) {
        let src_str = src.to_string();
        // ### DEBUG ###
        {
            println!("Adding client: {}", src_str);
        }

        self.clients
            .write()
            .unwrap()
            .insert(src_str, Client::new(src));
        let mut buf = [0u8; HEADER + SERVER_STATE_PAYLOAD];
        write_header(
            EPacketType::SHOOK,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut buf,
        );

        let mut amt = HEADER;
        
        // TODO: Windows implementation needed
        #[cfg(target_os = "linux")]
        if let Ok((widths, heights)) = DisplayMeta::get_display_resolutions() {
            amt += write_state_payload(&mut buf[HEADER..], ServerStatePayload {
                widths,
                heights,
                width: 0,
                height: 0
            });
        }

        self.socket.send_to(&buf[..amt], src).unwrap();

        let mut buf = [0u8; MTU];
        write_header(EPacketType::NAL, 0, HEADER.try_into().unwrap(), 0, &mut buf);
        buf[HEADER..HEADER + headers.len()].copy_from_slice(headers);
        self.socket
            .send_to(&buf[0..HEADER + headers.len()], src)
            .unwrap();
    }

    #[inline]
    pub fn broadcast(&self, buf: &[u8]) {
        for client in self.clients.read().unwrap().values() {
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
        }
    }
}
