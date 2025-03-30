mod convert;

use std::{
    fs::File,
    io::{Error, ErrorKind, Write},
    thread,
};

use ffmpeg_next::frame;
use kanal::{unbounded, Receiver, Sender};

use log::debug;
use mrial_proto::*;

use super::slint_generatedMainWindow::BarialState;
use super::slint_generatedMainWindow::ControlPanelAdapter;

use slint::{ComponentHandle, Model};

use crate::client::Client;

pub struct VideoThread {
    pub channel: (Sender<Vec<u8>>, Receiver<Vec<u8>>),
    packet_constructor: NALPacketConstructor,
    clock: std::time::Instant,
    ping_buf: [u8; HEADER],
    file: Option<File>,
}

impl VideoThread {
    pub fn new() -> VideoThread {
        let mut ping_buf = [0u8; HEADER];

        write_header(
            crate::EPacketType::Ping,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut ping_buf,
        );

        VideoThread {
            packet_constructor: NALPacketConstructor::new(),
            clock: std::time::Instant::now(),
            channel: unbounded(),
            ping_buf,
            file: None,
        }
    }

    pub fn rgb_to_slint_pixel_buffer(
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<slint::SharedPixelBuffer<slint::Rgb8Pixel>, Error> {
        // TODO: Handle error accordingly
        // TODO: consider removing this check, or somehow optimizing it
        if width * height * 3 != rgb.len() as u32 {
            return Err(Error::new(ErrorKind::InvalidData, "Invalid RGB buffer"));
        }

        // TODO: Cache this and overwrite the buffer instead of creating a new one each time
        let mut pixel_buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
        pixel_buffer.make_mut_bytes().copy_from_slice(rgb);

        Ok(pixel_buffer)
    }

    #[cfg(target_os = "windows")]
    pub fn run(
        &mut self,
        app_weak: slint::Weak<super::slint_generatedMainWindow::MainWindow>,
        _conn_sender: Sender<super::ConnectionAction>,
        client: Client,
    ) {
        use ffmpeg_next::{
            format::Pixel,
            software::{self, scaling::Context},
        };

        ffmpeg_next::init().unwrap();

        let mut ffmpeg_decoder = ffmpeg_next::decoder::new()
            .open_as(ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264))
            .unwrap()
            .video()
            .unwrap();

        let receiver = self.channel.1.clone();
        let _video_thread = thread::spawn(move || {
            // TODO: switch scalar depending on bitrate to reduce latency
            let meta_clone = client.get_meta_clone();

            let mut previous_width = 0;
            let mut previous_height = 0;

            let mut lanczos_scalar: Option<Context> = None;

            loop {
                let buf = receiver.recv().unwrap();
                let pt: ffmpeg_next::Packet = ffmpeg_next::packet::Packet::copy(&buf);

                match ffmpeg_decoder.send_packet(&pt) {
                    Ok(_) => (),
                    Err(e) => {
                        println!("Error Sending Packet: {}", e);
                        continue;
                    }
                };

                let mut yuv_frame = frame::Video::empty();
                let mut rgb_frame = frame::Video::empty();

                while ffmpeg_decoder.receive_frame(&mut yuv_frame).is_ok() {
                    if ffmpeg_decoder.width() != previous_width
                        || ffmpeg_decoder.height() != previous_height
                    {
                        previous_width = ffmpeg_decoder.width();
                        previous_height = ffmpeg_decoder.height();

                        meta_clone.write().unwrap().width = ffmpeg_decoder.width() as usize;
                        meta_clone.write().unwrap().height = ffmpeg_decoder.height() as usize;

                        lanczos_scalar = Some(
                            software::scaling::context::Context::get(
                                ffmpeg_decoder.format(),
                                ffmpeg_decoder.width() as u32,
                                ffmpeg_decoder.height() as u32,
                                Pixel::RGB24,
                                ffmpeg_decoder.width() as u32,
                                ffmpeg_decoder.height() as u32,
                                software::scaling::flag::Flags::LANCZOS,
                            )
                            .unwrap(),
                        );

                        let app_weak_clone = app_weak.clone();
                        let resolution_id =
                            format!("{}x{}", ffmpeg_decoder.width(), ffmpeg_decoder.height());

                        let _ = slint::invoke_from_event_loop(move || {
                            let resolution_index = app_weak_clone
                                .unwrap()
                                .global::<ControlPanelAdapter>()
                                .get_resolutions()
                                .iter()
                                .position(|res| res.value == resolution_id);

                            if let Some(resolution_index) = resolution_index {
                                app_weak_clone
                                    .unwrap()
                                    .global::<ControlPanelAdapter>()
                                    .set_resolution_index(resolution_index.try_into().unwrap());
                            }
                        });
                    }

                    if let Some(scalar) = &mut lanczos_scalar {
                        scalar.run(&yuv_frame, &mut rgb_frame).unwrap();
                    }

                    let rgb_buffer: &[u8] = rgb_frame.data(0);
                    if let Ok(pixel_buffer) = VideoThread::rgb_to_slint_pixel_buffer(
                        rgb_buffer,
                        ffmpeg_decoder.width(),
                        ffmpeg_decoder.height(),
                    ) {
                        let app_copy: slint::Weak<super::slint_generatedMainWindow::MainWindow> =
                            app_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            app_copy
                                .unwrap()
                                .set_video_frame(slint::Image::from_rgb8(pixel_buffer));
                            // TODO: test if this actually improves smoothness
                            // app_copy.unwrap().window().request_redraw();
                        });
                    };
                }
            }
        });
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn run(
        &mut self,
        app_weak: slint::Weak<super::slint_generatedMainWindow::MainWindow>,
        _conn_sender: Sender<super::ConnectionAction>,
        client: Client,
    ) {
        use log::{debug, trace};

        use crate::video::convert::RGBBuffer;

        ffmpeg_next::init().unwrap();

        let mut ffmpeg_decoder = ffmpeg_next::decoder::new()
            .open_as(ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264))
            .unwrap()
            .video()
            .unwrap();

        let receiver = self.channel.1.clone();
        let _video_thread = thread::spawn(move || {
            let meta_clone = client.get_meta_clone();
            let mut fps_clock = std::time::Instant::now();
            let mut frame_count = 0u8;
            let mut fps = 0u8;
            let mut frame_interval = 0u128;

            let mut previous_width = 0;
            let mut previous_height = 0;

            let mut rgb_buffer = Option::<RGBBuffer>::None;

            let mut last: Option<std::time::Instant> = None;

            loop {
                let buf = match receiver.recv() {
                    Ok(buf) => buf,
                    Err(_e) => {
                        debug!("Video Tunnel Closed");
                        break;
                    }
                };
                let skip = match last {
                    Some(last) => {
                        if last.elapsed().as_millis() > frame_interval && receiver.len() > 0 {
                            debug!(
                                "Skipping Frame | Interval: {} | Elapsed: {} | Queue: {}",
                                frame_interval,
                                last.elapsed().as_millis(),
                                receiver.len()
                            );
                            true
                        } else {
                            false
                        }
                    }
                    None => false,
                };

                last = Some(std::time::Instant::now());

                let pt: ffmpeg_next::Packet = ffmpeg_next::packet::Packet::copy(&buf);

                match ffmpeg_decoder.send_packet(&pt) {
                    Ok(_) => (),
                    Err(e) => {
                        debug!("Error Sending Packet due to \"{}\"", e);
                        continue;
                    }
                };

                let mut yuv_frame = frame::Video::empty();

                while ffmpeg_decoder.receive_frame(&mut yuv_frame).is_ok() {
                    if ffmpeg_decoder.width() != previous_width
                        || ffmpeg_decoder.height() != previous_height
                    {
                        previous_width = ffmpeg_decoder.width();
                        previous_height = ffmpeg_decoder.height();

                        meta_clone.write().unwrap().width = ffmpeg_decoder.width() as usize;
                        meta_clone.write().unwrap().height = ffmpeg_decoder.height() as usize;

                        rgb_buffer = Some(RGBBuffer::new(
                            ffmpeg_decoder.width() as usize,
                            ffmpeg_decoder.height() as usize,
                        ));

                        let app_weak_clone = app_weak.clone();
                        let resolution_id =
                            format!("{}x{}", ffmpeg_decoder.width(), ffmpeg_decoder.height());

                        let _ = slint::invoke_from_event_loop(move || {
                            let resolution_index = app_weak_clone
                                .unwrap()
                                .global::<ControlPanelAdapter>()
                                .get_resolutions()
                                .iter()
                                .position(|res| res.value == resolution_id);

                            if let Some(resolution_index) = resolution_index {
                                app_weak_clone
                                    .unwrap()
                                    .global::<ControlPanelAdapter>()
                                    .set_resolution_index(resolution_index.try_into().unwrap());
                            }
                        });
                    } else if skip {
                        debug!("Skipping Frame");
                        continue;
                    }

                    if let Some(rgb) = &mut rgb_buffer {
                        frame_count += 1;
                        if fps_clock.elapsed().as_secs() >= 1 {
                            fps_clock = std::time::Instant::now();
                            trace!("FPS: {}", frame_count);
                            fps = frame_count;

                            if fps > 0 {
                                frame_interval = (1000.0 / fps as f32).ceil() as u128;
                            }

                            frame_count = 0;
                        }

                        rgb.read_420_for_rgb8(
                            yuv_frame.data(0),
                            yuv_frame.data(1),
                            yuv_frame.data(2),
                        );

                        let rgb_slice: &[u8] = rgb.as_slice();

                        if let Ok(pixel_buffer) = VideoThread::rgb_to_slint_pixel_buffer(
                            rgb_slice,
                            ffmpeg_decoder.width(),
                            ffmpeg_decoder.height(),
                        ) {
                            let app_copy: slint::Weak<
                                super::slint_generatedMainWindow::MainWindow,
                            > = app_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                app_copy
                                    .unwrap()
                                    .global::<BarialState>()
                                    .set_fps(fps as i32);

                                app_copy
                                    .unwrap()
                                    .set_video_frame(slint::Image::from_rgb8(pixel_buffer));
                                // TODO: test if this actually improves smoothness
                                // app_copy.unwrap().window().request_redraw();
                            });
                        };
                    }
                }
            }
        });
    }

    #[inline]
    #[allow(dead_code)]
    pub fn write_stream(&mut self, nalu: &[u8]) {
        if let Some(file) = &mut self.file {
            file.write_all(nalu).unwrap();
        }
    }

    #[inline]
    pub fn packet(&mut self, buf: &[u8], client: &Client, number_of_bytes: usize) {
        let state =self
            .packet_constructor
            .assemble_packet(buf, number_of_bytes, &|encrypted_nalu: Vec<u8>| {
                let sym_key = client.get_sym_key();
                let sym_key = sym_key.read().unwrap();
        
                let nalu = match decrypt_frame(sym_key.as_ref().unwrap(), &encrypted_nalu) {
                    Some(nalu) => nalu,
                    None => return,
                };
        
                self.channel.0.send(nalu).unwrap();
            }, &|frame_id, real_packet_size, subpacket_ids| {
                if let Err(e) = client.retransmit(
                    frame_id,
                    real_packet_size,
                    subpacket_ids,
                ) {
                    log::error!("Error Requesting Retransmission: {}", e);
                }
            });       


        match state {
            EAssemblerState::Waiting => {
                return
            }
            EAssemblerState::Queue(queue) => {
                debug!("Queue Loop: {}", queue.len());
                // this is "no bueno"
                for nalu in queue {
                    self.packet(&nalu, client, nalu.len());
                }
            }
            _ => {}
        }

        if self.clock.elapsed().as_secs() > CLIENT_PING_FREQUENCY {
            self.clock = std::time::Instant::now();
            client.send(&self.ping_buf).unwrap();
        }
    }
}
