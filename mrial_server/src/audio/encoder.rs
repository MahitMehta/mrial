use opus::{Channels, Encoder};

pub const ENCODE_FRAME_SIZE : usize = 1920 * 2;
pub struct OpusEncoder {
    encoder: Encoder,
    buffer: [f32; ENCODE_FRAME_SIZE]
}

impl OpusEncoder {
    pub fn new(
        sample_rate: u32,
        channels: Channels) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            encoder: Encoder::new(sample_rate, channels, opus::Application::LowDelay)?,
            buffer: [0f32; ENCODE_FRAME_SIZE]
        })
    }
 
    pub fn encode_32bit(&mut self, samples: &[u8], output: &mut [u8]) -> Result<usize, Box<dyn std::error::Error>> {
        if samples.len() % 4 != 0 {
            return Err("Invalid sample length (Not divisible by 4)".into());
        }

        let f32_slice: &[f32] = unsafe {
            std::slice::from_raw_parts(
                samples.as_ptr() as *const f32, samples.len() / 4)
        };

        if f32_slice.len() < ENCODE_FRAME_SIZE {
            self.buffer[0..f32_slice.len()].copy_from_slice(f32_slice);
        } else {
            self.buffer.copy_from_slice(&f32_slice[0..ENCODE_FRAME_SIZE]);
        }

        Ok(self.encoder.encode_float(&self.buffer, output)?)
    }
}
