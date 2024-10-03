use chacha20poly1305::{aead::Aead, AeadCore, ChaCha20Poly1305};
use rand::rngs::ThreadRng;

use crate::{
    subpacket_count, write_dynamic_header, write_packet_type, write_packets_remaining, EPacketType,
    HEADER, MTU, PAYLOAD,
};

pub struct PacketDeployer {
    buf: [u8; MTU],
    packet_id: u8,
    rng: ThreadRng,
    sym_key: Option<ChaCha20Poly1305>,
}

impl PacketDeployer {
    pub fn new(packet_type: EPacketType) -> Self {
        let mut buf = [0u8; MTU];
        write_packet_type(packet_type, &mut buf);

        Self {
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
        let ciphertext = match self.encrypted_frame(frame) {
            Some(ciphertext) => ciphertext,
            None => return,
        };

        let real_packet_size = ciphertext.len() as u32;
        let packets = subpacket_count(real_packet_size);

        write_dynamic_header(real_packet_size, self.packet_id, &mut self.buf);

        for i in 0..packets {
            write_packets_remaining(packets - i - 1, &mut self.buf);

            let start = (i as usize) * PAYLOAD;
            let addition = if start + PAYLOAD <= ciphertext.len() {
                PAYLOAD
            } else {
                ciphertext.len() - start
            };
            self.buf[HEADER..addition + HEADER]
                .copy_from_slice(&ciphertext[start..(addition + start)]);

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
