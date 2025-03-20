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

#[derive(Clone, Copy)]
pub enum Key {
    DownArrow = 1,
    UpArrow = 2,
    LeftArrow = 3,
    RightArrow = 4,
    Space = 32,
    Backspace = 8,
    Tab = 9,
    Return = 10,
    None = 0,
    Unicode = 65,
}

impl From<u8> for Key {
    fn from(byte: u8) -> Self {
        match byte {
            1 => Key::DownArrow,
            2 => Key::UpArrow,
            3 => Key::LeftArrow,
            4 => Key::RightArrow,
            9 => Key::Tab,
            10 => Key::Return,
            32 => Key::Space,
            8 => Key::Backspace,
            0 => Key::None,
            _ => {
                // refine ascii range check
                if byte >= 33 {
                    Key::Unicode
                } else {
                    Key::None
                }
            }
        }
    }
}

impl Into<u8> for Key {
    fn into(self) -> u8 {
        self as u8
    }
}

impl PartialEq<u8> for Key {
    fn eq(&self, other: &u8) -> bool {
        *self as u8 == *other
    }
}

#[derive(Clone, Copy)]
pub enum KeyEvent {
    Press = 1,
    Release = 2,
    None = 0,
}

impl Into<u8> for KeyEvent {
    fn into(self) -> u8 {
        self as u8
    }
}

impl PartialEq<u8> for KeyEvent {
    fn eq(&self, other: &u8) -> bool {
        *self as u8 == *other
    }
}

pub const PAYLOAD: usize = 24;

#[inline]
pub fn is_control_pressed(buf: &[u8]) -> bool {
    KeyEvent::Press == buf[0]
}

#[inline]
pub fn is_control_released(buf: &[u8]) -> bool {
    KeyEvent::Release == buf[0]
}

#[inline]
pub fn is_shift_pressed(buf: &[u8]) -> bool {
    KeyEvent::Press == buf[1]
}

#[inline]
pub fn is_shift_released(buf: &[u8]) -> bool {
    KeyEvent::Release == buf[1]
}

#[inline]
pub fn is_alt_pressed(buf: &[u8]) -> bool {
    KeyEvent::Press == buf[2]
}

#[inline]
pub fn is_alt_released(buf: &[u8]) -> bool {
    KeyEvent::Release == buf[2]
}

#[inline]
pub fn is_meta_pressed(buf: &[u8]) -> bool {
    KeyEvent::Press == buf[3]
}

#[inline]
pub fn is_meta_released(buf: &[u8]) -> bool {
    KeyEvent::Release == buf[3]
}

#[inline]
pub fn write_click(buf: &mut [u8], x: f32, y: f32, width: f32, height: f32, right: bool) {
    let x_percent = (x / width * 10000.0).round() as u16 + 1;
    let y_percent = (y / height * 10000.0).round() as u16 + 1;

    buf[4..6].copy_from_slice(&x_percent.to_be_bytes());
    buf[6..8].copy_from_slice(&y_percent.to_be_bytes());

    if right {
        let mask = 1 << 7;
        buf[4] = buf[4] & !mask
    }
}

#[inline]
pub fn reset_click(buf: &mut [u8]) {
    buf[4..8].copy_from_slice(&[0; 4]);
}

#[inline]
pub fn click_requested(buf: &[u8]) -> bool {
    buf[5] != 0 || buf[7] != 0
}

#[inline]
pub fn parse_click(buf: &[u8], width: usize, height: usize) -> (i32, i32, bool) {
    let mut right_click = false;

    let mut x_percent_bytes = [0u8; 2];
    x_percent_bytes.copy_from_slice(&buf[4..6]);

    if x_percent_bytes[0] >> 7 == 1 {
        right_click = true;
        let mask = 1 << 7;
        x_percent_bytes[0] = x_percent_bytes[0] & !mask
    }

    let x_percent = u16::from_be_bytes(x_percent_bytes) - 1;
    let y_percent = u16::from_be_bytes(buf[6..8].try_into().unwrap()) - 1;

    let x = (x_percent as f32 / 10000.0 * width as f32).round() as i32;
    let y = (y_percent as f32 / 10000.0 * height as f32).round() as i32;

    (x, y, right_click)
}

#[inline]
pub fn mouse_move_requested(buf: &[u8]) -> bool {
    buf[10] != 0 || buf[12] != 0
}

#[inline]
pub fn scroll_requested(buf: &[u8]) -> bool {
    buf[15] != 0 || buf[17] != 0
}

#[inline]
pub fn write_mouse_move(buf: &mut [u8], x: f32, y: f32, width: f32, height: f32, pressed: bool) {
    let x_percent = (x / width * 10000.0).round() as u16 + 1;
    let y_percent = (y / height * 10000.0).round() as u16 + 1;

    buf[10..12].copy_from_slice(&x_percent.to_be_bytes());
    buf[12..14].copy_from_slice(&y_percent.to_be_bytes());

    buf[14] = pressed as u8;
}

#[inline]
pub fn parse_mouse_move(buf: &[u8], width: f32, height: f32) -> (i32, i32, bool) {
    let x_percent = u16::from_be_bytes(buf[10..12].try_into().unwrap()) - 1;
    let y_percent = u16::from_be_bytes(buf[12..14].try_into().unwrap()) - 1;

    let x = (x_percent as f32 / 10000.0 * width).round() as i32;
    let y = (y_percent as f32 / 10000.0 * height).round() as i32;

    let pressed = buf[14] == 1;

    (x, y, pressed)
}

#[inline]
pub fn write_scroll(buf: &mut [u8], delta_x: i16, delta_y: i16) {
    buf[14..16].copy_from_slice(&delta_x.to_be_bytes());
    buf[16..18].copy_from_slice(&delta_y.to_be_bytes());
}
