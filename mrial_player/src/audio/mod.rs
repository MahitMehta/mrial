use rodio::{buffer::SamplesBuffer, Sink};

use mrial_proto::*; 

pub struct AudioClient {
    audio_stream: Vec<u8>,
    sink: Sink
}

impl AudioClient {
    pub fn new(sink : Sink) -> AudioClient {
        AudioClient {
            audio_stream: Vec::new(),
            sink
        }
    }

    pub fn play_audio_stream(&mut self, buf: &[u8], number_of_bytes: usize) {
        let u8_slice = &buf[HEADER..number_of_bytes];
        self.audio_stream.extend_from_slice(u8_slice);
        
        let packets_remaining_bytes: [u8; 2] = buf[1..3].try_into().unwrap();
        let packets_remaining = u16::from_be_bytes(packets_remaining_bytes) as usize;
        if packets_remaining != 0 { return; }  

        let f32_slice = unsafe {
            std::slice::from_raw_parts(self.audio_stream.as_ptr() as *const f32, self.audio_stream.len() / std::mem::size_of::<f32>())
        };
        
        let audio_buf = SamplesBuffer::new(2, 48000, f32_slice);
    
        self.sink.append(audio_buf);
        self.audio_stream.clear();
    }
}
