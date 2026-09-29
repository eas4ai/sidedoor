use crate::{install::Source, protocol::DataSource};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
/// A plugin found on disk: a folder whose `index.tsx` calls `definePlugin`.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// The folder's name; stored in the dock config as `plugin:<id>`.
    pub id: String,
    pub name: String,
    /// Lucide icon name, e.g. `"timer"`.
    pub icon: String,
    pub width: f64,
    /// A fixed card height, or `None` to fit the content.
    pub height: Option<f64>,
    pub settings: Vec<SettingSpec>,
    /// Whether clicking the dock tile runs the plugin's `onClick`.
    pub clickable: bool,
    /// Commands for the item's context menu.
    pub actions: Vec<PluginAction>,
    /// Windows the plugin can open, by key.
    pub windows: Vec<PluginWindow>,
    /// Native live data explicitly requested by this plugin.
    pub data: Vec<DataSource>,
    pub dir: PathBuf,
    /// The entry file, relative to `dir`.
    pub main: PathBuf,
    /// Where it was installed from, for plugins installed from GitHub.
    pub source: Option<Source>,
}

/// One setting a plugin declares, shown in Settings › Plugins.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SettingSpec {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: SettingKind,
    /// The choices of a `choice` setting.
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub default: Option<Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    #[default]
    Text,
    /// Text that is hidden while typed, like an API key.
    Secret,
    Toggle,
    Choice,
    Number,
}

impl SettingSpec {
    /// The value before the user changes it.
    pub fn default_value(&self) -> Value {
        if let Some(value) = &self.default {
            return value.clone();
        }
        match self.kind {
            SettingKind::Text | SettingKind::Secret => Value::from(""),
            SettingKind::Toggle => Value::from(false),
            SettingKind::Number => Value::from(0),
            SettingKind::Choice => self
                .options
                .first()
                .cloned()
                .map_or(Value::Null, Value::from),
        }
    }
}

/// A command a plugin adds to its dock item's context menu.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PluginAction {
    pub key: String,
    pub title: String,
}

/// A window a plugin declares, opened with `sidedoor.openWindow(key)`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PluginWindow {
    pub key: String,
    pub title: String,
    #[serde(default = "default_window_width")]
    pub width: f64,
    #[serde(default = "default_window_height")]
    pub height: f64,
}

fn default_window_width() -> f64 {
    480.0
}
fn default_window_height() -> f64 {
    360.0
}

/// What a plugin tells the host about itself when it starts, from its
/// `definePlugin` call.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Described {
    pub name: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default = "default_width")]
    pub width: f64,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub settings: Vec<SettingSpec>,
    #[serde(default)]
    pub clickable: bool,
    #[serde(default)]
    pub actions: Vec<PluginAction>,
    #[serde(default)]
    pub windows: Vec<PluginWindow>,
    #[serde(default)]
    pub data: Vec<DataSource>,
}

fn default_icon() -> String {
    "puzzle".into()
}
fn default_width() -> f64 {
    280.0
}

/// Files a plugin can start from, in the order they're looked for.
const ENTRIES: &[&str] = &["index.tsx", "index.ts", "index.jsx", "index.js"];

impl Manifest {
    /// A plugin folder: one with an `index.tsx` (or `.ts`, `.jsx`, `.js`).
    ///
    /// Everything about a plugin lives in its `definePlugin` call, which the
    /// host only learns once the plugin runs. Until then, which may be never
    /// if the user doesn't trust it, the name and icon are read from the
    /// source as text, without running it.
    pub fn read(dir: &Path) -> Option<Self> {
        let main = ENTRIES
            .iter()
            .map(PathBuf::from)
            .find(|entry| dir.join(entry).is_file())?;
        let source = fs::read_to_string(dir.join(&main)).ok()?;
        if !calls_define_plugin(&source) {
            return None;
        }
        let id = dir.file_name()?.to_string_lossy().into_owned();
        Some(Self {
            name: peek(&source, "name").unwrap_or_else(|| id.clone()),
            icon: peek(&source, "icon").unwrap_or_else(default_icon),
            id,
            width: default_width(),
            height: None,
            settings: Vec::new(),
            clickable: false,
            actions: Vec::new(),
            windows: Vec::new(),
            data: Vec::new(),
            source: Source::read(dir),
            dir: dir.to_path_buf(),
            main,
        })
    }

    /// Takes on what the running plugin says about itself.
    pub fn update(&mut self, described: Described) {
        self.name = described.name;
        self.icon = described.icon;
        self.width = described.width.clamp(120.0, 480.0);
        self.height = described
            .height
            .map(|height| height.clamp(40.0, MAX_HEIGHT));
        self.data = described.data;
        self.settings = described.settings;
        self.clickable = described.clickable;
        self.actions = described.actions;
        self.windows = described
            .windows
            .into_iter()
            .map(|window| PluginWindow {
                width: window.width.clamp(280.0, 1200.0),
                height: window.height.clamp(200.0, 900.0),
                ..window
            })
            .collect();
    }

    /// The asset path of the plugin's icon.
    pub fn icon_path(&self) -> String {
        icon_path(&self.icon)
    }

    /// Every setting's value: what the user saved, else the default.
    pub fn settings_with(&self, saved: Option<&Map<String, Value>>) -> Map<String, Value> {
        let mut values: Map<String, Value> = self
            .settings
            .iter()
            .map(|spec| (spec.key.clone(), spec.default_value()))
            .collect();
        if let Some(saved) = saved {
            values.extend(saved.clone());
        }
        values
    }
}

/// Whether `source` calls `definePlugin(…)`, rather than only naming it the
/// way the SDK's own index re-exports it.
fn calls_define_plugin(source: &str) -> bool {
    source.match_indices("definePlugin").any(|(at, name)| {
        let before = source[..at].trim_end();
        let after = source[at + name.len()..].trim_start();
        !before.ends_with("function") && (after.starts_with('(') || after.starts_with('<'))
    })
}

/// The tallest card a plugin gets, fitted or fixed.
pub const MAX_HEIGHT: f64 = 600.0;

/// The first string given for `key` inside `definePlugin(…)`, as in
/// `name: "Pomodoro"`. Only for listing a plugin that hasn't run yet.
pub(crate) fn peek(source: &str, key: &str) -> Option<String> {
    let body = &source[source.find("definePlugin(")?..];
    let mut searched = 0;
    while let Some(found) = body[searched..].find(key) {
        let at = searched + found;
        searched = at + key.len();
        let whole_word = !body[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        let Some(value) = body[searched..].trim_start().strip_prefix(':') else {
            continue;
        };
        if !whole_word {
            continue;
        }
        let value = value.trim_start();
        let quote = value
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\'' | '`'))?;
        let inner = &value[1..];
        return Some(inner[..inner.find(quote)?].to_string());
    }
    None
}

/// Where Lucide icons live in the app's assets.
pub fn icon_path(name: &str) -> String {
    format!("icons/{name}.svg")
}
