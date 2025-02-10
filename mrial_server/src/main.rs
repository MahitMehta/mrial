mod audio;
mod conn;
mod events;
mod video;

use audio::{AudioServerThread, IAudioController};
use conn::Connection;
use video::{VideoServerAction, VideoServerThread};

#[tokio::main]
async fn main() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    println!("Starting Mrial Server Version {}\n", VERSION);

    pretty_env_logger::init_timed();
    let conn: Connection = Connection::new();

    let mut video_server = match VideoServerThread::new(conn.clone()) {
        Ok(server) => server,
        Err(e) => {
            log::error!("Failed to start Video Server: {}", e);
            return;
        }
    };

    let audio_server = AudioServerThread::new();

    audio_server.run(conn);
    video_server.run().await;
}
