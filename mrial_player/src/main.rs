mod audio;
mod client;
mod input;
mod storage;
mod video;

use audio::AudioClient;
use cli_clipboard::{ClipboardContext, ClipboardProvider};
use client::{Client, ClientMetaData, ConnectionState};
use input::Input;
use mrial_proto::*;
use storage::{Servers, Storage};
use video::VideoThread;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{rc::Rc, thread};

use i_slint_backend_winit::WinitWindowAccessor;
use kanal::unbounded;
use log::{debug, info};
use slint::{ComponentHandle, SharedString, VecModel};

slint::include_modules!();

#[derive(PartialEq)]
pub enum ConnectionAction {
    Disconnect,
    Connect,
    Reconnect,
    Handshake,
    UpdateState,
    CloseApplication,
    Volume,
}

fn populate_servers(server_state: &Servers, app_weak: &slint::Weak<MainWindow>) {
    if let Some(servers) = server_state.get_servers() {
        let slint_servers = Rc::new(VecModel::default());
        for server in servers {
            slint_servers.push(IServer {
                name: SharedString::from(server.name),
                address: SharedString::from(server.address),
                port: server.port.into(),
                os: SharedString::from(server.os),
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
    pretty_env_logger::init_timed();

    let backend = i_slint_backend_winit::Backend::new().unwrap();
    let _ = slint::platform::set_platform(Box::new(backend));

    let app: MainWindow = MainWindow::new().unwrap();
    let app_weak = app.as_weak();

    let mut clipboard_ctx = ClipboardContext::new().unwrap();

    let (width, height) = app
        .window()
        .with_winit_window(|winit_window: &winit::window::Window| {
            let monitor = winit_window.primary_monitor().unwrap();
            let scale_factor = monitor.scale_factor();
            let size = monitor.size();
            let width = (size.width as f64 / scale_factor) as usize;
            let height = (size.height as f64 / scale_factor) as usize;

            (width, height)
        })
        .unwrap();

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    app_weak
        .unwrap()
        .global::<GlobalVars>()
        .set_app_version(VERSION.into());

    let conn_channel = unbounded::<ConnectionAction>();
    let conn_sender = conn_channel.0.clone();
    let mut client = Client::new(
        ClientMetaData {
            width,
            height,
            widths: vec![],
            heights: vec![],
        },
        conn_channel.0.clone(),
    );

    let conn_sender_clone = conn_sender.clone();
    app.window().on_close_requested(move || {
        info!("Application Close Requested");
        conn_sender_clone
            .send(ConnectionAction::CloseApplication)
            .unwrap();
        slint::CloseRequestResponse::KeepWindowShown
    });

    let mut server_state = Servers::new();
    server_state.load().unwrap();
    let mut server_state_clone = server_state.try_clone();
    populate_servers(&server_state, &app_weak);

    let server_id = Arc::new(Mutex::new(String::new()));
    let server_id_clone = server_id.clone();
    let volume = Arc::new(Mutex::new(1.0f32));
    let volume_clone = volume.clone();

    slint::invoke_from_event_loop(move || {
        let conn_sender_clone = conn_sender.clone();
        let app_weak_clone = app_weak.clone();
        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_connect(move |name| {
                app_weak_clone.unwrap()
                    .global::<VideoState>()
                    .set_connected(false);
                *server_id_clone.lock().unwrap() = name.to_string();
                conn_sender_clone.send(ConnectionAction::Connect).unwrap();
            });
        
        let conn_sender_clone = conn_sender.clone();
        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_disconnect(move || {
                conn_sender_clone
                    .send(ConnectionAction::Disconnect)
                    .unwrap();
            });

        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_volume(move |value| {
                *volume_clone.lock().unwrap() = value as f32 / 100f32;
                conn_sender.send(ConnectionAction::Volume).unwrap();
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
                    "ubuntu".to_string(),
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

        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_copy(move |address| clipboard_ctx.set_contents(address.to_string()).unwrap());
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
        video.run(app_weak.clone(), video_conn_sender, client.clone());

        let mut input = Input::new();
        input.capture(app_weak.clone(), client.clone());

        loop {
            if !client.connected() || conn_channel.1.len() > 0 {
                match conn_channel.1.try_recv_realtime().unwrap() {
                    None => {
                        if !client.connected() {
                            thread::sleep(Duration::from_millis(25));
                            continue;
                        }
                    }
                    Some(ConnectionAction::Volume) => {
                        audio.set_volume(volume.lock().unwrap().clone());
                    }
                    Some(ConnectionAction::UpdateState) => {
                        let widths = client.get_meta().widths.clone();
                        let heights = client.get_meta().heights.clone();

                        let app_weak_clone = app_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            let resolutions_model = Rc::new(VecModel::default());
                            widths.iter().zip(heights.iter()).for_each(|(w, h)| {
                                resolutions_model.push(IMrialDropdownItem {
                                    label: SharedString::from(format!("{}x{}", w, h)),
                                    value: SharedString::from(format!("{}x{}", w, h)),
                                });
                            });

                            app_weak_clone
                                .unwrap()
                                .global::<ControlPanelAdapter>()
                                .set_resolutions(resolutions_model.into());
                        });
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
                            ConnectionState::Connected => {
                                input.send_loop(&client);
                                let app_weak_clone: slint::Weak<MainWindow> = app_weak.clone();
                                let _ = app_weak.upgrade_in_event_loop(move |_| {
                                    app_weak_clone
                                        .unwrap()
                                        .global::<VideoState>()
                                        .set_connected(true);
                                });
                            },
                            ConnectionState::Connecting => {
                                thread::sleep(Duration::from_millis(1000));
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
                    Some(ConnectionAction::CloseApplication) => {
                        if client.connected() {
                            input.close_send_loop();
                        }

                        client.disconnect();
                        let app_weak_clone: slint::Weak<MainWindow> = app_weak.clone();
                        let _ = app_weak.upgrade_in_event_loop(move |_| {
                            let _ = app_weak_clone.unwrap().hide();
                        });

                        break;
                    }
                    Some(ConnectionAction::Disconnect) => {
                        if client.connected() {
                            input.close_send_loop();
                        }

                        client.disconnect();

                        // Clear stream
                        let app_weak_clone = app_weak.clone();
                        let rgb = vec![0; client.get_meta().width * client.get_meta().height * 3];
                        if let Ok(pixel_buffer) = VideoThread::rgb_to_slint_pixel_buffer(
                            &rgb,
                            client.get_meta().width as u32,
                            client.get_meta().height as u32,
                        ) {
                            let _ = slint::invoke_from_event_loop(move || {
                                app_weak_clone
                                    .unwrap()
                                    .set_video_frame(slint::Image::from_rgb8(pixel_buffer));
                            });
                        }
                        continue;
                    }
                }
            }

            match client.recv_from(&mut buf) {
                Ok((number_of_bytes, _)) => {
                    let packet_type = parse_packet_type(&buf);

                    match packet_type {
                        EPacketType::Audio => audio.play_audio_stream(&buf, number_of_bytes),
                        EPacketType::NAL => video.packet(&buf, &client, number_of_bytes),
                        _ => {}
                    }
                }
                Err(_e) => {
                    debug!("Lost Connection, Reconnecting...");
                    if client.connected() {
                        conn_channel.0.send(ConnectionAction::Reconnect).unwrap();
                    }
                }
            }
        }
    });

    app.run().unwrap();
}
