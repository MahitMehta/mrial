mod audio;
mod client;
mod input;
mod video;

use audio::{AudioClientThread, AudioPacket};
use cli_clipboard::{ClipboardContext, ClipboardProvider};
use client::{Client, ClientMetaData, ConnectionState};
use input::Input;
use mrial_fs::Server;
use mrial_fs::{storage::StorageMultiType, Servers, User, Users};
use mrial_proto::*;
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

fn populate_users(users: Vec<User>, app_weak: &slint::Weak<MainWindow>) {
    let slint_users = Rc::new(VecModel::default());
    for user in users {
        slint_users.push(IUser {
            username: SharedString::from(user.username),
            enabled: true,
        });
    }
    app_weak
        .unwrap()
        .global::<HostingAdapter>()
        .set_users(slint_users.into());
}

fn populate_servers(servers: Vec<Server>, app_weak: &slint::Weak<MainWindow>) {
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
        .global::<HomeAdapter>()
        .set_servers(slint_servers.into());
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
        .with_winit_window(|winit_window| {
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
        ClientMetaData::default(),
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

    let volume = Arc::new(Mutex::new(1.0f32));
    let volume_clone = volume.clone();

    // =========== Start USER Management ============

    let mut users_storage = Users::new();
    match users_storage.load() {
        Ok(_) => {
            debug!("Users Loaded");

            let users = users_storage.users.get().unwrap();
            populate_users(users, &app_weak)
        }
        Err(e) => {
            debug!("Failed to Load Users: {}", e);
        }
    }

    let app_weak_clone = app_weak.clone();
    slint::invoke_from_event_loop(move || {
        let mut users_storage_copy = users_storage.clone();
        let app_weak_add_clone = app_weak_clone.clone();
        app_weak_clone
            .unwrap()
            .global::<HostingFunctions>()
            .on_add_user(move |username, pass| {
                match users_storage_copy.add(User {
                    username: username.to_string(),
                    pass: pass.to_string(),
                }) {
                    Ok(_) => {
                        if let Err(e) = users_storage_copy.save() {
                            // reload users from disk because of error
                            users_storage_copy.users.load().unwrap();
                            debug!("Failed to Add User: {}", e);
                        } else {
                            let users = users_storage_copy.users.get().unwrap();
                            populate_users(users, &app_weak_add_clone);

                            debug!("User Added: {}", username);
                        }
                    }
                    Err(e) => {
                        debug!("Failed to Add User: {}", e);
                    }
                }
            });
        app_weak_clone
            .unwrap()
            .global::<HostingFunctions>()
            .on_remove_user(
                move |username| match users_storage.remove(username.to_string()) {
                    Ok(_) => {
                        if let Err(e) = users_storage.save() {
                            // reload users from disk because of error
                            users_storage.users.load().unwrap();
                            debug!("Failed to Remove User: {}", e);
                        } else {
                            let users = users_storage.users.get().unwrap();
                            populate_users(users, &app_weak_clone);

                            debug!("User Removed: {}", username);
                        }
                    }
                    Err(e) => {
                        debug!("Failed to Remove User: {}", e);
                    }
                },
            );
    })
    .unwrap();

    // =========== END USER Management ============
    // --------------------------------------------
    // =========== Start Server Functions Management ============

    let mut servers_storage = Servers::new();
    let mut servers_storage_clone = servers_storage.clone();
    match servers_storage.load() {
        Ok(_) => {
            debug!("Servers Loaded");

            let servers = servers_storage.servers.get().unwrap();
            populate_servers(servers, &app_weak)
        }
        Err(e) => {
            debug!("Failed to Load Servers: {}", e);
        }
    }

    let server_id = Arc::new(Mutex::new(String::new()));
    let server_id_clone = server_id.clone();

    slint::invoke_from_event_loop(move || {
        let conn_sender_clone = conn_sender.clone();
        let app_weak_clone = app_weak.clone();
        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_connect(move |name| {
                app_weak_clone
                    .unwrap()
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
        let mut servers_storage_remove_clone = servers_storage_clone.clone();
        app_weak.unwrap().global::<CreateServerFunctions>().on_add(
            move |name, ip_addr, port, username, pass| match servers_storage_clone.add(Server {
                name: name.to_string(),
                address: ip_addr.to_string(),
                port: port.parse::<u16>().unwrap(),
                os: "ubuntu".to_string(),
                username: username.to_string(),
                pass: pass.to_string(),
            }) {
                Ok(_) => {
                    if let Err(e) = servers_storage_clone.save() {
                        servers_storage_clone.servers.load().unwrap();
                        debug!("Failed to Add Server: {}", e);
                    } else {
                        let servers = servers_storage_clone.servers.get().unwrap();
                        populate_servers(servers, &app_weak_clone);

                        debug!("Server Added: {}", username);
                    }
                }
                Err(e) => {
                    debug!("Failed to Add User: {}", e);
                }
            },
        );

        let app_weak_clone = app_weak.clone();
        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_delete(
                move |name| match servers_storage_remove_clone.remove(name.to_string()) {
                    Ok(_) => {
                        if let Err(e) = servers_storage_remove_clone.save() {
                            servers_storage_remove_clone.servers.load().unwrap();
                            debug!("Failed to Remove Server: {}", e);
                        } else {
                            let servers = servers_storage_remove_clone.servers.get().unwrap();
                            populate_servers(servers, &app_weak_clone);

                            debug!("Server Removed: {}", name);
                        }
                    }
                    Err(e) => {
                        debug!("Failed to Remove Server: {}", e);
                    }
                },
            );

        app_weak
            .unwrap()
            .global::<ServerFunctions>()
            .on_copy(move |address| clipboard_ctx.set_contents(address.to_string()).unwrap());
    })
    .unwrap();

    // =========== End Server Functions Management ============

    let app_weak = app.as_weak();

    let _conn: thread::JoinHandle<_> = thread::spawn(move || {
        let mut buf: [u8; MTU] = [0; MTU];

        let (_stream, handle) = rodio::OutputStream::try_default().unwrap();
        let sink = rodio::Sink::try_new(&handle).unwrap();

        let (audio_sender, audio_receiver) = unbounded::<AudioPacket>();
        let mut audio_client = AudioClientThread::new(sink, audio_sender);
        if let Err(e) = audio_client.run(audio_receiver, client.clone()) {
            debug!("Failed to run audio client: {}", e);
        }

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
                        audio_client.set_volume(volume.lock().unwrap().clone());
                    }
                    Some(ConnectionAction::UpdateState) => {
                        let meta_lock = client.get_meta();
                        let widths = meta_lock.widths.clone();
                        let heights = meta_lock.heights.clone();
                        let username = meta_lock.server.username.clone();
                        let opus = meta_lock.opus;
                        let muted = meta_lock.muted;

                        let server_name = meta_lock.server.name.clone();

                        let app_weak_clone = app_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            let resolutions_model = Rc::new(VecModel::default());
                            widths.iter().zip(heights.iter()).for_each(|(w, h)| {
                                resolutions_model.push(IMrialDropdownItem {
                                    label: SharedString::from(format!("{}x{}", w, h)),
                                    value: SharedString::from(format!("{}x{}", w, h)),
                                });
                            });

                            println!("Is Opus: {}", opus);
                            println!("Is Muted: {}", muted);

                            app_weak_clone
                                .unwrap()
                                .global::<ControlPanelAdapter>()
                                .set_resolutions(resolutions_model.into());
                            app_weak_clone
                                .unwrap()
                                .global::<ControlPanelAdapter>()
                                .set_opus(opus);
                            app_weak_clone
                                .unwrap()
                                .global::<ControlPanelAdapter>()
                                .set_muted(muted);
                            app_weak_clone
                                .unwrap()
                                .global::<BarialState>()
                                .set_user(SharedString::from(username));
                            app_weak_clone
                                .unwrap()
                                .global::<BarialState>()
                                .set_server_name(SharedString::from(server_name));
                        });
                    }
                    Some(ConnectionAction::Connect) => {
                        let server_id = server_id.lock().unwrap().clone();
                        if let Some(server) = servers_storage.find(server_id) {
                            client.set_socket_address(&server.address, server.port);
                            let meta = client.get_meta_clone();
                            meta.write().unwrap().server = server.clone();

                            let app_weak_clone = app_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                app_weak_clone
                                    .unwrap()
                                    .global::<BarialState>()
                                    .set_user(SharedString::from(server.username));
                                app_weak_clone
                                    .unwrap()
                                    .global::<BarialState>()
                                    .set_server_name(SharedString::from(server.name));
                            });
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
                            }
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
                        EPacketType::NAL | EPacketType::XOR => {
                            video.packet(&buf, &client, number_of_bytes)
                        }
                        EPacketType::AudioPCM | EPacketType::AudioOpus => {
                            if let Err(e) = audio_client.packet(packet_type, &buf, number_of_bytes)
                            {
                                debug!("Failed to play audio: {}", e);
                            }
                        }
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
