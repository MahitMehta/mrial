use std::{
    net::SocketAddr,
    sync::Arc, thread
};

use bytes::Bytes;
use enigo::{
    Direction::{Press, Release},
    Enigo, InputError, Keyboard, Mouse, NewConError, Settings,
};
use kanal::{bounded, AsyncReceiver, AsyncSender, Receiver, Sender};
use log::{debug, warn};
use mrial_proto::{input::*, packet::*, ClientStatePayload, JSONPayloadSE};

#[cfg(target_os = "linux")]
use mouse_keyboard_input;
use tokio::{sync::Mutex, task::JoinHandle};

#[cfg(target_os = "linux")]
use std::time::Duration;

use crate::video::VideoServerAction;
use crate::{audio::AudioServerAction, conn::ConnectionManager};

pub enum InputThreadAction {
    Restart,
    Input(InputPayload)
}

pub struct InputThread {
    enigo: Enigo,
    left_mouse_held: bool,
    // Refers to user session (mainly for linux user session) restart
    session_restart_in_progress: bool,

    video_server_ch_sender: Sender<VideoServerAction>,
    input_receiver: Receiver<InputThreadAction>,

    #[cfg(target_os = "linux")]
    uinput: mouse_keyboard_input::VirtualDevice,
}

impl InputThread {
    #[cfg(target_os = "linux")]
    fn new(
        input_receiver: Receiver<InputThreadAction>, 
        video_server_ch_sender: Sender<VideoServerAction>
    ) -> Result<Self, enigo::NewConError> {
        let uinput =
            mouse_keyboard_input::VirtualDevice::new(Duration::new(0.040 as u64, 0), 2000).unwrap();
        let enigo = Enigo::new(&Settings::default())?;

        Ok(Self {
            enigo,
            uinput,
            session_restart_in_progress: false,
            left_mouse_held: false,

            video_server_ch_sender,
            input_receiver,
        })
    }

    fn input_loop(&mut self) {
        loop {
            let action = match self.input_receiver.recv() {
                Ok(payload) => payload,
                Err(e) => {
                    warn!("Error receiving input payload: {}", e);
                    break;
                }
            };

            match action {
                InputThreadAction::Restart => {
                    match self.reconnect_input_modules() {
                        Ok(_) => {
                            debug!("Input Modules Reconnected");
                        }
                        Err(e) => {
                            warn!("Error reconnecting input modules: {}", e);
                        }
                    }
                    self.session_restart_in_progress = false;
                }
                InputThreadAction::Input((buf, width, height)) => {
                    self.input(&buf, width, height);
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn new(video_server_ch_sender: Sender<VideoServerAction>) -> Result<Self, enigo::NewConError> {
        let enigo = Enigo::new(&Settings::default())?;

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
        self.enigo = Enigo::new(&Settings::default())?;

        self.session_restart_in_progress = false;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn reconnect_input_modules(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.enigo = Enigo::new(&Settings::default())?;

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

    fn handle_meta_keys(&mut self, buf: &[u8]) -> Result<(), InputError> {
        if is_control_pressed(buf) {
            self.enigo.key(enigo::Key::Control, Press)?;
        } else if is_control_released(buf) {
            self.enigo.key(enigo::Key::Control, Release)?;
        }

        if is_shift_pressed(buf) {
            self.enigo.key(enigo::Key::Shift, Press)?;
        } else if is_shift_released(buf) {
            self.enigo.key(enigo::Key::Shift, Release)?;
        }

        if is_alt_pressed(buf) {
            self.enigo.key(enigo::Key::Alt, Press)?;
        } else if is_alt_released(buf) {
            self.enigo.key(enigo::Key::Alt, Release)?;
        }

        if is_meta_pressed(buf) {
            self.enigo.key(enigo::Key::Meta, Press)?;
        } else if is_meta_released(buf) {
            self.enigo.key(enigo::Key::Meta, Release)?;
        }

        Ok(())
    }

    fn handle_pressed_key(&mut self, buf: &[u8]) -> Result<(), InputError> {
        match Key::from(buf[8]) {
            Key::None => {}
            Key::Backspace => {
                self.enigo.key(enigo::Key::Backspace, Press)?;
            }
            Key::DownArrow => {
                self.enigo.key(enigo::Key::DownArrow, Press)?;
            }
            Key::UpArrow => {
                self.enigo.key(enigo::Key::UpArrow, Press)?;
            }
            Key::LeftArrow => {
                self.enigo.key(enigo::Key::LeftArrow, Press)?;
            }
            Key::RightArrow => {
                self.enigo.key(enigo::Key::RightArrow, Press)?;
            }
            Key::Space => {
                self.enigo.key(enigo::Key::Space, enigo::Direction::Press)?;
            }
            Key::Tab => {
                self.enigo.key(enigo::Key::Tab, enigo::Direction::Press)?;
            }
            Key::Return => {
                self.enigo
                    .key(enigo::Key::Return, enigo::Direction::Click)?;
            }
            Key::Unicode => self.enigo.key(enigo::Key::Unicode(buf[8] as char), Press)?,
        }

        Ok(())
    }

    fn handle_released_key(&mut self, buf: &[u8]) -> Result<(), InputError> {
        match Key::from(buf[9]) {
            Key::None => {}
            Key::Backspace => {
                self.enigo.key(enigo::Key::Backspace, Release)?;
            }
            Key::Space => {
                self.enigo
                    .key(enigo::Key::Space, enigo::Direction::Release)?;
            }
            Key::DownArrow => {
                self.enigo.key(enigo::Key::DownArrow, Release)?;
            }
            Key::UpArrow => {
                self.enigo.key(enigo::Key::UpArrow, Release)?;
            }
            Key::LeftArrow => {
                self.enigo.key(enigo::Key::LeftArrow, Release)?;
            }
            Key::RightArrow => {
                self.enigo.key(enigo::Key::RightArrow, Release)?;
            }
            Key::Tab => {
                self.enigo.key(enigo::Key::Tab, enigo::Direction::Release)?;
            }
            Key::Return => {}
            Key::Unicode => {
                self.enigo
                    .key(enigo::Key::Unicode((buf[9]) as char), Release)?;
            }
        }

        Ok(())
    }

    fn input(&mut self, buf: &[u8], width: usize, height: usize) {
        // TODO: Scroll only works on linux
        if scroll_requested(&buf) {
            let x_delta = i16::from_be_bytes(buf[14..16].try_into().unwrap());
            let y_delta = i16::from_be_bytes(buf[16..18].try_into().unwrap());

            if cfg!(target_os = "linux") {
                self.scroll(x_delta as i32, y_delta as i32);
            }
        }

        if click_requested(buf) {
            let (x, y, right) = parse_click(buf, width, height);

            match self.enigo.move_mouse(x, y, enigo::Coordinate::Abs) {
                Ok(_) => {
                    if right {
                        let _ = self
                            .enigo
                            .button(enigo::Button::Right, enigo::Direction::Click);
                    } else {
                        self.left_mouse_held = !self.left_mouse_held;
                        let _ = self
                            .enigo
                            .button(enigo::Button::Left, enigo::Direction::Click);
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

            if let Err(e) = self
                .enigo
                .move_mouse(x as i32, y as i32, enigo::Coordinate::Abs)
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
                let _ = self
                    .enigo
                    .button(enigo::Button::Left, enigo::Direction::Press);
            }
        }

        if let Err(e) = self.handle_meta_keys(&buf) {
            debug!("Error handling meta keys: {}", e);
        }

        if let Err(e) = self.handle_pressed_key(&buf) {
            debug!("Error handling pressed key: {}", e);
        }

        if let Err(e) = self.handle_released_key(&buf) {
            debug!("Error handling released key: {}", e);
        }
    }
}
pub enum EventsTaskAction {
    RestartInputTread,
}

type InputPayload = (Bytes, usize, usize);

pub struct EventsTask {
    input_thread_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    input_receiver: Receiver<InputThreadAction>,
    input_sender: AsyncSender<InputThreadAction>,

    conn: ConnectionManager,
    headers: Arc<Mutex<Option<Vec<u8>>>>,
    video_server_ch_sender: AsyncSender<VideoServerAction>,
    audio_server_ch_sender: AsyncSender<AudioServerAction>,
}

const MAX_INPUT_QUEUE: usize = 100;

impl EventsTask {
    pub fn new(
        conn: ConnectionManager,
        headers: Arc<Mutex<Option<Vec<u8>>>>,
        video_server_ch_sender: AsyncSender<VideoServerAction>,
        audio_server_ch_sender: AsyncSender<AudioServerAction>,
    ) -> Result<Self, NewConError> {
        let (input_sender, input_receiver) = bounded(MAX_INPUT_QUEUE);

        Ok(Self {
            input_receiver,
            input_sender: input_sender.clone_async(),
            input_thread_handle: Arc::new(Mutex::new(None)),

            conn,
            headers,
            video_server_ch_sender,
            audio_server_ch_sender,
        })
    }

    async fn handle_web_event(&mut self, buf: Bytes) {
        let packet_type = parse_packet_type(&buf);

        match packet_type {
            EPacketType::InputState => {
                let meta = self.conn.get_meta().await;

                let input = InputThreadAction::Input((buf, meta.width, meta.height));
                if let Err(e) = self.input_sender.send(input).await {
                    warn!("Error sending input payload to input thread: {}", e);

                    // TODO: Consider restarting input thread!
                }
            }
            _ => {}
        }
    }

    async fn handle_app_event(&mut self, buf: &[u8], src: SocketAddr, size: usize) {
        let packet_type = parse_packet_type(&buf);

        match packet_type {
            EPacketType::ShakeAE => {
                let mut app = self.conn.get_app();

                let headers = self.headers.lock().await.clone();
                let meta = match app.connect_client(src, &buf[HEADER..size], headers).await {
                    Ok(meta) => meta,
                    Err(e) => {
                        warn!("Error connecting client: {}", e);
                        return;
                    }
                };

                app.mute_client(src, meta.muted.try_into().unwrap()).await;

                self.conn
                    .set_dimensions(
                        meta.width.try_into().unwrap(),
                        meta.height.try_into().unwrap(),
                    )
                    .await;
            

                if let Err(e) = self.video_server_ch_sender.send(VideoServerAction::SymKey).await {
                    warn!(
                        "Error sending {:?} action to video server: {}",
                        VideoServerAction::SymKey,
                        e
                    );
                }

                if let Err(e) = self.audio_server_ch_sender.send(AudioServerAction::SymKey).await {
                    warn!(
                        "Error sending {:?} action to audio server: {}",
                        AudioServerAction::SymKey,
                        e
                    );
                }

                if let Err(e) = self
                    .video_server_ch_sender
                    .send(VideoServerAction::ConfigUpdate).await
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
                let _ = app.initialize_client(src).await;
            }
            EPacketType::ClientState => {
                let app = self.conn.get_app();
                let sym_key = app.get_sym_key().await;

                if sym_key.is_none() {
                    return;
                }

                let meta = match ClientStatePayload::from_payload(
                    &buf[HEADER..size],
                    &mut sym_key.unwrap(),
                ) {
                    Ok(meta) => meta,
                    Err(_) => return,
                };

                debug!("Client State: {:?}", meta);
                
                app.mute_client(src, meta.muted.try_into().unwrap()).await;

                self.conn
                    .set_dimensions(
                        meta.width.try_into().unwrap(),
                        meta.height.try_into().unwrap(),
                    )
                    .await;
            

                if let Err(e) = self.video_server_ch_sender
                    .send(VideoServerAction::ConfigUpdate)
                    .await {
                    warn!(
                        "Error sending {:?} action to video server: {}",
                        VideoServerAction::ConfigUpdate,
                        e
                    );
                }
            }
            EPacketType::Alive => {
                if let Err(e) = self.conn.get_app().send_alive(src).await {
                    warn!("Error sending alive packet: {}", e);
                }
            }
            EPacketType::PING => {
                self.conn.get_app().received_ping(src).await;
            }
            EPacketType::Disconnect => {
                self.conn.get_app().remove_client(src).await;

                if self.conn.has_clients().await {
                    return;
                }

                if let Err(e) = self
                    .video_server_ch_sender
                    .send(VideoServerAction::Inactive).await
                {
                    warn!("Error sending inactive action to video server: {}", e);
                }
            }
            EPacketType::InputState => {
                let meta = self.conn.get_meta().await;

                let bytes = Bytes::copy_from_slice(&buf[HEADER..size]);

                let input = InputThreadAction::Input((bytes, meta.width, meta.height));
                if let Err(e) = self.input_sender.send(input).await {
                    warn!("Error sending input payload to input thread: {}", e);
                }
            }
            _ => {}
        }
    }

    fn start_input_thread(&self) -> thread::JoinHandle<()> {
        let video_server_ch_sender = self.video_server_ch_sender.clone_sync(); 
        let input_receiver = self.input_receiver.clone();

        thread::spawn(move || {
            let mut input_thread = match InputThread::new(input_receiver, video_server_ch_sender) {
                Ok(input_thread) => input_thread,
                Err(e) => {
                    warn!("Failed to start input thread: {}", e);
                    return;
                }
            };

            input_thread.input_loop();
        })
    }

    async fn process_loop(&mut self, event_ch_receiver: AsyncReceiver<EventsTaskAction>) {
        let web_receiver = self.conn.web_receiver();
        let mut app_buf = [0u8; MTU];

        let mut input_thread_handle_lock = self.input_thread_handle.lock().await;

        if input_thread_handle_lock.is_none() {
            *input_thread_handle_lock = Some(self.start_input_thread());
        }

        drop(input_thread_handle_lock);

        loop {
            tokio::select! {
                // TODO: Determine if this is a good idea
                biased;

                app_ret = self.conn.app_recv_from(&mut app_buf) => {
                    match app_ret {
                        Ok((size, src)) => {
                            self.handle_app_event(&app_buf, src, size).await;
                        }
                        _ => {}
                    }
                }
                web_ret = web_receiver.recv() => {
                    match web_ret {
                        Ok(bytes) => {
                            self.handle_web_event(bytes).await;
                        }
                        _ => {}
                    }
                }
                action_ret = event_ch_receiver.recv() => {
                    match action_ret {
                        Ok(EventsTaskAction::RestartInputTread) => {
                            let mut input_thread_handle_lock = self.input_thread_handle.lock().await;
                            
                            if input_thread_handle_lock.is_none() || input_thread_handle_lock.as_ref().unwrap().is_finished() {
                                debug!("Fresh Restarting input thread");
                                *input_thread_handle_lock = Some(self.start_input_thread());
                            } else {
                                if let Err(e) = self.input_sender.send(InputThreadAction::Restart).await {
                                    warn!("Error sending restart action to input thread: {}", e);
                                }
                            }
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
        event_ch_receiver: AsyncReceiver<EventsTaskAction>,
        video_server_ch_sender: AsyncSender<VideoServerAction>,
        audio_server_ch_sender: AsyncSender<AudioServerAction>,
    ) -> JoinHandle<()> {
        let handle = tokio::runtime::Handle::current();

        handle.spawn(async move {
            match EventsTask::new(
                conn,
                headers,
                video_server_ch_sender,
                audio_server_ch_sender,
            ) {
                Ok(mut events_thread) => {
                    events_thread.process_loop(event_ch_receiver).await;
                }
                Err(e) => {
                    warn!("Failed to start events thread: {}", e);
                }
            };
        })
    }
}
