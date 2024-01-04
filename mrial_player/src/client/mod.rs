use std::{time::Duration, net::UdpSocket, thread};

use mrial_proto::*; 

#[derive(Clone, PartialEq, Debug)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected
}

pub struct Client {
    socket_address: String,
    socket: Option<UdpSocket>,
    state: ConnectionState
}

const CLIENT_ADDR: &'static str = "0.0.0.0:8080";

impl Client {
    pub fn new() -> Client {
        Client {
            socket_address: String::new(),
            socket: None,
            state: ConnectionState::Disconnected
        }
    }

    pub fn set_socket_address(&mut self, ip_addr: String, port: u16) {
        self.socket_address = format!("{}:{}", ip_addr, port);
    }

    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    pub fn connect(&mut self) {
        if !self.socket_connected() && self.state == ConnectionState::Connecting {
            let socket = UdpSocket::bind(CLIENT_ADDR).expect("Failed to Bind to Local Port");
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

    pub fn try_clone(&self) -> Client {
        if let Some(socket) = &self.socket {
            let socket = socket.try_clone().unwrap();
            return Client {
                socket_address: self.socket_address.clone(),
                socket: Some(socket),
                state: self.state.clone()
            }
        } 

        return Client {
            socket_address: self.socket_address.clone(),
            socket: None,
            state: ConnectionState::Disconnected
        }
    }

    #[inline]
    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, std::net::SocketAddr), std::io::Error> {
        match &self.socket {
            None => return Err(std::io::Error::new(std::io::ErrorKind::Other, "Socket Not Initialized")),
            Some(socket) => {
                let (amt, src) = socket.recv_from(buf)?;
                return Ok((amt, src));
            }
        }
    }

    #[inline]
    pub fn send(&self, buf: &[u8]) -> Result<usize, std::io::Error> {
        match &self.socket {
            None => return Err(std::io::Error::new(std::io::ErrorKind::Other, "Socket Not Initialized")),
            Some(socket) => {
                let amt = socket.send(buf)?;
                return Ok(amt);
            }
        }
    }

    pub fn send_handshake(&mut self) {
        if let Some(socket) = &self.socket {
            let _ = socket.set_read_timeout(Some(Duration::from_millis(1000))).expect("Failed to Set Timeout");
            let mut buf: [u8; HEADER] = [0; HEADER];
            
            write_header(
                EPacketType::SHAKE, 
                0, 
                HEADER as u32,
                &mut buf
            );

            let _ = socket.send(&buf);
            println!("Sent Handshake Packet");
            
            // TODO: Validate SRC
            let (_amt, _src) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(_e) => return,
            };
    
            if buf[0] == EPacketType::SHOOK as u8 {
                println!("Received Handshake Packet");
                let _ = socket.set_read_timeout(Some(Duration::from_millis(5000))).expect("Failed to Set Timeout");
            }         

            self.state = ConnectionState::Connected;
        }
    }
}