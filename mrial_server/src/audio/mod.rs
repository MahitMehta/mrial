mod encoder;

use crate::conn::Connection;

pub use self::encoder::*;

pub trait IAudioController {
    fn run(&self, conn: Connection);
}

pub struct AudioServerThread {}

impl AudioServerThread {
    pub fn new() -> AudioServerThread {
        AudioServerThread {}
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        mod linux;
        pub use self::linux::*;
    } else if #[cfg(target_os = "windows")] {
        mod windows;
        pub use self::windows::*;
    } else if #[cfg(target_os = "macos")] {
        mod macos;
        pub use self::macos::*;
    } else {
        compile_error!("Unsupported OS");
    }
}
