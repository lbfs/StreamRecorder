use std::fs::File;
use std::io::prelude::*;
use serde::{Deserialize, Serialize};
use std::{path};

#[derive(Serialize, Deserialize)]
pub struct Configuration {
    pub client_id: String,
    pub client_secret: String,
    pub login_names: Vec<String>,
    pub recording_path: path::PathBuf,
    pub cleanup_path: path::PathBuf,
    pub move_path: path::PathBuf,
    pub halt_until_next_live: bool,
    pub halt_newly_added: bool,
}

impl Configuration {
    pub fn new(filename: &str) -> Result<Configuration, std::io::Error> {
        let mut file = File::open(&filename)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config: Configuration = serde_json::from_str(&contents)?;
        Ok(config)
    }
}

