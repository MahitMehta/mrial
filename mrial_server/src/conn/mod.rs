use std::{collections::HashMap, net::{SocketAddr, UdpSocket}, sync::{Mutex, Arc, RwLock}, time::{SystemTime, UNIX_EPOCH}};

use mrial_proto::packet::*;

pub struct Client {
    last_ping: SystemTime,
    src: SocketAddr
}

const ALIVE_TOLERANCE: u64 = 6; // seconds

impl Client {
    pub fn new(src: SocketAddr) -> Self {
        Self {
            src,
            last_ping: SystemTime::now()
        }
    }

    pub fn is_alive(&self) -> bool {
        self.last_ping.elapsed().unwrap().as_secs() < ALIVE_TOLERANCE  
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
    
    pub fn ping_client(&self, src: SocketAddr) {
        let src_str: String = src.to_string();
        if self.clients.read().unwrap().contains_key(&src_str) {
            let current = SystemTime::now();

            self.clients.write()
            .unwrap()
            .get_mut(&src_str)
            .unwrap().last_ping = current;
        }
    }
    
    pub fn  filter_clients(&self) {
        let mut clients = self.clients.write().unwrap();
        clients.retain(|_, client| client.is_alive());
    }
    
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

        let mut buf = [0u8; MTU];
        write_header(
            EPacketType::NAL, 
            0, 
            HEADER.try_into().unwrap(), 
            0, 
            &mut buf
        );
        buf[HEADER..HEADER + headers.len()].copy_from_slice(headers);
        self.socket.send_to(&buf[0..HEADER + headers.len()], src).unwrap();
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