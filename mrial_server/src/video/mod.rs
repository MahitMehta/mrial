pub mod display;
pub mod yuv;

use deploy::PacketDeployer;
use display::DisplayMeta;
use futures::{executor::ThreadPool, future::RemoteHandle, task::SpawnExt};
use kanal::{unbounded, Receiver, Sender};
use log::debug;
use mrial_proto::*;
use scrap::{Capturer, Display};
use std::{
    collections::VecDeque, env, fs::File, io::{ErrorKind::WouldBlock, Write}, process::Command, sync::RwLockReadGuard, thread, time::{Duration, Instant}
};
use x264::{Encoder, Param, Picture};
use yuv::YUVBuffer;

use crate::{
    conn::{Connection, ServerMetaData},
    events::{EventsThread, EventsThreadAction},
};

use self::yuv::EColorSpace;

#[derive(PartialEq)]
pub enum VideoServerAction {
    Inactive,
    ConfigUpdate,
    NewUserSession,
    RestartStream,
    SymKey,
    RestartSession,
}

#[derive(PartialEq)]
pub enum Setting {
    Unknown,
    PreLogin,
    PostLogin
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
    deployer: PacketDeployer,
    conn: Connection,
    setting: Setting,
    setting_thread: Option<thread::JoinHandle<()>>,

    events_sender: Sender<EventsThreadAction>,
    events_receiver: Receiver<EventsThreadAction>,
    events_thread: Option<thread::JoinHandle<()>>,
}

fn get_x11_authenicated_client() -> Option<String> {
    let gui_users_output = Command::new("sh")
        .arg("-c")
        .arg("who | grep tty7")
        .output()
        .unwrap();

    if gui_users_output.stdout.is_empty() || !gui_users_output.status.success() {
        return None;
    }

    let output_str = String::from_utf8(gui_users_output.stdout).unwrap();
    if let Some(user) = output_str.split_whitespace().next() {
        return Some(user.to_string());
    }
    
    None
}

struct SessionSettingThread {
}

impl SessionSettingThread {
    pub fn run(video_server_ch_sender: kanal::Sender<VideoServerAction>) -> thread::JoinHandle<()> {
        return thread::spawn(move || {
            loop {
                if get_x11_authenicated_client().is_some() {
                    debug!("User has logged in");
                    video_server_ch_sender.send(VideoServerAction::NewUserSession).unwrap();
                    break;
                }
                debug!("Waiting for user to login");

                thread::sleep(Duration::from_secs(1));
            }
        });
    }
}

impl VideoServerThread {
    pub fn new(conn: Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let mut setting = Setting::Unknown;

        if cfg!(target_os = "linux") {
            setting = VideoServerThread::config_xenv()?;
        }
         
        let display: Display = Display::primary()?;
        let capturer = Capturer::new(display)?;
    
        conn.set_dimensions(capturer.width(), capturer.height());

        let pool = ThreadPool::builder().pool_size(1).create()?;
        let yuv_handles = VecDeque::new();

        let row_len = 4 * capturer.width() * capturer.height();

        let mut par: Param = VideoServerThread::get_parameters(conn.get_meta());
        let encoder = x264::Encoder::open(&mut par)?;
        let pic = Picture::from_param(&par)?;

        let (events_sender, events_receiver) = unbounded::<EventsThreadAction>();

        Ok(Self {
            events_receiver,
            events_sender,
            events_thread: None,
            
            setting_thread: None,
            pool,
            yuv_handles,
            row_len,
            file: None,
            par,
            pic,
            capturer: Some(capturer),
            encoder,
            setting,
            deployer: PacketDeployer::new(EPacketType::NAL, false),
            conn
        })
    }

    /*
     *  Configures the X environment for the server by setting 
     *  correct display and Xauthority variables. 
     * 
     *  Additionally, it sets the XDG_RUNTIME_DIR and DBUS_SESSION_BUS_ADDRESS
     *  for pipewire connection from root.
     * 
     */

    // TODO: Make DISPLAY variable dynamic AND 
    // TODO: not assume the display manager is lightdm
    
    #[cfg(target_os = "linux")]
    fn config_xenv() -> Result<Setting, Box<dyn std::error::Error>> {
        env::set_var("DISPLAY", ":0");

        if let Some(username) = get_x11_authenicated_client() {
            /* 
             * Environment variables needed to connect to 
             * user graphical user session from root 
             */
            let xauthority_path = format!("/home/{}/.Xauthority", username);
            debug!("Xauthority User Path: {}", xauthority_path);
            env::set_var("XAUTHORITY", xauthority_path);

            /* 
             * Environment variables needed for pipewire connection from root. 
             */ 
            let user_id_cmd = format!("id -u {}", username);
            let user_id_output = Command::new("sh")
                .arg("-c")
                .arg(user_id_cmd)
                .output()
                .unwrap();

            let user_id = String::from_utf8(user_id_output.stdout).unwrap();
            let xdg_runtime_dir = format!("/run/user/{}", user_id.trim());
            let dbus_session_bus_address = format!("unix:path={}/bus", xdg_runtime_dir);

            debug!("XDG_RUNTIME_DIR: {}", &xdg_runtime_dir);
            debug!("DBUS_SESSION_BUS_ADDRESS: {}", &dbus_session_bus_address);

            env::set_var("XDG_RUNTIME_DIR", xdg_runtime_dir);
            env::set_var("DBUS_SESSION_BUS_ADDRESS", dbus_session_bus_address);

            return Ok(Setting::PostLogin);
        }

        debug!("No user logged in to graphical session");
        env::set_var("XAUTHORITY", "/var/lib/lightdm/.Xauthority");
        return Ok(Setting::PreLogin);
    }

    #[inline]
    #[allow(dead_code)]
    fn write_stream(&mut self, bitstream: &[u8]) {
        if let Some(file) = &mut self.file {
            file.write(bitstream).unwrap();
        }
    }

    fn get_parameters(meta: RwLockReadGuard<'_, ServerMetaData>) -> Param {
        let mut par = Param::default_preset("ultrafast", "zerolatency").unwrap();

        par = par.set_csp(EColorSpace::YUV444.into());
        par = par.set_dimension(meta.height, meta.width);
        if cfg!(target_os = "windows") {
            par = par.set_fullrange(1);
        }

        par = par.param_parse("repeat_headers", "1").unwrap();
        par = par.param_parse("annexb", "1").unwrap();
        par = par.param_parse("bframes", "0").unwrap();
        par = par.param_parse("crf", "20").unwrap();
        par = par.apply_profile("high444").unwrap();

        par
    }

    fn drop_capturer(&mut self) {
        if let Some(capturer) = self.capturer.take() {
            debug!("Dropping Capturer");
            drop(capturer);
        }
    }

    fn restart_stream(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let display = Display::primary()?;

        let capturer: Capturer = Capturer::new(display)?;
 
        self.row_len = 4 * capturer.width() * capturer.height();
        self.conn
            .set_dimensions(capturer.width(), capturer.height());
        self.par = VideoServerThread::get_parameters(self.conn.get_meta());
        self.encoder = x264::Encoder::open(&mut self.par)?;

        if self.deployer.has_sym_key() {
            let headers = self.encoder.get_headers()?;
            let header_bytes = headers.as_bytes();
            self.deployer.prepare(
                &header_bytes,
                Box::new(|subpacket| {
                    self.conn.broadcast(&subpacket);
                }),
            );
        }

        self.pic = Picture::from_param(&self.par)?;

        self.yuv_handles.clear();
        self.capturer = Some(capturer);

        Ok(())
    }

    fn handle_server_action(
        &mut self, 
        server_action: Option<VideoServerAction>,
        video_server_ch_sender: &kanal::Sender<VideoServerAction>
    ) {
        match server_action {
            Some(VideoServerAction::RestartSession) => {
                match VideoServerThread::config_xenv() {
                    Ok(Setting::PostLogin) => {
                        self.setting = Setting::PostLogin;
                    }
                    Ok(Setting::PreLogin) => {
                        self.setting = Setting::PreLogin;
                        self.start_session_thread(video_server_ch_sender.clone());
                    }
                    _ => {}
                }

                video_server_ch_sender.send(VideoServerAction::RestartStream).unwrap();
                self.events_sender.send(EventsThreadAction::ReconnectInputModules).unwrap();
            }
            Some(VideoServerAction::NewUserSession) => {
                match VideoServerThread::config_xenv() {
                    Ok(Setting::PostLogin) => {
                        self.setting = Setting::PostLogin;

                        // TODO: This does not work, maybe this needs to be done after more time,
                        // TODO: because the resolution does not change.
                        
                        let requested_width = self.conn.get_meta().width;
                        let requested_height = self.conn.get_meta().height;

                        if let Err(e) = DisplayMeta::update_display_resolution(
                            requested_width, requested_height
                        ) {
                            debug!("Error syncing display resolution after login: {}", e.to_string());
                        }
                        
                        video_server_ch_sender.send(VideoServerAction::RestartStream).unwrap();
                    }
                    _ => {}
                }
            }
            Some(VideoServerAction::Inactive) => {
                self.encoder = x264::Encoder::open(&mut self.par).unwrap();
            }
            Some(VideoServerAction::SymKey) => {
                if let Some(sym_key) = self.conn.get_sym_key() {
                    self.deployer.set_sym_key(sym_key.clone());
                }
            }
            Some(VideoServerAction::RestartStream) => {
                self.drop_capturer();
                match self.restart_stream() {
                    Ok(_) => {
                        debug!("Restarted Stream Successfully");
                    }
                    Err(e) => {
                        debug!("Error Restarting Stream: {}", e);
                        video_server_ch_sender.send(VideoServerAction::RestartStream).unwrap();
                    }
                }
            }
            Some(VideoServerAction::ConfigUpdate) => {
                let requested_width = self.conn.get_meta().width;
                let requested_height = self.conn.get_meta().height;

                if let Err(e) = DisplayMeta::update_display_resolution(requested_width, requested_height) {
                    debug!("Error updating display resolution: {}", e);
                }

                video_server_ch_sender.send(VideoServerAction::RestartStream).unwrap();
            }
            None => {
                return;
            }
        }
    }

    fn start_session_thread(&mut self, ch_sender: Sender<VideoServerAction>) -> bool {
        let has_setting_thread = match &self.setting_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_setting_thread { return false; }

        self.setting_thread = Some(SessionSettingThread::run(ch_sender));
        true
    }

    fn start_events_thread(
        &mut self, 
        headers: Vec<u8>, 
        ch_sender: Sender<VideoServerAction>) -> bool {
        let has_events_thread = match &self.events_thread {
            Some(handle) => !handle.is_finished(),
            None => false,
        };

        if has_events_thread { return false; }

        self.events_thread = Some(EventsThread::run(
            &self.conn, 
            headers,
            ch_sender, 
            self.events_receiver.clone()
        ));

        true
    }

    #[inline]
    pub async fn run(&mut self) {
        let (ch_sender, ch_receiver) = unbounded::<VideoServerAction>();

        let mut frames = 0u8;
        let mut fps_time = Instant::now();

        // Send update to client to update headers
        let headers = self.encoder.get_headers().unwrap();

        self.start_events_thread(headers.as_bytes().to_vec(), ch_sender.clone());
        
        if self.setting == Setting::PreLogin {
            self.start_session_thread(ch_sender.clone());
        }   

        loop {
            while ch_receiver.len() > 0 {
                if let Ok(server_action) = ch_receiver.try_recv_realtime() {
                    self.handle_server_action(server_action, &ch_sender);
                }
            }

            let capturer = match &mut self.capturer {
                Some(capturer) => capturer,
                None => {
                    continue;
                }
            };

            if !self.conn.has_clients() {
                self.conn.filter_clients();
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }

            let sleep = Instant::now();
            let width = capturer.width();
            let height = capturer.height();

            match capturer.frame() {
                Ok(frame) => {
                    let bgra_frame = frame.chunks(self.row_len).next().unwrap().to_vec();

                    if (width * height * 4) != bgra_frame.len() {
                        debug!("Frame size: {} Expected: {}", bgra_frame.len(), width * height * 4);
                    }

                    if bgra_frame.len() < (width * height * 4) {
                        debug!("Frame size less than expected");
                        continue;
                    }

                    let cvt_rgb_yuv = async move {
                        // TODO: figure out why this is neccessary
                        let yuv = YUVBuffer::with_bgra_for_444(
                            width,
                            height,
                            &bgra_frame[0..width * height * 4],
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
                        u_plane.copy_from_slice(yuv.u_444());
                        let v_plane = self.pic.as_mut_slice(2).unwrap();
                        v_plane.copy_from_slice(yuv.v_444());

                        // TODO: Is this important?
                        //self.pic.set_timestamp(self.frame_count);

                        if let Some((nal, _, _)) = self.encoder.encode(&self.pic).unwrap() {
                            self.deployer.prepare(
                                &nal.as_bytes(),
                                Box::new(|subpacket| {
                                    self.conn.broadcast(&subpacket);
                                }),
                            );
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
                    println!("Error: {}", e);
                    debug!("Error Capturing Frame")
                }
            }
        }
    }
}
