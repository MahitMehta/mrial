use std::{time::Duration, net::UdpSocket};

use mrial_proto::*; 

pub struct Client {
    socket: Option<UdpSocket>
}

const SERVER_ADDR: &'static str = "150.136.127.166:8554";
const CLIENT_ADDR: &'static str = "0.0.0.0:8080";

impl Client {
    pub fn new() -> Client {
        Client {
            socket: None
        }
    }

    pub fn connect(&mut self) {
        if !self.connected() {
            let socket = UdpSocket::bind(CLIENT_ADDR).expect("Failed to Bind to Incoming Socket");
            self.socket = Some(socket);
        }

        self.send_handshake();
    }

    pub fn disconnect(&mut self) {
        self.socket = None;
    }

    #[inline]
    pub fn connected(&self) -> bool {
        return self.socket.is_some();
    }

    pub fn try_clone(&self) -> Client {
        if let Some(socket) = &self.socket {
            let socket = socket.try_clone().unwrap();
            return Client {
                socket: Some(socket)
            }
        } 

        return Client {
            socket: None
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
                let amt = socket.send_to(buf, SERVER_ADDR)?;
                return Ok(amt);
            }
        }
    }

    pub fn send_handshake(&self) {
        if let Some(socket) = &self.socket {
            let _ = socket.set_read_timeout(Some(Duration::from_millis(1000))).expect("Failed to Set Timeout");
            let mut buf: [u8; HEADER] = [0; HEADER];
            
            write_header(
                EPacketType::SHAKE, 
                0, 
                HEADER as u32,
                &mut buf
            );

            loop {
                let _ = socket.send_to(&buf, SERVER_ADDR);
                println!("Sent Handshake Packet");
                
                // validate src
                let (_amt, _src) = match socket.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(_e) => continue,
                };
        
                if buf[0] == EPacketType::SHOOK as u8 {
                    break;
                }
            }
            println!("Received Handshake Packet");
            let _ = socket.set_read_timeout(Some(Duration::from_millis(5000))).expect("Failed to Set Timeout");
        }
    }
}