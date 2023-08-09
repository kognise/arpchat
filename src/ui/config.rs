use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::{fs, str, sync::Mutex};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::net::EtherType;

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub username: Option<String>,
    pub interface: Option<String>,
    pub ether_type: Option<EtherType>,
}

impl Config {
    pub fn load() -> Self {
        let data: Option<Vec<u8>> = try { fs::read(Self::get_config_path()?).ok()? };
        let data = data.unwrap_or_default();
        let data: &str = str::from_utf8(&data).unwrap_or_default();
        toml::from_str(data).unwrap_or_default()
    }

    pub fn save(&self) {
        let _: Option<()> = try {
            let data = toml::to_string(&self).ok()?;
            let path = Self::get_config_path()?;
            fs::create_dir_all(path.parent()?).ok()?;
            fs::write(path, data).ok()?;
        };
    }

    pub fn get_config_path() -> Option<PathBuf> {
        let dirs = ProjectDirs::from("dev", "kognise", "arpchat")?;
        Some(dirs.config_dir().join("arpchat.toml"))
    }
}

pub static CONFIG: Lazy<Mutex<Config>> = Lazy::new(|| Mutex::new(Config::load()));
