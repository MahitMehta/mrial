use super::AudioController;
use mrial_proto::*;

use std::net::UdpSocket;
use pipewire as pw;
use pw::spa::WritableDict;
use pw::spa::format::{MediaType, MediaSubtype};
use pw::{properties, spa};
use spa::param::format_utils;
use spa::pod::Pod;
use std::mem;

struct UserData {
    format: spa::param::audio::AudioInfoRaw,
    cursor_move: bool,
}

struct Opt {
    target: Option<String>,
}

impl AudioController {
    pub fn new() -> AudioController {
        AudioController {

        }
    }

    pub fn begin_transmission(&self, socket: UdpSocket, src: std::net::SocketAddr) {
        std::thread::spawn(move || {
            pw::init();

            let mainloop = pw::MainLoop::new().unwrap();
            let context = pw::Context::new(&mainloop).unwrap();

            // run if error  
            // 1. systemctl --user restart pipewire.service
            // 2. "systemctl --user restart pipewire-pulse.service" 
            // 3. pactl load-module module-null-sink media.class=Audio/Sink sink_name=mrial_sink channel_map=stereo
            let core = context.connect(None).unwrap();

            let data = UserData {
                format: Default::default(),
                cursor_move: false,
            };

            /* Create a simple stream, the simple stream manages the core and remote
            * objects for you if you don't need to deal with them.
            *
            * If you plan to autoconnect your stream, you need to provide at least
            * media, category and role properties.
            *
            * Pass your events and a user_data pointer as the last arguments. This
            * will inform you about the stream state. The most important event
            * you need to listen to is the process event where you need to produce
            * the data.
            */
            #[cfg(not(feature = "v0_3_44"))]
            let mut props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Music",
            };
            // uncomment if you want to capture from the sink monitor ports
            props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");

            let stream = pw::stream::Stream::new(&core, "audio-capture", props).unwrap();

            let _listener = stream
                .add_local_listener_with_user_data(data)
                .param_changed(move |_, id, user_data, param| {
                    // NULL means to clear the format
                
                    let Some(param) = param else {
                        return;
                    };
                    // println!("Reached Here: {}", id);
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
                        let datas = buffer.datas_mut();
                        if datas.is_empty() {
                            return;
                        }

                        let data = &mut datas[0];
                    
                        let n_channels = user_data.format.channels();
                        let n_samples = data.chunk().size() / (mem::size_of::<f32>() as u32);

                        if let Some(samples) = data.data() {
                            if samples[0] == 0 && samples[1] == 0 {
                                if samples[2] != 0 {
                                    println!("Next: {}", samples[2]);
                                } 
                                return; 
                            } // is this true? if first and second byte is 0 then no audio is playing...
                            let buf = &samples[0..(n_samples * n_channels * 2) as usize]; 
                            let packets = (buf.len() as f64 / 1024 as f64).ceil() as usize;

                            for i in 0..buf.len()/1024 {
                                let mut buf2 = [0u8; 1032];
                                buf2[0] = EPacketType::AUDIO as u8; 
                                buf2[1..3].copy_from_slice(&((packets - i - 1) as u16).to_be_bytes());
                                buf2[3..7].copy_from_slice(&(buf.len() as i32).to_be_bytes());

                                let start = i * 1024;
                                let addition = if start + 1024 <= buf.len() { 1024 } else { buf.len() - start };
                                buf2[8..1032].copy_from_slice(&buf[start..start+addition]);
                                socket.send_to(&buf2, src).unwrap();// pass src in the future 
                            }

                            
                            //println!("Number of channels: {}", n_samples);
                            //println!("captured {:?}", &samples[(n_samples * n_channels * 2) as usize]);
                        }
                    }
                })
                .register().unwrap();

            /* Make one parameter with the supported formats. The SPA_PARAM_EnumFormat
            * id means that this is a format enumeration (of 1 value).
            * We leave the channels and rate empty to accept the native graph
            * rate and channels. */
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
            )
            .unwrap()
            .0
            .into_inner();

            let mut params = [Pod::from_bytes(&values).unwrap()];

            /* Now connect this stream. We ask that our process function is
            * called in a realtime thread. */
                    
            stream.connect(
                spa::Direction::Input,
                None,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut params,
            ).unwrap();



            // and wait while we let things run
            mainloop.run();
        });
    }
}