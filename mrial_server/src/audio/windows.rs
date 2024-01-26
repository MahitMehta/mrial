use crate::conn::Connection;

use super::{AudioServerThread, IAudioController};

impl IAudioController for AudioServerThread {
    fn run(&self, conn: Connection) {
        println!("AudioServerThread Unimplemented on MacOS");
    }
}
