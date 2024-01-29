use kanal::{unbounded, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::client::Client;
use mrial_proto as proto;
use mrial_proto::*;
use slint::platform::PointerEventButton;

use super::ComponentHandle;
slint::include_modules!();

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

    pub fn capture(
        &self,
        app_weak: slint::Weak<super::slint_generatedMainWindow::MainWindow>,
        client: Client,
    ) {
        // TODO: don't store, make access to these values dynamic
        let width: usize = client.get_meta().width;
        let height = client.get_meta().height;
        
        let mut buf = [0; packet::HEADER + input::PAYLOAD];
        proto::write_header(
            EPacketType::STATE,
            0,
            (packet::HEADER + input::PAYLOAD) as u32,
            0,
            &mut buf,
        );

        let sender = self.channel.0.clone();
        let connected = Arc::clone(&self.connected);

        let _ = slint::invoke_from_event_loop(move || {
            let click_sender = sender.clone();
            let click_connected = connected.clone();

            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_click(move |x, y, button| {
                    if !*click_connected.lock().unwrap() {
                        return;
                    }

                    
                    if y < 45f32 {
                        return;
                    }
                    println!("x: {}, y: {}", x, y - 45f32);     

                    input::write_click(
                        x,
                        y - 45f32,
                        width, // TODO: should be width of video element
                        height - 90,
                        button == PointerEventButton::Right,
                        &mut buf[HEADER..],
                    );

                    click_sender.send(buf.to_vec()).unwrap();
                    input::reset_click(&mut buf[HEADER..]);
                });

            let mouse_move_sender = sender.clone();
            let mouse_move_connected = connected.clone();
            app_weak
                .unwrap()
                .global::<KeyVideoFunctions>()
                .on_mouse_move(move |x, y, pressed| {
                    if !*mouse_move_connected.lock().unwrap() {
                        return;
                    }
                    let mut payload = [0; input::PAYLOAD];

                    let x_percent = (x / (width as f32) * 10000.0).round() as u16 + 1;
                    let y_percent = (y / (height as f32) * 10000.0).round() as u16 + 1;

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
        });
    }
}
