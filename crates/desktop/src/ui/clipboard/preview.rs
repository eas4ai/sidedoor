use super::*;

/// The space an image preview gets, from the window's fixed layout. GPUI
/// sizes a loaded image by its aspect ratio, so it needs an explicit box.
pub(super) fn image_box(
    viewport: gpui_kit::Size<gpui_kit::Pixels>,
    info_rows: usize,
) -> (f32, f32) {
    let info = INFO_TITLE + info_rows as f32 * (INFO_ROW + INFO_GAP);
    let width = f32::from(viewport.width) - LIST_WIDTH - 1.0 - 2.0 * DETAIL_PADDING;
    let height = f32::from(viewport.height)
        - TOOLBAR_HEIGHT
        - FOOTER_HEIGHT
        - 2.0
        - 2.0 * DETAIL_PADDING
        - DETAIL_GAP
        - info;
    (
        (width - 2.0 * IMAGE_INSET).max(0.0),
        (height - 2.0 * IMAGE_INSET).max(0.0),
    )
}

/// The largest size with the image's proportions that fits in `space`,
/// never larger than its native size (pixels are half-points on Retina).
fn fit(width: u32, height: u32, space: (f32, f32)) -> (f32, f32) {
    if width == 0 || height == 0 {
        return space;
    }
    let (width, height) = (width as f32, height as f32);
    let scale = (space.0 / width).min(space.1 / height).min(0.5);
    (width * scale, height * scale)
}

pub(super) fn preview(
    dock: &Entity<Dock>,
    entry: &ClipEntry,
    image_box: (f32, f32),
    palette: Palette,
) -> impl IntoElement {
    let frame = div()
        .id("preview-frame")
        .test_support()
        .flex_1()
        .min_h_0()
        .rounded(px(10.0))
        .bg(palette.fill)
        .overflow_hidden();
    let content: AnyElement = match &entry.kind {
        ClipKind::Text { text } => div()
            .id(("preview", entry.id))
            .size_full()
            .overflow_y_scroll()
            .p(px(14.0))
            .line_height(relative(1.45))
            .child(text.clone())
            .into_any_element(),
        // Pinned to the frame so the image has a definite box to fit into.
        ClipKind::Image {
            path,
            width,
            height,
        } => {
            let (w, h) = fit(*width, *height, image_box);
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(path.clone())
                        .id("preview-image")
                        .w(px(w))
                        .h(px(h))
                        .object_fit(ObjectFit::Contain)
                        .test_support(),
                )
                .into_any_element()
        }
        ClipKind::Link { url } => {
            let dock = dock.clone();
            let target = url.clone();
            centered_stack()
                .child(glyph(IconName::Link, 28.0, palette.secondary))
                .child(
                    div()
                        .px(px(20.0))
                        .text_color(palette.blue)
                        .text_center()
                        .child(url.clone()),
                )
                .child(small_button("open-link", "Open Link", palette, move |cx| {
                    // `open` hands URLs to the default browser.
                    dock.read(cx).open_path(std::path::Path::new(&target));
                }))
                .into_any_element()
        }
        ClipKind::File { path } => {
            let dock = dock.clone();
            let target = path.clone();
            let folder = path
                .parent()
                .map(|parent| parent.display().to_string())
                .unwrap_or_default();
            centered_stack()
                .child(glyph(IconName::File, 40.0, palette.secondary))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(entry.kind.title()),
                )
                .child(
                    div()
                        .px(px(20.0))
                        .text_size(px(text::SUBHEADLINE))
                        .text_color(palette.secondary)
                        .text_center()
                        .child(folder),
                )
                .child(small_button(
                    "reveal-file",
                    crate::app::host::REVEAL_LABEL,
                    palette,
                    move |cx| {
                        dock.read(cx).reveal_path(&target);
                    },
                ))
                .into_any_element()
        }
    };
    frame.child(content)
}

fn centered_stack() -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
}

pub(super) fn information_rows(entry: &ClipEntry, utc_offset: i64) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = vec![
        (
            "Source",
            entry.source.clone().unwrap_or_else(|| "Unknown".into()),
        ),
        ("Type", entry.kind.type_label().into()),
    ];
    match &entry.kind {
        ClipKind::Text { text } => {
            let (characters, words) = clipboard::text_counts(text);
            rows.push(("Characters", characters.to_string()));
            rows.push(("Words", words.to_string()));
        }
        ClipKind::Image { width, height, .. } => {
            rows.push(("Dimensions", format!("{width} × {height}")));
        }
        ClipKind::Link { .. } | ClipKind::File { .. } => {}
    }
    rows.push((
        "Copied",
        clipboard::format_timestamp(entry.copied_at, utc_offset),
    ));
    rows
}

pub(super) fn information(rows: Vec<(&'static str, String)>, palette: Palette) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(INFO_GAP))
        .child(
            div()
                .h(px(INFO_TITLE))
                .px(px(10.0))
                .text_size(px(text::SUBHEADLINE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.secondary)
                .child("Information"),
        )
        .children(rows.into_iter().enumerate().map(|(index, (label, value))| {
            div()
                .h(px(INFO_ROW))
                .px(px(10.0))
                .rounded(px(5.0))
                .when(index % 2 == 0, |row| row.bg(palette.fill))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(text::CALLOUT))
                .child(div().text_color(palette.secondary).child(label))
                .child(single_line(div()).child(value))
        }))
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn images_fit_without_growing_past_native_size() {
        assert_eq!(fit(512, 512, (400.0, 200.0)), (200.0, 200.0));
        assert_eq!(fit(2000, 1000, (400.0, 400.0)), (400.0, 200.0));
        // A small image stays at its own size.
        assert_eq!(fit(64, 32, (400.0, 400.0)), (32.0, 16.0));
    }
}
