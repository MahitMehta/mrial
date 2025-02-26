mod audio;
mod cli;
mod conn;
mod events;
mod video;

use std::env;

use audio::{AudioServerThread, IAudioController};
use cli::handle_cli;
use conn::ConnectionManager;
use video::VideoServerThread;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        handle_cli(&args);
        return Ok(());
    }

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    println!("Starting Mrial Server Version {}\n", VERSION);

    pretty_env_logger::init_timed();
    let conn = ConnectionManager::new();
    let conn_clone = conn.try_clone()?;

    let mut video_server = match VideoServerThread::new(conn_clone) {
        Ok(server) => server,
        Err(e) => {
            log::error!("Failed to start Video Server: {}", e);
            return Ok(());
        }
    };

    let audio_server = AudioServerThread::new();

    audio_server.run(conn);
    video_server.run().await;

    Ok(())
}
