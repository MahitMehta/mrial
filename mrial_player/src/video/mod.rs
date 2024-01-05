use std::{thread, fs::File, io::Write};

use ffmpeg_next::{frame, software, format::Pixel }; 
use kanal::{unbounded, Sender, Receiver};
use mrial_proto::*;

const W: usize = 1440; 
const H: usize = 900;

// should be max resolution of monitor
const TARGET_WIDTH: usize = 1440; // 2560
const TARGET_HEIGHT: usize = 900; // 1600

pub struct VideoThread {
    nal: Vec<Vec<u8>>,
    prev: i16,
    incorrect_order: bool,
    pub channel: (Sender<Vec<u8>>, Receiver<Vec<u8>>)
}

impl VideoThread {
    pub fn new() -> VideoThread {
        VideoThread {
            prev: -1,
            incorrect_order: false,
            nal: Vec::new(),
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

            let mut file = File::create("recording.h264").unwrap();

            loop {
                let buf = receiver.recv().unwrap(); 
                file.write(&buf).unwrap();

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
        number_of_bytes: usize,
        packets_remaining: u16, 
        real_packet_size: u32
    ) {
        if self.prev != (packets_remaining + 1) as i16 && self.prev > 0 {
            println!("Packet Order Mixup: {} -> {}", self.prev, packets_remaining);
            self.incorrect_order = true; 
        } 
        self.prev = packets_remaining as i16;

        self.nal.push(buf[..number_of_bytes].to_vec());
        if packets_remaining != 0 { return; }

        if self.incorrect_order {
            let nal_size = (self.nal.len() - 1) * PAYLOAD + self.nal.last().unwrap().len() - HEADER;
            if real_packet_size as usize != nal_size {
                if real_packet_size as usize > nal_size {
                    println!("Not Fixable");
                    self.nal.clear();
                    return;
                } else {
                    println!("Fixable");
                    let last_packet_id = parse_packet_id(self.nal.last().unwrap());
                    self.nal.retain(|packet| {
                        parse_packet_id(&packet) == last_packet_id
                    })
                }
            } else {
                self.nal.sort_by(|a, b| {
                    let a_size = parse_packets_remaining(&a);
                    let b_size = parse_packets_remaining(&b);
                    b_size.cmp(&a_size)
                })
            }
            self.incorrect_order = false;
        }

        let mut nalu = Vec::new();
        for packet in &self.nal {
            nalu.extend_from_slice(&packet[HEADER..]);
        }
        
        // self.file.write_all(&self.nal).unwrap();
        self.channel.0.send(nalu).unwrap();
        self.nal.clear();    
    }
}