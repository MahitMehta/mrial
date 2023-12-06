mod proto;
mod audio; 

use std::thread;
use ffmpeg_next::{ frame, format::Pixel, software };
use audio::AudioClient;
use device_query::DeviceQuery;
use proto::{ MTU, EPacketType, HEADER, Client };
use slint::ComponentHandle;

const W: usize = 1440; 
const H: usize = 900;

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

    let client = Client::new();
    client.send_handshake();

    let client_clone = client.try_clone();
    let _state = thread::spawn(move || {
        let mut buf = [0; MTU];
        let device_state = device_query::DeviceState::new();


        proto::Packet::new(EPacketType::STATE, 0, HEADER as u32)
            .write_header(&mut buf);

        // State Payload
        // 1 Byte for Control 
        // 1 Byte for Shift
        // 1 Byte for Alt
        // 1 Byte for Meta
        // 2 Bytes for X for click
        // 2 Bytes for Y for click
        // 1 Byte for key pressed
        // 1 Byte for key released
        // 2 Bytes for X location
        // 2 Bytes for Y location
        // 1 Byte for drag
        // 2 Bytes for X scroll delta
        // 2 Bytes for Y scroll delta

        let _ = slint::invoke_from_event_loop(move || {
            let socket_click = client_clone.try_clone();
            app_weak.unwrap().on_click(move |x, y| {
                let x_percent = (x / 1440.0 * 10000.0).round() as u16 + 1; 
                let y_percent = (y / 900.0 * 10000.0).round() as u16 + 1;
                
                buf[HEADER + 4..HEADER + 6].copy_from_slice(&x_percent.to_be_bytes());
                buf[HEADER + 6..HEADER + 8].copy_from_slice(&y_percent.to_be_bytes());

                let _ = socket_click.socket.send_to(&buf[0..32], "150.136.127.166:8554");

                buf[HEADER + 4..HEADER + 6].fill(0);
                buf[HEADER + 6..HEADER + 8].fill(0);            
            });

            let socket_drag = client_clone.try_clone();
            let device_state_clone = device_state.clone();
            // send packets less frequently 
            app_weak.unwrap().on_drag(move |x, y| {
                let x_percent = (x / 1440.0 * 10000.0).round() as u16 + 1; 
                let y_percent = (y / 900.0 * 10000.0).round() as u16 + 1;
                
                buf[HEADER + 10..HEADER + 12].copy_from_slice(&x_percent.to_be_bytes());
                buf[HEADER + 12..HEADER + 14].copy_from_slice(&y_percent.to_be_bytes());

                let is_dragging: bool = device_state_clone.get_mouse().button_pressed[1]; 
                buf[HEADER + 14] = is_dragging as u8; 

                let _ = socket_drag.socket.send_to(&buf[0..32], "150.136.127.166:8554");

                buf[HEADER + 10..HEADER + 12].fill(0);
                buf[HEADER + 12..HEADER + 14].fill(0);
               
                buf[HEADER + 14] = 0 as u8; 
            });

            let socket_scroll = client_clone.try_clone();

            let mut x_delta = 0i16; 
            let mut y_delta = 0i16; 

            let line_height = 16i16; 

            app_weak.unwrap().on_scroll(move |y, x| {
               
                
                x_delta += x as i16; 
                y_delta += y as i16; 

               // println!("Scroll X: {} Y: {}", x_delta, y_delta); // need to scale local x,y pixel deltas 

                if x_delta.abs() >= line_height {
                    let x_lines = x_delta / line_height; 
                    println!("X: {}", x_lines);
                    buf[HEADER + 14..HEADER + 16].copy_from_slice(&x_lines.to_be_bytes());
                    x_delta %= line_height; 
                }

                if y_delta.abs() >= line_height {
                    let y_lines = y_delta / line_height;
                    println!("Y: {}", y_lines);
                    buf[HEADER + 16..HEADER + 18].copy_from_slice(&y_lines.to_be_bytes());
                    y_delta %= line_height;
                }

                let _ = socket_scroll.socket.send_to(&buf[0..32], "150.136.127.166:8554");
                
                buf[HEADER + 14..HEADER + 16].fill(0);
                buf[HEADER + 16..HEADER + 18].fill(0);
            });

            let socket_key_pressed = client_clone.try_clone();
            app_weak.unwrap().on_key_pressed(move |event| {
                match event.text.bytes().next() {
                    Some(key) => {
                        buf[HEADER] = event.modifiers.control as u8;
                        buf[HEADER + 1] = event.modifiers.shift as u8;
                        buf[HEADER + 2] = event.modifiers.alt as u8;
                        buf[HEADER + 3] = event.modifiers.meta as u8;
                        buf[HEADER + 8] = key as u8;

                        println!("Key Pressed: {}", buf[HEADER + 8]);
                        let _ = socket_key_pressed.socket.send_to(&buf[0..32], "150.136.127.166:8554");

                        buf[HEADER..HEADER + 4].fill(0);
                        buf[HEADER + 8] = 0; 
                    }
                    None => {
                        println!("Key Pressed: None");
                    }
                }
            });


            app_weak.unwrap().on_key_released(move |event| {
                match event.text.bytes().next() {
                    Some(key) => {
                        buf[HEADER] = if event.modifiers.control { event.modifiers.control as u8 + 1 } else { 0 };
                        buf[HEADER + 1] = if event.modifiers.shift { event.modifiers.shift as u8 + 1 } else { 0 };
                        buf[HEADER + 2] = if event.modifiers.alt { event.modifiers.alt as u8 + 1 } else { 0 };
                        buf[HEADER + 3] = if event.modifiers.meta { event.modifiers.meta as u8 + 1 } else { 0 };
                        buf[HEADER + 9] = key;

                        let _ = client_clone.socket.send_to(&buf[0..32], "150.136.127.166:8554");

                        buf[HEADER..HEADER + 4].fill(0);
                        buf[HEADER + 9] = 0; 
                    }
                    None => {
                        println!("Key Pressed: None");
                    }
                }
            });
        });
    });

    let app_weak = app.as_weak();

    let _conn = thread::spawn(move || {
        let (_stream, handle) = rodio::OutputStream::try_default().unwrap();
        let sink = rodio::Sink::try_new(&handle).unwrap();

        let mut buf: [u8; MTU] = [0; MTU];
        let mut nal: Vec<u8> = Vec::new();
    
        ffmpeg_next::init().unwrap();
        let mut decoder = ffmpeg_next::decoder::new()
            .open_as(ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264))
            .unwrap()
            .video()
            .unwrap(); 
        let mut scalar = software::converter((W as u32, H as u32), Pixel::YUV420P, Pixel::RGB24)
            .unwrap();

        let mut audio = AudioClient::new(sink);

        loop {
            let (number_of_bytes, _) = client.socket.recv_from(&mut buf).expect("Failed to Receive Packet");
            
            if buf[0] == EPacketType::AUDIO as u8 {
                audio.play_audio_stream(&buf, number_of_bytes);
                continue;
            }

            nal.extend_from_slice(&buf[HEADER..number_of_bytes]); 
            
            if buf[1] == 0 { 
                let nal_size_bytes: [u8; 4] = buf[2..6].try_into().unwrap();
                let nal_size = u32::from_be_bytes(nal_size_bytes) as usize;
    
                let size = if nal_size <= nal.len() { nal_size } else { nal.len() }; 
                let pt: ffmpeg_next::Packet = ffmpeg_next::packet::Packet::copy(&nal[0..size as usize]);

                match decoder.send_packet(&pt) {
                    Ok(_) => {
                        let mut raw_frame = frame::Video::empty();
                        let mut rgb_frame = frame::Video::empty();

                        while decoder.receive_frame(&mut raw_frame).is_ok() {
                            let app_copy = app_weak.clone();

                            scalar.run(&raw_frame, &mut rgb_frame).unwrap();
                            let rgb_buffer: &[u8] = rgb_frame.data(0);

                            let pixel_buffer = rgb_to_slint_pixel_buffer(rgb_buffer);
                            let _ = slint::invoke_from_event_loop(move || {
                                    app_copy.unwrap().set_video_frame(slint::Image::from_rgb8(pixel_buffer));
                                    app_copy.unwrap().window().request_redraw(); // test if this actually improves smoothness
                            });
                        }

                        
                    },
                    Err(e) => {
                        println!("Error Sending Packet: {}", e);
                        client.send_handshake(); // limit number of handshakes
                    }
                };
    
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

        title: "MRIAL";
        padding: 0;

        pure callback drag(/* x */ length, /* y */ length);
        pure callback click(/* x */ length, /* y */ length);
        pure callback modifiers_pressed(/* control */ bool, /* shift */ bool, /* alt */ bool, /* meta */ bool);
        pure callback key_pressed(KeyEvent);
        pure callback key_released(KeyEvent);
        pure callback scroll(/* x */ length, /* y */ length);

        forward-focus: key-handler;
        key-handler := FocusScope {
            key-pressed(event) => {
                key_pressed(event);
                accept
            }
            key-released(event) => {
                debug(event.text);
                key-released(event);
                accept
            }
        }

        VerticalBox {
            padding: 0;
            image := Image {
                padding: 0;
                height: 900px;
                touch := TouchArea {
                    clicked => {
                        click(touch.mouse-x, touch.mouse-y);
                    }
                    scroll-event(event) => {
                        scroll(event.delta-x, event.delta-y);
                        accept
                    }
                    pointer-event(event) => {
                       // debug(touch.mouse-x) // can use touch in combination with button right to detect right click
                    }
                    moved => {
                        drag(touch.mouse-x, touch.mouse-y);
                    }
                }
            }
        }
    }
}