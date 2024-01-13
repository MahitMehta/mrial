pub mod input; 
pub mod packet;
pub mod conn; 

pub use packet::*;
pub use conn::*;

pub const SERVER_PING_TOLERANCE : u64 = 6; 
pub const CLIENT_PING_FREQUENCY : u64 = (SERVER_PING_TOLERANCE / 2) as u64; 