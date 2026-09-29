//! Plugins shipped with the SDK. Their sources and runtime live in the app
//! bundle; starting them never writes into that (signed, possibly read-only) bundle.

use crate::plugin::{DataSource, Manifest};

pub const WEATHER: &str = "builtin.weather";
pub const CLIPBOARD: &str = "builtin.clipboard";
pub const STATS: &str = "builtin.stats";

pub fn legacy_id(id: &str) -> Option<&'static str> {
    match id {
        "weather" => Some(WEATHER),
        "clipboard" => Some(CLIPBOARD),
        "stats" => Some(STATS),
        _ => None,
    }
}

pub fn manifests() -> Vec<Manifest> {
    [
        (
            WEATHER,
            "Weather",
            "cloud",
            Some(190.0),
            DataSource::Weather,
        ),
        (
            CLIPBOARD,
            "Clipboard",
            "clipboard",
            None,
            DataSource::Clipboard,
        ),
        (STATS, "Stats", "cpu", Some(206.0), DataSource::Stats),
    ]
    .into_iter()
    .map(|(id, name, icon, height, source)| Manifest {
        id: id.into(),
        name: name.into(),
        icon: icon.into(),
        width: 300.0,
        height,
        settings: Vec::new(),
        clickable: id == CLIPBOARD,
        actions: Vec::new(),
        windows: Vec::new(),
        data: vec![source],
        dir: directory().join(id.trim_start_matches("builtin.")),
        main: if bundled_directory().is_some() {
            "index.js"
        } else {
            "index.tsx"
        }
        .into(),
    })
    .collect()
}

pub fn contains(id: &str) -> bool {
    matches!(id, WEATHER | CLIPBOARD | STATS)
}

fn bundled_directory() -> Option<std::path::PathBuf> {
    crate::platform::bundled_resource("builtins")
}

fn directory() -> std::path::PathBuf {
    bundled_directory().unwrap_or_else(|| {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/builtins")
    })
}
