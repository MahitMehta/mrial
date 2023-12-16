use std::net::UdpSocket;

use super::AudioController;

impl AudioController {
    pub fn new() -> AudioController {
        AudioController {

        }
    }

    pub fn begin_transmission(&self, socket: UdpSocket, src: std::net::SocketAddr) {
        
    }
}