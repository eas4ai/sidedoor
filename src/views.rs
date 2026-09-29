//! The dock and its hover card, both read from the shared [`Dock`].

use crate::{
    clipboard::{self, ClipEntry, ClipKind},
    dock::{Dock, DockItem, ItemKind, STATS_INTERVAL, WeatherState},
    geometry::{self as geometry, CardPlacement, DOCK_PADDING, DOCK_RADIUS, Edge, PathStep, SLOT},
    motion,
    platform::AppInfo,
    stats::{Snapshot, format_bytes, format_memory},
    style::{Palette, text},
    weather::{Condition, Weather},
};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, Animation, AnimationExt as _, AnyElement, App, AppContext as _, Bounds, Context, Div,
    Entity, ExternalPaths, FontWeight, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement as _, PathBuilder,
    Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Subscription, TestSupportExt as _, Window,
    assets::IconName,
    base::{Easing, Spring, Transition, spring, transition},
    canvas, div, img, point, px, relative, svg,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const APP_ICON: f32 = 44.0;
const TILE: f32 = 36.0;
const TOOLTIP_HEIGHT: f64 = 28.0;
const TOOLTIP_PADDING: f64 = 24.0;
/// Space between a tooltip's name and its shortcut.
const TOOLTIP_HINT_GAP: f64 = 8.0;
const CARD_WIDTH: f64 = 300.0;
const WEATHER_HEIGHT: f64 = 190.0;
const STATS_HEIGHT: f64 = 206.0;
/// Clipboard entries shown on the card.
pub const CLIPBOARD_ROWS: usize = 5;
const CLIP_ROW: f64 = 46.0;

// MARK: Actions

/// Dispatched by the dock's context menu.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidekick, no_json)]
pub struct OpenItem {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidekick, no_json)]
pub struct RevealItem {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidekick, no_json)]
pub struct RemoveItem {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidekick, no_json)]
pub struct AssignShortcut {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidekick, no_json)]
pub struct RemoveShortcut {
    pub id: SharedString,
}

#[derive(Clone, Default, PartialEq, Action)]
#[action(namespace = sidekick, no_json)]
pub struct OpenSettings;

/// Opens the settings file in the user's text editor.
#[derive(Clone, Default, PartialEq, Action)]
#[action(namespace = sidekick, no_json)]
pub struct OpenConfigFile;

/// Size of the card or tooltip for `item`, arrow excluded. `text_width`
/// measures a tooltip label in points.
pub fn card_size(item: &DockItem, dock: &Dock, text_width: impl Fn(&str) -> f64) -> (f64, f64) {
    match &item.kind {
        ItemKind::App(app) => {
            let hint = dock.shortcut_for(&item.id).map_or(0.0, |shortcut| {
                TOOLTIP_HINT_GAP + text_width(&shortcut.to_string())
            });
            (
                text_width(&app.name) + hint + TOOLTIP_PADDING,
                TOOLTIP_HEIGHT,
            )
        }
        ItemKind::Plugin(manifest) => (manifest.width, manifest.height),
        ItemKind::Weather => (CARD_WIDTH, WEATHER_HEIGHT),
        ItemKind::Stats => (CARD_WIDTH, STATS_HEIGHT),
        ItemKind::Clipboard => {
            let rows = dock.history.len().min(CLIPBOARD_ROWS);
            let height = if rows == 0 {
                112.0
            } else {
                92.0 + rows as f64 * CLIP_ROW
            };
            (CARD_WIDTH, height)
        }
    }
}

fn condition_icon(condition: Condition, is_day: bool) -> SharedString {
    let name = match (condition, is_day) {
        (Condition::Clear, true) => IconName::Sun,
        (Condition::Clear, false) => IconName::Moon,
        (Condition::PartlyCloudy, true) => IconName::CloudSun,
        (Condition::PartlyCloudy, false) => IconName::CloudMoon,
        (Condition::Cloudy, _) => IconName::Cloud,
        (Condition::Fog, _) => IconName::CloudFog,
        (Condition::Drizzle, _) => IconName::CloudDrizzle,
        (Condition::Rain, _) => IconName::CloudRain,
        (Condition::Snow, _) => IconName::CloudSnow,
        (Condition::Thunderstorm, _) => IconName::CloudLightning,
    };
    name.path()
}

pub(crate) fn icon(path: SharedString, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(path)
        .size(px(size))
        .flex_shrink_0()
        .text_color(color)
}

fn degrees(value: f64) -> String {
    format!("{:.0}°", value.round())
}

fn percent(value: f32) -> String {
    format!("{:.0}%", value.round())
}

fn single_line(element: Div) -> Div {
    element
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
}

// MARK: Dock

/// What is carried while reordering the dock.
#[derive(Clone)]
struct DraggedSlot {
    index: usize,
    icon: Option<PathBuf>,
    glyph: SharedString,
}

struct DragPreview(DraggedSlot);

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let artwork = match &self.0.icon {
            Some(path) => img(path.clone()).size(px(APP_ICON)).into_any_element(),
            None => div()
                .size(px(TILE))
                .rounded(px(9.0))
                .bg(gpui_kit::hsla(0.0, 0.0, 0.5, 0.35))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(self.0.glyph.clone(), 18.0, gpui_kit::white()))
                .into_any_element(),
        };
        div().opacity(0.85).child(artwork)
    }
}

/// Per-item motion sampled each frame.
#[derive(Clone, Copy)]
struct SlotMotion {
    /// Magnification × press, around 1.0.
    scale: f32,
    /// 0 → 1 as the item arrives with the dock.
    arrival: f32,
    /// Running-dot opacity.
    dot: f32,
}

pub struct DockView {
    dock: Entity<Dock>,
    /// The pointer's position along the dock, while it is over it.
    pointer: Option<f32>,
    pressed: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl DockView {
    pub fn new(dock: Entity<Dock>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        Self {
            dock,
            pointer: None,
            pressed: None,
            _subscriptions: subscriptions,
        }
    }

    /// Samples every item's springs and transitions for this frame.
    fn sample_motion(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<SlotMotion> {
        let (items, shown): (Vec<(SharedString, bool)>, bool) = {
            let dock = self.dock.read(cx);
            let items = dock
                .items
                .iter()
                .map(|item| {
                    let running = match &item.kind {
                        ItemKind::App(app) => dock.is_running(app),
                        _ => false,
                    };
                    (item.id.clone(), running)
                })
                .collect();
            (items, dock.is_shown())
        };
        let dragging = cx.has_active_drag();
        let count = items.len();

        items
            .iter()
            .enumerate()
            .map(|(index, (id, running))| {
                let center = (DOCK_PADDING + SLOT * index as f64 + SLOT / 2.0) as f32;
                let pointer = self.pointer.filter(|_| !dragging);
                let mut target = motion::magnification(pointer, center, SLOT as f32);
                if self.pressed == Some(index) {
                    target *= motion::PRESSED;
                }
                let scale = spring(
                    SharedString::from(format!("scale:{id}")),
                    target,
                    Spring::new(Duration::from_millis(240)).with_damping(0.82),
                    window,
                    cx,
                );

                // Arrive one after another from the edge; leave together.
                let arrive = if shown {
                    Transition::new(motion::ICON_IN.duration)
                        .delay(motion::ICON_STAGGER * index as u32)
                        .ease(|t| motion::sample(motion::ICON_IN.curve, t))
                } else {
                    Transition::new(motion::DOCK_OUT.duration)
                        .delay(motion::ICON_STAGGER * (count.saturating_sub(index + 1)) as u32 / 3)
                };
                let arrival = transition(
                    SharedString::from(format!("arrive:{id}")),
                    if shown { 1.0_f32 } else { 0.0 },
                    arrive,
                    window,
                    cx,
                );
                let dot = transition(
                    SharedString::from(format!("dot:{id}")),
                    if *running { 1.0_f32 } else { 0.0 },
                    Transition::new(Duration::from_millis(260)).easing(Easing::EaseInOut),
                    window,
                    cx,
                );
                SlotMotion {
                    scale,
                    arrival,
                    dot,
                }
            })
            .collect()
    }
}

impl Render for DockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motions = self.sample_motion(window, cx);
        let (cpu, ram) = {
            let stats = self.dock.read(cx).stats.clone();
            (
                stats.as_ref().map(|s| s.cpu),
                stats.as_ref().map(Snapshot::memory_percent),
            )
        };
        let cpu = cpu.map(|value| transition("tile:cpu", value, number_tween(), window, cx));
        let ram = ram.map(|value| transition("tile:ram", value, number_tween(), window, cx));

        let view = cx.entity();
        let dock = self.dock.read(cx);
        let palette = Palette::new(window, dock.accessibility);
        let edge = dock.edge;
        let vertical = edge.is_vertical();
        let count = dock.items.len();
        let slots: Vec<AnyElement> = dock
            .items
            .iter()
            .zip(motions)
            .enumerate()
            .map(|(index, (item, motion))| {
                let content = match &item.kind {
                    ItemKind::App(app) => app_tile(app, motion, palette, vertical),
                    ItemKind::Weather => weather_tile(&dock.weather, motion.scale, palette),
                    ItemKind::Stats => stats_tile(cpu, ram, motion.scale, palette),
                    ItemKind::Clipboard => {
                        clipboard_tile(dock.history.len(), motion.scale, palette)
                    }
                    ItemKind::Plugin(manifest) => {
                        plugin_tile(&self.dock, dock, manifest, motion.scale, palette)
                    }
                };
                let shortcut = dock.shortcut_for(&item.id).map(ToString::to_string);
                slot(
                    &self.dock, &view, index, item, content, motion, edge, shortcut, palette,
                )
            })
            .collect();

        let append_paths = self.dock.clone();
        let append_slot = self.dock.clone();
        let container = div()
            .id("dock")
            .test_support()
            .size_full()
            .flex()
            .rounded(px(DOCK_RADIUS as f32))
            .border_1()
            .border_color(palette.stroke)
            .bg(palette.surface)
            .text_color(palette.label)
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let along = if vertical {
                    event.position.y
                } else {
                    event.position.x
                };
                this.pointer = Some(f32::from(along));
                cx.notify();
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.pointer = None;
                    this.pressed = None;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.pressed = None;
                    cx.notify();
                }),
            )
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(palette.blue))
            .on_drop(move |paths: &ExternalPaths, _, cx: &mut App| {
                append_paths.update(cx, |dock, cx| dock.add_paths(paths.paths(), None, cx));
            })
            .on_drop(move |dragged: &DraggedSlot, _, cx: &mut App| {
                append_slot.update(cx, |dock, cx| dock.move_item(dragged.index, count, cx));
            })
            .children(slots);
        if vertical {
            container.flex_col().py(px(DOCK_PADDING as f32))
        } else {
            container.flex_row().px(px(DOCK_PADDING as f32))
        }
    }
}

/// Cubic ease-out for fades.
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Numbers ease toward new readings instead of jumping.
fn number_tween() -> Transition {
    Transition::new(Duration::from_millis(700)).easing(Easing::EaseOut)
}

#[allow(clippy::too_many_arguments)]
fn slot(
    dock_entity: &Entity<Dock>,
    view: &Entity<DockView>,
    index: usize,
    item: &DockItem,
    content: AnyElement,
    motion: SlotMotion,
    edge: Edge,
    shortcut: Option<String>,
    palette: Palette,
) -> AnyElement {
    let dragged = DraggedSlot {
        index,
        icon: match &item.kind {
            ItemKind::App(app) => app.icon.clone(),
            _ => None,
        },
        glyph: widget_glyph(&item.kind),
    };
    let is_app = matches!(item.kind, ItemKind::App(_));
    let id = item.id.clone();

    let hover = dock_entity.clone();
    let click = dock_entity.clone();
    let drop_slot = dock_entity.clone();
    let drop_paths = dock_entity.clone();
    let press = view.clone();

    // Items travel in from the screen edge and fade up as they arrive.
    let travel = (1.0 - motion.arrival) * motion::ICON_TRAVEL;
    let arriving = div()
        .relative()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .opacity(motion.arrival.clamp(0.0, 1.0));
    let arriving = match edge {
        Edge::Right => arriving.left(px(travel)),
        Edge::Left => arriving.left(px(-travel)),
        Edge::Bottom => arriving.top(px(travel)),
    };

    let element = div()
        .id(item.id.clone())
        .test_support()
        .relative()
        .flex_shrink_0()
        .rounded(px(12.0))
        .on_hover(move |hovered: &bool, _, cx: &mut App| {
            if cx.has_active_drag() {
                return;
            }
            hover.update(cx, |dock, cx| dock.set_item_hovered(index, *hovered, cx));
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx: &mut App| {
            press.update(cx, |this, cx| {
                this.pressed = Some(index);
                cx.notify();
            });
        })
        .on_click(move |_, _, cx: &mut App| {
            click.update(cx, |dock, cx| dock.activate(index, cx));
        })
        .on_mouse_down(
            MouseButton::Right,
            move |event: &MouseDownEvent, window, cx| {
                let mut menu = NativeMenu::new();
                if is_app {
                    menu = menu
                        .menu("Open", Box::new(OpenItem { id: id.clone() }))
                        .menu("Show in Finder", Box::new(RevealItem { id: id.clone() }))
                        .separator();
                }
                menu = match &shortcut {
                    Some(keys) => menu
                        .menu(
                            format!("Change Shortcut ({keys})…"),
                            Box::new(AssignShortcut { id: id.clone() }),
                        )
                        .menu(
                            "Remove Shortcut",
                            Box::new(RemoveShortcut { id: id.clone() }),
                        ),
                    None => menu.menu(
                        "Assign Shortcut…",
                        Box::new(AssignShortcut { id: id.clone() }),
                    ),
                };
                menu.separator()
                    .menu("Remove from Dock", Box::new(RemoveItem { id: id.clone() }))
                    .separator()
                    .menu("Dock Settings…", Box::new(OpenSettings))
                    .show(event.position, window, cx);
                cx.stop_propagation();
            },
        )
        .on_drag(dragged, |dragged, _, _, cx| {
            cx.new(|_| DragPreview(dragged.clone()))
        })
        .drag_over::<DraggedSlot>(move |style, _, _, _| style.bg(palette.accent_fill))
        .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(palette.accent_fill))
        .on_drop(move |dragged: &DraggedSlot, _, cx: &mut App| {
            // Dropping on an item takes its place: after it when moving
            // forward, before it when moving back.
            let to = if dragged.index < index {
                index + 1
            } else {
                index
            };
            drop_slot.update(cx, |dock, cx| dock.move_item(dragged.index, to, cx));
            cx.stop_propagation();
        })
        .on_drop(move |paths: &ExternalPaths, _, cx: &mut App| {
            drop_paths.update(cx, |dock, cx| {
                dock.add_paths(paths.paths(), Some(index), cx)
            });
            cx.stop_propagation();
        })
        .child(arriving.child(content));

    let element = if edge.is_vertical() {
        element.w_full().h(px(SLOT as f32))
    } else {
        element.h_full().w(px(SLOT as f32))
    };
    element.into_any_element()
}

pub fn widget_glyph(kind: &ItemKind) -> SharedString {
    match kind {
        ItemKind::Weather => IconName::Cloud.path(),
        ItemKind::Stats => IconName::Cpu.path(),
        ItemKind::Clipboard => IconName::Clipboard.path(),
        ItemKind::App(_) => IconName::AppWindow.path(),
        ItemKind::Plugin(manifest) => manifest.icon_path().into(),
    }
}

/// A plugin's own tile, or its icon until it draws one.
fn plugin_tile(
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    manifest: &crate::plugin::Manifest,
    scale: f32,
    palette: Palette,
) -> AnyElement {
    let surface = crate::plugin_ui::Surface {
        dock: dock_entity.clone(),
        plugin: manifest.id.clone().into(),
        palette,
    };
    match dock
        .plugin(&manifest.id)
        .and_then(|state| state.tile.as_ref())
    {
        Some(tile) => div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .children(surface.render(tile, "tile"))
            .into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(icon(
                manifest.icon_path().into(),
                20.0 * scale,
                palette.label,
            ))
            .into_any_element(),
    }
}

/// A plugin's card: what it drew, or why it can't draw yet.
fn plugin_card(
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    manifest: &crate::plugin::Manifest,
    palette: Palette,
) -> AnyElement {
    let state = dock.plugin(&manifest.id);
    if let Some(problem) = state.and_then(|state| state.problem.as_ref()) {
        return message_card(&manifest.name, problem, palette);
    }
    let Some(card) = state.and_then(|state| state.card.as_ref()) else {
        return message_card(&manifest.name, "Starting…", palette);
    };
    let surface = crate::plugin_ui::Surface {
        dock: dock_entity.clone(),
        plugin: manifest.id.clone().into(),
        palette,
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .children(surface.render(card, "card"))
        .into_any_element()
}

fn app_tile(app: &AppInfo, motion: SlotMotion, palette: Palette, vertical: bool) -> AnyElement {
    let size = APP_ICON * motion.scale;
    let artwork = match &app.icon {
        Some(path) => img(path.clone()).size(px(size)).into_any_element(),
        None => div()
            .size(px(TILE * motion.scale))
            .rounded(px(9.0 * motion.scale))
            .bg(palette.fill)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(text::TITLE3 * motion.scale))
            .font_weight(FontWeight::SEMIBOLD)
            .child(app.name.chars().next().unwrap_or('?').to_string())
            .into_any_element(),
    };
    let dot = div()
        .absolute()
        .size(px(4.0))
        .rounded_full()
        .bg(palette.secondary)
        .opacity(motion.dot);
    // The running dot sits on the dock's inner edge, like the macOS Dock.
    let dot = if vertical {
        dot.right(px(3.0)).top(px(SLOT as f32 / 2.0 - 2.0))
    } else {
        dot.bottom(px(3.0)).left(px(SLOT as f32 / 2.0 - 2.0))
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(artwork)
        .children((motion.dot > 0.0).then_some(dot))
        .into_any_element()
}

fn weather_tile(state: &WeatherState, scale: f32, palette: Palette) -> AnyElement {
    let (glyph, reading) = match state {
        WeatherState::Ready { weather, .. } => (
            condition_icon(weather.condition, weather.is_day),
            degrees(weather.temperature),
        ),
        WeatherState::Loading => (IconName::Cloud.path(), "--°".to_string()),
        WeatherState::Failed(_) => (IconName::CloudOff.path(), "--°".to_string()),
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(3.0 * scale))
        .child(icon(glyph, 17.0 * scale, palette.label))
        .child(
            div()
                .text_size(px(text::CALLOUT * scale))
                .font_weight(FontWeight::SEMIBOLD)
                .child(reading),
        )
        .into_any_element()
}

fn stats_tile(cpu: Option<f32>, ram: Option<f32>, scale: f32, palette: Palette) -> AnyElement {
    let metric = |label: &'static str, value: Option<f32>| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .line_height(relative(1.05))
            .child(
                div()
                    .text_size(px(text::MICRO * scale))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.secondary)
                    .child(label),
            )
            .child(
                div()
                    .text_size(px(text::SUBHEADLINE * scale))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value.map_or_else(|| "--".into(), percent)),
            )
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.0 * scale))
        .child(metric("CPU", cpu))
        .child(metric("RAM", ram))
        .into_any_element()
}

fn clipboard_tile(count: usize, scale: f32, palette: Palette) -> AnyElement {
    let badge = div()
        .absolute()
        .right(px(-5.0))
        .bottom(px(-4.0))
        .min_w(px(16.0))
        .h(px(16.0))
        .px(px(4.0))
        .rounded_full()
        .bg(gpui_kit::hsla(0.0, 0.0, 0.1, 0.9))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(9.0))
        .font_weight(FontWeight::BOLD)
        .text_color(gpui_kit::white())
        .child(if count > 99 {
            "99+".into()
        } else {
            count.to_string()
        });
    // A new copy makes the badge pop.
    let badge = badge.with_animation(
        SharedString::from(format!("badge:{count}")),
        Animation::new(Duration::from_millis(420)),
        |badge, t| {
            let pop = 1.0 + 0.45 * (1.0 - motion::sample(motion::ICON_IN.curve, t));
            badge
                .min_w(px(16.0 * pop))
                .h(px(16.0 * pop))
                .text_size(px(9.0 * pop))
        },
    );
    div()
        .relative()
        .size(px(TILE * scale))
        .rounded(px(9.0 * scale))
        .bg(gpui_kit::linear_gradient(
            180.0,
            gpui_kit::linear_color_stop(palette.purple, 0.0),
            gpui_kit::linear_color_stop(palette.purple_deep, 1.0),
        ))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(
            IconName::Clipboard.path(),
            19.0 * scale,
            gpui_kit::white(),
        ))
        .children((count > 0).then_some(badge))
        .into_any_element()
}

// MARK: Card

/// Where the card window currently sits, shared by the native side (which
/// places it) and [`CardView`] (which draws inside it).
#[derive(Default)]
pub struct CardChrome {
    pub placement: Option<CardPlacement>,
}

pub struct CardView {
    dock: Entity<Dock>,
    chrome: Entity<CardChrome>,
    _subscriptions: Vec<Subscription>,
}

impl CardView {
    pub fn new(
        dock: Entity<Dock>,
        chrome: Entity<CardChrome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe(&chrome, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        Self {
            dock,
            chrome,
            _subscriptions: subscriptions,
        }
    }
}

/// Stats readings as currently drawn, easing toward the latest sample.
struct Readings {
    cpu: f32,
    cpu_fraction: f32,
    memory_fraction: f32,
    disk_fraction: f32,
}

impl CardView {
    fn sample_readings(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<Readings> {
        let stats = self.dock.read(cx).stats.clone()?;
        let gauge = || Spring::new(Duration::from_millis(520)).with_damping(0.78);
        Some(Readings {
            cpu: transition("card:cpu", stats.cpu, number_tween(), window, cx),
            cpu_fraction: spring("gauge:cpu", stats.cpu / 100.0, gauge(), window, cx),
            memory_fraction: spring(
                "gauge:memory",
                stats.memory_percent() / 100.0,
                gauge(),
                window,
                cx,
            ),
            disk_fraction: spring(
                "gauge:disk",
                stats.disk_percent() / 100.0,
                gauge(),
                window,
                cx,
            ),
        })
    }
}

impl Render for CardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let readings = self.sample_readings(window, cx);
        let animate = !cx.reduce_motion();
        let dock = self.dock.read(cx);
        let palette = Palette::new(window, dock.accessibility);
        let placement = self.chrome.read(cx).placement;
        let content = dock.card().map(|(_, item)| {
            let content = match &item.kind {
                ItemKind::App(app) => tooltip(
                    &app.name,
                    dock.shortcut_for(&item.id).map(ToString::to_string),
                    palette,
                ),
                ItemKind::Weather => weather_card(dock, palette),
                ItemKind::Stats => stats_card(dock, readings.as_ref(), palette),
                ItemKind::Clipboard => clipboard_card(&self.dock, dock, animate, palette),
                ItemKind::Plugin(manifest) => plugin_card(&self.dock, dock, manifest, palette),
            };
            let content = div().relative().size_full().child(content);
            if !animate {
                return content.into_any_element();
            }
            // Each item's content fades up as the card arrives or glides over.
            // A tooltip is a single word: it only needs a quick cross-fade.
            // Tooltip text starts part-way in, so the pill never shows empty.
            let (duration, rise_by, floor) = match item.kind {
                ItemKind::App(_) => (Duration::from_millis(90), 0.0, 0.4),
                _ => (Duration::from_millis(200), 3.0, 0.0),
            };
            content
                .with_animation(
                    SharedString::from(format!("card-content:{}", item.id)),
                    Animation::new(duration),
                    move |content, t| {
                        let rise = motion::sample(motion::CARD_IN.curve, t);
                        content
                            .opacity(floor + (1.0 - floor) * ease_out(t))
                            .top(px((1.0 - rise) * rise_by))
                    },
                )
                .into_any_element()
        });

        let body = placement.map_or_else(
            || crate::geometry::Rect::new(0.0, 0.0, 0.0, 0.0),
            |placement| placement.body(),
        );
        let hover = self.dock.clone();
        let card = div()
            .id("card")
            .test_support()
            .absolute()
            .left(px(body.x as f32))
            .top(px(body.y as f32))
            .w(px(body.width as f32))
            .h(px(body.height as f32))
            .text_color(palette.label)
            .on_hover(move |hovered: &bool, _, cx: &mut App| {
                hover.update(cx, |dock, cx| dock.set_card_hovered(*hovered, cx));
            })
            .children(content);

        div()
            .size_full()
            .relative()
            .children(
                placement.map(|placement| silhouette(placement, palette.surface, palette.stroke)),
            )
            .child(card)
    }
}

/// The card's silhouette: filled when the material is replaced by an opaque
/// surface (Reduce Transparency), and always edged with a hairline that runs
/// around the arrow too, so the arrow reads as part of the card.
fn silhouette(placement: CardPlacement, fill: Hsla, stroke: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let origin = bounds.origin;
            let (width, height) = (placement.frame.width, placement.frame.height);
            // Filled shapes use the outline as is; the 1-point stroke is pulled
            // in by half its width so no side is clipped by the window edge.
            let place = |p: geometry::Point, inset: f64| {
                let x = inset + p.x * (width - 2.0 * inset) / width;
                let y = inset + p.y * (height - 2.0 * inset) / height;
                point(origin.x + px(x as f32), origin.y + px(y as f32))
            };
            let trace = |mut builder: PathBuilder, inset: f64| {
                let at = |p| place(p, inset);
                for step in placement.outline() {
                    match step {
                        PathStep::Move(to) => builder.move_to(at(to)),
                        PathStep::Line(to) => builder.line_to(at(to)),
                        PathStep::Cubic {
                            control_a,
                            control_b,
                            to,
                        } => builder.cubic_bezier_to(at(to), at(control_a), at(control_b)),
                        PathStep::Close => builder.close(),
                    }
                }
                builder.build().ok()
            };
            if fill.a > 0.0
                && let Some(path) = trace(PathBuilder::fill(), 0.0)
            {
                window.paint_path(path, fill);
            }
            if let Some(path) = trace(PathBuilder::stroke(px(1.0)), 0.5) {
                window.paint_path(path, stroke);
            }
        },
    )
    .absolute()
    .size_full()
}

fn tooltip(name: &str, shortcut: Option<String>, palette: Palette) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(TOOLTIP_HINT_GAP as f32))
        .text_size(px(text::CALLOUT))
        .child(name.to_string())
        .children(shortcut.map(|shortcut| div().text_color(palette.secondary).child(shortcut)))
        .into_any_element()
}

pub(crate) fn card_body() -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .px(px(14.0))
        .py(px(12.0))
}

pub(crate) fn title(label: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(text::TITLE3))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

fn footer(left: impl IntoElement, right: impl IntoElement, palette: Palette) -> Div {
    div()
        .mt_auto()
        .pt(px(8.0))
        .border_t_1()
        .border_color(palette.separator)
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(text::SUBHEADLINE))
        .text_color(palette.tertiary)
        .child(left)
        .child(right)
}

fn updated_label(updated: Instant) -> String {
    match updated.elapsed().as_secs() / 60 {
        0 => "Updated just now".into(),
        1 => "Updated 1 min ago".into(),
        minutes => format!("Updated {minutes} min ago"),
    }
}

fn message_card(heading: &str, message: &str, palette: Palette) -> AnyElement {
    card_body()
        .gap(px(4.0))
        .child(title(heading.to_string()))
        .child(
            div()
                .text_size(px(text::CALLOUT))
                .text_color(palette.secondary)
                .child(message.to_string()),
        )
        .into_any_element()
}

fn weather_card(dock: &Dock, palette: Palette) -> AnyElement {
    let (weather, updated): (&Weather, Instant) = match &dock.weather {
        WeatherState::Ready { weather, updated } => (weather, *updated),
        WeatherState::Loading => {
            return message_card(&dock.location.name, "Loading weather…", palette);
        }
        WeatherState::Failed(message) => {
            return message_card(&dock.location.name, message, palette);
        }
    };

    let header = div()
        .flex()
        .items_start()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .child(title(dock.location.name.clone()))
                .child(
                    div()
                        .text_size(px(text::CALLOUT))
                        .text_color(palette.secondary)
                        .child(weather.condition.label()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(icon(
                    condition_icon(weather.condition, weather.is_day),
                    24.0,
                    palette.label,
                ))
                .child(
                    div()
                        .text_size(px(text::DISPLAY))
                        .font_weight(FontWeight::LIGHT)
                        .child(degrees(weather.temperature)),
                ),
        );

    let range = div()
        .mt(px(2.0))
        .text_size(px(text::CALLOUT))
        .text_color(palette.secondary)
        .child(format!(
            "High {}, low {}",
            degrees(weather.high),
            degrees(weather.low)
        ));

    let hours = div()
        .mt(px(10.0))
        .flex()
        .justify_between()
        .children(weather.hours.iter().map(|hour| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(5.0))
                .w(px(36.0))
                .child(
                    div()
                        .text_size(px(text::SUBHEADLINE))
                        .text_color(palette.secondary)
                        .child(format!("{:02}", hour.hour)),
                )
                .child(icon(
                    condition_icon(hour.condition, (6..20).contains(&hour.hour)),
                    15.0,
                    palette.label,
                ))
                .child(
                    div()
                        .text_size(px(text::CALLOUT))
                        .font_weight(FontWeight::MEDIUM)
                        .child(degrees(hour.temperature)),
                )
        }));

    card_body()
        .child(header)
        .child(range)
        .child(hours)
        .child(footer(updated_label(updated), "Open-Meteo", palette))
        .into_any_element()
}

pub(crate) fn gauge(fraction: f32, color: Hsla, palette: Palette) -> impl IntoElement {
    div()
        .h(px(6.0))
        .w_full()
        .rounded_full()
        .bg(palette.track)
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(color),
        )
}

pub(crate) fn meter(
    glyph: Option<SharedString>,
    label: impl Into<SharedString>,
    value: String,
    fraction: f32,
    color: Hsla,
    palette: Palette,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .children(glyph.map(|glyph| icon(glyph, 13.0, palette.secondary)))
                .child(
                    div()
                        .text_size(px(text::CALLOUT))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.into()),
                )
                .child(
                    div()
                        .ml_auto()
                        .text_size(px(text::CALLOUT))
                        .text_color(palette.secondary)
                        .child(value),
                ),
        )
        .child(gauge(fraction, color, palette))
}

fn stats_card(dock: &Dock, readings: Option<&Readings>, palette: Palette) -> AnyElement {
    let (Some(stats), Some(readings)) = (dock.stats.as_ref(), readings) else {
        return message_card("System", "Reading your Mac…", palette);
    };
    let sparkline =
        div()
            .h(px(18.0))
            .flex()
            .items_end()
            .gap(px(2.0))
            .children(dock.cpu_history.iter().map(|cpu| {
                div()
                    .w(px(3.0))
                    .h(relative((cpu / 100.0).clamp(0.12, 1.0)))
                    .rounded(px(1.0))
                    .bg(palette.tertiary)
            }));

    card_body()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(title("System"))
                .child(sparkline),
        )
        .child(meter(
            Some(IconName::Cpu.path()),
            "CPU",
            percent(readings.cpu),
            readings.cpu_fraction,
            palette.blue,
            palette,
        ))
        .child(meter(
            Some(IconName::MemoryStick.path()),
            "Memory",
            format!(
                "{} of {}",
                format_memory(stats.memory_used),
                format_memory(stats.memory_total)
            ),
            readings.memory_fraction,
            palette.green,
            palette,
        ))
        .child(meter(
            Some(IconName::HardDrive.path()),
            "Storage",
            format!(
                "{} free",
                format_bytes(stats.disk_total.saturating_sub(stats.disk_used))
            ),
            readings.disk_fraction,
            palette.orange,
            palette,
        ))
        .child(
            div()
                .text_size(px(text::CAPTION))
                .text_color(palette.tertiary)
                .child(format!("Refreshes every {} s", STATS_INTERVAL.as_secs())),
        )
        .into_any_element()
}

fn clipboard_card(
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    animate: bool,
    palette: Palette,
) -> AnyElement {
    let count = dock.history.len();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .child(title("Clipboard"))
        .child(
            div()
                .text_size(px(text::CALLOUT))
                .text_color(palette.secondary)
                .child(format!("{count} copied")),
        );

    if count == 0 {
        return card_body()
            .gap(px(4.0))
            .child(header)
            .child(
                div()
                    .text_size(px(text::CALLOUT))
                    .text_color(palette.secondary)
                    .child("Text, links, images and files you copy will appear here."),
            )
            .into_any_element();
    }

    let now = clipboard::now_secs();
    let rows = dock
        .history
        .entries
        .iter()
        .take(CLIPBOARD_ROWS)
        .enumerate()
        .map(|(index, entry)| {
            let row = clip_row(dock_entity, entry, now, palette);
            if !animate {
                return row;
            }
            // Rows cascade in as the card opens, and a new copy slides in on top.
            let delay = motion::ROW_STAGGER.as_secs_f32() * index as f32;
            let duration = 0.24 + delay;
            div()
                .relative()
                .child(row)
                .with_animation(
                    SharedString::from(format!("clip-row:{}", entry.id)),
                    Animation::new(Duration::from_secs_f32(duration)),
                    move |row, t| {
                        let local = ((t * duration - delay) / (duration - delay)).clamp(0.0, 1.0);
                        let rise = motion::sample(motion::ICON_IN.curve, local);
                        row.opacity(ease_out(local)).top(px((1.0 - rise) * 4.0))
                    },
                )
                .into_any_element()
        });

    let clear = dock_entity.clone();
    let armed = dock.is_clear_armed();
    let clear_button = div()
        .id("clear-history")
        .test_support()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .text_color(if armed { palette.red } else { palette.blue })
        .hover(|style| style.bg(palette.fill))
        .on_click(move |_, _, cx: &mut App| {
            clear.update(cx, |dock, cx| dock.request_clear_history(cx));
        })
        .child(if armed {
            "Click Again to Clear"
        } else {
            "Clear History"
        });

    let show = dock_entity.clone();
    let show_all = div()
        .id("show-all-history")
        .test_support()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .text_color(palette.blue)
        .hover(|style| style.bg(palette.fill))
        .on_click(move |_, _, cx: &mut App| {
            show.update(cx, |dock, cx| dock.show_clipboard_history(cx));
        })
        .child("Show All");

    card_body()
        .px(px(8.0))
        .gap(px(4.0))
        .child(div().px(px(6.0)).child(header))
        .child(div().flex().flex_col().children(rows))
        .child(
            div()
                .px(px(6.0))
                .mt_auto()
                .child(footer(show_all, clear_button, palette)),
        )
        .into_any_element()
}

fn clip_row(
    dock_entity: &Entity<Dock>,
    entry: &ClipEntry,
    now: u64,
    palette: Palette,
) -> AnyElement {
    let leading = match &entry.kind {
        ClipKind::Image { path, .. } => img(path.clone())
            .size(px(28.0))
            .rounded(px(6.0))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        kind => {
            let glyph = match kind {
                ClipKind::Link { .. } => IconName::Link,
                ClipKind::File { .. } => IconName::File,
                _ => IconName::FileText,
            };
            div()
                .size(px(28.0))
                .flex_shrink_0()
                .rounded(px(6.0))
                .bg(palette.fill)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(glyph.path(), 14.0, palette.secondary))
                .into_any_element()
        }
    };
    let detail = match &entry.source {
        Some(source) => format!("{} · {source}", entry.age_label(now)),
        None => entry.age_label(now),
    };
    let copy = dock_entity.clone();
    let id = entry.id;
    div()
        .id(("clip", entry.id))
        .test_support()
        .h(px(CLIP_ROW as f32))
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(6.0))
        .rounded(px(8.0))
        .hover(|style| style.bg(palette.fill))
        .active(|style| style.opacity(0.7))
        .on_click(move |_, _, cx: &mut App| {
            copy.update(cx, |dock, cx| dock.copy_entry(id, cx));
        })
        .child(leading)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(single_line(div().text_size(px(text::BODY))).child(entry.kind.title()))
                .child(
                    single_line(div().text_size(px(text::SUBHEADLINE)))
                        .text_color(palette.secondary)
                        .child(detail),
                ),
        )
        .into_any_element()
}
