mod encoder;

pub use self::encoder::*;
use crate::conn::ConnectionManager;

use kanal::Receiver;
use tokio::task::JoinHandle;

pub trait IAudioStream {
    async fn stream(&self) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct AudioServerTask {
    conn: ConnectionManager,
    receiver: Receiver<AudioServerAction>,
}

#[derive(Debug)]
pub enum AudioServerAction {}

impl AudioServerTask {
    pub fn new(conn: ConnectionManager, receiver: Receiver<AudioServerAction>) -> Self {
        Self { conn, receiver }
    }

    pub fn run(conn: ConnectionManager, receiver: Receiver<AudioServerAction>) -> JoinHandle<()> {
        let tokio_handle = tokio::runtime::Handle::current();
        tokio_handle.spawn(async move {
            let server = AudioServerTask::new(conn, receiver);

            if let Err(e) = server.stream().await {
                log::error!("Failed to start streaming audio: {}", e);
            }
        })
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        mod linux;
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
