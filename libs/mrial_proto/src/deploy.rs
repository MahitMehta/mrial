use chacha20poly1305::{aead::Aead, AeadCore, ChaCha20Poly1305};
use rand::rngs::ThreadRng;

use crate::{
    subpacket_count, 
    write_dynamic_header,
    write_packet_type, 
    write_packets_remaining, 
    EPacketType, 
    HEADER, 
    MTU, 
    PAYLOAD
};

pub struct PacketDeployer {
    xor: bool,
    xor_buf: [u8; MTU],
    buf: [u8; MTU],
    packet_id: u8,
    rng: ThreadRng,
    sym_key: Option<ChaCha20Poly1305>,
}

impl PacketDeployer {
    pub fn new(packet_type: EPacketType, xor: bool) -> Self {
        let mut buf = [0u8; MTU];
        let mut xor_buf = [0u8; MTU];

        write_packet_type(packet_type, &mut buf);
        write_packet_type(EPacketType::XOR, &mut xor_buf);

        Self {
            xor,
            xor_buf,
            buf,
            packet_id: 1,
            rng: rand::thread_rng(),
            sym_key: None,
        }
    }

    pub fn set_sym_key(&mut self, sym_key: ChaCha20Poly1305) {
        self.sym_key = Some(sym_key);
    }

    pub fn has_sym_key(&self) -> bool {
        self.sym_key.is_some()
    }

    #[inline]
    pub fn prepare<'a>(&mut self, frame: &[u8], broadcast: Box<dyn Fn(&[u8]) + 'a>) {
        let bytes = match self.encrypted_frame(frame) {
            Some(ciphertext) => ciphertext,
            None => return,
        };

        let real_packet_size = bytes.len() as u32;
        let subpackets = subpacket_count(real_packet_size);

        write_dynamic_header(real_packet_size, self.packet_id, &mut self.buf);
        write_dynamic_header(real_packet_size, self.packet_id, &mut self.xor_buf);

        if subpackets > 2 {
            let parity_packet_count = (subpackets as f32 / 3.0).ceil() as usize; // 4

            for i in 0..parity_packet_count {
            // for i in (parity_packet_count / 2)..parity_packet_count {
                let packet_one = i + parity_packet_count * 0;
                let packet_two = i +  parity_packet_count * 1;
                let packet_three = i + parity_packet_count * 2; 

                write_packets_remaining(subpackets - i as u16 - 1, &mut self.xor_buf);

                for n in 0..PAYLOAD {
                    let byte_one = bytes.get(packet_one * PAYLOAD + n).unwrap_or(&0);
                    let byte_two = bytes.get(packet_two * PAYLOAD + n).unwrap_or(&0);
                    let byte_three = bytes.get(packet_three * PAYLOAD + n).unwrap_or(&0);

                    self.xor_buf[HEADER + n] = byte_one ^ byte_two ^ byte_three;
                }

                broadcast(&self.xor_buf);
            }
        }

        for i in 0..subpackets {
            write_packets_remaining(subpackets - i - 1, &mut self.buf);

            let start = (i as usize) * PAYLOAD;
            let addition = if start + PAYLOAD <= bytes.len() {
                PAYLOAD
            } else {
                bytes.len() - start
            };
            self.buf[HEADER..addition + HEADER]
                .copy_from_slice(&bytes[start..addition + start]);

            broadcast(&self.buf[0..addition + HEADER]);
        }

        self.packet_id += 1;
    }

    fn encrypted_frame(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        if let Some(sym_key) = &self.sym_key {
            let nonce = ChaCha20Poly1305::generate_nonce(&mut self.rng);
            let mut ciphertext = sym_key.encrypt(&nonce, frame).unwrap();
            ciphertext.extend_from_slice(&nonce);

            return Some(ciphertext);
        }

        None
    }
}
