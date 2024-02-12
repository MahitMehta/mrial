#[cfg(target_os = "linux")]
use xrandr::{ScreenResources, XHandle};

pub struct DisplayMeta {}

impl DisplayMeta {
    #[cfg(target_os = "linux")]
    pub fn get_display_resolutions() -> Result<(Vec<u16>, Vec<u16>), xrandr::XrandrError> {
        let mut handle = XHandle::open().unwrap();
        let mon1 = &handle.monitors()?[0];

        let mut widths: Vec<u16> = Vec::new();
        let mut heights: Vec<u16> = Vec::new();

        let res = ScreenResources::new(&mut handle)?;
        res.modes.iter().for_each(|m| {
            widths.push(m.width as u16);
            heights.push(m.height as u16);
        });

        Ok((widths, heights))
    }

    #[cfg(target_os = "linux")]
    pub fn update_display_resolution(
        width: usize,
        height: usize,
    ) -> Result<bool, xrandr::XrandrError> {
        let mut handle = XHandle::open().unwrap();
        let mon1 = &handle.monitors()?[0];

        if mon1.width_px == width as i32 && mon1.height_px == height as i32 {
            return Ok(false);
        }

        let res = ScreenResources::new(&mut handle)?;
        let requested_mode = res
            .modes
            .iter()
            .find(|m| m.width == width as u32 && m.height == height as u32);

        // TODO: Handle the possibility of the mode not existing
        if let Some(mode) = requested_mode {
            handle.set_mode(&mon1.outputs[0], &mode)?;
            return Ok(true);
        }

        Ok(false)
    }

    #[cfg(target_os = "windows")]
    pub fn update_display_resolution(width: usize, height: usize) -> Result<bool, std::io::Error> {
        Ok(false)
    }

    #[cfg(target_os = "macos")]
    pub fn update_display_resolution(width: usize, height: usize) -> Result<bool, std::io::Error> {
        Ok(false)
    }
}
