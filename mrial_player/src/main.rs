mod audio; 
mod client; 
mod video; 
mod input;
mod storage; 

use storage::{Servers, Storage};
use audio::AudioClient;
use client::{Client, ConnectionState};
use input::Input;
use kanal::unbounded;
use video::VideoThread; 

use mrial_proto::*;
use mrial_proto as proto; 

use std::thread;
use std::time::Duration;
use slint::ComponentHandle;

slint::include_modules!();

#[derive(PartialEq)]
pub enum ConnectionAction {
    Disconnect,
    Connect,
    Reconnect,
    Handshake
}

fn main() {
    let app: MainWindow = MainWindow::new().unwrap();
    let app_weak = app.as_weak();

    let mut client = Client::new();
    let conn_channel =  unbounded::<ConnectionAction>();
    let conn_sender = conn_channel.0.clone();
    
    let mut server_state = Servers::new();
    server_state.load().unwrap();

    slint::invoke_from_event_loop(move || {
        let conn_sender_clone = conn_sender.clone();
        app_weak.unwrap().global::<VideoFunctions>().on_connect(move || {
            conn_sender_clone.send(ConnectionAction::Connect).unwrap();
        });

        app_weak.unwrap().global::<VideoFunctions>().on_disconnect(move || {
            conn_sender.send(ConnectionAction::Disconnect).unwrap();
        });

        app_weak.unwrap().global::<CreateServerFunctions>().on_add(move |name, ip_addr, port| {
            server_state.add(
                name.to_string(), 
                ip_addr.to_string(),
                port.parse::<u16>().unwrap()
            );
            
            server_state.save().unwrap();
        });
    }).unwrap();

    let app_weak = app.as_weak();

    let _conn: thread::JoinHandle<_> = thread::spawn(move || {
        let mut buf: [u8; MTU] = [0; MTU];
       
        let (_stream, handle) = rodio::OutputStream::try_default().unwrap();
        let sink = rodio::Sink::try_new(&handle).unwrap();
        let mut audio = AudioClient::new(sink);

        let mut video = VideoThread::new();
        let video_conn_sender = conn_channel.0.clone();
        video.begin_decoding(app_weak.clone(), video_conn_sender);

        let mut input = Input::new();
        input.capture(app_weak);

        loop {
            // TODO: avoid performing this computation in the stream loop
            if !client.connected() || conn_channel.1.len() > 0 {
                match conn_channel.1.try_recv_realtime().unwrap() {
                    None => {   
                        if !client.connected() { 
                            thread::sleep(Duration::from_millis(25));
                            continue;
                        }
                    }
                    Some(ConnectionAction::Connect) => {
                        client.set_state(ConnectionState::Connecting);
                        conn_channel.0.send(ConnectionAction::Handshake).unwrap();
                        continue
                    }
                    Some(ConnectionAction::Handshake) => {
                        client.connect();

                        match client.connection_state() {
                            ConnectionState::Connected => input.send_loop(&client),
                            ConnectionState::Connecting => {
                                conn_channel.0.send(ConnectionAction::Handshake).unwrap();
                                continue
                            }
                            _ => continue
                        }
                    }
                    Some(ConnectionAction::Reconnect) => {
                        client.connect();
                        if !client.connected() {
                            continue
                        }
                    }
                    Some(ConnectionAction::Disconnect) => {
                        if client.connected() {
                            input.close_send_loop();
                        }

                        client.disconnect();
                        continue;
                    }
                }
            }

            let (number_of_bytes, _) = client.recv_from(&mut buf).expect("Failed to Receive Packet");
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