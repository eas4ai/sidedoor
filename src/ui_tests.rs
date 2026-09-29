//! UI integration tests: the production views in headless windows, driven by
//! real pointer events, against an in-memory platform.

use crate::{
    clipboard::{ClipKind, History},
    clipboard_window::{ClipboardWindow, ClipboardWindowEvent},
    config::{Appearance, Config, ItemConfig},
    dock::{Dock, Services},
    geometry::{self, Edge, Point},
    platform::{LoginItem, Platform, fake::FakePlatform},
    plugin::{HostMessage, Incoming, PluginMessage},
    settings_window::{SettingsEvent, SettingsWindow, Tab},
    views::{CardChrome, CardView, DockView},
    weather::Place,
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
        cx.subscribe(&h.dock, move |_, event: &crate::dock::DockEvent, _| {
            if *event == crate::dock::DockEvent::OpenClipboardHistory {
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
        h.dock
            .update(cx, |dock, cx| dock.trigger_shortcut("clipboard", cx))
    });
    assert_eq!(opened.get(), 1);

    // A widget shortcut shows the dock with that widget's card.
    assert!(!cx.update(|cx| h.dock.read(cx).is_shown()));
    cx.update(|cx| {
        h.dock
            .update(cx, |dock, cx| dock.trigger_shortcut("weather", cx))
    });
    assert!(cx.update(|cx| h.dock.read(cx).is_shown()));
    assert_eq!(h.open_card_id(cx).as_deref(), Some("weather"));
}

#[gpui_kit::test]
fn assigned_shortcuts_are_saved_and_follow_their_item(cx: &mut TestAppContext) {
    let h = setup(cx, vec![app("com.example.alpha"), app("com.example.beta")]);
    let shortcut = crate::shortcut::Shortcut::parse("alt-cmd-a").unwrap();
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
    Entity<crate::shortcut_recorder::ShortcutRecorder>,
    Rc<std::cell::RefCell<Vec<crate::shortcut_recorder::RecorderEvent>>>,
) {
    use crate::{shortcut::Shortcut, shortcut_recorder::*};
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
    use crate::{shortcut::Shortcut, shortcut_recorder::RecorderEvent};
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
    use crate::shortcut_recorder::RecorderEvent;
    let (window, _, events) = open_recorder(cx, Some("ctrl-cmd-v"), "");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("remove-shortcut", cx);
        window.press("escape", cx);
    })
    .unwrap();
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
    let lookup: crate::settings_window::PlaceLookup = std::sync::Arc::new(|query: &str| {
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
        let (width, height) = crate::settings_window::WINDOW_SIZE;
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

impl SettingsHarness {
    fn click(&self, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) {
        let id = id.into();
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.press(key, cx);
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
    assert_eq!(h.tab(cx), Tab::Weather);
    h.press(cx, "cmd-2");
    assert_eq!(h.tab(cx), Tab::Dock);
    assert!(h.events.borrow().is_empty());
    h.press(cx, "cmd-w");
    assert_eq!(*h.events.borrow(), vec![SettingsEvent::Dismiss]);
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
    assert_eq!(h.item_ids(cx), ["app:com.example.alpha", "weather"]);

    // Only missing widgets are offered.
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("add-widget:Weather").is_none());
    })
    .unwrap();
    h.click(cx, "add-widget:Stats");
    assert_eq!(
        h.item_ids(cx),
        ["app:com.example.alpha", "weather", "stats"]
    );

    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to("item-row:app:com.example.alpha", "item-row:stats", cx);
    })
    .unwrap();
    assert_eq!(
        h.item_ids(cx),
        ["weather", "stats", "app:com.example.alpha"]
    );
    let saved = h.platform.saved_configs.borrow();
    assert_eq!(
        saved.last().map(|config| config.items.clone()),
        Some(vec![
            ItemConfig::Weather,
            ItemConfig::Stats,
            app("com.example.alpha")
        ])
    );
}

#[gpui_kit::test]
fn weather_settings_search_and_choose_a_place(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![ItemConfig::Weather]);
    h.press(cx, "cmd-4");
    cx.update_window(h.window, |_, window, cx| {
        h.view.update(cx, |view, cx| view.focus_city(window, cx));
    })
    .unwrap();
    cx.update_window(h.window, |_, window, cx| window.input("Aal", cx))
        .unwrap();
    // Nothing is looked up until typing pauses.
    cx.update_window(h.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("place", 0usize)).is_none());
    })
    .unwrap();
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
}

// MARK: Plugins

fn plugin_renders(h: &Harness, cx: &mut TestAppContext, surface: &str, tree: serde_json::Value) {
    h.platform.plugin_says(
        "counter",
        Incoming::Message(PluginMessage::Render {
            surface: surface.into(),
            tree: serde_json::from_value(tree).unwrap(),
        }),
    );
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
    plugin_renders(
        &h,
        cx,
        "card",
        serde_json::json!([{ "t": "Card", "p": { "title": "Counter" }, "c": [
            { "t": "div", "p": { "id": "box", "w": 120, "h": 30, "bg": "blue", "rounded": 8,
                                 "on_click": { "$h": "card:W/box#on_click" } }, "c": ["3"] },
            { "t": "Button", "p": { "id": "add", "label": "Add", "variant": "primary",
                                    "on_click": { "$h": "card:W/add#on_click" } }, "c": [] },
            { "t": "Switch", "p": { "id": "sound", "checked": false,
                                    "on_change": { "$h": "card:W/sound#on_change" } }, "c": [] }
        ]}]),
    );

    h.reveal(cx);
    cx.update_window(h.dock_window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("plugin:counter", cx);
    })
    .unwrap();
    cx.update_window(h.card_window, |_, window, cx| {
        window.render_frame(cx);
        // Custom styles land as written.
        let size = window.find("plugin:counter:box").bounds().size;
        assert_eq!((size.width, size.height), (px(120.0), px(30.0)));
        window.click("plugin:counter:add", cx);
        window.click("plugin:counter:box", cx);
        window.click("plugin:counter:sound", cx);
    })
    .unwrap();

    let sent = h.platform.plugin_sent.borrow();
    let events: Vec<(String, serde_json::Value)> = sent
        .iter()
        .map(|(plugin, HostMessage::Event { handler, value })| {
            assert_eq!(plugin, "counter");
            (handler.clone(), value.clone())
        })
        .collect();
    assert_eq!(
        events,
        vec![
            ("card:W/add#on_click".to_string(), serde_json::Value::Null),
            ("card:W/box#on_click".to_string(), serde_json::Value::Null),
            (
                "card:W/sound#on_change".to_string(),
                serde_json::Value::Bool(true)
            ),
        ]
    );
}

#[gpui_kit::test]
fn plugins_show_their_problems_and_can_be_added_from_settings(cx: &mut TestAppContext) {
    let h = open_settings(cx, vec![ItemConfig::Weather]);
    h.press(cx, "cmd-3");
    h.click(cx, "add-plugin:counter");
    assert_eq!(h.item_ids(cx), ["weather", "plugin:counter"]);
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

    // Bun's compile errors show up in the card instead of a blank.
    h.platform.plugin_says(
        "counter",
        Incoming::Log("error: Expected \">\" but found \"}\"".into()),
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
