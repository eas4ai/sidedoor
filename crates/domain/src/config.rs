//! The user's dock layout, stored as JSON in Application Support.

use crate::geometry::Edge;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Sidekick caps the dock at twelve apps, links and widgets.
pub const MAX_ITEMS: usize = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub items: Vec<ItemConfig>,
    #[serde(default)]
    pub edge: Edge,
    #[serde(default)]
    pub appearance: Appearance,
    /// Where the weather was for, before Weather became a plugin. Read once
    /// to carry it over to the plugin's City setting.
    #[serde(default, skip_serializing)]
    pub weather: Option<LegacyLocation>,
    /// Global shortcuts by item id ("app:com.apple.Safari", "plugin:clipboard", …),
    /// written like "ctrl-cmd-v". Missing means the defaults; an empty map
    /// means none.
    #[serde(default = "default_shortcuts")]
    pub shortcuts: BTreeMap<String, String>,
    /// Each plugin's settings by plugin id, as changed in Settings › Plugins.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugin_settings: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    /// Plugins the user agreed to run, by id. Built-ins are always trusted.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub trusted_plugins: BTreeSet<String>,
    #[serde(default = "default_update_checks")]
    pub automatically_check_for_updates: bool,
}

fn default_update_checks() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemConfig {
    App {
        bundle_id: String,
    },
    /// Widgets from before Weather, Stats and Clipboard were plugins; read
    /// only to carry them over to the plugins.
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

/// A place the weather was for, in configs from before Weather was a plugin.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct LegacyLocation {
    pub name: String,
}

/// The official plugins that used to be part of the app, by the old names
/// their dock items and shortcuts went by.
const OFFICIAL: [(&str, &str); 3] = [
    ("weather", "builtin.weather"),
    ("clipboard", "builtin.clipboard"),
    ("stats", "builtin.stats"),
];

/// Clipboard History from anywhere, once the Clipboard plugin is installed;
/// ⌃⌘V (Ctrl+Alt+V on a PC) is rarely taken.
fn default_shortcuts() -> BTreeMap<String, String> {
    BTreeMap::from([(
        "plugin:clipboard".into(),
        if crate::shortcut::PC_KEYS {
            "ctrl-alt-v"
        } else {
            "ctrl-cmd-v"
        }
        .into(),
    )])
}

/// Apps offered in a fresh dock, in order; only installed ones are kept.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
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

/// Desktop file IDs: file managers, browsers, terminals and editors.
#[cfg(target_os = "linux")]
const DEFAULT_APPS: &[&str] = &[
    "org.gnome.Nautilus",
    "org.kde.dolphin",
    "thunar",
    "firefox",
    "org.mozilla.firefox",
    "firefox_firefox",
    "google-chrome",
    "chromium",
    "brave-browser",
    "com.mitchellh.ghostty",
    "org.gnome.Ptyxis",
    "org.gnome.Console",
    "org.gnome.Terminal",
    "org.kde.konsole",
    "code",
    "org.gnome.TextEditor",
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
    /// A starter dock: up to four installed apps. Widgets come from the
    /// plugin gallery.
    pub fn starter(is_installed: impl Fn(&str) -> bool) -> Self {
        let items: Vec<ItemConfig> = DEFAULT_APPS
            .iter()
            .filter(|id| is_installed(id))
            .take(4)
            .map(|id| ItemConfig::App {
                bundle_id: (*id).to_string(),
            })
            .collect();
        Self {
            items,
            edge: Edge::default(),
            appearance: Appearance::default(),
            weather: None,
            shortcuts: default_shortcuts(),
            plugin_settings: BTreeMap::new(),
            trusted_plugins: BTreeSet::new(),
            automatically_check_for_updates: true,
        }
    }

    /// Points the widgets that used to be part of the app at the official
    /// plugins that replaced them, keeping their order and shortcuts, and
    /// hands the weather's place to the Weather plugin. The dock shows each
    /// once its plugin is installed.
    pub fn migrate_official(&mut self) -> bool {
        let before = self.clone();
        for item in &mut self.items {
            let id = match item {
                ItemConfig::Weather => "weather",
                ItemConfig::Clipboard => "clipboard",
                ItemConfig::Stats => "stats",
                ItemConfig::Plugin { id } => match OFFICIAL.iter().find(|(_, old)| old == id) {
                    Some((new, _)) => new,
                    None => continue,
                },
                ItemConfig::App { .. } => continue,
            };
            *item = ItemConfig::Plugin { id: id.into() };
        }
        for (id, old) in OFFICIAL {
            for old_key in [id.to_string(), format!("plugin:{old}")] {
                if let Some(shortcut) = self.shortcuts.remove(&old_key) {
                    self.shortcuts
                        .entry(format!("plugin:{id}"))
                        .or_insert(shortcut);
                }
            }
            if let Some(settings) = self.plugin_settings.remove(old) {
                self.plugin_settings.entry(id.into()).or_insert(settings);
            }
        }
        if let Some(place) = self.weather.take() {
            self.plugin_settings
                .entry("weather".into())
                .or_default()
                .entry("city")
                .or_insert(place.name.into());
        }
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_widgets_become_the_official_plugins() {
        let mut config: Config = serde_json::from_value(serde_json::json!({
            "items": [{"type":"stats"}, {"type":"plugin","id":"builtin.weather"}, {"type":"clipboard"}, {"type":"app","bundle_id":"a"}],
            "edge":"left", "appearance":"dark",
            "weather":{"name":"Odense","latitude":55.4,"longitude":10.4},
            "shortcuts":{"stats":"ctrl-cmd-s","clipboard":"alt-v","plugin:builtin.clipboard":"ctrl-v"}
        })).unwrap();
        assert!(config.migrate_official());
        let plugin = |id: &str| ItemConfig::Plugin { id: id.into() };
        assert_eq!(
            config.items,
            [
                plugin("stats"),
                plugin("weather"),
                plugin("clipboard"),
                ItemConfig::App {
                    bundle_id: "a".into()
                },
            ]
        );
        assert_eq!(
            config.shortcuts,
            BTreeMap::from([
                ("plugin:stats".into(), "ctrl-cmd-s".into()),
                ("plugin:clipboard".into(), "alt-v".into()),
            ])
        );
        assert_eq!(config.plugin_settings["weather"]["city"], "Odense");
        assert_eq!(config.edge, Edge::Left);
        assert_eq!(config.appearance, Appearance::Dark);
        assert!(!config.migrate_official());
        // The place isn't written back; the plugin has it now.
        let saved = serde_json::to_value(&config).unwrap();
        assert!(saved.get("weather").is_none());

        let mut disabled: Config =
            serde_json::from_str(r#"{"items":[{"type":"clipboard"}],"shortcuts":{}}"#).unwrap();
        disabled.migrate_official();
        assert!(disabled.shortcuts.is_empty());
    }

    #[test]
    fn starter_keeps_installed_apps() {
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
        assert_eq!(config.weather, None);
        assert_eq!(config.shortcuts, default_shortcuts());
        assert!(config.automatically_check_for_updates);
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
