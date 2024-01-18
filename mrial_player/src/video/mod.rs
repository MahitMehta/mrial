use std::{fs::File, io::Write, thread};

use ffmpeg_next::{
    format::Pixel,
    frame,
    software::{self, scaling::Context},
};
use kanal::{unbounded, Receiver, Sender};
use mrial_proto::*;

use crate::client::Client;

pub struct VideoThread {
    packet_constructor: PacketConstructor,
    clock: std::time::Instant,
    pub channel: (Sender<Vec<u8>>, Receiver<Vec<u8>>),
    ping_buf: [u8; HEADER],
    file: Option<File>,
}

impl VideoThread {
    pub fn new() -> VideoThread {
        let mut ping_buf = [0u8; HEADER];

        write_header(
            crate::EPacketType::PING,
            0,
            HEADER.try_into().unwrap(),
            0,
            &mut ping_buf,
        );

        VideoThread {
            packet_constructor: PacketConstructor::new(),
            clock: std::time::Instant::now(),
            channel: unbounded(),
            ping_buf,
            file: None,
        }
    }

    fn rgb_to_slint_pixel_buffer(
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> slint::SharedPixelBuffer<slint::Rgb8Pixel> {
        let mut pixel_buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
        pixel_buffer.make_mut_bytes().copy_from_slice(rgb);

        pixel_buffer
    }

    pub fn run(
        &mut self,
        app_weak: slint::Weak<super::slint_generatedMainWindow::MainWindow>,
        _conn_sender: Sender<super::ConnectionAction>,
        client: Client,
    ) {
        ffmpeg_next::init().unwrap();

        let mut ffmpeg_decoder = ffmpeg_next::decoder::new()
            .open_as(ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264))
            .unwrap()
            .video()
            .unwrap();

        let receiver = self.channel.1.clone();
        let _video_thread = thread::spawn(move || {
            let mut _simple_scalar = software::converter(
                (
                    client.get_meta().width as u32,
                    client.get_meta().height as u32,
                ),
                Pixel::YUV444P,
                Pixel::RGB24,
            )
            .unwrap();

            // TODO: switch scalar depending on bitrate to reduce latency
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
                    if lanczos_scalar.is_none() {
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
                    }

                    if let Some(scalar) = &mut lanczos_scalar {
                        scalar.run(&yuv_frame, &mut rgb_frame).unwrap();
                    }

                    let rgb_buffer: &[u8] = rgb_frame.data(0);
                    let pixel_buffer = VideoThread::rgb_to_slint_pixel_buffer(
                        rgb_buffer,
                        ffmpeg_decoder.width(),
                        ffmpeg_decoder.height(),
                    );

                    let app_copy: slint::Weak<super::slint_generatedMainWindow::MainWindow> =
                        app_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        app_copy
                            .unwrap()
                            .set_video_frame(slint::Image::from_rgb8(pixel_buffer));
                        // TODO: test if this actually improves smoothness
                        // app_copy.unwrap().window().request_redraw();
                    });
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
        let nalu = match self
            .packet_constructor
            .assemble_packet(buf, number_of_bytes)
        {
            Some(nalu) => nalu,
            None => return,
        };

        self.channel.0.send(nalu).unwrap();

        // TODO: Possibly remove this computation from the main thread
        if self.clock.elapsed().as_secs() > CLIENT_PING_FREQUENCY {
            self.clock = std::time::Instant::now();
            client.send(&self.ping_buf).unwrap();
        }
    }
}
