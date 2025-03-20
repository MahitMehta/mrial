use super::{AudioServerThread, IAudioStream};

impl IAudioStream for AudioServerThread {
    async fn stream(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::warn!("Audio streaming is not supported on Windows.");

        Ok(())
    }
}
