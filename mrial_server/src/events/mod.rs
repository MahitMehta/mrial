use std::thread;

use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Mouse, Settings,
};
use kanal::Sender;
use mrial_proto::{input::*, packet::*, parse_client_state_payload};

#[cfg(target_os = "linux")]
use mouse_keyboard_input;

use super::{conn::Connection, VideoServerActions};

pub struct EventsEmitter {
    enigo: Enigo,
    mouse: mouse_rs::Mouse,

    #[cfg(target_os = "linux")]
    uinput: mouse_keyboard_input::VirtualDevice,
}

impl EventsEmitter {
    #[cfg(target_os = "linux")]
    pub fn new() -> Self {
        use std::time::Duration;

        let mouse = mouse_rs::Mouse::new(); // requires package install on linux (libxdo-dev)
        let uinput =
            mouse_keyboard_input::VirtualDevice::new(Duration::new(0.040 as u64, 0), 2000).unwrap();
        let enigo = Enigo::new(&Settings::default()).unwrap();

        Self {
            enigo,
            mouse,
            uinput,
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new() -> Self {
        let mouse = mouse_rs::Mouse::new(); // requires package install on linux (libxdo-dev)
        let enigo = Enigo::new(&Settings::default()).unwrap();

        Self { mouse, enigo }
    }

    // sudo apt install libudev-dev libevdev-dev libhidapi-dev
    // sudo usermod -a -G input user
    // sudo reboot

    #[cfg(target_os = "linux")]
    pub fn scroll(&mut self, x: i32, y: i32) {
        if x != 0 {
            let _ = &self.uinput.scroll_x(-x * 3);
        }

        if y != 0 {
            let _ = &self.uinput.scroll_y(-y * 3);
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn scroll(&self, x: i32, y: i32) {}

    pub fn input(&mut self, buf: &mut [u8], width: usize, height: usize) {
        if click_requested(&buf) {
            let (x, y, right) = parse_click(buf, width, height);

            let _ = &self.mouse.move_to(x, y);
            if right {
                let _ = &self
                    .enigo
                    .button(enigo::Button::Right, enigo::Direction::Click);
            } else {
                let _ = &self
                    .enigo
                    .button(enigo::Button::Left, enigo::Direction::Click);
            }
        }
        if mouse_move_requested(&buf) {
            let x_percent =
                u16::from_be_bytes(buf[10..12].try_into().unwrap()) - 1;
            let y_percent =
                u16::from_be_bytes(buf[12..14].try_into().unwrap()) - 1;

            let x: i32 = (x_percent as f32 / 10000.0 * width as f32).round() as i32;
            let y = (y_percent as f32 / 10000.0 * height as f32).round() as i32;

            let _ = &self.mouse.move_to(x, y);

            // TODO: handle right mouse button too
            if buf[14] == 1 {
                let _ = &self
                    .enigo
                    .button(enigo::Button::Left, enigo::Direction::Press);
            }
        }
        if buf[15] != 0 || buf[17] != 0 {
            let x_delta = i16::from_be_bytes(buf[14..16].try_into().unwrap());
            let y_delta = i16::from_be_bytes(buf[16..18].try_into().unwrap());

            if cfg!(target_os = "linux") {
                self.scroll(x_delta as i32, y_delta as i32);
            }
        }

        if buf[0] == 1 {
            self.enigo.key(Key::Control, Press).unwrap();
        } else if buf[0] == 2 {
            self.enigo.key(Key::Control, Release).unwrap();
        }
        if buf[1] == 1 {
            self.enigo.key(Key::Shift, Press).unwrap();
        } else if buf[1] == 2 {
            self.enigo.key(Key::Shift, Release).unwrap();
        }

        if buf[2] == 1 {
            self.enigo.key(Key::Alt, Press).unwrap();
        } else if buf[2] == 2 {
            self.enigo.key(Key::Alt, Release).unwrap();
        }

        if buf[3] == 1 {
            self.enigo.key(Key::Meta, Press).unwrap();
        } else if buf[3] == 2 {
            self.enigo.key(Key::Meta, Release).unwrap();
        }

        if buf[8] != 0 {
            if buf[8] == 32 {
                self.enigo.key(Key::Space, enigo::Direction::Click).unwrap();
            } else if buf[8] == 8 {
                self.enigo.key(Key::Backspace, Press).unwrap();
            } else if buf[8] == 10 {
                self.enigo
                    .key(Key::Return, enigo::Direction::Click)
                    .unwrap();
            } else if buf[8] >= 33 {
                // add ascii range check

                self.enigo
                    .key(Key::Unicode((buf[8]) as char), Press)
                    .unwrap();
            }
        }

        if buf[9] != 0 {
            if buf[9] == 32 {
                self.enigo.key(Key::Space, Release).unwrap();
            } else if buf[9] == 8 {
                self.enigo.key(Key::Backspace, Release).unwrap();
            } else if buf[9] >= 33 {
                // add ascii range check
                self.enigo
                    .key(Key::Unicode((buf[9]) as char), Release)
                    .unwrap();
            }
        }
    }
}

pub struct EventsThread {}

impl EventsThread {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run(
        &self,
        conn: &mut Connection,
        headers: Vec<u8>,
        video_server_ch_sender: Sender<VideoServerActions>,
    ) {
        let mut conn = conn.clone();
        let _ = thread::spawn(move || {
            let mut emitter = EventsEmitter::new();

            loop {
                let mut buf = [0u8; MTU];
                let (size, src) = conn.recv_from(&mut buf).unwrap();
                let packet_type = parse_packet_type(&buf);

                match packet_type {
                    EPacketType::SHAKE => {
                        if let Ok(meta ) = parse_client_state_payload(&mut buf[HEADER..size]) {
                            conn.set_dimensions(
                                meta.width.try_into().unwrap(), 
                                meta.height.try_into().unwrap()
                            );
                            video_server_ch_sender
                                .send(VideoServerActions::ConfigUpdate)
                                .unwrap();
                            // TODO: Need to requery headers from encoder
                            conn.add_client(src, &headers);
                        };
                    }
                    EPacketType::CLIENT_STATE => {
                        if let Ok(meta ) = parse_client_state_payload(&mut buf[HEADER..size]) {
                            conn.set_dimensions(
                                meta.width.try_into().unwrap(), 
                                meta.height.try_into().unwrap()
                            );
                            video_server_ch_sender
                                .send(VideoServerActions::ConfigUpdate)
                                .unwrap();
                        };
                    }
                    EPacketType::PING => {
                        conn.client_pinged(src);
                    }
                    EPacketType::DISCONNECT => {
                        conn.remove_client(src);
                        if !conn.has_clients() {
                            video_server_ch_sender
                                .send(VideoServerActions::Inactive)
                                .unwrap();
                        }
                    }
                    EPacketType::INPUT_STATE => {
                        emitter.input(
                            &mut buf[HEADER..], 
                            conn.get_meta().width, 
                            conn.get_meta().height
                        );
                    }
                    _ => {}
                }
            }
        });
    }
}
