// had to install libxcb-randr0-dev

mod audio;

use std::{time::{Instant, Duration}, collections::VecDeque, thread, sync::{Mutex, Arc}, net::UdpSocket};
use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings,
    {Axis::Horizontal, Axis::Vertical},
    Mouse,
};

use openh264::{encoder::{EncoderConfig, Encoder}, formats::YUVSource};
use futures::{executor::ThreadPool, task::SpawnExt, future::RemoteHandle};
use audio::AudioController;
use x264::{Param, Picture};
use scrap::{Capturer, Display};

#[cfg(target_os = "linux")]
use libyuv_sys::{ARGBToI420, ARGBToI444};

pub enum EPacketType {
    SHAKE = 0,
    SHOOK = 1,
    NAL = 2,
    Audio = 4
}

pub struct Packet {
    pub packet_type: EPacketType, 
    pub count: u8, // nunber of remaining packets
    pub size: u32 // size of entire packet
}

const MTU: usize = 1032;
const HEADER: usize = 8;
const PAYLOAD: usize = MTU - HEADER;

impl Packet {
    pub fn new(packet_type: EPacketType, count: u8, size: u32) -> Self {
        Self {
            packet_type,
            count,
            size
        }
    }
    pub fn get_packet(self, buf: &mut [u8; MTU]) {
        buf[0] = self.packet_type as u8;
        buf[1] = self.count;
        buf[2..6].copy_from_slice(&self.size.to_be_bytes());
    }
}

pub struct YUVBuffer {
    yuv: Vec<u8>,
    width: usize,
    height: usize,
}

impl YUVBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            yuv: vec![0u8; (3 * (width * height)) / 2],
            width,
            height,
        }
    }

    pub fn with_bgra_for_420(width: usize, height: usize, bgra: &[u8]) -> Self {
        let mut rval = Self {
            yuv: vec![0u8; (3 * (width * height)) / 2],
            width,
            height,
        };

        rval.read_bgra_for_420(bgra);
        rval
    }

    #[cfg(target_os = "linux")]
    pub fn with_bgra_for_444(width: usize, height: usize, bgra: &[u8]) -> Self {
        let mut rval = Self {
            yuv: vec![0u8; 3 * width * height],
            width,
            height,
        };

        rval.read_bgra_for_444(bgra);
        rval
    }

    #[cfg(target_os = "linux")]
    pub fn read_bgra_for_444(&mut self, bgra: &[u8]) {
        assert_eq!(bgra.len(), self.width * self.height * 4);
        assert_eq!(self.width % 2, 0, "width needs to be multiple of 2");
        assert_eq!(self.height % 2, 0, "height needs to be a multiple of 2");

    
        let u = self.width * self.height;
        let v = u + u;
        let dst_stride_y = self.width;
        let dst_stride_uv = self.width;
        let dst_y = self.yuv.as_mut_ptr();
        let dst_u = self.yuv[u..].as_mut_ptr();
        let dst_v = self.yuv[v..].as_mut_ptr();

        unsafe {
            ARGBToI444(
                bgra.as_ptr(),
                (bgra.len() / self.height) as _,
                dst_y,
                dst_stride_y as _,
                dst_u,
                dst_stride_uv as _,
                dst_v,
                dst_stride_uv as _,
                self.width as _,
                self.height as _,
            );
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn read_bgra_for_420(&mut self, bgra: &[u8]) {
        
    }

    #[cfg(target_os = "linux")]
    pub fn read_bgra_for_420(&mut self, bgra: &[u8]) {
        assert_eq!(bgra.len(), self.width * self.height * 4);
        assert_eq!(self.width % 2, 0, "width needs to be multiple of 2");
        assert_eq!(self.height % 2, 0, "height needs to be a multiple of 2");

        let u = self.width * self.height;
        let v = u + u / 4;
        let dst_stride_y = self.width;
        let dst_stride_uv = self.width / 2;
        let dst_y = self.yuv.as_mut_ptr();
        let dst_u = self.yuv[u..].as_mut_ptr();
        let dst_v = self.yuv[v..].as_mut_ptr();
        unsafe {
            ARGBToI420(
                bgra.as_ptr(),
                (bgra.len() / self.height) as _,
                dst_y,
                dst_stride_y as _,
                dst_u,
                dst_stride_uv as _,
                dst_v,
                dst_stride_uv as _,
                self.width as _,
                self.height as _,
            );
        }
    }
    
}

// impl <'a>openh264::formats::YUVSource for YUVBuffer {
//     fn width(&self) -> i32 {
//         self.width as i32
//     }

//     fn height(&self) -> i32 {
//         self.height as i32
//     }

//     fn y(&self) -> &[u8] {
//         &self.yuv[0..self.width * self.height]
//     }

//     fn u(&self) -> &[u8] {
//         let base_u = self.width * self.height;
//         &self.yuv[base_u..base_u + base_u]
//     }

//     fn v(&self) -> &[u8] {
//         let base_u = self.width * self.height;
//         let base_v = base_u + base_u;
//         &self.yuv[base_v..]
//     }

//     fn y_stride(&self) -> i32 {
//         self.width as i32
//     }

//     fn u_stride(&self) -> i32 {
//         self.width as i32
//     }

//     fn v_stride(&self) -> i32 {
//         self.width as i32
//     }
// }

impl <'a>openh264::formats::YUVSource for YUVBuffer {
    fn width(&self) -> i32 {
        self.width as i32
    }

    fn height(&self) -> i32 {
        self.height as i32
    }

    fn y(&self) -> &[u8] {
        &self.yuv[0..self.width * self.height]
    }

    fn u(&self) -> &[u8] {
        let base_u = self.width * self.height;
        &self.yuv[base_u..base_u + base_u / 4]
    }

    fn v(&self) -> &[u8] {
        let base_u = self.width * self.height;
        let base_v = base_u + base_u / 4;
        &self.yuv[base_v..]
    }

    fn y_stride(&self) -> i32 {
        self.width as i32
    }

    fn u_stride(&self) -> i32 {
        (self.width / 2) as i32
    }

    fn v_stride(&self) -> i32 {
        (self.width / 2) as i32
    }
}

#[tokio::main]
async fn main() {
    use std::io::ErrorKind::WouldBlock;

    let socket: UdpSocket = UdpSocket::bind("0.0.0.0:8554").expect("Failed to Bind to 0.0.0.0:3000");
    let mut buf: [u8; MTU] = [0; MTU]; 
 
    let Ok((_len, src)) = socket.recv_from(&mut buf) else {
        panic!("Failed!");
    };

    if buf[0] != EPacketType::SHAKE as u8 { 
       panic!("Invalid Handsake");
    }

    let _ = Packet::new(EPacketType::SHOOK, 1, HEADER as u32).get_packet(&mut buf); 
    socket.send_to(&buf[0..HEADER], src).expect("Failed to send pong");

    let d = Display::primary().unwrap();
    
    const W:usize = 1440; 
    const H:usize = 900;

    let mut capturer = Capturer::new(d).unwrap();
    
    let config = EncoderConfig::new(
        W.try_into().unwrap(),
        H.try_into().unwrap()
    );

    //config.enable_skip_frame(true);
    //config.rate_control_mode(openh264::encoder::RateControlMode::Quality);

    let mut encoder = Encoder::with_config(config).unwrap();

    let mut par = Param::default_preset("veryfast", "zerolatency").unwrap();

    par = par.set_dimension(H, W);
    //par = par.param_parse("repeat_headers", "0").unwrap();
    //par = par.param_parse("csp", "i444").unwrap();
    //spar = par.set_csp(12); // i444
    par = par.param_parse("annexb", "1").unwrap();
    par = par.param_parse("bframes", "0").unwrap();
    par = par.param_parse("crf", "18").unwrap();
    // par = par.apply_profile("high444").unwrap();
    par = par.apply_profile("high").unwrap();

    let mut pic = Picture::from_param(&par).unwrap();
    let mut enc = x264::Encoder::open(&mut par).unwrap();


    //unsafe {
        // look into this --> ENCODER_OPTION_IDR_INTERVAL
        // let mut option_value = "main";
        // encoder.raw_api().set_option(11, addr_of_mut!(option_value).cast());
    //}
    //let mut file = File::create("fade.h264").unwrap();

    let pool = ThreadPool::new().unwrap();
    let mut yuv_handles: VecDeque<RemoteHandle<YUVBuffer>> = VecDeque::new();

    let rowlen: usize = 4 * W * H;
    
    let mut frames = 0u8; 
    let mut fps = Instant::now();

    let audio_controller = AudioController::new();
    audio_controller.begin_transmission(socket.try_clone().unwrap(), src);

    let attempt_reconnect = Arc::new(Mutex::new(false));

    let attempt_reconnect_clone = Arc::clone(&attempt_reconnect);
    let socket_clone = socket.try_clone().unwrap();
    let _state = thread::spawn(move || {
        let mouse = mouse_rs::Mouse::new(); // requires package install on linux (libxdo-dev)
        let mut enigo = Enigo::new(&Settings::default()).unwrap();

        loop {
            let mut buf: [u8; MTU] = [0; MTU];
            let (_size, _src) = socket_clone.recv_from(&mut buf).unwrap();

            if buf[0] == EPacketType::SHAKE as u8 {
                *attempt_reconnect_clone.lock().unwrap() = true; 
            }

            // double check this validation is correct for detecting a click 
            if buf[HEADER + 5] != 0 && buf[HEADER + 7] != 0 {
                let x_percent = u16::from_be_bytes(buf[HEADER + 4..HEADER + 6].try_into().unwrap()) - 1; 
                let y_percent = u16::from_be_bytes(buf[HEADER + 6..HEADER + 8].try_into().unwrap()) - 1;

                let x = (x_percent as f32 / 10000.0 * W as f32).round() as i32;
                let y = (y_percent as f32 / 10000.0 * H as f32).round() as i32;

                let _ = mouse.move_to(x, y);
                let _ = mouse.click(&mouse_rs::types::keys::Keys::LEFT);
                // println!("Click: {}, {}", x, y);
            }
            if buf[HEADER + 10] != 0 && buf[HEADER + 12] != 0 {
                let x_percent = u16::from_be_bytes(buf[HEADER + 10..HEADER + 12].try_into().unwrap()) - 1; 
                let y_percent = u16::from_be_bytes(buf[HEADER + 12..HEADER + 14].try_into().unwrap()) - 1;

                let x = (x_percent as f32 / 10000.0 * W as f32).round() as i32;
                let y = (y_percent as f32 / 10000.0 * H as f32).round() as i32;

                let _ = mouse.move_to(x, y);

                // handle right mouse button too
                if buf[HEADER + 14] == 1 {
                    let _ = mouse.press(&mouse_rs::types::keys::Keys::LEFT);
                } else {
                    //let _ = mouse.release(&mouse_rs::types::keys::Keys::LEFT);
                }
                
                // println!("Click: {}, {}", x, y);
            }
            if buf[HEADER + 15] != 0 || buf[HEADER + 17] != 0 {
                let x_delta = i16::from_be_bytes(buf[HEADER + 14..HEADER + 16].try_into().unwrap()); 
                let y_delta = i16::from_be_bytes(buf[HEADER + 16..HEADER + 18].try_into().unwrap());

                enigo.scroll((-x_delta).into(), Vertical).unwrap();
                enigo.scroll((-y_delta).into(), Horizontal).unwrap();
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

                    enigo.key(Key::Unicode((buf[HEADER + 8]) as char), Press).unwrap();
                }
            }
        
            if buf[HEADER + 9] != 0 {
                if buf[HEADER + 9] == 32 {
                    enigo.key(Key::Space, Release).unwrap();
                } else if buf[HEADER + 9] == 8 {
                    enigo.key(Key::Backspace, Release).unwrap();
                } else if buf[HEADER + 9] >= 33 {
                    // add ascii range check
                   enigo.key(Key::Unicode((buf[HEADER + 9]) as char), Release).unwrap();
                }

            }
        }
    });

    let mut timestamp = 1;  
                                                                    
    loop {
        let sleep = Instant::now();
        match capturer.frame() {
            Ok(frame) => {
                let data = frame.chunks(rowlen).next().unwrap().to_vec(); 
                
                let cvt_rgb_yuv = async move {
                    let yuv = YUVBuffer::with_bgra_for_420(W, H, &data);
                    yuv
                };
                yuv_handles.push_back(pool.spawn_with_handle(cvt_rgb_yuv).unwrap());


                // set to 1 to increase FPS at the cost of latency, or 0  for the opposite effect
                if yuv_handles.len() > 1 {
                    //let start = Instant::now();
                    let yuv = yuv_handles.pop_front().unwrap().await;
                    
                    let y_plane = pic.as_mut_slice(0).unwrap();
                    y_plane.copy_from_slice(yuv.y());
                    let u_plane = pic.as_mut_slice(1).unwrap();
                    u_plane.copy_from_slice(yuv.u());
                    let v_plane = pic.as_mut_slice(2).unwrap();
                    v_plane.copy_from_slice(yuv.v());

                    pic = pic.set_timestamp(timestamp);
                    timestamp += 1; 

                    if let Some((nal, _, _)) = enc.encode(&pic).unwrap() {
                        let bitstream = nal.as_bytes();
                        //println!("Encoding Time: {}", start.elapsed().as_millis());
                        
                        // let first_bit = bitstream[0] >> 7; // bitstream[0] & 1;

                        let packets = (bitstream.to_vec().len() as f64 / PAYLOAD as f64).ceil() as usize;
                        buf[3..7].copy_from_slice(&(bitstream.to_vec().len() as u32).to_be_bytes());
                        buf[0] = EPacketType::NAL as u8; // Move this outside of loop

                        for i in 0..packets {
                            buf[1..3].copy_from_slice(&((packets - i - 1) as u16).to_be_bytes());
                            let start = i * PAYLOAD;
                            let addition = if start + PAYLOAD <= bitstream.to_vec().len() { PAYLOAD } else { bitstream.to_vec().len() - start };
                            buf[HEADER..addition + HEADER].copy_from_slice(&bitstream.to_vec()[start..(addition + start)]);
                            socket.send_to(&buf[0..addition + HEADER], src).expect("Failed to send NAL Unit");
                        }
                        frames += 1; 
                      
                    }
                    
                    //let bitstream = encoder.encode(&yuv).unwrap();
                    // file.write_all(&bitstream.to_vec()).unwrap();

                    // move this to a separate thread
                    

        
                    // replace possibly with spin-sleep: https://github.com/alexheretic/spin-sleep
                    if sleep.elapsed().as_millis() > 0 && sleep.elapsed().as_millis() < 17 {
                        let delay = 17 - sleep.elapsed().as_millis();
                        thread::sleep(Duration::from_millis(delay as u64));
                    }
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    
                    // println!("NAL Packet Size: {}", bitstream.to_vec().len());

                    if fps.elapsed().as_millis() > 1000 && frames > 0 {
                        println!("FPS: {}", frames as f32 / fps.elapsed().as_secs() as f32);

                        frames = 0; 
                        fps = Instant::now();
                    }
    
                    if *attempt_reconnect.lock().unwrap() {
                        println!("Reconnecting...");
                        // encoder = Encoder::with_config(config).unwrap();
                        enc = x264::Encoder::open(&mut par).unwrap();
                        buf[0] = EPacketType::SHOOK as u8;
                        socket.send_to(&buf[0..HEADER], src).expect("Failed to send NAL Unit");
                        *attempt_reconnect.lock().unwrap() = false;
                    }
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