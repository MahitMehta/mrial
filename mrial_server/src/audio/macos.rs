use super::{AudioServerTask, IAudioStream};

impl IAudioStream for AudioServerTask {
    async fn stream(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::warn!("Audio streaming is not supported on macOS.");

        Ok(())
    }
}
