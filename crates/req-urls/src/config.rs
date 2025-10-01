use std::{fs::OpenOptions, io::Read, path::PathBuf};

use anyerr::AnyError;
use serde::{Deserialize, Serialize};

use crate::AnyResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub urls: Vec<String>,
    pub con: usize,
    pub timeout: u64,
}

impl Config {
    pub fn load(file: impl Into<PathBuf>) -> AnyResult<Config> {
        let mut file = OpenOptions::new()
            .read(true)
            .open(file.into())
            .map_err(|e| AnyError::wrap(e))?;
        let mut s = String::new();
        file.read_to_string(&mut s).map_err(|e| AnyError::wrap(e))?;
        let c: Config = serde_json::from_str(&s).map_err(|e| AnyError::wrap(e))?;
        Ok(c)
    }
}
