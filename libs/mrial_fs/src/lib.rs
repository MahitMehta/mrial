use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::error::Error;
use storage::{StorageMulti, StorageMultiType};

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

const ROOT_DATA_DIR: &'static str = "/var/lib/mrial_server";

impl StorageMultiType<User, String> for Users {
    #[cfg(not(target_os = "linux"))]
    fn new() -> Self {
        Users {
            users: StorageMulti::new( "users.json".to_string()),
        }
    }

    #[cfg(target_os = "linux")]
    fn new() -> Self {
        use std::path::PathBuf;
        let file_dir = PathBuf::from(ROOT_DATA_DIR);

        Users {
            users: StorageMulti::new_with_custom_dir(
                "users.json".to_string(), 
                file_dir
            ),
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

    fn find(&self, username: String) -> Option<User> {
        self.users.find(&|u| u.username == username)
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
