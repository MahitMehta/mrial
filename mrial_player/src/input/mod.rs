use input::{Key, KeyEvent};
use kanal::{unbounded, Receiver, Sender};
use log::debug;
use slint::SharedString;
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
                if parse_packet_type(&next_input) == EPacketType::InternalEOL {
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
        let mut buf = [0; packet::HEADER + input::PAYLOAD];
        proto::write_header(
            EPacketType::InputState,
            0,
            input::PAYLOAD as u32,
            0,
            &mut buf,
        );

        let sender = self.channel.0.clone();
        let connected = Arc::clone(&self.connected);

        let _ = slint::invoke_from_event_loop(move || {
            let click_sender = sender.clone();
            let click_connected = connected.clone();
            let app_weak_clone = app_weak.clone();
            let meta = client.get_meta_clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_click(move |x, y, button| {
                    if !*click_connected.lock().unwrap() {
                        return;
                    }

                    let (x_offset, y_offset, win_width, win_height) =
                        Input::video_offset(&meta, &app_weak_clone);
                    if y < y_offset
                        || y > win_height - y_offset
                        || x < x_offset
                        || x > win_width - x_offset
                    {
                        return;
                    }

                    input::write_click(
                        &mut buf[HEADER..],
                        x - x_offset,
                        y - y_offset,
                        win_width - x_offset * 2.0,
                        win_height - y_offset * 2.0,
                        button == PointerEventButton::Right,
                    );

                    click_sender.send(buf.to_vec()).unwrap();
                    input::reset_click(&mut buf[HEADER..]);
                });

            let mouse_move_sender = sender.clone();
            let mouse_move_connected = connected.clone();
            let app_weak_clone = app_weak.clone();
            let meta = client.get_meta_clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_mouse_move(move |x, y, pressed| {
                    if !*mouse_move_connected.lock().unwrap() {
                        return;
                    }

                    let (x_offset, y_offset, win_width, win_height) =
                        Input::video_offset(&meta, &app_weak_clone);
                    if y < y_offset
                        || y > win_height - y_offset
                        || x < x_offset
                        || x > win_width - x_offset
                    {
                        return;
                    }

                    input::write_mouse_move(
                        &mut buf[HEADER..],
                        x - x_offset,
                        y - y_offset,
                        win_width - x_offset * 2.0,
                        win_height - y_offset * 2.0,
                        pressed,
                    );

                    mouse_move_sender.send(buf.to_vec()).unwrap();
                });

            let scroll_sender = sender.clone();
            let scroll_connected = connected.clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_scroll(move |delta_x, delta_y| {
                    if !*scroll_connected.lock().unwrap() {
                        return;
                    }

                    if delta_x == 0.0 && delta_y == 0.0 {
                        return;
                    }

                    input::write_scroll(&mut buf[HEADER..], delta_x as i16, delta_y as i16);
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

                    buf[HEADER + 0] =
                        if event.text == SharedString::from(slint::platform::Key::Meta) {
                            KeyEvent::Press.into() // Mac: Command Key
                        } else {
                            KeyEvent::None.into()
                        };
                    buf[HEADER + 1] =
                        if event.text == SharedString::from(slint::platform::Key::Shift) {
                            KeyEvent::Press.into()
                        } else {
                            KeyEvent::None.into()
                        };
                    buf[HEADER + 2] = if event.text == SharedString::from(slint::platform::Key::Alt)
                    {
                        KeyEvent::Press.into()
                    } else {
                        KeyEvent::None.into()
                    };
                    buf[HEADER + 3] =
                        if event.text == SharedString::from(slint::platform::Key::Control) {
                            KeyEvent::Press.into()
                        } else {
                            KeyEvent::None.into()
                        };
                    debug!("Pressed Modifiers: {:?}", &buf[HEADER..HEADER + 4]);

                    if event.text == SharedString::from(slint::platform::Key::DownArrow) {
                        buf[HEADER + 8] = Key::DownArrow.into();
                    } else if event.text == SharedString::from(slint::platform::Key::UpArrow) {
                        buf[HEADER + 8] = Key::UpArrow.into();
                    } else if event.text == SharedString::from(slint::platform::Key::LeftArrow) {
                        buf[HEADER + 8] = Key::LeftArrow.into();
                    } else if event.text == SharedString::from(slint::platform::Key::RightArrow) {
                        buf[HEADER + 8] = Key::RightArrow.into();
                    } else {
                        // TODO: Only first byte of key, need to handle multi-byte keys (emoji, etc)
                        let mut key_bytes = event.text.bytes();
                        if key_bytes.len() == 1 {
                            let key = key_bytes.next().unwrap();
                            if key != 17 && key != 16 && key != 18 && key != 23 {
                                buf[HEADER + 8] = key;
                            }
                        } else {
                            debug!("Original Key Pressed Input: {:?}", event.text);
                        }
                    }

                    debug!("Key Pressed: {}", buf[HEADER + 8]);

                    key_pressed_sender.send(buf.to_vec()).unwrap();
                    buf[HEADER + 8] = 0; // Reset key
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

                    buf[HEADER + 0] =
                        if event.text == SharedString::from(slint::platform::Key::Meta) {
                            KeyEvent::Release.into() // Mac: Command Key
                        } else {
                            KeyEvent::None.into()
                        };
                    buf[HEADER + 1] =
                        if event.text == SharedString::from(slint::platform::Key::Shift) {
                            KeyEvent::Release.into()
                        } else {
                            KeyEvent::None.into()
                        };
                    buf[HEADER + 2] = if event.text == SharedString::from(slint::platform::Key::Alt)
                    {
                        KeyEvent::Release.into()
                    } else {
                        KeyEvent::None.into()
                    };
                    buf[HEADER + 3] =
                        if event.text == SharedString::from(slint::platform::Key::Control) {
                            KeyEvent::Release.into()
                        } else {
                            KeyEvent::None.into()
                        };
                    debug!(
                        "Released Modifiers Keys: {:?}",
                        &buf[HEADER + 0..HEADER + 4]
                    );

                    if event.text == SharedString::from(slint::platform::Key::DownArrow) {
                        buf[HEADER + 9] = Key::DownArrow.into();
                    } else if event.text == SharedString::from(slint::platform::Key::UpArrow) {
                        buf[HEADER + 9] = Key::UpArrow.into();
                    } else if event.text == SharedString::from(slint::platform::Key::LeftArrow) {
                        buf[HEADER + 9] = Key::LeftArrow.into();
                    } else if event.text == SharedString::from(slint::platform::Key::RightArrow) {
                        buf[HEADER + 9] = Key::RightArrow.into();
                    } else {
                        let mut key_bytes = event.text.bytes();
                        if key_bytes.len() == 1 {
                            let key = key_bytes.next().unwrap();
                            if key != 17 && key != 16 && key != 18 && key != 23 {
                                buf[HEADER + 9] = key;
                            }
                        } else {
                            debug!("Original Key Released Input: {:?}", event.text);
                        }
                    }

                    debug!("Key Released: {}", buf[HEADER + 9]);
                    key_released_sender.send(buf.to_vec()).unwrap();
                    buf[HEADER + 9] = 0; // Reset key
                });

            let client_state_sender: Sender<Vec<u8>> = sender.clone();
            let client_state_sender_connected = connected.clone();
            let sym_key = client.get_sym_key();
            let mut client_clone = client.clone();

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

                    let mut buf = [0; MTU];

                    client_clone.set_meta_via_state(&state);
                    let client_state = ClientStatePayload {
                        width,
                        height,
                        muted: state.muted,
                        opus: state.opus,
                    };

                    if let Ok(sym_key) = sym_key.read() {
                        let sym_key = sym_key.clone();
                        debug!("Client State: {:?}", client_state);

                        let size = match ClientStatePayload::write_payload(
                            &mut buf[HEADER..],
                            sym_key,
                            &client_state,
                        ) {
                            Ok(size) => size,
                            Err(e) => {
                                debug!("Error writing client state payload: {:?}", e);
                                return;
                            }
                        };

                        write_header(
                            EPacketType::ClientState,
                            0,
                            size as u32,
                            0,
                            &mut buf[0..HEADER + size],
                        );

                        client_state_sender
                            .send(buf[0..HEADER + size].to_vec())
                            .unwrap();
                    }
                })
        });
    }
}
