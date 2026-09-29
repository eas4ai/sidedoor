//! The user's dock layout, stored as JSON in Application Support.

use crate::geometry::Edge;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io, path::PathBuf};

/// Sidekick caps the dock at twelve apps, links and widgets.
pub const MAX_ITEMS: usize = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub items: Vec<ItemConfig>,
    #[serde(default)]
    pub edge: Edge,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub weather: WeatherLocation,
    /// Global shortcuts by item id ("app:com.apple.Safari", "clipboard", …),
    /// written like "ctrl-cmd-v". Missing means the defaults; an empty map
    /// means none.
    #[serde(default = "default_shortcuts")]
    pub shortcuts: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemConfig {
    App {
        bundle_id: String,
    },
    Weather,
    Stats,
    Clipboard,
    /// A plugin from the plugins folder, by folder name.
    Plugin {
        id: String,
    },
}

/// Light or dark, or whatever macOS is set to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherLocation {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Default for WeatherLocation {
    fn default() -> Self {
        Self {
            name: "Aalborg".into(),
            latitude: 57.048,
            longitude: 9.919,
        }
    }
}

/// Clipboard History from anywhere; ⌃⌘V is rarely taken.
fn default_shortcuts() -> BTreeMap<String, String> {
    BTreeMap::from([("clipboard".into(), "ctrl-cmd-v".into())])
}

/// Apps offered in a fresh dock, in order; only installed ones are kept.
const DEFAULT_APPS: &[&str] = &[
    "com.apple.finder",
    "com.apple.Safari",
    "com.brave.Browser",
    "com.google.Chrome",
    "com.mitchellh.ghostty",
    "com.apple.Terminal",
    "com.apple.Notes",
    "com.apple.Music",
];

impl Config {
    /// A starter dock: up to four installed apps, then the widgets.
    pub fn starter(is_installed: impl Fn(&str) -> bool) -> Self {
        let mut items: Vec<ItemConfig> = DEFAULT_APPS
            .iter()
            .filter(|id| is_installed(id))
            .take(4)
            .map(|id| ItemConfig::App {
                bundle_id: (*id).to_string(),
            })
            .collect();
        items.extend([
            ItemConfig::Weather,
            ItemConfig::Clipboard,
            ItemConfig::Stats,
        ]);
        Self {
            items,
            edge: Edge::default(),
            appearance: Appearance::default(),
            weather: WeatherLocation::default(),
            shortcuts: default_shortcuts(),
        }
    }

    pub fn path() -> PathBuf {
        crate::platform::support_dir().join("config.json")
    }

    /// Reads the saved config, or writes and returns a starter one.
    pub fn load_or_create(is_installed: impl Fn(&str) -> bool) -> io::Result<Self> {
        match fs::read(Self::path()) {
            Ok(bytes) => {
                let mut config: Self = serde_json::from_slice(&bytes)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                config.items.truncate(MAX_ITEMS);
                Ok(config)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let config = Self::starter(is_installed);
                config.save()?;
                Ok(config)
            }
            Err(err) => Err(err),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        // Write then rename, so a crash never leaves a half-written file.
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, json)?;
        fs::rename(temp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_keeps_installed_apps_then_widgets() {
        let config = Config::starter(|id| id == "com.apple.finder" || id == "com.apple.Music");
        assert_eq!(
            config.items,
            vec![
                ItemConfig::App {
                    bundle_id: "com.apple.finder".into()
                },
                ItemConfig::App {
                    bundle_id: "com.apple.Music".into()
                },
                ItemConfig::Weather,
                ItemConfig::Clipboard,
                ItemConfig::Stats,
            ]
        );
    }

    #[test]
    fn older_configs_get_defaults() {
        let json = r#"{"items":[{"type":"app","bundle_id":"com.apple.Safari"},{"type":"stats"}]}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.items.len(), 2);
        assert_eq!(config.edge, Edge::Right);
        assert_eq!(config.appearance, Appearance::System);
        assert_eq!(config.weather, WeatherLocation::default());
        assert_eq!(config.shortcuts, default_shortcuts());
        let none: Config = serde_json::from_str(r#"{"items":[],"shortcuts":{}}"#).unwrap();
        assert!(none.shortcuts.is_empty());
    }

    #[test]
    fn edge_is_lowercase_in_json() {
        let json = r#"{"items":[],"edge":"bottom","appearance":"dark"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.edge, Edge::Bottom);
        assert_eq!(config.appearance, Appearance::Dark);
    }
}
