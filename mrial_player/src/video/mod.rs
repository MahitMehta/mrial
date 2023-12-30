use std::thread;

use ffmpeg_next::{frame, software, format::Pixel }; 
use kanal::{unbounded, Sender, Receiver};
use mrial_proto::*;

const W: usize = 1440; 
const H: usize = 900;

// should be max resolution of monitor
const TARGET_WIDTH: usize = 1440; // 2560
const TARGET_HEIGHT: usize = 900; // 1600

pub struct VideoThread {
    nal: Vec<u8>,
    // file: File,
    pub channel: (Sender<Vec<u8>>, Receiver<Vec<u8>>)
}

impl VideoThread {
    pub fn new() -> VideoThread {
        VideoThread {
            nal: Vec::new(),
            // file: File::create("recording.h264").unwrap(),
            channel: unbounded()
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
                Pixel::YUVJ422P, 
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
                            //let start = Instant::now();
                            lanczos_scalar.run(&yuv_frame, &mut rgb_frame).unwrap();
                            //println!("Scaling: {:?}", start.elapsed());
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
                        conn_sender.send(super::ConnectionAction::Reconnect).unwrap();
                    }
                };
            }
        });
    }

    #[inline]
    pub fn packet(
        &mut self, 
        buf: &[u8], 
        number_of_bytes: usize,
        packets_remaining: u16, 
    ) {
        if !assembled_packet(&mut self.nal, &buf, number_of_bytes, packets_remaining) {
            return; 
        }; 
        
        // self.file.write_all(&self.nal).unwrap();
        self.channel.0.send(self.nal.clone()).unwrap();
        self.nal.clear();    
    }
}