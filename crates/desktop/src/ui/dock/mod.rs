//! The dock and its hover card, both read from the shared [`Dock`].

use crate::app::dock::{Dock, DockItem, ItemKind};
use crate::app::host::AppInfo;
use crate::ui::menu::ContextMenu;
use crate::ui::theme::{Palette, text};
use domain::geometry::{
    self as geometry, CardPlacement, DOCK_PADDING, DOCK_RADIUS, Edge, PathStep, SLOT,
};
use domain::motion;
use gpui_kit::{
    Action, Animation, AnimationExt as _, AnyElement, App, AppContext as _, Bounds, Context, Div,
    DragMoveEvent, Entity, ExternalPaths, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PathBuilder,
    Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    TestSupportExt as _, Window,
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

/// Opens the list of installed apps to add to the dock (Linux).
#[derive(Clone, Default, PartialEq, Action)]
#[action(namespace = sidedoor, no_json)]
pub struct AddApps;

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
    /// A plugin carries its live tile along.
    plugin: Option<plugin_host::Manifest>,
}

struct DragPreview {
    slot: DraggedSlot,
    dock: Entity<Dock>,
}

impl Render for DragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Lifted out of the dock whole, as the Dock does, not as a ghost.
        if let Some(manifest) = &self.slot.plugin {
            let palette = Palette::new(window, self.dock.read(cx).accessibility);
            let tile = self
                .dock
                .read(cx)
                .plugin(&manifest.id)
                .and_then(|state| state.tile.clone());
            // Off the dock's surface, the tile brings a piece of it along.
            return div()
                .size(px(SLOT as f32))
                .rounded(px(12.0))
                .bg(palette.surface)
                .border_1()
                .border_color(palette.stroke)
                .shadow_md()
                .text_color(palette.label)
                .child(plugin_tile(
                    &self.dock,
                    manifest,
                    tile.as_deref(),
                    1.0,
                    palette,
                    window,
                    cx,
                ))
                .into_any_element();
        }
        let artwork = match &self.slot.icon {
            Some(path) => img(path.clone()).size(px(APP_ICON)).into_any_element(),
            None => div()
                .size(px(TILE))
                .rounded(px(9.0))
                .bg(gpui_kit::hsla(0.0, 0.0, 0.5, 0.35))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(IconName::AppWindow.path(), 18.0, gpui_kit::white()))
                .into_any_element(),
        };
        div().child(artwork).into_any_element()
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
    /// How far the item is drawn from its slot along the dock, while items
    /// part for a drag or settle after one.
    shift: f32,
    /// The item being dragged: its slot is the gap.
    lifted: bool,
}

/// An item being dragged along the dock.
#[derive(Clone, Copy, PartialEq)]
struct Reorder {
    from: usize,
    /// The slot under the pointer, where it would land.
    to: usize,
}

pub struct DockView {
    dock: Entity<Dock>,
    /// Images this window draws, freed once it stops drawing them.
    images: Entity<crate::ui::image_cache::FrameImages>,
    /// The pointer's position along the dock, while it is over it.
    pointer: Option<f32>,
    pressed: Option<usize>,
    reorder: Option<Reorder>,
    _subscriptions: Vec<Subscription>,
}

impl DockView {
    pub fn new(dock: Entity<Dock>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        Self {
            images: crate::ui::image_cache::FrameImages::new(cx),
            dock,
            pointer: None,
            pressed: None,
            reorder: None,
            _subscriptions: subscriptions,
        }
    }

    /// Samples every item's springs and transitions for this frame.
    fn sample_motion(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<SlotMotion> {
        let (items, shown, incoming): (Vec<(SharedString, bool)>, bool, Option<usize>) = {
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
            (items, dock.is_shown(), dock.incoming())
        };
        let dragging = cx.has_active_drag();
        let count = items.len();
        let reorder = self.reorder;

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
                // Each item springs toward where it would sit if the drag
                // ended now. Tracked in dock coordinates rather than as an
                // offset, so when the drop reorders the items each one is
                // already where its new slot puts it, and nothing jumps.
                // Apps dragged in from Finder part the items the same way.
                let target = match (reorder, incoming) {
                    (Some(reorder), _) => geometry::reordered_slot(index, reorder.from, reorder.to),
                    (None, Some(at)) if index >= at => index + 1,
                    _ => index,
                };
                let position = spring(
                    SharedString::from(format!("position:{id}")),
                    SLOT as f32 * target as f32,
                    Spring::new(motion::REORDER).with_damping(motion::REORDER_DAMPING),
                    window,
                    cx,
                );
                SlotMotion {
                    scale,
                    arrival,
                    dot,
                    shift: position - SLOT as f32 * index as f32,
                    lifted: reorder.is_some_and(|reorder| reorder.from == index),
                }
            })
            .collect()
    }
}

impl DockView {
    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A drag that ended anywhere, dropped or not, lets the items settle.
        if !cx.has_active_drag() {
            self.reorder = None;
            if self.dock.read(cx).incoming().is_some() {
                let dock = self.dock.clone();
                cx.defer(move |cx| dock.update(cx, |dock, cx| dock.set_incoming(None, cx)));
            }
        }
        let motions = self.sample_motion(window, cx);
        let view = cx.entity();
        let palette = Palette::new(window, self.dock.read(cx).accessibility);
        // Plugin tiles draw with the window, which the dock borrow below
        // would block.
        let plugin_tiles: Vec<(usize, plugin_host::Manifest, Option<Vec<plugin_host::Node>>)> = {
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
                    &self.dock, &view, index, item, content, motion, edge, shortcut,
                )
            })
            .collect();

        let (append_paths, incoming_dock) = (self.dock.clone(), self.dock.clone());
        let (drop_dock, drop_view) = (self.dock.clone(), view.clone());
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
            // Apps dragged in from Finder open a gap under the pointer, and
            // land in it.
            .on_drag_move(
                move |event: &DragMoveEvent<ExternalPaths>, _, cx: &mut App| {
                    let paths = event.drag(cx).paths().to_vec();
                    let (along, across, length, thickness) = if vertical {
                        (
                            event.event.position.y - event.bounds.top(),
                            event.event.position.x - event.bounds.left(),
                            event.bounds.size.height,
                            event.bounds.size.width,
                        )
                    } else {
                        (
                            event.event.position.x - event.bounds.left(),
                            event.event.position.y - event.bounds.top(),
                            event.bounds.size.width,
                            event.bounds.size.height,
                        )
                    };
                    let over = along >= px(0.0)
                        && along <= length
                        && across >= px(0.0)
                        && across <= thickness;
                    incoming_dock.update(cx, |dock, cx| {
                        let at = (over
                            && (dock.incoming().is_some() || dock.accepts_paths(&paths)))
                        .then(|| {
                            // The gap is a slot of its own, so the pointer
                            // picks among one more slot than there are items.
                            geometry::drop_slot(f64::from(f32::from(along)), dock.items.len() + 1)
                        });
                        dock.set_incoming(at, cx);
                    });
                },
            )
            .on_drop(move |paths: &ExternalPaths, _, cx: &mut App| {
                append_paths.update(cx, |dock, cx| {
                    let at = dock.incoming();
                    dock.add_paths(paths.paths(), at, cx)
                });
            })
            // Follows a drag of one of the items: the slot under the pointer
            // opens up. Off the dock, the gap closes back where it came from.
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<DraggedSlot>, _, cx| {
                    let from = event.drag(cx).index;
                    let (along, across, length, thickness) = if vertical {
                        (
                            event.event.position.y - event.bounds.top(),
                            event.event.position.x - event.bounds.left(),
                            event.bounds.size.height,
                            event.bounds.size.width,
                        )
                    } else {
                        (
                            event.event.position.x - event.bounds.left(),
                            event.event.position.y - event.bounds.top(),
                            event.bounds.size.width,
                            event.bounds.size.height,
                        )
                    };
                    let over = along >= px(0.0)
                        && along <= length
                        && across >= px(0.0)
                        && across <= thickness;
                    let to = if over {
                        geometry::drop_slot(f64::from(f32::from(along)), count)
                    } else {
                        from
                    };
                    let reorder = Some(Reorder { from, to });
                    if this.reorder != reorder {
                        this.reorder = reorder;
                        cx.notify();
                    }
                }),
            )
            .on_drop(move |dragged: &DraggedSlot, _, cx: &mut App| {
                let to = drop_view
                    .read(cx)
                    .reorder
                    .map_or(dragged.index, |reorder| reorder.to);
                // `move_item` takes the position before removal.
                let before = if to > dragged.index { to + 1 } else { to };
                drop_dock.update(cx, |dock, cx| dock.move_item(dragged.index, before, cx));
            })
            .children(slots);
        if vertical {
            container.flex_col().py(px(DOCK_PADDING as f32))
        } else {
            container.flex_row().px(px(DOCK_PADDING as f32))
        }
    }
}

impl Render for DockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.images
            .update(cx, |images, cx| images.sweep(window, cx));
        gpui_kit::image_cache(self.images.clone())
            .size_full()
            .child(self.content(window, cx))
    }
}

mod card;
/// Cubic ease-out for fades.
mod tile;
pub use card::{CardChrome, CardView};
pub(crate) use card::{card_body, meter, title};
pub use tile::widget_glyph;
use tile::{app_tile, ease_out, plugin_tile, slot};
