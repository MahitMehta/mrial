use chacha20poly1305::{aead::Aead, AeadCore, ChaCha20Poly1305, Error};
use log::{debug, trace, warn};
use rand::rngs::ThreadRng;
use std::{collections::{HashMap, HashSet, VecDeque}, mem, time::{SystemTime, UNIX_EPOCH}};

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
    // Header (Unecrypted) + JSON Containing Server state (Symmetrically Encrypted)
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
    Retransmit = 14,
    RNAL = 15, // Retransmitted NAL
    // TODO: Add Server Pings in addition to Client Pings
    InternalEOL = 30,
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
            14 => EPacketType::Retransmit,
            15 => EPacketType::RNAL,
            30 => EPacketType::InternalEOL,
            _ => EPacketType::Unknown,
        }
    }
}

pub const MTU: usize = 1200;

/// ## Header Schema
/// 1. Packet Type Variant Details + Packet Type = 3 bits + 5 bits = 1 byte
/// 2. Packets Remaining = 2 byte
// TODO: reduce to 3 bytes
/// 3. Real Packet Byte Size = 4 bytes, size of the entire frame (excludes size of headers)
/// 4. Frame ID = 1 Byte
pub const HEADER: usize = 8;

pub const PAYLOAD: usize = MTU - HEADER;


/// Note: Resets the packet variant bits to 0
#[inline]
pub fn write_static_header(
    packet_type: EPacketType,
    real_packet_size: u32,
    frame_id: u8,
    buf: &mut [u8],
) {
    buf[0] = packet_type as u8;
    buf[3..7].copy_from_slice(&real_packet_size.to_be_bytes());
    buf[7] = frame_id;
}

#[inline] 
pub fn write_packet_type_variant(packet_type_variant: u8, buf: &mut [u8]) {
    buf[0] &= 0b00011111; // reset the first 3 bits
    buf[0] |= (packet_type_variant << 5) & 0b11100000; // first 3 bits
}

#[inline]
pub fn write_dynamic_header(real_packet_size: u32, frame_id: u8, buf: &mut [u8]) {
    buf[3..7].copy_from_slice(&real_packet_size.to_be_bytes());
    buf[7] = frame_id;
}

/// Note: Resets the packet variant bits to 0
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
    frame_id: u8,
    buf: &mut [u8],
) {
    write_static_header(packet_type, real_packet_size, frame_id, buf);
    write_packets_remaining(packets_remaining, buf);
}

#[inline]
/// Writes the retransmit body.
/// The first 4 bytes are reserved for the real packet size.
/// The next 2 bytes are reserved for the frame ID + padding.
/// The rest of the bytes are reserved for the subpacket IDs.
/// ## Arguments
/// * `frame_id` - The frame ID.
/// * `real_packet_size` - The real packet size.
/// * `subpacket_ids` - The subpacket IDs.
/// * `buf` - The buffer to write to.
/// ## Returns
/// The number of bytes written.
pub fn write_retransmit_body(
    frame_id: u8,
    real_packet_size: u32,
    subpacket_ids: Vec<u16>,
    buf: &mut [u8],
) -> usize {
    // 2 = 1 byte for frame ID + 1 byte for padding (alignment of following u16s)
    let header_len = mem::size_of::<u32>() + 2;
    let body_len = subpacket_ids.len() * mem::size_of::<u16>() + header_len;

    assert!(buf.len() >= body_len);

    buf[0..mem::size_of::<u32>()].copy_from_slice(&real_packet_size.to_be_bytes());
    buf[mem::size_of::<u32>()] = frame_id;

    let mut offset = header_len;

    for packet in subpacket_ids {
        buf[offset..offset + mem::size_of::<u16>()].copy_from_slice(&packet.to_be_bytes());
        offset += mem::size_of::<u16>();
    }

    body_len
}

#[inline]
/// Parses the retransmit body.
/// ## Arguments
/// * `buf` - The buffer to parse.
/// ## Returns
/// * `frame_id` - The frame ID.
/// * `real_packet_size` - The real packet size.
/// * `subpacket_ids` - The subpacket IDs.
pub fn parse_retransmit_body(buf: &[u8]) -> (u8, u32, Vec<u16>) {
    let real_packet_size_bytes: [u8; 4] = buf[0..4].try_into().unwrap();
    let real_packet_size = u32::from_be_bytes(real_packet_size_bytes);

    let frame_id = buf[4];
    let header_len = mem::size_of::<u32>() + 2;

    let subpacket_count = (buf.len() - header_len) / mem::size_of::<u16>();
    let mut subpacket_ids = Vec::with_capacity(subpacket_count);

    for i in 0..subpacket_count {
        let start = header_len + i * mem::size_of::<u16>();
        let end = start + mem::size_of::<u16>();
        let subpacket_id_bytes: [u8; 2] = buf[start..end].try_into().unwrap();
        subpacket_ids.push(u16::from_be_bytes(subpacket_id_bytes));
    }

    (frame_id, real_packet_size, subpacket_ids)
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

const MAX_NAL_FRAME_SIZE: usize = 1024 * 512;
const MAX_FRAME_SIZE: usize = 1024 * 32;

#[derive(PartialEq)]
pub enum EAssemblerState {
    Assembling,
    Waiting,
    Finished,
    Queue(VecDeque<Vec<u8>>),
}

/// Unreliable, but fast packet constructor.
/// Drops any frames that dont't arrive completely or are missing.
/// Additionally, drops frames that are out of order.
pub struct PacketConstructor {
    frame: Vec<u8>,
    current_frame_id: i16,
    total_packets: usize,
    remaining_subpacket_ids: HashSet<usize>,
}

impl PacketConstructor {
    pub fn new() -> Self {
        let frame = Vec::with_capacity(MAX_FRAME_SIZE);

        Self {
            frame,
            current_frame_id: -1,
            total_packets: 0,
            remaining_subpacket_ids: HashSet::new(),
        }
    }

    #[inline]
    fn reset_for_next_frame(&mut self) {
        self.current_frame_id = -1;
        self.total_packets = 0;
        self.remaining_subpacket_ids.clear();
    }

    #[inline]
    pub fn assemble_packet<F>(
        &mut self, 
        fragment: &[u8], number_of_bytes: usize,
        assembled: &F) -> EAssemblerState
        where 
            F: Fn(Vec<u8>) -> ()  
    {
        let frame_id = parse_frame_id(fragment) as i16;
        let remaing_packets = parse_packets_remaining(fragment);
        let real_packet_size = parse_real_packet_size(fragment);

        if self.current_frame_id == -1 {
            self.current_frame_id = frame_id;

            self.total_packets = (real_packet_size as usize + PAYLOAD - 1) / PAYLOAD;

            self.remaining_subpacket_ids.clear();
            for i in 0..self.total_packets {
                self.remaining_subpacket_ids.insert(i);
            }

            self.frame.resize(real_packet_size as usize, 0u8);
        } else if self.current_frame_id != frame_id {
            // Frame ID mismatch
            #[cfg(feature = "stat")]
            debug!("Skipping {:?} Frame ({})", parse_packet_type(fragment), frame_id);

            self.current_frame_id = frame_id;
            self.total_packets = (real_packet_size as usize + PAYLOAD - 1) / PAYLOAD;

            self.remaining_subpacket_ids.clear();
            for i in 0..self.total_packets {
                self.remaining_subpacket_ids.insert(i);
            }
            self.frame.resize(real_packet_size as usize, 0u8);
        }

        let data = &fragment[HEADER..number_of_bytes];
        let packet_index = self.total_packets - 1 - remaing_packets as usize;

        let start = packet_index * PAYLOAD;
        let end = start + number_of_bytes - HEADER;
        self.frame[start..end]
            .copy_from_slice(data);
        self.remaining_subpacket_ids.remove(&(remaing_packets as usize));

        if remaing_packets == 0 || self.remaining_subpacket_ids.len() == 0 {
            if self.remaining_subpacket_ids.len() != 0 {
                return EAssemblerState::Waiting;
            }

            let mut assembled_packet = Vec::with_capacity(MAX_FRAME_SIZE);
            mem::swap(&mut self.frame, &mut assembled_packet);

            self.reset_for_next_frame();
            assembled(assembled_packet);

            return EAssemblerState::Finished;
        }

        EAssemblerState::Assembling
    }
}

 // TODO: optimal delay for retransmission in ms based on packet loss rate, internet network delay, etc.
const NAL_RETRANSMISSION_DELAY: u128 = 16;

pub struct NALPacketConstructor {
    frame: Vec<u8>,
    current_frame_id: i16,
    last_processed_frame_id: i16,
    current_real_packet_size: u32,
    total_packets: usize,
    retransmit_requests: HashMap<u16, u128>,
    remaining_subpacket_ids: HashSet<usize>,
    previous_subpacket_number: i16,
    packet_queue: VecDeque<Vec<u8>>,
    waiting: bool,

    #[cfg(feature = "stat")]
    retransmit_count: usize,
    #[cfg(feature = "stat")]
    retransmit_pollutants: usize,
    #[cfg(feature = "stat")]
    organic_count: usize,
    #[cfg(feature = "stat")]
    frame_count: usize,
}

impl NALPacketConstructor {
    pub fn new() -> Self {
        let frame = Vec::with_capacity(MAX_NAL_FRAME_SIZE);

        Self {
            frame,
            current_frame_id: -1,
            last_processed_frame_id: -1,
            current_real_packet_size: 0,
            total_packets: 0,
            previous_subpacket_number: -1,
            retransmit_requests: HashMap::new(),
            remaining_subpacket_ids: HashSet::new(),
            packet_queue: VecDeque::new(),
            waiting: false,

            #[cfg(feature = "stat")]
            frame_count: 0,
            #[cfg(feature = "stat")]
            organic_count: 0,
            #[cfg(feature = "stat")]
            retransmit_count: 0,
            #[cfg(feature = "stat")]
            retransmit_pollutants: 0
        }
    }

    #[inline]
    fn reset_for_next_frame(&mut self) {
        self.waiting = false;
        self.current_frame_id = -1;
        self.current_real_packet_size = 0;
        self.total_packets = 0;
        self.previous_subpacket_number = -1;
        self.remaining_subpacket_ids.clear();
        self.retransmit_requests.clear();
    }

    #[inline]
    pub fn assemble_packet<F, T>(
        &mut self, 
        fragment: &[u8], number_of_bytes: usize,
        assembled: &F,
        retransmit: &T
    ) -> EAssemblerState
        where 
            F: Fn(Vec<u8>) -> (),
            T: Fn(u8, u32, Vec<u16>) -> () 
    {
        let packet_type = parse_packet_type(fragment);
        let frame_id = parse_frame_id(fragment) as i16;

        if packet_type == EPacketType::RNAL && self.current_frame_id != frame_id {
            #[cfg(feature = "stat")]
            {
                self.retransmit_pollutants += 1;
            }
            return EAssemblerState::Assembling;
        } 

        #[cfg(feature = "stat")]
        if packet_type == EPacketType::NAL {
            self.organic_count += 1;

            // ~ roughly every 2 seconds (assuming 60 fps avg.)
            if self.frame_count == 120 {
                // Organic : Retransmit Ratio
                debug!("O:R Ratio: {}:{}",
                    self.organic_count, 
                    self.retransmit_count,
                );
                debug!("Pollutants: {}", self.retransmit_pollutants);

                self.organic_count = 0;
                self.retransmit_count = 0;
                self.retransmit_pollutants = 0;
                self.frame_count = 0;
            }
        }

        let remaing_packets = parse_packets_remaining(fragment);
        let real_packet_size = parse_real_packet_size(fragment);

        if self.current_frame_id == -1 {
            if self.last_processed_frame_id >= 0 && (frame_id as u8).wrapping_sub(self.last_processed_frame_id as u8) > 1 {
                warn!("Lost Frame: {} -> {}", self.last_processed_frame_id, frame_id);
            }

            #[cfg(feature = "stat")] {
                self.frame_count += 1;
            }
            
            self.current_frame_id = frame_id;
            self.current_real_packet_size = real_packet_size;

            self.total_packets = (real_packet_size as usize + PAYLOAD - 1) / PAYLOAD;

            self.remaining_subpacket_ids.clear();
            for i in 0..self.total_packets {
                self.remaining_subpacket_ids.insert(i);
            }

            self.frame.resize(real_packet_size as usize, 0u8);
        } else if self.current_frame_id != frame_id {     
            let nal_variant = parse_packet_type_variant(fragment);

            // If the frame id belongs to a key frame, clear queue and start a new frame
            if nal_variant == ENalVariant::KeyFrame as u8 {
                #[cfg(feature = "stat")] {
                    trace!("Freeing {} Queue Packets after Keyframe", self.packet_queue.len());
                    self.frame_count += 1;
                }

                self.packet_queue.clear();

                self.waiting = false;

                self.current_frame_id = frame_id;
                self.current_real_packet_size = real_packet_size;
                self.total_packets = (real_packet_size as usize + PAYLOAD - 1) / PAYLOAD;

                self.remaining_subpacket_ids.clear();
                for i in 0..self.total_packets {
                    self.remaining_subpacket_ids.insert(i);
                }
                self.previous_subpacket_number = -1;
                self.retransmit_requests.clear();

                self.frame.resize(real_packet_size as usize, 0u8);
            } else {
                // TODO: Request retransmissions of missing packets within the queue also!
                // so that when we recover the current frame, future frames are also recovered
                // store them in a seperate data structure (Retransmission Packet Pool)
               
                self.packet_queue.push_back(fragment[..number_of_bytes].to_vec());

                // At this point, we are mainly waiting for subpackets towards the end of the frame
                // that are missing 
                
                let mut potentionally_missing_packets= Vec::new();
                let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
                for subpacket_id in &self.remaining_subpacket_ids {
                    if let Some(previous_timestamp) = self.retransmit_requests.get(&(*subpacket_id as u16)) {
                        // request retransmission only if the previous request was more than NAL_RETRANSMISSION_DELAY (ms) ago
                        if (timestamp - previous_timestamp) < NAL_RETRANSMISSION_DELAY {
                            continue
                        } 
                    }

                    self.retransmit_requests.insert(*subpacket_id as u16, timestamp);
                    potentionally_missing_packets.push(*subpacket_id as u16);
                }

                if potentionally_missing_packets.len() > 0 {
                    #[cfg(feature = "stat")]
                    {
                        trace!("Requesting Retransmission (While Queuing) of: {:?}", potentionally_missing_packets);
                        self.retransmit_count += potentionally_missing_packets.len();
                    }
                    
                    retransmit(
                        self.current_frame_id as u8,
                        self.current_real_packet_size,
                        potentionally_missing_packets
                    );
                }

                self.waiting = true;
                return EAssemblerState::Waiting;
            }
        }

        // !this code will likely crash if frame id repeats before retransmission / receiving all subpackets

        let data = &fragment[HEADER..number_of_bytes];
        let packet_index = self.total_packets - 1 - remaing_packets as usize;

        let start = packet_index * PAYLOAD;
        let end = start + number_of_bytes - HEADER;
        
        // dangerous `end` index
        self.frame[start..end]
            .copy_from_slice(data);
        self.remaining_subpacket_ids.remove(&(remaing_packets as usize));

        // Should only ask for restramissions during initial assembly 
        // if assembler is waiting for restramissions, then request again once 
        if !self.waiting && remaing_packets.abs_diff(self.previous_subpacket_number as u16) > 1
            && self.previous_subpacket_number > 0
        {
            let mut potentionally_missing_packets= Vec::new();
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();

            if remaing_packets < self.previous_subpacket_number as u16 {
                for i in (remaing_packets+1)..(self.previous_subpacket_number as u16) {
                    if self.remaining_subpacket_ids.contains(&(i as usize)) {
                        if let Some(previous_timestamp) = self.retransmit_requests.get(&i) {
                            // request retransmission only if the previous request was more than 32ms ago
                            if (timestamp - previous_timestamp) < NAL_RETRANSMISSION_DELAY {
                                continue
                            } 
                        }
    
                        self.retransmit_requests.insert(i, timestamp);
                        potentionally_missing_packets.push(i);
                    }
                }
            } else {
                for i in (self.previous_subpacket_number as u16 + 1)..remaing_packets {
                    if self.remaining_subpacket_ids.contains(&(i as usize)) {
                        if self.retransmit_requests.contains_key(&i) {
                            continue;
                        }

                        self.retransmit_requests.insert(i, timestamp);
                        potentionally_missing_packets.push(i);
                    }
                }
            }

            if potentionally_missing_packets.len() != 0 {
                #[cfg(feature = "stat")]
                {
                    trace!("Requesting retransmission of packets: {:?}", potentionally_missing_packets);
                    self.retransmit_count += potentionally_missing_packets.len();
                }
    
                retransmit(frame_id as u8, real_packet_size, potentionally_missing_packets);        
            } 
        }

        // TODO: Should we keep the greater subpacket number?
        self.previous_subpacket_number = remaing_packets as i16;
        
        if remaing_packets == 0 || self.remaining_subpacket_ids.len() == 0 {
            if self.remaining_subpacket_ids.len() != 0 {
                // retransmit unrequested packets
                let mut potentionally_missing_packets= Vec::new();
                let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
                for subpacket_id in &self.remaining_subpacket_ids {
                    if self.retransmit_requests.contains_key(&(*subpacket_id as u16)) {
                        continue; 
                    }

                    self.retransmit_requests.insert(*subpacket_id as u16, timestamp);
                    potentionally_missing_packets.push(*subpacket_id as u16);
                }

                if potentionally_missing_packets.len() > 0 {
                    #[cfg(feature = "stat")]
                    trace!("Requesting Retransmission (While EOF) of: {:?}", potentionally_missing_packets);
                    retransmit(
                        self.current_frame_id as u8,
                        self.current_real_packet_size,
                        potentionally_missing_packets
                    );
                }

                #[cfg(feature = "stat")]
                trace!("Missing {:?} packets: {:?}", self.remaining_subpacket_ids.len(), self.remaining_subpacket_ids);
                self.waiting = true;
                return EAssemblerState::Waiting;
            }

            let mut assembled_packet = Vec::with_capacity(MAX_FRAME_SIZE);
            mem::swap(&mut self.frame, &mut assembled_packet);

            #[cfg(feature = "stat")]
            if self.waiting {
                trace!("Assembled after Waiting: {}", self.current_frame_id);
            }

            self.reset_for_next_frame();

            self.last_processed_frame_id = frame_id;
            assembled(assembled_packet);

            if self.packet_queue.len() > 0 {
                let mut packet_queue = VecDeque::new();
                mem::swap(&mut self.packet_queue, &mut packet_queue);
                return EAssemblerState::Queue(packet_queue);
            }

            return EAssemblerState::Finished;
        }

        EAssemblerState::Assembling
    }
}