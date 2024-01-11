mod encoder;

use crate::conn::Connections;

pub use self::encoder::*; 

pub trait IAudioController {
    fn begin_transmission(&self, conn: Connections);  
} 

pub struct AudioController {
    
}

impl AudioController {
    pub fn new() -> AudioController {
        AudioController {

        }
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