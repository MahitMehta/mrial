// had to install libxcb-randr0-dev

mod audio;
mod events;
mod video; 
mod conn; 

use video::yuv::YUVBuffer;
use mrial_proto::*;


use std::{
    collections::VecDeque,
    net::UdpSocket,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant}, io::Write,
};

use spin_sleep;
use audio::AudioController;
use futures::{executor::ThreadPool, future::RemoteHandle, task::SpawnExt};
use scrap::{Capturer, Display};
use x264::{Param, Picture};

use crate::{conn::Connections, events::EventsThread, audio::IAudioController};

#[tokio::main]
async fn main() {
    use std::io::ErrorKind::WouldBlock;

    let socket: UdpSocket =
        UdpSocket::bind("0.0.0.0:8554").expect("Failed to Bind UdpSocket to Port");

    let mut conn = Connections::new(socket.try_clone().unwrap());
    let mut buf: [u8; MTU] = [0; MTU];

    let display: Display = Display::primary().unwrap();
    let mut capturer = Capturer::new(display).unwrap();

    let width = capturer.width();
    let height = capturer.height();

    let mut par = Param::default_preset("ultrafast", "zerolatency").unwrap();
    par = par.param_parse("repeat_headers", "1").unwrap();
    par = par.set_csp(12); // 12 = 444, 7 = 422
    par = par.set_dimension(height, width);
    // par = par.set_fullrange(1); // not needed for 444
    par = par.param_parse("annexb", "1").unwrap();
    par = par.param_parse("bframes", "0").unwrap();
    par = par.param_parse("crf", "20").unwrap();
    par = par.apply_profile("high444").unwrap(); // high444

    let mut pic = Picture::from_param(&par).unwrap();
    let mut enc = x264::Encoder::open(&mut par).unwrap();

    let headers = enc.get_headers().unwrap();
  
    let pool = ThreadPool::builder().pool_size(1).create().unwrap();
    let mut yuv_handles: VecDeque<RemoteHandle<YUVBuffer>> = VecDeque::new();

    let rowlen: usize = 4 * width * height;

    let mut frames = 0u8;
    let mut fps_time = Instant::now();

    let audio_controller = AudioController::new();
    audio_controller.begin_transmission(conn.clone());

    // let attempt_reconnect = Arc::new(Mutex::new(false));

    // let attempt_reconnect_clone = Arc::clone(&attempt_reconnect);

    let events = EventsThread::new();
    events.run(
        socket.try_clone().unwrap(), 
        &mut conn, 
        headers.as_bytes().to_vec()
    );
    // let mut file = std::fs::File::create("recording.h264").unwrap();

    let mut frame_count = 1;
    let mut packet_id = 1;

    loop {
        if !conn.has_clients() {
            std::thread::sleep(Duration::from_millis(250));
            continue
        }

        let sleep = Instant::now();
        match capturer.frame() {
            Ok(frame) => {
                let data = frame.chunks(rowlen).next().unwrap().to_vec();

                let cvt_rgb_yuv = async move {
                    let yuv = YUVBuffer::with_bgra_for_444(width, height, &data);
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

                    if let Some((nal, _, _)) = enc.encode(&pic).unwrap() {
                        let bitstream = nal.as_bytes();
                        // file.write(bitstream).unwrap();
                        // println!("Encoding Time: {}", start.elapsed().as_millis());
                         let packets =
                            (bitstream.to_vec().len() as f64 / PAYLOAD as f64).ceil() as usize;

                        write_static_header(
                            EPacketType::NAL, 
                            bitstream.to_vec().len().try_into().unwrap(), 
                            packet_id, 
                            &mut buf
                        );

                        packet_id += 1;

                        for i in 0..packets {
                            write_packets_remaining(
                                (packets - i - 1).try_into().unwrap(), 
                                &mut buf
                            );

                            let start = i * PAYLOAD;
                            let addition = if start + PAYLOAD <= bitstream.to_vec().len() {
                                PAYLOAD
                            } else {
                                bitstream.to_vec().len() - start
                            };
                            buf[HEADER..addition + HEADER]
                                .copy_from_slice(&bitstream.to_vec()[start..(addition + start)]);

                            conn.broadcast(&buf[0..addition + HEADER]);
                        }
                        frames += 1;
                    }

                    if fps_time.elapsed().as_millis() > 1000 && frames > 0 {
                        conn.filter_clients();
                        println!("FPS: {}", frames as f32 / fps_time.elapsed().as_secs() as f32);
                        frames = 0;
                        fps_time = Instant::now();
                    }

                    if sleep.elapsed().as_millis() > 0 && sleep.elapsed().as_millis() < 16 {
                        let delay = 16 - sleep.elapsed().as_millis();
                        spin_sleep::sleep(Duration::from_millis(delay as u64));
                    }
                }
            }
            Err(ref e) if e.kind() == WouldBlock => {
                
            }
            Err(_) => {
                println!("Error Capturing Frame.");
                break;
            }
        }
    }
}
