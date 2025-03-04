pub mod conn;
pub mod input;
pub mod packet;

pub use conn::*;
pub use packet::*;

pub const SERVER_PING_TOLERANCE: u64 = 6;
pub const CLIENT_PING_FREQUENCY: u64 = (SERVER_PING_TOLERANCE / 2) as u64;
