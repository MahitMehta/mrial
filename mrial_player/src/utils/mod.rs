pub mod state;

#[derive(PartialEq)]
pub enum ConnectionAction {
    Disconnect,
    Connect,
    Reconnect,
    Handshake,
    UpdateState,
    CloseApplication,
    Volume,
}
