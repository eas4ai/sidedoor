//! UI integration tests: the production views in headless windows, driven by
//! real pointer events, against an in-memory platform.

use crate::{
    clipboard::{ClipKind, History},
    clipboard_window::{ClipboardWindow, ClipboardWindowEvent},
    config::{Config, ItemConfig},
    dock::{Dock, Services},
    geometry::{self, Point},
    platform::{Platform, fake::FakePlatform},
    views::{CardChrome, CardView, DockView},
};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, InputEvent as _, Modifiers,
    MouseMoveEvent, TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
    test::TestWindowExt as _,
};
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
    let shared: Rc<dyn Platform> = platform.clone();

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
    h.reveal(cx);

    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("app:com.example.alpha", cx);
    })
    .unwrap();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("app:com.example.alpha"));

    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("stats", cx);
    })
    .unwrap();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("stats"));

    // Leaving the item starts a short grace period, then the card closes.
    cx.update_window(h.dock_window, |_, window, cx| move_to_padding(window, cx))
        .unwrap();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("stats"));
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    assert_eq!(h.open_card_id(cx), None);
}

#[gpui_kit::test]
fn moving_onto_the_card_keeps_it_open(cx: &mut TestAppContext) {
    let h = setup(cx, vec![ItemConfig::Clipboard]);
    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("clipboard", cx);
        move_to_padding(window, cx);
    })
    .unwrap();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("card", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(h.open_card_id(cx).as_deref(), Some("clipboard"));
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
        ["app:com.example.alpha", "app:com.example.delta", "weather"]
    );

    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.remove("weather", cx)));
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

    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("clipboard", cx);
    })
    .unwrap();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(("clip", newest)).visible());
        window.click(("clip", oldest), cx);
    })
    .unwrap();

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
        window.click("clear-history", cx);
    })
    .unwrap();
    assert_eq!(cx.update(|cx| h.dock.read(cx).history.len()), 2);
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("clear-history", cx);
    })
    .unwrap();
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
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.press(key, cx);
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
    assert_eq!(
        h.selected_title(cx).as_deref(),
        Some("https://example.com/page")
    );
}

#[gpui_kit::test]
fn image_previews_fit_their_frame(cx: &mut TestAppContext) {
    let h = open_history(cx, &[]);
    h.platform.copy(ClipKind::Image {
        path: "/tmp/sidekick-missing.png".into(),
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
}

#[gpui_kit::test]
fn show_all_opens_the_history_window(cx: &mut TestAppContext) {
    let h = setup(cx, vec![ItemConfig::Clipboard]);
    h.platform.copy(ClipKind::from_text("hello".into()));
    cx.update(|cx| h.dock.update(cx, |dock, cx| dock.poll_pasteboard(cx)));
    let opened = Rc::new(std::cell::Cell::new(0));
    let count = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&h.dock, move |_, event: &crate::dock::DockEvent, _| {
            if *event == crate::dock::DockEvent::OpenClipboardHistory {
                count.set(count.get() + 1);
            }
        })
        .detach();
    });
    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("clipboard", cx);
    })
    .unwrap();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("show-all-history", cx);
    })
    .unwrap();
    assert_eq!(opened.get(), 1);
    // Clicking the clipboard tile itself opens it too.
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("clipboard", cx);
    })
    .unwrap();
    assert_eq!(opened.get(), 2);
}
