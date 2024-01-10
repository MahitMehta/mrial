use std::thread;

use ffmpeg_next::{frame, software, format::Pixel }; 
use kanal::{unbounded, Sender, Receiver};
use mrial_proto::*;

use crate::client::Client;

pub const W: usize = 1440; 
pub const H: usize = 900;

// should be max resolution of monitor
const TARGET_WIDTH: usize = 1440; // 2560
const TARGET_HEIGHT: usize = 900; // 1600

pub struct VideoThread {
    packet_constructor: PacketConstructor,
    clock: std::time::Instant,
    pub channel: (Sender<Vec<u8>>, Receiver<Vec<u8>>),
    ping_buf: [u8; HEADER]
}

impl VideoThread {
    pub fn new() -> VideoThread {
        let mut ping_buf = [0u8; HEADER];

        write_header(
            crate::EPacketType::PING, 
            0, 
            HEADER.try_into().unwrap(), 
            0, 
            &mut ping_buf
        );

        VideoThread {
            packet_constructor: PacketConstructor::new(),
            clock: std::time::Instant::now(),
            channel: unbounded(),
            ping_buf
        }
    }

    fn rgb_to_slint_pixel_buffer(
        rgb: &[u8],
    ) -> slint::SharedPixelBuffer<slint::Rgb8Pixel> {
        let mut pixel_buffer =
            slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(TARGET_WIDTH as u32, TARGET_HEIGHT as u32);
            pixel_buffer.make_mut_bytes().copy_from_slice(rgb);
    
        pixel_buffer
    }

    pub fn begin_decoding(
        &mut self,
        app_weak: slint::Weak<super::slint_generatedMainWindow::MainWindow>,
        conn_sender: Sender<super::ConnectionAction>
    ) {
        ffmpeg_next::init().unwrap();

        // get signal stats
        // ffmpeg -i test.h264 -vf "signalstats,metadata=print:file=logfile.txt" -an -f null -
        let mut ffmpeg_decoder = ffmpeg_next::decoder::new()
            .open_as(ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264))
            .unwrap()
            .video()
            .unwrap(); 

        let receiver = self.channel.1.clone();
        let _video_thread = thread::spawn(move || {
            let mut _simple_scalar = software::converter((W as u32, H as u32), Pixel::YUVJ422P, Pixel::RGB24)
                .unwrap();
        
              // TODO: switch scalar depending on bitrate to reduce latency
            let mut lanczos_scalar = software::scaling::context::Context::get(
                Pixel::YUV444P, 
                W as u32, 
                H as u32, 
                Pixel::RGB24, 
                TARGET_WIDTH as u32, 
                TARGET_HEIGHT as u32, 
                software::scaling::flag::Flags::LANCZOS
            ).unwrap();

            loop {
                let buf = receiver.recv().unwrap(); 
                let pt: ffmpeg_next::Packet = ffmpeg_next::packet::Packet::copy(&buf);
  
                match ffmpeg_decoder.send_packet(&pt) {
                    Ok(_) => {
                        let mut yuv_frame = frame::Video::empty();
                        let mut rgb_frame = frame::Video::empty();
                       
                        while ffmpeg_decoder.receive_frame(&mut yuv_frame).is_ok() {
                            // let start = std::time::Instant::now();
                            lanczos_scalar.run(&yuv_frame, &mut rgb_frame).unwrap();
                            // println!("Scaling: {:?}", start.elapsed());
                            let rgb_buffer: &[u8] = rgb_frame.data(0);
                            let pixel_buffer = VideoThread::rgb_to_slint_pixel_buffer(rgb_buffer);
                        
                            let app_copy: slint::Weak<super::slint_generatedMainWindow::MainWindow> = app_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                    app_copy.unwrap().set_video_frame(slint::Image::from_rgb8(pixel_buffer));
                                    // app_copy.unwrap().window().request_redraw(); // test if this actually improves smoothness
                            });
                        }
                    },
                    Err(e) => {
                        println!("Error Sending Packet: {}", e);
                       //  conn_sender.send(super::ConnectionAction::Reconnect).unwrap();
                    }
                };
            }
        });
    }

    #[inline]
    pub fn packet(
        &mut self, 
        buf: &[u8], 
        client: &Client,
        number_of_bytes: usize,
    ) {
        let nalu = match self.packet_constructor.assemble_packet(
            buf, number_of_bytes) {
            Some(nalu) => nalu,
            None => return
        };

        // let mut file = File::create("recording.h264").unwrap();
        // file.write_all(&nalu).unwrap();
        self.channel.0.send(nalu).unwrap();

        // TODO: Possibly remove this computation from the main thread
        if self.clock.elapsed().as_secs() > CLIENT_PING_FREQUENCY {
            self.clock = std::time::Instant::now();
            client.send(&self.ping_buf).unwrap();
        } 
    }
}