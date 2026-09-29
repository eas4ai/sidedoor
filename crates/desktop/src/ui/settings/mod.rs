//! Settings, laid out like a SwiftUI `Settings` scene: a toolbar of tabs
//! over grouped forms. Every change applies and saves right away.

use crate::app::dock::{Dock, DockItem, ItemKind, WeatherState, Widget};
use crate::app::host::LoginItem;
use crate::ui::dock::{self as views, AssignShortcut, OpenConfigFile};
use crate::ui::theme::{Palette, text};
use domain::config::{Appearance, MAX_ITEMS, WeatherLocation};
use domain::geometry::Edge;
use domain::shortcut::PC_KEYS;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Context, Div, ElementId,
    Entity, EventEmitter, FocusHandle, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task,
    TestSupportExt as _, Window, WindowControlArea,
    assets::IconName,
    component::input::{Input, InputEvent, InputState},
    div, img, linear_color_stop, linear_gradient,
    prelude::FluentBuilder as _,
    px, rgba, svg, transparent_black,
};
use services::weather::Place;
use std::{sync::Arc, time::Duration};

/// Key context of the window, so its keys are handled only here.
pub const CONTEXT: &str = "Settings";
/// The title row, which lines up with the traffic lights. The system
/// caption shows the title elsewhere.
const TITLE_HEIGHT: f32 = crate::ui::chrome::title_strip(28.0);
pub const WINDOW_SIZE: (f32, f32) = (620.0, 532.0 + TITLE_HEIGHT);
/// How long typing pauses before a place search starts.
const SEARCH_DELAY: Duration = Duration::from_millis(300);
const PAGE_FADE: Duration = Duration::from_millis(180);
/// Thumbnails in the edge and theme pickers.
const THUMB: (f32, f32) = (68.0, 44.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    General,
    Dock,
    Items,
    Plugins,
    Weather,
}

impl Tab {
    pub const ALL: [Self; 5] = [
        Self::General,
        Self::Dock,
        Self::Items,
        Self::Plugins,
        Self::Weather,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Dock => "Dock",
            Self::Items => "Items",
            Self::Plugins => "Plugins",
            Self::Weather => "Weather",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Dock => IconName::PanelRight,
            Self::Items => IconName::LayoutGrid,
            Self::Plugins => IconName::Puzzle,
            Self::Weather => IconName::CloudSun,
        }
    }
}

/// What the window asks of the app around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    /// ⌘W: close the window.
    Dismiss,
    /// The close button was used; the window is already closing.
    Closed,
}

/// Looks up places by name; blocking, so it runs in the background. Tests
/// supply canned results.
pub type PlaceLookup = Arc<dyn Fn(&str) -> Result<Vec<Place>, String> + Send + Sync>;

enum PlaceSearch {
    Idle,
    Searching,
    Found {
        query: SharedString,
        places: Vec<Place>,
    },
    Failed(SharedString),
}

pub struct SettingsWindow {
    dock: Entity<Dock>,
    tab: Tab,
    city: Entity<InputState>,
    places: PlaceSearch,
    lookup: PlaceLookup,
    /// The pending search; replacing it drops a stale one.
    search_task: Option<Task<()>>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsWindow {}

impl SettingsWindow {
    pub fn new(
        dock: Entity<Dock>,
        lookup: PlaceLookup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let city = cx.new(|cx| InputState::new(window, cx).placeholder("Search for a city"));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let view = cx.entity().downgrade();
        let closing = view.clone();
        window.on_window_should_close(cx, move |_, cx| {
            closing
                .update(cx, |_, cx| cx.emit(SettingsEvent::Closed))
                .ok();
            true
        });
        crate::ui::theme::sync_kit_theme(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&city, window, |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = state.read(cx).value();
                    this.search(query, cx);
                }
            }),
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::ui::theme::sync_kit_theme(window, cx);
                cx.notify();
            }),
            // The app has no menu bar, so ⌘W and ⌘1–5 are handled here.
            cx.intercept_keystrokes(move |event, window, cx| {
                if !event
                    .context_stack
                    .iter()
                    .any(|context| context.contains(CONTEXT))
                {
                    return;
                }
                let keystroke = &event.keystroke;
                let modifiers = keystroke.modifiers;
                let primary = if PC_KEYS {
                    modifiers.control
                } else {
                    modifiers.platform
                };
                let other = if PC_KEYS {
                    modifiers.platform
                } else {
                    modifiers.control
                };
                if !primary || modifiers.alt || other || modifiers.shift {
                    return;
                }
                let handled = view
                    .update(cx, |this, cx| match keystroke.key.as_str() {
                        "w" => {
                            cx.emit(SettingsEvent::Dismiss);
                            true
                        }
                        key => match key.parse::<usize>() {
                            Ok(number @ 1..=5) => {
                                this.set_tab(Tab::ALL[number - 1], window, cx);
                                true
                            }
                            _ => false,
                        },
                    })
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            }),
        ];
        Self {
            dock,
            tab: Tab::General,
            city,
            places: PlaceSearch::Idle,
            lookup,
            search_task: None,
            focus,
            _subscriptions: subscriptions,
        }
    }

    #[cfg(test)]
    pub fn tab(&self) -> Tab {
        self.tab
    }

    #[cfg(test)]
    pub fn focus_city(&self, window: &mut Window, cx: &mut App) {
        self.city.update(cx, |city, cx| city.focus(window, cx));
    }

    pub fn set_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if tab != self.tab {
            self.tab = tab;
            window.set_window_title(tab.title());
            cx.notify();
        }
    }

    fn search(&mut self, query: SharedString, cx: &mut Context<Self>) {
        let query = query.trim().to_string();
        if query.chars().count() < 2 {
            self.places = PlaceSearch::Idle;
            self.search_task = None;
            cx.notify();
            return;
        }
        self.places = PlaceSearch::Searching;
        let lookup = self.lookup.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let request = query.clone();
            let result = cx
                .background_executor()
                .spawn(async move { lookup(&request) })
                .await;
            this.update(cx, |this, cx| {
                this.places = match result {
                    Ok(places) => PlaceSearch::Found {
                        query: query.into(),
                        places,
                    },
                    Err(message) => PlaceSearch::Failed(message.into()),
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn choose_place(&mut self, place: &Place, window: &mut Window, cx: &mut Context<Self>) {
        self.dock
            .update(cx, |dock, cx| dock.set_location(place.location(), cx));
        // `set_value` doesn't report a change, so reset the search here.
        self.city
            .update(cx, |city, cx| city.set_value("", window, cx));
        self.places = PlaceSearch::Idle;
        self.search_task = None;
        cx.notify();
    }

    fn add_apps(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add to Dock".into()),
        });
        let dock = self.dock.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                cx.update(|cx| {
                    dock.update(cx, |dock, cx| {
                        dock.add_paths(&paths, None, cx);
                    });
                });
            }
        })
        .detach();
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let palette = Palette::new(window, self.dock.read(cx).accessibility);
        // The Plugins page has text fields, which need the window.
        let plugins = (self.tab == Tab::Plugins)
            .then(|| crate::ui::settings::plugins::plugins_page(&self.dock, palette, window, cx));
        let dock = self.dock.read(cx);
        let sections = match self.tab {
            Tab::Plugins => plugins.unwrap_or_default(),
            Tab::General => general_page(&self.dock, dock, palette),
            Tab::Dock => dock_page(&self.dock, dock, palette),
            Tab::Items => items_page(&view, &self.dock, dock, palette),
            Tab::Weather => weather_page(&view, &self.city, &self.places, dock, palette),
        };
        let page = div().flex_1().min_h_0().flex().flex_col().child(
            div()
                .id("settings-page")
                .test_support()
                .size_full()
                .overflow_y_scroll()
                .child(
                    div()
                        .px(px(20.0))
                        .pt(px(18.0))
                        .pb(px(22.0))
                        .flex()
                        .flex_col()
                        .gap(px(20.0))
                        .children(sections),
                ),
        );
        let page = if dock.accessibility.reduce_motion {
            page.into_any_element()
        } else {
            page.with_animation(
                SharedString::from(format!("settings-page:{}", self.tab.title())),
                Animation::new(PAGE_FADE),
                |page, t| page.opacity(1.0 - (1.0 - t).powi(3)),
            )
            .into_any_element()
        };

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.surface)
            .text_color(palette.label)
            .text_size(px(text::BODY))
            .child(toolbar(&view, self.tab, palette))
            .child(page)
    }
}

// MARK: Chrome

fn toolbar(view: &Entity<SettingsWindow>, current: Tab, palette: Palette) -> impl IntoElement {
    let tabs = Tab::ALL.iter().enumerate().map(|(index, &tab)| {
        let chosen = tab == current;
        let tint = if chosen {
            palette.blue
        } else {
            palette.secondary
        };
        let view = view.clone();
        div()
            .id(("tab", index))
            .test_support()
            .w(px(72.0))
            .h(px(50.0))
            .rounded(px(8.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(3.0))
            .when_else(
                chosen,
                |button| button.bg(palette.fill),
                |button| button.hover(|style| style.bg(palette.fill)),
            )
            .child(
                svg()
                    .path(tab.icon().path())
                    .size(px(20.0))
                    .text_color(tint),
            )
            .child(
                div()
                    .text_size(px(text::SUBHEADLINE))
                    .text_color(if chosen { palette.blue } else { palette.label })
                    .child(tab.title()),
            )
            .on_click(move |_, window, cx: &mut App| {
                view.update(cx, |this, cx| this.set_tab(tab, window, cx));
            })
    });

    div()
        .flex_shrink_0()
        .pb(px(6.0))
        .border_b_1()
        .border_color(palette.separator)
        .window_control_area(WindowControlArea::Drag)
        .when(crate::ui::chrome::INSET_TITLE_BAR, |this| {
            this.child(
                div()
                    .h(px(TITLE_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(current.title()),
            )
        })
        .when(!crate::ui::chrome::INSET_TITLE_BAR, |this| this.pt(px(6.0)))
        .child(div().flex().justify_center().gap(px(2.0)).children(tabs))
}

/// A grouped form section: a bold title, rounded rows split by hairlines,
/// and an optional note underneath.
pub(crate) fn section(
    title: Option<&'static str>,
    rows: Vec<AnyElement>,
    footer: Option<SharedString>,
    palette: Palette,
) -> AnyElement {
    let mut body = Vec::with_capacity(rows.len() * 2);
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            body.push(
                div()
                    .h(px(1.0))
                    .mx(px(12.0))
                    .bg(palette.separator)
                    .into_any_element(),
            );
        }
        body.push(row);
    }
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .children(title.map(|title| {
            div()
                .px(px(4.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title)
        }))
        .child(
            div()
                .rounded(px(10.0))
                .bg(palette.group)
                .border_1()
                .border_color(palette.separator)
                .flex()
                .flex_col()
                .children(body),
        )
        .children(footer.map(|footer| {
            div()
                .px(px(4.0))
                .text_size(px(text::SUBHEADLINE))
                .text_color(palette.secondary)
                .child(footer)
        }))
        .into_any_element()
}

/// A labelled form row with its control on the trailing side.
pub(crate) fn row(
    label: impl Into<SharedString>,
    detail: Option<SharedString>,
    control: impl IntoElement,
    palette: Palette,
) -> AnyElement {
    div()
        .min_h(px(40.0))
        .px(px(12.0))
        .py(px(8.0))
        .flex()
        .items_center()
        .gap(px(16.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(label.into())
                .children(detail.map(|detail| {
                    div()
                        .text_size(px(text::SUBHEADLINE))
                        .text_color(palette.secondary)
                        .child(detail)
                })),
        )
        .child(control)
        .into_any_element()
}

/// A macOS push button.
pub(crate) fn push_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    palette: Palette,
    enabled: bool,
    destructive: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .flex_shrink_0()
        .when(destructive, |button| button.text_color(palette.red))
        .h(px(24.0))
        .px(px(12.0))
        .rounded(px(6.0))
        .bg(palette.keycap)
        .border_1()
        .border_color(palette.stroke)
        .flex()
        .items_center()
        .justify_center()
        .when_else(
            enabled,
            |button| {
                button
                    .active(|style| style.bg(palette.fill))
                    .on_click(move |_, window, cx| on_click(window, cx))
            },
            |button| button.opacity(0.45),
        )
        .child(label.into())
}

/// One thumbnail in a row of choices, ringed in the accent color when
/// chosen, like the Appearance picker in System Settings.
fn choice(
    id: ElementId,
    label: &'static str,
    chosen: bool,
    art: AnyElement,
    palette: Palette,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(5.0))
        .child(
            div()
                .p(px(2.0))
                .rounded(px(9.0))
                .border_2()
                .border_color(if chosen {
                    palette.blue
                } else {
                    transparent_black()
                })
                .child(art),
        )
        .child(
            div()
                .text_size(px(text::SUBHEADLINE))
                .when_else(
                    chosen,
                    |label| label.font_weight(FontWeight::MEDIUM),
                    |label| label.text_color(palette.secondary),
                )
                .child(label),
        )
        .on_click(move |_, _, cx| on_click(cx))
}

fn thumbnail(palette: Palette) -> Div {
    div()
        .relative()
        .w(px(THUMB.0))
        .h(px(THUMB.1))
        .rounded(px(6.0))
        .overflow_hidden()
        .border_1()
        .border_color(palette.stroke)
}

/// A tiny desktop with the dock at `edge`.
fn edge_art(edge: Edge, palette: Palette) -> AnyElement {
    let wallpaper = linear_gradient(
        135.0,
        linear_color_stop(Hsla::from(rgba(0x5b8defff)), 0.0),
        linear_color_stop(Hsla::from(rgba(0x9b6bdfff)), 1.0),
    );
    let bar = div().absolute().rounded(px(2.0)).bg(rgba(0xffffffe6));
    let bar = match edge {
        Edge::Left => bar.left(px(3.0)).top(px(12.0)).w(px(5.0)).h(px(24.0)),
        Edge::Right => bar.right(px(3.0)).top(px(12.0)).w(px(5.0)).h(px(24.0)),
        Edge::Bottom => bar.bottom(px(3.0)).left(px(19.0)).w(px(30.0)).h(px(5.0)),
    };
    thumbnail(palette)
        .bg(wallpaper)
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(4.0))
                .bg(rgba(0xffffff66)),
        )
        .child(bar)
        .into_any_element()
}

/// A tiny window in light, dark, or both halves for "System".
fn theme_art(appearance: Appearance, palette: Palette) -> AnyElement {
    let desk = |dark: bool| {
        div()
            .flex_1()
            .h_full()
            .bg(rgba(if dark { 0x2c2c30ff } else { 0xdcdce2ff }))
    };
    let pane = |dark: bool| {
        div()
            .flex_1()
            .h_full()
            .bg(rgba(if dark { 0x48484eff } else { 0xffffffff }))
    };
    let (desks, panes) = match appearance {
        Appearance::System => (vec![desk(false), desk(true)], vec![pane(false), pane(true)]),
        Appearance::Light => (vec![desk(false)], vec![pane(false)]),
        Appearance::Dark => (vec![desk(true)], vec![pane(true)]),
    };
    thumbnail(palette)
        .flex()
        .children(desks)
        .child(
            div()
                .absolute()
                .left(px(12.0))
                .top(px(10.0))
                .w(px(44.0))
                .h(px(28.0))
                .rounded(px(3.0))
                .overflow_hidden()
                .flex()
                .children(panes),
        )
        .into_any_element()
}

fn glyph(name: IconName, size: f32, color: Hsla) -> impl IntoElement {
    svg().path(name.path()).size(px(size)).text_color(color)
}

// MARK: Pages

pub(crate) mod app_picker;
mod dock;
mod general;
mod items;
pub(crate) mod plugins;
mod weather;
use dock::dock_page;
use general::general_page;
use items::items_page;
use weather::weather_page;
