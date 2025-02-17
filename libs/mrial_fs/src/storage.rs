use std::{
    error::Error, fs::{self, File, OpenOptions}, io::{BufReader, ErrorKind, Write}, path::PathBuf, process::Command, sync::{Arc, Mutex}
};

use log::debug;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub struct StorageMulti<T> {
    state: Arc<Mutex<Option<Vec<T>>>>,
    file_name: String,
    file_dir: PathBuf
}

#[derive(Serialize, Deserialize)]
pub struct StorageMultiWrapper<T> {
    data: Vec<T>,
}

pub trait StorageMultiType<T: Serialize + DeserializeOwned + Clone, K> {
    fn new() -> Self;
    fn load(&mut self) -> Result<(), Box<dyn Error>>;
    fn save(&self) -> Result<(), Box<dyn Error>>;
    fn clone(&self) -> Self;
    fn remove(&mut self, key: K) -> Result<(), Box<dyn Error>>;
    fn add(&mut self, item: T) -> Result<(), Box<dyn Error>>;
    fn find(&self, key: K) -> Option<T>;
}

const DB_PATH: &'static str = "Mrial/db";

impl<T: Serialize + DeserializeOwned + Clone> StorageMulti<T> {
    pub fn new(file_name: String) -> Self {
        let os_data_dir = dirs::data_dir().unwrap();
        let file_dir = os_data_dir.join(DB_PATH);
        StorageMulti::new_with_custom_dir(file_name, file_dir)
    }

    pub fn new_with_custom_dir(file_name: String, file_dir: PathBuf) -> Self {
        if !file_name.ends_with(".json") {
            panic!("File Name Must End with .json");
        }

        StorageMulti {
            state: Arc::new(Mutex::new(None)),
            file_name,
            file_dir
        }
    }

    pub fn clone(&self) -> StorageMulti<T> {
        StorageMulti {
            state: self.state.clone(),
            file_name: self.file_name.clone(),
            file_dir: self.file_dir.clone(),
        }
    }

    pub fn get(&self) -> Option<Vec<T>> {
        self.state.lock().unwrap().clone()
    }

    pub fn remove(&self, func: &mut dyn FnMut(&T) -> bool) -> Result<(), Box<dyn Error>> {
        if let Ok(mut state) = self.state.lock() {
            if let Some(state) = state.as_mut() {
                let index = state.iter().position(|item| func(item));
                if let Some(index) = index {
                    state.remove(index);
                    return Ok(());
                }
            }
        }

        Err("Failed to Remove Item".into())
    }

    pub fn find(&self, func: &dyn Fn(&T) -> bool) -> Option<T> {
        if let Ok(state) = self.state.lock() {
            if let Some(state) = state.as_ref() {
                for item in state.iter() {
                    if func(item) {
                        return Some(item.clone());
                    }
                }
            }
        }

        None
    }

    pub fn add(&self, item: T) -> Result<(), Box<dyn Error>> {
        if let Ok(mut state) = self.state.lock() {
            if let Some(state) = state.as_mut() {
                state.push(item);
                return Ok(());
            }
        }

        Err("Failed to Add Item".into())
    }

    pub fn load(&mut self) -> Result<(), Box<dyn Error>> {
        let path = self.file_dir.join(&self.file_name);

        debug!("Loading Data from Disk @ {:?}", path);
        let file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                *self.state.lock().unwrap() = Some(Vec::new());
                return Ok(());
            }
        };

        let reader = BufReader::new(file);

        if let Ok(wrapped_state) =
            serde_json::from_reader::<BufReader<File>, StorageMultiWrapper<T>>(reader)
        {
            *self.state.lock().unwrap() = Some(wrapped_state.data);
            return Ok(());
        }

        *self.state.lock().unwrap() = Some(Vec::new());
        Err("Failed to Load Data".into())
    }

    fn save_with_elevated_permissions(&self) -> Result<(), Box<dyn Error>> {
        let file_path = self.file_dir.join(&self.file_name);

        let status = Command::new("pkexec")
            .arg("tee") // Use tee to write to a root-protected file
            .arg(&file_path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                let value: StorageMultiWrapper<T> = StorageMultiWrapper {
                    data: self.state.lock().unwrap().clone().unwrap(),
                };
                let json = serde_json::to_string(&value)?;

                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(json.as_bytes())?;
                }
                child.wait()
            })?;

        if status.success() {
            debug!("Saved Data to Disk with Elevated Permissions @ {:?}", file_path);
            Ok(())
        } else {
            Err("Failed to Save Data to Disk".into())
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        let file_path = self.file_dir.join(&self.file_name);

        fs::create_dir_all(&self.file_dir)?;

        let mut file = match OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file_path) {
                Ok(file) => file,
                Err(ref e) if e.kind() == ErrorKind::PermissionDenied => {
                    return self.save_with_elevated_permissions();
                }
                Err(e) => {
                    return Err(e.into());
                }
            }; 
      
        let value: StorageMultiWrapper<T> = StorageMultiWrapper {
            data: self.state.lock().unwrap().clone().unwrap(),
        };

        let json = serde_json::to_string(&value)?;
        file.write_all(json.as_bytes())?;
        debug!("Saved Data to Disk @ {:?}", file_path);
        
        Ok(())
    }
}
