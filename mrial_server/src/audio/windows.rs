use crate::conn::Connection;

use super::{AudioController, IAudioController};

impl IAudioController for AudioController {
    fn begin_transmission(&self, conn: Connection) {}
}
