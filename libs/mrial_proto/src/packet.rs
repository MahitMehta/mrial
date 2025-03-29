use chacha20poly1305::{aead::Aead, AeadCore, ChaCha20Poly1305, Error};
use log::{debug, trace};
use rand::rngs::ThreadRng;
use std::collections::HashMap;

use crate::SE_NONCE;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum ENalVariant {
    KeyFrame = 0,
    NonKeyFrame = 1,
}

impl From<u8> for ENalVariant {
    fn from(v: u8) -> Self {
        match v {
            0 => ENalVariant::KeyFrame,
            1 => ENalVariant::NonKeyFrame,
            _ => ENalVariant::NonKeyFrame,
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum EPacketType {
    /// Header (Unecrypted)
    ShakeUE = 0,
    /// Header (Unecrypted) + JSON Containing Public Key (Unencrypted)
    ShookUE = 1,
    /// Header (Unecrypted) + JSON Containing Generated Symmetric Key
    /// Using Public Key + Credential Hash (Asymmetrically Encrypted)
    ShakeAE = 2,
    // Header (Unecrypted) + JSON Containing Server State (Symmetrically Encrypted)
    ShookSE = 3,
    /// Header (Unecrypted) + Byte stream (Encrypted)
    NAL = 4,
    /// Header (Unecrypted) + Byte stream (Encrypted)
    AudioPCM = 5,
    AudioOpus = 6,
    InputState = 7,
    ClientState = 8,
    ServerState = 9,
    /// Header (Unecrypted)
    Disconnect = 10,
    /// Header (Unecrypted)
    Ping = 11,
    Alive = 12,
    XOR = 13,
    // TODO: Add Server Pings in addition to Client Pings
    InternalEOL = 14,
    Unknown = 31,
}

impl From<u8> for EPacketType {
    fn from(v: u8) -> Self {
        match v {
            0 => EPacketType::ShakeUE,
            1 => EPacketType::ShookUE,
            2 => EPacketType::ShakeAE,
            3 => EPacketType::ShookSE,
            4 => EPacketType::NAL,
            5 => EPacketType::AudioPCM,
            6 => EPacketType::AudioOpus,
            7 => EPacketType::InputState,
            8 => EPacketType::ClientState,
            9 => EPacketType::ServerState,
            10 => EPacketType::Disconnect,
            11 => EPacketType::Ping,
            12 => EPacketType::Alive,
            13 => EPacketType::XOR,
            14 => EPacketType::InternalEOL,
            _ => EPacketType::Unknown,
        }
    }
}

pub const MTU: usize = 1200; // 1032;
pub const HEADER: usize = 8;
pub const PAYLOAD: usize = MTU - HEADER;

// Header Schema
// Packet Type Variant Details + Packet Type = 3 bits + 5 bits = 1 byte
// Packets Remaining = 2 byte
// Real Packet Byte Size = 4 bytes // TODO: reduce to 3 bytes
// Frame ID = 1 Byte

// Payload Schema
// variables sized unencrypted bytes (MAX = MTU - HEADER)

#[inline]
pub fn write_static_header(
    packet_type: EPacketType,
    real_packet_size: u32,
    frame_id: u8,
    buf: &mut [u8],
) {
    buf[0] |= packet_type as u8;
    buf[3..7].copy_from_slice(&real_packet_size.to_be_bytes());
    buf[7] = frame_id;
}

#[inline] 
pub fn write_packet_type_variant(packet_type_variant: u8, buf: &mut [u8]) {
    buf[0] |= (packet_type_variant << 5) & 0b11100000; // first 3 bits
}

#[inline]
pub fn write_dynamic_header(real_packet_size: u32, frame_id: u8, buf: &mut [u8]) {
    buf[3..7].copy_from_slice(&real_packet_size.to_be_bytes());
    buf[7] = frame_id;
}

#[inline]
pub fn write_packet_type(packet_type: EPacketType, buf: &mut [u8]) {
    buf[0] |= packet_type as u8;
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
    frame_id: u8,
    buf: &mut [u8],
) {
    write_static_header(packet_type, real_packet_size, frame_id, buf);
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
    EPacketType::from(buf[0] & 0x1F)
}

#[inline]
pub fn parse_packet_type_variant(buf: &[u8]) -> u8 {
    (buf[0] & 0b11100000) >> 5
}

#[inline]
pub fn parse_frame_id(buf: &[u8]) -> u8 {
    buf[7]
}

#[inline]
pub fn parse_header(buf: &[u8]) -> (EPacketType, u16, u32, u8) {
    let packet_type = parse_packet_type(buf);
    let packets_remaining = parse_packets_remaining(buf);
    let real_packet_size = parse_real_packet_size(buf);
    let frame_id = parse_frame_id(buf);

    (packet_type, packets_remaining, real_packet_size, frame_id)
}

#[inline]
pub fn encrypt_frame(sym_key: &ChaCha20Poly1305, frame: &[u8]) -> Result<Vec<u8>, Error> {
    let nonce = ChaCha20Poly1305::generate_nonce(ThreadRng::default());

    match sym_key.encrypt(&nonce, frame) {
        Ok(mut ciphertext) => {
            ciphertext.extend_from_slice(&nonce);
            return Ok(ciphertext);
        }
        Err(e) => {
            return Err(e);
        }
    }
}

#[inline]
pub fn decrypt_frame(sym_key: &ChaCha20Poly1305, encrypted_frame: &[u8]) -> Option<Vec<u8>> {
    if encrypted_frame.len() < SE_NONCE {
        debug!("\x1b[93mCorrupted Frame\x1b[0m");
        return None;
    }

    let encrypted_payload = &encrypted_frame[0..encrypted_frame.len() - SE_NONCE];
    let nonce = &encrypted_frame[encrypted_frame.len() - 12..encrypted_frame.len()];
    let nonce = nonce.try_into().map_err(|_| "Corrupted SE Nonce").unwrap();

    if let Ok(decrypted_payload) = sym_key.decrypt(nonce, encrypted_payload) {
        return Some(decrypted_payload);
    }

    None
}

pub fn subpacket_count(len: u32) -> u16 {
    (len as f64 / PAYLOAD as f64).ceil() as u16
}

pub fn calculate_frame_size(packet: &Vec<Vec<u8>>) -> usize {
    (packet.len() - 1) * PAYLOAD + packet.last().unwrap().len() - HEADER
}

// TODO: 2 packet loss modes (high redundancy and low redundancy)
// based on packet loss rate, high redundancy will retransmit packets when they are out of order
// even if they are not lost just in case they are lost too

// create a packet id using (frame id + subpacket number + real packet size)

pub struct PacketConstructor {
    packets: Vec<Vec<u8>>,
    previous_subpacket_number: i16,
    order_mismatch: bool,
    xor_packets: HashMap<u8, Vec<Vec<u8>>>,
    cached_packets: HashMap<u8, Vec<Vec<u8>>>,
    previous_frame_id: i16,

    #[cfg(feature = "stat")]
    recieved_packets: u8,
    #[cfg(feature = "stat")]
    potential_packets: u8,
    #[cfg(feature = "stat")]
    latest_frame_id: i16,
    #[cfg(feature = "stat")]
    received_subpackets: f64,
    #[cfg(feature = "stat")]
    potential_subpackets: f64,
    #[cfg(feature = "stat")]
    recovered_frames: u16,
}

impl PacketConstructor {
    pub fn new() -> Self {
        Self {
            packets: Vec::new(),
            previous_subpacket_number: -1,
            order_mismatch: false,
            cached_packets: HashMap::new(),
            previous_frame_id: -1,
            xor_packets: HashMap::new(),

            #[cfg(feature = "stat")]
            recieved_packets: 0,
            #[cfg(feature = "stat")]
            potential_packets: 0,
            #[cfg(feature = "stat")]
            latest_frame_id: -1,
            #[cfg(feature = "stat")]
            received_subpackets: 0.0,
            #[cfg(feature = "stat")]
            potential_subpackets: 0.0,
            #[cfg(feature = "stat")]
            recovered_frames: 0,
        }
    }

    // TODO: Actually make this method functional.
    // Cache previously dropped/out of order messages and reassemble them
    #[inline]
    fn reconstruct_when_deficient(&mut self) -> bool {
        let last_frame_id = parse_frame_id(self.packets.last().unwrap());
        if false {
            // let Some(_cached_packets) = self.cached_packets.get(&last_frame_id) {
            debug!("TODO: Append Found Cached Packets");
        } else {
            if let Some(xor_packets) = self.xor_packets.get(&last_frame_id) {
                if xor_packets.len() > 0 {
                    debug!("Attempting Recovery from XOR");

                    // packets could have multiple frames
                    let subpackets =
                        subpacket_count(parse_real_packet_size(self.packets.last().unwrap()));
                    let parity_packet_count = (subpackets as f32 / 3.0).ceil() as u16;

                    for i in 0..self.packets.len() - 1 {
                        let curr_packet = &self.packets[i];
                        let next_packet = &self.packets[i + 1];

                        let curr_remaining_packets = parse_packets_remaining(curr_packet);
                        let next_remaining_packets = parse_packets_remaining(next_packet);

                        if curr_remaining_packets - next_remaining_packets != 1 {
                            trace!("{} {}", curr_remaining_packets, next_remaining_packets);
                            let missing_subpacket_id = curr_remaining_packets - 1;
                            let xor_packet = xor_packets.iter().find(|packet| {
                                let missing_packet_index = subpackets - missing_subpacket_id;
                                let xor_remaining_packets =
                                    subpackets - (missing_packet_index % parity_packet_count);
                                parse_packets_remaining(&packet) == xor_remaining_packets
                            });
                            if let Some(xor_packet) = xor_packet {
                                trace!("Found XOR Packet: {}", parse_packets_remaining(xor_packet));

                                let complement_packets: Vec<&Vec<u8>> = self
                                    .packets
                                    .iter()
                                    .filter(|packet| {
                                        let subpacket_index =
                                            subpackets - parse_packets_remaining(packet);
                                        let xor_remaining_packets =
                                            subpackets - (subpacket_index % parity_packet_count);
                                        xor_remaining_packets == parse_packets_remaining(xor_packet)
                                    })
                                    .collect();

                                if complement_packets.len() == 2 {
                                    trace!("Found Complement Packets");

                                    let mut recovered_packet = xor_packet.clone();
                                    if recovered_packet.len() != complement_packets[0].len()
                                        || recovered_packet.len() != complement_packets[1].len()
                                    {
                                        trace!("Packet Size Mismatch");
                                        continue;
                                    }
                                    for i in 0..recovered_packet.len() {
                                        recovered_packet[i] ^= complement_packets[0][i];
                                        recovered_packet[i] ^= complement_packets[1][i];
                                    }

                                    write_packets_remaining(
                                        missing_subpacket_id,
                                        &mut recovered_packet,
                                    );
                                    self.packets.insert(i + 1, recovered_packet);
                                }
                            }
                        }
                    }

                    if self.packets.len() as u16 == subpackets {
                        trace!("Recovered Frame");

                        #[cfg(feature = "stat")]
                        self.increment_recovered_frames();

                        return true;
                    }
                }
            }

            debug!("Cached Packet Units for Potential Future Reconstruction");
            // TODO: implement a way of clearing all packets that have an id in incoming cached packets

            for packet in &self.packets {
                PacketConstructor::cache_packet(
                    &mut self.cached_packets,
                    packet,
                    parse_frame_id(packet),
                );
            }
        }

        self.order_mismatch = false;
        self.packets.clear();
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
        current_frame_id: u8,
    ) {
        if !cached_packets.contains_key(&current_frame_id) {
            // ### debug ###
            {
                trace!(
                    "No Cache for Previous Packet ID (Frame: {current_frame_id}) so dropped Packet Unit: {:?}", 
                    parse_packets_remaining(packet_unit)
                );
            }
            return;
        }

        let real_packet_size = parse_real_packet_size(packet_unit);
        let cache_packet_size =
            PacketConstructor::get_cached_packet_size(&cached_packets[&current_frame_id]);
        let potential_packet_size = cache_packet_size + (packet_unit.len() - HEADER);

        if potential_packet_size == real_packet_size as usize {
            debug!("Will Reconstruct Packet");
        } else {
            trace!(
                "Caching, Packet Built: {}%",
                (cache_packet_size as f64 / real_packet_size as f64) * 100.0
            );
        }
    }

    #[inline]
    fn cache_packet(
        cached_packets: &mut HashMap<u8, Vec<Vec<u8>>>,
        packet_unit: &Vec<u8>,
        current_frame_id: u8,
    ) {
        if cached_packets.contains_key(&current_frame_id) {
            cached_packets
                .get_mut(&current_frame_id)
                .unwrap()
                .push(packet_unit.clone());
        } else {
            cached_packets.insert(current_frame_id, vec![packet_unit.clone()]);
        }
    }

    // TODO: Find Method to Clear Cached Packets
    #[inline]
    fn filter_packet(&mut self) {
        debug!("Filtering Packets");

        let last_frame_id = parse_frame_id(self.packets.last().unwrap());
        self.packets.retain(|packet_unit| {
            let current_frame_id = parse_frame_id(&packet_unit);
            if current_frame_id != last_frame_id {
                if current_frame_id < last_frame_id {
                    PacketConstructor::reconstruct_when_surplus(
                        &mut self.cached_packets,
                        packet_unit,
                        current_frame_id,
                    );
                } else {
                    debug!(
                        "Caching Packet Unit {:?} with Packet ID {}",
                        parse_packets_remaining(packet_unit),
                        current_frame_id
                    );

                    PacketConstructor::cache_packet(
                        &mut self.cached_packets,
                        packet_unit,
                        current_frame_id,
                    );
                }
                return false;
            }

            true
        });
    }

    #[cfg(feature = "stat")]
    fn print_packet_order(&mut self) {
        debug!("Packet Type: {:?}", EPacketType::from(self.packets[0][0]));
        let mut packet_order = String::new();
        let mut xor_packet_order = String::new();
        for packet in &self.packets {
            if self.latest_frame_id != parse_frame_id(packet) as i16 {
                self.potential_subpackets += subpacket_count(parse_real_packet_size(packet)) as f64;
                self.latest_frame_id = parse_frame_id(packet) as i16;
            }
            packet_order.push_str(&format!(
                "{}-{}, ",
                parse_frame_id(&packet),
                parse_packets_remaining(&packet)
            ));
        }
        if let Some(xor_packet) = self
            .xor_packets
            .get(&parse_frame_id(self.packets.last().unwrap()))
        {
            for packet in xor_packet {
                xor_packet_order.push_str(&format!(
                    "{}-{}, ",
                    parse_frame_id(&packet),
                    parse_packets_remaining(&packet)
                ));
            }
        }

        debug!(
            "Subpackets: {} (Packet ID:{})",
            subpacket_count(parse_real_packet_size(self.packets.last().unwrap())),
            parse_frame_id(self.packets.last().unwrap())
        );
        debug!("Packet Order: {}", packet_order);
        debug!("XOR Packet Order: {}", xor_packet_order);
    }

    #[inline]
    fn handle_order_mismatch(&mut self, real_packet_size: usize) -> bool {
        // TODO: Is this neccessary?
        self.packets.sort_by(|a, b| {
            let a_id = parse_frame_id(&a);
            let b_id = parse_frame_id(&b);

            if a_id != b_id {
                let forward_diff = b_id.wrapping_sub(a_id);
                let backward_diff = a_id.wrapping_sub(b_id);
                return forward_diff.cmp(&backward_diff);
            }

            let a_size = parse_packets_remaining(&a);
            let b_size = parse_packets_remaining(&b);
            b_size.cmp(&a_size)
        });

        #[cfg(feature = "stat")]
        self.print_packet_order();

        let frame_size = calculate_frame_size(&self.packets);
        if real_packet_size > frame_size {
            return self.reconstruct_when_deficient();
        } else if real_packet_size < frame_size {
            self.filter_packet();

            if cfg!(feature = "stat") {
                let updated_frame_size = calculate_frame_size(&self.packets);
                debug!(
                    "Packet Size After Fix: {} vs {}",
                    updated_frame_size, real_packet_size
                );
            }
        }

        self.order_mismatch = false;

        true
    }

    #[cfg(feature = "stat")]
    pub fn calculate_yield(&mut self, current_frame_id: u8) {
        use log::info;

        if self.latest_frame_id != parse_frame_id(&self.packets[0]) as i16 {
            self.potential_subpackets +=
                subpacket_count(parse_real_packet_size(&self.packets[0])) as f64;
            self.latest_frame_id = parse_frame_id(&self.packets[0]) as i16;
        }

        self.recieved_packets += 1;
        if self.previous_frame_id != -1 {
            self.potential_packets += current_frame_id.wrapping_sub(self.previous_frame_id as u8);
        } else {
            self.potential_packets += 1;
        }

        if self.potential_packets >= 100 {
            info!(
                "Packet Yield: {}% ({}/{})",
                self.received_subpackets / self.potential_subpackets * 100.0,
                self.received_subpackets,
                self.potential_subpackets
            );
            info!(
                "Frame Yield: {}% ({} Recovered Frames)",
                self.recieved_packets, self.recovered_frames
            );

            self.recieved_packets = 0;
            self.potential_packets = 0;
            self.recovered_frames = 0;

            self.received_subpackets = 0.0;
            self.potential_subpackets = 0.0;
        }
    }

    #[cfg(feature = "stat")]
    pub fn increment_recovered_frames(&mut self) {
        self.recovered_frames += 1;
    }

    #[cfg(feature = "stat")]
    pub fn increment_subpacket_count(&mut self) {
        self.received_subpackets += 1.0;
    }

    #[inline]
    pub fn assemble_packet(&mut self, buf: &[u8], number_of_bytes: usize) -> Option<Vec<u8>> {
        let packet_type = parse_packet_type(buf);
        if packet_type == EPacketType::XOR {
            if !self.xor_packets.contains_key(&parse_frame_id(buf)) {
                self.xor_packets
                    .insert(parse_frame_id(buf), vec![buf[..number_of_bytes].to_vec()]);
            } else if let Some(xor_packets) = self.xor_packets.get_mut(&parse_frame_id(buf)) {
                xor_packets.push(buf[..number_of_bytes].to_vec());
            }

            return None;
        }

        let packets_remaining = parse_packets_remaining(buf);
        let real_packet_size = parse_real_packet_size(buf);
        let frame_id = parse_frame_id(buf);

        if self.previous_subpacket_number != (packets_remaining + 1) as i16
            && self.previous_subpacket_number > 0
        {
            trace!(
                "Packet Order Mixup: {} -> {}",
                self.previous_subpacket_number,
                packets_remaining
            );

            self.order_mismatch = true;
        }
        self.previous_subpacket_number = packets_remaining as i16;

        self.packets.push(buf[..number_of_bytes].to_vec());
        #[cfg(feature = "stat")]
        self.increment_subpacket_count();

        if packets_remaining != 0 {
            return None;
        }

        if self.order_mismatch && !self.handle_order_mismatch(real_packet_size.try_into().unwrap())
        {
            return None;
        }

        let mut assembled_packet = Vec::new();
        for packet in &self.packets {
            assembled_packet.extend_from_slice(&packet[HEADER..]);
        }

        if packet_type == EPacketType::NAL {
            debug!("Assembled Packet Variant: {:?}",
                ENalVariant::from(parse_packet_type_variant(&self.packets[0]))
            );
        }


        #[cfg(feature = "stat")]
        self.calculate_yield(frame_id);

        let packets_diff = frame_id.wrapping_sub(self.previous_frame_id as u8);
        for i in 1..=packets_diff {
            let calculated_frame_id = if self.previous_frame_id == -1 {
                frame_id
            } else {
                (self.previous_frame_id as u8) + i
            };
            if let Some(xor_packets) = self.xor_packets.get_mut(&calculated_frame_id) {
                xor_packets.clear();
            }
        }
        self.previous_frame_id = frame_id as i16;
        self.packets.clear();
        Some(assembled_packet)
    }
}
