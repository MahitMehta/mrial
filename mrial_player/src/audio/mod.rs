use std::{
    sync::{Arc, RwLock},
    thread::JoinHandle,
};

use kanal::{Receiver, SendError, Sender};
use log::debug;
use mrial_proto::*;
use opus::{Channels, Decoder};
use rodio::{buffer::SamplesBuffer, Sink};

use crate::client::Client;

pub type AudioPacket = (EPacketType, Vec<u8>);

const AUDIO_LATENCY_TOLERANCE: usize = 5; // ~max (0.04 * 5) = 200ms
const SAMPLE_RATE: u32 = 48000;
const CHANNELS: u16 = 2;
const OPUS_MAX_FRAME_SIZE: usize = 1920;

pub struct AudioClientThread {
    packet_constructor: PacketConstructor,
    sink: Arc<RwLock<Sink>>,
    audio_sender: Sender<AudioPacket>,
}

impl AudioClientThread {
    pub fn new(sink: Sink, audio_sender: Sender<AudioPacket>) -> AudioClientThread {
        AudioClientThread {
            packet_constructor: PacketConstructor::new(),
            sink: Arc::new(RwLock::new(sink)),
            audio_sender,
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        debug!("Setting Volume to {}%", volume * 100.0);

        if let Ok(sink) = self.sink.read() {
            sink.set_volume(volume * volume);
        }
    }

    pub fn run(
        &self,
        audio_receiver: Receiver<AudioPacket>,
        client: Client,
    ) -> Result<JoinHandle<()>, Box<dyn std::error::Error>> {
        let sink = self.sink.clone();

        let handle = std::thread::spawn(move || {
            let mut uncompressed_audio_buf = [0f32; OPUS_MAX_FRAME_SIZE * CHANNELS as usize];
            let opus_channels = match CHANNELS {
                1 => Channels::Mono,
                2 => Channels::Stereo,
                _ => unreachable!(),
            };

            let mut opus_decoder = Decoder::new(SAMPLE_RATE, opus_channels).unwrap();

            while let Ok((packet_type, encrypted_audio)) = audio_receiver.recv() {
                let audio_packet = match AudioClientThread::decrypt_audio(&encrypted_audio, &client)
                {
                    Ok(audio_packet) => audio_packet,
                    Err(e) => {
                        debug!("Failed to decrypt audio: {}", e);
                        continue;
                    }
                };

                match packet_type {
                    EPacketType::Audio(EAudioType::PCM) => {
                        let f32_audio_slice = unsafe {
                            std::slice::from_raw_parts(
                                audio_packet.as_ptr() as *const f32,
                                audio_packet.len() / std::mem::size_of::<f32>(),
                            )
                        };

                        let samples = SamplesBuffer::new(CHANNELS, SAMPLE_RATE, f32_audio_slice);

                        if let Ok(sink) = sink.read() {
                            sink.append(samples);
                        }
                    }
                    EPacketType::Audio(EAudioType::Opus) => {
                        let uncompressed_len = match opus_decoder.decode_float(
                            &audio_packet,
                            &mut uncompressed_audio_buf,
                            false,
                        ) {
                            Ok(audio_frame) => audio_frame,
                            Err(e) => {
                                debug!("Failed to decode audio: {}", e);
                                continue;
                            }
                        };

                        let samples = SamplesBuffer::new(
                            CHANNELS,
                            SAMPLE_RATE,
                            &uncompressed_audio_buf[..uncompressed_len * CHANNELS as usize],
                        );

                        if let Ok(sink) = sink.read() {
                            sink.append(samples);
                        }
                    }
                    _ => unreachable!(),
                }
            }
        });

        Ok(handle)
    }

    pub fn handle_latency_by_dropping(&mut self) {
        if let Ok(sink) = self.sink.read() {
            if sink.len() > AUDIO_LATENCY_TOLERANCE {
                debug!("Correcting Latency by Dropping Audio Packet");
                sink.skip_one();
                sink.skip_one();
                return;
            }

            if sink.len() == 0 {
                debug!("Sink Buffer at {}", sink.len());
            }
        }
    }

    #[inline]
    fn decrypt_audio(
        encrypted_audio: &Vec<u8>,
        client: &Client,
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
        packet_type: EPacketType,
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
        self.audio_sender.send((packet_type, encrypted_audio))?;

        Ok(())
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
}
