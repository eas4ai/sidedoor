//! The dock model: its items, the live data behind them, and the edits the
//! user makes by dragging and right-clicking.

use crate::{
    clipboard::{self, ClipKind, History},
    config::{Appearance, Config, ItemConfig, MAX_ITEMS, WeatherLocation},
    geometry::{self, Edge, Rect, Reveal, Screen},
    platform::{Accessibility, AppInfo, Platform},
    stats::{Sampler, Snapshot},
    weather::{self, Weather},
};
use gpui_kit::{Context, EventEmitter, SharedString, Task};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

const POINTER_INTERVAL: Duration = Duration::from_millis(40);
const SYSTEM_INTERVAL: Duration = Duration::from_secs(2);
pub const STATS_INTERVAL: Duration = Duration::from_secs(2);
const PASTEBOARD_INTERVAL: Duration = Duration::from_millis(500);
const WEATHER_INTERVAL: Duration = Duration::from_secs(20 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(60);
/// How long "Clear History" waits for its confirming second click.
const CLEAR_CONFIRM_WINDOW: Duration = Duration::from_secs(3);
/// Grace period before a card closes, so the pointer can travel onto it.
const CARD_CLOSE_DELAY: Duration = Duration::from_millis(160);

pub struct DockItem {
    /// Stable identity for element IDs, derived from the item itself.
    pub id: SharedString,
    pub kind: ItemKind,
}

pub enum ItemKind {
    App(AppInfo),
    Weather,
    Stats,
    Clipboard,
}

impl DockItem {
    fn app(app: AppInfo) -> Self {
        Self {
            id: format!("app:{}", app.bundle_id).into(),
            kind: ItemKind::App(app),
        }
    }

    fn widget(kind: ItemKind) -> Self {
        let id = match kind {
            ItemKind::Weather => "weather",
            ItemKind::Stats => "stats",
            ItemKind::Clipboard => "clipboard",
            ItemKind::App(_) => unreachable!("apps use DockItem::app"),
        };
        Self {
            id: id.into(),
            kind,
        }
    }

    fn config(&self) -> ItemConfig {
        match &self.kind {
            ItemKind::App(app) => ItemConfig::App {
                bundle_id: app.bundle_id.clone(),
            },
            ItemKind::Weather => ItemConfig::Weather,
            ItemKind::Stats => ItemConfig::Stats,
            ItemKind::Clipboard => ItemConfig::Clipboard,
        }
    }
}

/// Requests the dock makes of the app around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEvent {
    OpenClipboardHistory,
}

impl EventEmitter<DockEvent> for Dock {}

pub enum WeatherState {
    Loading,
    Ready { weather: Weather, updated: Instant },
    Failed(SharedString),
}

/// Which background work a dock runs. Tests turn it all off.
#[derive(Clone, Copy)]
pub struct Services {
    pub live: bool,
}

pub struct Dock {
    platform: Rc<dyn Platform>,
    pub items: Vec<DockItem>,
    pub edge: Edge,
    appearance: Appearance,
    pub location: WeatherLocation,
    pub running: HashSet<String>,
    pub accessibility: Accessibility,
    pub stats: Option<Snapshot>,
    pub cpu_history: VecDeque<f32>,
    pub weather: WeatherState,
    pub history: History,
    /// The item whose card is open.
    card: Option<usize>,
    pointer_on_item: Option<usize>,
    pointer_on_card: bool,
    close_card: Option<Task<()>>,
    /// Set after a first "Clear History" click; cleared when it expires.
    clear_armed: Option<Task<()>>,
    screen: Screen,
    reveal: Reveal,
    pasteboard_count: isize,
    sampler: Option<Sampler>,
    /// Whether this dock follows the real pointer (off in tests, which drive
    /// the views directly).
    live: bool,
    _tasks: Vec<Task<()>>,
}

impl Dock {
    pub fn new(
        config: Config,
        platform: Rc<dyn Platform>,
        screen: Screen,
        history: History,
        services: Services,
        cx: &mut Context<Self>,
    ) -> Self {
        let items = resolve_items(&config.items, platform.as_ref());
        let mut tasks = Vec::new();
        if services.live {
            tasks.push(every(POINTER_INTERVAL, cx, Self::poll_pointer));
            tasks.push(every(SYSTEM_INTERVAL, cx, Self::poll_system));
            tasks.push(every(STATS_INTERVAL, cx, Self::poll_stats));
            tasks.push(every(PASTEBOARD_INTERVAL, cx, Self::poll_pasteboard));
            tasks.push(Self::weather_task(config.weather.clone(), cx));
        }
        Self {
            running: platform.running_bundle_ids(),
            accessibility: platform.accessibility(),
            pasteboard_count: platform.pasteboard_change_count(),
            sampler: services.live.then(Sampler::new),
            platform,
            items,
            edge: config.edge,
            appearance: config.appearance,
            location: config.weather,
            stats: None,
            cpu_history: VecDeque::new(),
            weather: WeatherState::Loading,
            history,
            card: None,
            pointer_on_item: None,
            pointer_on_card: false,
            close_card: None,
            clear_armed: None,
            screen,
            reveal: Reveal::default(),
            live: services.live,
            _tasks: tasks,
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// Frame of the dock while shown.
    pub fn frame(&self) -> Rect {
        geometry::dock_frame(self.screen, self.edge, self.items.len())
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
            Some(ItemKind::Clipboard) => {
                self.show_clipboard_history(cx);
                return;
            }
            _ => return,
        };
        if let Err(err) = self.platform.open(&app.path) {
            eprintln!("sidekick: couldn't open {}: {err}", app.name);
        }
        // Show the running dot without waiting for the next poll.
        if self.running.insert(app.bundle_id.clone()) {
            cx.notify();
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
            self.items.remove(index);
            self.items_changed(cx);
        }
    }

    fn items_changed(&mut self, cx: &mut Context<Self>) {
        self.card = None;
        self.pointer_on_item = None;
        self.close_card = None;
        let config = Config {
            items: self.items.iter().map(DockItem::config).collect(),
            edge: self.edge,
            appearance: self.appearance,
            weather: self.location.clone(),
        };
        if let Err(err) = self.platform.save_config(&config) {
            eprintln!("sidekick: couldn't save the dock: {err}");
        }
        cx.notify();
    }

    // MARK: Clipboard

    pub fn show_clipboard_history(&mut self, cx: &mut Context<Self>) {
        self.card = None;
        self.pointer_on_item = None;
        self.pointer_on_card = false;
        cx.emit(DockEvent::OpenClipboardHistory);
        cx.notify();
    }

    pub fn delete_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(image) = self.history.remove(id) {
            if let Some(file) = image {
                std::fs::remove_file(file).ok();
            }
            self.save_history();
            cx.notify();
        }
    }

    pub fn utc_offset(&self) -> i64 {
        self.platform.utc_offset()
    }

    pub fn reveal_path(&self, path: &std::path::Path) {
        self.platform.reveal_in_finder(path);
    }

    pub fn open_path(&self, path: &std::path::Path) {
        if let Err(err) = self.platform.open(path) {
            eprintln!("sidekick: couldn't open {}: {err}", path.display());
        }
    }

    /// Puts a history entry back on the pasteboard and moves it to the top.
    pub fn copy_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(entry) = self.history.promote(id, clipboard::now_secs()) else {
            return;
        };
        self.platform.write_pasteboard(&entry.kind);
        // Our own write shouldn't come back as a new copy.
        self.pasteboard_count = self.platform.pasteboard_change_count();
        self.save_history();
        cx.notify();
    }

    /// Clears the history on the second request within a few seconds, so a
    /// stray click can't wipe it.
    pub fn request_clear_history(&mut self, cx: &mut Context<Self>) {
        if self.clear_armed.is_some() {
            self.clear_armed = None;
            self.clear_history(cx);
            return;
        }
        self.clear_armed = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CLEAR_CONFIRM_WINDOW).await;
            this.update(cx, |this, cx| {
                this.clear_armed = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn is_clear_armed(&self) -> bool {
        self.clear_armed.is_some()
    }

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        for file in self.history.clear() {
            std::fs::remove_file(file).ok();
        }
        self.save_history();
        cx.notify();
    }

    fn record(&mut self, kind: ClipKind, source: Option<String>) {
        for file in self.history.push(kind, source, clipboard::now_secs()) {
            std::fs::remove_file(file).ok();
        }
        self.save_history();
    }

    fn save_history(&self) {
        if let Err(err) = self.platform.save_history(&self.history) {
            eprintln!("sidekick: couldn't save clipboard history: {err}");
        }
    }

    fn has(&self, predicate: impl Fn(&ItemKind) -> bool) -> bool {
        self.items.iter().any(|item| predicate(&item.kind))
    }

    // MARK: Polling

    pub(crate) fn poll_pointer(&mut self, cx: &mut Context<Self>) {
        if let Some(screen) = self.platform.main_screen()
            && screen != self.screen
        {
            self.screen = screen;
            cx.notify();
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
            Instant::now(),
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

    fn poll_stats(&mut self, cx: &mut Context<Self>) {
        if !self.has(|kind| matches!(kind, ItemKind::Stats)) {
            return;
        }
        if let Some(sampler) = &mut self.sampler {
            self.stats = Some(sampler.sample());
            self.cpu_history = sampler.history.clone();
            cx.notify();
        }
    }

    pub fn poll_pasteboard(&mut self, cx: &mut Context<Self>) {
        if !self.has(|kind| matches!(kind, ItemKind::Clipboard)) {
            return;
        }
        let count = self.platform.pasteboard_change_count();
        if count == self.pasteboard_count {
            return;
        }
        self.pasteboard_count = count;
        if let Some(copied) = self.platform.read_pasteboard(&History::image_dir()) {
            self.record(copied.kind, copied.source);
            cx.notify();
        }
    }

    fn weather_task(location: WeatherLocation, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                let wanted = this
                    .update(cx, |this, _| {
                        this.has(|kind| matches!(kind, ItemKind::Weather))
                    })
                    .unwrap_or(false);
                let delay = if wanted {
                    let request = location.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move { weather::fetch(&request) })
                        .await;
                    let delay = if result.is_ok() {
                        WEATHER_INTERVAL
                    } else {
                        WEATHER_RETRY
                    };
                    let updated = this.update(cx, |this, cx| {
                        this.weather = match result {
                            Ok(weather) => WeatherState::Ready {
                                weather,
                                updated: Instant::now(),
                            },
                            // Keep the last good forecast through a failed refresh.
                            Err(_) if matches!(this.weather, WeatherState::Ready { .. }) => return,
                            Err(message) => WeatherState::Failed(message.into()),
                        };
                        cx.notify();
                    });
                    if updated.is_err() {
                        break;
                    }
                    delay
                } else {
                    WEATHER_RETRY
                };
                cx.background_executor().timer(delay).await;
            }
        })
    }
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

fn resolve_items(configs: &[ItemConfig], platform: &dyn Platform) -> Vec<DockItem> {
    let mut seen = HashSet::new();
    configs
        .iter()
        .filter_map(|config| {
            let item = match config {
                ItemConfig::App { bundle_id } => match platform.app_by_bundle_id(bundle_id) {
                    Some(app) => DockItem::app(app),
                    None => {
                        eprintln!("sidekick: skipping {bundle_id}, it isn't installed");
                        return None;
                    }
                },
                ItemConfig::Weather => DockItem::widget(ItemKind::Weather),
                ItemConfig::Stats => DockItem::widget(ItemKind::Stats),
                ItemConfig::Clipboard => DockItem::widget(ItemKind::Clipboard),
            };
            // Duplicates are skipped, as in Sidekick.
            seen.insert(item.id.clone()).then_some(item)
        })
        .take(MAX_ITEMS)
        .collect()
}
