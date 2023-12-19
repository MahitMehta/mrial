use std::{thread, time::Instant, fs::File, io::Write};

use ffmpeg_next::{frame, codec::decoder, software::{scaling, self}, format::Pixel }; 
use kanal::{unbounded, Sender, Receiver};
use mrial_proto::*;

use super::client::Client;

const W: usize = 1440; 
const H: usize = 900;

pub struct VideoThread {
    nal: Vec<u8>,
    decoder: decoder::Video,
    scalar: scaling::Context,
    client: Client,
    file: File,
    pub channel: (Sender<Vec<u8>>, Receiver<Vec<u8>>)
}

impl VideoThread {
    pub fn new(
        decoder: decoder::Video, 
        scalar: scaling::Context, 
        client: Client,
    ) -> VideoThread {
        VideoThread {
            nal: Vec::new(),
            decoder,
            scalar,
            client,
            file: File::create("test.h264").unwrap(),
            channel: unbounded()
        }
    }

    fn rgb_to_slint_pixel_buffer(
        &self, rgb: &[u8],
    ) -> slint::SharedPixelBuffer<slint::Rgb8Pixel> {
        let mut pixel_buffer =
            slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(2560 as u32, 1600 as u32);
            pixel_buffer.make_mut_bytes().copy_from_slice(rgb);
    
        pixel_buffer
    }

    pub fn decode(&mut self) {
        ffmpeg_next::init().unwrap();
        let ffmpeg_decoder = ffmpeg_next::decoder::new()
            .open_as(ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264))
            .unwrap()
            .video()
            .unwrap(); 

        let scalar = software::converter((W as u32, H as u32), Pixel::YUVJ420P, Pixel::RGB24)
            .unwrap();

        let receiver = self.channel.1.clone();
        let _video_thread = thread::spawn(move || {
            let mut nal: Vec<u8> = Vec::new();
            loop {
                let buf = receiver.recv().unwrap(); // possibly only send entire nal unit
            }
        });
    }

    pub fn packet(
        &mut self, 
        buf: &[u8], 
        number_of_bytes: usize,
        packets_remaining: u16, 
    ) -> Option<slint::SharedPixelBuffer<slint::Rgb8Pixel>> {
        if !assembled_packet(&mut self.nal, &buf, number_of_bytes, packets_remaining) {
            return None;
        }; 
        
        self.file.write_all(&self.nal).unwrap();
        let pt: ffmpeg_next::Packet = ffmpeg_next::packet::Packet::copy(&self.nal);

        match self.decoder.send_packet(&pt) {
            Ok(_) => {
                let mut yuv_frame = frame::Video::empty();
                let mut rgb_frame = frame::Video::empty();

                // yuv_frame.set_color_space(ffmpeg_next::util::color::Space::BT709);
                while self.decoder.receive_frame(&mut yuv_frame).is_ok() {
                    self.scalar.run(&yuv_frame, &mut rgb_frame).unwrap();
                    let rgb_buffer: &[u8] = rgb_frame.data(0);

                    let pixel_buffer = self.rgb_to_slint_pixel_buffer(rgb_buffer);
                    self.nal.clear();

                    return Some(pixel_buffer);
                }

            },
            Err(e) => {
                println!("Error Sending Packet: {}", e);
                self.nal.clear();
                self.client.send_handshake(); // limit number of handshakes
            }
        };
        
        None
    }
}