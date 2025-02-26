use crate::conn::ConnectionManager;

use super::{AudioServerThread, IAudioController};

impl IAudioController for AudioServerThread {
    fn run(&self, conn: ConnectionManager) {
        println!("AudioServerThread Unimplemented on MacOS");
    }
}
