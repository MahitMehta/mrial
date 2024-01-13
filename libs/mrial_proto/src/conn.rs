pub const HANDSHAKE_PAYLOAD : usize = 4; 

pub struct EHandshakePayload {
    pub width: u16,
    pub height: u16
}

pub fn write_handshake_payload(buf: &mut [u8], payload: EHandshakePayload) {
    buf[0..2].copy_from_slice( &payload.width.to_be_bytes());
    buf[2..4].copy_from_slice(&payload.height.to_be_bytes());
}

pub fn parse_handshake_payload(buf: &mut [u8]) -> EHandshakePayload {
    EHandshakePayload {
        width: u16::from_be_bytes(buf[0..2].try_into().unwrap()),
        height: u16::from_be_bytes(buf[2..4].try_into().unwrap())
    }
}