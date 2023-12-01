pub enum EPacketType {
    SHAKE = 0, 
    SHOOK = 1,
    NAL = 2,  
    STATE = 3,
    AUDIO = 4
}

pub const MTU: usize = 1032; 
pub const HEADER: usize = 8; 

pub struct Packet {
    packet_type: u8,
    packets_remaining: u8,
    real_packet_size: u32
}

// Header Schema
// Packet Type = 1 byte
// Packets Remaining = 1 byte
// Real Packet Byte Size = 4 bytes
// 2 Bytes are currently unoccupied in the header
// Payload Schema
// variables sized unencrypted bytes (MAX = MTU - HEADER) 

impl Packet {
    pub fn new(packet_type: EPacketType, packets_remaining: u8, real_packet_size: u32) -> Packet {
        Packet {
            packet_type: packet_type as u8,
            packets_remaining,
            real_packet_size
        }
    }

    pub fn write_header(&self, buf: &mut [u8]) {
        buf[0] = self.packet_type;
        buf[1] = self.packets_remaining;
        buf[2..6].copy_from_slice(&self.real_packet_size.to_be_bytes());
    }
}