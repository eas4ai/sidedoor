//! The Clipboard History window: search and filter everything copied,
//! preview it, and copy it back with the keyboard.

use crate::{
    clipboard::{self, ClipEntry, ClipKind, Filter},
    dock::Dock,
    style::{Palette, text},
};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, EventEmitter, FontWeight,
    Hsla, InteractiveElement as _, IntoElement, Keystroke, ObjectFit, ParentElement as _, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Subscription, TestSupportExt as _, Window, WindowControlArea,
    assets::IconName,
    component::input::{Input, InputEvent, InputState},
    div, img,
    prelude::FluentBuilder as _,
    px, relative, svg,
};

/// Key context of the window, so its keys are handled only here.
pub const CONTEXT: &str = "ClipboardHistory";
/// Height of the toolbar, which doubles as the title bar.
pub const TOOLBAR_HEIGHT: f32 = 52.0;
const LIST_WIDTH: f32 = 290.0;
const ROW_HEIGHT: f32 = 34.0;
const FOOTER_HEIGHT: f32 = 44.0;
const DETAIL_PADDING: f32 = 14.0;
const DETAIL_GAP: f32 = 12.0;
const INFO_TITLE: f32 = 16.0;
const INFO_ROW: f32 = 24.0;
const INFO_GAP: f32 = 4.0;
/// Space between an image preview and its frame.
const IMAGE_INSET: f32 = 10.0;

/// What the window asks of the app around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardWindowEvent {
    /// Close the window; `copied` when an entry was just put on the
    /// pasteboard, so focus can return to where the user will paste.
    Dismiss { copied: bool },
}

pub struct ClipboardWindow {
    dock: Entity<Dock>,
    search: Entity<InputState>,
    query: SharedString,
    filter: Filter,
    selected: Option<u64>,
    list_scroll: ScrollHandle,
    /// List child index of each visible row, for scrolling to the selection
    /// (section headers are children too).
    row_children: Vec<(u64, usize)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ClipboardWindowEvent> for ClipboardWindow {}

impl ClipboardWindow {
    pub fn new(dock: Entity<Dock>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type to filter entries…"));
        let view = cx.entity().downgrade();
        crate::style::sync_kit_theme(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.query = state.read(cx).value();
                    this.keep_selection_valid(cx);
                    cx.notify();
                }
            }),
            cx.observe(&dock, |this, _, cx| {
                this.keep_selection_valid(cx);
                cx.notify();
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::style::sync_kit_theme(window, cx);
                cx.notify();
            }),
            // The search field binds the arrows, Return and Escape itself;
            // intercept them first while this window has focus.
            cx.intercept_keystrokes(move |event, window, cx| {
                if !event
                    .context_stack
                    .iter()
                    .any(|context| context.contains(CONTEXT))
                {
                    return;
                }
                let handled = view
                    .update(cx, |this, cx| this.handle_key(&event.keystroke, window, cx))
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            }),
        ];
        search.update(cx, |search, cx| search.focus(window, cx));

        let mut this = Self {
            dock,
            search,
            query: SharedString::default(),
            filter: Filter::All,
            selected: None,
            list_scroll: ScrollHandle::new(),
            row_children: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.keep_selection_valid(cx);
        this
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    fn visible_ids(&self, cx: &App) -> Vec<u64> {
        clipboard::search(&self.dock.read(cx).history, &self.query, self.filter)
            .iter()
            .map(|entry| entry.id)
            .collect()
    }

    /// Keeps a visible entry selected as the list changes under it.
    fn keep_selection_valid(&mut self, cx: &App) {
        let visible = self.visible_ids(cx);
        if self.selected.is_none_or(|id| !visible.contains(&id)) {
            self.selected = visible.first().copied();
        }
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.keep_selection_valid(cx);
        cx.notify();
    }

    fn select(&mut self, id: u64, cx: &mut Context<Self>) {
        self.selected = Some(id);
        if let Some((_, child)) = self.row_children.iter().find(|(row, _)| *row == id) {
            self.list_scroll.scroll_to_item(*child);
        }
        cx.notify();
    }

    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let visible = self.visible_ids(cx);
        if visible.is_empty() {
            return;
        }
        let current = self
            .selected
            .and_then(|id| visible.iter().position(|&row| row == id));
        let next = match current {
            Some(index) => (index as isize + by).clamp(0, visible.len() as isize - 1) as usize,
            None => 0,
        };
        self.select(visible[next], cx);
    }

    fn copy_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        self.dock.update(cx, |dock, cx| dock.copy_entry(id, cx));
        cx.emit(ClipboardWindowEvent::Dismiss { copied: true });
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        // Select the next entry down (or up, at the end) before removing.
        let visible = self.visible_ids(cx);
        if let Some(index) = visible.iter().position(|&row| row == id) {
            self.selected = visible
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|up| visible.get(up)))
                .copied();
        }
        self.dock.update(cx, |dock, cx| dock.delete_entry(id, cx));
        cx.notify();
    }

    /// Handles the window's own keys. Returns whether the key was used.
    fn handle_key(&mut self, key: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = key.modifiers;
        let other = modifiers.shift
            || modifiers.alt
            || if cfg!(windows) {
                modifiers.platform
            } else {
                modifiers.control
            };
        match (
            key.key.as_str(),
            if cfg!(windows) {
                modifiers.control
            } else {
                modifiers.platform
            },
            other,
        ) {
            ("down", false, false) => self.move_selection(1, cx),
            ("up", false, false) => self.move_selection(-1, cx),
            ("enter", false, false) => self.copy_selected(cx),
            ("backspace", true, false) => self.delete_selected(cx),
            ("escape", false, false) => {
                if self.query.is_empty() {
                    cx.emit(ClipboardWindowEvent::Dismiss { copied: false });
                } else {
                    // Setting the value programmatically emits no change event.
                    self.search
                        .update(cx, |search, cx| search.set_value("", window, cx));
                    self.query = SharedString::default();
                    self.keep_selection_valid(cx);
                    cx.notify();
                }
            }
            _ => return false,
        }
        true
    }
}

impl Render for ClipboardWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let dock = self.dock.read(cx);
        let palette = Palette::new(window, dock.accessibility);
        let now = clipboard::now_secs();
        let offset = dock.utc_offset();
        let entries = clipboard::search(&dock.history, &self.query, self.filter);
        let total = dock.history.len();
        let selected = self
            .selected
            .and_then(|id| entries.iter().find(|entry| entry.id == id).copied());

        // The list, grouped by day, remembering where each row lands.
        let mut children: Vec<AnyElement> = Vec::new();
        let mut row_children = Vec::new();
        let mut group = None;
        for entry in &entries {
            let label = clipboard::day_group(entry.copied_at, now, offset);
            if group != Some(label) {
                group = Some(label);
                children.push(section_header(label, palette));
            }
            row_children.push((entry.id, children.len()));
            let is_selected = Some(entry.id) == self.selected;
            children.push(list_row(&view, entry, is_selected, palette));
        }
        self.row_children = row_children;

        let list = div()
            .id("clip-list")
            .test_support()
            .w(px(LIST_WIDTH))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(palette.separator)
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .p(px(8.0))
            .flex()
            .flex_col()
            .children(children)
            .children(entries.is_empty().then(|| {
                empty_note(
                    if total == 0 {
                        "Nothing copied yet"
                    } else {
                        "No matching entries."
                    },
                    palette,
                )
            }));

        let detail = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(DETAIL_PADDING))
            .flex()
            .flex_col()
            .gap(px(DETAIL_GAP))
            .children(selected.map(|entry| {
                let rows = information_rows(entry, offset);
                let image_box = image_box(window.viewport_size(), rows.len());
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(DETAIL_GAP))
                    .child(preview(&self.dock, entry, image_box, palette))
                    .child(information(rows, palette))
            }));

        div()
            .key_context(CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.surface)
            .text_color(palette.label)
            .text_size(px(text::BODY))
            .child(toolbar(&view, &self.search, self.filter, palette))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .border_t_1()
                    .border_color(palette.separator)
                    .child(list)
                    .child(detail),
            )
            .child(footer(&view, total, selected.is_some(), palette))
    }
}

fn toolbar(
    view: &Entity<ClipboardWindow>,
    search: &Entity<InputState>,
    filter: Filter,
    palette: Palette,
) -> impl IntoElement {
    let segments = Filter::EVERY.iter().enumerate().map(|(index, &option)| {
        let view = view.clone();
        let chosen = option == filter;
        div()
            .id(("filter", index))
            .test_support()
            .px(px(9.0))
            .py(px(3.0))
            .rounded(px(5.0))
            .text_size(px(text::CALLOUT))
            .when_else(
                chosen,
                |segment| segment.bg(palette.segment).font_weight(FontWeight::MEDIUM),
                |segment| {
                    segment
                        .text_color(palette.secondary)
                        .hover(|style| style.text_color(palette.label))
                },
            )
            .on_click(move |_, _, cx: &mut App| {
                view.update(cx, |this, cx| this.set_filter(option, cx));
            })
            .child(option.label())
    });

    div()
        .h(px(TOOLBAR_HEIGHT))
        .flex_shrink_0()
        // Room for the window's traffic lights.
        .pl(px(84.0))
        .pr(px(12.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .window_control_area(WindowControlArea::Drag)
        .child(
            div()
                .flex_1()
                .h(px(30.0))
                .px(px(8.0))
                .rounded(px(8.0))
                .bg(palette.fill)
                .flex()
                .items_center()
                .child(
                    Input::new(search)
                        .appearance(false)
                        .cleanable(true)
                        .prefix(glyph(IconName::Search, 14.0, palette.secondary)),
                ),
        )
        .child(
            div()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(7.0))
                .bg(palette.fill)
                .children(segments),
        )
}

fn glyph(name: IconName, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(name.path())
        .size(px(size))
        .flex_shrink_0()
        .text_color(color)
}

fn kind_glyph(kind: &ClipKind) -> IconName {
    match kind {
        ClipKind::Text { .. } => IconName::FileText,
        ClipKind::Link { .. } => IconName::Link,
        ClipKind::Image { .. } => IconName::Image,
        ClipKind::File { .. } => IconName::File,
    }
}

fn section_header(label: &'static str, palette: Palette) -> AnyElement {
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

fn empty_note(message: &'static str, palette: Palette) -> AnyElement {
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

fn list_row(
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

fn single_line(element: Div) -> Div {
    element
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
}

fn small_button(
    id: &'static str,
    label: &'static str,
    palette: Palette,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(palette.fill)
        .hover(|style| style.bg(palette.track))
        .active(|style| style.opacity(0.7))
        .text_size(px(text::CALLOUT))
        .font_weight(FontWeight::MEDIUM)
        .on_click(move |_, _, cx: &mut App| on_click(cx))
        .child(label)
}

/// The space an image preview gets, from the window's fixed layout. GPUI
/// sizes a loaded image by its aspect ratio, so it needs an explicit box.
fn image_box(viewport: gpui_kit::Size<gpui_kit::Pixels>, info_rows: usize) -> (f32, f32) {
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

fn preview(
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
                    crate::platform::REVEAL_LABEL,
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

fn information_rows(entry: &ClipEntry, utc_offset: i64) -> Vec<(&'static str, String)> {
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

fn information(rows: Vec<(&'static str, String)>, palette: Palette) -> impl IntoElement {
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

fn footer(
    view: &Entity<ClipboardWindow>,
    total: usize,
    has_selection: bool,
    palette: Palette,
) -> impl IntoElement {
    let action = |id: &'static str,
                  label: &'static str,
                  keys: &'static str,
                  run: fn(&mut ClipboardWindow, &mut Context<ClipboardWindow>)| {
        let view = view.clone();
        div()
            .id(id)
            .test_support()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(text::CALLOUT))
            .font_weight(FontWeight::MEDIUM)
            .when_else(
                has_selection,
                |button| {
                    button
                        .hover(|style| style.bg(palette.fill))
                        .on_click(move |_, _, cx: &mut App| view.update(cx, run))
                },
                |button| button.opacity(0.4),
            )
            .child(label)
            // Key equivalents read like a menu's: plain symbols, dimmed.
            .child(div().text_color(palette.tertiary).child(keys))
    };

    div()
        .h(px(FOOTER_HEIGHT))
        .flex_shrink_0()
        .px(px(12.0))
        .border_t_1()
        .border_color(palette.separator)
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(7.0))
                .bg(palette.fill)
                .text_size(px(text::CALLOUT))
                .child(
                    div()
                        .size(px(18.0))
                        .rounded(px(5.0))
                        .bg(palette.purple)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(glyph(IconName::Clipboard, 11.0, palette.on_accent)),
                )
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Clipboard History"),
                )
                .child(div().text_color(palette.secondary).child(total.to_string())),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(action(
                    "copy-entry",
                    "Copy to Clipboard",
                    "↩",
                    ClipboardWindow::copy_selected,
                ))
                .child(div().w(px(1.0)).h(px(16.0)).bg(palette.separator))
                .child(action(
                    "delete-entry",
                    "Delete",
                    if cfg!(windows) {
                        "Ctrl+Backspace"
                    } else {
                        "⌘⌫"
                    },
                    ClipboardWindow::delete_selected,
                )),
        )
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
