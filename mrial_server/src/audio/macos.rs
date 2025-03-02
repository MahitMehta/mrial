use crate::conn::ConnectionManager;

use super::{AudioServerThread, IAudioStream, AudioServerAction};

use std::thread::JoinHandle;
use kanal::Receiver;

impl IAudioStream for AudioServerThread {
    fn stream(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::warn!("Audio streaming is not supported on macOS.");

        Ok(())
    }
}
