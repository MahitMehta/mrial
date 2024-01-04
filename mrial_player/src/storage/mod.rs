use std::{fs::{File, OpenOptions}, path::Path, error::Error, io::{BufReader, Write}};
use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Server {
    pub name: String,
    pub address: String,
    pub port: u16,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ServerState {
    servers: Vec<Server>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct StorageWrapper<T> {
    data: T
}

pub trait Storage<T: serde::de::DeserializeOwned> {
    fn load(&mut self) -> Result<(), Box<dyn Error>>;
    fn save(&self) -> Result<(), Box<dyn Error>>;
}

pub struct Servers {
    state: Option<ServerState>,
    db_path: String
}

impl Servers {
    pub fn new() -> Self {
        Servers {
            state: None,
            db_path: "./db/servers.json".to_string()
        }
    }

    pub fn get_servers(&self) -> Option<Vec<Server>> {
        if let Some(state) = &self.state {
            return Some(state.servers.clone());
        }
       
        None
    }

    pub fn find_server(&self, server_id: String) -> Option<Server> {
        if let Some(state) = &self.state {
            for server in &state.servers {
                if server.name == server_id {
                    return Some(server.clone());
                }
            }
        }

        None
    }

    pub fn try_clone(&self) -> Servers {
        Servers {
            state: self.state.clone(),
            db_path: self.db_path.clone()
        }
    }

    pub fn add(&mut self, name: String, address: String, port: u16) {
        if let Some(state) = &mut self.state {
            // TODO: display duplicate server error in slint
            for server in &state.servers {
                if server.name == name {
                    return; 
                }
            }

            state.servers.push(Server {
                name,
                address,
                port,
            });
        }
    }
}

impl Storage<ServerState> for Servers {
    fn load(&mut self) -> Result<(), Box<dyn Error>> {
        let path = Path::new(&self.db_path);
        let file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                self.state = Some(ServerState {
                    servers: Vec::new()
                });
                return Ok(())
            }
        };

        let reader = BufReader::new(file);
    
        let state: StorageWrapper<ServerState> = serde_json::from_reader(reader)?;
        self.state = Some(state.data);

        Ok(())
    }

    fn save(&self) -> Result<(), Box<dyn Error>> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.db_path)
            .unwrap();

        let value = StorageWrapper { 
            data: self.state.clone().unwrap() 
        };

        let json = serde_json::to_string(&value)?;
        file.write_all(json.as_bytes())?;

        Ok(())
    }
}