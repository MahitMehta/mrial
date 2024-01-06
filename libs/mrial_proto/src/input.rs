use super::HEADER;

pub const PAYLOAD : usize = 24; 

#[inline]
pub fn write_click(
    x_percent: u16,
    y_percent: u16,
    clicked: bool,
    right: bool,
    buf: &mut [u8]
) {
    if clicked {
        buf[HEADER + 4..HEADER + 6].copy_from_slice(&x_percent.to_be_bytes());
        buf[HEADER + 6..HEADER + 8].copy_from_slice(&y_percent.to_be_bytes());
    } else {
        buf[HEADER + 4..HEADER + 6].copy_from_slice(&[0; 2]);
        buf[HEADER + 6..HEADER + 8].copy_from_slice(&[0; 2]);
    }

    if right {
        let mask = 1 << 7; 
        buf[HEADER + 4] = (buf[HEADER + 4] & !mask) | (1 << 7); 
    }
}

#[inline]
pub fn click_requested(buf: &[u8]) -> bool {
    buf[HEADER + 5] != 0 || buf[HEADER + 7] != 0
}

#[inline]
pub fn parse_mouse_position(buf: &mut [u8], width: usize, height: usize) -> (i32, i32, bool) {
    let mut right_click = false; 

    if buf[HEADER + 4] >> 7 == 1 {
        right_click = true; 
        let mask = 1 << 7; 
        buf[HEADER + 4] = (buf[HEADER + 4] & !mask) | (0 << 7);
    }

    let x_percent = u16::from_be_bytes(buf[HEADER + 4..HEADER + 6].try_into().unwrap()) - 1;
    let y_percent = u16::from_be_bytes(buf[HEADER + 6..HEADER + 8].try_into().unwrap()) - 1;

    let x = (x_percent as f32 / 10000.0 * width as f32).round() as i32;
    let y = (y_percent as f32 / 10000.0 * height as f32).round() as i32;

    (x, y, right_click)
}