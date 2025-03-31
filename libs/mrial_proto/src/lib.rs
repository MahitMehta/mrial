pub mod conn;
pub mod deploy;
pub mod input;
pub mod packet;
pub mod video;

pub use conn::*;
pub use packet::*;

pub const SERVER_PING_TOLERANCE: u64 = 6;
pub const CLIENT_PING_FREQUENCY: u64 = (SERVER_PING_TOLERANCE / 3) as u64;
