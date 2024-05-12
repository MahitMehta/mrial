use rodio::{buffer::SamplesBuffer, Sink};
use log::debug;
use mrial_proto::*;

pub struct AudioClient {
    packet_constructor: PacketConstructor,
    sink: Sink,
    // speed_adjustment_counter: f32,
}

const AUDIO_LATENCY_TOLERANCE: usize = 4;
// const MAX_SPEED_ADJUSTMENT: f32 = 0.25;

impl AudioClient {
    pub fn new(sink: Sink) -> AudioClient {
        AudioClient {
            packet_constructor: PacketConstructor::new(),
            sink,
            // speed_adjustment_counter: 0.0
        }
    }

    /* Experimental */
    // pub fn handle_latency_by_speed_up(&mut self) {
    //     if self.sink.len() > AUDIO_LATENCY_TOLERANCE {
    //         println!("Correcting Latency by Speeding up Audio: {}", self.sink.len());

    //         self.speed_adjustment_counter += 1.0;
    //         let adjustment = MAX_SPEED_ADJUSTMENT - MAX_SPEED_ADJUSTMENT / self.speed_adjustment_counter;

    //         self.sink.set_speed(1.00 + adjustment);
    //     } else if self.sink.speed() != 1.0 {
    //         self.sink.set_speed(1.0);
    //         self.speed_adjustment_counter = 0.0;
    //     }
    // }

    pub fn handle_latency_by_dropping(&mut self) {
        if self.sink.len() == 0 {
            debug!("Sink Buffer at {}", self.sink.len());
            // self.sink.set_volume(0f32);
        } else {
            // self.sink.set_volume(1f32);
        }

        if self.sink.len() > AUDIO_LATENCY_TOLERANCE {
            // println!("Correcting Latency by Skipping: {}", self.sink.len());
            for _ in 0..AUDIO_LATENCY_TOLERANCE - 1 {
                // self.sink.skip_one();
            }
        }
    }

    #[inline]
    pub fn play_audio_stream(&mut self, buf: &[u8], number_of_bytes: usize) {
        let audio_stream = match self
            .packet_constructor
            .assemble_packet(buf, number_of_bytes)
        {
            Some(audio_stream) => audio_stream,
            None => return,
        };

        let f32_slice = unsafe {
            std::slice::from_raw_parts(
                audio_stream.as_ptr() as *const f32,
                audio_stream.len() / std::mem::size_of::<f32>(),
            )
        };

        let audio_buf = SamplesBuffer::new(2, 48000, f32_slice);

        self.handle_latency_by_dropping();
        self.sink.append(audio_buf);
    }
}
