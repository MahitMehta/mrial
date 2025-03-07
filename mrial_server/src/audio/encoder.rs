use opus::{Channels, Encoder};

pub struct AudioEncoder {
    encoder: Encoder
}

impl AudioEncoder {
    pub fn new(
        sample_rate: u32,
        channels: Channels) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            encoder: Encoder::new(sample_rate, channels, opus::Application::LowDelay)?,
        })
    }

    pub fn encode(&self, samples: &[i32]) {
      
    }
}
