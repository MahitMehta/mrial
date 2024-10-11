pub mod display;
pub mod yuv;

use deploy::PacketDeployer;
use display::DisplayMeta;
use futures::{executor::ThreadPool, future::RemoteHandle, task::SpawnExt};
use kanal::unbounded;
use log::debug;
use mrial_proto::*;
use scrap::{Capturer, Display};
use std::{
    collections::VecDeque,
    fs::File,
    io::{ErrorKind::WouldBlock, Write},
    sync::RwLockReadGuard,
    time::{Duration, Instant},
};
use x264::{Encoder, Param, Picture};
use yuv::YUVBuffer;

use crate::{
    conn::{Connection, ServerMetaData},
    events::EventsThread,
};

use self::yuv::EColorSpace;

#[derive(PartialEq)]
pub enum VideoServerAction {
    Inactive,
    ConfigUpdate,
    SymKey,
}

pub struct VideoServerThread {
    pool: ThreadPool,
    yuv_handles: VecDeque<RemoteHandle<YUVBuffer>>,
    file: Option<File>,
    row_len: usize,
    par: Param,
    pic: Picture,
    capturer: Capturer,
    encoder: Encoder,
    deployer: PacketDeployer,
    conn: Connection,
}

impl VideoServerThread {
    pub fn new(conn: Connection) -> Self {
        let display: Display = Display::primary().unwrap();
        let capturer = Capturer::new(display).unwrap();

        conn.set_dimensions(capturer.width(), capturer.height());

        let pool = ThreadPool::builder().pool_size(1).create().unwrap();
        let yuv_handles = VecDeque::new();

        let row_len = 4 * conn.get_meta().width * conn.get_meta().width;

        let mut par: Param = VideoServerThread::get_parameters(conn.get_meta());
        let encoder = x264::Encoder::open(&mut par).unwrap();
        let pic = Picture::from_param(&par).unwrap();

        Self {
            pool,
            yuv_handles,
            row_len,
            file: None,
            par,
            pic,
            capturer,
            encoder,
            deployer: PacketDeployer::new(EPacketType::NAL, true),
            conn,
        }
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

    fn handle_server_action(&mut self, server_action: Option<VideoServerAction>) {
        match server_action {
            Some(VideoServerAction::Inactive) => {
                self.encoder = x264::Encoder::open(&mut self.par).unwrap();
            }
            Some(VideoServerAction::SymKey) => {
                if let Some(sym_key) = self.conn.get_sym_key() {
                    self.deployer.set_sym_key(sym_key.clone());
                }
            }
            Some(VideoServerAction::ConfigUpdate) => {
                let requested_width = self.conn.get_meta().width;
                let requested_height = self.conn.get_meta().height;

                if requested_width == self.capturer.width() as usize
                    && requested_height == self.capturer.height() as usize
                {
                    return;
                }

                let _updated_resolution =
                    match DisplayMeta::update_display_resolution(requested_width, requested_height)
                    {
                        Ok(updated) => updated,
                        Err(e) => {
                            debug!("Error updating display resolution: {}", e);
                            false
                        }
                    };

                let display = Display::primary().unwrap();
                self.capturer = Capturer::new(display).unwrap();

                self.conn
                    .set_dimensions(self.capturer.width(), self.capturer.height());

                self.par = VideoServerThread::get_parameters(self.conn.get_meta());
                self.encoder = x264::Encoder::open(&mut self.par).unwrap();

                if self.deployer.has_sym_key() {
                    let headers = self.encoder.get_headers().unwrap();
                    let header_bytes = headers.as_bytes();
                    self.deployer.prepare(
                        &header_bytes,
                        Box::new(|subpacket| {
                            self.conn.broadcast(&subpacket);
                        }),
                    );
                }

                self.pic = Picture::from_param(&self.par).unwrap();

                self.yuv_handles.clear();
            }
            None => {
                return;
            }
        }
    }

    #[inline]
    pub async fn run(&mut self) {
        let (ch_sender, ch_receiver) = unbounded::<VideoServerAction>();

        let mut frames = 0u8;
        let mut fps_time = Instant::now();

        // Send update to client to update headers
        let headers = self.encoder.get_headers().unwrap();
        EventsThread::run(&self.conn, headers.as_bytes().to_vec(), ch_sender.clone());

        loop {
            while ch_receiver.len() > 0 {
                if let Ok(server_action) = ch_receiver.try_recv_realtime() {
                    self.handle_server_action(server_action);
                }
            }

            if !self.conn.has_clients() {
                self.conn.filter_clients();
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }

            let sleep = Instant::now();
            let width = self.capturer.width();
            let height = self.capturer.height();

            match self.capturer.frame() {
                Ok(frame) => {
                    let bgra_frame = frame.chunks(self.row_len).next().unwrap().to_vec();

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
                Err(_) => {
                    debug!("Error Capturing Frame")
                }
            }
        }
    }
}
