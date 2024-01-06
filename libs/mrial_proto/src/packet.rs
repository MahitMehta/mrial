#[derive(Debug)]
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
// Real Packet Byte Size = 4 bytes // TODO: reduce to 3 bytes
// Packet ID = 1 Byte

// Payload Schema
// variables sized unencrypted bytes (MAX = MTU - HEADER) 

// let start = SystemTime::now();
// let since_the_epoch = start.duration_since(UNIX_EPOCH).unwrap();
// println!("{}", since_the_epoch.subsec_millis());

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
    let real_packet_size =  parse_real_packet_size(buf);
    let packet_id = parse_packet_id(buf);

    (packet_type, packets_remaining, real_packet_size, packet_id)
}

pub struct PacketConstructor {
    packet: Vec<Vec<u8>>,
    previous_subpacket_number: i16,
    order_mismatch: bool
}

impl PacketConstructor {
    pub fn new() -> Self {
        Self {
            packet: Vec::new(),
            previous_subpacket_number: -1,
            order_mismatch: false
        }
    }

    // TODO: Actually make this method functional.
    // Cache previously dropped/out of order messages and reassemble them
    #[inline]
    fn reconstruct_packet(&mut self) -> bool {
        println!("Entire Packet Dropped due to Mixup");
        self.order_mismatch = false; 
        self.packet.clear();
        return false
    }

    #[inline]
    fn filter_packet(&mut self) {
        println!("Excess Packets Filtered");
        let last_packet_id = parse_packet_id(self.packet.last().unwrap());
        self.packet.retain(|packet_unit| {
            parse_packet_id(&packet_unit) == last_packet_id
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

        let packet_size = (self.packet.len() - 1) * PAYLOAD + self.packet.last().unwrap().len() - HEADER;
        if real_packet_size > packet_size {
            return self.reconstruct_packet();
        } else if real_packet_size < packet_size {
            self.filter_packet();
            
            // ### DEBUG ###
            {
                let nal_size = (self.packet.len() - 1) * PAYLOAD + self.packet.last().unwrap().len() - HEADER;
                println!("Packet Size After Fix: {} vs {}", nal_size, real_packet_size);
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
    pub fn assemble_packet(
        &mut self, 
        buf: &[u8], 
        number_of_bytes: usize
    ) -> Option<Vec<u8>> {
        let packets_remaining = parse_packets_remaining(buf);
        let real_packet_size = parse_real_packet_size(buf);

        if self.previous_subpacket_number != (packets_remaining + 1) as i16 && 
            self.previous_subpacket_number > 0 {
            println!("Packet Order Mixup: {} -> {}", self.previous_subpacket_number, packets_remaining);
            self.order_mismatch = true; 
        } 
        self.previous_subpacket_number = packets_remaining as i16;

        self.packet.push(buf[..number_of_bytes].to_vec());
        if packets_remaining != 0 { return None; }

        if self.order_mismatch && 
            !self.handle_order_mismatch(
                real_packet_size.try_into().unwrap()) {
            return None
        }

        let mut assembled_packet = Vec::new();
        for packet in &self.packet {
            assembled_packet.extend_from_slice(&packet[HEADER..]);
        }
        self.packet.clear();    

        Some(assembled_packet) 
    }
}