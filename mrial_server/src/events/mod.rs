use std::{net::UdpSocket, thread};

use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings, Mouse
};
use mrial_proto::{input::*, packet::*};

#[cfg(target_os = "linux")]
use mouse_keyboard_input;

use crate::conn::Connections;

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
        let uinput = mouse_keyboard_input::VirtualDevice::new(
            Duration::new(0.040 as u64, 
                0), 2000
            ).unwrap();
        let enigo = Enigo::new(&Settings::default()).unwrap();
            
        Self {
            enigo,
            mouse,
            uinput,
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new() -> Self {
        use std::time::Duration;

        let mouse = mouse_rs::Mouse::new(); // requires package install on linux (libxdo-dev)
        let enigo = Enigo::new(&Settings::default()).unwrap();

        Self {
            mouse,
            enigo
        }
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
    pub fn scroll(&self, x: i32, y: i32) {
        
    }

    pub fn input(&mut self, buf: &mut [u8]) {
        if click_requested(&buf) {
            let (x, y, right) = parse_click(buf, 1440, 900);

            let _ = &self.mouse.move_to(x, y);
            if right {
                let _ = &self.enigo.button(enigo::Button::Right, enigo::Direction::Click);
            } else {
                let _ = &self.enigo.button(enigo::Button::Left, enigo::Direction::Click);
            }                        
        }
        if mouse_move_requested(&buf) {
            let x_percent =
                u16::from_be_bytes(buf[HEADER + 10..HEADER + 12].try_into().unwrap()) - 1;
            let y_percent =
                u16::from_be_bytes(buf[HEADER + 12..HEADER + 14].try_into().unwrap()) - 1;

            let x: i32 = (x_percent as f32 / 10000.0 * 1440 as f32).round() as i32;
            let y = (y_percent as f32 / 10000.0 * 900 as f32).round() as i32;

            let _ = &self.mouse.move_to(x, y);

            // TODO: handle right mouse button too
            if buf[HEADER + 14] == 1 {
                let _ = &self.enigo.button(enigo::Button::Left, enigo::Direction::Press);
            } 
        }
        if buf[HEADER + 15] != 0 || buf[HEADER + 17] != 0 {
            let x_delta = i16::from_be_bytes(buf[HEADER + 14..HEADER + 16].try_into().unwrap());
            let y_delta = i16::from_be_bytes(buf[HEADER + 16..HEADER + 18].try_into().unwrap());

            if cfg!(target_os = "linux") {
                self.scroll(x_delta as i32, y_delta as i32);
            }
        }

        if buf[HEADER] == 1 {
            self.enigo.key(Key::Control, Press).unwrap();
        } else if buf[HEADER] == 2 {
            self.enigo.key(Key::Control, Release).unwrap();
        }
        if buf[HEADER + 1] == 1 {
            self. enigo.key(Key::Shift, Press).unwrap();
        } else if buf[HEADER + 1] == 2 {
            self.enigo.key(Key::Shift, Release).unwrap();
        }

        if buf[HEADER + 2] == 1 {
            self.enigo.key(Key::Alt, Press).unwrap();
        } else if buf[HEADER + 2] == 2 {
            self.enigo.key(Key::Alt, Release).unwrap();
        }

        if buf[HEADER + 3] == 1 {
            self.enigo.key(Key::Meta, Press).unwrap();
        } else if buf[HEADER + 3] == 2 {
            self.enigo.key(Key::Meta, Release).unwrap();
        }

        if buf[HEADER + 8] != 0 {
            if buf[HEADER + 8] == 32 {
                self.enigo.key(Key::Space, enigo::Direction::Click).unwrap();
            } else if buf[HEADER + 8] == 8 {
                self.enigo.key(Key::Backspace, Press).unwrap();
            } else if buf[HEADER + 8] == 10 {
                self.enigo.key(Key::Return, enigo::Direction::Click).unwrap();
            } else if buf[HEADER + 8] >= 33 {
                // add ascii range check

                self.enigo
                    .key(Key::Unicode((buf[HEADER + 8]) as char), Press)
                    .unwrap();
            }
        }

        if buf[HEADER + 9] != 0 {
            if buf[HEADER + 9] == 32 {
                self.enigo.key(Key::Space, Release).unwrap();
            } else if buf[HEADER + 9] == 8 {
                self.enigo.key(Key::Backspace, Release).unwrap();
            } else if buf[HEADER + 9] >= 33 {
                // add ascii range check
                self.enigo
                    .key(Key::Unicode((buf[HEADER + 9]) as char), Release)
                    .unwrap();
            }
        }
    }
}

pub struct EventsThread {
}

impl EventsThread {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run(&self, socket: UdpSocket, conn: &mut Connections, headers: Vec<u8>) {
        let mut conn = conn.clone();

        let _ = thread::spawn(move || {
            
            let mut emitter = EventsEmitter::new();

            loop {
                let mut buf: [u8; MTU] = [0; MTU];
                let (_size, src) = socket.recv_from(&mut buf).unwrap();
                let packet_type = parse_packet_type(&buf);
    
                match packet_type {
                    EPacketType::SHAKE => {
                        // *attempt_reconnect_clone.lock().unwrap() = true;
                        
                        // TODO: Need to requery headers from encoder
                        conn.add_client(src, &headers);
                    }
                    EPacketType::PING => {
                        conn.ping_client(src);
                    }
                    EPacketType::DISCONNECT => {
                        conn.remove_client(src);
                    }
                    EPacketType::STATE => {
                        emitter.input(&mut buf);
                    }
                    _ => {}
                }            
            }
        });
    }
}