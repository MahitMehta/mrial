// had to install libxcb-randr0-dev

mod audio;
mod events;
mod video; 
mod conn; 

use video::yuv::YUVBuffer;
use mrial_proto::{*, input::{click_requested, parse_click, mouse_move_requested}};

use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings
};
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

use crate::conn::Connections;

#[tokio::main]
async fn main() {
    use std::io::ErrorKind::WouldBlock;

    let socket: UdpSocket =
        UdpSocket::bind("0.0.0.0:8554").expect("Failed to Bind UdpSocket to Port");

    let conn = Connections::new(socket.try_clone().unwrap());
    let mut buf: [u8; MTU] = [0; MTU];

    // let Ok((_len, src)) = socket.recv_from(&mut buf) else {
    //     panic!("Failed!");
    // };

    // if buf[0] != EPacketType::SHAKE as u8 {
    //     panic!("Invalid Handsake");
    // }

    // write_header(EPacketType::SHOOK, 0, HEADER as u32, 0, &mut buf);
    // socket
    //     .send_to(&buf[0..HEADER], src)
    //     .expect("Failed to send SHOOK");

    let display: Display = Display::primary().unwrap();
    let mut capturer = Capturer::new(display).unwrap();

    const W: usize = 1440;
    const H: usize = 900;


    let mut par = Param::default_preset("ultrafast", "zerolatency").unwrap();
    par = par.param_parse("repeat_headers", "1").unwrap();
    par = par.set_csp(12); // 12 = 444, 7 = 422
    par = par.set_dimension(H, W);
    // par = par.set_fullrange(1); // not needed for 444
    par = par.param_parse("annexb", "1").unwrap();
    par = par.param_parse("bframes", "0").unwrap();
    par = par.param_parse("crf", "20").unwrap();
    par = par.apply_profile("high444").unwrap(); // high444

    let mut pic = Picture::from_param(&par).unwrap();
    let mut enc = x264::Encoder::open(&mut par).unwrap();

    //let headers = enc.headers().unwrap();
    //while mrial_proto::assembled_packet(packet, buf, number_of_bytes, packets_remaining)

    let pool = ThreadPool::builder().pool_size(1).create().unwrap();
    let mut yuv_handles: VecDeque<RemoteHandle<YUVBuffer>> = VecDeque::new();

    let rowlen: usize = 4 * W * H;

    let mut frames = 0u8;
    let mut fps = Instant::now();

    let audio_controller = AudioController::new();
    audio_controller.begin_transmission(conn.clone());

    // let attempt_reconnect = Arc::new(Mutex::new(false));

    // let attempt_reconnect_clone = Arc::clone(&attempt_reconnect);
    let socket_clone = socket.try_clone().unwrap();
    let mut conn_clone = conn.clone();
    let _state = thread::spawn(move || {
        let mouse = mouse_rs::Mouse::new(); // requires package install on linux (libxdo-dev)
        let mut enigo = Enigo::new(&Settings::default()).unwrap();
        let mut event_emitter = events::EventEmitter::new();

        loop {
            let mut buf: [u8; MTU] = [0; MTU];
            let (_size, src) = socket_clone.recv_from(&mut buf).unwrap();
            let packet_type = parse_packet_type(&buf);

            match packet_type {
                EPacketType::SHAKE => {
                    // *attempt_reconnect_clone.lock().unwrap() = true;
                    conn_clone.add_client(src);
                }
                EPacketType::DISCONNECT => {
                    conn_clone.remove_client(src);
                }
                EPacketType::STATE => {
                    if click_requested(&buf) {
                        let (x, y, right) = parse_click(&mut buf, W, H);
                        let _ = mouse.move_to(x, y);
                        if right {
                            let _ = mouse.click(&mouse_rs::types::keys::Keys::RIGHT);
                        } else {
                            let _ = mouse.click(&mouse_rs::types::keys::Keys::LEFT);
                        }                        
                    }
                    if mouse_move_requested(&buf) {
                        let x_percent =
                            u16::from_be_bytes(buf[HEADER + 10..HEADER + 12].try_into().unwrap()) - 1;
                        let y_percent =
                            u16::from_be_bytes(buf[HEADER + 12..HEADER + 14].try_into().unwrap()) - 1;

                        let x: i32 = (x_percent as f32 / 10000.0 * W as f32).round() as i32;
                        let y = (y_percent as f32 / 10000.0 * H as f32).round() as i32;

                        let _ = mouse.move_to(x, y);

                        // TODO: handle right mouse button too
                        if buf[HEADER + 14] == 1 {
                            let _ = mouse.press(&mouse_rs::types::keys::Keys::LEFT);
                        } 
                    }
                    if buf[HEADER + 15] != 0 || buf[HEADER + 17] != 0 {
                        let x_delta = i16::from_be_bytes(buf[HEADER + 14..HEADER + 16].try_into().unwrap());
                        let y_delta = i16::from_be_bytes(buf[HEADER + 16..HEADER + 18].try_into().unwrap());

                        if cfg!(target_os = "linux") {
                            event_emitter.scroll(x_delta as i32, y_delta as i32);
                        }
                        //enigo.scroll((-x_delta).into(), Vertical).unwrap();
                        //enigo.scroll((-y_delta).into(), Horizontal).unwrap();
                    }

                    if buf[HEADER] == 1 {
                        enigo.key(Key::Control, Press).unwrap();
                    } else if buf[HEADER] == 2 {
                        enigo.key(Key::Control, Release).unwrap();
                    }
                    if buf[HEADER + 1] == 1 {
                        enigo.key(Key::Shift, Press).unwrap();
                    } else if buf[HEADER + 1] == 2 {
                        enigo.key(Key::Shift, Release).unwrap();
                    }

                    if buf[HEADER + 2] == 1 {
                        enigo.key(Key::Alt, Press).unwrap();
                    } else if buf[HEADER + 2] == 2 {
                        enigo.key(Key::Alt, Release).unwrap();
                    }

                    if buf[HEADER + 3] == 1 {
                        enigo.key(Key::Meta, Press).unwrap();
                    } else if buf[HEADER + 3] == 2 {
                        enigo.key(Key::Meta, Release).unwrap();
                    }

                    if buf[HEADER + 8] != 0 {
                        if buf[HEADER + 8] == 32 {
                            enigo.key(Key::Space, enigo::Direction::Click).unwrap();
                        } else if buf[HEADER + 8] == 8 {
                            enigo.key(Key::Backspace, Press).unwrap();
                        } else if buf[HEADER + 8] == 10 {
                            enigo.key(Key::Return, enigo::Direction::Click).unwrap();
                        } else if buf[HEADER + 8] >= 33 {
                            // add ascii range check

                            enigo
                                .key(Key::Unicode((buf[HEADER + 8]) as char), Press)
                                .unwrap();
                        }
                    }

                    if buf[HEADER + 9] != 0 {
                        if buf[HEADER + 9] == 32 {
                            enigo.key(Key::Space, Release).unwrap();
                        } else if buf[HEADER + 9] == 8 {
                            enigo.key(Key::Backspace, Release).unwrap();
                        } else if buf[HEADER + 9] >= 33 {
                            // add ascii range check
                            enigo
                                .key(Key::Unicode((buf[HEADER + 9]) as char), Release)
                                .unwrap();
                        }
                    }
                }
                _ => {}
            }            
        }
    });

    // let mut file = std::fs::File::create("recording.h264").unwrap();

    let mut frame_count = 1;
    let mut packet_id = 0;

    loop {
        if !conn.has_clients() {
            std::thread::sleep(Duration::from_millis(1000));
            println!("Awaiting Client Connection");
            continue
        }

        let sleep = Instant::now();
        match capturer.frame() {
            Ok(frame) => {
                let data = frame.chunks(rowlen).next().unwrap().to_vec();

                let cvt_rgb_yuv = async move {
                    let yuv = YUVBuffer::with_bgra_for_444(W, H, &data);
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

                    // replace possibly with spin-sleep: https://github.com/alexheretic/spin-sleep
                    if sleep.elapsed().as_millis() > 0 && sleep.elapsed().as_millis() < 16 {
                        let delay = 16 - sleep.elapsed().as_millis();
                        
                        //std::thread::sleep(Duration::from_millis(delay as u64));
                        spin_sleep::sleep(Duration::from_millis(delay as u64));
                    }

                    if fps.elapsed().as_millis() > 1000 && frames > 0 {
                        println!("FPS: {}", frames as f32 / fps.elapsed().as_secs() as f32);
                        frames = 0;
                        fps = Instant::now();
                    }

                    // if *attempt_reconnect.lock().unwrap() {
                    //     println!("Reconnecting...");
                    //     enc = x264::Encoder::open(&mut par).unwrap();
                    //     buf[0] = EPacketType::SHOOK as u8;
                    //     socket
                    //         .send_to(&buf[0..HEADER], src)
                    //         .expect("Failed to send NAL Unit");
                    //     *attempt_reconnect.lock().unwrap() = false;
                    // }
                }
            }
            Err(ref e) if e.kind() == WouldBlock => {
                // Wait for the frame.
            }
            Err(_) => {
                // We're done here.
                break;
            }
        }
    }
}
