use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum EColorSpace {
    YUV444 = 12,
    YUV420 = 2,
}

impl Serialize for EColorSpace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl Into<usize> for EColorSpace {
    fn into(self) -> usize {
        self as usize
    }
}