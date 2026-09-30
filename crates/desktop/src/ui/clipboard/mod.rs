//! The Clipboard History window: search and filter everything copied,
//! preview it, and copy it back with the keyboard.

use crate::app::dock::Dock;
use crate::ui::theme::{Palette, text};
use domain::clipboard::{self, ClipEntry, ClipKind, Filter};
use domain::shortcut::PC_KEYS;
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
    /// Images this window draws, freed once it stops drawing them.
    images: Entity<crate::ui::image_cache::FrameImages>,
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
        crate::ui::theme::sync_kit_theme(window, cx);
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
                crate::ui::theme::sync_kit_theme(window, cx);
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
            images: crate::ui::image_cache::FrameImages::new(cx),
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
            || if PC_KEYS {
                modifiers.platform
            } else {
                modifiers.control
            };
        match (
            key.key.as_str(),
            if PC_KEYS {
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

impl ClipboardWindow {
    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

impl Render for ClipboardWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.images
            .update(cx, |images, cx| images.sweep(window, cx));
        gpui_kit::image_cache(self.images.clone())
            .size_full()
            .child(self.content(window, cx))
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
        // Room for the window's traffic lights, where they sit in the toolbar.
        .pl(px(if crate::ui::chrome::INSET_TITLE_BAR {
            84.0
        } else {
            12.0
        }))
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
                    if PC_KEYS { "Ctrl+Backspace" } else { "⌘⌫" },
                    ClipboardWindow::delete_selected,
                )),
        )
}

mod list;
mod preview;
use list::{empty_note, list_row, section_header};
use preview::{image_box, information, information_rows, preview};
