//! Persistence, outside the domain model.
use domain::config::{Config, MAX_ITEMS};
use std::{fs, io, path::PathBuf};
pub fn config_path() -> PathBuf {
    platform::support_dir().join("config.json")
}

/// Reads the saved config, or writes and returns a starter one.
pub fn load_config(is_installed: impl Fn(&str) -> bool) -> io::Result<Config> {
    match fs::read(config_path()) {
        Ok(bytes) => {
            let mut config: Config = serde_json::from_slice(&bytes)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            config.items.truncate(MAX_ITEMS);
            if config.migrate_official() {
                save_config(&config)?;
            }
            Ok(config)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let config = Config::starter(is_installed);
            save_config(&config)?;
            Ok(config)
        }
        Err(err) => Err(err),
    }
}

pub fn save_config(config: &Config) -> io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(config).map_err(io::Error::other)?;
    // Write then rename, so a crash never leaves a half-written file.
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)?;
    fs::rename(temp, path)
}
