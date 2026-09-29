//! Choosing apps for the dock from those installed, where the system has no
//! file panel that lists apps the way Finder lists /Applications (Linux).

use super::{push_button, section};
use crate::app::dock::Dock;
use crate::app::host::AppInfo;
use crate::ui::theme::{Palette, text};
use domain::config::MAX_ITEMS;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, InteractiveElement as _,
    IntoElement, KeyDownEvent, ObjectFit, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription,
    TestSupportExt as _, Window,
    assets::IconName,
    component::input::{Input, InputEvent, InputState},
    div, img, px, svg,
};

pub const WINDOW_SIZE: (f32, f32) = (440.0, 520.0);

/// The window is done: closed, or dismissed with Escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPickerEvent {
    Dismiss,
    Closed,
}

pub struct AppPicker {
    dock: Entity<Dock>,
    apps: Vec<AppInfo>,
    search: Entity<InputState>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AppPickerEvent> for AppPicker {}

impl AppPicker {
    pub fn new(dock: Entity<Dock>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let apps = dock.read(cx).installed_apps();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search apps"));
        search.update(cx, |search, cx| search.focus(window, cx));
        let view = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            view.update(cx, |_, cx| cx.emit(AppPickerEvent::Closed))
                .ok();
            true
        });
        crate::ui::theme::sync_kit_theme(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::ui::theme::sync_kit_theme(window, cx);
            }),
        ];
        Self {
            dock,
            apps,
            search,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Apps whose name contains every word typed, ignoring case.
    fn matching(&self, cx: &Context<Self>) -> Vec<&AppInfo> {
        let query = self.search.read(cx).value().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        self.apps
            .iter()
            .filter(|app| {
                let name = app.name.to_lowercase();
                words.iter().all(|word| name.contains(word))
            })
            .collect()
    }
}

fn app_icon(app: &AppInfo, palette: Palette) -> gpui_kit::AnyElement {
    match &app.icon {
        Some(path) => img(path.clone())
            .size(px(24.0))
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        None => div()
            .size(px(24.0))
            .rounded(px(6.0))
            .bg(palette.fill)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(text::CALLOUT))
            .child(app.name.chars().next().unwrap_or('?').to_string())
            .into_any_element(),
    }
}

impl Render for AppPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock.read(cx);
        let palette = Palette::new(window, dock.accessibility);
        let room = dock.items.len() < MAX_ITEMS;
        let rows: Vec<_> = self
            .matching(cx)
            .into_iter()
            .map(|app| {
                let added = dock.has_app(&app.bundle_id);
                let (handler, path) = (self.dock.clone(), app.path.clone());
                div()
                    .min_h(px(40.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(app_icon(app, palette))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(app.name.clone()),
                    )
                    .child(if added {
                        div()
                            .text_size(px(text::SUBHEADLINE))
                            .text_color(palette.secondary)
                            .child("In the Dock")
                            .into_any_element()
                    } else {
                        push_button(
                            SharedString::from(format!("pick:{}", app.bundle_id)),
                            "Add",
                            palette,
                            room,
                            false,
                            move |_, cx| {
                                handler.update(cx, |dock, cx| {
                                    dock.add_paths(std::slice::from_ref(&path), None, cx);
                                });
                            },
                        )
                        .into_any_element()
                    })
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let used = format!("{} of {MAX_ITEMS} places used.", dock.items.len());
        let footer = if room {
            used
        } else {
            format!("{used} Remove an item to add another.")
        };

        div()
            .key_context("AppPicker")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(AppPickerEvent::Dismiss);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.surface)
            .text_color(palette.label)
            .text_size(px(text::BODY))
            .child(
                div().p(px(12.0)).flex_shrink_0().child(
                    div()
                        .id("app-search")
                        .test_support()
                        .h(px(28.0))
                        .px(px(8.0))
                        .rounded(px(7.0))
                        .bg(palette.fill)
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            svg()
                                .path(IconName::Search.path())
                                .size(px(14.0))
                                .text_color(palette.secondary),
                        )
                        .child(Input::new(&self.search).appearance(false)),
                ),
            )
            .child(
                div()
                    .id("installed-apps")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(12.0))
                    .pb(px(12.0))
                    .child(if empty {
                        div()
                            .pt(px(24.0))
                            .flex()
                            .justify_center()
                            .text_color(palette.secondary)
                            .child(if self.apps.is_empty() {
                                "No apps found."
                            } else {
                                "No matching apps."
                            })
                            .into_any_element()
                    } else {
                        section(None, rows, Some(footer.into()), palette)
                    }),
            )
    }
}
