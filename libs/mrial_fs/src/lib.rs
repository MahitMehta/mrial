use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    sync::{Arc, Mutex},
};
use storage::{StorageMulti, StorageMultiType, StorageSingleton, StorageSingletonType};

pub mod storage;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Server {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub os: String,
    pub username: String,
    pub pass: String,
}

pub struct Servers {
    pub servers: StorageMulti<Server>,
}

impl StorageMultiType<Server, String> for Servers {
    fn new() -> Self {
        Servers {
            servers: StorageMulti::new("servers.json".to_string()),
        }
    }

    fn clone(&self) -> Servers {
        Servers {
            servers: self.servers.clone(),
        }
    }

    fn load(&mut self) -> Result<(), Box<dyn Error>> {
        self.servers.load()
    }

    fn save(&self) -> Result<(), Box<dyn Error>> {
        self.servers.save()
    }

    fn find(&self, server_id: String) -> Option<Server> {
        self.servers.find(&mut |s| s.name == server_id)
    }

    fn remove(&mut self, server_id: String) -> Result<(), Box<dyn Error>> {
        self.servers.remove(&mut |s| s.name == server_id)
    }

    fn add(&mut self, server: Server) -> Result<(), Box<dyn Error>> {
        let mut hasher = Sha256::new();
        hasher.update(server.pass);
        let hash = hasher.finalize();
        let hex = hash.iter().map(|b| format!("{:x}", b)).collect::<String>();
        self.servers.add(Server {
            name: server.name,
            address: server.address,
            port: server.port,
            os: server.os,
            username: server.username,
            pass: hex,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppStateType {
    pub passed_setup: bool,
}

pub struct AppState {
    state: StorageSingleton<AppStateType>,
}

impl StorageSingletonType<AppStateType, String> for AppState {
    fn new() -> Self {
        AppState {
            state: StorageSingleton::new("state.json".to_string()),
        }
    }

    fn get(&self) -> Arc<Mutex<Option<AppStateType>>> {
        self.state.get()
    }

    fn clone(&self) -> AppState {
        AppState {
            state: self.state.clone(),
        }
    }

    fn load(&mut self) -> Result<(), Box<dyn Error>> {
        self.state.load()
    }

    fn save(&self) -> Result<(), Box<dyn Error>> {
        self.state.save()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    pub username: String,
    pub pass: String,
}

pub struct Users {
    pub users: StorageMulti<User>,
}

impl Users {
    pub fn find_user_by_credentials(&self, username: &String, pass: &String) -> Option<User> {
        self.users
            .find(&mut |u| u.username == *username && u.pass == *pass)
    }
}

impl StorageMultiType<User, String> for Users {
    fn new() -> Self {
        Users {
            users: StorageMulti::new("users.json".to_string()),
        }
    }

    fn clone(&self) -> Users {
        Users {
            users: self.users.clone(),
        }
    }

    fn load(&mut self) -> Result<(), Box<dyn Error>> {
        self.users.load()
    }

    fn save(&self) -> Result<(), Box<dyn Error>> {
        self.users.save()
    }

    fn find(&self, _username: String) -> Option<User> {
        todo!()
    }

    fn remove(&mut self, username: String) -> Result<(), Box<dyn Error>> {
        self.users.remove(&mut |u| u.username == username)
    }

    fn add(&mut self, user: User) -> Result<(), Box<dyn Error>> {
        let mut hasher = Sha256::new();
        hasher.update(user.pass);
        let hash = hasher.finalize();
        let hex = hash.iter().map(|b| format!("{:x}", b)).collect::<String>();
        self.users.add(User {
            username: user.username,
            pass: hex,
        })
    }
}
