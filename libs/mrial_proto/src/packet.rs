use std::{
    collections::HashMap,
    error::Error,
    net::{self, UdpSocket},
};

use chacha20poly1305::{aead::Aead, ChaCha20Poly1305};

use crate::SE_NONCE;

// use chacha20poly1305::{aead::{AeadMut, AeadMutInPlace}, AeadCore, ChaCha20Poly1305};
// use rand::rngs::ThreadRng;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum EPacketType {
    /// Header (Unecrypted)
    ShakeUE = 0,
    /// Header (Unecrypted) + JSON Containing Public Key (Unencrypted)
    ShookUE = 1,
    /// Header (Unecrypted) + Byte stream (Encrypted)
    NAL = 2,
    InputState = 3,
    /// Header (Unecrypted) + Byte stream (Encrypted)
    Audio = 4,
    /// Header (Unecrypted)
    Disconnect = 5,
    /// Header (Unecrypted)
    PING = 6,
    ClientState = 7,
    ServerState = 8,
    /// Header (Unecrypted) + JSON Containing Generated Symmetric Key
    /// Using Public Key + Credential Hash (Asymmetrically Encrypted)
    ShakeAE = 9,
    // Header (Unecrypted) + JSON Containing Server State (Symmetrically Encrypted)
    ShookSE = 10,
    Alive = 11,
    // TODO: Add Server Pings in addition to Client Pings
    InternalEOL = 13,
}

impl From<u8> for EPacketType {
    fn from(v: u8) -> Self {
        match v {
            0 => EPacketType::ShakeUE,
            1 => EPacketType::ShookUE,
            2 => EPacketType::NAL,
            3 => EPacketType::InputState,
            4 => EPacketType::Audio,
            5 => EPacketType::Disconnect,
            6 => EPacketType::PING,
            7 => EPacketType::ClientState,
            8 => EPacketType::ServerState,
            9 => EPacketType::ShakeAE,
            10 => EPacketType::ShookSE,
            11 => EPacketType::Alive,
            13 => EPacketType::InternalEOL,
            _ => panic!("Invalid Packet Type"),
        }
    }
}

pub const MTU: usize = 1032;
pub const HEADER: usize = 8;
pub const PAYLOAD: usize = MTU - HEADER;

// Header Schema
// Packet Type = 1 byte
// Packets Remaining = 2 byte
// Real Packet Byte Size = 4 bytes // TODO: reduce to 3 bytes
// Packet ID = 1 Byte

// Payload Schema
// variables sized unencrypted bytes (MAX = MTU - HEADER)

#[inline]
pub fn write_static_header(
    packet_type: EPacketType,
    real_packet_size: u32,
    packet_id: u8,
    buf: &mut [u8],
) {
    buf[0] = packet_type as u8;
    buf[3..7].copy_from_slice(&real_packet_size.to_be_bytes());
    buf[7] = packet_id;
}

#[inline]
pub fn write_var_frame_header(real_packet_size: u32, packet_id: u8, buf: &mut [u8]) {
    buf[3..7].copy_from_slice(&real_packet_size.to_be_bytes());
    buf[7] = packet_id;
}

#[inline]
pub fn write_packet_type(packet_type: EPacketType, buf: &mut [u8]) {
    buf[0] = packet_type as u8;
}

#[inline]
pub fn write_packets_remaining(packets_remaining: u16, buf: &mut [u8]) {
    buf[1..3].copy_from_slice(&packets_remaining.to_be_bytes());
}

#[inline]
pub fn write_header(
    packet_type: EPacketType,
    packets_remaining: u16,
    real_packet_size: u32,
    packet_id: u8,
    buf: &mut [u8],
) {
    write_static_header(packet_type, real_packet_size, packet_id, buf);
    write_packets_remaining(packets_remaining, buf);
}

#[inline]
pub fn parse_packets_remaining(buf: &[u8]) -> u16 {
    let packets_remaining_bytes: [u8; 2] = buf[1..3].try_into().unwrap();
    u16::from_be_bytes(packets_remaining_bytes)
}

#[inline]
pub fn parse_real_packet_size(buf: &[u8]) -> u32 {
    let real_packet_size_bytes: [u8; 4] = buf[3..7].try_into().unwrap();
    u32::from_be_bytes(real_packet_size_bytes)
}

#[inline]
pub fn parse_packet_type(buf: &[u8]) -> EPacketType {
    EPacketType::from(buf[0])
}

#[inline]
pub fn parse_packet_id(buf: &[u8]) -> u8 {
    buf[7]
}

#[inline]
pub fn parse_header(buf: &[u8]) -> (EPacketType, u16, u32, u8) {
    let packet_type = parse_packet_type(buf);
    let packets_remaining = parse_packets_remaining(buf);
    let real_packet_size = parse_real_packet_size(buf);
    let packet_id = parse_packet_id(buf);

    (packet_type, packets_remaining, real_packet_size, packet_id)
}

// pub struct PacketDeployer {
//     buf: [u8; MTU],
//     socket: UdpSocket,
//     packet_type: EPacketType
// }

// impl PacketDeployer {
//     pub fn new(socket: UdpSocket, packet_type: EPacketType) -> Self {
//         let mut buf = [0u8; MTU];
//         write_packet_type(packet_type, &mut buf);

//         Self {
//             buf,
//             socket,
//             packet_type
//         }
//     }

//     #[inline]
//     pub fn encrypted_frame(
//         &mut self,
//         frame: &mut [u8],
//         rng: ThreadRng,
//         mut sym_key: ChaCha20Poly1305
//     ) -> (Vec<u8>, Vec<u8>){
//         let nonce = ChaCha20Poly1305::generate_nonce(rng);
//         let auth_tag = sym_key.encrypt_in_place_detached(
//             &nonce, &[0u8; 0], frame).unwrap();

//         (auth_tag.to_vec(), nonce.to_vec())
//     }

//     #[inline]
//     pub fn frame_chunks(
//         &mut self,
//         frame: &mut [u8],
//         packets_remaining: u16,
//         packet_id: u8,
//         rng: ThreadRng,
//         sym_key: ChaCha20Poly1305
//     ) {
//         write_variable_header(packets_remaining, packet_id, &mut self.buf);
//         let (auth_tag, nounce) = self.encrypted_frame(
//             frame, rng, sym_key);

//         let packets = (frame.len() as f64 / PAYLOAD as f64).ceil() as usize;
//     }

//     pub fn try_clone(&self) -> Result<Self, Box<(dyn serde::ser::StdError + 'static)>> {
//         Ok(Self {
//             buf: self.buf,
//             socket: self.socket.try_clone()?,
//             packet_type: self.packet_type
//         })
//     }
// }

pub fn decrypt_frame(sym_key: &ChaCha20Poly1305, encrypted_frame: &[u8]) -> Option<Vec<u8>> {
    let encrypted_payload = &encrypted_frame[0..encrypted_frame.len() - SE_NONCE];
    let nonce = &encrypted_frame[encrypted_frame.len() - 12..encrypted_frame.len()];
    let nonce = nonce.try_into().map_err(|_| "Corrupted SE Nonce").unwrap();

    if let Ok(decrypted_payload) = sym_key.decrypt(nonce, encrypted_payload) {
        return Some(decrypted_payload);
    }

    None
}

pub struct PacketConstructor {
    packet: Vec<Vec<u8>>,
    previous_subpacket_number: i16,
    order_mismatch: bool,
    cached_packets: HashMap<u8, Vec<Vec<u8>>>,
}

impl PacketConstructor {
    pub fn new() -> Self {
        Self {
            packet: Vec::new(),
            previous_subpacket_number: -1,
            order_mismatch: false,
            cached_packets: HashMap::new(),
        }
    }

    // TODO: Actually make this method functional.
    // Cache previously dropped/out of order messages and reassemble them
    #[inline]
    fn reconstruct_when_deficient(&mut self) -> bool {
        let last_packet_id = parse_packet_id(self.packet.last().unwrap());
        if let Some(_cached_packets) = self.cached_packets.get(&last_packet_id) {
            println!("TODO: Append Found Cached Packets");
        } else {
            println!("Cached Packet Units for Potential Future Reconstruction");
            // TODO: implement a way of clearing all packets that have an id in incoming cached packets

            for packet in &self.packet {
                PacketConstructor::cache_packet(
                    &mut self.cached_packets,
                    packet,
                    parse_packet_id(packet),
                );
            }
        }

        self.order_mismatch = false;
        self.packet.clear();
        return false;
    }

    #[inline]
    fn get_cached_packet_size(cached_packets_id: &Vec<Vec<u8>>) -> usize {
        let mut cached_packet_size = 0;
        for packet in cached_packets_id {
            cached_packet_size += packet.len() - HEADER;
        }
        cached_packet_size
    }

    #[inline]
    fn reconstruct_when_surplus(
        cached_packets: &mut HashMap<u8, Vec<Vec<u8>>>,
        packet_unit: &Vec<u8>,
        current_packet_id: u8,
    ) {
        if !cached_packets.contains_key(&current_packet_id) {
            // ### DEBUG ###
            {
                println!(
                    "No Cache for Previous Packet ID (Frame: {current_packet_id}) so dropped Packet Unit: {:?}", 
                    parse_packets_remaining(packet_unit)
                );
            }
            return;
        }

        let real_packet_size = parse_real_packet_size(packet_unit);
        let cache_packet_size =
            PacketConstructor::get_cached_packet_size(&cached_packets[&current_packet_id]);
        let potential_packet_size = cache_packet_size + (packet_unit.len() - HEADER);

        if potential_packet_size == real_packet_size as usize {
            println!("Will Reconstruct Packet");
        } else {
            println!("Caching for Future Reconstruction");
        }
    }

    #[inline]
    fn cache_packet(
        cached_packets: &mut HashMap<u8, Vec<Vec<u8>>>,
        packet_unit: &Vec<u8>,
        current_packet_id: u8,
    ) {
        if cached_packets.contains_key(&current_packet_id) {
            cached_packets
                .get_mut(&current_packet_id)
                .unwrap()
                .push(packet_unit.clone());
        } else {
            cached_packets.insert(current_packet_id, vec![packet_unit.clone()]);
        }
    }

    // TODO: Find Method to Clear Cached Packets
    #[inline]
    fn filter_packet(&mut self) {
        // ### DEBUG ###
        {
            println!("Filtering Packets");
        }

        let last_packet_id = parse_packet_id(self.packet.last().unwrap());

        self.packet.retain(|packet_unit| {
            let current_packet_id = parse_packet_id(&packet_unit);
            if current_packet_id != last_packet_id {
                if current_packet_id < last_packet_id {
                    PacketConstructor::reconstruct_when_surplus(
                        &mut self.cached_packets,
                        packet_unit,
                        current_packet_id,
                    );
                } else {
                    // ### DEBUG ###
                    {
                        println!(
                            "Caching Packet Unit {:?} with Packet ID {}",
                            parse_packets_remaining(packet_unit),
                            current_packet_id
                        );
                    }

                    PacketConstructor::cache_packet(
                        &mut self.cached_packets,
                        packet_unit,
                        current_packet_id,
                    );
                }
                return false;
            }

            true
        });
    }

    #[inline]
    fn handle_order_mismatch(&mut self, real_packet_size: usize) -> bool {
        // ### DEBUG ###
        {
            println!("Packet Type: {:?}", EPacketType::from(self.packet[0][0]));
            for i in &self.packet {
                print!("{:?}, ", parse_packet_id(i));
            }
            println!();
        }

        let packet_size =
            (self.packet.len() - 1) * PAYLOAD + self.packet.last().unwrap().len() - HEADER;
        if real_packet_size > packet_size {
            return self.reconstruct_when_deficient();
        } else if real_packet_size < packet_size {
            self.filter_packet();

            // ### DEBUG ###
            {
                let nal_size =
                    (self.packet.len() - 1) * PAYLOAD + self.packet.last().unwrap().len() - HEADER;
                println!(
                    "Packet Size After Fix: {} vs {}",
                    nal_size, real_packet_size
                );
            }
        }

        self.packet.sort_by(|a, b| {
            let a_size = parse_packets_remaining(&a);
            let b_size = parse_packets_remaining(&b);
            b_size.cmp(&a_size)
        });

        self.order_mismatch = false;

        true
    }

    #[inline]
    pub fn assemble_packet(&mut self, buf: &[u8], number_of_bytes: usize) -> Option<Vec<u8>> {
        let packets_remaining = parse_packets_remaining(buf);
        let real_packet_size = parse_real_packet_size(buf);

        if self.previous_subpacket_number != (packets_remaining + 1) as i16
            && self.previous_subpacket_number > 0
        {
            // ### DEBUG ###
            {
                println!(
                    "Packet Order Mixup: {} -> {}",
                    self.previous_subpacket_number, packets_remaining
                );
            }

            self.order_mismatch = true;
        }
        self.previous_subpacket_number = packets_remaining as i16;

        self.packet.push(buf[..number_of_bytes].to_vec());
        if packets_remaining != 0 {
            return None;
        }

        if self.order_mismatch && !self.handle_order_mismatch(real_packet_size.try_into().unwrap())
        {
            return None;
        }

        let mut assembled_packet = Vec::new();
        for packet in &self.packet {
            assembled_packet.extend_from_slice(&packet[HEADER..]);
        }
        self.packet.clear();

        Some(assembled_packet)
    }
}
