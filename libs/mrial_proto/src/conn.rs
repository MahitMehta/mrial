use chacha20poly1305::{aead::AeadMut, AeadCore, ChaCha20Poly1305};
use rand::rngs::ThreadRng;
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};

use crate::video::EColorSpace;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientStatePayload {
    pub version: String,
    pub width: u16,
    pub height: u16,
    pub muted: bool,
    pub opus: bool,
    pub csp: EColorSpace
}

impl JSONPayloadSE for ClientStatePayload {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerStatePayload {
    pub widths: Vec<u16>,
    pub heights: Vec<u16>,
    pub width: u16,
    pub height: u16
}

impl JSONPayloadSE for ServerStatePayload {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerShookUE {
    pub pub_key: String,
}

impl JSONPayloadUE for ServerShookUE {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientShakeAE {
    pub state: ClientStatePayload,
    pub username: String,
    pub pass: String,
    pub sym_key: String,
}

impl JSONPayloadAE for ClientShakeAE {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerShookSE {
    pub server_state: ServerStatePayload,
}

impl JSONPayloadSE for ServerShookSE {}

pub const SE_NONCE: usize = 12;

#[derive(Debug)]
struct JSONPayloadSEError;
impl std::fmt::Display for JSONPayloadSEError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Error parsing JSON Payload from SE Payload")
    }
}
impl std::error::Error for JSONPayloadSEError {}

pub trait JSONPayloadSE: serde::Serialize + serde::de::DeserializeOwned {
    fn write_payload(
        buf: &mut [u8],
        sym_key: Option<ChaCha20Poly1305>,
        payload: &Self,
    ) -> Result<usize, &'static str> {
        let serialized_payload = serde_json::to_string(&payload).unwrap();
        let nonce = ChaCha20Poly1305::generate_nonce(ThreadRng::default());
        let bytes = serialized_payload.as_bytes();

        let mut sym_key = match sym_key {
            Some(key) => key,
            None => return Err("No Symmetric Key"),
        };

        let encrypted_payload: Vec<u8> = sym_key.encrypt(&nonce, bytes).unwrap();
        let payload_len: usize = encrypted_payload.len();
        buf[0..payload_len].copy_from_slice(&encrypted_payload);
        buf[payload_len..payload_len + SE_NONCE].copy_from_slice(&nonce);

        Ok(payload_len + SE_NONCE)
    }

    fn from_payload(
        buf: &[u8],
        sym_key: &mut ChaCha20Poly1305,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let encrypted_payload = &buf[0..buf.len() - SE_NONCE];
        let nonce = &buf[buf.len() - 12..buf.len()];
        let nonce = nonce.try_into().map_err(|_| "Corrupted SE Nonce")?;

        if let Ok(decrypted_payload) = sym_key.decrypt(nonce, encrypted_payload) {
            let serialized_payload = std::str::from_utf8(&decrypted_payload)?;
            let payload: Self = serde_json::from_str(serialized_payload)?;

            return Ok(payload);
        }

        Err(Box::new(JSONPayloadSEError {}))
    }
}

pub trait JSONPayloadUE: serde::Serialize + serde::de::DeserializeOwned {
    fn write_payload(buf: &mut [u8], payload: &Self) -> usize {
        let serialized_payload = serde_json::to_string(&payload).unwrap();
        let bytes = serialized_payload.as_bytes();
        let len = bytes.len();
        buf[0..len].copy_from_slice(bytes);

        len
    }

    fn from_payload(buf: &mut [u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let serialized_payload = std::str::from_utf8(buf)?;
        let payload: Self = serde_json::from_str(serialized_payload)?;

        Ok(payload)
    }
}

pub trait JSONPayloadAE: serde::Serialize + serde::de::DeserializeOwned {
    fn write_payload(
        buf: &mut [u8],
        rng: &mut ThreadRng,
        pub_key: RsaPublicKey,
        payload: &Self,
    ) -> usize {
        let serialized_payload = serde_json::to_string(&payload).unwrap();

        // TODO: Allow larger payloads (current max is 256 bytes)
        let encrypted_payload = pub_key
            .encrypt(rng, Pkcs1v15Encrypt, serialized_payload.as_bytes())
            .unwrap();

        let len = encrypted_payload.len();
        buf[0..len].copy_from_slice(&encrypted_payload);

        len
    }

    fn from_payload(
        buf: &[u8],
        priv_key: RsaPrivateKey,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let unencypted_payload = priv_key.decrypt(Pkcs1v15Encrypt, &buf)?;
        let serialized_payload = std::str::from_utf8(&unencypted_payload)?;
        let payload: Self = serde_json::from_str(serialized_payload)?;

        Ok(payload)
    }
}
