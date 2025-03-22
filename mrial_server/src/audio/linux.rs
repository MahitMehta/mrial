use crate::conn::BroadcastTaskError;

use super::{AudioServerTask, IAudioStream, OpusEncoder, ENCODE_FRAME_SIZE};
use mrial_proto::*;

use log::{debug, error};

use opus::Decoder;
use pipewire::spa::param::audio::AudioInfoRaw;
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;
use std::io::Write;
use std::mem;

struct UserData {
    format: spa::param::audio::AudioInfoRaw,
}

impl AudioServerTask {
    fn is_zero(buf: &[u8]) -> bool {
        let (prefix, aligned, suffix) = unsafe { buf.align_to::<u128>() };

        prefix.iter().all(|&x| x == 0)
            && suffix.iter().all(|&x| x == 0)
            && aligned.iter().all(|&x| x == 0)
    }
}

const CHANNELS: usize = 2;

impl IAudioStream for AudioServerTask {
    async fn stream(&self) -> Result<(), Box<dyn std::error::Error>> {
        pw::init();

        let mainloop = pw::main_loop::MainLoop::new(None)?;
        let context = pw::context::Context::new(&mainloop)?;

        let core = match context.connect(None) {
            Ok(core) => core,
            Err(e) => {
                debug!("Failed to connect to PipeWire Context: {}", e);
                // TODO: Should attempt to reconnect
                return Ok(());
            }
        };

        let data = UserData {
            format: Default::default(),
        };

        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
        };

        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
        props.insert(*pw::keys::NODE_LATENCY, "1024/48000");

        let stream = pw::stream::Stream::new(&core, "audio-capture", props)?;

        let mut opus_encoder = OpusEncoder::new(48000, CHANNELS)?;
        let mut compressed_audio = [0u8; ENCODE_FRAME_SIZE * CHANNELS];
        // let mut compressed_file = std::fs::File::create("compressed.pcm").unwrap();
        // let mut uncompressed_output = [0f32; ENCODE_FRAME_SIZE * CHANNELS];

        let conn = self.conn.clone();
        let receiver = self.receiver.clone();
        let handle = tokio::runtime::Handle::current();

        let _listener = stream
            .add_local_listener_with_user_data(data)
            .param_changed(move |_, user_data, id, param| {
                // NULL means to clear the format

                let Some(param) = param else {
                    return;
                };

                if id != pw::spa::param::ParamType::Format.as_raw() {
                    return;
                }

                let (media_type, media_subtype) = match format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                // only accept raw audio
                if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                    return;
                }

                // call a helper function to parse the format for us.
                user_data
                    .format
                    .parse(param)
                    .expect("Failed to parse param changed to AudioInfoRaw");

                println!(
                    "capturing rate:{} channels:{}",
                    user_data.format.rate(),
                    user_data.format.channels()
                );
            })
            .process(move |stream, _user_data| match stream.dequeue_buffer() {
                None => println!("out of buffers"),
                Some(mut buffer) => {
                    while let Ok(Some(action)) = receiver.try_recv() {
                        match action {
                        }
                    }

                    handle.block_on(async {
                        let datas = buffer.datas_mut();
                        if datas.is_empty() {
                            return;
                        }

                        let data = &mut datas[0];

                        // let n_channels = user_data.format.channels() as usize;
                        let chunk_len = data.chunk().size();
    
                        if let Some(samples) = data.data() {
                            // TODO: find a better solution to detect if audio is not playings
                            if AudioServerTask::is_zero(&samples[0..32]) {
                                if samples[32] != 0 {
                                    debug!("Next: {}", samples[32]);
                                }
                                return;
                            }

                            let sample: &[u8] = &samples[0..chunk_len as usize];

                            if conn.has_app_clients().await {
                                match opus_encoder.encode_f32(&sample, &mut compressed_audio) {
                                    Ok(Some(compressed_len)) => {
                                        if let Err(e) = conn.app_encrypted_broadcast(
                                            EPacketType::AudioOpus, &compressed_audio[..compressed_len]).await {
                                            match e {
                                                BroadcastTaskError::TaskNotRunning => {
                                                    error!("App Broadcast Task Not Running");
                                                    conn.get_app().start_broadcast_async_task();
                                                }
                                                BroadcastTaskError::TransferFailed(msg) => {
                                                    error!("App Broadcast Send Error: {msg}");
                                                }
                                                BroadcastTaskError::EncryptionFailed(msg) => {
                                                    error!("App Broadcast Encryption Error: {msg}");
                                                }
                                            }
                                        }
                                    }
                                    Err(e) =>  error!("Failed to encode audio: {}", e),
                                    _ => {}
                                }
                            }

                            if conn.has_web_clients().await {
                                if let Err(e) = conn.web_encrypted_broadcast(
                                    EPacketType::AudioPCM, &sample) {
                                    match e {
                                        BroadcastTaskError::TaskNotRunning => {
                                            error!("Web Broadcast Task Not Running");
                                         
                                            debug!("Restarting Web Broadcast Task");
                                            conn.get_web().start_broadcast_async_task();
                                        }
                                        BroadcastTaskError::TransferFailed(msg) => {
                                            error!("Web Broadcast Send Error: {msg}");
                                        }
                                        _ => {}
                                    }
                                }

                            //     match opus_encoder.encode_f32(&sample, &mut compressed_audio) {
                            //         Ok(Some(compressed_len)) => {
                            //             match opus_decoder.decode_float(&compressed_audio[..compressed_len], &mut uncompressed_output, false) {
                            //                 Ok(len) => {
                            //                     println!("Compressed Len: {}", compressed_len);
                            //                     compressed_file.write_all(unsafe { 
                            //                         std::slice::from_raw_parts(
                            //                         uncompressed_output[0..len * CHANNELS].as_ptr() as *const u8,
                            //                         CHANNELS * mem::size_of::<f32>() * len) 
                            //                     }).unwrap();
                            //                 }
                            //                 Err(e) => error!("Failed to decode audio: {}", e)
                            //             }

                            //             if let Err(e) = conn.web_encrypted_broadcast(
                            //                 EPacketType::Audio, &compressed_audio[..compressed_len]) {
                            //                 match e {
                            //                     BroadcastTaskError::TaskNotRunning => {
                            //                         error!("Web Broadcast Task Not Running");
                                                 
                            //                         debug!("Restarting Web Broadcast Task");
                            //                         conn.get_web().start_broadcast_async_task();
                            //                     }
                            //                     BroadcastTaskError::TransferFailed(msg) => {
                            //                         error!("Web Broadcast Send Error: {msg}");
                            //                     }
                            //                     _ => {}
                            //                 }
                            //             }
                            //         }
                            //         Err(e) =>  error!("Failed to encode audio: {}", e),
                            //         _ => {}
                            //     }
                            }
                        }
                    });
                }
            })
            .register()?;

        let mut audio_info = spa::param::audio::AudioInfoRaw::new();
        audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
        let obj = pw::spa::pod::Object {
            type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: pw::spa::param::ParamType::EnumFormat.as_raw(),
            properties: audio_info.into(),
        };
        let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &pw::spa::pod::Value::Object(obj),
        )?
        .0
        .into_inner();

        let mut params = [Pod::from_bytes(&values).unwrap()];

        stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )?;

        mainloop.run();

        Ok(())
    }
}
