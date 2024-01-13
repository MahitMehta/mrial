use std::{
    net::{SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use mrial_proto::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug)]
pub struct ClientMetaData {
    pub width: usize,
    pub height: usize,
}

pub struct Client {
    socket_address: String,
    socket: Option<UdpSocket>,
    state: ConnectionState,
    meta: Arc<RwLock<ClientMetaData>>,
}

const CLIENT_PORT: u16 = 8000;

impl Client {
    pub fn new(meta: ClientMetaData) -> Client {
        Client {
            socket_address: String::new(),
            socket: None,
            state: ConnectionState::Disconnected,
            meta: Arc::new(RwLock::new(meta)),
        }
    }

    pub fn get_meta(&self) -> std::sync::RwLockReadGuard<ClientMetaData> {
        self.meta.read().unwrap()
    }

    pub fn set_socket_address(&mut self, ip_addr: String, port: u16) {
        self.socket_address = format!("{}:{}", ip_addr, port);
    }

    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    pub fn connect(&mut self) {
        if !self.socket_connected() && self.state == ConnectionState::Connecting {
            let client_address = SocketAddr::from(([0, 0, 0, 0], CLIENT_PORT));
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

    pub fn disconnect(&mut self) {
        let mut buf = [0u8; HEADER];
        write_header(
            EPacketType::DISCONNECT,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut buf,
        );
        let _ = self.socket.as_ref().unwrap().send(&buf);

        self.socket = None;
        self.state = ConnectionState::Disconnected;
    }

    #[inline]
    pub fn connection_state(&self) -> &ConnectionState {
        return &self.state;
    }

    #[inline]
    pub fn socket_connected(&self) -> bool {
        return self.socket.is_some();
    }

    #[inline]
    pub fn connected(&self) -> bool {
        return self.socket_connected() && self.state == ConnectionState::Connected;
    }

    pub fn clone(&self) -> Client {
        if let Some(socket) = &self.socket {
            let socket = socket.try_clone().unwrap();
            return Client {
                socket_address: self.socket_address.clone(),
                socket: Some(socket),
                state: self.state,
                meta: self.meta.clone(),
            };
        }

        return Client {
            socket_address: self.socket_address.clone(),
            socket: None,
            state: ConnectionState::Disconnected,
            meta: self.meta.clone(),
        };
    }

    #[inline]
    pub fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> Result<(usize, std::net::SocketAddr), std::io::Error> {
        match &self.socket {
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Socket Not Initialized",
                ))
            }
            Some(socket) => {
                let (amt, src) = socket.recv_from(buf)?;
                return Ok((amt, src));
            }
        }
    }

    #[inline]
    pub fn send(&self, buf: &[u8]) -> Result<usize, std::io::Error> {
        match &self.socket {
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Socket Not Initialized",
                ))
            }
            Some(socket) => {
                let amt = socket.send(buf)?;
                return Ok(amt);
            }
        }
    }
    
    pub fn send_handshake(&mut self) {
        if let Some(socket) = &self.socket {
            let _ = socket
                .set_read_timeout(Some(Duration::from_millis(1000)))
                .expect("Failed to Set Timeout");
            let mut buf = [0u8; HEADER + HANDSHAKE_PAYLOAD];

            write_header(EPacketType::SHAKE, 0, HEADER as u32, 0, &mut buf);

            let meta = self.meta.read().unwrap();
            write_handshake_payload(&mut buf[HEADER..], EHandshakePayload { 
                width: meta.width.try_into().unwrap(),
                height: meta.height.try_into().unwrap()
            });

            let _ = socket.send(&buf);
            println!("Sent Handshake Packet");

            let (_amt, _src) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(_e) => return,
            };

            if buf[0] == EPacketType::SHOOK as u8 {
                println!("Received Handshake Packet");
                let _ = socket
                    .set_read_timeout(Some(Duration::from_millis(5000)))
                    .expect("Failed to Set Timeout");
            }
            self.state = ConnectionState::Connected;
        }
    }
}
