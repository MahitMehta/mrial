use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{BufReader, Write},
    sync::{Arc, Mutex},
};

use log::debug;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub struct StorageMulti<T> {
    state: Arc<Mutex<Option<Vec<T>>>>,
    db_path: String,
    file_name: String,
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
        if !file_name.ends_with(".json") {
            panic!("File Name Must End with .json");
        }

        StorageMulti {
            state: Arc::new(Mutex::new(None)),
            db_path: DB_PATH.to_string(),
            file_name,
        }
    }

    pub fn clone(&self) -> StorageMulti<T> {
        StorageMulti {
            state: self.state.clone(),
            db_path: self.db_path.clone(),
            file_name: self.file_name.clone(),
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
        let os_data_dir = dirs::data_dir().unwrap();
        let path = os_data_dir.join(&self.db_path).join(&self.file_name);
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

    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        let os_data_dir = dirs::data_dir().unwrap();
        let data_dir = os_data_dir.join(&self.db_path);
        let file_path = data_dir.join(&self.file_name);

        fs::create_dir_all(&data_dir)?;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file_path)?;

        let value: StorageMultiWrapper<T> = StorageMultiWrapper {
            data: self.state.lock().unwrap().clone().unwrap(),
        };

        let json = serde_json::to_string(&value)?;
        file.write_all(json.as_bytes())?;
        debug!("Saved Data to Disk @ {:?}", file_path);

        Ok(())
    }
}
