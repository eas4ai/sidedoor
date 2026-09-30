//! UI integration tests: the production views in headless windows, driven by
//! real pointer events, against an in-memory platform.

use ::platform::Platform as _;

use crate::app::dock::{Dock, InstallState, Services, UpdateState};
use crate::app::host::{Host, LoginItem, fake::FakePlatform};
use crate::ui::clipboard::{ClipboardWindow, ClipboardWindowEvent};
use crate::ui::dock::{CardChrome, CardView, DockView};
use crate::ui::settings::{SettingsEvent, SettingsWindow, Tab};
use domain::clipboard::{ClipKind, History};
use domain::config::{Appearance, Config, ItemConfig};
use domain::geometry::{self, Edge, Point};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, InputEvent as _, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Render, SharedString, Styled as _, TestAppContext, Window, WindowBounds, WindowOptions, img,
    point, px, size, test::TestWindowExt as _,
};
use plugin_host::{HostMessage, PluginMessage};
use services::weather::Place;
use std::{path::PathBuf, rc::Rc, time::Duration};

struct Harness {
    platform: Rc<FakePlatform>,
    dock: Entity<Dock>,
    dock_window: AnyWindowHandle,
    card_window: AnyWindowHandle,
}

fn options(width: f32, height: f32) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(width), px(height)),
        })),
        ..Default::default()
    }
}

fn app(bundle_id: &str) -> ItemConfig {
    ItemConfig::App {
        bundle_id: bundle_id.into(),
    }
}

fn setup(cx: &mut TestAppContext, items: Vec<ItemConfig>) -> Harness {
    let platform = Rc::new(FakePlatform::with_apps(&[
        ("com.example.alpha", "Alpha"),
        ("com.example.beta", "Beta"),
        ("com.example.gamma", "Gamma"),
        ("com.example.delta", "Delta"),
    ]));
    let config = Config {
        items,
        ..Config::starter(|_| false)
    };
    let screen = platform.main_screen().unwrap();
    let shared: Rc<dyn Host> = platform.clone();

    cx.update(|cx| {
        gpui_kit::init(cx);
        let dock = cx.new(|cx| {
            Dock::new(
                config,
                shared,
                screen,
                History::default(),
                Services { live: false },
                cx,
            )
        });
        dock.update(cx, |dock, cx| dock.start_plugins(cx));
        let frame = dock.read(cx).frame();
        let (dock_window, _) = gpui_kit::open_window(
            options(frame.width as f32, frame.height as f32),
            cx,
            |window, cx| cx.new(|cx| DockView::new(dock.clone(), window, cx)),
        )
        .unwrap();
        let chrome = cx.new(|_| CardChrome {
            placement: Some(geometry::card_placement(
                screen,
                frame,
                geometry::Edge::Right,
                0,
                300.0,
                320.0,
            )),
        });
        let (card_window, _) = gpui_kit::open_window(options(307.0, 320.0), cx, |window, cx| {
            cx.new(|cx| CardView::new(dock.clone(), chrome, window, cx))
        })
        .unwrap();
        // Headless windows start with the pointer at their origin, which is
        // on the card; park it in the arrow column instead.
        card_window
            .update(cx, |_, window, cx| move_pointer(window, 305.0, 2.0, cx))
            .unwrap();
        Harness {
            platform,
            dock,
            dock_window,
            card_window,
        }
    })
}

impl Harness {
    /// Puts the pointer at the screen edge beside the dock, as a user would.
    fn reveal(&self, cx: &mut TestAppContext) {
        let frame = cx.update(|cx| self.dock.read(cx).frame());
        *self.platform.pointer.borrow_mut() = Point {
            x: 1511.5,
            y: frame.mid_y(),
        };
        cx.update(|cx| self.dock.update(cx, |dock, cx| dock.poll_pointer(cx)));
    }

    fn open_card_id(&self, cx: &mut TestAppContext) -> Option<String> {
        cx.update(|cx| {
            self.dock
                .read(cx)
                .card()
                .map(|(_, item)| item.id.to_string())
        })
    }

    fn item_ids(&self, cx: &mut TestAppContext) -> Vec<String> {
        cx.update(|cx| {
            self.dock
                .read(cx)
                .items
                .iter()
                .map(|item| item.id.to_string())
                .collect()
        })
    }
}

/// Moves the pointer into the dock's top padding, off every item.
fn move_to_padding(window: &mut Window, cx: &mut App) {
    move_pointer(window, 30.0, 2.0, cx);
}

fn move_pointer(window: &mut Window, x: f32, y: f32, cx: &mut App) {
    window.dispatch_event(
        MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}

#[gpui_kit::test]
fn hovering_an_item_opens_its_card_until_the_pointer_leaves(cx: &mut TestAppContext) {
    let h = setup(cx, vec![app("com.example.alpha"), ItemConfig::Stats]);
    cx.run_until_parked();
    h.reveal(cx);

    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("app:com.example.alpha", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("app:com.example.alpha"));

    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("plugin:builtin.stats", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("plugin:builtin.stats"));

    // Leaving the item starts a short grace period, then the card closes.
    cx.update_window(h.dock_window, |_, window, cx| move_to_padding(window, cx))
        .unwrap();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("plugin:builtin.stats"));
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    assert_eq!(h.open_card_id(cx), None);
}

#[gpui_kit::test]
fn the_dock_hides_even_if_the_pointer_left_without_a_hover_event(cx: &mut TestAppContext) {
    let h = setup(cx, vec![app("com.example.alpha"), app("com.example.beta")]);
    cx.run_until_parked();
    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("app:com.example.alpha", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("app:com.example.alpha"));

    // A quick flick out of the panel: the window never hears the pointer
    // leave, so only the polled position says it is gone. It is 200 points
    // from the dock, well clear of the tooltip.
    let dock = cx.update(|cx| h.dock.read(cx).frame());
    *h.platform.pointer.borrow_mut() = Point {
        x: dock.x - 200.0,
        y: dock.mid_y(),
    };
    for _ in 0..8 {
        cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pointer(cx)));
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    assert_eq!(h.open_card_id(cx), None);
    assert!(!cx.update(|cx| h.dock.read(cx).is_shown()));
}

#[gpui_kit::test]
fn moving_onto_the_card_keeps_it_open(cx: &mut TestAppContext) {
    let h = setup(cx, vec![ItemConfig::Clipboard]);
    cx.run_until_parked();
    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("plugin:builtin.clipboard", cx);
        move_to_padding(window, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("card", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        h.open_card_id(cx).as_deref(),
        Some("plugin:builtin.clipboard")
    );
}

#[gpui_kit::test]
fn an_armed_clear_expires(cx: &mut TestAppContext) {
    let h = setup(cx, vec![ItemConfig::Clipboard]);
    h.platform.copy(ClipKind::from_text("keep me".into()));
    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.poll_pasteboard(cx);
            dock.request_clear_history(cx);
        })
    });
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.request_clear_history(cx)));
    assert_eq!(cx.update(|cx| h.dock.read(cx).history.len()), 1);
}

#[gpui_kit::test]
fn clicking_an_app_opens_it(cx: &mut TestAppContext) {
    let h = setup(cx, vec![app("com.example.beta")]);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("app:com.example.beta", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        *h.platform.opened.borrow(),
        vec![PathBuf::from("/Applications/Beta.app")]
    );
    assert!(cx.update(|cx| h.dock.read(cx).running.contains("com.example.beta")));
}

#[gpui_kit::test]
fn dragging_reorders_and_saves(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![
            app("com.example.alpha"),
            app("com.example.beta"),
            app("com.example.gamma"),
        ],
    );
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to("app:com.example.alpha", "app:com.example.gamma", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        h.item_ids(cx),
        [
            "app:com.example.beta",
            "app:com.example.gamma",
            "app:com.example.alpha"
        ]
    );
    let saved = h.platform.saved_configs.borrow();
    assert_eq!(
        saved.last().map(|config| config.items.clone()),
        Some(vec![
            app("com.example.beta"),
            app("com.example.gamma"),
            app("com.example.alpha"),
        ])
    );
}

#[gpui_kit::test]
fn dragging_parts_the_items_under_the_pointer_like_the_dock(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![
            app("com.example.alpha"),
            app("com.example.beta"),
            app("com.example.gamma"),
            app("com.example.delta"),
        ],
    );
    let center = |cx: &mut TestAppContext, id: &str| {
        let id = SharedString::from(format!("app:com.example.{id}"));
        cx.update_window(h.dock_window, |_, window, cx| {
            window.render_frame(cx);
            window.find(id).bounds().center()
        })
        .unwrap()
    };
    let (alpha, beta, gamma, delta) = (
        center(cx, "alpha"),
        center(cx, "beta"),
        center(cx, "gamma"),
        center(cx, "delta"),
    );
    let pointer = |cx: &mut TestAppContext, to: gpui_kit::Point<gpui_kit::Pixels>| {
        cx.update_window(h.dock_window, |_, window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: to,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Modifiers::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        })
        .unwrap();
    };

    // Pick up alpha and hold it over gamma.
    cx.update_window(h.dock_window, |_, window, cx| {
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: alpha,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    for step in 1..=8 {
        let t = step as f32 / 8.0;
        pointer(
            cx,
            point(
                alpha.x + (gamma.x - alpha.x) * t,
                alpha.y + (gamma.y - alpha.y) * t,
            ),
        );
    }
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    pointer(cx, gamma);

    // Beta and gamma have slid up to close alpha's spot and open a gap
    // under the pointer; delta stays. Nothing is saved yet.
    let near = |a: gpui_kit::Point<gpui_kit::Pixels>, b: gpui_kit::Point<gpui_kit::Pixels>| {
        (f32::from(a.x) - f32::from(b.x)).abs() < 1.0
            && (f32::from(a.y) - f32::from(b.y)).abs() < 1.0
    };
    assert!(
        near(center(cx, "beta"), alpha),
        "beta should move into alpha's slot"
    );
    assert!(
        near(center(cx, "gamma"), beta),
        "gamma should move into beta's slot"
    );
    assert!(near(center(cx, "delta"), delta));
    assert!(h.platform.saved_configs.borrow().is_empty());

    // Letting go drops alpha into the gap, and nothing jumps: each item is
    // already where its new slot is.
    cx.update_window(h.dock_window, |_, window, cx| {
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: gamma,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        h.item_ids(cx),
        [
            "app:com.example.beta",
            "app:com.example.gamma",
            "app:com.example.alpha",
            "app:com.example.delta"
        ]
    );
    assert!(near(center(cx, "beta"), alpha));
    assert!(near(center(cx, "alpha"), gamma));
}

#[gpui_kit::test]
fn dropped_apps_are_added_once_and_others_ignored(cx: &mut TestAppContext) {
    let h = setup(cx, vec![app("com.example.alpha"), ItemConfig::Weather]);
    let added = cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.add_paths(
                &[
                    PathBuf::from("/Applications/Delta.app"),
                    PathBuf::from("/Applications/Alpha.app"),
                    PathBuf::from("/Users/me/notes.txt"),
                ],
                Some(1),
                cx,
            )
        })
    });
    assert_eq!(added, 1);
    assert_eq!(
        h.item_ids(cx),
        [
            "app:com.example.alpha",
            "app:com.example.delta",
            "plugin:builtin.weather"
        ]
    );

    cx.update(|cx| {
        h.dock
            .update(cx, |dock, cx| dock.remove("plugin:builtin.weather", cx))
    });
    assert_eq!(
        h.item_ids(cx),
        ["app:com.example.alpha", "app:com.example.delta"]
    );
    assert_eq!(h.platform.saved_configs.borrow().len(), 2);
}

#[gpui_kit::test]
fn copies_show_in_the_card_and_click_to_copy_back(cx: &mut TestAppContext) {
    let h = setup(cx, vec![ItemConfig::Clipboard]);
    h.platform.copy(ClipKind::from_text("Standup notes".into()));
    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));
    h.platform.copy(ClipKind::from_text(
        "https://github.com/zed-industries".into(),
    ));
    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));
    // Nothing new on the pasteboard: nothing recorded.
    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));

    let (newest, oldest) = cx.update(|cx| {
        let history = &h.dock.read(cx).history;
        assert_eq!(history.len(), 2);
        (history.entries[0].id, history.entries[1].id)
    });

    cx.run_until_parked();
    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("plugin:builtin.clipboard", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find(gpui_kit::SharedString::from(format!(
                    "plugin:builtin.clipboard:clip:{newest}"
                )))
                .visible()
        );
        window.click(
            gpui_kit::SharedString::from(format!("plugin:builtin.clipboard:clip:{oldest}")),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();

    assert_eq!(
        *h.platform.written.borrow(),
        vec![ClipKind::Text {
            text: "Standup notes".into()
        }]
    );
    let top = cx.update(|cx| h.dock.read(cx).history.entries[0].id);
    assert_eq!(top, oldest);

    // One click only arms Clear History; the second clears.
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin:builtin.clipboard:clear-history", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| h.dock.read(cx).history.len()), 2);
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin:builtin.clipboard:clear-history", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| h.dock.read(cx).history.len()), 0);
}

// MARK: Clipboard History window

struct HistoryHarness {
    platform: Rc<FakePlatform>,
    dock: Entity<Dock>,
    window: AnyWindowHandle,
    view: Entity<ClipboardWindow>,
    events: Rc<std::cell::RefCell<Vec<ClipboardWindowEvent>>>,
}

fn open_history(cx: &mut TestAppContext, copies: &[&str]) -> HistoryHarness {
    let h = setup(cx, vec![ItemConfig::Clipboard]);
    for copy in copies {
        h.platform.copy(ClipKind::from_text((*copy).into()));
        cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));
    }
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let (window, view) = cx.update(|cx| {
        let (window, view) = gpui_kit::open_window(options(780.0, 500.0), cx, |window, cx| {
            cx.new(|cx| ClipboardWindow::new(h.dock.clone(), window, cx))
        })
        .unwrap();
        let log = events.clone();
        cx.subscribe(&view, move |_, event: &ClipboardWindowEvent, _| {
            log.borrow_mut().push(*event);
        })
        .detach();
        (window, view)
    });
    HistoryHarness {
        platform: h.platform,
        dock: h.dock,
        window,
        view,
        events,
    }
}

impl HistoryHarness {
    fn selected_title(&self, cx: &mut TestAppContext) -> Option<String> {
        cx.update(|cx| {
            let id = self.view.read(cx).selected()?;
            let dock = self.dock.read(cx);
            let entry = dock.history.entries.iter().find(|entry| entry.id == id)?;
            Some(entry.kind.title())
        })
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        let key = if domain::shortcut::PC_KEYS {
            key.replace("cmd-", "ctrl-")
        } else {
            key.to_owned()
        };
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.press(&key, cx);
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn history_window_filters_as_you_type_and_moves_with_arrows(cx: &mut TestAppContext) {
    let h = open_history(cx, &["alpha notes", "beta link list", "gamma notes"]);
    // Newest first, and the newest is selected.
    assert_eq!(h.selected_title(cx).as_deref(), Some("gamma notes"));

    h.press(cx, "down");
    assert_eq!(h.selected_title(cx).as_deref(), Some("beta link list"));
    h.press(cx, "up");
    h.press(cx, "up");
    assert_eq!(h.selected_title(cx).as_deref(), Some("gamma notes"));

    cx.update_window(h.window, |_, window, cx| window.input("notes", cx))
        .unwrap();
    // The search field reports the change once that update has finished.
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("history-row", 2u64)).is_none());
        assert!(window.find(("history-row", 1u64)).visible());
    })
    .unwrap();
    cx.run_until_parked();
    h.press(cx, "down");
    assert_eq!(h.selected_title(cx).as_deref(), Some("alpha notes"));

    // Escape clears the search first, then closes.
    h.press(cx, "escape");
    assert!(h.events.borrow().is_empty());
    h.press(cx, "escape");
    assert_eq!(
        *h.events.borrow(),
        vec![ClipboardWindowEvent::Dismiss { copied: false }]
    );
}

#[gpui_kit::test]
fn history_window_copies_with_return_and_deletes_with_cmd_backspace(cx: &mut TestAppContext) {
    let h = open_history(cx, &["first", "second", "third"]);
    h.press(cx, "down");
    h.press(cx, "cmd-backspace");
    assert_eq!(cx.update(|cx| h.dock.read(cx).history.len()), 2);
    // The next entry down takes the selection.
    assert_eq!(h.selected_title(cx).as_deref(), Some("first"));

    h.press(cx, "enter");
    assert_eq!(
        *h.platform.written.borrow(),
        vec![ClipKind::Text {
            text: "first".into()
        }]
    );
    assert_eq!(
        *h.events.borrow(),
        vec![ClipboardWindowEvent::Dismiss { copied: true }]
    );
}

#[gpui_kit::test]
fn history_window_filter_segments_narrow_by_type(cx: &mut TestAppContext) {
    let h = open_history(cx, &["plain words", "https://example.com/page"]);
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        // Filters: All, Text, Links, Images, Files.
        window.click(("filter", 2usize), cx);
        window.render_frame(cx);
        assert!(window.try_find(("history-row", 1u64)).is_none());
        assert!(window.find(("history-row", 2u64)).visible());
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        h.selected_title(cx).as_deref(),
        Some("https://example.com/page")
    );
}

#[gpui_kit::test]
fn image_previews_fit_their_frame(cx: &mut TestAppContext) {
    let h = open_history(cx, &[]);
    h.platform.copy(ClipKind::Image {
        path: "/tmp/sidedoor-missing.png".into(),
        width: 512,
        height: 512,
    });
    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        let frame = window.find("preview-frame").bounds();
        let image = window.find("preview-image").bounds();
        assert!(image.size.height <= frame.size.height);
        assert!(image.size.width <= frame.size.width);
        // Square in, square out, centered in the frame.
        assert_eq!(image.size.width, image.size.height);
        let center = |b: gpui_kit::Bounds<gpui_kit::Pixels>| b.center();
        assert!((center(image).y - center(frame).y).abs() < px(1.0));
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn show_all_opens_the_history_window(cx: &mut TestAppContext) {
    let h = setup(cx, vec![ItemConfig::Clipboard]);
    h.platform.copy(ClipKind::from_text("hello".into()));
    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));
    let opened = Rc::new(std::cell::Cell::new(0));
    let count = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&h.dock, move |_, event: &crate::app::dock::DockEvent, _| {
            if *event == crate::app::dock::DockEvent::OpenClipboardHistory {
                count.set(count.get() + 1);
            }
        })
        .detach();
    });
    cx.run_until_parked();
    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("plugin:builtin.clipboard", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin:builtin.clipboard:show-all-history", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(opened.get(), 1);
    // Clicking the clipboard tile itself opens it too.
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin:builtin.clipboard", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(opened.get(), 2);
}

// MARK: Shortcuts

#[gpui_kit::test]
fn shortcuts_open_apps_history_and_peek_at_widgets(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![
            app("com.example.alpha"),
            ItemConfig::Weather,
            ItemConfig::Clipboard,
        ],
    );
    let opened = Rc::new(std::cell::Cell::new(0));
    let count = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&h.dock, move |_, event: &crate::app::dock::DockEvent, _| {
            if *event == crate::app::dock::DockEvent::OpenClipboardHistory {
                count.set(count.get() + 1);
            }
        })
        .detach();
    });

    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.trigger_shortcut("app:com.example.alpha", cx)
        })
    });
    assert_eq!(
        *h.platform.opened.borrow(),
        vec![PathBuf::from("/Applications/Alpha.app")]
    );

    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.trigger_shortcut("plugin:builtin.clipboard", cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(opened.get(), 1);

    // A widget shortcut shows the dock with that widget's card.
    assert!(!cx.update(|cx| h.dock.read(cx).is_shown()));
    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.trigger_shortcut("plugin:builtin.weather", cx)
        })
    });
    assert!(cx.update(|cx| h.dock.read(cx).is_shown()));
    assert_eq!(
        h.open_card_id(cx).as_deref(),
        Some("plugin:builtin.weather")
    );
}

#[gpui_kit::test]
fn assigned_shortcuts_are_saved_and_follow_their_item(cx: &mut TestAppContext) {
    let h = setup(cx, vec![app("com.example.alpha"), app("com.example.beta")]);
    let shortcut = domain::shortcut::Shortcut::parse("alt-cmd-a").unwrap();
    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.set_shortcut("app:com.example.alpha", Some(shortcut.clone()), cx)
        })
    });
    let saved = h.platform.saved_configs.borrow().last().cloned().unwrap();
    assert_eq!(
        saved
            .shortcuts
            .get("app:com.example.alpha")
            .map(String::as_str),
        Some("alt-cmd-a")
    );
    assert_eq!(
        cx.update(|cx| h
            .dock
            .read(cx)
            .shortcut_owner(&shortcut, "app:com.example.beta")),
        Some("Alpha".to_string())
    );

    // Removing the item drops its shortcut too.
    cx.update(|cx| {
        h.dock
            .update(cx, |dock, cx| dock.remove("app:com.example.alpha", cx))
    });
    let saved = h.platform.saved_configs.borrow().last().cloned().unwrap();
    assert!(!saved.shortcuts.contains_key("app:com.example.alpha"));
}

fn open_recorder(
    cx: &mut TestAppContext,
    current: Option<&str>,
    taken: &'static str,
) -> (
    AnyWindowHandle,
    Entity<crate::ui::shortcut_recorder::ShortcutRecorder>,
    Rc<std::cell::RefCell<Vec<crate::ui::shortcut_recorder::RecorderEvent>>>,
) {
    use crate::ui::shortcut_recorder::*;
    use domain::shortcut::Shortcut;
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let current = current.and_then(Shortcut::parse);
    let (window, view) = cx.update(|cx| {
        gpui_kit::init(cx);
        let check: ConflictCheck = Rc::new(move |shortcut, _| {
            if shortcut.to_config() == taken {
                Err(format!("Another app already uses {shortcut}."))
            } else {
                Ok(())
            }
        });
        let (window, view) = gpui_kit::open_window(options(400.0, 290.0), cx, |window, cx| {
            cx.new(|cx| {
                ShortcutRecorder::new(
                    "Safari",
                    None,
                    "icons/app-window.svg".into(),
                    current,
                    check,
                    window,
                    cx,
                )
            })
        })
        .unwrap();
        let log = events.clone();
        cx.subscribe(&view, move |_, event: &RecorderEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach();
        (window, view)
    });
    (window, view, events)
}

#[gpui_kit::test]
fn recorder_saves_a_valid_combination_with_return(cx: &mut TestAppContext) {
    use crate::ui::shortcut_recorder::RecorderEvent;
    use domain::shortcut::Shortcut;
    let (window, view, events) = open_recorder(cx, None, "ctrl-alt-x");
    let press = |cx: &mut TestAppContext, key: &str| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.press(key, cx);
        })
        .unwrap();
    };

    // A bare letter isn't a shortcut, and Return doesn't save it.
    press(cx, "k");
    assert!(cx.update(|cx| view.read(cx).ready().is_none()));
    press(cx, "enter");
    assert!(events.borrow().is_empty());

    // Taken elsewhere: reported, not saved.
    press(cx, "ctrl-alt-x");
    assert!(cx.update(|cx| view.read(cx).ready().is_none()));

    press(cx, "alt-cmd-s");
    press(cx, "enter");
    assert_eq!(
        *events.borrow(),
        vec![RecorderEvent::Save(Shortcut::parse("alt-cmd-s").unwrap())]
    );
}

#[gpui_kit::test]
fn recorder_cancels_with_escape_and_removes_existing(cx: &mut TestAppContext) {
    use crate::ui::shortcut_recorder::RecorderEvent;
    let (window, _, events) = open_recorder(cx, Some("ctrl-cmd-v"), "");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("remove-shortcut", cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        *events.borrow(),
        vec![RecorderEvent::Remove, RecorderEvent::Cancel]
    );
}

// MARK: Settings

struct SettingsHarness {
    platform: Rc<FakePlatform>,
    dock: Entity<Dock>,
    window: AnyWindowHandle,
    view: Entity<SettingsWindow>,
    events: Rc<std::cell::RefCell<Vec<SettingsEvent>>>,
}

fn place(name: &str, region: &str, latitude: f64) -> Place {
    Place {
        name: name.into(),
        latitude,
        longitude: 10.0,
        region: Some(region.into()),
        country: Some("Denmark".into()),
    }
}

fn open_settings(cx: &mut TestAppContext, items: Vec<ItemConfig>) -> SettingsHarness {
    let h = setup(cx, items);
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let lookup: crate::ui::settings::PlaceLookup = std::sync::Arc::new(|query: &str| {
        if query.eq_ignore_ascii_case("aal") {
            Ok(vec![
                place("Aalborg", "North Denmark", 57.05),
                place("Aalestrup", "North Denmark", 56.69),
            ])
        } else {
            Ok(Vec::new())
        }
    });
    let (window, view) = cx.update(|cx| {
        let (width, height) = crate::ui::settings::WINDOW_SIZE;
        let (window, view) = gpui_kit::open_window(options(width, height), cx, |window, cx| {
            cx.new(|cx| SettingsWindow::new(h.dock.clone(), lookup, window, cx))
        })
        .unwrap();
        let log = events.clone();
        cx.subscribe(&view, move |_, event: &SettingsEvent, _| {
            log.borrow_mut().push(*event);
        })
        .detach();
        (window, view)
    });
    SettingsHarness {
        platform: h.platform,
        dock: h.dock,
        window,
        view,
        events,
    }
}

/// Scrolls the settings page until `id` is on screen, as a user would.
fn scroll_into_view(window: &mut Window, id: &gpui_kit::ElementId, cx: &mut App) {
    for _ in 0..40 {
        let Some(target) = window.try_find(id.clone()) else {
            return;
        };
        let page = window.find("settings-page");
        let bounds = target.bounds();
        let inside =
            bounds.top() >= page.bounds().top() && bounds.bottom() <= page.bounds().bottom();
        if target.visible() && inside {
            return;
        }
        let step = if target.bounds().top() > page.bounds().top() {
            -80.0
        } else {
            80.0
        };
        window.scroll(
            "settings-page",
            gpui_kit::ScrollDelta::Pixels(point(px(0.0), px(step))),
            cx,
        );
        window.render_frame(cx);
    }
}

impl SettingsHarness {
    fn click(&self, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) {
        let id = id.into();
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            scroll_into_view(window, &id, cx);
            window.click(id, cx);
        })
        .unwrap();
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        let key = if domain::shortcut::PC_KEYS {
            key.replace("cmd-", "ctrl-")
        } else {
            key.to_owned()
        };
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.press(&key, cx);
        })
        .unwrap();
    }

    fn tab(&self, cx: &mut TestAppContext) -> Tab {
        cx.update(|cx| self.view.read(cx).tab())
    }

    fn item_ids(&self, cx: &mut TestAppContext) -> Vec<String> {
        cx.update(|cx| {
            self.dock
                .read(cx)
                .items
                .iter()
                .map(|item| item.id.to_string())
                .collect()
        })
    }
}

#[gpui_kit::test]
fn settings_tabs_switch_by_click_and_command_number(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![ItemConfig::Weather]);
    assert_eq!(h.tab(cx), Tab::General);
    h.click(cx, ("tab", 2usize));
    assert_eq!(h.tab(cx), Tab::Items);
    h.press(cx, "cmd-4");
    assert_eq!(h.tab(cx), Tab::Plugins);
    h.press(cx, "cmd-5");
    assert_eq!(h.tab(cx), Tab::Weather);
    h.press(cx, "cmd-2");
    assert_eq!(h.tab(cx), Tab::Dock);
    assert!(h.events.borrow().is_empty());
    h.press(cx, "cmd-w");
    assert_eq!(*h.events.borrow(), vec![SettingsEvent::Dismiss]);
}

#[gpui_kit::test]
fn settings_pages_draw_without_asking_the_system(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![ItemConfig::Weather]);
    let before = h.platform.slow_queries.get();
    for key in ["cmd-1", "cmd-4", "cmd-1", "cmd-4"] {
        h.press(cx, key);
        cx.update_window(h.window, |_, window, cx| {
            for _ in 0..3 {
                window.refresh();
                window.render_frame(cx);
            }
        })
        .unwrap();
    }
    // The General page shows the login-item status and the Plugins page
    // lists the plugins folder; both come from the last check, not from a
    // system call per frame.
    assert_eq!(h.platform.slow_queries.get(), before);
}

#[gpui_kit::test]
fn builtins_can_be_added_from_plugins_and_removed_like_other_items(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![]);
    h.press(cx, "cmd-4");
    h.click(cx, "plugin-dock:builtin.clipboard");
    cx.run_until_parked();
    assert_eq!(h.item_ids(cx), ["plugin:builtin.clipboard"]);
    cx.update(|cx| {
        let dock = h.dock.read(cx);
        let state = dock.plugin(crate::builtins::CLIPBOARD).unwrap();
        assert!(state.problem.is_none());
        assert!(state.tile.is_some() && state.card.is_some());
    });
    h.press(cx, "cmd-3");
    h.click(cx, "remove:plugin:builtin.clipboard");
    assert!(h.item_ids(cx).is_empty());
    assert!(cx.update(|cx| h.dock.read(cx).plugin(crate::builtins::CLIPBOARD).is_none()));
}

#[gpui_kit::test]
fn dock_settings_move_the_edge_and_change_the_theme(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![app("com.example.alpha")]);
    h.press(cx, "cmd-2");
    h.click(cx, "edge:Left");
    cx.update(|cx| {
        let dock = h.dock.read(cx);
        assert_eq!(dock.edge, Edge::Left);
        // It shows itself at the new edge.
        assert!(dock.is_shown());
        assert!(dock.frame().x < 100.0);
    });
    h.click(cx, "theme:Dark");
    assert_eq!(*h.platform.appearance.borrow(), Some(Appearance::Dark));
    let saved = h.platform.saved_configs.borrow();
    let last = saved.last().unwrap();
    assert_eq!(last.edge, Edge::Left);
    assert_eq!(last.appearance, Appearance::Dark);
}

#[gpui_kit::test]
fn general_settings_toggle_launch_at_login(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![ItemConfig::Clipboard]);
    h.click(cx, "launch-at-login");
    assert_eq!(*h.platform.login.borrow(), LoginItem::On);
    h.click(cx, "launch-at-login");
    assert_eq!(*h.platform.login.borrow(), LoginItem::Off);
}

struct UpdateBackend {
    checks: std::sync::atomic::AtomicUsize,
    downloads: std::sync::atomic::AtomicUsize,
    fail_download: bool,
    staged: std::sync::Mutex<Option<PathBuf>>,
}

impl services::updates::Backend for UpdateBackend {
    fn check(&self, _: &str) -> Result<Option<services::updates::Release>, String> {
        self.checks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Some(services::updates::Release {
            version: "2.0.0".into(), notes: "A new dock update".into(),
            installation: services::updates::Installation {
                owner: services::updates::Owner::MacBundle, os: "macos".into(), arch: "arm64".into(),
                root: PathBuf::from("/Applications/Sidedoor.app"), executable: PathBuf::from("/Applications/Sidedoor.app/Contents/MacOS/sidedoor"),
            },
            asset: services::updates::ManifestAsset { name: "Sidedoor-macos-arm64.zip".into(), size: 42, sha256: "a".repeat(64) },
            url: "https://github.com/lassejlv/sidedoor/releases/download/v2.0.0/Sidedoor-macos-arm64.zip".into(),
        }))
    }
    fn prepare(
        &self,
        release: &services::updates::Release,
    ) -> Result<services::updates::Prepared, String> {
        self.downloads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail_download {
            return Err("Update verification failed. Nothing was installed.".into());
        }
        let staging = tempfile::tempdir().unwrap();
        *self.staged.lock().unwrap() = Some(staging.path().to_path_buf());
        Ok(services::updates::Prepared {
            release: release.clone(),
            payload: staging.path().join("Sidedoor.app"),
            staging,
        })
    }
    fn launch(&self, _: services::updates::Prepared) -> Result<(), String> {
        Err("The installation folder isn't writable.".into())
    }
}

fn update_backend(
    h: &SettingsHarness,
    fail_download: bool,
    cx: &mut TestAppContext,
) -> std::sync::Arc<UpdateBackend> {
    let backend = std::sync::Arc::new(UpdateBackend {
        checks: std::sync::atomic::AtomicUsize::new(0),
        downloads: std::sync::atomic::AtomicUsize::new(0),
        fail_download,
        staged: std::sync::Mutex::new(None),
    });
    cx.update(|cx| {
        h.dock
            .read(cx)
            .app_updates
            .clone()
            .update(cx, |updates, _| updates.use_test_backend(backend.clone()))
    });
    backend
}

#[gpui_kit::test]
fn automatic_app_updates_check_after_startup_and_every_six_hours(cx: &mut TestAppContext) {
    use crate::app::updates::AppUpdates;
    use std::sync::atomic::Ordering;
    let h = open_settings(cx, vec![]);
    let backend = update_backend(&h, false, cx);
    let updates = cx.update(|cx| {
        cx.new(|cx| AppUpdates::with_backend(true, true, h.platform.clone(), backend.clone(), cx))
    });
    cx.run_until_parked();
    assert_eq!(backend.checks.load(Ordering::SeqCst), 0);
    cx.executor().advance_clock(Duration::from_secs(10));
    cx.run_until_parked();
    assert_eq!(backend.checks.load(Ordering::SeqCst), 1);
    cx.executor()
        .advance_clock(Duration::from_secs(6 * 60 * 60));
    cx.run_until_parked();
    assert_eq!(backend.checks.load(Ordering::SeqCst), 2);
    cx.update(|cx| updates.update(cx, |updates, _| updates.automatic = false));
    cx.executor()
        .advance_clock(Duration::from_secs(6 * 60 * 60));
    cx.run_until_parked();
    assert_eq!(backend.checks.load(Ordering::SeqCst), 2);
    // A manual check still works when automatic checks are disabled, and
    // repeated requests don't start overlapping operations.
    cx.update(|cx| {
        updates.update(cx, |updates, cx| {
            updates.check(false, cx);
            updates.check(false, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(backend.checks.load(Ordering::SeqCst), 3);
    cx.update(|cx| updates.update(cx, |updates, cx| updates.download(cx)));
    cx.run_until_parked();
    cx.update(|cx| updates.update(cx, |updates, _| updates.automatic = true));
    cx.executor()
        .advance_clock(Duration::from_secs(6 * 60 * 60));
    cx.run_until_parked();
    assert_eq!(backend.checks.load(Ordering::SeqCst), 3);
}

#[gpui_kit::test]
fn app_updates_check_download_discard_and_keep_failed_install_usable(cx: &mut TestAppContext) {
    use crate::app::updates::State;
    use std::sync::atomic::Ordering;
    let h = open_settings(cx, vec![]);
    let backend = update_backend(&h, false, cx);
    h.click(cx, "app-update-action");
    cx.run_until_parked();
    assert_eq!(backend.checks.load(Ordering::SeqCst), 1);
    cx.update(|cx| {
        assert!(matches!(
            h.dock.read(cx).app_updates.read(cx).state,
            State::Available(_)
        ))
    });
    h.click(cx, "app-update-action");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            h.dock.read(cx).app_updates.read(cx).state,
            State::Ready(_)
        ))
    });
    let staged = backend.staged.lock().unwrap().clone().unwrap();
    assert!(staged.exists());
    h.click(cx, "discard-app-update");
    cx.run_until_parked();
    assert!(!staged.exists());
    cx.update(|cx| {
        assert!(matches!(
            h.dock.read(cx).app_updates.read(cx).state,
            State::Available(_)
        ))
    });
    h.click(cx, "app-update-action");
    cx.run_until_parked();
    h.click(cx, "app-update-action");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            h.dock.read(cx).app_updates.read(cx).state,
            State::Failed(_)
        ))
    });
    assert_eq!(backend.downloads.load(Ordering::SeqCst), 2);
    // The app and its settings remain alive after the helper can't start.
    h.press(cx, "cmd-2");
    assert_eq!(h.tab(cx), Tab::Dock);
}

#[gpui_kit::test]
fn app_update_failures_are_retryable_and_automatic_checks_are_persisted(cx: &mut TestAppContext) {
    use crate::app::updates::State;
    let h = open_settings(cx, vec![]);
    update_backend(&h, true, cx);
    h.click(cx, "automatic-app-updates");
    assert!(
        !h.platform
            .saved_configs
            .borrow()
            .last()
            .unwrap()
            .automatically_check_for_updates
    );
    h.click(cx, "app-update-action");
    cx.run_until_parked();
    h.click(cx, "app-update-action");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            h.dock.read(cx).app_updates.read(cx).state,
            State::Failed(_)
        ))
    });
    h.click(cx, "app-update-action");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            h.dock.read(cx).app_updates.read(cx).state,
            State::Available(_)
        ))
    });
}

#[gpui_kit::test]
fn item_settings_remove_add_and_reorder(cx: &mut TestAppContext) {
    let h = open_settings(
        cx,
        vec![
            app("com.example.alpha"),
            app("com.example.beta"),
            ItemConfig::Weather,
        ],
    );
    h.press(cx, "cmd-3");
    h.click(cx, "remove:app:com.example.beta");
    assert_eq!(
        h.item_ids(cx),
        ["app:com.example.alpha", "plugin:builtin.weather"]
    );

    // Only missing widgets are offered.
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("add-widget:Weather").is_none());
    })
    .unwrap();
    cx.run_until_parked();
    h.click(cx, "add-widget:Stats");
    assert_eq!(
        h.item_ids(cx),
        [
            "app:com.example.alpha",
            "plugin:builtin.weather",
            "plugin:builtin.stats"
        ]
    );

    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(
            "item-row:app:com.example.alpha",
            "item-row:plugin:builtin.stats",
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        h.item_ids(cx),
        [
            "plugin:builtin.weather",
            "plugin:builtin.stats",
            "app:com.example.alpha"
        ]
    );
    let saved = h.platform.saved_configs.borrow();
    assert_eq!(
        saved.last().map(|config| config.items.clone()),
        Some(vec![
            ItemConfig::Plugin {
                id: crate::builtins::WEATHER.into()
            },
            ItemConfig::Plugin {
                id: crate::builtins::STATS.into()
            },
            app("com.example.alpha")
        ])
    );
}

#[gpui_kit::test]
fn weather_settings_search_and_choose_a_place(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![ItemConfig::Weather]);
    h.press(cx, "cmd-5");
    cx.update_window(h.window, |_, window, cx| {
        h.view.update(cx, |view, cx| view.focus_city(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(h.window, |_, window, cx| window.input("Aal", cx))
        .unwrap();
    // Nothing is looked up until typing pauses.
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("place", 0usize)).is_none());
    })
    .unwrap();
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(350));
    cx.run_until_parked();

    h.click(cx, ("place", 0usize));
    cx.update(|cx| {
        let dock = h.dock.read(cx);
        assert_eq!(dock.location.name, "Aalborg");
        assert_eq!(dock.location.latitude, 57.05);
    });
    assert_eq!(
        h.platform
            .saved_configs
            .borrow()
            .last()
            .map(|config| config.weather.name.clone()),
        Some("Aalborg".into())
    );
    // The search resets once a place is chosen.
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("place", 0usize)).is_none());
    })
    .unwrap();
    cx.run_until_parked();
}

// MARK: Plugins

fn plugin_says(h: &Harness, cx: &mut TestAppContext, id: &str, message: PluginMessage) {
    h.platform.plugin_says(id, message);
    cx.run_until_parked();
}

fn render(surface: &str, tree: serde_json::Value) -> PluginMessage {
    PluginMessage::Render {
        surface: surface.into(),
        tree: serde_json::from_value(tree).unwrap(),
    }
}

#[gpui_kit::test]
fn builtins_keep_card_sizes_and_receive_only_their_requested_data(cx: &mut TestAppContext) {
    use plugin_host::DataSource;
    let h = setup(
        cx,
        vec![
            ItemConfig::Weather,
            ItemConfig::Stats,
            ItemConfig::Clipboard,
            ItemConfig::Plugin {
                id: "counter".into(),
            },
        ],
    );
    cx.run_until_parked();
    for (id, source) in [
        (crate::builtins::WEATHER, DataSource::Weather),
        (crate::builtins::STATS, DataSource::Stats),
        (crate::builtins::CLIPBOARD, DataSource::Clipboard),
    ] {
        let messages = sent(&h, id);
        assert!(messages.iter().any(
            |message| matches!(message, HostMessage::Data { source: got, .. } if *got == source)
        ));
        assert!(!messages.iter().any(
            |message| matches!(message, HostMessage::Data { source: got, .. } if *got != source)
        ));
    }
    assert!(
        !sent(&h, "counter")
            .iter()
            .any(|message| matches!(message, HostMessage::Data { .. }))
    );
    cx.update(|cx| {
        let dock = h.dock.read(cx);
        assert_eq!(
            crate::ui::dock::card_size(&dock.items[0], dock, |_| 0.0),
            (300.0, 190.0)
        );
        assert_eq!(
            crate::ui::dock::card_size(&dock.items[1], dock, |_| 0.0),
            (300.0, 206.0)
        );
    });
    for index in 0..7 {
        h.platform
            .copy(ClipKind::from_text(format!("Copy {index}")));
        cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));
    }
    cx.run_until_parked();
    open_plugin_card(&h, cx, "plugin:builtin.clipboard");
    cx.run_until_parked();
    cx.update(|cx| {
        let dock = h.dock.read(cx);
        assert_eq!(
            dock.plugin(crate::builtins::CLIPBOARD).unwrap().height,
            Some(322.0)
        );
        assert_eq!(
            crate::ui::dock::card_size(&dock.items[2], dock, |_| 0.0),
            (300.0, 322.0)
        );
    });
    // Model notifications and rendering must not resend an unchanged feed.
    let before = sent(&h, crate::builtins::CLIPBOARD)
        .iter()
        .filter(|m| matches!(m, HostMessage::Data { .. }))
        .count();
    cx.update(|cx| h.dock.update(cx, |_, cx| cx.notify()));
    cx.run_until_parked();
    let after = sent(&h, crate::builtins::CLIPBOARD)
        .iter()
        .filter(|m| matches!(m, HostMessage::Data { .. }))
        .count();
    assert_eq!(before, after);
}

/// Events the host sent to plugin `id`, as (handler, value).
fn events(h: &Harness, id: &str) -> Vec<(String, serde_json::Value)> {
    h.platform
        .plugin_sent
        .borrow()
        .iter()
        .filter(|(plugin, _)| plugin == id)
        .filter_map(|(_, message)| match message {
            HostMessage::Event { handler, value } => Some((handler.clone(), value.clone())),
            _ => None,
        })
        .collect()
}

fn sent(h: &Harness, id: &str) -> Vec<HostMessage> {
    h.platform
        .plugin_sent
        .borrow()
        .iter()
        .filter(|(plugin, _)| plugin == id)
        .map(|(_, message)| message.clone())
        .collect()
}

fn open_plugin_card(h: &Harness, cx: &mut TestAppContext, item: &str) {
    cx.run_until_parked();
    h.reveal(cx);
    let index = h.item_ids(cx).iter().position(|id| id == item).unwrap();
    cx.update(|cx| {
        h.dock
            .update(cx, |dock, cx| dock.set_item_hovered(index, true, cx))
    });
    cx.update_window(h.card_window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn plugins_draw_native_components_with_custom_styles(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    plugin_says(
        &h,
        cx,
        "counter",
        render(
            "card",
            serde_json::json!([{ "t": "Card", "p": { "title": "Counter" }, "c": [
                { "t": "div", "p": { "id": "box", "w": 120, "h": 30, "bg": "blue", "rounded": 8,
                                     "on_click": { "$h": "card:W/box#on_click" } }, "c": ["3"] },
                { "t": "Button", "p": { "id": "add", "label": "Add", "variant": "primary",
                                        "on_click": { "$h": "card:W/add#on_click" } }, "c": [] },
                { "t": "Switch", "p": { "id": "sound", "checked": false,
                                        "on_change": { "$h": "card:W/sound#on_change" } }, "c": [] },
                { "t": "div", "p": { "id": "play", "label": "Play",
                                     "on_click": { "$h": "card:W/play#on_click" } },
                  "c": [{ "t": "Icon", "p": { "name": "play" }, "c": [] }] }
            ]}]),
        ),
    );
    open_plugin_card(&h, cx, "plugin:counter");
    // Opening the card tells the plugin.
    assert_eq!(sent(&h, "counter"), vec![HostMessage::Card { open: true }]);

    cx.update_window(h.card_window, |_, window, cx| {
        // Custom styles land as written.
        let size = window.find("plugin:counter:box").bounds().size;
        assert_eq!((size.width, size.height), (px(120.0), px(30.0)));
        // An icon-only control is named for VoiceOver by its `label`.
        assert_eq!(window.find("plugin:counter:play").label(), Some("Play"));
        window.click("plugin:counter:add", cx);
        window.click("plugin:counter:box", cx);
        window.click("plugin:counter:sound", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        events(&h, "counter"),
        vec![
            ("card:W/add#on_click".to_string(), serde_json::Value::Null),
            ("card:W/box#on_click".to_string(), serde_json::Value::Null),
            (
                "card:W/sound#on_change".to_string(),
                serde_json::Value::Bool(true)
            ),
        ]
    );

    // Closing it tells the plugin too.
    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.set_item_hovered(0, false, cx);
            dock.set_card_hovered(false, cx);
        })
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    assert_eq!(
        sent(&h, "counter").last(),
        Some(&HostMessage::Card { open: false })
    );
}

#[gpui_kit::test]
fn plugin_text_fragments_and_titles_stay_on_one_line(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    plugin_says(
        &h,
        cx,
        "counter",
        render(
            "card",
            serde_json::json!([
                {"t":"div","p":{"flex":true,"flex_col":true},"c":[
                    {"t":"div","p":{"id":"count","text_size":12},"c":["33"," copied"]},
                    {"t":"div","p":{"id":"count-reference","text_size":12},"c":["33 copied"]},
                    {"t":"div","p":{"id":"detail","text_size":11,"truncate":true},"c":["3 min ago"," · Brave Browser"]},
                    {"t":"div","p":{"id":"detail-reference","text_size":11,"truncate":true},"c":["3 min ago · Brave Browser"]},
                    {"t":"div","p":{"id":"refresh","text_size":10},"c":["Refreshes every ","2"," s"]},
                    {"t":"div","p":{"id":"refresh-reference","text_size":10},"c":["Refreshes every 2 s"]},
                    {"t":"div","p":{"id":"heading"},"c":[{"t":"Title","c":["System"]}]},
                    {"t":"div","p":{"id":"heading-reference","text_size":15,"font_weight":"semibold"},"c":["System"]}
                ]}
            ]),
        ),
    );
    open_plugin_card(&h, cx, "plugin:counter");
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        for name in ["count", "detail", "refresh", "heading"] {
            let actual = window
                .find(gpui_kit::SharedString::from(format!(
                    "plugin:counter:{name}"
                )))
                .bounds();
            let reference = window
                .find(gpui_kit::SharedString::from(format!(
                    "plugin:counter:{name}-reference"
                )))
                .bounds();
            assert_eq!(
                actual.size.height, reference.size.height,
                "{name} gained an extra line"
            );
            assert!(actual.size.height > px(0.0));
        }
    })
    .unwrap();
    // Joining for display must not change the protocol paths used by patches.
    plugin_says(
        &h,
        cx,
        "counter",
        PluginMessage::Patch {
            surface: "card".into(),
            patches: serde_json::from_value(serde_json::json!([
                {"op":"replace","path":[0,4,1],"node":"5"}
            ]))
            .unwrap(),
        },
    );
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("plugin:counter:refresh").bounds().size.height,
            window
                .find("plugin:counter:refresh-reference")
                .bounds()
                .size
                .height
        );
    })
    .unwrap();
    assert!(!sent(&h, "counter").contains(&HostMessage::Resync));
}

#[gpui_kit::test]
fn plugin_patches_update_the_tree_and_mismatches_resync(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    plugin_says(
        &h,
        cx,
        "counter",
        render(
            "card",
            serde_json::json!([{ "t": "div", "p": { "id": "bar", "w": 40, "h": 10 }, "c": [] }]),
        ),
    );
    plugin_says(
        &h,
        cx,
        "counter",
        PluginMessage::Patch {
            surface: "card".into(),
            patches: serde_json::from_value(serde_json::json!([
                { "op": "props", "path": [0], "props": { "id": "bar", "w": 90, "h": 10 } }
            ]))
            .unwrap(),
        },
    );
    open_plugin_card(&h, cx, "plugin:counter");
    cx.update_window(h.card_window, |_, window, _| {
        assert_eq!(
            window.find("plugin:counter:bar").bounds().size.width,
            px(90.0)
        );
    })
    .unwrap();
    cx.run_until_parked();

    // A patch for a node that isn't there means the two sides disagree.
    plugin_says(
        &h,
        cx,
        "counter",
        PluginMessage::Patch {
            surface: "card".into(),
            patches: serde_json::from_value(serde_json::json!([
                { "op": "replace", "path": [5, 1], "node": "x" }
            ]))
            .unwrap(),
        },
    );
    assert_eq!(sent(&h, "counter").last(), Some(&HostMessage::Resync));
}

#[gpui_kit::test]
fn plugin_sliders_jump_drag_and_commit_on_release(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    let tree = |value: f64| {
        render(
            "card",
            serde_json::json!([{ "t": "Slider", "p": {
                "id": "seek", "w": 200, "value": value,
                "on_change": { "$h": "change" }, "on_commit": { "$h": "commit" }
            }, "c": [] }]),
        )
    };
    plugin_says(&h, cx, "counter", tree(0.25));
    open_plugin_card(&h, cx, "plugin:counter");

    // A click on the track jumps there. The knob's travel stops half a knob
    // short of each end, so the middle of the control is 0.5.
    cx.update_window(h.card_window, |_, window, cx| {
        assert_eq!(
            window.find("plugin:counter:seek").bounds().size.width,
            px(200.0)
        );
        window.click_at("plugin:counter:seek", point(px(100.0), px(8.0)), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        events(&h, "counter"),
        vec![
            ("change".to_string(), serde_json::Value::from(0.5)),
            ("commit".to_string(), serde_json::Value::from(0.5)),
        ]
    );

    // Dragging reports each move and commits once, where it was let go;
    // past the end it stops at 1.
    plugin_says(&h, cx, "counter", tree(0.5));
    h.platform.plugin_sent.borrow_mut().clear();
    cx.update_window(h.card_window, |_, window, cx| {
        let bounds = window.find("plugin:counter:seek").bounds();
        let y = bounds.center().y;
        window.drag(
            point(bounds.left() + px(100.0), y),
            point(bounds.right() + px(40.0), y),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    let got = events(&h, "counter");
    let (commits, changes): (Vec<_>, Vec<_>) =
        got.iter().partition(|(handler, _)| handler == "commit");
    assert!(changes.len() > 1, "{got:?}");
    assert_eq!(
        changes.last().unwrap().1,
        serde_json::Value::from(1.0),
        "{got:?}"
    );
    assert_eq!(
        commits,
        vec![&("commit".to_string(), serde_json::Value::from(1.0))]
    );
}

#[gpui_kit::test]
fn plugin_inputs_report_typing_and_submit(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    let tree = |value: &str| {
        render(
            "card",
            serde_json::json!([{ "t": "Input", "p": {
                "id": "name", "placeholder": "Name", "value": value,
                "on_change": { "$h": "change" }, "on_submit": { "$h": "submit" }
            }, "c": [] }]),
        )
    };
    plugin_says(&h, cx, "counter", tree(""));
    open_plugin_card(&h, cx, "plugin:counter");
    cx.update_window(h.card_window, |_, window, cx| {
        window.click("plugin:counter:name", cx);
        window.input("Ada", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let got = events(&h, "counter");
    assert_eq!(
        got.last(),
        Some(&("submit".to_string(), serde_json::Value::from("Ada")))
    );
    assert!(got.contains(&("change".to_string(), serde_json::Value::from("Ada"))));

    // The plugin clearing `value` empties the field; typing starts over.
    plugin_says(&h, cx, "counter", tree("Ada"));
    plugin_says(&h, cx, "counter", tree(""));
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.input("B", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        events(&h, "counter").last(),
        Some(&("change".to_string(), serde_json::Value::from("B")))
    );
}

#[gpui_kit::test]
fn fitted_plugin_cards_take_their_content_height(cx: &mut TestAppContext) {
    let h = setup(cx, vec![ItemConfig::Weather]);
    let manifest = cx
        .update(|cx| {
            h.dock
                .update(cx, |dock, cx| dock.create_plugin("Notes", cx))
        })
        .unwrap();
    assert_eq!(manifest.height, None);
    // The new plugin's code opens to edit.
    assert_eq!(
        h.platform.opened.borrow().last(),
        Some(&PathBuf::from("/plugins/notes/index.tsx"))
    );
    plugin_says(
        &h,
        cx,
        "notes",
        render(
            "card",
            serde_json::json!([{ "t": "div", "p": { "h": 236 }, "c": [] }]),
        ),
    );
    open_plugin_card(&h, cx, "plugin:notes");
    cx.run_until_parked();
    let height = cx.update(|cx| {
        h.dock
            .read(cx)
            .plugin("notes")
            .and_then(|state| state.height)
    });
    assert_eq!(height, Some(236.0));
}

#[gpui_kit::test]
fn plugins_show_their_problems_and_ask_before_being_added(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![ItemConfig::Weather]);
    h.press(cx, "cmd-3");
    h.click(cx, "add-plugin:counter");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(h.item_ids(cx), ["plugin:builtin.weather"]);

    h.click(cx, "add-plugin:counter");
    cx.simulate_prompt_answer("Add Plugin");
    cx.run_until_parked();
    assert_eq!(h.item_ids(cx), ["plugin:builtin.weather", "plugin:counter"]);
    assert_eq!(
        h.platform
            .saved_configs
            .borrow()
            .last()
            .unwrap()
            .items
            .last(),
        Some(&ItemConfig::Plugin {
            id: "counter".into()
        })
    );

    // Compile errors show up in the card instead of a blank.
    h.platform.plugin_says(
        "counter",
        PluginMessage::Error {
            message: "error: Expected \">\" but found \"}\"\n    at index.tsx:3:4".into(),
        },
    );
    cx.run_until_parked();
    let problem = cx.update(|cx| {
        h.dock
            .read(cx)
            .plugin("counter")
            .and_then(|state| state.problem.clone())
    });
    assert_eq!(problem.as_deref(), Some("Expected \">\" but found \"}\""));

    // Removing it stops it.
    h.click(cx, "remove:plugin:counter");
    assert!(cx.update(|cx| h.dock.read(cx).plugin("counter").is_none()));
}

#[gpui_kit::test]
fn plugin_settings_save_and_reach_the_plugin(cx: &mut TestAppContext) {
    let h = open_settings(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    // It started with the manifest's defaults.
    assert_eq!(
        h.platform.plugin_started.borrow()[0].1["unit"],
        serde_json::Value::from("clicks")
    );
    h.press(cx, "cmd-4");
    h.click(cx, "plugin-row:counter");
    h.click(cx, "plugin-field:counter:unit");
    h.press(cx, "cmd-a");
    cx.update_window(h.window, |_, window, cx| window.input("steps", cx))
        .unwrap();
    cx.run_until_parked();

    let messages: Vec<HostMessage> = h
        .platform
        .plugin_sent
        .borrow()
        .iter()
        .map(|(_, message)| message.clone())
        .collect();
    let expected = HostMessage::Settings {
        values: serde_json::Map::from_iter([("unit".into(), serde_json::Value::from("steps"))]),
    };
    assert_eq!(messages.last(), Some(&expected));
    let saved = h.platform.saved_configs.borrow();
    assert_eq!(
        saved.last().unwrap().plugin_settings["counter"]["unit"],
        serde_json::Value::from("steps")
    );
}

#[gpui_kit::test]
fn the_plugins_tab_creates_plugins_and_shows_logs(cx: &mut TestAppContext) {
    let h = open_settings(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    h.platform.plugin_says(
        "counter",
        PluginMessage::Log {
            line: "hello from counter".into(),
        },
    );
    cx.run_until_parked();
    let logs = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            h.dock.read(cx).plugin("counter").map(|state| {
                state
                    .logs
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
        })
    };
    assert_eq!(logs(cx), Some(vec!["hello from counter".to_string()]));

    h.press(cx, "cmd-4");
    h.click(cx, "plugin-row:counter");
    h.click(cx, "clear-logs:counter");
    assert_eq!(logs(cx), Some(Vec::new()));

    h.click(cx, "new-plugin-name");
    cx.update_window(h.window, |_, window, cx| window.input("Notes", cx))
        .unwrap();
    h.click(cx, "create-plugin");
    assert_eq!(h.item_ids(cx), ["plugin:counter", "plugin:notes"]);
}

impl SettingsHarness {
    fn type_into(&self, cx: &mut TestAppContext, field: &str, text: &str) {
        self.click(cx, SharedString::from(field.to_string()));
        self.press(cx, "cmd-a");
        cx.update_window(self.window, |_, window, cx| window.input(text, cx))
            .unwrap();
    }

    fn installed(&self, cx: &mut TestAppContext) -> Vec<String> {
        cx.update(|cx| {
            self.dock
                .read(cx)
                .installed_plugins()
                .into_iter()
                .map(|manifest| manifest.id)
                .collect()
        })
    }
}

#[gpui_kit::test]
fn plugins_install_from_links_after_asking(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![]);
    let commit = "a".repeat(40);
    h.platform
        .installer
        .publish("lasse/notes", "Notes", &commit);
    h.press(cx, "cmd-4");

    // Something that isn't a link says so and downloads nothing.
    h.type_into(cx, "plugin-url", "ftp://example.com/notes");
    h.click(cx, "install-url");
    assert!(matches!(
        cx.update(|cx| h.dock.read(cx).install.clone()),
        InstallState::Failed(_)
    ));
    assert!(h.platform.installer.fetched.lock().unwrap().is_empty());

    // Declining the trust prompt installs nothing.
    h.type_into(cx, "plugin-url", "github.com/lasse/notes");
    h.press(cx, "enter");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| h.dock.read(cx).install.clone()),
        InstallState::Idle
    );
    assert!(!h.installed(cx).contains(&"notes".to_string()));

    h.click(cx, "install-url");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Install");
    cx.run_until_parked();
    assert_eq!(h.item_ids(cx), ["plugin:notes"]);
    let manifest = cx.update(|cx| {
        h.dock
            .read(cx)
            .installed_plugins()
            .into_iter()
            .find(|manifest| manifest.id == "notes")
            .unwrap()
    });
    assert_eq!(manifest.source.unwrap().commit, commit);
    let saved = h.platform.saved_configs.borrow().last().unwrap().clone();
    assert!(saved.trusted_plugins.contains("notes"));

    // Trusted now: switching it off and on again doesn't ask.
    h.click(cx, "plugin-dock:notes");
    assert!(h.item_ids(cx).is_empty());
    h.click(cx, "plugin-dock:notes");
    assert!(!cx.has_pending_prompt());
    assert_eq!(h.item_ids(cx), ["plugin:notes"]);

    // Other hosts install the same way, and say where they're from.
    h.platform
        .installer
        .publish("gitlab.example.com/team/timer", "Timer", &commit);
    h.type_into(cx, "plugin-url", "https://gitlab.example.com/team/timer");
    h.click(cx, "install-url");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Install");
    cx.run_until_parked();
    assert_eq!(h.item_ids(cx), ["plugin:notes", "plugin:timer"]);
    let source = cx.update(|cx| {
        h.dock
            .read(cx)
            .installed_plugins()
            .into_iter()
            .find(|manifest| manifest.id == "timer")
            .and_then(|manifest| manifest.source)
            .unwrap()
    });
    assert_eq!(source.link.service(), "GitLab");
    assert_eq!(source.link.label(), "gitlab.example.com/team/timer");
}

#[gpui_kit::test]
fn installed_plugins_update_to_the_newest_commit(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![]);
    h.platform
        .installer
        .publish("lasse/notes", "Notes", &"a".repeat(40));
    h.press(cx, "cmd-4");
    h.type_into(cx, "plugin-url", "lasse/notes");
    h.click(cx, "install-url");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Install");
    cx.run_until_parked();

    // The newly installed plugin is open, so its update button shows.
    h.click(cx, "update-plugin:notes");
    cx.run_until_parked();
    let update =
        |cx: &mut TestAppContext| cx.update(|cx| h.dock.read(cx).updates.get("notes").cloned());
    assert_eq!(update(cx), Some(UpdateState::UpToDate));
    assert_eq!(h.platform.installer.fetched.lock().unwrap().len(), 1);

    *h.platform.installer.commit.lock().unwrap() = "b".repeat(40);
    let starts = h.platform.plugin_started.borrow().len();
    h.click(cx, "update-plugin:notes");
    cx.run_until_parked();
    assert_eq!(update(cx), Some(UpdateState::Updated));
    assert_eq!(h.platform.installer.fetched.lock().unwrap().len(), 2);
    assert_eq!(
        h.platform.plugin_started.borrow().len(),
        starts + 1,
        "restarted"
    );
    let commit = cx.update(|cx| match &h.dock.read(cx).items[0].kind {
        crate::app::dock::ItemKind::Plugin(manifest) => manifest.source.clone().unwrap().commit,
        _ => unreachable!(),
    });
    assert_eq!(commit, "b".repeat(40));
}

#[gpui_kit::test]
fn deleting_a_plugin_trashes_it_and_forgets_its_settings(cx: &mut TestAppContext) {
    let h = open_settings(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            let manifest = dock
                .installed_plugins()
                .into_iter()
                .find(|m| m.id == "counter");
            dock.set_plugin_setting(&manifest.unwrap(), "unit", "steps".into(), cx);
        })
    });
    h.press(cx, "cmd-4");
    h.click(cx, "plugin-row:counter");
    h.click(cx, "delete-plugin:counter");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(h.item_ids(cx), ["plugin:counter"]);

    h.click(cx, "delete-plugin:counter");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    assert!(h.item_ids(cx).is_empty());
    assert_eq!(
        *h.platform.trashed.borrow(),
        [PathBuf::from("/plugins/counter")]
    );
    assert!(!h.installed(cx).contains(&"counter".to_string()));
    let saved = h.platform.saved_configs.borrow().last().unwrap().clone();
    assert!(!saved.plugin_settings.contains_key("counter"));
    assert!(!saved.trusted_plugins.contains("counter"));
}

#[gpui_kit::test]
fn a_plugin_that_never_ran_asks_before_joining_the_dock(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![]);
    h.press(cx, "cmd-4");
    // Built-ins are trusted.
    h.click(cx, "plugin-dock:builtin.stats");
    assert!(!cx.has_pending_prompt());
    h.click(cx, "plugin-dock:counter");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Add Plugin");
    cx.run_until_parked();
    assert_eq!(h.item_ids(cx), ["plugin:builtin.stats", "plugin:counter"]);
    // Built-ins can't be deleted; others can be reloaded once in the dock.
    h.click(cx, "plugin-row:builtin.stats");
    h.click(cx, "plugin-row:counter");
    let starts = h.platform.plugin_started.borrow().len();
    h.click(cx, "reload-plugin:counter");
    assert_eq!(h.platform.plugin_started.borrow().len(), starts + 1);
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("delete-plugin:builtin.stats").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn plugin_transitions_ease_and_scroll_areas_clip(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    let tree = |width: u32| {
        render(
            "card",
            serde_json::json!([
                { "t": "div", "p": { "id": "bar", "w": width, "h": 8, "transition": 200 }, "c": [] },
                { "t": "div", "p": { "id": "list", "h": 50, "overflow_y_scroll": true }, "c": [
                    { "t": "div", "p": { "h": 400 }, "c": [] }
                ] }
            ]),
        )
    };
    plugin_says(&h, cx, "counter", tree(40));
    open_plugin_card(&h, cx, "plugin:counter");
    let width = |cx: &mut TestAppContext| {
        cx.update_window(h.card_window, |_, window, cx| {
            window.render_frame(cx);
            f32::from(window.find("plugin:counter:bar").bounds().size.width)
        })
        .unwrap()
    };
    assert_eq!(width(cx), 40.0);

    plugin_says(&h, cx, "counter", tree(140));
    cx.executor().advance_clock(Duration::from_millis(60));
    let midway = width(cx);
    assert!(midway > 40.0 && midway < 140.0, "midway at {midway}");
    cx.executor().advance_clock(Duration::from_millis(300));
    assert_eq!(width(cx), 140.0);

    // A scroll area keeps its own height; its content scrolls inside.
    cx.update_window(h.card_window, |_, window, _| {
        assert_eq!(
            window.find("plugin:counter:list").bounds().size.height,
            px(50.0)
        );
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_starting_plugin_describes_itself_to_the_dock(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    plugin_says(
        &h,
        cx,
        "counter",
        serde_json::from_value(serde_json::json!({
            "type": "manifest", "name": "Tally", "icon": "hash", "width": 320, "height": 150,
            "settings": [{ "key": "step", "title": "Step", "type": "number", "default": 2 }]
        }))
        .unwrap(),
    );
    cx.update(|cx| {
        let dock = h.dock.read(cx);
        assert_eq!(dock.item_name("plugin:counter"), "Tally");
        let crate::app::dock::ItemKind::Plugin(manifest) = &dock.items[0].kind else {
            panic!("expected the plugin");
        };
        assert_eq!((manifest.width, manifest.height), (320.0, Some(150.0)));
        assert_eq!(
            dock.plugin_values(manifest)["step"],
            serde_json::Value::from(2)
        );
    });
}

#[gpui_kit::test]
fn plugins_hear_clicks_shortcuts_and_menu_commands(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    cx.update(|cx| {
        h.dock.update(cx, |dock, cx| {
            dock.activate(0, cx);
            dock.trigger_shortcut("plugin:counter", cx);
            dock.run_plugin_action("counter", "reset");
        })
    });
    assert_eq!(
        sent(&h, "counter"),
        [
            HostMessage::Click,
            HostMessage::Click,
            HostMessage::Action {
                key: "reset".into()
            }
        ]
    );
    // A clicked shortcut doesn't peek at the card.
    cx.update(|cx| assert!(h.dock.read(cx).card().is_none()));
}

#[gpui_kit::test]
fn plugin_notifications_carry_the_plugins_name(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    plugin_says(
        &h,
        cx,
        "counter",
        PluginMessage::Notify {
            title: "Time's up".into(),
            body: "Take a break.".into(),
        },
    );
    assert_eq!(
        *h.platform.notified.borrow(),
        [["Counter", "Time's up", "Take a break."].map(String::from)]
    );
}

#[gpui_kit::test]
fn plugins_draw_line_area_and_bar_charts(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    let data = serde_json::json!([
        { "label": "Mon", "value": 3 }, { "label": "Tue", "value": 5 }, { "label": "Wed", "value": 2 }
    ]);
    let chart = |id: &str, kind: &str, height: u32| {
        serde_json::json!({ "t": "div", "p": { "id": id }, "c": [
            { "t": "Chart", "p": { "kind": kind, "data": data, "h": height, "color": "orange" }, "c": [] }
        ]})
    };
    plugin_says(
        &h,
        cx,
        "counter",
        render(
            "card",
            serde_json::json!([{ "t": "div", "p": { "flex": true, "flex_col": true }, "c": [
                chart("line", "line", 60), chart("area", "area", 70), chart("bar", "bar", 80)
            ]}]),
        ),
    );
    open_plugin_card(&h, cx, "plugin:counter");
    cx.update_window(h.card_window, |_, window, _| {
        for (id, height) in [("line", 60.0), ("area", 70.0), ("bar", 80.0)] {
            let size = window
                .find(gpui_kit::SharedString::from(format!("plugin:counter:{id}")))
                .bounds()
                .size;
            assert_eq!(size.height, px(height), "{id}");
        }
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn plugin_windows_open_draw_and_close(cx: &mut TestAppContext) {
    use crate::app::dock::DockEvent;
    let h = setup(
        cx,
        vec![ItemConfig::Plugin {
            id: "counter".into(),
        }],
    );
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&h.dock, move |_, event: &DockEvent, _| {
            seen.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let window_event = |open| HostMessage::Window {
        key: "history".into(),
        open,
    };

    // An undeclared window is refused and logged.
    plugin_says(
        &h,
        cx,
        "counter",
        PluginMessage::OpenWindow { key: "nope".into() },
    );
    assert!(events.borrow().is_empty());

    plugin_says(
        &h,
        cx,
        "counter",
        PluginMessage::OpenWindow {
            key: "history".into(),
        },
    );
    assert_eq!(
        *events.borrow(),
        [DockEvent::OpenPluginWindow {
            plugin: "counter".into(),
            key: "history".into()
        }]
    );
    assert_eq!(sent(&h, "counter"), [window_event(true)]);
    plugin_says(
        &h,
        cx,
        "counter",
        render(
            "window:history",
            serde_json::json!([{ "t": "div", "p": { "id": "list", "h": 40 }, "c": ["3 clicks"] }]),
        ),
    );

    let dock = h.dock.clone();
    let (window, _) = cx.update(|cx| {
        gpui_kit::open_window(options(400.0, 300.0), cx, |window, cx| {
            cx.new(|cx| {
                crate::ui::plugins::window::PluginWindow::new(
                    dock,
                    "counter".into(),
                    "history".into(),
                    "Counter History".into(),
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("plugin:counter:list").bounds();
        assert_eq!(list.size.height, px(40.0));
        // Below the title bar.
        assert!(list.origin.y >= px(crate::ui::plugins::window::TITLE_BAR));
    })
    .unwrap();
    cx.run_until_parked();

    // The close button tells the plugin, once.
    cx.update(|cx| {
        h.dock.update(cx, |dock, _| {
            dock.plugin_window_closed("counter", "history");
            dock.plugin_window_closed("counter", "history");
        })
    });
    assert_eq!(
        sent(&h, "counter"),
        [window_event(true), window_event(false)]
    );
    cx.update(|cx| {
        let dock = h.dock.read(cx);
        assert!(dock.plugin("counter").unwrap().windows.is_empty());
    });

    // Removing the plugin closes what it had open.
    plugin_says(
        &h,
        cx,
        "counter",
        PluginMessage::OpenWindow {
            key: "history".into(),
        },
    );
    cx.update(|cx| {
        h.dock
            .update(cx, |dock, cx| dock.remove("plugin:counter", cx))
    });
    assert_eq!(
        events.borrow().last(),
        Some(&DockEvent::ClosePluginWindow {
            plugin: "counter".into(),
            key: "history".into()
        })
    );
}

#[gpui_kit::test]
fn app_picker_lists_installed_apps_and_adds_them(cx: &mut TestAppContext) {
    use crate::ui::settings::app_picker::{AppPicker, WINDOW_SIZE};
    let h = setup(cx, vec![app("com.example.alpha")]);
    let window = cx.update(|cx| {
        let (width, height) = WINDOW_SIZE;
        gpui_kit::open_window(options(width, height), cx, |window, cx| {
            cx.new(|cx| AppPicker::new(h.dock.clone(), window, cx))
        })
        .unwrap()
        .0
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // An app already in the dock says so instead of offering to add it.
        assert!(window.try_find("pick:com.example.alpha").is_none());
        window.click("pick:com.example.beta", cx);
    })
    .unwrap();
    let ids: Vec<String> = cx.update(|cx| {
        h.dock
            .read(cx)
            .items
            .iter()
            .map(|item| item.id.to_string())
            .collect()
    });
    assert_eq!(ids, ["app:com.example.alpha", "app:com.example.beta"]);

    // Typing narrows the list by name.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("app-search", cx);
        window.input("gam", cx);
        window.render_frame(cx);
        assert!(window.try_find("pick:com.example.gamma").is_some());
        assert!(window.try_find("pick:com.example.delta").is_none());
    })
    .unwrap();
}

/// Draws one image through a [`FrameImages`] cache, the way the dock's
/// windows do.
struct OneImage {
    images: Entity<crate::ui::image_cache::FrameImages>,
    path: PathBuf,
}

impl Render for OneImage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.images
            .update(cx, |images, cx| images.sweep(window, cx));
        gpui_kit::image_cache(self.images.clone())
            .size_full()
            .child(img(self.path.clone()).size(px(64.0)))
    }
}

#[gpui_kit::test]
fn images_a_window_stops_drawing_are_freed(cx: &mut TestAppContext) {
    let (window, view) = cx.update(|cx| {
        gpui_kit::open_window(options(100.0, 100.0), cx, |_, cx| {
            let images = crate::ui::image_cache::FrameImages::new(cx);
            cx.new(|_| OneImage {
                images,
                path: PathBuf::from("/art/track-1.png"),
            })
        })
        .unwrap()
    });
    let cached = |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).images.read(cx).len());
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    };
    draw(cx);
    assert_eq!(cached(cx), 1);

    // A new track: the old art is freed on the next frame, not kept.
    for track in 2..=5 {
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.path = PathBuf::from(format!("/art/track-{track}.png"));
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
        assert_eq!(cached(cx), 1);
    }
}

/// Drags `paths` in from another app, as Finder does, holding them over the
/// dock at `at` (in the dock window) for a moment.
fn drag_in(
    h: &Harness,
    cx: &mut TestAppContext,
    paths: &[&str],
    at: gpui_kit::Point<gpui_kit::Pixels>,
) {
    let paths = gpui_kit::ExternalPaths(paths.iter().map(PathBuf::from).collect());
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(
            gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Entered {
                position: at,
                paths,
            }),
            cx,
        );
        window.dispatch_event(
            gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Pending { position: at }),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn apps_dragged_in_from_finder_open_a_gap_and_land_in_it(cx: &mut TestAppContext) {
    let h = setup(
        cx,
        vec![
            app("com.example.alpha"),
            app("com.example.beta"),
            app("com.example.gamma"),
        ],
    );
    let center = |cx: &mut TestAppContext, id: &str| {
        let id = SharedString::from(format!("app:com.example.{id}"));
        cx.update_window(h.dock_window, |_, window, cx| {
            window.render_frame(cx);
            window.find(id).bounds().center()
        })
        .unwrap()
    };
    let (beta, gamma) = (center(cx, "beta"), center(cx, "gamma"));
    let slots = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let frame = h.dock.read(cx).frame();
            frame.width.max(frame.height)
        })
    };
    let before = slots(cx);

    // Held over beta, Delta gets beta's slot: the dock grows a slot, and
    // beta and gamma slide along to make room.
    drag_in(&h, cx, &["/Applications/Delta.app"], beta);
    assert_eq!(cx.update(|cx| h.dock.read(cx).incoming()), Some(1));
    assert_eq!(slots(cx) - before, geometry::SLOT);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!((f32::from(center(cx, "beta").y) - f32::from(gamma.y)).abs() < 1.0);
    assert!(h.platform.saved_configs.borrow().is_empty());

    cx.update_window(h.dock_window, |_, window, cx| {
        window.dispatch_event(
            gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Submit { position: beta }),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        h.item_ids(cx),
        [
            "app:com.example.alpha",
            "app:com.example.delta",
            "app:com.example.beta",
            "app:com.example.gamma"
        ]
    );
    assert_eq!(cx.update(|cx| h.dock.read(cx).incoming()), None);
}

#[gpui_kit::test]
fn dragging_in_something_the_dock_cant_take_opens_no_gap(cx: &mut TestAppContext) {
    let h = setup(cx, vec![app("com.example.alpha"), app("com.example.beta")]);
    let beta = cx
        .update_window(h.dock_window, |_, window, cx| {
            window.render_frame(cx);
            window.find("app:com.example.beta").bounds().center()
        })
        .unwrap();
    // A document, and an app already in the dock.
    drag_in(
        &h,
        cx,
        &["/Users/me/notes.txt", "/Applications/Alpha.app"],
        beta,
    );
    assert_eq!(cx.update(|cx| h.dock.read(cx).incoming()), None);
    drag_out(&h, cx);

    // Leaving the window closes a gap that was open.
    drag_in(&h, cx, &["/Applications/Delta.app"], beta);
    assert_eq!(cx.update(|cx| h.dock.read(cx).incoming()), Some(1));
    drag_out(&h, cx);
    assert_eq!(cx.update(|cx| h.dock.read(cx).incoming()), None);
    assert_eq!(
        h.item_ids(cx),
        ["app:com.example.alpha", "app:com.example.beta"]
    );
}

/// Takes a drag from another app back out of the dock window.
fn drag_out(h: &Harness, cx: &mut TestAppContext) {
    cx.update_window(h.dock_window, |_, window, cx| {
        window.dispatch_event(
            gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Exited),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}
