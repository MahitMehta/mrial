use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EColorSpace {
    YUV444 = 12,
    YUV420 = 2,
}

impl Default for EColorSpace {
    fn default() -> Self {
        EColorSpace::YUV444
    }
}

impl Serialize for EColorSpace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for EColorSpace {
    fn deserialize<D>(deserializer: D) -> Result<EColorSpace, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        match value {
            12 => Ok(EColorSpace::YUV444),
            2 => Ok(EColorSpace::YUV420),
            _ => Err(serde::de::Error::custom(format!(
                "Invalid EColorSpace value: {}",
                value
            ))),
        }
    }
}

impl Into<usize> for EColorSpace {
    fn into(self) -> usize {
        self as usize
    }
}