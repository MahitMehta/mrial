mod audio; 
mod client; 
mod video; 

use audio::AudioClient;
use client::Client;
use video::VideoThread; 

use mrial_proto::*;
use mrial_proto as proto; 

use std::thread;
use slint::ComponentHandle;

slint::include_modules!();

fn main() {
    let app: MainWindow = MainWindow::new().unwrap();
    let app_weak = app.as_weak();

    let client = Client::new();
    client.send_handshake();

    let client_clone = client.try_clone();
    let _state = thread::spawn(move || {
        let mut buf = [0; MTU];
        //let device_state = device_query::DeviceState::new();

        proto::write_header(
            EPacketType::STATE, 
            0, 
            HEADER as u32,
            &mut buf
        );

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
        // 1 Byte for mouse_move
        // 2 Bytes for X scroll delta
        // 2 Bytes for Y scroll delta

        let _ = slint::invoke_from_event_loop(move || {
            let socket_click = client_clone.try_clone();
            app_weak.unwrap().global::<VideoFunctions>().on_click(move |x, y| {
                let window_width = 1440f32; 
                let window_height = 900f32;

                let x_percent = (x / window_width * 10000.0).round() as u16 + 1; 
                let y_percent = (y / window_height  * 10000.0).round() as u16 + 1;
                
                buf[HEADER + 4..HEADER + 6].copy_from_slice(&x_percent.to_be_bytes());
                buf[HEADER + 6..HEADER + 8].copy_from_slice(&y_percent.to_be_bytes());

                let _ = socket_click.socket.send_to(&buf[0..32], "150.136.127.166:8554");

                buf[HEADER + 4..HEADER + 6].fill(0);
                buf[HEADER + 6..HEADER + 8].fill(0);            
            });

            let socket_mouse_move = client_clone.try_clone();
            // send packets less frequently 
            app_weak.unwrap().global::<VideoFunctions>().on_mouse_move(move |x, y, pressed| {
                let x_percent = (x / 1440.0 * 10000.0).round() as u16 + 1; 
                let y_percent = (y / 900.0 * 10000.0).round() as u16 + 1;
                
                buf[HEADER + 10..HEADER + 12].copy_from_slice(&x_percent.to_be_bytes());
                buf[HEADER + 12..HEADER + 14].copy_from_slice(&y_percent.to_be_bytes());

                buf[HEADER + 14] = pressed as u8; 

                let _ = socket_mouse_move.socket.send_to(&buf[0..32], "150.136.127.166:8554");

                buf[HEADER + 10..HEADER + 12].fill(0);
                buf[HEADER + 12..HEADER + 14].fill(0);
               
                buf[HEADER + 14] = 0 as u8; 
            });

            let socket_scroll = client_clone.try_clone();

            let mut x_delta = 0i16; 
            let mut y_delta = 0i16; 

            let line_height = 12i16; 

            // improve scroll smoothness
            app_weak.unwrap().global::<VideoFunctions>().on_scroll(move |y, x| {                
                x_delta += x as i16; 
                y_delta += y as i16; 

                if x_delta.abs() >= line_height {
                    let x_lines = x_delta / line_height; 
                    buf[HEADER + 14..HEADER + 16].copy_from_slice(&x_lines.to_be_bytes());
                    x_delta %= line_height; 
                }

                if y_delta.abs() >= line_height {
                    let y_lines = y_delta / line_height;
                    buf[HEADER + 16..HEADER + 18].copy_from_slice(&y_lines.to_be_bytes());
                    y_delta %= line_height;
                }

                if buf[HEADER + 14] != 0 || buf[HEADER + 15] != 0 || buf[HEADER + 16] != 0 || buf[HEADER + 17] != 0 {
                    let _ = socket_scroll.socket.send_to(&buf[0..32], "150.136.127.166:8554");
                }
                
                buf[HEADER + 14..HEADER + 16].fill(0);
                buf[HEADER + 16..HEADER + 18].fill(0);
            });

            let socket_key_pressed = client_clone.try_clone();
            
            app_weak.unwrap().global::<VideoFunctions>().on_key_pressed(move |event| {
                match event.text.bytes().next() {
                    Some(key) => {
                        //buf[HEADER] = event.modifiers.control as u8;
                        buf[HEADER + 1] = event.modifiers.shift as u8;
                        buf[HEADER + 2] = event.modifiers.alt as u8;
                        buf[HEADER + 3] = event.modifiers.meta as u8;
                        if key != 17 {
                            buf[HEADER + 8] = key as u8;
                        }
                        
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


            app_weak.unwrap().global::<VideoFunctions>().on_key_released(move |event| {
                match event.text.bytes().next() {
                    Some(key) => {
                        //buf[HEADER] = if event.modifiers.control { event.modifiers.control as u8 + 1 } else { 0 };
                        buf[HEADER + 1] = if event.modifiers.shift { event.modifiers.shift as u8 + 1 } else { 0 };
                        buf[HEADER + 2] = if event.modifiers.alt { event.modifiers.alt as u8 + 1 } else { 0 };
                        buf[HEADER + 3] = if event.modifiers.meta { event.modifiers.meta as u8 + 1 } else { 0 };
                        
                        if key != 17 {
                            buf[HEADER + 9] = key;
                        }

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
        let mut buf: [u8; MTU] = [0; MTU];

        let (_stream, handle) = rodio::OutputStream::try_default().unwrap();
        let sink = rodio::Sink::try_new(&handle).unwrap();


        let mut audio = AudioClient::new(sink);

        let video_client = client.try_clone();
        let mut video = VideoThread::new();
        video.begin_decoding(app_weak.clone(), video_client);

        loop {
            let (number_of_bytes, _) = client.socket.recv_from(&mut buf).expect("Failed to Receive Packet");
            let (packet_type, packets_remaining, _real_packet_size) = proto::parse_header(&buf);

            match packet_type {
                EPacketType::AUDIO => audio.play_audio_stream(&buf, number_of_bytes, packets_remaining),
                EPacketType::NAL => video.packet(&buf, number_of_bytes, packets_remaining),
                _ => {}
            }
        }     
    });

    app.run().unwrap();
}