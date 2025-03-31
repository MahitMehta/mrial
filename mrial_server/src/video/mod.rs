pub mod display;
pub mod session;
pub mod yuv;

use display::DisplayMeta;
use kanal::{unbounded, unbounded_async, AsyncReceiver, AsyncSender, Receiver};
use log::{debug, error, warn};
use mrial_proto::{video::EColorSpace, ENalVariant, EPacketType};
use scrap::{Capturer, Display};
use session::{SessionSettingTask, Setting};
use std::{
    fs::File,
    io::{ErrorKind::WouldBlock, Write},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::Mutex,
    time::{sleep, Instant},
};
use x264::{Encoder, Param, Picture};
use yuv::YUVBuffer;

use crate::{
    audio::{AudioServerAction, AudioServerTask},
    conn::{BroadcastTaskError, ConnectionManager, ServerMeta},
    events::{EventsTask, EventsTaskAction},
};

#[derive(PartialEq, Debug)]
pub enum VideoServerAction {
    Inactive,
    ConfigUpdate,
    RestartStream,
    RestartSession,
    #[cfg(target_os = "linux")]
    NewUserSession,
}

pub struct VideoServerTask {
    file: Option<File>,
    row_len: usize,

    csp: EColorSpace,
    par: Param,
    pic: Picture,
    capturer: Option<Capturer>,
    encoder: Encoder,
    headers: Arc<Mutex<Option<Vec<u8>>>>,

    conn: ConnectionManager,

    setting: Setting,
    setting_thread: Option<tokio::task::JoinHandle<()>>,

    events_sender: AsyncSender<EventsTaskAction>,
    events_receiver: AsyncReceiver<EventsTaskAction>,
    events_thread: Option<tokio::task::JoinHandle<()>>,

    audio_sender: AsyncSender<AudioServerAction>,
    audio_receiver: Receiver<AudioServerAction>,
    audio_thread: Option<tokio::task::JoinHandle<()>>,
}

impl VideoServerTask {
    pub async fn new(conn: ConnectionManager) -> Result<Self, Box<dyn std::error::Error>> {
        #[cfg(not(target_os = "linux"))]
        let setting = Setting::Unknown;
        #[cfg(target_os = "linux")]
        let setting = session::config_xenv()?;

        let display: Display = Display::primary()?;
        let capturer = Capturer::new(display)?;

        conn.set_dimensions(capturer.width(), capturer.height())
            .await;

        let row_len = 4 * capturer.width() * capturer.height();

        let serde_meta = conn.get_meta().await;
        let mut par: Param = VideoServerTask::get_parameters(&serde_meta);
        let mut encoder = x264::Encoder::open(&mut par)?;
        let header = encoder.get_headers()?.as_bytes().to_vec();

        let pic = Picture::from_param(&par)?;

        let (events_sender, events_receiver) = unbounded_async::<EventsTaskAction>();
        let (audio_sender, audio_receiver) = unbounded::<AudioServerAction>();

        Ok(Self {
            row_len,
            file: None,

            csp: serde_meta.csp,
            par,
            pic,
            capturer: Some(capturer),
            encoder,
            headers: Arc::new(Mutex::new(Some(header))),

            conn,

            events_receiver,
            events_sender,
            events_thread: None,

            setting_thread: None,
            setting,

            audio_thread: None,
            audio_sender: audio_sender.clone_async(),
            audio_receiver,
        })
    }

    #[inline]
    #[allow(dead_code)]
    fn write_stream(&mut self, bitstream: &[u8]) {
        if let Some(file) = &mut self.file {
            file.write(bitstream).unwrap();
        }
    }

    fn get_parameters(server_meta: &ServerMeta) -> Param {
        let mut par = Param::default_preset("ultrafast", "zerolatency").unwrap();

        par = par.set_csp(server_meta.csp.into());
        par = par.set_dimension(server_meta.height, server_meta.width);

        if cfg!(target_os = "windows") {
            par = par.set_fullrange(1);
        }

        par = par.param_parse("repeat_headers", "1").unwrap();
        par = par.param_parse("annexb", "1").unwrap();
        par = par.param_parse("bframes", "0").unwrap();
        par = par.param_parse("crf", "20").unwrap();

        if server_meta.csp != EColorSpace::YUV444 {
            par = par.apply_profile("high").unwrap();
        } else {
            par = par.apply_profile("high444").unwrap();
        }

        par
    }

    fn drop_capturer(&mut self) {
        if let Some(capturer) = self.capturer.take() {
            debug!("Dropping Capturer");
            drop(capturer);
        }
    }

    async fn handle_app_broadcast(&self, buf: &[u8]) {
        // TODO: Find out why I-Frames are 7 instead of 5 and
        // TODO: make sure the 4th byte is always represents the NAL header
        
        let packet_type_variant = match buf[4] & 0x1F {
            7 => ENalVariant::KeyFrame as u8,
            _ => ENalVariant::NonKeyFrame as u8,
        };
        
        if let Err(e) = self
            .conn
            .app_encrypted_broadcast(EPacketType::NAL, packet_type_variant, buf)
            .await
        {
            match e {
                BroadcastTaskError::TaskNotRunning => {
                    error!("App Broadcast Task Not Running");

                    debug!("Restarting App Broadcast Task");
                    self.conn.get_app().start_broadcast_async_task();
                }
                BroadcastTaskError::TransferFailed(msg) => {
                    error!("App Broadcast Send Error: {msg}");
                }
                BroadcastTaskError::EncryptionFailed(msg) => {
                    error!("App Broadcast Encryption Error: {msg}");
                }
            }
        }
    }

    fn handle_web_broadcast(&self, buf: &[u8]) {
        if let Err(e) = self.conn.web_encrypted_broadcast(EPacketType::NAL, buf) {
            match e {
                BroadcastTaskError::TaskNotRunning => {
                    error!("Web Broadcast Task Not Running");

                    debug!("Restarting Web Broadcast Task");
                    self.conn.get_web().start_broadcast_async_task();
                }
                BroadcastTaskError::TransferFailed(msg) => {
                    error!("Web Broadcast Send Error: {msg}");
                }
                _ => {}
            }
        }
    }

    async fn restart_stream(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let display = Display::primary()?;
        let capturer: Capturer = Capturer::new(display)?;

        let server_meta = self.conn.get_meta().await;

        self.row_len = 4 * capturer.width() * capturer.height();
        self.conn
            .set_dimensions(capturer.width(), capturer.height())
            .await;
        self.csp = server_meta.csp;
        self.par = VideoServerTask::get_parameters(&server_meta);
        self.encoder = x264::Encoder::open(&mut self.par)?;

        let headers = self.encoder.get_headers()?;
        let header_bytes = headers.as_bytes();

        // Update the headers
        let mut header_ref = self.headers.lock().await;
        *header_ref = Some(header_bytes.to_vec());

        if self.conn.has_app_clients().await {
            self.handle_app_broadcast(&header_bytes).await;
        }

        if self.conn.has_web_clients().await {
            self.handle_web_broadcast(&header_bytes);
        }

        self.pic = Picture::from_param(&self.par)?;
        self.capturer = Some(capturer);

        Ok(())
    }

    async fn handle_server_action(
        &mut self,
        server_action: VideoServerAction,
        video_server_ch_sender: &AsyncSender<VideoServerAction>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match server_action {
            VideoServerAction::RestartSession => {
                #[cfg(target_os = "linux")]
                match session::config_xenv() {
                    Ok(Setting::PostLogin) => {
                        self.setting = Setting::PostLogin;
                    }
                    Ok(Setting::PreLogin) => {
                        self.setting = Setting::PreLogin;
                    }
                    _ => {}
                }

                if let Err(e) = video_server_ch_sender
                    .send(VideoServerAction::RestartStream)
                    .await
                {
                    error!("Error sending RestartStream action: {}", e);
                }

                if let Err(e) = self
                    .events_sender
                    .send(EventsTaskAction::RestartInputTread)
                    .await
                {
                    warn!("Error sending ReconnectInputModules action: {}", e);
                }
            }
            #[cfg(target_os = "linux")]
            VideoServerAction::NewUserSession => match session::config_xenv() {
                Ok(Setting::PostLogin) => {
                    self.setting = Setting::PostLogin;
                    video_server_ch_sender
                        .send(VideoServerAction::RestartStream)
                        .await?;
                }
                _ => {}
            },
            VideoServerAction::Inactive => {
                self.encoder = x264::Encoder::open(&mut self.par)?;
            }
            VideoServerAction::RestartStream => {
                self.drop_capturer();
                match self.restart_stream().await {
                    Ok(_) => {
                        debug!("Restarted Stream Successfully");
                    }
                    Err(e) => {
                        debug!("Error Restarting Stream: {}", e);
                        sleep(Duration::from_millis(100)).await;
                        video_server_ch_sender
                            .send(VideoServerAction::RestartStream)
                            .await?;
                    }
                }
            }
            VideoServerAction::ConfigUpdate => {
                let meta = self.conn.get_meta().await;

                let requested_width = meta.width;
                let requested_height = meta.height;

                if let Err(e) =
                    DisplayMeta::update_display_resolution(requested_width, requested_height)
                {
                    debug!("Error updating display resolution: {}", e);
                }

                video_server_ch_sender
                    .send(VideoServerAction::RestartStream)
                    .await?;
            }
        }

        Ok(())
    }

    fn start_session_thread(
        &mut self,
        ch_sender: AsyncSender<VideoServerAction>,
    ) -> Result<(), ()> {
        let has_setting_thread = match &self.setting_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_setting_thread {
            return Err(());
        }

        self.setting_thread = Some(SessionSettingTask::run(ch_sender, self.setting));

        Ok(())
    }

    fn start_audio_thread(&mut self) -> Result<(), ()> {
        let has_audio_thread = match &self.audio_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_audio_thread {
            return Err(());
        }

        let conn = self.conn.clone();

        self.audio_thread = Some(AudioServerTask::run(conn, self.audio_receiver.clone()));

        Ok(())
    }

    fn start_events_thread(
        &mut self,
        headers: Arc<Mutex<Option<Vec<u8>>>>,
        ch_sender: AsyncSender<VideoServerAction>,
    ) -> Result<(), ()> {
        let has_events_thread = match &self.events_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_events_thread {
            return Err(());
        }

        let conn = self.conn.clone();

        self.events_thread = Some(EventsTask::run(
            conn,
            headers,
            self.events_receiver.clone(),
            ch_sender,
            self.audio_sender.clone(),
        ));

        Ok(())
    }

    #[inline]
    async fn frame_delay(&self, duration: Duration) {
        let start = Instant::now();
        let accuracy = Duration::new(0, 1_000_000);
        if duration > accuracy {
            tokio::time::sleep(duration - accuracy).await;
        }
        // spin the rest of the duration
        while start.elapsed() < duration {
            std::hint::spin_loop();
        }
    }

    #[inline]
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (ch_sender, ch_receiver) = unbounded_async::<VideoServerAction>();

        let mut frames = 0u8;
        let mut fps_time = Instant::now();

        if let Err(_) = self.start_events_thread(self.headers.clone(), ch_sender.clone()) {
            error!("Error starting events thread.");
        }

        if let Err(_) = self.start_audio_thread() {
            error!("Error starting audio thread.");
        }

        if let Err(_) = self.start_session_thread(ch_sender.clone()) {
            error!("Error starting session thread.");
        }

        self.conn.get_app().start_broadcast_async_task();
        self.conn.get_web().start_broadcast_async_task();

        loop {
            while ch_receiver.len() > 0 {
                if let Ok(Some(server_action)) = ch_receiver.try_recv_realtime() {
                    if let Err(e) = self.handle_server_action(server_action, &ch_sender).await {
                        error!("Error handling server action: {}", e);
                    };
                }
            }

            let capturer = match &mut self.capturer {
                Some(capturer) => capturer,
                None => {
                    continue;
                }
            };

            let (has_app_clients, has_web_clients) = tokio::join! {
                self.conn.has_app_clients(),
                self.conn.has_web_clients(),
            };

            if !has_app_clients && !has_web_clients {
                self.conn.filter_clients().await;
                sleep(Duration::from_millis(250)).await;
                continue;
            }

            let sleep = Instant::now();
            let width = capturer.width();
            let height = capturer.height();

            match capturer.frame() {
                Ok(frame) => {
                    let argb_frame = frame.chunks(self.row_len).next().unwrap().to_vec();

                    if (width * height * 4) != argb_frame.len() {
                        debug!(
                            "Frame size: {} Expected: {}",
                            argb_frame.len(),
                            width * height * 4
                        );
                    }

                    if argb_frame.len() < (width * height * 4) {
                        debug!("Frame size less than expected");
                        continue;
                    }

                    match self.csp {
                        EColorSpace::YUV420 => {
                            let yuv = YUVBuffer::with_argb_for_i420(
                                width,
                                height,
                                &argb_frame[0..width * height * 4],
                            );
        
                            let y_plane = self.pic.as_mut_slice(0).unwrap();
                            y_plane.copy_from_slice(yuv.y());
                            let u_plane = self.pic.as_mut_slice(1).unwrap();
                            u_plane.copy_from_slice(yuv.u_420());
                            let v_plane = self.pic.as_mut_slice(2).unwrap();
                            v_plane.copy_from_slice(yuv.v_420());
                        }
                        EColorSpace::YUV444 => {
                            let yuv = YUVBuffer::with_argb_for_444(
                                width,
                                height,
                                &argb_frame[0..width * height * 4],
                            );
        
                            let y_plane = self.pic.as_mut_slice(0).unwrap();
                            y_plane.copy_from_slice(yuv.y());
                            let u_plane = self.pic.as_mut_slice(1).unwrap();
                            u_plane.copy_from_slice(yuv.u_444());
                            let v_plane = self.pic.as_mut_slice(2).unwrap();
                            v_plane.copy_from_slice(yuv.v_444());
                        }
                    } 

                    // TODO: Is this important?
                    // self.pic.set_timestamp(self.frame_count);

                    if let Ok(Some((nal, _, _))) = self.encoder.encode(&self.pic) {
                        if has_app_clients {
                            self.handle_app_broadcast(&nal.as_bytes()).await;
                        }

                        if has_web_clients {
                            self.handle_web_broadcast(&nal.as_bytes());
                        }

                        frames += 1;
                    }

                    if fps_time.elapsed().as_secs() >= 1 && frames > 0 {
                        self.conn.filter_clients().await;
                        debug!(
                            "FPS: {}",
                            frames as f32 / fps_time.elapsed().as_secs() as f32
                        );
                        frames = 0;
                        fps_time = Instant::now();

                        self.conn.app_drain_subpacket_cache().await;
                    }

                    let current_elapsed = sleep.elapsed().as_micros();
                    if current_elapsed > 0 && current_elapsed < 16667 {
                        let requested_delay = 16667 - current_elapsed;
                        self.frame_delay(Duration::from_micros(requested_delay as u64))
                            .await;
                    }
                }
                Err(ref e) if e.kind() == WouldBlock => {}
                Err(e) => {
                    error!("Error Capturing Frame: {}", e);
                }
            }
        }
    }
}
