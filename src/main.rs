use std::{thread, net::UdpSocket};
use openh264::{decoder::{Decoder, DecodedYUV}, nal_units};
use slint::ComponentHandle;

fn send_handshake(socket : &UdpSocket) {
    let _ = socket.send_to(b"shake", "150.136.127.166:8554");

    println!("Sent Handshake Packet");

    let mut buf: [u8; 5] = [0; 5];
    
    let (amt, _src) = socket.recv_from(&mut buf).expect("Failed to Receive Packet");

    assert!(amt == 5);

    println!("Received Handshake Packet");
}

fn ping(socket : &UdpSocket) {
    let _ = socket.send_to(b"ping", "150.136.127.166:8554");
}

const MTU: usize = 1032; 

// Header Schema
// Packets Remaining = 1 byte
// Real NAL Byte Size = 4 bytes
// 3 Bytes are currently unoccupied
const HEADER: usize = 8; 
// const PACKET: usize = MTU - HEADER;

const W: usize = 1440; 
const H: usize = 900;

const GAMMA_RGB_CORRECTION: [u8; 256] = [
    0,   0,   1,   1,   1,   2,   2,   3,   3,   4,   4,   5,   6,   6,   7,   7,
    8,   9,   9,  10,  11,  11,  12,  13,  13,  14,  15,  15,  16,  17,  18,  18,
    19,  20,  21,  21,  22,  23,  24,  24,  25,  26,  27,  28,  28,  29,  30,  31,
    32,  32,  33,  34,  35,  36,  37,  37,  38,  39,  40,  41,  42,  43,  44,  44,
    45,  46,  47,  48,  49,  50,  51,  52,  52,  53,  54,  55,  56,  57,  58,  59,
    60,  61,  62,  63,  64,  65,  66,  66,  67,  68,  69,  70,  71,  72,  73,  74,
    75,  76,  77,  78,  79,  80,  81,  82,  83,  84,  85,  86,  87,  88,  89,  90,
    91,  92,  93,  94,  95,  96,  97,  98,  99, 100, 101, 103, 104, 105, 106, 107,
   108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 121, 122, 123, 124,
   125, 126, 127, 128, 129, 130, 131, 132, 134, 135, 136, 137, 138, 139, 140, 141,
   142, 144, 145, 146, 147, 148, 149, 150, 151, 152, 154, 155, 156, 157, 158, 159,
   160, 162, 163, 164, 165, 166, 167, 168, 170, 171, 172, 173, 174, 175, 177, 178,
   179, 180, 181, 182, 184, 185, 186, 187, 188, 189, 191, 192, 193, 194, 195, 196,
   198, 199, 200, 201, 202, 204, 205, 206, 207, 208, 210, 211, 212, 213, 214, 216,
   217, 218, 219, 220, 222, 223, 224, 225, 227, 228, 229, 230, 231, 233, 234, 235,
   236, 238, 239, 240, 241, 243, 244, 245, 246, 248, 249, 250, 251, 253, 254, 255,
];

pub fn write_rgb8(yuv: DecodedYUV, target: &mut [u8]) {
    let dim = yuv.dimension_rgb();
    let strides = yuv.strides_yuv();

    for y in 0..dim.1 {
        for x in 0..dim.0 {
            let base_tgt = (y * dim.0 + x) * 3;
            let base_y = y * strides.0 + x;
            let base_u = (y / 2 * strides.1) + (x / 2);
            let base_v = (y / 2 * strides.2) + (x / 2);

            let rgb_pixel = &mut target[base_tgt..base_tgt + 3];

            let y = yuv.y_with_stride()[base_y] as f32;
            let u = yuv.u_with_stride()[base_u] as f32;
            let v = yuv.v_with_stride()[base_v] as f32;

            rgb_pixel[0] = (1.164 * (y - 16.0) + 1.596 * (v - 128.0)) as u8; 
            //rgb_pixel[0] = (y + 1.402 * (v - 128.0)) as u8;
            rgb_pixel[1] = (1.164 * (y - 16.0) - 0.183 * (v - 128.0) - 0.392 * (u - 128.0)) as u8;
            //rgb_pixel[1] = (y - 0.344 * (u - 128.0) - 0.714 * (v - 128.0)) as u8;
            rgb_pixel[2] = (1.164 * (y - 16.0) + 2.017 * (u - 128.0)) as u8;
            //rgb_pixel[2] = (y + 1.772 * (u - 128.0)) as u8;
        }
    }
}


fn rgb_to_slint_pixel_buffer(
    rgb: &[u8],
) -> slint::SharedPixelBuffer<slint::Rgb8Pixel> {
    let mut pixel_buffer =
        slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(W as u32, H as u32);
        pixel_buffer.make_mut_bytes().copy_from_slice(rgb);

    pixel_buffer
}

fn main() {
    let app: MainWindow = MainWindow::new().unwrap();
    let app_weak = app.as_weak();
    // washed out colors seem to be a problem with the decoder
    // additionally, I should experiment with x264 encoder and see if that provides equiavlent speeds
    let _conn = thread::spawn(move || {
        let socket = UdpSocket::bind("0.0.0.0:8080").expect("Failed to Bind to Incoming Socket");

        send_handshake(&socket);
    
        let mut buf: [u8; MTU] = [0; MTU];
        let mut nal: Vec<u8> = Vec::new();
    
        let mut decoder = Decoder::new().unwrap();
        // let mut file = File::create("fade.h264").unwrap();

        loop {
            socket.recv_from(&mut buf).expect("Failed to Receive Packet");
            nal.extend_from_slice(&buf[HEADER..]); 
    
            if buf[0] == 0 {
                let nal_size_bytes: [u8; 4] = buf[1..5].try_into().unwrap();
                let nal_size = u32::from_be_bytes(nal_size_bytes) as usize;
    
                // println!("Received Nal Packet with Length: {} Real Size: {}", nal.len(), nal_size);
                let size = if nal_size <= nal.len() { nal_size } else { nal.len() }; 
                
                // file.write_all(&nal[0..size]).unwrap();
                for packet in nal_units(&nal[0..size as usize]) {
                    let app_copy = app_weak.clone();
                    // let now = Instant::now();
                    let _result = match decoder.decode(packet) {
                        Ok(Some(maybe_some_yuv)) => {
                            let mut rgb: Vec<u8> = vec![0; W*H*3];
                            maybe_some_yuv.write_rgb8(&mut rgb);
                                for i in 0..W * H {
                                    rgb[i * 3] = GAMMA_RGB_CORRECTION[rgb[i * 3] as usize];
                                    rgb[i * 3 + 1] = GAMMA_RGB_CORRECTION[rgb[i * 3 + 1] as usize];
                                    rgb[i * 3 + 2] = GAMMA_RGB_CORRECTION[rgb[i * 3 + 2] as usize];
                                }
                                let pixel_buffer = rgb_to_slint_pixel_buffer(&rgb);
                                let _ = slint::invoke_from_event_loop(move || {
                                        app_copy.unwrap().set_video_frame(slint::Image::from_rgb8(pixel_buffer))
                                });
                            }
                        Ok(None) => {
                          println!("None Recieved");
                        }
                        Err(_) =>  {
                            println!("Error Recieved");
                        },
                    };
                    // println!("Decoded in {} ms", now.elapsed().as_millis());
                }
    
                nal.clear();
            }
        }     
    });

    app.run().unwrap();
}

slint::slint! {
    import { VerticalBox } from "std-widgets.slint";

    export component MainWindow inherits Window {
        in property <image> video-frame <=> image.source;

        min-width: 1440px;
        min-height: 900px;

        title: "MahitM RT";
        padding: 0;

        VerticalBox {
            padding: 0;
            image := Image {
                padding: 0;
                height: 900px;
            }
        }
    }
}