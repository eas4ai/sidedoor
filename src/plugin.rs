//! Plugins: widgets written in TSX against the SDK in `sdk/`, each run by
//! Bun in its own process. A plugin sends the trees it renders as JSON lines
//! on stdout; the host draws them (see `plugin_ui`) and sends events back on
//! stdin.

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
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
    pub height: f64,
    pub dir: PathBuf,
    /// The entry file, relative to `dir`.
    pub main: PathBuf,
}

#[derive(Deserialize)]
struct PackageJson {
    #[serde(default)]
    main: Option<String>,
    sidekick: Option<SidekickSection>,
}

#[derive(Deserialize)]
struct SidekickSection {
    name: String,
    #[serde(default = "default_icon")]
    icon: String,
    #[serde(default = "default_width")]
    width: f64,
    #[serde(default = "default_height")]
    height: f64,
}

fn default_icon() -> String {
    "puzzle".into()
}
fn default_width() -> f64 {
    280.0
}
fn default_height() -> f64 {
    160.0
}

impl Manifest {
    /// Reads `dir/package.json`; `None` unless it declares a plugin.
    pub fn read(dir: &Path) -> Option<Self> {
        let json = fs::read(dir.join("package.json")).ok()?;
        let package: PackageJson = serde_json::from_slice(&json).ok()?;
        let section = package.sidekick?;
        Some(Self {
            id: dir.file_name()?.to_string_lossy().into_owned(),
            name: section.name,
            icon: section.icon,
            width: section.width.clamp(120.0, 480.0),
            height: section.height.clamp(60.0, 600.0),
            dir: dir.to_path_buf(),
            main: PathBuf::from(package.main.unwrap_or_else(|| "index.tsx".into())),
        })
    }

    /// The asset path of the plugin's icon.
    pub fn icon_path(&self) -> String {
        icon_path(&self.icon)
    }
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
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
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

/// What a plugin sends.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginMessage {
    Render { surface: String, tree: Vec<Node> },
    OpenUrl { url: String },
    OpenPath { path: String },
    Copy { text: String },
    Error { message: String },
}

/// What the host sends.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    Event { handler: String, value: Value },
}

/// Everything that comes back from a plugin process.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Message(PluginMessage),
    /// A line the plugin wrote to stderr: its logs, or why it failed.
    Log(String),
    /// The process ended.
    Exited,
}

/// The host's side of a running plugin.
pub trait PluginLink {
    fn send(&mut self, message: &HostMessage);
}

/// A running plugin: messages to it, and what it sends back.
pub struct Connection {
    pub link: Box<dyn PluginLink>,
    pub incoming: UnboundedReceiver<Incoming>,
}

// MARK: Processes

/// A plugin running under Bun. Dropping it stops the process.
struct Process {
    child: Child,
    stdin: ChildStdin,
}

impl PluginLink for Process {
    fn send(&mut self, message: &HostMessage) {
        if let Ok(line) = serde_json::to_string(message) {
            // A plugin that has exited just misses the event.
            let _ = writeln!(self.stdin, "{line}").and_then(|()| self.stdin.flush());
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts `manifest`'s plugin with Bun. It exits by itself when the host
/// goes away and its stdin closes.
pub fn spawn(manifest: &Manifest, bun: &Path, sdk: &Path) -> io::Result<Connection> {
    prepare(&manifest.dir, sdk)?;
    let data = crate::platform::support_dir()
        .join("plugin-data")
        .join(&manifest.id);
    fs::create_dir_all(&data)?;

    let mut child = Command::new(bun)
        .arg(&manifest.main)
        .current_dir(&manifest.dir)
        .env("SIDEKICK_PLUGIN", "1")
        .env("SIDEKICK_DATA_DIR", &data)
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

    let (sender, incoming) = unbounded();
    read_lines(stdout, sender.clone(), |line| {
        match serde_json::from_str::<PluginMessage>(&line) {
            Ok(message) => Incoming::Message(message),
            Err(_) => Incoming::Log(line),
        }
    });
    read_lines(stderr, sender, Incoming::Log);
    Ok(Connection {
        link: Box::new(Process { child, stdin }),
        incoming,
    })
}

fn read_lines(
    source: impl io::Read + Send + 'static,
    sender: UnboundedSender<Incoming>,
    parse: impl Fn(String) -> Incoming + Send + 'static,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(source).lines() {
            let Ok(line) = line else { break };
            if sender.unbounded_send(parse(line)).is_err() {
                return;
            }
        }
        let _ = sender.unbounded_send(Incoming::Exited);
    });
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

/// Makes `@sidekick/sdk` importable from the plugin, and JSX compile against
/// it, without the plugin having to install anything.
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

    #[test]
    fn reads_trees_and_messages() {
        let json = r#"{"type":"render","surface":"card","tree":[
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

        let open: PluginMessage =
            serde_json::from_str(r#"{"type":"open_url","url":"https://example.com"}"#).unwrap();
        assert_eq!(
            open,
            PluginMessage::OpenUrl {
                url: "https://example.com".into()
            }
        );
    }

    #[test]
    fn events_are_json_lines_the_sdk_reads() {
        let event = HostMessage::Event {
            handler: "card:Widget/#on_click".into(),
            value: Value::Bool(true),
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"type":"event","handler":"card:Widget/#on_click","value":true}"#
        );
    }

    #[test]
    fn manifests_come_from_package_json() {
        let dir = std::env::temp_dir().join(format!("sidekick-plugin-{}", std::process::id()));
        let plugin = dir.join("pomodoro");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"pomodoro","main":"widget.tsx","sidekick":{"name":"Pomodoro","icon":"timer","height":9000}}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("not-a-plugin")).unwrap();
        fs::write(dir.join("not-a-plugin/package.json"), r#"{"name":"x"}"#).unwrap();

        let found = discover(&dir);
        fs::remove_dir_all(&dir).ok();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "pomodoro");
        assert_eq!(found[0].main, PathBuf::from("widget.tsx"));
        assert_eq!(found[0].width, 280.0);
        assert_eq!(found[0].height, 600.0);
        assert_eq!(found[0].icon_path(), "icons/timer.svg");
    }
}
