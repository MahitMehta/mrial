use crate::conn::Connections;

use super::{AudioController, IAudioController};


impl IAudioController for AudioController {
    fn begin_transmission(&self, conn: Connections) {
        
    }
}
