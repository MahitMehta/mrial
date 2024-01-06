mod audio; 
mod client; 
mod video; 
mod input;
mod storage; 

use mrial_proto::*;

use audio::AudioClient;
use client::{Client, ConnectionState};
use input::Input;
use video::VideoThread; 
use storage::{Servers, Storage};

use kanal::unbounded;
use std::sync::{Mutex, Arc};
use std::{thread, rc::Rc};
use std::time::Duration;
use slint::{ComponentHandle, SharedString, VecModel};

slint::include_modules!();

#[derive(PartialEq)]
pub enum ConnectionAction {
    Disconnect,
    Connect,
    Reconnect,
    Handshake
}

fn populate_servers(server_state: &Servers, app_weak: &slint::Weak<MainWindow>) {    
    if let Some(servers) = server_state.get_servers() {
        let slint_servers = Rc::new(VecModel::default());
        for server in servers {
            slint_servers.push(IServer {
                name: SharedString::from(server.name),
                address: SharedString::from(server.address),
                port: server.port.into(),
                shareable: false,
                os: SharedString::from("macos"),
                ram: 8,
                storage: 512,
                vcpu: 8
            });
        }
        app_weak.unwrap().global::<HomePageAdapter>().set_servers(slint_servers.into());
    }
}

fn main() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    let app: MainWindow = MainWindow::new().unwrap();
    let app_weak = app.as_weak();
    app_weak.unwrap().global::<GlobalVars>().set_app_version(VERSION.into());

    let mut client = Client::new();
    let conn_channel =  unbounded::<ConnectionAction>();
    let conn_sender = conn_channel.0.clone();
    
    let mut server_state = Servers::new();
    server_state.load().unwrap();
    let mut server_state_clone = server_state.try_clone();

    let server_id = Arc::new(Mutex::new(String::new()));
    let server_id_clone = server_id.clone();

    populate_servers(&server_state, &app_weak);

    slint::invoke_from_event_loop(move || {
        let conn_sender_clone = conn_sender.clone();
        app_weak.unwrap().global::<VideoFunctions>().on_connect(move |name| {
            *server_id_clone.lock().unwrap() = name.to_string(); 
            conn_sender_clone.send(ConnectionAction::Connect).unwrap();
        });

        app_weak.unwrap().global::<VideoFunctions>().on_disconnect(move || {
            conn_sender.send(ConnectionAction::Disconnect).unwrap();
        });

        app_weak.unwrap().global::<CreateServerFunctions>().on_add(move |name, ip_addr, port| {
            server_state_clone.add(
                name.to_string(), 
                ip_addr.to_string(),
                port.parse::<u16>().unwrap()
            );
            
            populate_servers(&server_state_clone, &app_weak);
            server_state_clone.save().unwrap();
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
        input.capture(app_weak.clone());

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
                        let server_id = server_id.lock().unwrap().clone(); 
                        if let Some(server) = server_state.find_server(server_id) {
                            client.set_socket_address(server.address, server.port);
                        }

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
            let packet_type = parse_packet_type(&buf);

            match packet_type {
                EPacketType::AUDIO => audio.play_audio_stream(&buf, number_of_bytes),
                EPacketType::NAL => video.packet(&buf, number_of_bytes),
                _ => {}
            }
        }     
    });
     
    app.run().unwrap();
}