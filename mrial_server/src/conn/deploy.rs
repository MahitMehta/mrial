use std::sync::{Arc, RwLock};

use bytes::Bytes;
use chacha20poly1305::{aead::Aead, AeadCore, ChaCha20Poly1305};
use mrial_proto::{subpacket_count, write_dynamic_header, write_packet_type, write_packets_remaining, EPacketType, HEADER, MTU, PAYLOAD};
use rand::rngs::ThreadRng;

use super::web::WebConnection;

pub struct PacketDeployer {
    xor: bool,
  
    encrypted_frame_id: u8,
    encrypted_xor_buf: [u8; MTU],
    encrypted_buf: [u8; MTU],
    rng: ThreadRng,
    sym_key: Arc<RwLock<Option<ChaCha20Poly1305>>>,

    web_frame_id: u8,
    web_xor_buf: [u8; MTU],
    web_buf: [u8; MTU],
}

impl PacketDeployer {
    pub fn new(packet_type: EPacketType, xor: bool) -> Self {
        let mut encrypted_buf = [0u8; MTU];
        let mut encrypted_xor_buf = [0u8; MTU];

        let mut unencrypted_buf = [0u8; MTU];
        let mut unencrypted_xor_buf = [0u8; MTU];

        write_packet_type(packet_type, &mut encrypted_buf);
        write_packet_type(EPacketType::XOR, &mut encrypted_xor_buf);

        write_packet_type(packet_type, &mut unencrypted_buf);
        write_packet_type(EPacketType::XOR, &mut unencrypted_xor_buf);

        Self {
            xor,
  
            encrypted_frame_id: 1,
            encrypted_xor_buf,
            encrypted_buf,
            rng: rand::thread_rng(),
            sym_key: Arc::new(RwLock::new(None)),

            web_frame_id: 1,
            web_xor_buf: unencrypted_xor_buf,
            web_buf: unencrypted_buf,
        }
    }

    pub fn set_sym_key(&mut self, sym_key: ChaCha20Poly1305) {
        if let Ok(mut sym_key_lock) = self.sym_key.write() {
            *sym_key_lock = Some(sym_key);
        }
    }

    pub fn has_sym_key(&self) -> bool {
        if let Ok(sym_key_lock) = self.sym_key.read() {
            return sym_key_lock.is_some();
        }

        false
    }

    #[inline]
    pub fn prepare_encrypted<'a>(
        &mut self, 
        frame: &[u8], 
        broadcast_encrypted: Box<dyn Fn(&[u8]) + 'a>
    ) {
        self.helper_prepare_encrypted(frame, &broadcast_encrypted);

        self.encrypted_frame_id += 1;
    }

    #[inline]
    fn helper_prepare_encrypted(&mut self, frame: &[u8], broadcast: &dyn Fn(&[u8])) {
        let bytes = match self.encrypted_frame(frame) {
            Some(ciphertext) => ciphertext,
            _ => return,
        };

        let real_packet_size = bytes.len() as u32;
        let subpackets = subpacket_count(real_packet_size);

        write_dynamic_header(real_packet_size, self.encrypted_frame_id, &mut self.encrypted_buf);
        write_dynamic_header(real_packet_size, self.encrypted_frame_id, &mut self.encrypted_xor_buf);

        // if self.xor && subpackets > 2 {
        //     PacketDeployer::broadcast_xor(
        //         subpackets, 
        //         &bytes, 
        //         &mut self.encrypted_xor_buf, 
        //         &broadcast);
        // }

        PacketDeployer::fragment_and_broadcast(
            subpackets, 
            &bytes, 
            &mut self.encrypted_buf, 
            &broadcast);
    }

    #[inline]
    fn fragment_and_broadcast(
        subpackets: u16, 
        bytes: &[u8], 
        deployment_buf: &mut [u8], 
        broadcast: &dyn Fn(&[u8])) {
        for i in 0..subpackets {
            write_packets_remaining(subpackets - i - 1, deployment_buf);

            let start = (i as usize) * PAYLOAD;
            let addition = if start + PAYLOAD <= bytes.len() {
                PAYLOAD
            } else {
                bytes.len() - start
            };
            deployment_buf[HEADER..addition + HEADER]
                .copy_from_slice(&bytes[start..addition + start]);

            broadcast(&deployment_buf[0..addition + HEADER]);
        }
    }

    #[inline]
    pub async fn prepare_web(
        &mut self, 
        frame: Vec<u8>, 
        conn: WebConnection) {
        self.helper_prepare_web(&frame, conn).await;

        self.web_frame_id += 1;
    }

    #[inline]
    pub async fn prepare_web_from_ref(
        &mut self, 
        frame: &[u8], 
        conn: WebConnection) {
        self.helper_prepare_web(frame, conn).await;

        self.web_frame_id += 1;
    }
    

    #[inline]
    async fn helper_prepare_web(
        &mut self, 
        frame: &[u8], 
        conn: WebConnection) {
        let real_packet_size = frame.len() as u32;
        let subpackets = subpacket_count(real_packet_size);

        write_dynamic_header(real_packet_size, self.web_frame_id, &mut self.web_buf);
        write_dynamic_header(real_packet_size, self.web_frame_id, &mut self.web_xor_buf);

        // if self.xor && subpackets > 2 {
        //     PacketDeployer::broadcast_xor(
        //         subpackets, 
        //         &frame, 
        //         &mut self.unencrypted_xor_buf, 
        //         &broadcast);
        // }

        PacketDeployer::fragment_and_broadcast_web(
            subpackets, 
            &frame, 
            &mut self.web_buf, 
            conn).await;
    }

    #[inline]
    async fn fragment_and_broadcast_web(
        subpackets: u16, 
        bytes: &[u8], 
        deployment_buf: &mut [u8], 
        conn: WebConnection) {
        for i in 0..subpackets {
            write_packets_remaining(subpackets - i - 1, deployment_buf);

            let start = (i as usize) * PAYLOAD;
            let addition = if start + PAYLOAD <= bytes.len() {
                PAYLOAD
            } else {
                bytes.len() - start
            };
            deployment_buf[HEADER..addition + HEADER]
                .copy_from_slice(&bytes[start..addition + start]);

            
            let bytes = Bytes::copy_from_slice(&deployment_buf[0..addition + HEADER]);
            conn.broadcast(bytes).await;
        }
    }

    // TODO: The broadcast XOR function needs to be rewritten for improved performance and flexibility

    // #[inline]
    // fn broadcast_xor(
    //     subpackets: u16, 
    //     bytes: &[u8], 
    //     deployment_xor_buf: &mut [u8],
    //     broadcast: &dyn Fn(&[u8])
    // ) {
    //     let parity_packet_count = (subpackets as f32 / 3.0).ceil() as usize; // 4

    //     for i in 0..parity_packet_count {
    //     // for i in (parity_packet_count / 2)..parity_packet_count {
    //         let packet_one = i + parity_packet_count * 0;
    //         let packet_two = i +  parity_packet_count * 1;
    //         let packet_three = i + parity_packet_count * 2; 

    //         write_packets_remaining(subpackets - i as u16 - 1, deployment_xor_buf);

    //         for n in 0..PAYLOAD {
    //             let byte_one = bytes.get(packet_one * PAYLOAD + n).unwrap_or(&0);
    //             let byte_two = bytes.get(packet_two * PAYLOAD + n).unwrap_or(&0);
    //             let byte_three = bytes.get(packet_three * PAYLOAD + n).unwrap_or(&0);

    //             deployment_xor_buf[HEADER + n] = byte_one ^ byte_two ^ byte_three;
    //         }

    //         broadcast(&deployment_xor_buf);
    //     }
    // }

    fn encrypted_frame(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        if let Ok(sym_key_lock) = self.sym_key.read() {
            if let Some(sym_key) = sym_key_lock.as_ref() {
                let nonce = ChaCha20Poly1305::generate_nonce(&mut self.rng);
                let mut ciphertext = sym_key.encrypt(&nonce, frame).unwrap();
                ciphertext.extend_from_slice(&nonce);
    
                return Some(ciphertext);
            }
        }

        None
    }
}
