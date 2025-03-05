use super::{AudioEncoder, AudioServerAction, AudioServerThread, IAudioStream};
use mrial_proto::deploy::PacketDeployer;
use mrial_proto::*;

use log::debug;

use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;
#[cfg(feature = "v0_3_44")]
use spa::WritableDict;
use std::mem;

struct UserData {
    format: spa::param::audio::AudioInfoRaw,
}

impl AudioServerThread {
    fn is_zero(buf: &[u8]) -> bool {
        let (prefix, aligned, suffix) = unsafe { buf.align_to::<u128>() };

        prefix.iter().all(|&x| x == 0)
            && suffix.iter().all(|&x| x == 0)
            && aligned.iter().all(|&x| x == 0)
    }
}

impl IAudioStream for AudioServerThread {
    fn stream(&self) -> Result<(), Box<dyn std::error::Error>> {
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

        #[cfg(not(feature = "v0_3_44"))]
        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
        };
        #[cfg(feature = "v0_3_44")]
        let mut props = {
            let opt = Opt::parse();

            let mut props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Music",
            };
            if let Some(target) = opt.target {
                props.insert(*pw::keys::TARGET_OBJECT, target);
            }
            props
        };

        // uncomment if you want to capture from the sink monitor ports
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");

        let stream = pw::stream::Stream::new(&core, "audio-capture", props)?;
        
        let _encoder = AudioEncoder::new(2, 16, 48000);

        let mut deployer = PacketDeployer::new(EPacketType::Audio, false);
        let conn = self.conn.try_clone()?;
        let receiver = self.receiver.clone();
        let handle = self.tokio_handle.clone();

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
            .process(move |stream, user_data| match stream.dequeue_buffer() {
                None => println!("out of buffers"),
                Some(mut buffer) => {
                    while let Ok(Some(action)) = receiver.try_recv() {
                        match action {
                            AudioServerAction::SymKey => {
                                if let Ok(app) = conn.get_app() {
                                    if let Some(key) = app.get_sym_key() {
                                        deployer.set_sym_key(key);
                                    }
                                }
                            }
                        }
                    }

                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }

                    let data = &mut datas[0];

                    let n_channels = user_data.format.channels();
                    let n_samples = data.chunk().size() / (mem::size_of::<f32>() as u32);

                    if let Some(samples) = data.data() {
                        // TODO: find a better solution to detect if audio is not playings
                        if AudioServerThread::is_zero(&samples[0..32]) {
                            if samples[32] != 0 {
                                println!("Next: {}", samples[32]);
                            }
                            return;
                        }

                        let sample: &[u8] = &samples[0..(n_samples * n_channels * 2) as usize];

                        if conn.has_app_clients() && deployer.has_sym_key() {
                            deployer.prepare_encrypted(
                                &sample,
                                Box::new(|subpacket| {
                                    conn.app_broadcast_audio(subpacket);
                                }),
                            );
                        }

                        if conn.has_web_clients_blocking() {
                            deployer.prepare_unencrypted(
                                &sample,
                                Box::new(|subpacket| {
                                    if let Err(e) = conn.web_broadcast(subpacket) {
                                        
                                    }
                                }),
                            );
                        }
                    }
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

        stream
            .connect(
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
