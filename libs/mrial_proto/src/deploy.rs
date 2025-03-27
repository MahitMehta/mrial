use crate::{
    subpacket_count, write_dynamic_header, write_packet_type, write_packets_remaining, EPacketType,
    HEADER, MTU, PAYLOAD,
};

pub trait Broadcaster {
    fn broadcast(&self, bytes: &[u8]) -> impl std::future::Future<Output = ()> + Send;
}

pub struct PacketDeployer {
    frame_id: u8,
    xor: bool,
    xor_buf: [u8; MTU],
    buf: [u8; MTU],
}

impl PacketDeployer {
    pub fn new(packet_type: EPacketType, xor: bool) -> Self {
        let mut buf = [0u8; MTU];
        let mut xor_buf = [0u8; MTU];

        write_packet_type(packet_type, &mut buf);
        write_packet_type(EPacketType::XOR, &mut xor_buf);

        Self {
            xor,
            frame_id: 1,
            xor_buf,
            buf,
        }
    }

    #[inline]
    pub async fn slice_and_send<T: Broadcaster>(&mut self, bytes: &[u8], broadcaster: &T) {
        let real_packet_size = bytes.len() as u32;
        let subpackets = subpacket_count(real_packet_size);

        write_dynamic_header(real_packet_size, self.frame_id, &mut self.buf);
        write_dynamic_header(real_packet_size, self.frame_id, &mut self.xor_buf);

        if self.xor && subpackets > 2 {
            PacketDeployer::broadcast_xor(subpackets, &bytes, &mut self.xor_buf, broadcaster).await;
        }

        for i in 0..subpackets {
            write_packets_remaining(subpackets - i - 1, &mut self.buf);

            let start = (i as usize) * PAYLOAD;
            let addition = if start + PAYLOAD <= bytes.len() {
                PAYLOAD
            } else {
                bytes.len() - start
            };
            self.buf[HEADER..addition + HEADER].copy_from_slice(&bytes[start..addition + start]);

            broadcaster.broadcast(&self.buf[0..addition + HEADER]).await;
        }
    }

    // TODO: The broadcast XOR function needs to be rewritten for improved performance and flexibility

    #[inline]
    async fn broadcast_xor<T: Broadcaster>(
        subpackets: u16,
        bytes: &[u8],
        deployment_xor_buf: &mut [u8],
        broadcaster: &T,
    ) {
        let parity_packet_count = (subpackets as f32 / 3.0).ceil() as usize; // 4

        for i in 0..parity_packet_count {
            // for i in (parity_packet_count / 2)..parity_packet_count {
            let packet_one = i + parity_packet_count * 0;
            let packet_two = i + parity_packet_count * 1;
            let packet_three = i + parity_packet_count * 2;

            write_packets_remaining(subpackets - i as u16 - 1, deployment_xor_buf);

            for n in 0..PAYLOAD {
                let byte_one = bytes.get(packet_one * PAYLOAD + n).unwrap_or(&0);
                let byte_two = bytes.get(packet_two * PAYLOAD + n).unwrap_or(&0);
                let byte_three = bytes.get(packet_three * PAYLOAD + n).unwrap_or(&0);

                deployment_xor_buf[HEADER + n] = byte_one ^ byte_two ^ byte_three;
            }

            broadcaster.broadcast(&deployment_xor_buf).await;
        }
    }
}
