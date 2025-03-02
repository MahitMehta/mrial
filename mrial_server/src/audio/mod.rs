
mod encoder;

pub use self::encoder::*;
use crate::conn::ConnectionManager;

use std::thread::JoinHandle;
use kanal::Receiver;

pub trait IAudioStream {
    fn stream(&self) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct AudioServerThread {
    conn: ConnectionManager,
    receiver: Receiver<AudioServerAction>,
}

#[derive(Debug)]
pub enum AudioServerAction {
    SymKey,
}

impl AudioServerThread {
    pub fn new(conn: ConnectionManager, receiver: Receiver<AudioServerAction>) -> Self {
        Self {
            conn,
            receiver,
        }
    }

    pub fn run(conn: ConnectionManager, receiver: Receiver<AudioServerAction>) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let server = AudioServerThread::new(conn, receiver);
            
            if let Err(e) = server.stream() {
                log::error!("Failed to start streaming audio: {}", e);
            }
        })
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        mod linux;
        pub use self::linux::*;
    } else if #[cfg(target_os = "windows")] {
        mod windows;
        pub use self::windows::*;
    } else if #[cfg(target_os = "macos")] {
        mod macos;
        pub use self::macos::*;
    } else {
        compile_error!("Unsupported OS");
    }
}
