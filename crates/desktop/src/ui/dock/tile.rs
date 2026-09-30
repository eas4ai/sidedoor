use super::*;

pub(super) fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn slot(
    dock_entity: &Entity<Dock>,
    view: &Entity<DockView>,
    index: usize,
    item: &DockItem,
    content: AnyElement,
    motion: SlotMotion,
    edge: Edge,
    shortcut: Option<String>,
) -> AnyElement {
    let dragged = DraggedSlot {
        index,
        icon: match &item.kind {
            ItemKind::App(app) => app.icon.clone(),
            _ => None,
        },
        plugin: match &item.kind {
            ItemKind::Plugin(manifest) => Some(manifest.clone()),
            _ => None,
        },
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
    let preview_dock = dock_entity.clone();
    let press = view.clone();
    let menu_dock = dock_entity.clone();

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
                let mut menu = ContextMenu::new();
                if is_app {
                    menu = menu
                        .menu("Open", Box::new(OpenItem { id: id.clone() }))
                        .menu(
                            crate::app::host::REVEAL_LABEL,
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
                    .show(event.position, &menu_dock, window, cx);
                cx.stop_propagation();
            },
        )
        // Reordering is handled by the dock: it opens a gap under the
        // pointer and drops the item there.
        .on_drag(dragged, move |dragged, _, _, cx| {
            let dock = preview_dock.clone();
            cx.new(|_| DragPreview {
                slot: dragged.clone(),
                dock,
            })
        })
        // The dragged item's own slot stays empty: it is the gap.
        .when(motion.lifted, |element| element.opacity(0.0))
        .child(arriving.child(content));

    let element = if edge.is_vertical() {
        element.w_full().h(px(SLOT as f32)).top(px(motion.shift))
    } else {
        element.h_full().w(px(SLOT as f32)).left(px(motion.shift))
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
pub(super) fn plugin_tile(
    dock_entity: &Entity<Dock>,
    manifest: &plugin_host::Manifest,
    tile: Option<&[plugin_host::Node]>,
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
    let surface = crate::ui::plugins::renderer::Surface {
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

pub(super) fn app_tile(
    app: &AppInfo,
    motion: SlotMotion,
    palette: Palette,
    vertical: bool,
) -> AnyElement {
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
            .text_size(px(text::snap(text::TITLE3 * motion.scale)))
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
