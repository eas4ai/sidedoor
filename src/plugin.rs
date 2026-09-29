//! Plugins: widgets written in TSX against the SDK in `sdk/`. One Bun
//! process, the SDK's `supervisor.ts`, runs every plugin in its own Worker
//! thread. A plugin sends each surface it renders once, then patches; the
//! host draws them (see `plugin_ui`) and sends back events, settings and
//! whether the card is open.

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    rc::Rc,
    sync::{Arc, Mutex},
};

/// A plugin found on disk: a folder whose `package.json` has a `sidekick`
/// section.
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
    pub dir: PathBuf,
    /// The entry file, relative to `dir`.
    pub main: PathBuf,
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
        if !source.contains("definePlugin") {
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
        self.settings = described.settings;
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

/// The tallest card a plugin gets, fitted or fixed.
pub const MAX_HEIGHT: f64 = 600.0;

/// The first string given for `key` inside `definePlugin(…)`, as in
/// `name: "Pomodoro"`. Only for listing a plugin that hasn't run yet.
fn peek(source: &str, key: &str) -> Option<String> {
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

pub fn plugins_dir() -> PathBuf {
    crate::platform::support_dir().join("plugins")
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
            !name.starts_with('.') && name != "node_modules"
        })
        .filter_map(|entry| Manifest::read(&entry.path()))
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

// MARK: Protocol

/// A rendered node: text, or an element with props and children.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Node {
    Text(String),
    Element {
        #[serde(rename = "t")]
        kind: String,
        #[serde(rename = "p", default)]
        props: Map<String, Value>,
        #[serde(rename = "c", default)]
        children: Vec<Node>,
    },
}

/// A change to a rendered surface. `path` indexes into children, starting
/// with the root.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Patch {
    Replace {
        path: Vec<usize>,
        node: Node,
    },
    Props {
        path: Vec<usize>,
        props: Map<String, Value>,
    },
}

fn node_at<'a>(tree: &'a mut [Node], path: &[usize]) -> Option<&'a mut Node> {
    let (first, rest) = path.split_first()?;
    let mut node = tree.get_mut(*first)?;
    for index in rest {
        match node {
            Node::Element { children, .. } => node = children.get_mut(*index)?,
            Node::Text(_) => return None,
        }
    }
    Some(node)
}

/// Applies `patches` in order. `false` if one doesn't fit the tree, which
/// means the host and the plugin disagree and the surface should be resent.
pub fn apply_patches(tree: &mut [Node], patches: Vec<Patch>) -> bool {
    for patch in patches {
        match patch {
            Patch::Replace { path, node } => match node_at(tree, &path) {
                Some(target) => *target = node,
                None => return false,
            },
            Patch::Props { path, props } => match node_at(tree, &path) {
                Some(Node::Element { props: target, .. }) => *target = props,
                _ => return false,
            },
        }
    }
    true
}

/// What a plugin sends, and what the supervisor says about it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginMessage {
    /// Sent first: what `definePlugin` declares.
    Manifest(Described),
    Render {
        surface: String,
        tree: Vec<Node>,
    },
    Patch {
        surface: String,
        patches: Vec<Patch>,
    },
    OpenUrl {
        url: String,
    },
    OpenPath {
        path: String,
    },
    Copy {
        text: String,
    },
    /// A `console.log` line.
    Log {
        line: String,
    },
    Error {
        message: String,
    },
    /// The plugin's worker stopped.
    Exited,
}

/// What the host sends a plugin.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    Event {
        handler: String,
        value: Value,
    },
    /// Whether the plugin's card is showing.
    Card {
        open: bool,
    },
    Settings {
        values: Map<String, Value>,
    },
    /// Send every surface whole again.
    Resync,
}

/// The host's side of a running plugin.
pub trait PluginLink {
    fn send(&mut self, message: &HostMessage);
}

/// A running plugin: messages to it, and what it sends back.
pub struct Connection {
    pub link: Box<dyn PluginLink>,
    pub incoming: UnboundedReceiver<PluginMessage>,
}

// MARK: Supervisor

type Routes = Arc<Mutex<HashMap<String, UnboundedSender<PluginMessage>>>>;

/// The Bun process that runs every plugin. Dropping it stops them all.
struct Supervisor {
    child: Child,
    stdin: ChildStdin,
    routes: Routes,
}

impl Supervisor {
    fn spawn(bun: &Path, sdk: &Path) -> io::Result<Self> {
        let mut child = Command::new(bun)
            .arg(sdk.join("src/supervisor.ts"))
            // Plugins compile JSX with the SDK's tsconfig, found from here.
            .current_dir(sdk)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("no stderr"))?;
        let routes: Routes = Arc::default();

        let routing = routes.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                route(&routing, &line);
            }
            // The supervisor is gone, and every plugin with it.
            let senders: Vec<_> = routing
                .lock()
                .map(|routes| routes.values().cloned().collect())
                .unwrap_or_default();
            for sender in senders {
                let _ = sender.unbounded_send(PluginMessage::Exited);
            }
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("[plugins] {line}");
            }
        });
        Ok(Self {
            child,
            stdin,
            routes,
        })
    }

    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn write(&mut self, plugin: &str, mut message: Value) {
        if let Value::Object(fields) = &mut message {
            fields.insert("plugin".into(), Value::from(plugin));
        }
        // A supervisor that has exited just misses the message; its plugins
        // hear `Exited` from the reader thread.
        let _ = writeln!(self.stdin, "{message}").and_then(|()| self.stdin.flush());
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Sends a line from the supervisor to the plugin it names.
fn route(routes: &Routes, line: &str) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        eprintln!("[plugins] {line}");
        return;
    };
    let Some(plugin) = value.get("plugin").and_then(Value::as_str) else {
        return;
    };
    let message = match serde_json::from_value::<PluginMessage>(value.clone()) {
        Ok(message) => message,
        Err(err) => PluginMessage::Log {
            line: format!("unreadable message ({err}): {line}"),
        },
    };
    let sender = routes
        .lock()
        .ok()
        .and_then(|routes| routes.get(plugin).cloned());
    if let Some(sender) = sender {
        let _ = sender.unbounded_send(message);
    }
}

/// Starts plugins in one shared supervisor, launched on first use and again
/// if it dies.
#[derive(Clone, Default)]
pub struct Runner {
    supervisor: Rc<RefCell<Option<Supervisor>>>,
}

impl Runner {
    pub fn start(
        &self,
        manifest: &Manifest,
        settings: &Map<String, Value>,
        bun: &Path,
        sdk: &Path,
    ) -> io::Result<Connection> {
        prepare(&manifest.dir, sdk)?;
        let data = crate::platform::support_dir()
            .join("plugin-data")
            .join(&manifest.id);
        fs::create_dir_all(&data)?;

        let mut slot = self.supervisor.borrow_mut();
        if !slot.as_mut().is_some_and(Supervisor::is_running) {
            *slot = Some(Supervisor::spawn(bun, sdk)?);
        }
        let supervisor = slot.as_mut().expect("just started");
        let (sender, incoming) = unbounded();
        if let Ok(mut routes) = supervisor.routes.lock() {
            routes.insert(manifest.id.clone(), sender);
        }
        let entry = manifest.dir.join(&manifest.main);
        let entry = entry.canonicalize().unwrap_or(entry);
        supervisor.write(
            &manifest.id,
            serde_json::json!({
                "type": "start",
                "entry": entry,
                "data_dir": data,
                "settings": settings,
            }),
        );
        Ok(Connection {
            link: Box::new(Link {
                plugin: manifest.id.clone(),
                supervisor: self.supervisor.clone(),
            }),
            incoming,
        })
    }
}

/// One plugin inside the supervisor. Dropping it stops the plugin.
struct Link {
    plugin: String,
    supervisor: Rc<RefCell<Option<Supervisor>>>,
}

impl PluginLink for Link {
    fn send(&mut self, message: &HostMessage) {
        let Ok(message) = serde_json::to_value(message) else {
            return;
        };
        if let Some(supervisor) = self.supervisor.borrow_mut().as_mut() {
            supervisor.write(&self.plugin, message);
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(supervisor) = self.supervisor.borrow_mut().as_mut() {
            supervisor.write(&self.plugin, serde_json::json!({ "type": "stop" }));
            if let Ok(mut routes) = supervisor.routes.lock() {
                routes.remove(&self.plugin);
            }
        }
    }
}

/// A value that changes whenever a file in the plugin changes, so the host
/// can reload it on save. Skips `node_modules` and hidden files.
pub fn fingerprint(dir: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    fn visit(dir: &Path, depth: usize, hasher: &mut DefaultHasher) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                if depth > 0 {
                    visit(&entry.path(), depth - 1, hasher);
                }
            } else {
                name.hash(hasher);
                metadata.len().hash(hasher);
                metadata.modified().ok().hash(hasher);
            }
        }
    }
    let mut hasher = DefaultHasher::new();
    visit(dir, 4, &mut hasher);
    hasher.finish()
}

/// Makes `@sidekick/sdk` importable from the plugin, and gives editors a
/// tsconfig, without the plugin having to install anything.
fn prepare(dir: &Path, sdk: &Path) -> io::Result<()> {
    let scope = dir.join("node_modules").join("@sidekick");
    let link = scope.join("sdk");
    if fs::symlink_metadata(&link).is_err() {
        fs::create_dir_all(&scope)?;
        std::os::unix::fs::symlink(sdk, &link)?;
    }
    let tsconfig = dir.join("tsconfig.json");
    if !tsconfig.exists() {
        fs::write(&tsconfig, TSCONFIG)?;
    }
    Ok(())
}

pub const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "jsxImportSource": "@sidekick/sdk",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  }
}
"#;

/// A folder name from a display name: "My Widget!" → "my-widget".
fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "widget".into()
    } else {
        slug.into()
    }
}

/// Creates a working plugin named `name` in `dir`, ready to edit.
pub fn create(dir: &Path, name: &str) -> io::Result<Manifest> {
    let name = match name.trim() {
        "" => "My Widget",
        name => name,
    };
    let base = slug(name);
    let mut folder = dir.join(&base);
    let mut n = 2;
    while folder.exists() {
        folder = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    fs::create_dir_all(&folder)?;
    fs::write(folder.join("tsconfig.json"), TSCONFIG)?;
    let title = serde_json::to_string(name).map_err(io::Error::other)?;
    fs::write(folder.join("index.tsx"), TEMPLATE.replace("TITLE", &title))?;
    Manifest::read(&folder).ok_or_else(|| io::Error::other("the new plugin can't be read"))
}

const TEMPLATE: &str = r#"import { Button, Card, Text, definePlugin, useSetting, useState } from "@sidekick/sdk";

// Save this file and the card reloads. The @sidekick/sdk README lists
// every element, component and style prop.
export default definePlugin({
  name: TITLE,
  icon: "sparkles",
  settings: {
    greeting: { title: "Greeting", type: "text", default: "Hello" },
  },

  card() {
    const [count, setCount] = useState(0);
    const greeting = useSetting<string>("greeting");
    return (
      <Card title={TITLE} accessory={`${count} clicks`}>
        <Text secondary>{`${greeting}! Edit index.tsx to make this yours.`}</Text>
        <div flex gap={8}>
          <Button variant="primary" label="Click me" on_click={() => setCount(count + 1)} />
          <Button label="Reset" on_click={() => setCount(0)} />
        </div>
      </Card>
    );
  },
});
"#;

/// The SDK shipped inside the app bundle, or the repository's when run
/// with `cargo run`.
pub fn sdk_dir() -> PathBuf {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/sdk")));
    match bundled {
        Some(path) if path.exists() => path,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sdk"),
    }
}

/// Finds Bun. Apps opened from Finder don't get the shell's `PATH`, so ask
/// a login shell first, then try the usual install locations.
pub fn find_bun() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SIDEKICK_BUN") {
        return Some(PathBuf::from(path));
    }
    let from_shell = Command::new("/bin/zsh")
        .args(["-lc", "command -v bun"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|output| {
            let path = String::from_utf8(output.stdout).ok()?;
            let path = PathBuf::from(path.trim());
            path.is_file().then_some(path)
        });
    if from_shell.is_some() {
        return from_shell;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    [
        home.join(".bun/bin/bun"),
        PathBuf::from("/opt/homebrew/bin/bun"),
        PathBuf::from("/usr/local/bin/bun"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(kind: &str, children: Vec<Node>) -> Node {
        Node::Element {
            kind: kind.into(),
            props: Map::new(),
            children,
        }
    }

    #[test]
    fn reads_trees_and_messages() {
        let json = r#"{"type":"render","surface":"card","plugin":"timer","tree":[
            {"t":"Card","p":{"title":"Timer"},"c":[
                {"t":"div","p":{"flex":true,"on_click":{"$h":"card:Widget/#on_click"}},"c":["25:00"]}
            ]}
        ]}"#;
        let PluginMessage::Render { surface, tree } = serde_json::from_str(json).unwrap() else {
            panic!("expected a render");
        };
        assert_eq!(surface, "card");
        let Node::Element { kind, children, .. } = &tree[0] else {
            panic!("expected an element");
        };
        assert_eq!(kind, "Card");
        let Node::Element {
            props, children, ..
        } = &children[0]
        else {
            panic!("expected an element");
        };
        assert_eq!(props["flex"], Value::Bool(true));
        assert_eq!(children, &vec![Node::Text("25:00".into())]);
        assert_eq!(
            serde_json::from_str::<PluginMessage>(r#"{"type":"exited","plugin":"timer"}"#).unwrap(),
            PluginMessage::Exited
        );
    }

    #[test]
    fn host_messages_are_json_the_sdk_reads() {
        let event = HostMessage::Event {
            handler: "card:Widget/#on_click".into(),
            value: Value::Bool(true),
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"type":"event","handler":"card:Widget/#on_click","value":true}"#
        );
        assert_eq!(
            serde_json::to_string(&HostMessage::Card { open: true }).unwrap(),
            r#"{"type":"card","open":true}"#
        );
    }

    #[test]
    fn patches_edit_the_tree_in_place() {
        let mut tree = vec![element(
            "div",
            vec![Node::Text("a".into()), element("div", vec![])],
        )];
        let patches: Vec<Patch> = serde_json::from_str(
            r#"[{"op":"replace","path":[0,0],"node":"b"},
                {"op":"props","path":[0,1],"props":{"w":20}}]"#,
        )
        .unwrap();
        assert!(apply_patches(&mut tree, patches));
        let Node::Element { children, .. } = &tree[0] else {
            panic!("expected an element");
        };
        assert_eq!(children[0], Node::Text("b".into()));
        let Node::Element { props, .. } = &children[1] else {
            panic!("expected an element");
        };
        assert_eq!(props["w"], Value::from(20));

        let wrong: Vec<Patch> =
            serde_json::from_str(r#"[{"op":"props","path":[0,0],"props":{}}]"#).unwrap();
        assert!(!apply_patches(&mut tree, wrong), "text has no props");
        let missing: Vec<Patch> =
            serde_json::from_str(r#"[{"op":"replace","path":[3],"node":"x"}]"#).unwrap();
        assert!(!apply_patches(&mut tree, missing));
    }

    #[test]
    fn plugins_are_found_by_their_definition_without_running_them() {
        let dir = std::env::temp_dir().join(format!("sidekick-plugin-{}", std::process::id()));
        let plugin = dir.join("pomodoro");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join("index.tsx"),
            r#"import { definePlugin } from "@sidekick/sdk";
               const label = { name: "not this one" };
               export default definePlugin({
                 icon: 'timer',
                 name: "Pomodoro",
                 card: () => <div>{label.name}</div>,
               });"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("unnamed")).unwrap();
        fs::write(
            dir.join("unnamed/index.ts"),
            "export default definePlugin({ card })",
        )
        .unwrap();
        fs::create_dir_all(dir.join("not-a-plugin")).unwrap();
        fs::write(dir.join("not-a-plugin/index.ts"), "console.log(1)").unwrap();

        let found = discover(&dir);
        fs::remove_dir_all(&dir).ok();
        assert_eq!(found.len(), 2);
        let pomodoro = found.iter().find(|m| m.id == "pomodoro").unwrap();
        assert_eq!(pomodoro.name, "Pomodoro");
        assert_eq!(pomodoro.icon_path(), "icons/timer.svg");
        assert_eq!(pomodoro.main, PathBuf::from("index.tsx"));
        let unnamed = found.iter().find(|m| m.id == "unnamed").unwrap();
        assert_eq!(
            (unnamed.name.as_str(), unnamed.icon.as_str()),
            ("unnamed", "puzzle")
        );
    }

    #[test]
    fn the_running_plugin_describes_itself() {
        let json = r#"{"type":"manifest","plugin":"pomodoro","name":"Pomodoro","icon":"timer",
            "width":9000,"height":null,
            "settings":[{"key":"sound","title":"Sound","type":"toggle"},
                        {"key":"mode","title":"Mode","type":"choice","options":["focus","break"]}]}"#;
        let PluginMessage::Manifest(described) = serde_json::from_str(json).unwrap() else {
            panic!("expected a manifest");
        };
        let mut manifest = Manifest {
            id: "pomodoro".into(),
            name: "pomodoro".into(),
            icon: "puzzle".into(),
            width: 280.0,
            height: None,
            settings: Vec::new(),
            dir: PathBuf::from("/plugins/pomodoro"),
            main: PathBuf::from("index.tsx"),
        };
        manifest.update(described);
        assert_eq!(manifest.name, "Pomodoro");
        assert_eq!(manifest.width, 480.0);
        assert_eq!(manifest.height, None);

        let saved = Map::from_iter([("mode".to_string(), Value::from("break"))]);
        let values = manifest.settings_with(Some(&saved));
        assert_eq!(values["sound"], Value::from(false));
        assert_eq!(values["mode"], Value::from("break"));
        assert_eq!(manifest.settings_with(None)["mode"], Value::from("focus"));
    }

    #[test]
    fn new_plugins_get_a_folder_and_a_working_template() {
        let dir = std::env::temp_dir().join(format!("sidekick-new-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let first = create(&dir, "My Widget!").unwrap();
        let second = create(&dir, "My Widget!").unwrap();
        let source = fs::read_to_string(first.dir.join("index.tsx")).unwrap();
        let has_package = first.dir.join("package.json").exists();
        fs::remove_dir_all(&dir).ok();
        assert_eq!(first.id, "my-widget");
        assert_eq!(second.id, "my-widget-2");
        assert_eq!(first.name, "My Widget!");
        assert_eq!(first.icon, "sparkles");
        assert!(source.contains(r#"name: "My Widget!","#));
        assert!(source.contains(r#"<Card title={"My Widget!"}"#));
        assert!(!has_package, "everything lives in definePlugin");
        assert_eq!(slug("  ..  "), "widget");
    }
}
