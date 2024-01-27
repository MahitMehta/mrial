use serde::{Deserialize, Serialize};

use crate::{HEADER, MTU};

pub const HANDSHAKE_PAYLOAD : usize = 512 - HEADER; 
pub const CONN_STATE_PAYLOAD : usize = MTU - HEADER;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EHandshakePayload {
    pub width: u16,
    pub height: u16
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EConnStatePayload {
    pub widths: Vec<u16>,
    pub heights: Vec<u16>,
    pub width: u16,
    pub height: u16,
}

pub fn write_handshake_payload(buf: &mut [u8], payload: EHandshakePayload) -> usize {
    let serialized_payload = serde_json::to_string(&payload).unwrap();
    let bytes = serialized_payload.as_bytes();
    let len = bytes.len();
    buf[0..len].copy_from_slice(bytes);

    len
}

pub fn parse_handshake_payload(buf: &mut [u8]) -> Result<EHandshakePayload, serde_json::Error> {
    let serialized_payload = std::str::from_utf8(buf).unwrap();
    let payload: EHandshakePayload = serde_json::from_str(serialized_payload)?;
    
    Ok(payload)
}
pub fn write_state_payload(buf: &mut [u8], payload: EConnStatePayload) -> usize {
    let serialized_payload = serde_json::to_string(&payload).unwrap();
    let bytes = serialized_payload.as_bytes();
    let len = bytes.len();
    buf[0..len].copy_from_slice(bytes);

    bytes.len()
}

pub fn parse_handshook_payload(buf: &mut [u8]) -> Result<EConnStatePayload, serde_json::Error> {
    let serialized_payload = std::str::from_utf8(buf).unwrap();
    let payload: EConnStatePayload = serde_json::from_str(serialized_payload)?;
    
    Ok(payload)
}