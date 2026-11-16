use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub game_folder: Option<PathBuf>,
    pub target_folder: Option<PathBuf>,
}

fn config_path() -> io::Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "WoWs Monitor", "Companion")
        .ok_or_else(|| io::Error::other("Could not locate the user config directory."))?;
    Ok(dirs.config_dir().join("settings.json"))
}

pub fn load() -> (AppConfig, Option<String>) {
    let result = config_path().and_then(|path| match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(error) => Err(error),
    });

    match result {
        Ok(config) => (config, None),
        Err(error) => (
            AppConfig::default(),
            Some(format!("Could not read settings: {error}")),
        ),
    }
}

pub fn save(config: &AppConfig) -> io::Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(path, bytes)
}
