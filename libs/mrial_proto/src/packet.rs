pub enum EPacketType {
    SHAKE = 0, 
    SHOOK = 1,
    NAL = 2,  
    STATE = 3,
    AUDIO = 4
}

impl From<u8> for EPacketType {
    fn from(v: u8) -> Self {
        match v {
            0 => EPacketType::SHAKE,
            1 => EPacketType::SHOOK,
            2 => EPacketType::NAL,
            3 => EPacketType::STATE,
            4 => EPacketType::AUDIO,
            _ => panic!("Invalid Packet Type")
        }
    }
}

pub const MTU: usize = 1032; 
pub const HEADER: usize = 8; 
pub const PAYLOAD: usize = MTU - HEADER;

// Header Schema
// Packet Type = 1 byte
// Packets Remaining = 2 byte
// Real Packet Byte Size = 4 bytes // replace with packet number (2 bytes?)
// 1 Byte is currently unoccupied in the header

// Payload Schema
// variables sized unencrypted bytes (MAX = MTU - HEADER) 


pub fn write_header(
    packet_type: EPacketType, 
    packets_remaining: u16, 
    real_packet_size: u32, 
    buf: &mut [u8]
) {
    buf[0] = packet_type as u8;
    buf[1..3].copy_from_slice(&packets_remaining.to_be_bytes());
    buf[3..7].copy_from_slice(&real_packet_size.to_be_bytes());
}

pub fn parse_header(buf: &[u8]) -> (EPacketType, u16, u32) {
    let packet_type = EPacketType::from(buf[0]);

    let packets_remaining_bytes: [u8; 2] = buf[1..3].try_into().unwrap();
    let packets_remaining = u16::from_be_bytes(packets_remaining_bytes);

    let real_packet_size_bytes: [u8; 4] = buf[3..7].try_into().unwrap();
    let real_packet_size = u32::from_be_bytes(real_packet_size_bytes);

    (packet_type, packets_remaining, real_packet_size)
}

pub fn assemble_packet(packet: &mut Vec<u8>, packets_remaining: u16, number_of_bytes: usize, buf: &[u8]) -> bool {
    packet.extend_from_slice(&buf[HEADER..number_of_bytes]);
   
    packets_remaining == 0 // packet assembled
}