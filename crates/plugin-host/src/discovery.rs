use crate::Manifest;
use std::{
    fs,
    path::{Path, PathBuf},
};
pub fn plugins_dir() -> PathBuf {
    platform::support_dir().join("plugins")
}

/// Every plugin in `dir`, by name.
pub fn discover(dir: &Path) -> Vec<Manifest> {
    let mut found: Vec<Manifest> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name != "node_modules" && !name.starts_with("builtin.")
        })
        .filter_map(|entry| Manifest::read(&entry.path()))
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}
