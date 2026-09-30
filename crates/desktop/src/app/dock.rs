//! The dock model: its items, the plugins behind them, and the edits the
//! user makes by dragging and right-clicking.

use crate::app::host::{Accessibility, AppInfo, Host, LoginItem};
use domain::config::{Appearance, Config, ItemConfig, MAX_ITEMS};
use domain::geometry::{self, Edge, Rect, Reveal, Screen};
use domain::shortcut::Shortcut;
use futures::StreamExt as _;
use gpui_kit::{AppContext as _, Context, EventEmitter, SharedString, Task};
use plugin_host::{
    HostMessage, Installer, Manifest, Node, PluginLink, PluginMessage, Staged, apply_patches,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

const POINTER_INTERVAL: Duration = Duration::from_millis(40);
const SYSTEM_INTERVAL: Duration = Duration::from_secs(2);
const PLUGIN_RELOAD_INTERVAL: Duration = Duration::from_secs(1);
/// How long a shortcut shows a widget's card before tucking the dock away.
const PEEK: Duration = Duration::from_secs(3);
/// Grace period before a card closes, so the pointer can travel onto it.
const CARD_CLOSE_DELAY: Duration = Duration::from_millis(160);

pub struct DockItem {
    /// Stable identity for element IDs, derived from the item itself.
    pub id: SharedString,
    pub kind: ItemKind,
}

// A dock holds at most a dozen items; boxing the manifest buys nothing.
#[allow(clippy::large_enum_variant)]
pub enum ItemKind {
    App(AppInfo),
    Plugin(Manifest),
}

impl DockItem {
    fn plugin(manifest: Manifest) -> Self {
        Self {
            id: format!("plugin:{}", manifest.id).into(),
            kind: ItemKind::Plugin(manifest),
        }
    }

    fn app(app: AppInfo) -> Self {
        Self {
            id: format!("app:{}", app.bundle_id).into(),
            kind: ItemKind::App(app),
        }
    }

    fn config(&self) -> ItemConfig {
        match &self.kind {
            ItemKind::App(app) => ItemConfig::App {
                bundle_id: app.bundle_id.clone(),
            },
            ItemKind::Plugin(manifest) => ItemConfig::Plugin {
                id: manifest.id.clone(),
            },
        }
    }
}

/// A plugin while it is in the dock: its process and what it last drew.
pub struct PluginState {
    pub tile: Option<Vec<Node>>,
    pub card: Option<Vec<Node>>,
    /// What each open window shows, by the window's key.
    pub windows: HashMap<String, Vec<Node>>,
    /// Windows open on screen; they stay open across a reload.
    pub open_windows: BTreeSet<String>,
    /// Why it isn't drawing, e.g. a compile error.
    pub problem: Option<SharedString>,
    /// Recent `console.log` lines and errors, oldest first.
    pub logs: VecDeque<SharedString>,
    /// The card's content height as last drawn, for cards that fit it.
    pub height: Option<f64>,
    link: Option<Box<dyn PluginLink>>,
    /// The plugin's files when it started; a change reloads it.
    fingerprint: u64,
    _task: Option<Task<()>>,
}

/// Requests the dock makes of the app around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockEvent {
    /// A plugin asked for one of its windows.
    OpenPluginWindow { plugin: String, key: String },
    /// A plugin closed one of its windows, or stopped.
    ClosePluginWindow { plugin: String, key: String },
    /// Shortcuts were assigned or removed; re-register them.
    ShortcutsChanged,
}

impl EventEmitter<DockEvent> for Dock {}

/// Installing a plugin from a link.
#[derive(Clone, Debug, PartialEq)]
pub enum InstallState {
    Idle,
    /// Downloading the plugin at this label, e.g. `owner/repo`.
    Downloading(SharedString),
    Failed(SharedString),
}

/// Checking an installed plugin's link for a newer version.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    Checking,
    Updating,
    UpToDate,
    Updated,
    Failed(SharedString),
}

/// Which background work a dock runs. Tests turn it all off.
#[derive(Clone, Copy)]
pub struct Services {
    pub live: bool,
}

pub struct Dock {
    pub app_updates: gpui_kit::Entity<super::updates::AppUpdates>,
    pub automatic_update_checks: bool,
    platform: Rc<dyn Host>,
    pub items: Vec<DockItem>,
    pub edge: Edge,
    appearance: Appearance,
    /// Global shortcuts by item id.
    shortcuts: BTreeMap<String, Shortcut>,
    pub running: HashSet<String>,
    pub accessibility: Accessibility,
    /// The item whose card is open.
    card: Option<usize>,
    pointer_on_item: Option<usize>,
    pointer_on_card: bool,
    close_card: Option<Task<()>>,
    screen: Screen,
    reveal: Reveal,
    /// Where the open card's window is, in screen points.
    card_frame: Option<Rect>,
    /// A context menu is open; the dock stays as it is until it closes,
    /// as it does while a macOS menu tracks the pointer.
    menu_open: bool,
    /// Whether this dock follows the real pointer (off in tests, which drive
    /// the views directly).
    live: bool,
    /// Running plugins, by plugin id.
    plugins: BTreeMap<String, PluginState>,
    /// Saved plugin settings, by plugin id.
    plugin_settings: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    /// The plugin whose card was last told it is open.
    open_plugin: Option<String>,
    /// Plugins the user agreed to run, by id.
    trusted: BTreeSet<String>,
    /// Plugins to offer in Settings › Plugins: the built-in list until the
    /// current one is fetched.
    pub gallery: Vec<services::gallery::Entry>,
    /// The current gallery is being fetched, or was this session.
    gallery_task: Option<Task<()>>,
    /// Where apps dragged in from another app would land, while they're held
    /// over the dock. The dock grows a slot to make room for them.
    incoming: Option<usize>,
    /// Plugins in the plugins folder, as of the last scan. Reading them is
    /// disk work, so views use this rather than scanning as they draw.
    discovered: Vec<Manifest>,
    /// Whether the app opens at login, as of the last check. Asking is a
    /// round trip to a system service, too slow to do on every frame.
    login_item: LoginItem,
    /// The install from a link in progress, or why the last one failed.
    pub install: InstallState,
    /// Update checks, by plugin id.
    pub updates: HashMap<String, UpdateState>,
    update_tasks: HashMap<String, Task<()>>,
    _observe_self: Option<gpui_kit::Subscription>,
    _observe_updates: gpui_kit::Subscription,
    _quit: Option<gpui_kit::Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Dock {
    pub fn new(
        mut config: Config,
        platform: Rc<dyn Host>,
        screen: Screen,
        services: Services,
        cx: &mut Context<Self>,
    ) -> Self {
        config.migrate_official();
        let discovered = discover_plugins(platform.as_ref());
        let items = resolve_items(&config.items, platform.as_ref(), &discovered);
        // Plugins already in the dock were agreed to when they were added.
        let mut trusted = std::mem::take(&mut config.trusted_plugins);
        trusted.extend(items.iter().filter_map(|item| match &item.kind {
            ItemKind::Plugin(manifest) => Some(manifest.id.clone()),
            _ => None,
        }));
        let shortcuts = config
            .shortcuts
            .iter()
            .filter_map(|(id, text)| match Shortcut::parse(text) {
                Some(shortcut) => Some((id.clone(), shortcut)),
                None => {
                    eprintln!("sidedoor: ignoring the shortcut \"{text}\" for {id}");
                    None
                }
            })
            .collect();
        let mut tasks = Vec::new();
        if services.live {
            tasks.push(every(POINTER_INTERVAL, cx, Self::poll_pointer));
            tasks.push(every(SYSTEM_INTERVAL, cx, Self::poll_system));
            tasks.push(every(PLUGIN_RELOAD_INTERVAL, cx, Self::poll_plugins));
        }
        // Stop plugin processes with the app rather than leave them behind.
        let quit = services.live.then(|| {
            cx.on_app_quit(|this: &mut Self, _| {
                this.plugins.clear();
                async {}
            })
        });
        let automatic_update_checks = config.automatically_check_for_updates;
        let app_updates = cx.new(|cx| {
            super::updates::AppUpdates::new(
                automatic_update_checks,
                services.live,
                platform.clone(),
                cx,
            )
        });
        let observe_updates = cx.observe(&app_updates, |_, _, cx| cx.notify());
        Self {
            app_updates,
            automatic_update_checks,
            running: platform.running_bundle_ids(),
            accessibility: platform.accessibility(),
            platform,
            items,
            edge: config.edge,
            appearance: config.appearance,
            shortcuts,
            card: None,
            pointer_on_item: None,
            pointer_on_card: false,
            close_card: None,
            screen,
            menu_open: false,
            card_frame: None,
            reveal: Reveal::default(),
            live: services.live,
            plugins: BTreeMap::new(),
            plugin_settings: config.plugin_settings,
            open_plugin: None,
            trusted,
            gallery: services::gallery::bundled(),
            gallery_task: None,
            incoming: None,
            discovered,
            login_item: LoginItem::default(),
            install: InstallState::Idle,
            updates: HashMap::new(),
            update_tasks: HashMap::new(),
            _observe_self: None,
            _observe_updates: observe_updates,
            _quit: quit,
            _tasks: tasks,
        }
    }

    /// Starts the plugins in the dock; call once the dock is an entity.
    pub fn start_plugins(&mut self, cx: &mut Context<Self>) {
        // Tell a plugin when its card opens and closes, whatever caused it.
        self._observe_self = Some(cx.observe_self(|this, _| {
            this.sync_open_plugin();
        }));
        let manifests: Vec<Manifest> = self
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Plugin(manifest) => Some(manifest.clone()),
                _ => None,
            })
            .collect();
        for manifest in manifests {
            self.start_plugin(&manifest, cx);
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// Frame of the dock while shown.
    pub fn frame(&self) -> Rect {
        let slots = self.items.len() + usize::from(self.incoming.is_some());
        geometry::dock_frame(self.screen, self.edge, slots)
    }

    /// The slot that apps dragged in from another app would land in.
    pub fn incoming(&self) -> Option<usize> {
        self.incoming
    }

    /// Opens a gap at slot `at` for apps being dragged in, or closes it.
    pub fn set_incoming(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let at = at.map(|at| at.min(self.items.len()));
        if at != self.incoming {
            self.incoming = at;
            cx.notify();
        }
    }

    /// Whether dropping `paths` would add anything: an app not already in
    /// the dock, with room for it.
    pub fn accepts_paths(&self, paths: &[PathBuf]) -> bool {
        self.items.len() < MAX_ITEMS
            && paths.iter().any(|path| {
                self.platform
                    .app_at(path)
                    .is_some_and(|app| self.index_of(&DockItem::app(app).id).is_none())
            })
    }

    pub fn is_shown(&self) -> bool {
        self.reveal.is_shown()
    }

    /// The item whose card or tooltip is open.
    pub fn card(&self) -> Option<(usize, &DockItem)> {
        let index = self.card.filter(|_| self.is_shown())?;
        Some((index, self.items.get(index)?))
    }

    pub fn is_running(&self, app: &AppInfo) -> bool {
        self.running.contains(&app.bundle_id)
    }

    /// Installed apps, where the app picker lists them itself.
    pub fn installed_apps(&self) -> Vec<AppInfo> {
        self.platform.installed_apps()
    }

    pub fn has_app(&self, bundle_id: &str) -> bool {
        self.index_of(&format!("app:{bundle_id}")).is_some()
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }

    // MARK: Hover

    pub fn set_item_hovered(&mut self, index: usize, hovered: bool, cx: &mut Context<Self>) {
        // While the dock slides in it passes under a pointer resting at the
        // screen edge; that isn't the user pointing at an item.
        if hovered && self.live && !self.frame().contains(self.platform.pointer()) {
            return;
        }
        if hovered {
            self.pointer_on_item = Some(index);
            self.open_card(index, cx);
        } else if self.pointer_on_item == Some(index) {
            self.pointer_on_item = None;
            self.schedule_close(cx);
        }
    }

    pub fn set_card_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        // A card typed into can report the pointer leaving while it hasn't
        // (key events in an X11 pop-up); believe the real pointer.
        if !hovered
            && self.live
            && self
                .card_frame
                .is_some_and(|frame| frame.contains(self.platform.pointer()))
        {
            return;
        }
        self.pointer_on_card = hovered;
        if hovered {
            self.close_card = None;
        } else {
            self.schedule_close(cx);
        }
    }

    fn open_card(&mut self, index: usize, cx: &mut Context<Self>) {
        self.close_card = None;
        if self.card != Some(index) {
            self.card = Some(index);
            cx.notify();
        }
    }

    fn schedule_close(&mut self, cx: &mut Context<Self>) {
        self.close_card = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CARD_CLOSE_DELAY).await;
            this.update(cx, |this, cx| this.close_now(cx)).ok();
        }));
    }

    pub fn close_now(&mut self, cx: &mut Context<Self>) {
        self.close_card = None;
        if self.pointer_on_item.is_none() && !self.pointer_on_card && self.card.take().is_some() {
            cx.notify();
        }
    }

    // MARK: Items

    pub fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        let app = match self.items.get(index).map(|item| &item.kind) {
            Some(ItemKind::App(app)) => app,
            Some(ItemKind::Plugin(manifest)) => {
                if manifest.clickable {
                    let id = manifest.id.clone();
                    self.send_plugin(&id, &HostMessage::Click);
                }
                return;
            }
            _ => return,
        };
        if let Err(err) = self.platform.open(&app.path) {
            eprintln!("sidedoor: couldn't open {}: {err}", app.name);
        }
        // Show the running dot without waiting for the next poll.
        if self.running.insert(app.bundle_id.clone()) {
            cx.notify();
        }
    }

    /// A plugin window closed, by its close button or the plugin's word.
    pub fn plugin_window_closed(&mut self, plugin: &str, key: &str) {
        let Some(state) = self.plugins.get_mut(plugin) else {
            return;
        };
        if state.open_windows.remove(key) {
            state.windows.remove(key);
            if let Some(link) = state.link.as_mut() {
                link.send(&HostMessage::Window {
                    key: key.to_string(),
                    open: false,
                });
            }
        }
    }

    /// Runs a command from a plugin item's context menu. `id` is the
    /// plugin's id, not the item's.
    pub fn run_plugin_action(&mut self, id: &str, key: &str) {
        self.send_plugin(id, &HostMessage::Action { key: key.into() });
    }

    fn send_plugin(&mut self, id: &str, message: &HostMessage) {
        if let Some(link) = self
            .plugins
            .get_mut(id)
            .and_then(|state| state.link.as_mut())
        {
            link.send(message);
        }
    }

    pub fn reveal_in_finder(&self, id: &str) {
        if let Some(DockItem {
            kind: ItemKind::App(app),
            ..
        }) = self.index_of(id).and_then(|index| self.items.get(index))
        {
            self.platform.reveal_in_finder(&app.path);
        }
    }

    pub fn open_item(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.index_of(id) {
            self.activate(index, cx);
        }
    }

    /// Adds dropped app bundles before slot `at` (or at the end). Returns
    /// how many were added; non-apps, duplicates and overflow are skipped.
    pub fn add_paths(
        &mut self,
        paths: &[PathBuf],
        at: Option<usize>,
        cx: &mut Context<Self>,
    ) -> usize {
        self.incoming = None;
        let mut at = at.unwrap_or(self.items.len()).min(self.items.len());
        let mut added = 0;
        for path in paths {
            if self.items.len() >= MAX_ITEMS {
                break;
            }
            let Some(app) = self.platform.app_at(path) else {
                continue;
            };
            let item = DockItem::app(app);
            if self.index_of(&item.id).is_some() {
                continue;
            }
            self.items.insert(at, item);
            at += 1;
            added += 1;
        }
        if added > 0 {
            self.items_changed(cx);
        } else {
            // Nothing to add, but a gap held open for the drop closes.
            cx.notify();
        }
        added
    }

    /// Moves the item at `from` so it lands in front of the item currently at
    /// `to` (or at the end when `to` is the item count).
    pub fn move_item(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from >= self.items.len() || to > self.items.len() || from == to || from + 1 == to {
            return;
        }
        let item = self.items.remove(from);
        let to = if to > from { to - 1 } else { to };
        self.items.insert(to, item);
        self.items_changed(cx);
    }

    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.index_of(id) {
            let item = self.items.remove(index);
            if let ItemKind::Plugin(manifest) = &item.kind {
                // Dropping its state stops the process.
                if let Some(state) = self.plugins.remove(&manifest.id) {
                    for key in state.open_windows {
                        cx.emit(DockEvent::ClosePluginWindow {
                            plugin: manifest.id.clone(),
                            key,
                        });
                    }
                }
            }
            if self.shortcuts.remove(id).is_some() {
                cx.emit(DockEvent::ShortcutsChanged);
            }
            self.items_changed(cx);
        }
    }

    // MARK: Plugins

    fn start_plugin(&mut self, manifest: &Manifest, cx: &mut Context<Self>) {
        let fingerprint = if self.live {
            plugin_host::fingerprint(&manifest.dir)
        } else {
            0
        };
        // A reload keeps showing what the plugin last drew until it draws again.
        let old = self.plugins.remove(&manifest.id);
        let (tile, card, windows, open_windows, logs, height) = match old {
            Some(old) => (
                old.tile,
                old.card,
                old.windows,
                old.open_windows,
                old.logs,
                old.height,
            ),
            None => Default::default(),
        };
        let settings = self.plugin_values(manifest);
        let state = match self.platform.start_plugin(manifest, &settings) {
            Ok(connection) => {
                let id = manifest.id.clone();
                let mut incoming = connection.incoming;
                let task = cx.spawn(async move |this, cx| {
                    while let Some(message) = incoming.next().await {
                        let alive = this
                            .update(cx, |this, cx| this.plugin_message(&id, message, cx))
                            .is_ok();
                        if !alive {
                            break;
                        }
                    }
                });
                let mut link = connection.link;
                // Windows still on screen from before a reload draw again.
                for key in &open_windows {
                    link.send(&HostMessage::Window {
                        key: key.clone(),
                        open: true,
                    });
                }
                PluginState {
                    tile,
                    card,
                    windows,
                    open_windows,
                    problem: None,
                    logs,
                    height,
                    link: Some(link),
                    fingerprint,
                    _task: Some(task),
                }
            }
            Err(problem) => PluginState {
                tile,
                card,
                windows,
                open_windows,
                problem: Some(problem.into()),
                logs,
                height,
                link: None,
                fingerprint,
                _task: None,
            },
        };
        self.plugins.insert(manifest.id.clone(), state);
        // A restarted worker needs the current card state even if the pointer
        // has not moved.
        let open = self
            .card()
            .is_some_and(|(_, item)| item.id.as_ref() == format!("plugin:{}", manifest.id));
        if open {
            self.send_plugin(&manifest.id, &HostMessage::Card { open });
        }
    }

    fn plugin_message(&mut self, id: &str, message: PluginMessage, cx: &mut Context<Self>) {
        let Some(state) = self.plugins.get_mut(id) else {
            return;
        };
        match message {
            PluginMessage::Manifest(described) => {
                // The running plugin's own word on its name, look and settings.
                for item in &mut self.items {
                    if let ItemKind::Plugin(manifest) = &mut item.kind
                        && manifest.id == id
                    {
                        manifest.update(described.clone());
                    }
                }
            }
            PluginMessage::Render { surface, tree } => {
                match surface.as_str() {
                    "tile" => state.tile = Some(tree),
                    "card" => state.card = Some(tree),
                    other => {
                        if let Some(key) = other.strip_prefix("window:") {
                            state.windows.insert(key.to_string(), tree);
                        }
                    }
                }
                state.problem = None;
            }
            PluginMessage::Patch { surface, patches } => {
                let tree = match surface.as_str() {
                    "tile" => state.tile.as_mut(),
                    "card" => state.card.as_mut(),
                    other => other
                        .strip_prefix("window:")
                        .and_then(|key| state.windows.get_mut(key)),
                };
                let applied = tree.is_some_and(|tree| apply_patches(tree, patches));
                if !applied {
                    // Out of step with the plugin: ask for everything again.
                    if let Some(link) = state.link.as_mut() {
                        link.send(&HostMessage::Resync);
                    }
                    return;
                }
                state.problem = None;
            }
            PluginMessage::Error { message } => {
                push_log(&mut state.logs, &message);
                state.problem = Some(
                    message
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .trim_start_matches("error: ")
                        .to_string()
                        .into(),
                );
            }
            PluginMessage::Log { line } => {
                push_log(&mut state.logs, &line);
            }
            PluginMessage::Exited => {
                state.link = None;
                if state.problem.is_none() {
                    state.problem = Some("The plugin stopped.".into());
                }
            }
            PluginMessage::OpenUrl { url } => {
                self.open_path(std::path::Path::new(&url));
                return;
            }
            PluginMessage::OpenPath { path } => {
                self.open_path(std::path::Path::new(&path));
                return;
            }
            PluginMessage::Copy { text } => {
                self.platform.copy_text(&text);
                return;
            }
            PluginMessage::OpenWindow { key } => {
                let declared = self.items.iter().any(|item| match &item.kind {
                    ItemKind::Plugin(manifest) => {
                        manifest.id == id && manifest.windows.iter().any(|w| w.key == key)
                    }
                    _ => false,
                });
                if !declared {
                    push_log(
                        &mut state.logs,
                        &format!("error: openWindow(\"{key}\"): no such window in definePlugin"),
                    );
                    cx.notify();
                    return;
                }
                if state.open_windows.insert(key.clone())
                    && let Some(link) = state.link.as_mut()
                {
                    link.send(&HostMessage::Window {
                        key: key.clone(),
                        open: true,
                    });
                }
                cx.emit(DockEvent::OpenPluginWindow {
                    plugin: id.to_string(),
                    key,
                });
                return;
            }
            PluginMessage::CloseWindow { key } => {
                self.plugin_window_closed(id, &key);
                cx.emit(DockEvent::ClosePluginWindow {
                    plugin: id.to_string(),
                    key,
                });
                return;
            }
            PluginMessage::Notify { title, body } => {
                let source = self
                    .items
                    .iter()
                    .find_map(|item| match &item.kind {
                        ItemKind::Plugin(manifest) if manifest.id == id => {
                            Some(manifest.name.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| id.to_string());
                self.platform.notify(&source, &title, &body);
                return;
            }
        }
        cx.notify();
    }

    /// Tells plugins when their card opens or closes.
    fn sync_open_plugin(&mut self) {
        let open = self.card().and_then(|(_, item)| match &item.kind {
            ItemKind::Plugin(manifest) => Some(manifest.id.clone()),
            _ => None,
        });
        if open == self.open_plugin {
            return;
        }
        let closed = std::mem::replace(&mut self.open_plugin, open.clone());
        for (id, open) in [(closed, false), (open, true)] {
            let link = id
                .and_then(|id| self.plugins.get_mut(&id))
                .and_then(|state| state.link.as_mut());
            if let Some(link) = link {
                link.send(&HostMessage::Card { open });
            }
        }
    }

    /// A plugin's settings: saved values over the manifest's defaults.
    pub fn plugin_values(&self, manifest: &Manifest) -> serde_json::Map<String, serde_json::Value> {
        manifest.settings_with(self.plugin_settings.get(&manifest.id))
    }

    pub fn set_plugin_setting(
        &mut self,
        manifest: &Manifest,
        key: &str,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let saved = self.plugin_settings.entry(manifest.id.clone()).or_default();
        if saved.get(key) == Some(&value) {
            return;
        }
        saved.insert(key.to_string(), value);
        self.save_config();
        let values = self.plugin_values(manifest);
        if let Some(link) = self
            .plugins
            .get_mut(&manifest.id)
            .and_then(|state| state.link.as_mut())
        {
            link.send(&HostMessage::Settings { values });
        }
        cx.notify();
    }

    /// Records how tall a fitted card's content drew, so the card can match.
    pub fn set_plugin_height(&mut self, id: &str, height: f64, cx: &mut Context<Self>) {
        let height = height.clamp(40.0, plugin_host::MAX_HEIGHT).round();
        if let Some(state) = self.plugins.get_mut(id)
            && state.height != Some(height)
        {
            state.height = Some(height);
            cx.notify();
        }
    }

    pub fn clear_plugin_logs(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(state) = self.plugins.get_mut(id) {
            state.logs.clear();
            cx.notify();
        }
    }

    /// Creates a plugin from the template, puts it in the dock and opens its
    /// source to edit.
    pub fn create_plugin(
        &mut self,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<Manifest, String> {
        let manifest = self.platform.create_plugin(name)?;
        self.rescan_plugins();
        if !self.add_plugin(manifest.clone(), cx) {
            return Err("The dock is full. Remove an item to add another.".into());
        }
        self.open_path(&manifest.dir.join(&manifest.main));
        Ok(manifest)
    }

    /// Fetches the current plugin gallery, once a session. Until it arrives,
    /// or if it can't, the built-in list shows.
    pub fn refresh_gallery(&mut self, cx: &mut Context<Self>) {
        if !self.live || self.gallery_task.is_some() {
            return;
        }
        self.gallery_task = Some(cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_executor()
                .spawn(async { services::gallery::fetch() })
                .await;
            match fetched {
                Ok(gallery) => {
                    this.update(cx, |this, cx| {
                        if this.gallery != gallery {
                            this.gallery = gallery;
                            cx.notify();
                        }
                    })
                    .ok();
                }
                Err(err) => eprintln!("sidedoor: couldn't fetch the plugin gallery: {err}"),
            }
        }));
    }

    /// Picks up plugins added to or removed from the plugins folder.
    fn rescan_plugins(&mut self) -> bool {
        let discovered = discover_plugins(self.platform.as_ref());
        let changed = discovered != self.discovered;
        self.discovered = discovered;
        changed
    }

    /// Reloads plugins whose files changed, as on save, and notices plugins
    /// added to or removed from the folder by hand.
    fn poll_plugins(&mut self, cx: &mut Context<Self>) {
        if self.rescan_plugins() {
            cx.notify();
        }
        let changed: Vec<Manifest> = self
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Plugin(manifest) => Some(manifest),
                _ => None,
            })
            .filter(|manifest| {
                self.plugins.get(&manifest.id).is_some_and(|state| {
                    state.fingerprint != plugin_host::fingerprint(&manifest.dir)
                })
            })
            .cloned()
            .collect();
        for manifest in changed {
            self.start_plugin(&manifest, cx);
            cx.notify();
        }
    }

    pub fn plugin(&self, id: &str) -> Option<&PluginState> {
        self.plugins.get(id)
    }

    /// Passes an event from a plugin's UI back to its handler.
    pub fn plugin_event(&mut self, id: &str, handler: &str, value: serde_json::Value) {
        if let Some(link) = self
            .plugins
            .get_mut(id)
            .and_then(|state| state.link.as_mut())
        {
            link.send(&HostMessage::Event {
                handler: handler.to_string(),
                value,
            });
        }
    }

    /// Installed plugins that aren't in the dock.
    pub fn available_plugins(&self) -> Vec<Manifest> {
        self.discovered
            .iter()
            .filter(|manifest| self.index_of(&format!("plugin:{}", manifest.id)).is_none())
            .cloned()
            .collect()
    }

    pub fn add_plugin(&mut self, manifest: Manifest, cx: &mut Context<Self>) -> bool {
        let item = DockItem::plugin(manifest.clone());
        if self.items.len() >= MAX_ITEMS || self.index_of(&item.id).is_some() {
            return false;
        }
        self.items.push(item);
        self.trusted.insert(manifest.id.clone());
        self.start_plugin(&manifest, cx);
        self.items_changed(cx);
        true
    }

    // MARK: Managing plugins

    /// Whether `id` can be added without asking; plugins run with the app's
    /// access, so a new one needs the user's go-ahead once.
    pub fn is_trusted(&self, id: &str) -> bool {
        self.trusted.contains(id)
    }

    /// Whether `manifest` was installed from one of the gallery's official
    /// plugins, made and kept up by the Sidedoor project.
    pub fn is_official(&self, manifest: &Manifest) -> bool {
        let Some(source) = &manifest.source else {
            return false;
        };
        self.gallery.iter().any(|entry| {
            entry.official
                && plugin_host::Link::parse(&entry.link)
                    .is_ok_and(|link| link.same_plugin(&source.link))
        })
    }

    pub fn in_dock(&self, id: &str) -> bool {
        self.index_of(&format!("plugin:{id}")).is_some()
    }

    /// The manifest of a plugin in the dock, as the running plugin described it.
    fn dock_manifest(&self, id: &str) -> Option<&Manifest> {
        self.items.iter().find_map(|item| match &item.kind {
            ItemKind::Plugin(manifest) if manifest.id == id => Some(manifest),
            _ => None,
        })
    }

    /// Every installed plugin, in the dock or not, by name.
    pub fn installed_plugins(&self) -> Vec<Manifest> {
        self.discovered
            .iter()
            .cloned()
            .map(|manifest| {
                self.dock_manifest(&manifest.id)
                    .cloned()
                    .unwrap_or(manifest)
            })
            .collect()
    }

    /// Downloads plugins, off the main thread.
    pub fn installer(&self) -> Arc<dyn Installer> {
        self.platform.plugin_installer()
    }

    pub fn set_install(&mut self, state: InstallState, cx: &mut Context<Self>) {
        self.install = state;
        cx.notify();
    }

    /// Installs a downloaded plugin the user agreed to, and adds it to the
    /// dock when there's room. Installing one that's already here updates it.
    pub fn finish_install(
        &mut self,
        staged: Staged,
        cx: &mut Context<Self>,
    ) -> Result<Manifest, String> {
        let manifest = self.platform.install_plugin(staged)?;
        self.rescan_plugins();
        self.trusted.insert(manifest.id.clone());
        self.install = InstallState::Idle;
        self.updates.remove(&manifest.id);
        if self.in_dock(&manifest.id) {
            self.replace_plugin(manifest.clone(), cx);
        } else {
            self.add_plugin(manifest.clone(), cx);
        }
        self.save_config();
        cx.notify();
        Ok(manifest)
    }

    /// Takes a plugin's new files and restarts it.
    fn replace_plugin(&mut self, manifest: Manifest, cx: &mut Context<Self>) {
        for item in &mut self.items {
            if let ItemKind::Plugin(old) = &mut item.kind
                && old.id == manifest.id
            {
                *old = manifest.clone();
            }
        }
        if self.plugins.contains_key(&manifest.id) {
            self.start_plugin(&manifest, cx);
        }
    }

    /// Checks an installed plugin's link for a newer version, and installs
    /// it if there is one.
    pub fn update_plugin(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(source) = self
            .installed_plugins()
            .into_iter()
            .find(|manifest| manifest.id == id)
            .and_then(|manifest| manifest.source)
        else {
            return;
        };
        if matches!(
            self.updates.get(id),
            Some(UpdateState::Checking | UpdateState::Updating)
        ) {
            return;
        }
        let installer = self.installer();
        let plugin = id.to_string();
        self.updates.insert(plugin.clone(), UpdateState::Checking);
        let task = cx.spawn(async move |this, cx| {
            let set = |state: UpdateState, cx: &mut gpui_kit::AsyncApp| {
                this.update(cx, |this, cx| {
                    this.updates.insert(plugin.clone(), state);
                    cx.notify();
                })
                .ok();
            };
            let (check, link) = (installer.clone(), source.link.clone());
            let latest = cx
                .background_executor()
                .spawn(async move { check.latest_commit(&link) })
                .await;
            match latest {
                Ok(commit) if commit == source.commit => return set(UpdateState::UpToDate, cx),
                Ok(_) => set(UpdateState::Updating, cx),
                Err(message) => return set(UpdateState::Failed(message.into()), cx),
            }
            let link = source.link.clone();
            let fetched = cx
                .background_executor()
                .spawn(async move { installer.fetch(&link) })
                .await;
            let installed = this.update(cx, |this, cx| {
                let manifest = this.platform.install_plugin(fetched?)?;
                this.rescan_plugins();
                if this.in_dock(&manifest.id) {
                    this.replace_plugin(manifest, cx);
                }
                Ok::<_, String>(())
            });
            match installed {
                Ok(Ok(())) => set(UpdateState::Updated, cx),
                Ok(Err(message)) => set(UpdateState::Failed(message.into()), cx),
                Err(_) => {}
            }
        });
        self.update_tasks.insert(id.to_string(), task);
        cx.notify();
    }

    /// Restarts a plugin in the dock, e.g. after it stopped.
    pub fn reload_plugin(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(manifest) = self.dock_manifest(id).cloned() {
            self.start_plugin(&manifest, cx);
            cx.notify();
        }
    }

    /// Takes a plugin out of the dock, moves its folder to the Trash and
    /// forgets its settings.
    pub fn delete_plugin(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let manifest = self
            .installed_plugins()
            .into_iter()
            .find(|manifest| manifest.id == id)
            .ok_or("The plugin isn't installed anymore.")?;
        self.remove(&format!("plugin:{id}"), cx);
        self.platform.delete_plugin(&manifest)?;
        self.rescan_plugins();
        self.plugin_settings.remove(id);
        self.trusted.remove(id);
        self.updates.remove(id);
        self.update_tasks.remove(id);
        self.save_config();
        cx.notify();
        Ok(())
    }

    // MARK: Settings

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// Moves the dock to another edge and shows it there for a moment.
    pub fn set_edge(&mut self, edge: Edge, cx: &mut Context<Self>) {
        if edge == self.edge {
            return;
        }
        self.edge = edge;
        self.reveal.show_for(cx.background_executor().now(), PEEK);
        self.items_changed(cx);
    }

    pub fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        if appearance == self.appearance {
            return;
        }
        self.appearance = appearance;
        self.platform.set_appearance(appearance);
        self.save_config();
        cx.notify();
    }

    pub fn login_item(&self) -> LoginItem {
        self.login_item
    }

    /// Asks the system again whether the app opens at login, for when
    /// Settings opens or comes forward: the user may have changed it in
    /// System Settings meanwhile.
    pub fn refresh_login_item(&mut self, cx: &mut Context<Self>) {
        let login_item = self.platform.login_item();
        if login_item != self.login_item {
            self.login_item = login_item;
            cx.notify();
        }
    }

    pub fn set_launch_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Err(err) = self.platform.set_launch_at_login(enabled) {
            eprintln!("sidedoor: couldn't change launch at login: {err}");
        }
        self.login_item = self.platform.login_item();
        cx.notify();
    }

    // MARK: Shortcuts

    pub fn shortcut_for(&self, id: &str) -> Option<&Shortcut> {
        self.shortcuts.get(id)
    }

    /// Every shortcut, in dock order.
    pub fn shortcuts(&self) -> Vec<(String, Shortcut)> {
        self.items
            .iter()
            .filter_map(|item| {
                let shortcut = self.shortcuts.get(item.id.as_ref())?;
                Some((item.id.to_string(), shortcut.clone()))
            })
            .collect()
    }

    /// The item already using `shortcut`, other than `except`.
    pub fn shortcut_owner(&self, shortcut: &Shortcut, except: &str) -> Option<String> {
        self.shortcuts
            .iter()
            .find(|(id, existing)| *existing == shortcut && id.as_str() != except)
            .map(|(id, _)| self.item_name(id))
    }

    /// What an item is called in menus and messages.
    pub fn item_name(&self, id: &str) -> String {
        match self.index_of(id).map(|index| &self.items[index].kind) {
            Some(ItemKind::App(app)) => app.name.clone(),
            Some(ItemKind::Plugin(manifest)) => manifest.name.clone(),
            None => id.to_string(),
        }
    }

    pub fn set_shortcut(&mut self, id: &str, shortcut: Option<Shortcut>, cx: &mut Context<Self>) {
        let changed = match shortcut {
            Some(shortcut) => {
                self.shortcuts.insert(id.to_string(), shortcut.clone()) != Some(shortcut)
            }
            None => self.shortcuts.remove(id).is_some(),
        };
        if changed {
            self.save_config();
            cx.emit(DockEvent::ShortcutsChanged);
            cx.notify();
        }
    }

    /// What pressing an item's shortcut does: apps open, plugins that take
    /// clicks get one, other plugins show their card for a moment.
    pub fn trigger_shortcut(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        match &self.items[index].kind {
            ItemKind::App(_) => self.activate(index, cx),
            // A plugin that handles clicks gets the shortcut as a click.
            ItemKind::Plugin(manifest) if manifest.clickable => self.activate(index, cx),
            ItemKind::Plugin(_) => self.peek(index, cx),
        }
    }

    fn peek(&mut self, index: usize, cx: &mut Context<Self>) {
        self.reveal.show_for(cx.background_executor().now(), PEEK);
        self.close_card = None;
        self.card = Some(index);
        cx.notify();
    }

    fn items_changed(&mut self, cx: &mut Context<Self>) {
        self.card = None;
        self.pointer_on_item = None;
        self.close_card = None;
        self.save_config();
        cx.notify();
    }

    fn save_config(&self) {
        let config = Config {
            items: self.items.iter().map(DockItem::config).collect(),
            edge: self.edge,
            appearance: self.appearance,
            weather: None,
            shortcuts: self
                .shortcuts
                .iter()
                .map(|(id, shortcut)| (id.clone(), shortcut.to_config()))
                .collect(),
            plugin_settings: self.plugin_settings.clone(),
            trusted_plugins: self.trusted.clone(),
            automatically_check_for_updates: self.automatic_update_checks,
        };
        if let Err(err) = self.platform.save_config(&config) {
            eprintln!("sidedoor: couldn't save the dock: {err}");
        }
    }

    pub fn set_automatic_update_checks(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.automatic_update_checks = enabled;
        self.app_updates.update(cx, |updates, cx| {
            updates.automatic = enabled;
            cx.notify();
        });
        self.save_config();
        cx.notify();
    }

    // MARK: Files

    pub fn reveal_path(&self, path: &std::path::Path) {
        self.platform.reveal_in_finder(path);
    }

    pub fn open_path(&self, path: &std::path::Path) {
        if let Err(err) = self.platform.open(path) {
            eprintln!("sidedoor: couldn't open {}: {err}", path.display());
        }
    }

    // MARK: Polling

    /// Records where the card's window is, as the native windows place it.
    pub fn set_card_frame(&mut self, frame: Option<Rect>) {
        self.card_frame = frame;
    }

    /// Holds the dock in place while a context menu is open. Only the drawn
    /// Linux menu needs it; system menus pause the dock's timers themselves.
    #[cfg(target_os = "linux")]
    pub fn set_menu_open(&mut self, open: bool) {
        self.menu_open = open;
    }

    pub(crate) fn poll_pointer(&mut self, cx: &mut Context<Self>) {
        if self.menu_open {
            return;
        }
        if let Some(screen) = self.platform.main_screen()
            && screen != self.screen
        {
            self.screen = screen;
            cx.notify();
        }
        // The windows report the pointer leaving an item or a card, but a
        // quick flick out of a panel can leave without that report, and a
        // card still open would then hold the dock up. The polled position
        // settles it: away from the dock and the card, nothing is hovered.
        let pointer = self.platform.pointer();
        if (self.pointer_on_item.is_some() || self.pointer_on_card)
            && !geometry::near_dock(pointer, self.screen, self.edge, self.frame())
            && !self
                .card_frame
                .is_some_and(|card| geometry::near_card(pointer, card))
        {
            self.pointer_on_item = None;
            self.pointer_on_card = false;
            if self.close_card.is_none() {
                self.schedule_close(cx);
            }
        }
        let keep = self
            .card
            .map(|_| geometry::card_zone(self.screen, self.frame(), self.edge));
        let changed = self.reveal.update(
            self.platform.pointer(),
            self.screen,
            self.edge,
            self.frame(),
            keep,
            cx.background_executor().now(),
        );
        if changed {
            if !self.reveal.is_shown() {
                self.card = None;
                self.pointer_on_item = None;
                self.pointer_on_card = false;
            }
            cx.notify();
        }
    }

    fn poll_system(&mut self, cx: &mut Context<Self>) {
        let running = self.platform.running_bundle_ids();
        let accessibility = self.platform.accessibility();
        let running_changed = self.items.iter().any(|item| match &item.kind {
            ItemKind::App(app) => {
                running.contains(&app.bundle_id) != self.running.contains(&app.bundle_id)
            }
            _ => false,
        });
        self.running = running;
        if running_changed || accessibility != self.accessibility {
            self.accessibility = accessibility;
            cx.notify();
        }
    }
}

/// How many log lines each plugin keeps.
const PLUGIN_LOG_LINES: usize = 200;

fn push_log(logs: &mut VecDeque<SharedString>, text: &str) {
    for line in text.lines() {
        if logs.len() == PLUGIN_LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(line.to_string().into());
    }
}

/// The plugins in the plugins folder.
fn discover_plugins(platform: &dyn Host) -> Vec<Manifest> {
    platform.plugins()
}

/// Runs `tick` on the dock every `interval` for as long as the dock exists.
fn every(
    interval: Duration,
    cx: &mut Context<Dock>,
    tick: fn(&mut Dock, &mut Context<Dock>),
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(interval).await;
            if this.update(cx, tick).is_err() {
                break;
            }
        }
    })
}

fn resolve_items(
    configs: &[ItemConfig],
    platform: &dyn Host,
    plugins: &[Manifest],
) -> Vec<DockItem> {
    let mut seen = HashSet::new();
    configs
        .iter()
        .filter_map(|config| {
            let item = match config {
                ItemConfig::App { bundle_id } => match platform.app_by_bundle_id(bundle_id) {
                    Some(app) => DockItem::app(app),
                    None => {
                        eprintln!("sidedoor: skipping {bundle_id}, it isn't installed");
                        return None;
                    }
                },
                ItemConfig::Weather | ItemConfig::Stats | ItemConfig::Clipboard => {
                    unreachable!("migrated to plugins above")
                }
                ItemConfig::Plugin { id } => {
                    match plugins.iter().find(|manifest| &manifest.id == id) {
                        Some(manifest) => DockItem::plugin(manifest.clone()),
                        None => {
                            eprintln!("sidedoor: skipping plugin {id}, it isn't installed");
                            return None;
                        }
                    }
                }
            };
            // Duplicates are skipped, as in Sidekick.
            seen.insert(item.id.clone()).then_some(item)
        })
        .take(MAX_ITEMS)
        .collect()
}
