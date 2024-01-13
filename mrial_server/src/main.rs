// dependencies: libxcb-randr0-dev

mod audio;
mod conn;
mod events;
mod video;

use kanal::unbounded;
use mrial_proto::*;
use video::yuv::YUVBuffer;

use std::{
    collections::VecDeque,
    io::Write,
    time::{Duration, Instant},
};

use audio::AudioController;
use futures::{executor::ThreadPool, future::RemoteHandle, task::SpawnExt};
use scrap::{Capturer, Display};
use spin_sleep;
use x264::{Param, Picture};

use crate::{audio::IAudioController, conn::Connection, events::EventsThread, video::yuv::EColorSpace};

#[derive(PartialEq)]
pub enum ServerActions {
    Inactive,
}

#[allow(dead_code)]
fn write_stream(bitstream: &[u8], file: &mut std::fs::File) {
    file.write(bitstream).unwrap();
}

#[tokio::main]
async fn main() {
    use std::io::ErrorKind::WouldBlock;

    let server_channel = unbounded::<ServerActions>();
    let mut conn = Connection::new();
    let mut buf = [0u8; MTU];

    let display: Display = Display::primary().unwrap();
    let mut capturer = Capturer::new(display).unwrap();

    let width = capturer.width();
    let height = capturer.height();

    let mut par = Param::default_preset("ultrafast", "zerolatency").unwrap();

    par = par.set_csp(EColorSpace::YUV444.into());
    par = par.set_dimension(height, width);
    if cfg!(target_os = "windows") { par = par.set_fullrange(1); }

    par = par.param_parse("repeat_headers", "1").unwrap();
    par = par.param_parse("annexb", "1").unwrap();
    par = par.param_parse("bframes", "0").unwrap();
    par = par.param_parse("crf", "20").unwrap();
    par = par.apply_profile("high444").unwrap();

    let mut pic = Picture::from_param(&par).unwrap();
    let mut encoder = x264::Encoder::open(&mut par).unwrap();

    let headers = encoder.get_headers().unwrap();

    let pool = ThreadPool::builder().pool_size(1).create().unwrap();
    let mut yuv_handles: VecDeque<RemoteHandle<YUVBuffer>> = VecDeque::new();

    let rowlen: usize = 4 * width * height;

    let mut frames = 0u8;
    let mut fps_time = Instant::now();

    let audio_controller = AudioController::new();
    audio_controller.begin_transmission(conn.clone());

    let events = EventsThread::new();
    events.run(
        &mut conn,
        headers.as_bytes().to_vec(),
        server_channel.0.clone(),
    );

    let mut frame_count = 1;
    let mut packet_id = 1;

    loop {
        if server_channel.1.len() > 0 {
            match server_channel.1.try_recv_realtime().unwrap() {
                Some(ServerActions::Inactive) => {
                    encoder = x264::Encoder::open(&mut par).unwrap();
                }
                None => {}
            }
        }

        if !conn.has_clients() {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }

        let sleep = Instant::now();
        match capturer.frame() {
            Ok(frame) => {
                let bgra_frame = frame.chunks(rowlen).next().unwrap().to_vec();

                let cvt_rgb_yuv = async move {
                    let yuv = YUVBuffer::with_bgra_for_444(width, height, &bgra_frame);
                    yuv
                };
                yuv_handles.push_back(pool.spawn_with_handle(cvt_rgb_yuv).unwrap());

                // set to 1 to increase FPS at the cost of latency, or 0  for the opposite effect
                if yuv_handles.len() > 0 {
                    //let start = Instant::now();
                    let yuv = yuv_handles.pop_front().unwrap().await;

                    let y_plane = pic.as_mut_slice(0).unwrap();
                    y_plane.copy_from_slice(yuv.y());
                    let u_plane = pic.as_mut_slice(1).unwrap();
                    u_plane.copy_from_slice(yuv.u_444());
                    let v_plane = pic.as_mut_slice(2).unwrap();
                    v_plane.copy_from_slice(yuv.v_444());

                    pic = pic.set_timestamp(frame_count);
                    frame_count += 1;

                    if let Some((nal, _, _)) = encoder.encode(&pic).unwrap() {
                        let bitstream = nal.as_bytes();

                        let packets = (bitstream.len() as f64 / PAYLOAD as f64).ceil() as usize;

                        write_static_header(
                            EPacketType::NAL,
                            bitstream.len().try_into().unwrap(),
                            packet_id,
                            &mut buf,
                        );

                        packet_id += 1;

                        for i in 0..packets {
                            write_packets_remaining(
                                (packets - i - 1).try_into().unwrap(),
                                &mut buf,
                            );

                            let start = i * PAYLOAD;
                            let addition = if start + PAYLOAD <= bitstream.len() {
                                PAYLOAD
                            } else {
                                bitstream.len() - start
                            };
                            buf[HEADER..addition + HEADER]
                                .copy_from_slice(&bitstream[start..(addition + start)]);

                            conn.broadcast(&buf[0..addition + HEADER]);
                        }
                        frames += 1;
                    }

                    if fps_time.elapsed().as_millis() > 1000 && frames > 0 {
                        conn.filter_clients();
                        println!(
                            "FPS: {}",
                            frames as f32 / fps_time.elapsed().as_secs() as f32
                        );
                        frames = 0;
                        fps_time = Instant::now();
                    }

                    if sleep.elapsed().as_millis() > 0 && sleep.elapsed().as_millis() < 16 {
                        let delay = 16 - sleep.elapsed().as_millis();
                        spin_sleep::sleep(Duration::from_millis(delay as u64));
                    }
                }
            }
            Err(ref e) if e.kind() == WouldBlock => {}
            Err(_) => {
                println!("Error Capturing Frame")
            }
        }
    }
}
