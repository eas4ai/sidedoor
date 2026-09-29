//! The dock and its hover card, both read from the shared [`Dock`].

use crate::{
    dock::{Dock, DockItem, ItemKind},
    geometry::{self as geometry, CardPlacement, DOCK_PADDING, DOCK_RADIUS, Edge, PathStep, SLOT},
    motion,
    platform::AppInfo,
    style::{Palette, text},
};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, Animation, AnimationExt as _, AnyElement, App, AppContext as _, Bounds, Context, Div,
    Entity, ExternalPaths, FontWeight, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PathBuilder, Pixels, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, TestSupportExt as _,
    Window,
    assets::IconName,
    base::{Easing, Spring, Transition, spring, transition},
    canvas, div, img, point,
    prelude::FluentBuilder as _,
    px, relative, svg,
};
use std::{path::PathBuf, time::Duration};

const APP_ICON: f32 = 44.0;
const TILE: f32 = 36.0;
const TOOLTIP_HEIGHT: f64 = 28.0;
const TOOLTIP_PADDING: f64 = 24.0;
/// Space between a tooltip's name and its shortcut.
const TOOLTIP_HINT_GAP: f64 = 8.0;
/// A fitted plugin card's height until its content has been measured.
const FITTED_START: f64 = 120.0;

// MARK: Actions

/// Dispatched by the dock's context menu.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct OpenItem {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct RevealItem {
    pub id: SharedString,
}

/// A command a plugin added to its item's context menu.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct RunPluginAction {
    pub plugin: SharedString,
    pub key: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct RemoveItem {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct AssignShortcut {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct RemoveShortcut {
    pub id: SharedString,
}

#[derive(Clone, Default, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct OpenSettings;

/// Opens the settings file in the user's text editor.
#[derive(Clone, Default, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
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
        ItemKind::Plugin(manifest) => {
            let fitted = dock.plugin(&manifest.id).and_then(|state| state.height);
            (
                manifest.width,
                manifest.height.or(fitted).unwrap_or(FITTED_START),
            )
        }
    }
}

pub(crate) fn icon(path: SharedString, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(path)
        .size(px(size))
        .flex_shrink_0()
        .text_color(color)
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
        let view = cx.entity();
        let palette = Palette::new(window, self.dock.read(cx).accessibility);
        // Plugin tiles draw with the window, which the dock borrow below
        // would block.
        let plugin_tiles: Vec<(
            usize,
            crate::plugin::Manifest,
            Option<Vec<crate::plugin::Node>>,
        )> = {
            let dock = self.dock.read(cx);
            dock.items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| match &item.kind {
                    ItemKind::Plugin(manifest) => Some((
                        index,
                        manifest.clone(),
                        dock.plugin(&manifest.id)
                            .and_then(|state| state.tile.clone()),
                    )),
                    _ => None,
                })
                .collect()
        };
        let mut plugin_tiles: std::collections::HashMap<usize, AnyElement> = plugin_tiles
            .into_iter()
            .map(|(index, manifest, tile)| {
                let scale = motions.get(index).map_or(1.0, |motion| motion.scale);
                let tile = plugin_tile(
                    &self.dock,
                    &manifest,
                    tile.as_deref(),
                    scale,
                    palette,
                    window,
                    cx,
                );
                (index, tile)
            })
            .collect();
        let dock = self.dock.read(cx);
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
                    ItemKind::Plugin(_) => plugin_tiles
                        .remove(&index)
                        .unwrap_or_else(|| div().into_any_element()),
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
    let plugin_actions: Vec<(SharedString, RunPluginAction)> = match &item.kind {
        ItemKind::Plugin(manifest) => manifest
            .actions
            .iter()
            .map(|action| {
                let run = RunPluginAction {
                    plugin: manifest.id.clone().into(),
                    key: action.key.clone().into(),
                };
                (action.title.clone().into(), run)
            })
            .collect(),
        _ => Vec::new(),
    };

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
                        .menu(
                            crate::platform::REVEAL_LABEL,
                            Box::new(RevealItem { id: id.clone() }),
                        )
                        .separator();
                }
                if !plugin_actions.is_empty() {
                    for (title, run) in &plugin_actions {
                        menu = menu.menu(title.clone(), Box::new(run.clone()));
                    }
                    menu = menu.separator();
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
        ItemKind::App(_) => IconName::AppWindow.path(),
        ItemKind::Plugin(manifest) => manifest.icon_path().into(),
    }
}

/// A plugin's own tile, or its icon until it draws one.
fn plugin_tile(
    dock_entity: &Entity<Dock>,
    manifest: &crate::plugin::Manifest,
    tile: Option<&[crate::plugin::Node]>,
    scale: f32,
    palette: Palette,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(tile) = tile else {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(icon(
                manifest.icon_path().into(),
                20.0 * scale,
                palette.label,
            ))
            .into_any_element();
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
        .items_center()
        .justify_center()
        .children(surface.render_tile(tile, scale, window, cx))
        .into_any_element()
}

/// What a plugin's card shows, copied out of the dock so it can be drawn
/// with the window at hand.
struct PluginCard {
    manifest: crate::plugin::Manifest,
    tree: Option<Vec<crate::plugin::Node>>,
    problem: Option<SharedString>,
}

impl PluginCard {
    fn of(dock: &Dock) -> Option<Self> {
        let (_, item) = dock.card()?;
        let ItemKind::Plugin(manifest) = &item.kind else {
            return None;
        };
        let state = dock.plugin(&manifest.id);
        Some(Self {
            manifest: manifest.clone(),
            tree: state.and_then(|state| state.card.clone()),
            problem: state.and_then(|state| state.problem.clone()),
        })
    }

    /// What the plugin drew, or why it can't draw yet. A card without a
    /// fixed height reports its content's height so the window can fit it.
    fn render(
        self,
        dock_entity: &Entity<Dock>,
        palette: Palette,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let name = self.manifest.name.clone();
        let content = match (&self.problem, &self.tree) {
            (Some(problem), _) => message_card(&name, problem, palette),
            (None, None) => message_card(&name, "Starting…", palette),
            (None, Some(tree)) => {
                let surface = crate::plugin_ui::Surface {
                    dock: dock_entity.clone(),
                    plugin: self.manifest.id.clone().into(),
                    palette,
                };
                // Whatever doesn't fit is cut off at the card's edge rather
                // than drawn over the arrow.
                div()
                    .w_full()
                    .when(self.manifest.height.is_some(), |el| el.h_full())
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .children(surface.render(tree, "card", window, cx))
                    .into_any_element()
            }
        };
        if self.manifest.height.is_some() {
            return content;
        }
        let (dock, id) = (dock_entity.clone(), self.manifest.id.clone());
        div()
            .w_full()
            .flex()
            .flex_col()
            .on_children_prepainted(move |bounds, _, cx| {
                let top = bounds.iter().map(|b| b.top()).min();
                let bottom = bounds.iter().map(|b| b.bottom()).max();
                if let (Some(top), Some(bottom)) = (top, bottom) {
                    let height = f64::from(f32::from(bottom - top));
                    dock.update(cx, |dock, cx| dock.set_plugin_height(&id, height, cx));
                }
            })
            .child(content)
            .into_any_element()
    }
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

impl Render for CardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let animate = !cx.reduce_motion();
        let palette = Palette::new(window, self.dock.read(cx).accessibility);
        // Plugin cards draw with the window, which the dock borrow below
        // would block.
        let mut plugin_card = PluginCard::of(self.dock.read(cx))
            .map(|card| card.render(&self.dock, palette, window, cx));
        let dock = self.dock.read(cx);
        let placement = self.chrome.read(cx).placement;
        let content = dock.card().map(|(_, item)| {
            let content = match &item.kind {
                ItemKind::App(app) => tooltip(
                    &app.name,
                    dock.shortcut_for(&item.id).map(ToString::to_string),
                    palette,
                ),
                ItemKind::Plugin(_) => plugin_card
                    .take()
                    .unwrap_or_else(|| div().into_any_element()),
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
