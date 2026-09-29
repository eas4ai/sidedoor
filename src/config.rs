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
    /// Each plugin's settings by plugin id, as changed in Settings › Plugins.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugin_settings: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
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
    BTreeMap::from([(
        "plugin:builtin.clipboard".into(),
        if cfg!(windows) {
            "ctrl-alt-v"
        } else {
            "ctrl-cmd-v"
        }
        .into(),
    )])
}

/// Apps offered in a fresh dock, in order; only installed ones are kept.
#[cfg(not(target_os = "windows"))]
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

#[cfg(target_os = "windows")]
const DEFAULT_APPS: &[&str] = &[
    "explorer.exe",
    "msedge.exe",
    "chrome.exe",
    "wt.exe",
    "notepad.exe",
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
            ItemConfig::Plugin {
                id: crate::builtins::WEATHER.into(),
            },
            ItemConfig::Plugin {
                id: crate::builtins::CLIPBOARD.into(),
            },
            ItemConfig::Plugin {
                id: crate::builtins::STATS.into(),
            },
        ]);
        Self {
            items,
            edge: Edge::default(),
            appearance: Appearance::default(),
            weather: WeatherLocation::default(),
            shortcuts: default_shortcuts(),
            plugin_settings: BTreeMap::new(),
        }
    }

    /// Upgrades old widget entries and shortcuts without changing their order,
    /// location or user-assigned keys. Explicit new shortcut keys win.
    pub fn migrate_builtins(&mut self) -> bool {
        let before = self.clone();
        for item in &mut self.items {
            let id = match item {
                ItemConfig::Weather => crate::builtins::WEATHER,
                ItemConfig::Clipboard => crate::builtins::CLIPBOARD,
                ItemConfig::Stats => crate::builtins::STATS,
                _ => continue,
            };
            *item = ItemConfig::Plugin { id: id.into() };
        }
        for old in ["weather", "clipboard", "stats"] {
            if let Some(shortcut) = self.shortcuts.remove(old) {
                let id = crate::builtins::legacy_id(old).unwrap();
                self.shortcuts
                    .entry(format!("plugin:{id}"))
                    .or_insert(shortcut);
            }
        }
        *self != before
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
                if config.migrate_builtins() {
                    config.save()?;
                }
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
    fn builtin_migration_preserves_order_location_and_shortcut_choices() {
        let mut config: Config = serde_json::from_value(serde_json::json!({
            "items": [{"type":"stats"}, {"type":"plugin","id":"weather"}, {"type":"clipboard"}, {"type":"weather"}],
            "edge":"left", "appearance":"dark",
            "weather":{"name":"Odense","latitude":55.4,"longitude":10.4},
            "shortcuts":{"stats":"ctrl-cmd-s","clipboard":"alt-v","plugin:builtin.clipboard":"ctrl-v"}
        })).unwrap();
        assert!(config.migrate_builtins());
        assert_eq!(
            config.items,
            [
                ItemConfig::Plugin {
                    id: crate::builtins::STATS.into()
                },
                ItemConfig::Plugin {
                    id: "weather".into()
                },
                ItemConfig::Plugin {
                    id: crate::builtins::CLIPBOARD.into()
                },
                ItemConfig::Plugin {
                    id: crate::builtins::WEATHER.into()
                },
            ]
        );
        assert_eq!(
            config.shortcuts,
            BTreeMap::from([
                ("plugin:builtin.stats".into(), "ctrl-cmd-s".into()),
                ("plugin:builtin.clipboard".into(), "ctrl-v".into()),
            ])
        );
        assert_eq!(config.weather.name, "Odense");
        assert_eq!(config.edge, Edge::Left);
        assert_eq!(config.appearance, Appearance::Dark);
        assert!(!config.migrate_builtins());
        let mut disabled: Config =
            serde_json::from_str(r#"{"items":[{"type":"clipboard"}],"shortcuts":{}}"#).unwrap();
        disabled.migrate_builtins();
        assert!(disabled.shortcuts.is_empty());
    }

    #[test]
    fn starter_keeps_installed_apps_then_widgets() {
        let selected = [DEFAULT_APPS[0], DEFAULT_APPS[DEFAULT_APPS.len() - 1]];
        let config = Config::starter(|id| selected.contains(&id));
        assert_eq!(
            config.items,
            vec![
                ItemConfig::App {
                    bundle_id: selected[0].into()
                },
                ItemConfig::App {
                    bundle_id: selected[1].into()
                },
                ItemConfig::Plugin {
                    id: crate::builtins::WEATHER.into()
                },
                ItemConfig::Plugin {
                    id: crate::builtins::CLIPBOARD.into()
                },
                ItemConfig::Plugin {
                    id: crate::builtins::STATS.into()
                },
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
