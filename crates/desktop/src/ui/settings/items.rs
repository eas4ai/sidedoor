use super::*;

/// A row of the item list being dragged to a new place.
#[derive(Clone)]
struct DraggedRow {
    from: usize,
    name: SharedString,
}

impl Render for DraggedRow {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::new(window, Default::default());
        div()
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(8.0))
            .bg(palette.keycap)
            .border_1()
            .border_color(palette.stroke)
            .text_size(px(text::BODY))
            .text_color(palette.label)
            .child(self.name.clone())
    }
}

fn item_icon(item: &DockItem, palette: Palette) -> AnyElement {
    if let ItemKind::App(app) = &item.kind
        && let Some(path) = &app.icon
    {
        return img(path.clone())
            .size(px(24.0))
            .object_fit(ObjectFit::Contain)
            .into_any_element();
    }
    let fill = match &item.kind {
        ItemKind::Plugin(manifest) => match manifest.id.as_str() {
            crate::builtins::WEATHER => palette.blue,
            crate::builtins::CLIPBOARD => palette.purple,
            crate::builtins::STATS => palette.green,
            _ => palette.orange,
        },
        ItemKind::App(_) => palette.fill,
    };
    div()
        .size(px(22.0))
        .m(px(1.0))
        .rounded(px(6.0))
        .bg(fill)
        .flex()
        .items_center()
        .justify_center()
        .child(
            svg()
                .path(views::widget_glyph(&item.kind))
                .size(px(13.0))
                .text_color(palette.on_accent),
        )
        .into_any_element()
}

fn item_row(
    index: usize,
    item: &DockItem,
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    palette: Palette,
) -> AnyElement {
    let id = item.id.clone();
    let name: SharedString = dock.item_name(&id).into();
    let kind = match item.kind {
        ItemKind::App(_) => "App",
        _ => "Widget",
    };

    let assign = id.clone();
    let shortcut = div()
        .id(SharedString::from(format!("shortcut:{id}")))
        .test_support()
        .flex_shrink_0()
        .h(px(24.0))
        .min_w(px(104.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .bg(palette.fill)
        .flex()
        .items_center()
        .justify_center()
        .hover(|style| style.bg(palette.track))
        .child(match dock.shortcut_for(&id) {
            Some(shortcut) => div()
                .font_weight(FontWeight::MEDIUM)
                .child(shortcut.to_string()),
            None => div().text_color(palette.tertiary).child("Record Shortcut"),
        })
        .on_click(move |_, window, cx| {
            window.dispatch_action(Box::new(AssignShortcut { id: assign.clone() }), cx);
        });

    let (remover, removed) = (dock_entity.clone(), id.clone());
    let remove = div()
        .id(SharedString::from(format!("remove:{id}")))
        .test_support()
        .flex_shrink_0()
        .size(px(22.0))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(palette.tertiary)
        .hover(|style| style.text_color(palette.red))
        .child(
            svg()
                .path(IconName::CircleMinus.path())
                .size(px(16.0))
                .text_color(palette.tertiary),
        )
        .on_click(move |_, _, cx| {
            remover.update(cx, |dock, cx| dock.remove(&removed, cx));
        });

    let mover = dock_entity.clone();
    div()
        .id(SharedString::from(format!("item-row:{id}")))
        .test_support()
        .h(px(52.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .rounded(px(10.0))
        .child(glyph(IconName::GripVertical, 14.0, palette.tertiary))
        .child(item_icon(item, palette))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(div().truncate().child(name.clone()))
                .child(
                    div()
                        .text_size(px(text::SUBHEADLINE))
                        .text_color(palette.secondary)
                        .child(kind),
                ),
        )
        .child(shortcut)
        .child(remove)
        .on_drag(
            DraggedRow {
                from: index,
                name: name.clone(),
            },
            |dragged, _, _, cx| cx.new(|_| dragged.clone()),
        )
        .drag_over::<DraggedRow>(move |style, _, _, _| style.bg(palette.accent_fill))
        .on_drop(move |dragged: &DraggedRow, _, cx| {
            // Dropping on a row takes its place: after it when moving down,
            // before it when moving up.
            let to = if dragged.from < index {
                index + 1
            } else {
                index
            };
            mover.update(cx, |dock, cx| dock.move_item(dragged.from, to, cx));
        })
        .into_any_element()
}

pub(super) fn items_page(
    view: &Entity<SettingsWindow>,
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    palette: Palette,
) -> Vec<AnyElement> {
    let mut rows: Vec<AnyElement> = dock
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| item_row(index, item, dock_entity, dock, palette))
        .collect();
    if rows.is_empty() {
        rows.push(
            div()
                .h(px(44.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(palette.secondary)
                .child("The dock is empty.")
                .into_any_element(),
        );
    }

    let room = dock.items.len() < MAX_ITEMS;
    let adder = view.clone();
    let mut additions = vec![row(
        "App",
        Some("Choose one or more apps to keep in the dock.".into()),
        push_button(
            "add-app",
            "Add App…",
            palette,
            room,
            false,
            move |_, cx| {
                adder.update(cx, |this, cx| this.add_apps(cx));
            },
        ),
        palette,
    )];
    for widget in dock.missing_widgets() {
        let handler = dock_entity.clone();
        let detail = match widget {
            Widget::Weather => "Conditions and the next hours at a glance.",
            Widget::Clipboard => "Everything you copy, searchable.",
            Widget::Stats => "CPU, memory and disk use.",
        };
        additions.push(row(
            widget.name(),
            Some(detail.into()),
            push_button(
                SharedString::from(format!("add-widget:{}", widget.name())),
                "Add",
                palette,
                room,
                false,
                move |_, cx| {
                    handler.update(cx, |dock, cx| {
                        dock.add_widget(widget, cx);
                    });
                },
            ),
            palette,
        ));
    }

    for manifest in dock
        .available_plugins()
        .into_iter()
        .filter(|manifest| !crate::builtins::contains(&manifest.id))
    {
        let handler = dock_entity.clone();
        let id = SharedString::from(format!("add-plugin:{}", manifest.id));
        additions.push(row(
            manifest.name.clone(),
            Some(format!("Plugin · {}", manifest.dir.display()).into()),
            push_button(id, "Add…", palette, room, false, move |window, cx| {
                crate::ui::settings::plugins::confirm_add(
                    handler.clone(),
                    manifest.clone(),
                    window,
                    cx,
                )
            }),
            palette,
        ));
    }

    let used = format!("{} of {MAX_ITEMS} places used.", dock.items.len());
    vec![
        section(
            Some("In the Dock"),
            rows,
            Some(
                "Drag to reorder. A shortcut opens its item from any app; widgets peek out.".into(),
            ),
            palette,
        ),
        section(
            Some("Add"),
            additions,
            Some(if room {
                used.into()
            } else {
                format!("{used} Remove an item to add another.").into()
            }),
            palette,
        ),
    ]
}
