use super::Connection;

#[derive(Debug, Clone)]
struct WebClient {}

pub struct WebConnection {
    clients: Vec<WebClient>,
}

impl WebConnection {
    pub fn new() -> Self {
        Self { clients: vec![] }
    }
}

impl Connection for WebConnection {
    fn filter_clients(&self) {
        // Filter clients
    }

    fn has_clients(&self) -> bool {
        // Check if any clients are connected
        false
    }
}

impl Clone for WebConnection {
    fn clone(&self) -> Self {
        Self {
            clients: self.clients.clone(),
        }
    }
}
