use std::{collections::HashMap, net::{SocketAddr, UdpSocket}, sync::{Mutex, Arc, RwLock}};

use mrial_proto::packet::*;

pub struct Client {
    last_ping: u64,
    src: SocketAddr
}

impl Client {
    pub fn new(src: SocketAddr) -> Self {
        Self {
            src,
            last_ping: 0
        }
    }

    pub fn is_alive(&self) -> bool {
        true
    }
}

pub struct Connections {
    clients: Arc<RwLock<HashMap<String, Client>>>,
    socket: UdpSocket,
}

impl Connections {
    pub fn new(socket: UdpSocket) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            socket,
        }
    }

    pub fn has_clients(&self) -> bool {
        self.clients.read().unwrap().len() > 0
    }

    pub fn remove_client(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        self.clients.write().unwrap().remove(&src_str);
    }

    pub fn add_client(&mut self, src: SocketAddr) {
        let src_str = src.to_string();
        println!("Adding client: {}", src_str);
        
        self.clients.write().unwrap().insert(src_str, Client::new(src));
        let mut buf = [0u8; HEADER];
        write_header(
            EPacketType::SHOOK, 
            0, 
            HEADER.try_into().unwrap(), 
            0, 
            &mut buf
        );
        self.socket.send_to(&buf, src).unwrap();
    }

    #[inline]
    pub fn broadcast(&self, buf: &[u8]) {
        for client in self.clients.read().unwrap().values() {
            self.socket.send_to(buf, client.src).unwrap();
        }
    }

    pub fn clone(&self) -> Self {
        Self {
            clients: self.clients.clone(),
            socket: self.socket.try_clone().unwrap()
        }
    }
}