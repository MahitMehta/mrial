use serde::{Deserialize, Serialize};

use crate::{HEADER, MTU};

pub const CLIENT_STATE_PAYLOAD : usize = 512 - HEADER; 
pub const SERVER_STATE_PAYLOAD : usize = MTU - HEADER;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientStatePayload {
    pub width: u16,
    pub height: u16
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerStatePayload {
    pub widths: Vec<u16>,
    pub heights: Vec<u16>,
    pub width: u16,
    pub height: u16,
}

pub fn write_handshake_payload(buf: &mut [u8], payload: ClientStatePayload) -> usize {
    let serialized_payload = serde_json::to_string(&payload).unwrap();
    let bytes = serialized_payload.as_bytes();
    let len = bytes.len();
    buf[0..len].copy_from_slice(bytes);

    len
}

pub fn parse_handshake_payload(buf: &mut [u8]) -> Result<ClientStatePayload, serde_json::Error> {
    let serialized_payload = std::str::from_utf8(buf).unwrap();
    let payload: ClientStatePayload = serde_json::from_str(serialized_payload)?;
    
    Ok(payload)
}
pub fn write_state_payload(buf: &mut [u8], payload: ServerStatePayload) -> usize {
    let serialized_payload = serde_json::to_string(&payload).unwrap();
    let bytes = serialized_payload.as_bytes();
    let len = bytes.len();
    buf[0..len].copy_from_slice(bytes);

    bytes.len()
}

pub fn parse_state_payload(buf: &mut [u8]) -> Result<ServerStatePayload, serde_json::Error> {
    let serialized_payload = std::str::from_utf8(buf).unwrap();
    let payload: ServerStatePayload = serde_json::from_str(serialized_payload)?;
    
    Ok(payload)
}