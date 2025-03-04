use super::{AudioServerThread, IAudioStream};

impl IAudioStream for AudioServerThread {
    fn stream(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::warn!("Audio streaming is not supported on macOS.");

        Ok(())
    }
}
