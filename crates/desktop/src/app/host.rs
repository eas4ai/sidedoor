//! Composes native services, persistence and the plugin runtime for the app.
pub use ::platform::paths::*;
pub use ::platform::{Accessibility, AppInfo, COMPUTER_NAME, Copied, LoginItem, REVEAL_LABEL};
use domain::{
    clipboard::{ClipKind, History},
    config::{Appearance, Config},
    geometry::{Point, Screen},
};
use plugin_host::{Connection, Installer, Manifest, Staged};
#[cfg(any(target_os = "macos", test))]
use std::path::PathBuf;
use std::{collections::HashSet, io, path::Path, sync::Arc};
pub trait Host: ::platform::Platform {
    fn save_config(&self, config: &Config) -> io::Result<()>;
    fn save_history(&self, history: &History) -> io::Result<()>;
    fn plugins(&self) -> Vec<Manifest>;
    fn start_plugin(
        &self,
        manifest: &Manifest,
        settings: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Connection, String>;
    fn create_plugin(&self, name: &str) -> Result<Manifest, String>;
    /// Downloads plugins; it runs off the main thread.
    fn plugin_installer(&self) -> Arc<dyn Installer>;
    /// Moves a downloaded plugin into the plugins folder.
    fn install_plugin(&self, staged: Staged) -> Result<Manifest, String>;
    /// Moves a plugin's folder to the Trash and deletes its saved data.
    fn delete_plugin(&self, manifest: &Manifest) -> Result<(), String>;
}
#[derive(Default)]
pub struct NativeHost {
    native: ::platform::native::NativePlatform,
    plugins: plugin_host::Runner,
}
impl ::platform::Platform for NativeHost {
    fn main_screen(&self) -> Option<Screen> {
        self.native.main_screen()
    }
    fn pointer(&self) -> Point {
        self.native.pointer()
    }
    fn running_bundle_ids(&self) -> HashSet<String> {
        self.native.running_bundle_ids()
    }
    fn accessibility(&self) -> Accessibility {
        self.native.accessibility()
    }
    fn utc_offset(&self) -> i64 {
        self.native.utc_offset()
    }
    fn app_by_bundle_id(&self, bundle_id: &str) -> Option<AppInfo> {
        self.native.app_by_bundle_id(bundle_id)
    }
    fn app_at(&self, path: &Path) -> Option<AppInfo> {
        self.native.app_at(path)
    }
    fn open(&self, path: &Path) -> io::Result<()> {
        self.native.open(path)
    }
    fn reveal_in_finder(&self, path: &Path) {
        self.native.reveal_in_finder(path)
    }
    fn trash(&self, path: &Path) -> io::Result<()> {
        self.native.trash(path)
    }
    fn pasteboard_change_count(&self) -> isize {
        self.native.pasteboard_change_count()
    }
    fn read_pasteboard(&self, image_dir: &Path) -> Option<Copied> {
        self.native.read_pasteboard(image_dir)
    }
    fn write_pasteboard(&self, kind: &ClipKind) {
        self.native.write_pasteboard(kind)
    }
    fn notify(&self, source: &str, title: &str, body: &str) {
        self.native.notify(source, title, body)
    }
    fn set_appearance(&self, appearance: Appearance) {
        self.native.set_appearance(appearance)
    }
    fn login_item(&self) -> LoginItem {
        self.native.login_item()
    }
    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        self.native.set_launch_at_login(enabled)
    }
}
impl Host for NativeHost {
    fn plugin_installer(&self) -> Arc<dyn Installer> {
        #[cfg(target_os = "macos")]
        let bun = bun().cloned();
        #[cfg(target_os = "windows")]
        let bun = plugin_host::find_bun();
        Arc::new(plugin_host::Downloader {
            plugins: plugin_host::plugins_dir(),
            bun,
        })
    }

    fn install_plugin(&self, staged: Staged) -> Result<Manifest, String> {
        let name = staged.manifest.name.clone();
        plugin_host::install::install(staged, &plugin_host::plugins_dir())
            .map_err(|err| format!("Couldn't install {name}: {err}"))
    }

    fn delete_plugin(&self, manifest: &Manifest) -> Result<(), String> {
        // Only what lives in the plugins folder; never the app's own files.
        let plugins = plugin_host::plugins_dir();
        if manifest.dir.parent() != Some(plugins.as_path()) {
            return Err(format!("{} isn't in the plugins folder.", manifest.name));
        }
        ::platform::Platform::trash(self, &manifest.dir)
            .map_err(|err| format!("Couldn't move {} to the Trash: {err}", manifest.name))?;
        let data = support_dir().join("plugin-data").join(&manifest.id);
        if data.exists()
            && let Err(err) = std::fs::remove_dir_all(&data)
        {
            eprintln!("sidedoor: couldn't delete {}: {err}", data.display());
        }
        Ok(())
    }

    fn save_config(&self, config: &Config) -> io::Result<()> {
        services::storage::save_config(config)
    }
    fn save_history(&self, history: &History) -> io::Result<()> {
        services::storage::save_history(history)
    }
    #[cfg(target_os = "macos")]
    fn plugins(&self) -> Vec<plugin_host::Manifest> {
        plugin_host::discover(&plugin_host::plugins_dir())
    }

    #[cfg(target_os = "macos")]
    fn start_plugin(
        &self,
        manifest: &plugin_host::Manifest,
        settings: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<plugin_host::Connection, String> {
        let bun = bun().ok_or("Plugins need Bun. Install it from bun.sh, then reload.")?;
        self.plugins
            .start(manifest, settings, bun, &plugin_host::sdk_dir())
            .map_err(|err| format!("Couldn't start {}: {err}", manifest.name))
    }

    #[cfg(target_os = "macos")]
    fn create_plugin(&self, name: &str) -> Result<plugin_host::Manifest, String> {
        let dir = plugin_host::plugins_dir();
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        plugin_host::create(&dir, name).map_err(|err| format!("Couldn't create the plugin: {err}"))
    }
    #[cfg(target_os = "windows")]
    fn plugins(&self) -> Vec<plugin_host::Manifest> {
        plugin_host::discover(&plugin_host::plugins_dir())
    }
    #[cfg(target_os = "windows")]
    fn start_plugin(
        &self,
        manifest: &plugin_host::Manifest,
        settings: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<plugin_host::Connection, String> {
        let bun = plugin_host::find_bun()
            .ok_or("Bun is missing from the installation. Reinstall Sidedoor, then reload.")?;
        self.plugins
            .start(manifest, settings, &bun, &plugin_host::sdk_dir())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "windows")]
    fn create_plugin(&self, name: &str) -> Result<plugin_host::Manifest, String> {
        let dir = plugin_host::plugins_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        plugin_host::create(&dir, name).map_err(|e| e.to_string())
    }
}
/// Bun, looked up once; finding it can mean asking a login shell.
#[cfg(target_os = "macos")]
fn bun() -> Option<&'static PathBuf> {
    static BUN: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    BUN.get_or_init(plugin_host::find_bun).as_ref()
}

#[cfg(test)]
pub mod fake {
    //! An in-memory platform for tests.

    use super::*;
    use futures::channel::mpsc::{UnboundedSender, unbounded};
    use plugin_host::{HostMessage, PluginLink, PluginMessage};
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    /// Records what the host sends a fake plugin.
    struct Recorder {
        id: String,
        sent: Rc<RefCell<Vec<(String, HostMessage)>>>,
        native: Option<NativePlugin>,
    }

    impl PluginLink for Recorder {
        fn send(&mut self, message: &HostMessage) {
            self.sent
                .borrow_mut()
                .push((self.id.clone(), message.clone()));
            if let Some(native) = &mut self.native {
                native.send(message);
            }
        }
    }

    // Runs each built-in's real TSX and SDK against fake native services.
    // A round trip completes before returning to GPUI's deterministic executor.
    struct NativePlugin {
        child: std::process::Child,
        input: std::process::ChildStdin,
        output: std::io::BufReader<std::process::ChildStdout>,
        incoming: UnboundedSender<PluginMessage>,
    }

    impl NativePlugin {
        fn start(manifest: &Manifest, incoming: UnboundedSender<PluginMessage>) -> Self {
            use std::process::{Command, Stdio};
            static BUN: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
            let bun = BUN.get_or_init(|| {
                plugin_host::find_bun().expect("Bun is required for the plugin UI tests")
            });
            let mut child = Command::new(bun)
                .arg(plugin_host::sdk_dir().join("test/native-host.ts"))
                .arg(manifest.dir.join(&manifest.main))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut plugin = Self {
                input: child.stdin.take().unwrap(),
                output: std::io::BufReader::new(child.stdout.take().unwrap()),
                child,
                incoming,
            };
            plugin.receive();
            plugin
        }

        fn receive(&mut self) {
            use std::io::BufRead as _;
            let mut line = String::new();
            self.output.read_line(&mut line).unwrap();
            let messages: Vec<PluginMessage> = serde_json::from_str(&line)
                .unwrap_or_else(|err| panic!("TSX test host stopped: {err}: {line}"));
            for message in messages {
                assert!(
                    !matches!(message, PluginMessage::Error { .. }),
                    "{message:?}"
                );
                self.incoming.unbounded_send(message).unwrap();
            }
        }

        fn send(&mut self, message: &HostMessage) {
            use std::io::Write as _;
            writeln!(self.input, "{}", serde_json::to_string(message).unwrap()).unwrap();
            self.input.flush().unwrap();
            self.receive();
        }
    }

    impl Drop for NativePlugin {
        fn drop(&mut self) {
            self.child.kill().ok();
            self.child.wait().ok();
        }
    }

    #[derive(Default)]
    pub struct FakePlatform {
        pub screen: RefCell<Option<Screen>>,
        pub pointer: RefCell<Point>,
        pub running: RefCell<HashSet<String>>,
        pub apps: RefCell<Vec<AppInfo>>,
        pub opened: RefCell<Vec<PathBuf>>,
        pub pasteboard: RefCell<(isize, Option<Copied>)>,
        pub written: RefCell<Vec<ClipKind>>,
        /// Notifications shown: source, title and body.
        pub notified: RefCell<Vec<[String; 3]>>,
        pub saved_configs: RefCell<Vec<Config>>,
        pub appearance: RefCell<Option<Appearance>>,
        pub login: RefCell<LoginItem>,
        pub plugins: RefCell<Vec<Manifest>>,
        /// Lets a test speak as each started plugin.
        pub plugin_inbox: RefCell<HashMap<String, UnboundedSender<PluginMessage>>>,
        pub plugin_sent: Rc<RefCell<Vec<(String, HostMessage)>>>,
        /// Folders moved to the Trash.
        pub trashed: RefCell<Vec<PathBuf>>,
        pub installer: Arc<FakeInstaller>,
        /// The settings each plugin was last started with.
        pub plugin_started: RefCell<Vec<(String, serde_json::Map<String, serde_json::Value>)>>,
    }

    impl FakePlatform {
        pub fn with_apps(apps: &[(&str, &str)]) -> Self {
            let fake = Self::default();
            *fake.screen.borrow_mut() = Some(Screen {
                frame: domain::geometry::Rect::new(0.0, 0.0, 1512.0, 982.0),
                visible: domain::geometry::Rect::new(0.0, 0.0, 1512.0, 949.0),
            });
            *fake.apps.borrow_mut() = apps
                .iter()
                .map(|(id, name)| AppInfo {
                    bundle_id: (*id).into(),
                    name: (*name).into(),
                    path: PathBuf::from(format!("/Applications/{name}.app")),
                    icon: None,
                })
                .collect();
            *fake.plugins.borrow_mut() = vec![Manifest {
                id: "counter".into(),
                name: "Counter".into(),
                icon: "timer".into(),
                width: 260.0,
                height: Some(200.0),
                settings: vec![plugin_host::SettingSpec {
                    key: "unit".into(),
                    title: "Unit".into(),
                    description: None,
                    kind: plugin_host::SettingKind::Text,
                    options: Vec::new(),
                    default: Some(serde_json::Value::from("clicks")),
                }],
                clickable: true,
                actions: vec![plugin_host::PluginAction {
                    key: "reset".into(),
                    title: "Reset Counter".into(),
                }],
                windows: vec![plugin_host::PluginWindow {
                    key: "history".into(),
                    title: "Counter History".into(),
                    width: 400.0,
                    height: 300.0,
                }],
                data: Vec::new(),
                dir: PathBuf::from("/plugins/counter"),
                main: PathBuf::from("index.tsx"),
                source: None,
            }];
            fake
        }

        /// Delivers `incoming` as if plugin `id` had sent it.
        pub fn plugin_says(&self, id: &str, incoming: PluginMessage) {
            self.plugin_inbox.borrow()[id]
                .unbounded_send(incoming)
                .expect("the plugin is running");
        }

        pub fn copy(&self, kind: ClipKind) {
            let mut pasteboard = self.pasteboard.borrow_mut();
            pasteboard.0 += 1;
            pasteboard.1 = Some(Copied { kind, source: None });
        }
    }

    impl ::platform::Platform for FakePlatform {
        fn main_screen(&self) -> Option<Screen> {
            *self.screen.borrow()
        }
        fn pointer(&self) -> Point {
            *self.pointer.borrow()
        }
        fn running_bundle_ids(&self) -> HashSet<String> {
            self.running.borrow().clone()
        }
        fn accessibility(&self) -> Accessibility {
            Accessibility::default()
        }
        fn utc_offset(&self) -> i64 {
            0
        }
        fn app_by_bundle_id(&self, bundle_id: &str) -> Option<AppInfo> {
            self.apps
                .borrow()
                .iter()
                .find(|app| app.bundle_id == bundle_id)
                .cloned()
        }
        fn app_at(&self, path: &Path) -> Option<AppInfo> {
            self.apps
                .borrow()
                .iter()
                .find(|app| app.path == path)
                .cloned()
        }
        fn open(&self, path: &Path) -> io::Result<()> {
            self.opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
        fn reveal_in_finder(&self, _: &Path) {}
        fn trash(&self, path: &Path) -> io::Result<()> {
            self.trashed.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
        fn pasteboard_change_count(&self) -> isize {
            self.pasteboard.borrow().0
        }
        fn read_pasteboard(&self, _: &Path) -> Option<Copied> {
            self.pasteboard.borrow().1.clone()
        }
        fn write_pasteboard(&self, kind: &ClipKind) {
            self.written.borrow_mut().push(kind.clone());
        }
        fn notify(&self, source: &str, title: &str, body: &str) {
            self.notified
                .borrow_mut()
                .push([source, title, body].map(String::from));
        }
        fn set_appearance(&self, appearance: Appearance) {
            *self.appearance.borrow_mut() = Some(appearance);
        }
        fn login_item(&self) -> LoginItem {
            *self.login.borrow()
        }
        fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
            *self.login.borrow_mut() = if enabled {
                LoginItem::On
            } else {
                LoginItem::Off
            };
            Ok(())
        }
    }
    impl Host for FakePlatform {
        fn save_config(&self, config: &Config) -> io::Result<()> {
            self.saved_configs.borrow_mut().push(config.clone());
            Ok(())
        }
        fn save_history(&self, _: &History) -> io::Result<()> {
            Ok(())
        }
        fn plugins(&self) -> Vec<Manifest> {
            self.plugins.borrow().clone()
        }
        fn start_plugin(
            &self,
            manifest: &Manifest,
            settings: &serde_json::Map<String, serde_json::Value>,
        ) -> Result<Connection, String> {
            self.plugin_started
                .borrow_mut()
                .push((manifest.id.clone(), settings.clone()));
            let (sender, incoming) = unbounded();
            let native = crate::builtins::contains(&manifest.id)
                .then(|| NativePlugin::start(manifest, sender.clone()));
            self.plugin_inbox
                .borrow_mut()
                .insert(manifest.id.clone(), sender);
            Ok(Connection {
                link: Box::new(Recorder {
                    id: manifest.id.clone(),
                    sent: self.plugin_sent.clone(),
                    native,
                }),
                incoming,
            })
        }
        fn create_plugin(&self, name: &str) -> Result<Manifest, String> {
            let manifest = Manifest {
                id: name.to_lowercase().replace(' ', "-"),
                name: name.into(),
                icon: "sparkles".into(),
                width: 280.0,
                height: None,
                settings: Vec::new(),
                clickable: false,
                actions: Vec::new(),
                windows: Vec::new(),
                data: Vec::new(),
                dir: PathBuf::from(format!("/plugins/{}", name.to_lowercase())),
                main: PathBuf::from("index.tsx"),
                source: None,
            };
            self.plugins.borrow_mut().push(manifest.clone());
            Ok(manifest)
        }
        fn plugin_installer(&self) -> Arc<dyn Installer> {
            self.installer.clone()
        }
        fn install_plugin(&self, staged: Staged) -> Result<Manifest, String> {
            let manifest = Manifest {
                id: staged.id.clone(),
                dir: PathBuf::from(format!("/plugins/{}", staged.id)),
                source: Some(staged.source.clone()),
                ..staged.manifest.clone()
            };
            let mut plugins = self.plugins.borrow_mut();
            plugins.retain(|plugin| plugin.id != manifest.id);
            plugins.push(manifest.clone());
            Ok(manifest)
        }
        fn delete_plugin(&self, manifest: &Manifest) -> Result<(), String> {
            ::platform::Platform::trash(self, &manifest.dir).map_err(|err| err.to_string())?;
            self.plugins
                .borrow_mut()
                .retain(|plugin| plugin.id != manifest.id);
            Ok(())
        }
    }

    /// Serves plugins as if from the web: each link, by its label (such as
    /// `owner/repo`), holds one plugin, at the commit in `commit`.
    #[derive(Default)]
    pub struct FakeInstaller {
        pub repos: std::sync::Mutex<HashMap<String, String>>,
        pub commit: std::sync::Mutex<String>,
        /// Every download, by label.
        pub fetched: std::sync::Mutex<Vec<String>>,
    }

    impl FakeInstaller {
        pub fn publish(&self, repo: &str, name: &str, commit: &str) {
            self.repos.lock().unwrap().insert(repo.into(), name.into());
            *self.commit.lock().unwrap() = commit.into();
        }
    }

    impl Installer for FakeInstaller {
        fn fetch(&self, source: &plugin_host::Link) -> Result<Staged, String> {
            let label = source.label();
            self.fetched.lock().unwrap().push(label.clone());
            let name = self
                .repos
                .lock()
                .unwrap()
                .get(&label)
                .cloned()
                .ok_or_else(|| format!("Couldn't find {label} on GitHub."))?;
            Ok(Staged {
                manifest: Manifest {
                    id: source.name(),
                    name,
                    icon: "puzzle".into(),
                    width: 280.0,
                    height: None,
                    settings: Vec::new(),
                    clickable: false,
                    actions: Vec::new(),
                    windows: Vec::new(),
                    data: Vec::new(),
                    dir: PathBuf::from(format!("/staging/{}", source.name())),
                    main: PathBuf::from("index.tsx"),
                    source: None,
                },
                source: plugin_host::Source {
                    link: source.clone(),
                    commit: self.latest_commit(source)?,
                },
                id: source.name(),
                // Nothing on disk to clean up.
                root: PathBuf::from("/nonexistent/sidedoor-staging"),
            })
        }

        fn latest_commit(&self, _: &plugin_host::Link) -> Result<String, String> {
            Ok(self.commit.lock().unwrap().clone())
        }
    }
}
