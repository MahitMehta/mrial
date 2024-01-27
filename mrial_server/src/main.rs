// dependencies: libxcb-randr0-dev

mod audio;
mod conn;
mod events;
mod video;

use audio::{AudioServerThread, IAudioController};
use conn::Connection;
use video::{VideoServerActions, VideoServerThread};

#[tokio::main]
async fn main() {
    let conn: Connection = Connection::new();

    let mut video_server = VideoServerThread::new(conn.clone());
    let audio_server = AudioServerThread::new();

    audio_server.run(conn);
    video_server.run().await;
}
