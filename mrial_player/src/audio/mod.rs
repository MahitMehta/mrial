use std::{sync::{Arc, RwLock}, thread::JoinHandle};

use kanal::{Receiver, SendError, Sender};
use log::debug;
use mrial_proto::*;
use rodio::{buffer::SamplesBuffer, Sink};

use crate::client::Client;

pub struct AudioClientThread {
    packet_constructor: PacketConstructor,
    sink: Arc<RwLock<Sink>>,
    audio_sender: Sender<Vec<u8>>,
    // speed_adjustment_counter: f32,
}

const AUDIO_LATENCY_TOLERANCE: usize = 4;
// const MAX_SPEED_ADJUSTMENT: f32 = 0.25;

impl AudioClientThread {
    pub fn new(sink: Sink, audio_sender: Sender<Vec<u8>>) -> AudioClientThread {
        AudioClientThread {
            packet_constructor: PacketConstructor::new(),
            sink: Arc::new(RwLock::new(sink)), // speed_adjustment_counter: 0.0
            audio_sender
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        debug!("Setting Volume to {}%", volume * 100.0);

        if let Ok(sink) = self.sink.read() {
            sink.set_volume(volume * volume);
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

    pub fn run(&self, audio_receiver: Receiver<Vec<u8>>, client: Client) -> Result<JoinHandle<()>, Box<dyn std::error::Error>> {
        let sink = self.sink.clone();

        let handle = std::thread::spawn(move || {
            while let Ok(encrypted_audio) = audio_receiver.recv() {
                let audio_packet = match AudioClientThread::decrypt_audio(&encrypted_audio, &client) {
                    Ok(audio_packet) => audio_packet,
                    Err(e) => {
                        debug!("Failed to decrypt audio: {}", e);
                        continue;
                    }
                };

                let f32_audio_slice = unsafe {
                    std::slice::from_raw_parts(
                        audio_packet.as_ptr() as *const f32,
                        audio_packet.len() / std::mem::size_of::<f32>(),
                    )
                };
        
                let samples = SamplesBuffer::new(2, 48000, f32_audio_slice);
        
                if let Ok(sink) = sink.read() {
                    sink.append(samples);
                }
            }
        });

        Ok(handle)
    }

    pub fn handle_latency_by_dropping(&mut self) {
        if let Ok(sink) = self.sink.read() {
            if sink.len() == 0 {
                debug!("Sink Buffer at {}", sink.len());
                // self.sink.set_volume(0f32);
            } else {
                // self.sink.set_volume(1f32);
            }
    
            if sink.len() > AUDIO_LATENCY_TOLERANCE {
                // println!("Correcting Latency by Skipping: {}", self.sink.len());
                for _ in 0..AUDIO_LATENCY_TOLERANCE - 1 {
                    // self.sink.skip_one();
                }
            }
        }
    }

    #[inline]
    fn decrypt_audio(
        encrypted_audio:
        &Vec<u8>, 
        client: &Client
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if let Ok(sym_key) = client.get_sym_key().read() {
            if let Some(sym_key) = sym_key.as_ref() {
                match decrypt_frame(sym_key, &encrypted_audio) {
                    Some(audio_packet) => return Ok(audio_packet),
                    None => return Err("Failed to decrypt audio frame".into()),
                };
            }
        }

        Err("Failed to get symmetric key".into())
    }

    #[inline]
    pub fn packet(
        &mut self, 
        buf: &[u8], 
        number_of_bytes: usize,
    ) -> Result<(), SendError> {
        let encrypted_audio = match self
            .packet_constructor
            .assemble_packet(buf, number_of_bytes)
        {
            Some(encrypted_audio) => encrypted_audio,
            None => return Ok(()),
        };

        
        self.handle_latency_by_dropping();

        self.audio_sender.send(encrypted_audio)?;

        Ok(())
    }
}
