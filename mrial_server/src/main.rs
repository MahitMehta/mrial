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
    let mut conn: Connection = Connection::new();

    let audio_server = AudioServerThread::new();
    audio_server.run(conn.clone());

    let mut video_server = VideoServerThread::new();
    video_server.run(&mut conn).await;
}
