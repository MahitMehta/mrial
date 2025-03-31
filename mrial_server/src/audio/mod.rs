mod encoder;

pub use self::encoder::*;
use crate::conn::{BroadcastTaskError, ConnectionManager};

use kanal::Receiver;
use log::{debug, error};
use mrial_proto::EPacketType;
use tokio::task::JoinHandle;

pub trait IAudioStream {
    async fn stream(&self) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct AudioServerTask {
    conn: ConnectionManager,
    receiver: Receiver<AudioServerAction>,
}

#[derive(Debug)]
pub enum AudioServerAction {
}

async fn broadcast_web_audio(conn: &ConnectionManager, packet_type: EPacketType, bytes: &[u8]) {
    if let Err(e) = conn.web_encrypted_broadcast(packet_type, bytes) {
        match e {
            BroadcastTaskError::TaskNotRunning => {
                error!("Web Broadcast Task Not Running");

                debug!("Restarting Web Broadcast Task");
                conn.get_web().start_broadcast_async_task();
            }
            BroadcastTaskError::TransferFailed(msg) => {
                error!("Web Broadcast Send Error: {msg}");
            }
            _ => {}
        }
    }
}

async fn broadcast_app_audio(conn: &ConnectionManager, packet_type: EPacketType, bytes: &[u8]) {
    if let Err(e) = conn.app_encrypted_broadcast(packet_type, 0, bytes).await {
        match e {
            BroadcastTaskError::TaskNotRunning => {
                error!("App Broadcast Task Not Running");

                debug!("Restarting Web Broadcast Task");
                conn.get_app().start_broadcast_async_task();
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
