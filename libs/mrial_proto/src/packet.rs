pub enum EPacketType {
    SHAKE = 0, 
    SHOOK = 1,
    NAL = 2,  
    STATE = 3,
    AUDIO = 4,
    InternalEOL = 13
}

impl From<u8> for EPacketType {
    fn from(v: u8) -> Self {
        match v {
            0 => EPacketType::SHAKE,
            1 => EPacketType::SHOOK,
            2 => EPacketType::NAL,
            3 => EPacketType::STATE,
            4 => EPacketType::AUDIO,
            13 => EPacketType::InternalEOL,
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
// Packet ID = 1 Byte

// Payload Schema
// variables sized unencrypted bytes (MAX = MTU - HEADER) 

#[inline]
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

#[inline]
pub fn parse_packets_remaining(buf: &[u8]) -> u16 {
    let packets_remaining_bytes: [u8; 2] = buf[1..3].try_into().unwrap();
    u16::from_be_bytes(packets_remaining_bytes)
}

#[inline]
pub fn parse_packet_id(buf: &[u8]) -> u8 {
    buf[7]
}

#[inline]
pub fn parse_header(buf: &[u8]) -> (EPacketType, u16, u32) {
    let packet_type = EPacketType::from(buf[0]);

    let packets_remaining_bytes: [u8; 2] = buf[1..3].try_into().unwrap();
    let packets_remaining = u16::from_be_bytes(packets_remaining_bytes);

    let real_packet_size_bytes: [u8; 4] = buf[3..7].try_into().unwrap();
    let real_packet_size = u32::from_be_bytes(real_packet_size_bytes);

    (packet_type, packets_remaining, real_packet_size)
}

#[inline]
pub fn assembled_packet(
    packet: &mut Vec<u8>, 
    buf: &[u8], 
    number_of_bytes: usize, 
    packets_remaining: u16
) -> bool {
    packet.extend_from_slice(&buf[HEADER..number_of_bytes]);
   
    packets_remaining == 0 // packet assembled
}