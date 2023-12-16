use std::{time::Duration, net::UdpSocket};

use mrial_proto::*; 

pub struct Client {
   pub socket: UdpSocket
}

const SERVER_ADDR: &'static str = "150.136.127.166:8554";
const CLIENT_ADDR: &'static str = "0.0.0.0:8080";

impl Client {
    pub fn new() -> Client {
        let socket = UdpSocket::bind(CLIENT_ADDR).expect("Failed to Bind to Incoming Socket");
        
        Client {
            socket
        }
    }

    pub fn try_clone(&self) -> Client {
        let socket = self.socket.try_clone().unwrap();
        Client {
            socket,
        }
    }

    pub fn send_handshake(&self) {
        let _ = &self.socket.set_read_timeout(Some(Duration::from_millis(1000))).expect("Failed to Set Timeout");
        let mut buf: [u8; HEADER] = [0; HEADER];
        
        write_header(
            EPacketType::SHAKE, 
            0, 
            HEADER as u32,
            &mut buf
        );

        loop {
            let _ = &self.socket.send_to(&buf, SERVER_ADDR);
            println!("Sent Handshake Packet");
            
            // validate src
            let (_amt, _src) = match &self.socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(_e) => continue,
            };
    
            if buf[0] == EPacketType::SHOOK as u8 {
                break;
            }
        }
        println!("Received Handshake Packet");
        let _ = &self.socket.set_read_timeout(Some(Duration::from_millis(5000))).expect("Failed to Set Timeout");
    }
}