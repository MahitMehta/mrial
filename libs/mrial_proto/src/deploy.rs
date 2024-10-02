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