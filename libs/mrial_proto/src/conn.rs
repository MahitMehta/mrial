use std::net::UdpSocket;

use crate::{HEADER, write_header};

pub fn ping(socket: &UdpSocket) -> std::io::Result<()>{
    let mut buf = [0u8; HEADER];

    write_header(
        crate::EPacketType::PING, 
        0, 
        HEADER.try_into().unwrap(), 
        0, 
        &mut buf
    );

    socket.send(&buf)?;
    Ok(())
}