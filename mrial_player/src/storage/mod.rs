use log::debug;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{BufReader, Write},
    sync::{Arc, Mutex},
};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Server {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub os: String,
    pub username: String,
    pub pass: String
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ServerState {
    servers: Vec<Server>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct StorageWrapper<T> {
    data: T,
}

pub trait Storage<T: serde::de::DeserializeOwned> {
    fn load(&mut self) -> Result<(), Box<dyn Error>>;
    fn save(&self) -> Result<(), Box<dyn Error>>;
}

pub struct Servers {
    state: Arc<Mutex<Option<ServerState>>>,
    db_path: String,
    file_name: String,
}

impl Servers {
    pub fn new() -> Self {
        Servers {
            state: Arc::new(Mutex::new(None)),
            db_path: "Mrial/db".to_string(),
            file_name: "servers.json".to_string(),
        }
    }

    pub fn get_servers(&self) -> Option<Vec<Server>> {
        if let Some(state) = self.state.lock().unwrap().as_ref() {
            return Some(state.servers.clone());
        }

        None
    }

    pub fn delete(&mut self, server_id: String) {
        if let Some(state) = self.state.lock().unwrap().as_mut() {
            for (index, server) in state.servers.iter().enumerate() {
                if server.name == server_id {
                    state.servers.remove(index);
                    break;
                }
            }
        }
    }

    pub fn find_server(&self, server_id: String) -> Option<Server> {
        if let Some(state) = self.state.lock().unwrap().as_ref() {
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
            db_path: self.db_path.clone(),
            file_name: self.file_name.clone(),
        }
    }

    pub fn add(&mut self, name: String, address: String, port: u16, os: String, username: String, pass: String) {
        if let Some(state) = self.state.lock().unwrap().as_mut() {
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
                os,
                username,
                pass
            });
        }
    }
}

impl Storage<ServerState> for Servers {
    fn load(&mut self) -> Result<(), Box<dyn Error>> {
        let os_data_dir = dirs::data_dir().unwrap();
        let path = os_data_dir.join(&self.db_path).join(&self.file_name);
        let file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                *self.state.lock().unwrap() = Some(ServerState {
                    servers: Vec::new(),
                });
                return Ok(());
            }
        };

        let reader = BufReader::new(file);

        if let Ok(wrapped_state) = 
            serde_json::from_reader::<BufReader<File>, StorageWrapper<ServerState>>(reader) {
            *self.state.lock().unwrap() = Some(wrapped_state.data);
            return Ok(());
        }

        *self.state.lock().unwrap() = Some(ServerState {
            servers: Vec::new(),
        });
        Err("Failed to Load Servers".into())
    }

    fn save(&self) -> Result<(), Box<dyn Error>> {
        let os_data_dir = dirs::data_dir().unwrap();
        let data_dir = os_data_dir.join(&self.db_path);

        fs::create_dir_all(&data_dir)?;
       
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(data_dir.join(&self.file_name))?;

        let value = StorageWrapper {
            data: self.state.lock().unwrap().clone().unwrap(),
        };

        let json = serde_json::to_string(&value)?;
        file.write_all(json.as_bytes())?;
        debug!("Saved Data to Disk @ {:?}", data_dir);

        Ok(())
    }
}
