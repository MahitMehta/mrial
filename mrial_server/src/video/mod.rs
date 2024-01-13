pub mod yuv;

use futures::{executor::ThreadPool, future::RemoteHandle, task::SpawnExt};
use kanal::unbounded;
use mrial_proto::*;
use scrap::{Capturer, Display};
use spin_sleep;
use std::{
    collections::VecDeque,
    fs::File,
    io::{ErrorKind::WouldBlock, Write},
    time::{Duration, Instant},
};
use x264::{Param, Picture};
use yuv::YUVBuffer;

use crate::{conn::Connection, events::EventsThread};

use self::yuv::EColorSpace;

#[derive(PartialEq)]
pub enum VideoServerActions {
    Inactive,
}

pub struct VideoServerThread {
    file: Option<File>,

    frame_count: i64,
    packet_id: u8,
    buf: [u8; MTU],
}

impl VideoServerThread {
    pub fn new() -> Self {
        Self {
            file: None,
            frame_count: 0,
            packet_id: 1,
            buf: [0u8; MTU],
        }
    }

    #[inline]
    #[allow(dead_code)]
    fn write_stream(&mut self, bitstream: &[u8]) {
        if let Some(file) = &mut self.file {
            file.write(bitstream).unwrap();
        }
    }

    #[inline]
    pub async fn run(&mut self, conn: &mut Connection) {
        let (ch_sender, ch_receiver) = unbounded::<VideoServerActions>();

        let display: Display = Display::primary().unwrap();
        let mut capturer = Capturer::new(display).unwrap();

        let width = capturer.width();
        let height = capturer.height();

        let mut par = Param::default_preset("ultrafast", "zerolatency").unwrap();

        par = par.set_csp(EColorSpace::YUV444.into());
        par = par.set_dimension(height, width);
        if cfg!(target_os = "windows") {
            par = par.set_fullrange(1);
        }

        par = par.param_parse("repeat_headers", "1").unwrap();
        par = par.param_parse("annexb", "1").unwrap();
        par = par.param_parse("bframes", "0").unwrap();
        par = par.param_parse("crf", "20").unwrap();
        par = par.apply_profile("high444").unwrap();

        let mut pic = Picture::from_param(&par).unwrap();
        let mut encoder = x264::Encoder::open(&mut par).unwrap();

        let pool = ThreadPool::builder().pool_size(1).create().unwrap();
        let mut yuv_handles: VecDeque<RemoteHandle<YUVBuffer>> = VecDeque::new();

        let rowlen: usize = 4 * width * height;

        let mut frames = 0u8;
        let mut fps_time = Instant::now();

        let headers = encoder.get_headers().unwrap();

        let events = EventsThread::new();
        events.run(conn, headers.as_bytes().to_vec(), ch_sender.clone());

        loop {
            if ch_receiver.len() > 0 {
                match ch_receiver.try_recv_realtime().unwrap() {
                    Some(VideoServerActions::Inactive) => {
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

                        pic = pic.set_timestamp(self.frame_count);
                        self.frame_count += 1;

                        if let Some((nal, _, _)) = encoder.encode(&pic).unwrap() {
                            let bitstream = nal.as_bytes();

                            let packets = (bitstream.len() as f64 / PAYLOAD as f64).ceil() as usize;

                            write_static_header(
                                EPacketType::NAL,
                                bitstream.len().try_into().unwrap(),
                                self.packet_id,
                                &mut self.buf,
                            );

                            self.packet_id += 1;

                            for i in 0..packets {
                                write_packets_remaining(
                                    (packets - i - 1).try_into().unwrap(),
                                    &mut self.buf,
                                );

                                let start = i * PAYLOAD;
                                let addition = if start + PAYLOAD <= bitstream.len() {
                                    PAYLOAD
                                } else {
                                    bitstream.len() - start
                                };
                                self.buf[HEADER..addition + HEADER]
                                    .copy_from_slice(&bitstream[start..(addition + start)]);

                                conn.broadcast(&self.buf[0..addition + HEADER]);
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
}
