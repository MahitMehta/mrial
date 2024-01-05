#[cfg(target_os = "linux")]
use mouse_keyboard_input;

pub struct EventEmitter {
    #[cfg(target_os = "linux")]
    device: mouse_keyboard_input::VirtualDevice,
}

impl EventEmitter {
    #[cfg(target_os = "linux")]
    pub fn new() -> Self {
        use std::time::Duration;

        let device = mouse_keyboard_input::VirtualDevice::new(
            Duration::new(0.033 as u64, 
                0), 2000
            ).unwrap();

        Self {
            device,
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new() -> Self {
        Self {
        
        }
    }

    // sudo apt install libudev-dev libevdev-dev libhidapi-dev
    // sudo usermod -a -G input user
    // sudo reboot
    
    #[cfg(target_os = "linux")]
    pub fn scroll(&mut self, x: i32, y: i32) {
        if x != 0 {
            let _ = &self.device.scroll_x(-x * 3);
        }
        
        if y != 0 {
            let _ = &self.device.scroll_y(-y * 3);
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn scroll(&self, x: i32, y: i32) {
        
    }
}