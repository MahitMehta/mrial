use std::{net::SocketAddr, thread};

use enigo::{
    Direction::{Press, Release},
    Enigo, Keyboard, Mouse, Settings,
};
use kanal::Sender;
use log::debug;
use mrial_proto::{input::*, packet::*, ClientStatePayload, JSONPayloadSE};

#[cfg(target_os = "linux")]
use mouse_keyboard_input;

use super::{conn::Connection, VideoServerAction};

pub struct EventsEmitter {
    enigo: Enigo,
    mouse: mouse_rs::Mouse,

    left_mouse_held: bool,

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
            left_mouse_held: false,
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new() -> Self {
        let mouse = mouse_rs::Mouse::new(); // requires package install on linux (libxdo-dev)
        let enigo = Enigo::new(&Settings::default()).unwrap();

        Self {
            mouse,
            enigo,
            left_mouse_held: false,
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
    pub fn scroll(&self, _x: i32, _y: i32) {}

    pub fn input(&mut self, buf: &mut [u8], width: usize, height: usize) {
        if click_requested(buf) {
            let (x, y, right) = parse_click(buf, width, height);

            let _ = &self.mouse.move_to(x, y);
            if right {
                let _ = &self
                    .enigo
                    .button(enigo::Button::Right, enigo::Direction::Click);
            } else {
                self.left_mouse_held = !self.left_mouse_held;
                let _ = &self
                    .enigo
                    .button(enigo::Button::Left, enigo::Direction::Click);
            }
        }

        if mouse_move_requested(buf) {
            let (x, y, pressed) = parse_mouse_move(buf, width as f32, height as f32);
            let _ = &self.mouse.move_to(x, y);

            if pressed && !self.left_mouse_held {
                self.left_mouse_held = true;
                let _ = &self
                    .enigo
                    .button(enigo::Button::Left, enigo::Direction::Press);
            }
        }

        if scroll_requested(&buf) {
            let x_delta = i16::from_be_bytes(buf[14..16].try_into().unwrap());
            let y_delta = i16::from_be_bytes(buf[16..18].try_into().unwrap());

            if cfg!(target_os = "linux") {
                self.scroll(x_delta as i32, y_delta as i32);
            }
        }

        if is_control_pressed(buf) {
            self.enigo.key(enigo::Key::Control, Press).unwrap();
        } else if is_control_released(buf) {
            self.enigo.key(enigo::Key::Control, Release).unwrap();
        }

        if is_shift_pressed(buf) {
            self.enigo.key(enigo::Key::Shift, Press).unwrap();
        } else if is_shift_released(buf) {
            self.enigo.key(enigo::Key::Shift, Release).unwrap();
        }

        if is_alt_pressed(buf) {
            self.enigo.key(enigo::Key::Alt, Press).unwrap();
        } else if is_alt_released(buf) {
            self.enigo.key(enigo::Key::Alt, Release).unwrap();
        }

        if is_meta_pressed(buf) {
            self.enigo.key(enigo::Key::Meta, Press).unwrap();
        } else if is_meta_released(buf) {
            self.enigo.key(enigo::Key::Meta, Release).unwrap();
        }

        match Key::from(buf[8]) {
            Key::None => {}
            Key::Backspace => {
                self.enigo.key(enigo::Key::Backspace, Press).unwrap();
            }
            Key::DownArrow => {
                self.enigo.key(enigo::Key::DownArrow, Press).unwrap();
            }
            Key::UpArrow => {
                self.enigo.key(enigo::Key::UpArrow, Press).unwrap();
            }
            Key::LeftArrow => {
                self.enigo.key(enigo::Key::LeftArrow, Press).unwrap();
            }
            Key::RightArrow => {
                self.enigo.key(enigo::Key::RightArrow, Press).unwrap();
            }
            Key::Space => {
                self.enigo
                    .key(enigo::Key::Space, enigo::Direction::Press)
                    .unwrap();
            }
            Key::Tab => {
                self.enigo
                    .key(enigo::Key::Tab, enigo::Direction::Press)
                    .unwrap();
            }
            Key::Return => {
                self.enigo
                    .key(enigo::Key::Return, enigo::Direction::Click)
                    .unwrap();
            }
            Key::Unicode => {
                self.enigo
                    .key(enigo::Key::Unicode((buf[8]) as char), Press)
                    .unwrap();
            }
        }

        match Key::from(buf[9]) {
            Key::None => {}
            Key::Backspace => {
                self.enigo.key(enigo::Key::Backspace, Release).unwrap();
            }
            Key::Space => {
                self.enigo
                    .key(enigo::Key::Space, enigo::Direction::Release)
                    .unwrap();
            }
            Key::DownArrow => {
                self.enigo.key(enigo::Key::DownArrow, Release).unwrap();
            }
            Key::UpArrow => {
                self.enigo.key(enigo::Key::UpArrow, Release).unwrap();
            }
            Key::LeftArrow => {
                self.enigo.key(enigo::Key::LeftArrow, Release).unwrap();
            }
            Key::RightArrow => {
                self.enigo.key(enigo::Key::RightArrow, Release).unwrap();
            }
            Key::Tab => {
                self.enigo
                    .key(enigo::Key::Tab, enigo::Direction::Release)
                    .unwrap();
            }
            Key::Return => {}
            Key::Unicode => {
                self.enigo
                    .key(enigo::Key::Unicode((buf[9]) as char), Release)
                    .unwrap();
            }
        }
    }
}

pub struct EventsThread {
    emitter: EventsEmitter,
    conn: Connection,
    headers: Vec<u8>,
    video_server_ch_sender: Sender<VideoServerAction>,
}

impl EventsThread {
    pub fn new(
        conn: Connection,
        headers: Vec<u8>,
        video_server_ch_sender: Sender<VideoServerAction>,
    ) -> Self {
        Self {
            emitter: EventsEmitter::new(),
            conn,
            headers,
            video_server_ch_sender,
        }
    }

    fn handle_event(&mut self, buf: &mut [u8], src: SocketAddr, size: usize) {
        let packet_type = parse_packet_type(&buf);

        match packet_type {
            EPacketType::ShakeAE => {
                let meta = match self
                    .conn
                    .connect_client(src, &buf[HEADER..size], &self.headers)
                {
                    Some(meta) => meta,
                    None => return,
                };

                self.conn.mute_client(src, meta.muted.try_into().unwrap());

                self.conn.set_dimensions(
                    meta.width.try_into().unwrap(),
                    meta.height.try_into().unwrap(),
                );

                self.video_server_ch_sender
                    .send(VideoServerAction::SymKey)
                    .unwrap();

                self.video_server_ch_sender
                    .send(VideoServerAction::ConfigUpdate)
                    .unwrap();
            }
            EPacketType::ShakeUE => {
                self.conn.initialize_client(src);
            }
            EPacketType::ClientState => {
                let sym_key = self.conn.get_sym_key();
                if sym_key.is_none() {
                    return;
                }

                let meta = match ClientStatePayload::from_payload(
                    &mut buf[HEADER..size],
                    &mut sym_key.unwrap(),
                ) {
                    Ok(meta) => meta,
                    Err(_) => return,
                };

                debug!("Client State: {:?}", meta);

                self.conn.mute_client(src, meta.muted.try_into().unwrap());
                self.conn.set_dimensions(
                    meta.width.try_into().unwrap(),
                    meta.height.try_into().unwrap(),
                );

                // TODO: Don't refresh encoder if the dimensions are the same

                self.video_server_ch_sender
                    .send(VideoServerAction::ConfigUpdate)
                    .unwrap();
            }
            EPacketType::Alive => {
                self.conn.send_alive(src);
            }
            EPacketType::PING => {
                self.conn.received_ping(src);
            }
            EPacketType::Disconnect => {
                self.conn.remove_client(src);
                if self.conn.has_clients() {
                    return;
                }
                self.video_server_ch_sender
                    .send(VideoServerAction::Inactive)
                    .unwrap();
            }
            EPacketType::InputState => {
                self.emitter.input(
                    &mut buf[HEADER..],
                    self.conn.get_meta().width,
                    self.conn.get_meta().height,
                );
            }
            _ => {}
        }
    }

    fn start_loop(&mut self) {
        loop {
            let mut buf = [0u8; MTU];
            let (size, src) = self.conn.recv_from(&mut buf).unwrap();

            self.handle_event(&mut buf, src, size);
        }
    }

    pub fn run(
        conn: &Connection,
        headers: Vec<u8>,
        video_server_ch_sender: Sender<VideoServerAction>,
    ) {
        let conn = conn.clone();
        let _ = thread::spawn(move || {
            let mut events = EventsThread::new(conn, headers, video_server_ch_sender);

            events.start_loop();
        });
    }
}
