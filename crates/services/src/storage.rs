//! Persistence and clipboard file comparisons, outside the domain model.
use domain::{
    clipboard::{ClipKind, History},
    config::{Config, MAX_ITEMS},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
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
            if config.migrate_builtins() {
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
pub fn history_path() -> PathBuf {
    platform::support_dir().join("clipboard.json")
}
pub fn image_dir() -> PathBuf {
    platform::support_dir().join("clipboard-images")
}
pub fn load_history(path: &Path) -> History {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
pub fn save_history(history: &History) -> io::Result<()> {
    let path = history_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec(history).map_err(io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)?;
    fs::rename(temp, path)
}
pub fn push_history(
    history: &mut History,
    kind: ClipKind,
    source: Option<String>,
    now: u64,
) -> Vec<PathBuf> {
    history.push_with_image_comparison(
        kind,
        source,
        now,
        |a, b| matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_same_image_copied_twice_is_one_entry() {
        let dir = std::env::temp_dir().join(format!("sidedoor-dup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (first, second, other) = (dir.join("1.png"), dir.join("2.png"), dir.join("3.png"));
        std::fs::write(&first, b"same pixels").unwrap();
        std::fs::write(&second, b"same pixels").unwrap();
        std::fs::write(&other, b"other pixels").unwrap();
        let image = |path: &PathBuf| ClipKind::Image {
            path: path.clone(),
            width: 4,
            height: 4,
        };

        let mut history = History::default();
        push_history(&mut history, image(&first), None, 1);
        let unused = push_history(&mut history, image(&second), None, 2);
        assert_eq!(history.len(), 1);
        assert_eq!(unused, vec![second.clone()]);
        push_history(&mut history, image(&other), None, 3);
        assert_eq!(history.len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }
}
