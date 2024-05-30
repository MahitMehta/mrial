pub mod display;
pub mod yuv;

use chacha20poly1305::{aead::{Aead, AeadMutInPlace}, AeadCore, ChaCha20Poly1305};
use display::DisplayMeta;
use futures::{executor::ThreadPool, future::RemoteHandle, task::SpawnExt};
use kanal::unbounded;
use log::debug;
use mrial_proto::*;
use rand::rngs::ThreadRng;
use scrap::{Capturer, Display};
use spin_sleep;
use std::{
    borrow::BorrowMut, collections::VecDeque, fs::File, io::{ErrorKind::WouldBlock, Write}, sync::RwLockReadGuard, time::{Duration, Instant}
};
use x264::{Encoder, Param, Picture};
use yuv::YUVBuffer;

use crate::{
    conn::{Connection, ServerMetaData},
    events::EventsThread,
};

use self::yuv::EColorSpace;

#[derive(PartialEq)]
pub enum VideoServerActions {
    Inactive,
    ConfigUpdate,
    SymKey
}

pub struct VideoServerThread {
    conn: Connection,
    file: Option<File>,
    capturer: Capturer,
    frame_count: i64,
    packet_id: u8,
    buf: [u8; MTU],
    row_len: usize,

    par: Param,
    encoder: Encoder,
    rng: ThreadRng,
    sym_key: Option<ChaCha20Poly1305>
}

impl VideoServerThread {
    pub fn new(conn: Connection) -> Self {
        let display: Display = Display::primary().unwrap();
        let capturer = Capturer::new(display).unwrap();

        conn.set_dimensions(capturer.width(), capturer.height());

        let row_len = 4 * conn.get_meta().width * conn.get_meta().width;

        let mut par: Param = VideoServerThread::get_parameters(conn.get_meta());
        let encoder = x264::Encoder::open(&mut par).unwrap();

        let mut buf = [0u8; MTU]; 
        write_packet_type(EPacketType::NAL, &mut buf);

        Self {
            conn,
            row_len,
            file: None,
            capturer,
            frame_count: 0,
            packet_id: 1,
            buf,

            par,
            encoder,
            rng: rand::thread_rng(),
            sym_key: None
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

    #[inline]
    pub async fn run(&mut self) {
        let (ch_sender, ch_receiver) = unbounded::<VideoServerActions>();

        let mut pic = Picture::from_param(&self.par).unwrap();

        let pool = ThreadPool::builder().pool_size(1).create().unwrap();
        let mut yuv_handles: VecDeque<RemoteHandle<YUVBuffer>> = VecDeque::new();

        let mut frames = 0u8;
        let mut fps_time = Instant::now();

        let mut headers = self.encoder.get_headers().unwrap();

        let events = EventsThread::new();
        events.run(
            &mut self.conn,
            headers.as_bytes().to_vec(),
            ch_sender.clone(),
        );

        loop {
            while ch_receiver.len() > 0 {
                match ch_receiver.try_recv_realtime().unwrap() {
                    Some(VideoServerActions::Inactive) => {
                        self.encoder = x264::Encoder::open(&mut self.par).unwrap();
                    }
                    Some(VideoServerActions::SymKey) => {
                        if let Some(sym_key) = self.conn.get_sym_key() {
                            self.sym_key = Some(sym_key);
                        }
                    }
                    Some(VideoServerActions::ConfigUpdate) => {
                        let requested_width = self.conn.get_meta().width;
                        let requested_height = self.conn.get_meta().height;

                        if requested_width == self.capturer.width() as usize
                            && requested_height == self.capturer.height() as usize
                        {
                            continue;
                        }

                        let _updated_resolution = match DisplayMeta::update_display_resolution(
                            requested_width,
                            requested_height,
                        ) {
                            Ok(updated) => updated,
                            Err(e) => {
                                println!("Error updating display resolution: {}", e);
                                false
                            }
                        };

                        let display = Display::primary().unwrap();
                        self.capturer = Capturer::new(display).unwrap();

                        self.conn
                            .set_dimensions(self.capturer.width(), self.capturer.height());

                        self.par = VideoServerThread::get_parameters(self.conn.get_meta());

                        self.encoder = x264::Encoder::open(&mut self.par).unwrap();

                        if let Some(sym_key) = &self.sym_key {
                            headers = self.encoder.get_headers().unwrap();
                            let header_bytes = headers.as_bytes();
                            let nonce = ChaCha20Poly1305::generate_nonce(&mut self.rng);
                            let mut ciphertext = sym_key.encrypt(&nonce, header_bytes).unwrap();
                            ciphertext.extend_from_slice(&nonce);
    
                            let mut buf = [0u8; MTU];
                            write_header(
                                EPacketType::NAL, 
                                0, 
                                (HEADER + ciphertext.len()) as u32,
                                0,
                                &mut buf
                            );
                            buf[HEADER..HEADER + ciphertext.len()].copy_from_slice(&ciphertext);
                            self.conn.broadcast(&buf[0..HEADER + ciphertext.len()]);
                        }

                        pic = Picture::from_param(&self.par).unwrap();

                        yuv_handles.clear();
                    }
                    None => {
                        break;
                    }
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
                    yuv_handles.push_back(pool.spawn_with_handle(cvt_rgb_yuv).unwrap());

                    // set to 1 to increase FPS at the cost of latency, or 0  for the opposite effect
                    if yuv_handles.len() > 0 {
                        //let start = Instant::now();
                        let yuv = yuv_handles.pop_front().unwrap().await;

                        let y_plane = pic.as_mut_slice(0).unwrap();
                        y_plane.copy_from_slice(yuv.y());
                        let u_plane = pic.as_mut_slice(1).unwrap();
                        u_plane.copy_from_slice(yuv.u_444());
                        let v_plane = pic.as_mut_slice(2).unwrap();
                        v_plane.copy_from_slice(yuv.v_444());

                        pic = pic.set_timestamp(self.frame_count);
                        self.frame_count += 1;

                        if let Some((nal, _, _)) = self.encoder.encode(&pic).unwrap() {
                            if let Some(sym_key) = &self.sym_key {
                                let bitstream = nal.as_bytes();
                                let nonce = ChaCha20Poly1305::generate_nonce(&mut self.rng);
                                let mut ciphertext = sym_key.encrypt(&nonce, bitstream).unwrap();
                                ciphertext.extend_from_slice(&nonce);
                                
                                let packets = (ciphertext.len() as f64 / PAYLOAD as f64).ceil() as usize;

                                write_var_frame_header(
                                    ciphertext.len().try_into().unwrap(),
                                    self.packet_id,
                                    &mut self.buf,
                                );

                                self.packet_id += 1;

                                for i in 0..packets {
                                    write_packets_remaining(
                                        (packets - i - 1).try_into().unwrap(),
                                        &mut self.buf,
                                    );

                                    let start = i * PAYLOAD;
                                    let addition = if start + PAYLOAD <= ciphertext.len() {
                                        PAYLOAD
                                    } else {
                                        ciphertext.len() - start
                                    };
                                    self.buf[HEADER..addition + HEADER]
                                        .copy_from_slice(&ciphertext[start..(addition + start)]);

                                    self.conn.broadcast(&self.buf[0..addition + HEADER]);
                                }
                                frames += 1;
                            }
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
                    println!("Error Capturing Frame")
                }
            }
        }
    }
}
