use std::{
    net::SocketAddr, sync::Arc
};

use enigo::{
    Direction::{Press, Release}, Enigo, InputError, Keyboard, Mouse, NewConError, Settings
};
use kanal::{AsyncReceiver, Sender};
use log::{debug, warn};
use mrial_proto::{input::*, packet::*, ClientStatePayload, JSONPayloadSE};

#[cfg(target_os = "linux")]
use mouse_keyboard_input;

use tokio::{sync::{Mutex, RwLock}, task::JoinHandle};
#[cfg(target_os = "linux")]
use std::time::Duration;

use crate::{audio::AudioServerAction, conn::ConnectionManager};
use crate::video::VideoServerAction;

pub struct EventsEmitter {
    enigo: Arc<RwLock<Enigo>>,
    left_mouse_held: bool,
    session_restart_in_progress: bool,

    video_server_ch_sender: Sender<VideoServerAction>,

    #[cfg(target_os = "linux")]
    uinput: mouse_keyboard_input::VirtualDevice,
}

impl EventsEmitter {
    #[cfg(target_os = "linux")]
    fn new(video_server_ch_sender: Sender<VideoServerAction>) -> Result<Self, enigo::NewConError> {
        let uinput =
            mouse_keyboard_input::VirtualDevice::new(Duration::new(0.040 as u64, 0), 2000).unwrap();
        let enigo = Arc::new(RwLock::new(Enigo::new(&Settings::default())?));

        Ok(Self {
            enigo,
            uinput,
            video_server_ch_sender,
            session_restart_in_progress: false,
            left_mouse_held: false,
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn new(video_server_ch_sender: Sender<VideoServerAction>) -> Result<Self, enigo::NewConError> {
        let enigo = Arc::new(RwLock::new(Enigo::new(&Settings::default())?));

        Ok(Self {
            enigo,
            video_server_ch_sender,
            session_restart_in_progress: false,
            left_mouse_held: false,
        })
    }

    #[cfg(target_os = "linux")]
    fn reconnect_input_modules(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.uinput =
            mouse_keyboard_input::VirtualDevice::new(Duration::new(0.040 as u64, 0), 2000)?;
        self.enigo = Arc::new(RwLock::new(Enigo::new(&Settings::default())?));

        self.session_restart_in_progress = false;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn reconnect_input_modules(&mut self) -> Result<(), enigo::NewConError> {
        self.enigo = Arc::new(RwLock::new(Enigo::new(&Settings::default())?));

        self.session_restart_in_progress = false;
        Ok(())
    }

    // sudo apt install libudev-dev libevdev-dev libhidapi-dev
    // sudo usermod -a -G input user
    // sudo reboot

    #[cfg(target_os = "linux")]
    fn scroll(&mut self, x: i32, y: i32) {
        if x != 0 {
            let _ = &self.uinput.scroll_x(-x * 2);
        }

        if y != 0 {
            let _ = &self.uinput.scroll_y(-y * 2);
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn scroll(&self, _x: i32, _y: i32) {}

    async fn handle_meta_keys(&mut self, buf: &[u8]) -> Result<(), InputError> {
        let mut enigo = self.enigo.write().await;

        if is_control_pressed(buf) {
            enigo.key(enigo::Key::Control, Press)?;
        } else if is_control_released(buf) {
            enigo.key(enigo::Key::Control, Release)?;
        }

        if is_shift_pressed(buf) {
            enigo.key(enigo::Key::Shift, Press)?;
        } else if is_shift_released(buf) {
            enigo.key(enigo::Key::Shift, Release)?;
        }

        if is_alt_pressed(buf) {
            enigo.key(enigo::Key::Alt, Press)?;
        } else if is_alt_released(buf) {
            enigo.key(enigo::Key::Alt, Release)?;
        }

        if is_meta_pressed(buf) {
            enigo.key(enigo::Key::Meta, Press)?;
        } else if is_meta_released(buf) {
            enigo.key(enigo::Key::Meta, Release)?;
        }

        Ok(())
    }

    async fn handle_pressed_key(&mut self, buf: &[u8]) -> Result<(), InputError> {
        let mut enigo = self.enigo.write().await;

        match Key::from(buf[8]) {
            Key::None => {}
            Key::Backspace => {
                enigo.key(enigo::Key::Backspace, Press)?;
            }
            Key::DownArrow => {
                enigo.key(enigo::Key::DownArrow, Press)?;
            }
            Key::UpArrow => {
                enigo.key(enigo::Key::UpArrow, Press)?;
            }
            Key::LeftArrow => {
                enigo.key(enigo::Key::LeftArrow, Press)?;
            }
            Key::RightArrow => {
                enigo.key(enigo::Key::RightArrow, Press)?;
            }
            Key::Space => {
                enigo.key(enigo::Key::Space, enigo::Direction::Press)?;
            }
            Key::Tab => {
                enigo.key(enigo::Key::Tab, enigo::Direction::Press)?;
            }
            Key::Return => {
                enigo
                    .key(enigo::Key::Return, enigo::Direction::Click)?;
            }
            Key::Unicode => enigo.key(enigo::Key::Unicode(buf[8] as char), Press)?,
        }

        Ok(())
    }

    async fn handle_released_key(&mut self, buf: &[u8]) -> Result<(), InputError> {
        let mut enigo = self.enigo.write().await;

        match Key::from(buf[9]) {
            Key::None => {}
            Key::Backspace => {
                enigo.key(enigo::Key::Backspace, Release)?;
            }
            Key::Space => {
                enigo
                    .key(enigo::Key::Space, enigo::Direction::Release)?;
            }
            Key::DownArrow => {
                enigo.key(enigo::Key::DownArrow, Release)?;
            }
            Key::UpArrow => {
                enigo.key(enigo::Key::UpArrow, Release)?;
            }
            Key::LeftArrow => {
                enigo.key(enigo::Key::LeftArrow, Release)?;
            }
            Key::RightArrow => {
                enigo.key(enigo::Key::RightArrow, Release)?;
            }
            Key::Tab => {
                enigo.key(enigo::Key::Tab, enigo::Direction::Release)?;
            }
            Key::Return => {}
            Key::Unicode => {
                enigo
                    .key(enigo::Key::Unicode((buf[9]) as char), Release)?;
            }
        }

        Ok(())
    }

    async fn input(&mut self, buf: &mut [u8], width: usize, height: usize) {
        // TODO: Scroll only works on linux
        if scroll_requested(&buf) {
            let x_delta = i16::from_be_bytes(buf[14..16].try_into().unwrap());
            let y_delta = i16::from_be_bytes(buf[16..18].try_into().unwrap());

            if cfg!(target_os = "linux") {
                self.scroll(x_delta as i32, y_delta as i32);
            }
        }

        let mut enigo = self.enigo.write().await;

        if click_requested(buf) {
            let (x, y, right) = parse_click(buf, width, height);

            match enigo.move_mouse(x, y, enigo::Coordinate::Abs) {
                Ok(_) => {
                    if right {
                        let _ = enigo.button(enigo::Button::Right, enigo::Direction::Click);
                    } else {
                        self.left_mouse_held = !self.left_mouse_held;
                        let _ = enigo.button(enigo::Button::Left, enigo::Direction::Click);
                    }
                }
                Err(e) => {
                    debug!("Error moving mouse for click: {}", e);

                    if !self.session_restart_in_progress {
                        debug!("Session Restart Requested");
                        let _ = self
                            .video_server_ch_sender
                            .send(VideoServerAction::RestartSession);
                        self.session_restart_in_progress = true;
                    }
                }
            }
        }

        if mouse_move_requested(buf) {
            let (x, y, pressed) = parse_mouse_move(buf, width as f32, height as f32);

            if let Err(e) = enigo.move_mouse(x as i32, y as i32, enigo::Coordinate::Abs)
            {
                debug!("Error moving mouse: {}", e);
                if !self.session_restart_in_progress {
                    debug!("Session Restart Requested");
                    let _ = self
                        .video_server_ch_sender
                        .send(VideoServerAction::RestartSession);
                    self.session_restart_in_progress = true;
                }
            }

            if pressed && !self.left_mouse_held {
                self.left_mouse_held = true;
                let _ = enigo.button(enigo::Button::Left, enigo::Direction::Press);
            }
        }

        drop(enigo);

        if let Err(e) = self.handle_meta_keys(&buf).await {
            debug!("Error handling meta keys: {}", e);
        }

        if let Err(e) = self.handle_pressed_key(&buf).await {
            debug!("Error handling pressed key: {}", e);
        }

        if let Err(e) = self.handle_released_key(&buf).await {
            debug!("Error handling released key: {}", e);
        }
    }
}
pub enum EventsThreadAction {
    ReconnectInputModules,
}

pub struct EventsThread {
    emitter: EventsEmitter,
    conn: ConnectionManager,
    headers: Arc<Mutex<Option<Vec<u8>>>>,
    video_server_ch_sender: Sender<VideoServerAction>,
    audio_server_ch_sender: Sender<AudioServerAction>,
}

impl EventsThread {
    pub fn new(
        conn: ConnectionManager,
        headers: Arc<Mutex<Option<Vec<u8>>>>,
        video_server_ch_sender: Sender<VideoServerAction>,
        audio_server_ch_sender: Sender<AudioServerAction>,
    ) -> Result<Self, NewConError> {
        Ok(Self {
            emitter: EventsEmitter::new(video_server_ch_sender.clone())?,
            conn,
            headers,
            video_server_ch_sender,
            audio_server_ch_sender,
        })
    }

    async fn handle_event(&mut self, buf: &mut [u8], src: SocketAddr, size: usize) {
        let packet_type = parse_packet_type(&buf);

        match packet_type {
            EPacketType::ShakeAE => {
                let mut app = self.conn.get_app();

                let meta = match app.connect_client(src, &buf[HEADER..size], self.headers.clone()).await {
                    Ok(meta) => meta,
                    Err(e) => {
                        warn!("Error connecting client: {}", e);
                        return;
                    },
                };

                app.mute_client(src, meta.muted.try_into().unwrap());

                self.conn.set_dimensions(
                    meta.width.try_into().unwrap(),
                    meta.height.try_into().unwrap(),
                );

                if let Err(e) = self.video_server_ch_sender.send(VideoServerAction::SymKey) {
                    warn!(
                        "Error sending {:?} action to video server: {}",
                        VideoServerAction::SymKey,
                        e
                    );
                }

                if let Err(e) = self.audio_server_ch_sender.send(AudioServerAction::SymKey) {
                    warn!(
                        "Error sending {:?} action to audio server: {}",
                        AudioServerAction::SymKey,
                        e
                    );
                }

                if let Err(e) = self
                    .video_server_ch_sender
                    .send(VideoServerAction::ConfigUpdate)
                {
                    warn!(
                        "Error sending {:?} action to video server: {}",
                        VideoServerAction::ConfigUpdate,
                        e
                    );
                }
            }
            EPacketType::ShakeUE => {
                let app = self.conn.get_app();
                app.initialize_client(src);
            }
            EPacketType::ClientState => {
                let app = self.conn.get_app();
                let sym_key = app.get_sym_key().await;
            
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

                app.mute_client(src, meta.muted.try_into().unwrap());
                
                self.conn.set_dimensions(
                    meta.width.try_into().unwrap(),
                    meta.height.try_into().unwrap(),
                );

                self.video_server_ch_sender
                    .send(VideoServerAction::ConfigUpdate)
                    .unwrap();
            }
            EPacketType::Alive => {
                self.conn.get_app().send_alive(src);
            }
            EPacketType::PING => {
                self.conn.get_app().received_ping(src);
            }
            EPacketType::Disconnect => {
                self.conn.get_app().remove_client(src);

                if self.conn.has_clients().await {
                    return;
                }
                if let Err(e) = self
                    .video_server_ch_sender
                    .send(VideoServerAction::Inactive)
                {
                    warn!("Error sending inactive action to video server: {}", e);
                }
            }
            EPacketType::InputState => {
                let meta = self.conn.get_meta().await;

                self.emitter
                    .input(&mut buf[HEADER..], meta.width, meta.height);
            }
            _ => {}
        }
    }

    async fn process_loop(&mut self, event_ch_receiver: AsyncReceiver<EventsThreadAction>) {
        let web_receiver = self.conn.web_receiver();
        let mut buf = [0u8; MTU];

        loop {
            tokio::select! { 
                // TODO: Determine if biased is a good idea or not
                biased;

                app_ret = self.conn.app_recv_from(&mut buf) => {
                    if let Ok((size, src)) = app_ret {
                        self.handle_event(&mut buf, src, size).await;
                    }
                }    
                _web_ret = web_receiver.recv() => {
                    debug!("Web client input received");
                }     
                action = event_ch_receiver.recv() => { 
                    match action {
                        Ok(EventsThreadAction::ReconnectInputModules) => {
                            if let Ok(()) = self.emitter.reconnect_input_modules() {
                                debug!("Reconnected input modules");
                            } else {
                                debug!("Failed to reconnect input modules");
                            }
                            self.emitter.session_restart_in_progress = false;
                        }
                        Err(e) => {
                            warn!("Error receiving event action: {}", e);
                        }
                    }
                }   
            }
        }       
    }

    pub fn run(
        conn: ConnectionManager,
        headers: Arc<Mutex<Option<Vec<u8>>>>,
        event_ch_receiver: AsyncReceiver<EventsThreadAction>,
        video_server_ch_sender: Sender<VideoServerAction>,
        audio_server_ch_sender: Sender<AudioServerAction>,
    ) -> JoinHandle<()> {
        let tokio_handle = tokio::runtime::Handle::current();

        tokio_handle.spawn(async move {
            match EventsThread::new(
                conn, headers, video_server_ch_sender, audio_server_ch_sender) {
                Ok(mut events_thread) => {
                    events_thread.process_loop(event_ch_receiver).await;
                }
                Err(e) => {
                    warn!("Failed to start events thread: {}", e);
                }
            }
         })
    }
}
