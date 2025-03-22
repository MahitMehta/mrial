use std::mem;

use opus::{Channels, Encoder};

pub const ENCODE_FRAME_SIZE: usize = 1920;
pub struct OpusEncoder {
    encoder: Encoder,
    buf: Vec<u8>,
    cursor: usize,
}

impl OpusEncoder {
    pub fn new(sample_rate: u32, channels: usize) -> Result<Self, Box<dyn std::error::Error>> {
        assert!(channels == 1 || channels == 2);

        let channel_setting = match channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => unreachable!(),
        };

        let encoder = Encoder::new(
            sample_rate, 
            channel_setting, 
            opus::Application::LowDelay)?;
    
        let mut buf = Vec::with_capacity(ENCODE_FRAME_SIZE * channels * mem::size_of::<f32>());
        buf.resize(ENCODE_FRAME_SIZE * channels * mem::size_of::<f32>(), 0u8);

        Ok(Self {
            encoder,
            buf,
            cursor: 0
        })
    }

    pub fn encode_f32(
        &mut self,
        samples: &[u8],
        output: &mut [u8],
    ) -> Result<Option<usize>, Box<dyn std::error::Error>> {
        assert!(samples.len() % mem::size_of::<f32>() == 0, "Invalid sample length (Not divisible by sizeof(f32)): {}", samples.len()); 
        assert!(samples.len() <= self.buf.len(), "Invalid sample length (Exceeds max frame size): {}", self.buf.len());

        let buf_len = self.buf.len();
        if self.cursor + samples.len() <= buf_len {
            self.buf[self.cursor..self.cursor + samples.len()].copy_from_slice(samples);
            self.cursor += samples.len();

            // If the buffer is full, encode the buffer
            if self.cursor == buf_len {
                self.cursor = 0;
                let f32_slice: &[f32] = unsafe {
                    std::slice::from_raw_parts(self.buf.as_ptr() as *const f32, self.buf.len() / mem::size_of::<f32>())
                };

                return Ok(Some(self.encoder.encode_float(&f32_slice, output)?));
            } else {
                // Return early if the buffer is not full
                return Ok(None);
            }
        } 
        
        // If the buffer is not full, copy the needed samples to the buffer and encode the buffer
        self.buf[self.cursor..].copy_from_slice(&samples[0..buf_len - self.cursor]);
        
        let f32_slice: &[f32] = unsafe {
            std::slice::from_raw_parts(self.buf.as_ptr() as *const f32, buf_len / mem::size_of::<f32>())
        };

        let encoded = self.encoder.encode_float(&f32_slice, output)?;

        // Set the cursor to the remaining samples
        self.cursor = samples.len() - (buf_len - self.cursor); 

        // Copy the remaining samples to the buffer
        self.buf[0..self.cursor].copy_from_slice(&samples[samples.len() - self.cursor..]);

        return Ok(Some(encoded));
    }
}
