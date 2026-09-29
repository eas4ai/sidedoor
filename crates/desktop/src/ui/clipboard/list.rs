use super::*;

pub(super) fn section_header(label: &'static str, palette: Palette) -> AnyElement {
    div()
        .px(px(8.0))
        .pt(px(10.0))
        .pb(px(4.0))
        .text_size(px(text::SUBHEADLINE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.secondary)
        .child(label)
        .into_any_element()
}

pub(super) fn empty_note(message: &'static str, palette: Palette) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .py(px(40.0))
        .text_color(palette.secondary)
        .child(message)
        .into_any_element()
}

fn thumbnail(kind: &ClipKind, selected: bool, palette: Palette) -> AnyElement {
    match kind {
        ClipKind::Image { path, .. } => img(path.clone())
            .size(px(22.0))
            .rounded(px(4.0))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        kind => div()
            .size(px(22.0))
            .flex_shrink_0()
            .rounded(px(5.0))
            .bg(if selected {
                palette.on_accent.opacity(0.2)
            } else {
                palette.fill
            })
            .flex()
            .items_center()
            .justify_center()
            .child(glyph(
                kind_glyph(kind),
                12.0,
                if selected {
                    palette.on_accent
                } else {
                    palette.secondary
                },
            ))
            .into_any_element(),
    }
}

pub(super) fn list_row(
    view: &Entity<ClipboardWindow>,
    entry: &ClipEntry,
    selected: bool,
    palette: Palette,
) -> AnyElement {
    let view = view.clone();
    let id = entry.id;
    div()
        .id(("history-row", entry.id))
        .test_support()
        .h(px(ROW_HEIGHT))
        .flex_shrink_0()
        .px(px(8.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .when_else(
            selected,
            |row| row.bg(palette.blue).text_color(palette.on_accent),
            |row| row.hover(|style| style.bg(palette.fill)),
        )
        .on_click(move |event: &ClickEvent, _, cx: &mut App| {
            view.update(cx, |this, cx| {
                this.select(id, cx);
                // Double-click copies, like pressing Return.
                if event.click_count() >= 2 {
                    this.copy_selected(cx);
                }
            });
        })
        .child(thumbnail(&entry.kind, selected, palette))
        .child(single_line(div().flex_1()).child(entry.kind.title()))
        .into_any_element()
}
