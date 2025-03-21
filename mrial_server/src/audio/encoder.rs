use opus::{Channels, Encoder};

pub const ENCODE_FRAME_SIZE: usize = 1920 * 2;
pub struct OpusEncoder {
    encoder: Encoder,
    buffer: [f32; ENCODE_FRAME_SIZE],
    cursor: usize
}

impl OpusEncoder {
    pub fn new(sample_rate: u32, channels: Channels) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            encoder: Encoder::new(sample_rate, channels, opus::Application::LowDelay)?,
            buffer: [0f32; ENCODE_FRAME_SIZE],
            cursor: 0,
        })
    }

    pub fn encode_32bit(
        &mut self,
        samples: &[u8],
        output: &mut [u8],
    ) -> Result<Option<usize>, Box<dyn std::error::Error>> {
        if samples.len() % 4 != 0 {
            return Err("Invalid sample length (Not divisible by 4)".into());
        }

        let f32_slice: &[f32] = unsafe {
            std::slice::from_raw_parts(samples.as_ptr() as *const f32, samples.len() / 4)
        };

        // If the buffer has enough space, copy all the samples into the buffer
        if self.cursor + f32_slice.len() <= ENCODE_FRAME_SIZE {
            self.buffer[self.cursor..f32_slice.len()].copy_from_slice(f32_slice);
            self.cursor += f32_slice.len();
        } else {
            // Copy the whatever samples that can fit in the buffer
            self.buffer[self.cursor..ENCODE_FRAME_SIZE]
                .copy_from_slice(&f32_slice[..ENCODE_FRAME_SIZE - self.cursor]);
            self.cursor = ENCODE_FRAME_SIZE;
        }

        if self.cursor < ENCODE_FRAME_SIZE {
            return Ok(None);
        }

        let compressed_len = self.encoder.encode_float(&self.buffer, output)?;
        self.cursor = 0;

        Ok(Some(compressed_len))
    }
}
