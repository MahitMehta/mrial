use super::HEADER;

// State Payload:

// 1 Byte for Control 
// 1 Byte for Shift
// 1 Byte for Alt
// 1 Byte for Meta
// 2 Bytes for X for click, first bit for right click
// 2 Bytes for Y for click
// 1 Byte for key pressed
// 1 Byte for key released
// 2 Bytes for X location
// 2 Bytes for Y location
// 1 Byte for mouse_move
// 2 Bytes for X scroll delta
// 2 Bytes for Y scroll delta

pub const PAYLOAD : usize = 24; 

#[inline]
pub fn write_click(
    x: f32,
    y: f32,
    width: usize,
    height: usize,
    right: bool,
    buf: &mut [u8]
) {
    let x_percent = (x / (width as f32) * 10000.0).round() as u16 + 1; 
    let y_percent = (y / (height as f32)  * 10000.0).round() as u16 + 1;

    buf[HEADER + 4..HEADER + 6].copy_from_slice(&x_percent.to_be_bytes());
    buf[HEADER + 6..HEADER + 8].copy_from_slice(&y_percent.to_be_bytes());
     
    if right {
        let mask = 1 << 7; 
        buf[HEADER + 4] = (buf[HEADER + 4] & !mask) | (1 << 7); 
    }
}

#[inline] 
pub fn reset_click(buf: &mut [u8]) {
    buf[HEADER + 4..HEADER + 8].copy_from_slice(&[0; 4]);
}

#[inline]
pub fn click_requested(buf: &[u8]) -> bool {
    buf[HEADER + 5] != 0 || buf[HEADER + 7] != 0
}

#[inline]
pub fn parse_click(buf: &mut [u8], width: usize, height: usize) -> (i32, i32, bool) {
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

#[inline]
pub fn mouse_move_requested(buf: &[u8]) -> bool {
    buf[HEADER + 10] != 0 || buf[HEADER + 12] != 0
}