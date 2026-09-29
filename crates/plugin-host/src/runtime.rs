use crate::{Connection, HostMessage, Manifest, PluginLink, PluginMessage, sdk::prepare};
use futures::channel::mpsc::{UnboundedSender, unbounded};
use serde_json::{Map, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    rc::Rc,
    sync::{Arc, Mutex},
};
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
        let mut command = Command::new(bun);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            // The GUI host owns these pipes; no console should flash at launch.
            command.creation_flags(0x08000000);
        }
        let mut child = command
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
        if !domain::builtins::contains(&manifest.id) {
            prepare(&manifest.dir, sdk)?;
        }
        let data = platform::support_dir()
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
