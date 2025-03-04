pub mod display;
pub mod session;
pub mod yuv;

use display::DisplayMeta;
use futures::{executor::ThreadPool, future::RemoteHandle, task::SpawnExt};
use kanal::{unbounded, Receiver, Sender};
use log::{debug, error};
use mrial_proto::{deploy::PacketDeployer, *};
use scrap::{Capturer, Display};
use session::{SessionSettingThread, Setting};
use std::{
    collections::VecDeque, fs::File, io::{ErrorKind::WouldBlock, Write}, sync::{Arc, Mutex}, thread, time::{Duration, Instant}
};
use x264::{Encoder, Param, Picture};
use yuv::YUVBuffer;

use crate::{
    audio::{AudioServerAction, AudioServerThread},
    conn::{Connection, ConnectionManager, ServerMeta}, 
    events::{EventsThread, EventsThreadAction}
};

use self::yuv::EColorSpace;

#[derive(PartialEq, Debug)]
pub enum VideoServerAction {
    Inactive,
    ConfigUpdate,
    NewUserSession,
    RestartStream,
    SymKey,
    RestartSession,
}

pub struct VideoServerThread {
    pool: ThreadPool,
    yuv_handles: VecDeque<RemoteHandle<YUVBuffer>>,
    file: Option<File>,
    row_len: usize,

    par: Param,
    pic: Picture,
    capturer: Option<Capturer>,
    encoder: Encoder,
    headers: Arc<Mutex<Option<Vec<u8>>>>,

    deployer: PacketDeployer,
    conn: ConnectionManager,
    
    setting: Setting,
    setting_thread: Option<thread::JoinHandle<()>>,

    events_sender: Sender<EventsThreadAction>,
    events_receiver: Receiver<EventsThreadAction>,
    events_thread: Option<thread::JoinHandle<()>>,

    audio_sender: Sender<AudioServerAction>,
    audio_receiver: Receiver<AudioServerAction>,
    audio_thread: Option<thread::JoinHandle<()>>,
}

impl VideoServerThread {
    pub fn new(conn: ConnectionManager) -> Result<Self, Box<dyn std::error::Error>> {
        let mut setting = Setting::Unknown;

        #[cfg(target_os = "linux")]
        {
            setting = session::config_xenv()?;
        }

        let display: Display = Display::primary()?;
        let capturer = Capturer::new(display)?;

        conn.set_dimensions(capturer.width(), capturer.height());

        let pool = ThreadPool::builder().pool_size(1).create()?;
        let yuv_handles = VecDeque::new();

        let row_len = 4 * capturer.width() * capturer.height();

        let mut par: Param = VideoServerThread::get_parameters(conn.get_meta());
        let mut encoder = x264::Encoder::open(&mut par)?;
        let header = encoder.get_headers()?.as_bytes().to_vec();

        let pic = Picture::from_param(&par)?;

        let (events_sender, events_receiver) = unbounded::<EventsThreadAction>();
        let (audio_sender, audio_receiver) = unbounded::<AudioServerAction>();

        Ok(Self {
            pool,
            yuv_handles,
            row_len,
            file: None,

            par,
            pic,
            capturer: Some(capturer),
            encoder,
            headers: Arc::new(Mutex::new(Some(header))),
         
            deployer: PacketDeployer::new(EPacketType::NAL, false),
            conn,

            events_receiver,
            events_sender,
            events_thread: None,

            setting_thread: None,
            setting,

            audio_thread: None,
            audio_sender,
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

    fn get_parameters(server_meta: Option<ServerMeta>) -> Param {
        let mut par = Param::default_preset("ultrafast", "zerolatency").unwrap();

        // par = par.set_csp(EColorSpace::YUV444.into());
        par = par.set_csp(EColorSpace::YUV420.into());

        if let Some(server_meta) = server_meta {
            par = par.set_dimension(server_meta.height, server_meta.width);
        }

        if cfg!(target_os = "windows") {
            par = par.set_fullrange(1);
        }

        par = par.param_parse("repeat_headers", "1").unwrap();
        par = par.param_parse("annexb", "1").unwrap();
        par = par.param_parse("bframes", "0").unwrap();
        par = par.param_parse("crf", "20").unwrap();
        par = par.apply_profile("high").unwrap();
        // par = par.apply_profile("high444").unwrap();

        par
    }

    fn drop_capturer(&mut self) {
        if let Some(capturer) = self.capturer.take() {
            debug!("Dropping Capturer");
            drop(capturer);
        }
    }

    async fn restart_stream(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let display = Display::primary()?;
        let capturer: Capturer = Capturer::new(display)?;

        self.row_len = 4 * capturer.width() * capturer.height();
        self.conn
            .set_dimensions(capturer.width(), capturer.height());
        self.par = VideoServerThread::get_parameters(self.conn.get_meta());
        self.encoder = x264::Encoder::open(&mut self.par)?;

        let headers = self.encoder.get_headers()?;
        let header_bytes = headers.as_bytes();

        // Update the headers
        if let Ok(mut headers_ref) = self.headers.lock() {
            *headers_ref = Some(header_bytes.to_vec());
        }

        if self.deployer.has_sym_key() {
            if self.conn.has_app_clients() {
                self.deployer.prepare_encrypted(
                    &header_bytes,
                    Box::new(|subpacket| {
                        self.conn.app_broadcast(&subpacket);
                    }),
                );
            }
            
            if self.conn.has_web_clients().await {
                self.deployer.prepare_unencrypted(
                    &header_bytes,
                    Box::new(|subpacket| {
                        self.conn.web_broadcast(subpacket);
                    }),
                );
            }
        }

        self.pic = Picture::from_param(&self.par)?;

        self.yuv_handles.clear();
        self.capturer = Some(capturer);

        Ok(())
    }

    async fn handle_server_action(
        &mut self,
        server_action: VideoServerAction,
        video_server_ch_sender: &kanal::Sender<VideoServerAction>,
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

                video_server_ch_sender
                    .send(VideoServerAction::RestartStream)?;
                self.events_sender
                    .send(EventsThreadAction::ReconnectInputModules)?;
            }
            VideoServerAction::NewUserSession =>
            {
                #[cfg(target_os = "linux")]
                match session::config_xenv() {
                    Ok(Setting::PostLogin) => {
                        self.setting = Setting::PostLogin;
                        video_server_ch_sender
                            .send(VideoServerAction::RestartStream)?;
                    }
                    _ => {}
                }
            }
            VideoServerAction::Inactive => {
                self.encoder = x264::Encoder::open(&mut self.par)?;
            }
            VideoServerAction::SymKey => {
                if let Ok(app) = self.conn.get_app() {
                    if let Some(sym_key) = app.get_sym_key() {
                        self.deployer.set_sym_key(sym_key.clone());
                    }
                }
            }
            VideoServerAction::RestartStream => {
                self.drop_capturer();
                match self.restart_stream().await {
                    Ok(_) => {
                        debug!("Restarted Stream Successfully");
                    }
                    Err(e) => {
                        debug!("Error Restarting Stream: {}", e);
                        video_server_ch_sender
                            .send(VideoServerAction::RestartStream)?;
                    }
                }
            }
            VideoServerAction::ConfigUpdate => {
                let meta = match self.conn.get_meta() {
                    Some(meta) => meta,
                    None => {
                        return Ok(());
                    }
                };

                let requested_width = meta.width;
                let requested_height = meta.height;

                if let Err(e) =
                    DisplayMeta::update_display_resolution(requested_width, requested_height)
                {
                    debug!("Error updating display resolution: {}", e);
                }

                video_server_ch_sender
                    .send(VideoServerAction::RestartStream)?;
            }
        }

        Ok(())
    }

    fn start_session_thread(&mut self, ch_sender: Sender<VideoServerAction>) -> bool {
        let has_setting_thread = match &self.setting_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_setting_thread {
            return false;
        }

        self.setting_thread = Some(SessionSettingThread::run(ch_sender, self.setting));
        
        true
    }

    fn start_audio_thread(
        &mut self,
    ) -> Result<bool, std::io::Error> {
        let has_audio_thread = match &self.audio_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_audio_thread {
            return Ok(false);
        }

        let conn = self.conn.try_clone()?;
       
        self.audio_thread = Some(AudioServerThread::run(
            conn, 
            self.audio_receiver.clone(),
            tokio::runtime::Handle::current(),
        ));

        Ok(true)
    }

    fn start_events_thread(
        &mut self,
        headers:Arc<Mutex<Option<Vec<u8>>>>,
        ch_sender: Sender<VideoServerAction>,
    ) -> Result<bool, std::io::Error> {
        let has_events_thread = match &self.events_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_events_thread {
            return Ok(false);
        }

        let conn = self.conn.try_clone()?;

        self.events_thread = Some(EventsThread::run(
            conn,
            headers,
            self.events_receiver.clone(),
            ch_sender,
            self.audio_sender.clone(),
        ));

        Ok(true)
    }

    #[inline]
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (ch_sender, ch_receiver) = unbounded::<VideoServerAction>();

        let mut frames = 0u8;
        let mut fps_time = Instant::now();

        if let Err(_) = self.start_events_thread(self.headers.clone(), ch_sender.clone()) {
            error!("Error starting events thread.");
        }

        if let Err(_) = self.start_audio_thread() {
            error!("Error starting audio thread.");
        }

        self.start_session_thread(ch_sender.clone());

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

            if !self.conn.has_clients().await {
                self.conn.filter_clients().await;
                std::thread::sleep(Duration::from_millis(250));
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

                    let cvt_rgb_yuv = async move {
                        // TODO: figure out why this is neccessary
                        let yuv = YUVBuffer::with_argb_for_i420(
                            width,
                            height,
                            &argb_frame[0..width * height * 4],
                        );
                        yuv
                    };
                    self.yuv_handles
                        .push_back(self.pool.spawn_with_handle(cvt_rgb_yuv).unwrap());

                    // set to 1 to increase FPS at the cost of latency, or 0  for the opposite effect
                    if self.yuv_handles.len() > 0 {
                        let yuv = self.yuv_handles.pop_front().unwrap().await;

                        let y_plane = self.pic.as_mut_slice(0).unwrap();
                        y_plane.copy_from_slice(yuv.y());
                        let u_plane = self.pic.as_mut_slice(1).unwrap();
                        u_plane.copy_from_slice(yuv.u_420());
                        let v_plane = self.pic.as_mut_slice(2).unwrap();
                        v_plane.copy_from_slice(yuv.v_420());

                        // TODO: Is this important?
                        //self.pic.set_timestamp(self.frame_count);

                        if let Ok(Some((nal, _, _))) = self.encoder.encode(&self.pic) {
                            if self.conn.has_app_clients() {
                                self.deployer.prepare_encrypted(
                                    &nal.as_bytes(),
                                    Box::new(|subpacket| {
                                        self.conn.app_broadcast(&subpacket);
                                    }),
                                );
                            }

                            if self.conn.has_web_clients().await {
                                self.deployer.prepare_unencrypted(
                                    &nal.as_bytes(),
                                    Box::new(|subpacket| {
                                        self.conn.web_broadcast(subpacket);
                                    }),
                                );
                            }

                            frames += 1;
                        }

                        if fps_time.elapsed().as_secs() >= 1 && frames > 0 {
                            self.conn.filter_clients();
                            debug!(
                                "FPS: {}",
                                frames as f32 / fps_time.elapsed().as_secs() as f32
                            );
                            frames = 0;
                            fps_time = Instant::now();
                        }

                        if sleep.elapsed().as_millis() > 0 && sleep.elapsed().as_millis() < 17 {
                            let delay = 17 - sleep.elapsed().as_millis();
                            spin_sleep::sleep(Duration::from_millis(delay as u64));
                        }
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
