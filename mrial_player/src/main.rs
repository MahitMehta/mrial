mod audio;
mod client;
mod input;
mod storage;
mod video;

use mrial_proto::*;

use audio::AudioClient;
use client::{Client, ConnectionState};
use input::Input;
use storage::{Servers, Storage};
use video::VideoThread;

use kanal::unbounded;
use slint::{ComponentHandle, SharedString, VecModel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{rc::Rc, thread};

slint::include_modules!();

#[derive(PartialEq)]
pub enum ConnectionAction {
    Disconnect,
    Connect,
    Reconnect,
    Handshake,
}

fn populate_servers(server_state: &Servers, app_weak: &slint::Weak<MainWindow>) {
    if let Some(servers) = server_state.get_servers() {
        let slint_servers = Rc::new(VecModel::default());
        for server in servers {
            slint_servers.push(IServer {
                name: SharedString::from(server.name),
                address: SharedString::from(server.address),
                port: server.port.into(),
                os: SharedString::from("ubuntu"),
                ram: 24,
                storage: 40,
                vcpu: 4,
            });
        }
        app_weak
            .unwrap()
            .global::<HomePageAdapter>()
            .set_servers(slint_servers.into());
    }
}

fn main() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    let app: MainWindow = MainWindow::new().unwrap();
    let app_weak = app.as_weak();
    app_weak
        .unwrap()
        .global::<GlobalVars>()
        .set_app_version(VERSION.into());

    let mut client = Client::new();
    let conn_channel = unbounded::<ConnectionAction>();
    let conn_sender = conn_channel.0.clone();

    let mut server_state = Servers::new();
    server_state.load().unwrap();
    let mut server_state_clone = server_state.try_clone();

    let server_id = Arc::new(Mutex::new(String::new()));
    let server_id_clone = server_id.clone();

    populate_servers(&server_state, &app_weak);

    slint::invoke_from_event_loop(move || {
        let conn_sender_clone = conn_sender.clone();
        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_connect(move |name| {
                *server_id_clone.lock().unwrap() = name.to_string();
                conn_sender_clone.send(ConnectionAction::Connect).unwrap();
            });

        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_disconnect(move || {
                conn_sender.send(ConnectionAction::Disconnect).unwrap();
            });

        let app_weak_clone = app_weak.clone();
        let mut server_state_create_clone = server_state_clone.try_clone();
        app_weak
            .unwrap()
            .global::<CreateServerFunctions>()
            .on_add(move |name, ip_addr, port| {
                server_state_create_clone.add(
                    name.to_string(),
                    ip_addr.to_string(),
                    port.parse::<u16>().unwrap(),
                );

                populate_servers(&server_state_create_clone, &app_weak_clone);
                server_state_create_clone.save().unwrap();
            });

        let app_weak_clone = app_weak.clone();
        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_delete(move |name| {
                server_state_clone.delete(name.to_string());
                populate_servers(&server_state_clone, &app_weak_clone);
                server_state_clone.save().unwrap();
            });
    })
    .unwrap();

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
                        continue;
                    }
                    Some(ConnectionAction::Handshake) => {
                        client.connect();

                        match client.connection_state() {
                            ConnectionState::Connected => input.send_loop(&client),
                            ConnectionState::Connecting => {
                                conn_channel.0.send(ConnectionAction::Handshake).unwrap();
                                continue;
                            }
                            _ => continue,
                        }
                    }
                    Some(ConnectionAction::Reconnect) => {
                        client.connect();
                        if !client.connected() {
                            continue;
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

            let (number_of_bytes, _) = client
                .recv_from(&mut buf)
                .expect("Failed to Receive Packet");
            let packet_type = parse_packet_type(&buf);

            match packet_type {
                EPacketType::AUDIO => audio.play_audio_stream(&buf, number_of_bytes),
                EPacketType::NAL => video.packet(&buf, &client, number_of_bytes),
                _ => {}
            }
        }
    });

    app.run().unwrap();
}
