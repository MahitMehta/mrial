use kanal::{unbounded, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use crate::client::{Client, ClientMetaData};
use mrial_proto as proto;
use mrial_proto::*;
use slint::platform::PointerEventButton;

use super::ComponentHandle;
slint::include_modules!();

use super::slint_generatedMainWindow::ControlPanelFunctions as CPFunctions;
use super::slint_generatedMainWindow::VideoFunctions as KeyVideoFunctions;

pub struct Input {
    channel: (Sender<Vec<u8>>, Receiver<Vec<u8>>),
    connected: Arc<Mutex<bool>>,
}

impl Input {
    pub fn new() -> Self {
        Self {
            channel: unbounded(),
            connected: Arc::new(Mutex::new(false)),
        }
    }

    pub fn send_loop<'a>(&mut self, client: &'a Client) {
        *self.connected.lock().unwrap() = client.connected();
        let mut inner_client = client.clone();

        let receiver = self.channel.1.clone();
        let connected_clone = Arc::clone(&self.connected);

        let _handle = thread::spawn(move || {
            loop {
                if !*connected_clone.lock().unwrap() {
                    inner_client.disconnect();
                    break;
                }
                let next_input = receiver.recv().unwrap();
                if next_input[0] == EPacketType::InternalEOL as u8 {
                    inner_client.disconnect();
                    break;
                }
                inner_client.send(&next_input).unwrap();
            }
            *connected_clone.lock().unwrap() = false;
        });
    }

    pub fn close_send_loop(&self) {
        self.channel
            .0
            .send(vec![EPacketType::InternalEOL as u8])
            .unwrap();

        while *self.connected.lock().unwrap() {
            thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn video_offset(
        meta_clone: &Arc<RwLock<ClientMetaData>>,
        app_weak_clone: &slint::Weak<super::slint_generatedMainWindow::MainWindow>,
    ) -> (f32, f32, f32, f32) {
        let size = app_weak_clone.unwrap().window().size();
        let scale_factor = app_weak_clone.unwrap().window().scale_factor();

        let win_width = (size.width as f32) / scale_factor;
        let win_height = (size.height as f32) / scale_factor;

        let vid_height = meta_clone.read().unwrap().height as f32;
        let vid_width = meta_clone.read().unwrap().width as f32;

        let win_ratio = win_width / win_height;
        let vid_ratio = vid_width / vid_height;

        let mut x_offset = 0f32;
        let mut y_offset = 0f32;

        if win_ratio > vid_ratio {
            let new_width = win_height * vid_ratio;
            x_offset = (win_width - new_width) / 2f32;
        } else {
            let new_height = win_width / vid_ratio;
            y_offset = (win_height - new_height) / 2f32;
        }

        (x_offset, y_offset, win_width, win_height)
    }

    pub fn capture(
        &self,
        app_weak: slint::Weak<super::slint_generatedMainWindow::MainWindow>,
        client: Client,
    ) {
        // TODO: don't store, make access to these values dynamic
        let meta = client.get_meta_clone();

        let mut buf = [0; packet::HEADER + input::PAYLOAD];
        proto::write_header(
            EPacketType::InputState,
            0,
            (packet::HEADER + input::PAYLOAD) as u32,
            0,
            &mut buf,
        );

        let sender = self.channel.0.clone();
        let connected = Arc::clone(&self.connected);
        let meta_clone = meta.clone();

        let _ = slint::invoke_from_event_loop(move || {
            let click_sender = sender.clone();
            let click_connected = connected.clone();
            let app_weak_clone = app_weak.clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_click(move |x, y, button| {
                    if !*click_connected.lock().unwrap() {
                        return;
                    }

                    let (x_offset, y_offset, win_width, win_height) =
                        Input::video_offset(&meta_clone, &app_weak_clone);
                    if y < y_offset
                        || y > win_height - y_offset
                        || x < x_offset
                        || x > win_width - x_offset
                    {
                        return;
                    }

                    input::write_click(
                        x - x_offset,
                        y - y_offset,
                        (win_width as usize) - (x_offset as usize * 2),
                        (win_height as usize) - (y_offset as usize * 2),
                        button == PointerEventButton::Right,
                        &mut buf[HEADER..],
                    );

                    click_sender.send(buf.to_vec()).unwrap();
                    input::reset_click(&mut buf[HEADER..]);
                });

            let mouse_move_sender = sender.clone();
            let mouse_move_connected = connected.clone();
            let meta_clone = meta.clone();
            let app_weak_clone = app_weak.clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_mouse_move(move |x, y, pressed| {
                    if !*mouse_move_connected.lock().unwrap() {
                        return;
                    }

                    let (x_offset, y_offset, win_width, win_height) =
                        Input::video_offset(&meta_clone, &app_weak_clone);
                    if y < y_offset
                        || y > win_height - y_offset
                        || x < x_offset
                        || x > win_width - x_offset
                    {
                        return;
                    }

                    let mut payload = [0; input::PAYLOAD];

                    let ele_width = (win_width as usize) - (x_offset as usize * 2);
                    let ele_height = (win_height as usize) - (y_offset as usize * 2);

                    let x_percent =
                        ((x - x_offset) / (ele_width as f32) * 10000.0).round() as u16 + 1;
                    let y_percent =
                        ((y - y_offset) / (ele_height as f32) * 10000.0).round() as u16 + 1;

                    payload[10..12].copy_from_slice(&x_percent.to_be_bytes());
                    payload[12..14].copy_from_slice(&y_percent.to_be_bytes());

                    payload[14] = pressed as u8;

                    buf[HEADER..HEADER + input::PAYLOAD].copy_from_slice(&payload);
                    mouse_move_sender.send(buf.to_vec()).unwrap();
                });

            let scroll_sender = sender.clone();
            let scroll_connected = connected.clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_scroll(move |x, y| {
                    if !*scroll_connected.lock().unwrap() {
                        return;
                    }
                    let mut payload = [0; input::PAYLOAD];

                    if x == 0.0 && y == 0.0 {
                        return;
                    }

                    payload[14..16].copy_from_slice(&(x as i16).to_be_bytes());
                    payload[16..18].copy_from_slice(&(y as i16).to_be_bytes());

                    buf[HEADER..HEADER + input::PAYLOAD].copy_from_slice(&payload);
                    scroll_sender.send(buf.to_vec()).unwrap();
                });

            let key_pressed_sender = sender.clone();
            let key_pressed_connected = connected.clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_key_pressed(move |event| {
                    if !*key_pressed_connected.lock().unwrap() {
                        return;
                    }
                    let mut payload = [0; input::PAYLOAD];

                    match event.text.bytes().next() {
                        Some(key) => {
                            //buf[HEADER] = event.modifiers.control as u8;
                            payload[1] = event.modifiers.shift.into();
                            payload[2] = event.modifiers.alt.into();
                            payload[3] = event.modifiers.meta.into();
                            if key != 17 {
                                payload[8] = key.into();
                            }

                            println!("Key Pressed: {}", buf[HEADER + 8]);

                            buf[HEADER..HEADER + input::PAYLOAD].copy_from_slice(&payload);
                            key_pressed_sender.send(buf.to_vec()).unwrap();
                        }
                        None => {
                            println!("Key Pressed: None");
                        }
                    }
                });

            let key_released_sender: Sender<Vec<u8>> = sender.clone();
            let key_released_connected = connected.clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_key_released(move |event| {
                    if !*key_released_connected.lock().unwrap() {
                        return;
                    }
                    let mut payload = [0; input::PAYLOAD];

                    match event.text.bytes().next() {
                        Some(key) => {
                            //buf[HEADER] = if event.modifiers.control { event.modifiers.control as u8 + 1 } else { 0 };
                            payload[1] = if event.modifiers.shift {
                                event.modifiers.shift as u8 + 1
                            } else {
                                0
                            };
                            payload[2] = if event.modifiers.alt {
                                event.modifiers.alt as u8 + 1
                            } else {
                                0
                            };
                            payload[3] = if event.modifiers.meta {
                                event.modifiers.meta as u8 + 1
                            } else {
                                0
                            };

                            if key != 17 {
                                payload[9] = key;
                            }

                            buf[HEADER..HEADER + input::PAYLOAD].copy_from_slice(&payload);
                            key_released_sender.send(buf.to_vec()).unwrap();
                        }
                        None => {
                            println!("Key Pressed: None");
                        }
                    }
                });

            let client_state_sender: Sender<Vec<u8>> = sender.clone();
            let client_state_sender_connected = connected.clone();

            app_weak
                .unwrap()
                .global::<CPFunctions>()
                .on_state_update(move |state| {
                    if !*client_state_sender_connected.lock().unwrap() {
                        return;
                    }

                    let items: Vec<u16> = state
                        .resolution
                        .split("x")
                        .map(|x| x.parse::<u16>().unwrap())
                        .collect();
                    let (width, height) = (items[0], items[1]);

                    let mut buf = [0; CLIENT_STATE_PAYLOAD + HEADER];
                    let size = write_client_state_payload(
                        &mut buf[HEADER..],
                        ClientStatePayload { width, height },
                    );

                    write_header(
                        EPacketType::ClientState,
                        0,
                        size.try_into().unwrap(),
                        0,
                        &mut buf,
                    );

                    client_state_sender.send(buf[0..HEADER+size].to_vec()).unwrap();
                })
        });
    }
}
