use rodio::{buffer::SamplesBuffer, Sink};

use mrial_proto::*; 

pub struct AudioClient {
    audio_stream: Vec<u8>,
    sink: Sink
}

const AUDIO_LATENCY_TOLERANCE: usize = 3; 

impl AudioClient {
    pub fn new(sink : Sink) -> AudioClient {
        AudioClient {
            audio_stream: Vec::new(),
            sink
        }
    }

    pub fn play_audio_stream(&mut self, buf: &[u8], number_of_bytes: usize, packets_remaining: u16) {
        if !assembled_packet(&mut self.audio_stream, &buf, number_of_bytes, packets_remaining) {
            return;
        }; 

        let f32_slice = unsafe {
            std::slice::from_raw_parts(self.audio_stream.as_ptr() as *const f32, self.audio_stream.len() / std::mem::size_of::<f32>())
        };
        
        let audio_buf = SamplesBuffer::new(2, 48000, f32_slice);
    
        self.sink.append(audio_buf);

        // Skip audio packet to correct latency, 
        // consider speeding up audio instead
        if self.sink.len() > AUDIO_LATENCY_TOLERANCE {
            println!("Recorrecting Audio by Skipping One: {}", self.sink.len());
            self.sink.skip_one();
        } 

        self.audio_stream.clear();
    }
}

