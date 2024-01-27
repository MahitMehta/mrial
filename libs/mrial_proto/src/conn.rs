use serde::{Deserialize, Serialize};

use crate::HEADER;

pub const HANDSHAKE_PAYLOAD : usize = 512 - HEADER; 

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EHandshakePayload {
    pub width: u16,
    pub height: u16
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EHandshookPayload {
    pub widths: Vec<u16>,
    pub heights: Vec<u16>,
    pub width: u16,
    pub height: u16
}

pub fn write_handshake_payload(buf: &mut [u8], payload: EHandshakePayload) -> usize {
    let serialized_payload = serde_json::to_string(&payload).unwrap();
    let bytes = serialized_payload.as_bytes();
    buf.copy_from_slice(bytes);

    bytes.len()
}

pub fn parse_handshake_payload(buf: &mut [u8]) -> EHandshakePayload {
    let serialized_payload = std::str::from_utf8(buf).unwrap();
    let payload: EHandshakePayload = serde_json::from_str(serialized_payload).unwrap();
    
    payload
}
pub fn write_handshook_payload(buf: &mut [u8], payload: EHandshookPayload) -> usize {
    let serialized_payload = serde_json::to_string(&payload).unwrap();
    let bytes = serialized_payload.as_bytes();
    buf.copy_from_slice(bytes);

    bytes.len()
}

pub fn parse_handshook_payload(buf: &mut [u8]) -> EHandshookPayload {
    let serialized_payload = std::str::from_utf8(buf).unwrap();
    let payload: EHandshookPayload = serde_json::from_str(serialized_payload).unwrap();
    
    payload
}