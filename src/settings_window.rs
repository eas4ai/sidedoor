//! Settings, laid out like a SwiftUI `Settings` scene: a toolbar of tabs
//! over grouped forms. Every change applies and saves right away.

use crate::{
    config::{Appearance, Config, MAX_ITEMS, WeatherLocation},
    dock::{Dock, DockItem, ItemKind, WeatherState, Widget},
    geometry::Edge,
    platform::LoginItem,
    style::{Palette, text},
    views::{self, AssignShortcut, OpenConfigFile},
    weather::Place,
};
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Context, Div, ElementId,
    Entity, EventEmitter, FocusHandle, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task,
    TestSupportExt as _, Window, WindowControlArea,
    assets::IconName,
    component::{
        Disableable as _, Sizable as _,
        input::{Input, InputEvent, InputState},
        switch::Switch,
    },
    div, img, linear_color_stop, linear_gradient,
    prelude::FluentBuilder as _,
    px, rgba, svg, transparent_black,
};
use std::{sync::Arc, time::Duration};

/// Key context of the window, so its keys are handled only here.
pub const CONTEXT: &str = "Settings";
pub const WINDOW_SIZE: (f32, f32) = (620.0, 560.0);
/// The title row, which lines up with the traffic lights.
const TITLE_HEIGHT: f32 = 28.0;
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
    Weather,
}

impl Tab {
    pub const ALL: [Self; 4] = [Self::General, Self::Dock, Self::Items, Self::Weather];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Dock => "Dock",
            Self::Items => "Items",
            Self::Weather => "Weather",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Dock => IconName::PanelRight,
            Self::Items => IconName::LayoutGrid,
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
        crate::style::sync_kit_theme(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&city, window, |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = state.read(cx).value();
                    this.search(query, cx);
                }
            }),
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::style::sync_kit_theme(window, cx);
                cx.notify();
            }),
            // The app has no menu bar, so ⌘W and ⌘1–4 are handled here.
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
                if !modifiers.platform || modifiers.alt || modifiers.control || modifiers.shift {
                    return;
                }
                let handled = view
                    .update(cx, |this, cx| match keystroke.key.as_str() {
                        "w" => {
                            cx.emit(SettingsEvent::Dismiss);
                            true
                        }
                        key => match key.parse::<usize>() {
                            Ok(number @ 1..=4) => {
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
        let dock = self.dock.read(cx);
        let palette = Palette::new(window, dock.accessibility);
        let sections = match self.tab {
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
        .child(
            div()
                .h(px(TITLE_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .font_weight(FontWeight::SEMIBOLD)
                .child(current.title()),
        )
        .child(div().flex().justify_center().gap(px(2.0)).children(tabs))
}

/// A grouped form section: a bold title, rounded rows split by hairlines,
/// and an optional note underneath.
fn section(
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
fn row(
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
fn push_button(
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

fn general_page(dock_entity: &Entity<Dock>, dock: &Dock, palette: Palette) -> Vec<AnyElement> {
    let login = dock.login_item();
    let detail: Option<SharedString> = match login {
        LoginItem::NeedsApproval => {
            Some("Allow Sidekick Clone in System Settings › General › Login Items.".into())
        }
        LoginItem::Unavailable => Some("Only the installed app can open at login.".into()),
        LoginItem::On | LoginItem::Off => None,
    };
    let handler = dock_entity.clone();
    let launch = Switch::new("launch-at-login")
        .small()
        .color(palette.blue)
        .checked(matches!(login, LoginItem::On | LoginItem::NeedsApproval))
        .disabled(login == LoginItem::Unavailable)
        .on_click(move |checked, _, cx| {
            handler.update(cx, |dock, cx| dock.set_launch_at_login(*checked, cx));
        });

    let count = dock.history.len();
    let armed = dock.is_clear_armed();
    let handler = dock_entity.clone();
    let clear = push_button(
        "clear-history",
        if armed {
            "Click Again to Clear"
        } else {
            "Clear History"
        },
        palette,
        count > 0,
        armed,
        move |_, cx| handler.update(cx, |dock, cx| dock.request_clear_history(cx)),
    );

    let path = Config::path();
    let shown_path = match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => path.display().to_string().replacen(&home, "~", 1),
        _ => path.display().to_string(),
    };
    let handler = dock_entity.clone();
    let file_buttons = div()
        .flex()
        .gap(px(8.0))
        .child(push_button(
            "reveal-config",
            "Show in Finder",
            palette,
            true,
            false,
            move |_, cx| handler.read(cx).reveal_path(&Config::path()),
        ))
        .child(push_button(
            "open-config",
            "Open",
            palette,
            true,
            false,
            |window, cx| window.dispatch_action(Box::new(OpenConfigFile), cx),
        ));

    vec![
        section(
            Some("Startup"),
            vec![row("Open at login", detail, launch, palette)],
            None,
            palette,
        ),
        section(
            Some("Clipboard"),
            vec![row(
                "Clipboard history",
                Some(match count {
                    1 => "1 item kept on this Mac.".into(),
                    count => format!("{count} items kept on this Mac.").into(),
                }),
                clear,
                palette,
            )],
            None,
            palette,
        ),
        section(
            Some("Advanced"),
            vec![row(
                "Settings file",
                Some(shown_path.into()),
                file_buttons,
                palette,
            )],
            Some("Everything here is saved to this file as you change it.".into()),
            palette,
        ),
        div()
            .flex()
            .justify_center()
            .text_size(px(text::SUBHEADLINE))
            .text_color(palette.tertiary)
            .child(format!(
                "Sidekick Clone {} · Weather by Open-Meteo",
                env!("CARGO_PKG_VERSION")
            ))
            .into_any_element(),
    ]
}

fn dock_page(dock_entity: &Entity<Dock>, dock: &Dock, palette: Palette) -> Vec<AnyElement> {
    let edges = [
        (Edge::Left, "Left"),
        (Edge::Right, "Right"),
        (Edge::Bottom, "Bottom"),
    ];
    let edge_picker =
        div()
            .flex()
            .gap(px(10.0))
            .children(edges.into_iter().map(|(edge, label)| {
                let handler = dock_entity.clone();
                choice(
                    SharedString::from(format!("edge:{label}")).into(),
                    label,
                    dock.edge == edge,
                    edge_art(edge, palette),
                    palette,
                    move |cx| handler.update(cx, |dock, cx| dock.set_edge(edge, cx)),
                )
            }));

    let themes = [
        (Appearance::System, "Automatic"),
        (Appearance::Light, "Light"),
        (Appearance::Dark, "Dark"),
    ];
    let theme_picker =
        div()
            .flex()
            .gap(px(10.0))
            .children(themes.into_iter().map(|(appearance, label)| {
                let handler = dock_entity.clone();
                choice(
                    SharedString::from(format!("theme:{label}")).into(),
                    label,
                    dock.appearance() == appearance,
                    theme_art(appearance, palette),
                    palette,
                    move |cx| handler.update(cx, |dock, cx| dock.set_appearance(appearance, cx)),
                )
            }));

    vec![
        section(
            Some("Position"),
            vec![row(
                "Screen edge",
                Some("Move the pointer to this edge to bring up the dock.".into()),
                edge_picker,
                palette,
            )],
            None,
            palette,
        ),
        section(
            Some("Appearance"),
            vec![row(
                "Theme",
                Some("Automatic follows macOS.".into()),
                theme_picker,
                palette,
            )],
            None,
            palette,
        ),
    ]
}

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
    let fill = match item.kind {
        ItemKind::Weather => palette.blue,
        ItemKind::Clipboard => palette.purple,
        ItemKind::Stats => palette.green,
        ItemKind::Plugin(_) => palette.orange,
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

fn items_page(
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

    for manifest in dock.available_plugins() {
        let handler = dock_entity.clone();
        let id = SharedString::from(format!("add-plugin:{}", manifest.id));
        additions.push(row(
            manifest.name.clone(),
            Some(format!("Plugin · {}", manifest.dir.display()).into()),
            push_button(id, "Add", palette, room, false, move |_, cx| {
                let manifest = manifest.clone();
                handler.update(cx, |dock, cx| {
                    dock.add_plugin(manifest, cx);
                });
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

fn coordinates(location: &WeatherLocation) -> String {
    let north = if location.latitude >= 0.0 { "N" } else { "S" };
    let east = if location.longitude >= 0.0 { "E" } else { "W" };
    format!(
        "{:.2}° {north}, {:.2}° {east}",
        location.latitude.abs(),
        location.longitude.abs()
    )
}

fn weather_page(
    view: &Entity<SettingsWindow>,
    city: &Entity<InputState>,
    places: &PlaceSearch,
    dock: &Dock,
    palette: Palette,
) -> Vec<AnyElement> {
    let location = &dock.location;
    let now = match &dock.weather {
        WeatherState::Ready { weather, .. } => format!(
            "{}° {}",
            weather.temperature.round() as i64,
            weather.condition.label()
        ),
        WeatherState::Loading => "Loading…".into(),
        WeatherState::Failed(_) => "Unavailable".into(),
    };
    let current = row(
        location.name.clone(),
        Some(coordinates(location).into()),
        div().text_color(palette.secondary).child(now),
        palette,
    );

    let field = div().px(px(10.0)).py(px(8.0)).child(
        div()
            .h(px(28.0))
            .px(px(8.0))
            .rounded(px(7.0))
            .bg(palette.fill)
            .flex()
            .items_center()
            .child(
                Input::new(city)
                    .appearance(false)
                    .cleanable(true)
                    .prefix(glyph(IconName::Search, 14.0, palette.secondary)),
            ),
    );
    let mut rows = vec![field.into_any_element()];
    let note = |message: SharedString, color: Hsla| {
        div()
            .px(px(12.0))
            .py(px(10.0))
            .text_color(color)
            .child(message)
            .into_any_element()
    };
    match places {
        PlaceSearch::Idle => {}
        PlaceSearch::Searching => rows.push(note("Searching…".into(), palette.secondary)),
        PlaceSearch::Failed(message) => rows.push(note(message.clone(), palette.orange)),
        PlaceSearch::Found { query, places } if places.is_empty() => rows.push(note(
            format!("No places match “{query}”.").into(),
            palette.secondary,
        )),
        PlaceSearch::Found { places, .. } => {
            rows.extend(places.iter().enumerate().map(|(index, place)| {
                let current = place.location() == *location;
                let (view, chosen) = (view.clone(), place.clone());
                let detail = place.detail();
                div()
                    .id(("place", index))
                    .test_support()
                    .mx(px(4.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .hover(|style| style.bg(palette.fill))
                    .child(glyph(IconName::MapPin, 14.0, palette.secondary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(place.name.clone())
                            .when(!detail.is_empty(), |text| {
                                text.child(
                                    div()
                                        .text_size(px(text::SUBHEADLINE))
                                        .text_color(palette.secondary)
                                        .child(detail),
                                )
                            }),
                    )
                    .when(current, |row| {
                        row.child(glyph(IconName::Check, 14.0, palette.blue))
                    })
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.choose_place(&chosen, window, cx));
                    })
                    .into_any_element()
            }));
            // Breathing room under the last result.
            rows.push(div().h(px(4.0)).into_any_element());
        }
    }

    vec![
        section(Some("Location"), vec![current], None, palette),
        // One block: the field and its results, without hairlines between.
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .px(px(4.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Change Location"),
            )
            .child(
                div()
                    .rounded(px(10.0))
                    .bg(palette.group)
                    .border_1()
                    .border_color(palette.separator)
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .child(
                div()
                    .px(px(4.0))
                    .text_size(px(text::SUBHEADLINE))
                    .text_color(palette.secondary)
                    .child("Forecasts come from Open-Meteo and refresh every 20 minutes."),
            )
            .into_any_element(),
    ]
}
