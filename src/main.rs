//! A minimal Sidekick: a second dock that hides at a screen edge, with app
//! launchers and live Weather, Clipboard and Stats widgets.

mod clipboard;
mod clipboard_window;
mod config;
mod dock;
mod geometry;
mod macos;
mod motion;
mod platform;
mod stats;
mod status_menu;
mod style;
mod views;
mod weather;

#[cfg(test)]
mod ui_tests;

use clipboard::History;
use clipboard_window::{ClipboardWindow, ClipboardWindowEvent};
use config::Config;
use dock::{Dock, DockEvent, Services};
use geometry::{CardPlacement, Rect};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, FontWeight, Refineable as _,
    StyleRefinement, Styled as _, TextRun, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, assets::icon_assets, base::Root, font, point, px,
    size, transparent_black,
};
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow};
use platform::Platform;
use std::{cell::RefCell, rc::Rc};
use views::{CardChrome, CardView, DockView, OpenItem, RemoveItem, RevealItem};

icon_assets!(
    AppAssets,
    [
        Sun,
        Moon,
        Cloud,
        CloudSun,
        CloudMoon,
        CloudFog,
        CloudDrizzle,
        CloudRain,
        CloudSnow,
        CloudLightning,
        CloudOff,
        Cpu,
        MemoryStick,
        HardDrive,
        Clipboard,
        Link,
        File,
        FileText,
        AppWindow,
        Image,
        Search,
        CircleX,
    ]
);

fn panel_options(width: f64, height: f64) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(width as f32), px(height as f32)),
        })),
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

struct Panel {
    window: Retained<NSWindow>,
    handle: AnyWindowHandle,
    material: Option<Retained<NSView>>,
}

impl Panel {
    /// Resizes through GPUI so its layout follows, then runs `after` once
    /// the resize has landed (both are queued on the main thread in order).
    fn resize_then(&self, width: f64, height: f64, cx: &mut App, after: impl FnOnce() + 'static) {
        self.handle
            .update(cx, |_, window, _| {
                window.resize(size(px(width as f32), px(height as f32)));
            })
            .ok();
        cx.spawn(async move |_| after()).detach();
    }

    fn set_material_hidden(&self, hidden: bool) {
        if let Some(material) = &self.material {
            material.setHidden(hidden);
        }
    }
}

/// Keeps the native dock and card windows in step with the [`Dock`] model.
struct Panels {
    dock: Panel,
    card: Panel,
    chrome: Entity<CardChrome>,
    shown: bool,
    dock_frame: Option<Rect>,
    card_placement: Option<CardPlacement>,
    reduce_transparency: bool,
    _status: status_menu::StatusMenu,
}

impl Panels {
    fn sync(&mut self, dock: &Entity<Dock>, cx: &mut App) {
        let label = dock
            .read(cx)
            .card()
            .and_then(|(_, item)| tooltip_text(item));
        let label_width = label.map_or(0.0, |label| measure(&self.card, &label, cx));
        let (shown, frame, hidden_frame, accessibility, placement) = {
            let model = dock.read(cx);
            let frame = model.frame();
            let hidden = geometry::hidden_dock_frame(model.screen(), model.edge, model.items.len());
            let placement = model.card().map(|(index, item)| {
                let (width, height) = views::card_size(item, model, |_| label_width);
                geometry::card_placement(model.screen(), frame, model.edge, index, width, height)
            });
            (
                model.is_shown(),
                frame,
                hidden,
                model.accessibility,
                placement,
            )
        };

        if accessibility.reduce_transparency != self.reduce_transparency {
            self.reduce_transparency = accessibility.reduce_transparency;
            self.dock.set_material_hidden(self.reduce_transparency);
            self.card.set_material_hidden(self.reduce_transparency);
        }

        // The dock grows or shrinks as items are added and removed.
        let target = if shown { frame } else { hidden_frame };
        if self.dock_frame != Some(frame) {
            let window = self.dock.window.clone();
            let first = self.dock_frame.is_none();
            self.dock_frame = Some(frame);
            self.dock
                .resize_then(frame.width, frame.height, cx, move || {
                    macos::slide_dock(&window, target, shown && !first, false);
                });
        }
        if shown != self.shown {
            self.shown = shown;
            macos::slide_dock(
                &self.dock.window,
                target,
                shown,
                !accessibility.reduce_motion,
            );
        }

        if placement == self.card_placement {
            return;
        }
        let previous = self.card_placement;
        self.card_placement = placement;
        let motion = !accessibility.reduce_motion;
        match placement {
            Some(placement) => {
                self.chrome.update(cx, |chrome, cx| {
                    chrome.placement = Some(placement);
                    cx.notify();
                });
                let frame = placement.frame;
                let entry = match previous {
                    _ if !motion => macos::CardEntry::Snap,
                    Some(previous) => macos::CardEntry::Glide {
                        from: geometry::glide_start(previous.frame, frame, placement.side),
                    },
                    None => macos::CardEntry::Appear {
                        from: placement.side.toward_dock(motion::CARD_TRAVEL),
                    },
                };
                let window = self.card.window.clone();
                let material = self.card.material.clone();
                self.card
                    .resize_then(frame.width, frame.height, cx, move || {
                        if let Some(material) = &material {
                            macos::shape_card(material, &placement);
                        }
                        macos::show_card(&window, frame, entry);
                    });
            }
            None => {
                let drift = match (previous, motion) {
                    (Some(previous), true) => previous.side.toward_dock(motion::CARD_TRAVEL / 2.0),
                    _ => (0.0, 0.0),
                };
                macos::hide_card(&self.card.window, drift);
            }
        }
    }
}

fn tooltip_text(item: &dock::DockItem) -> Option<String> {
    match &item.kind {
        dock::ItemKind::App(app) => Some(app.name.clone()),
        _ => None,
    }
}

/// Width of a tooltip label as the card window will draw it.
fn measure(panel: &Panel, label: &str, cx: &mut App) -> f64 {
    panel
        .handle
        .update(cx, |_, window, _| {
            let run = TextRun {
                len: label.len(),
                font: gpui_kit::Font {
                    weight: FontWeight::NORMAL,
                    ..font(".SystemUIFont")
                },
                color: gpui_kit::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line =
                window
                    .text_system()
                    .layout_line(label, px(style::text::CALLOUT), &[run], None);
            f64::from(f32::from(line.width))
        })
        .unwrap_or(label.chars().count() as f64 * 6.6)
}

fn open_panel<V: gpui_kit::Render>(
    cx: &mut App,
    width: f64,
    height: f64,
    corner_radius: f64,
    backdrop: macos::Backdrop,
    build: impl FnOnce(&mut gpui_kit::Window, &mut App) -> Entity<V>,
) -> Result<Panel, String> {
    let (handle, _) = gpui_kit::open_window(panel_options(width, height), cx, build)
        .map_err(|err| err.to_string())?;
    let window = handle
        .update(cx, |_, window, cx| {
            // Root paints the theme background; these panels show a system
            // material through the window instead.
            Root::update(window, cx, |root, _, _| {
                root.style()
                    .refine(&StyleRefinement::default().bg(transparent_black()));
            });
            macos::ns_window(window)
        })
        .ok()
        .flatten()
        .ok_or("couldn't reach the native window")?;
    let material = macos::configure_panel(&window, corner_radius, backdrop);
    Ok(Panel {
        window,
        handle,
        material,
    })
}

/// The Clipboard History window, while it is open, and the app to hand
/// focus back to when it closes.
#[derive(Default)]
struct HistoryWindow {
    handle: Option<AnyWindowHandle>,
    previous_app: Option<i32>,
}

fn open_clipboard_history(
    dock: &Entity<Dock>,
    state: &Rc<RefCell<HistoryWindow>>,
    cx: &mut App,
) -> Result<(), String> {
    // Already open: bring it forward.
    let existing = state.borrow().handle;
    if let Some(handle) = existing
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(());
    }

    state.borrow_mut().previous_app = macos::frontmost_app();
    cx.activate(true);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(780.0), px(500.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Clipboard History".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(
                px(18.0),
                px(clipboard_window::TOOLBAR_HEIGHT / 2.0 - 7.0),
            )),
        }),
        window_min_size: Some(size(px(620.0), px(380.0))),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let (handle, view) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| ClipboardWindow::new(dock.clone(), window, cx))
    })
    .map_err(|err| err.to_string())?;
    handle
        .update(cx, |_, window, cx| {
            Root::update(window, cx, |root, _, _| {
                root.style()
                    .refine(&StyleRefinement::default().bg(transparent_black()));
            });
            if let Some(native) = macos::ns_window(window) {
                macos::add_window_material(&native);
                if !cx.reduce_motion() {
                    macos::fade_in(&native);
                }
            }
        })
        .ok();

    let state_for_events = state.clone();
    cx.subscribe(&view, move |_, event, cx| match event {
        ClipboardWindowEvent::Dismiss { .. } => {
            let previous = {
                let mut state = state_for_events.borrow_mut();
                if let Some(handle) = state.handle.take() {
                    handle
                        .update(cx, |_, window, _| window.remove_window())
                        .ok();
                }
                state.previous_app.take()
            };
            // Hand the keyboard back to the app the user came from, ready
            // to paste.
            if let Some(pid) = previous {
                macos::activate_app(pid);
            }
        }
    })
    .detach();
    state.borrow_mut().handle = Some(handle);
    Ok(())
}

fn run(cx: &mut App) -> Result<(), String> {
    gpui_kit::init(cx);
    macos::set_accessory_policy();

    let platform: Rc<dyn Platform> = Rc::new(macos::MacPlatform);
    let screen = platform.main_screen().ok_or("no display found")?;
    let config = Config::load_or_create(|id| platform.app_by_bundle_id(id).is_some())
        .map_err(|err| format!("couldn't read {}: {err}", Config::path().display()))?;
    macos::set_appearance(config.appearance);
    let history = History::load(&History::path());
    let dock: Entity<Dock> = cx.new(|cx| {
        Dock::new(
            config,
            platform,
            screen,
            history,
            Services { live: true },
            cx,
        )
    });
    let chrome = cx.new(|_| CardChrome::default());

    let frame = dock.read(cx).frame();
    let dock_panel = open_panel(
        cx,
        frame.width,
        frame.height,
        geometry::DOCK_RADIUS,
        macos::Backdrop::Dock,
        |window, cx| cx.new(|cx| DockView::new(dock.clone(), window, cx)),
    )?;
    let card_panel = open_panel(
        cx,
        300.0,
        200.0,
        geometry::CARD_RADIUS,
        macos::Backdrop::Card,
        |window, cx| cx.new(|cx| CardView::new(dock.clone(), chrome.clone(), window, cx)),
    )?;
    macos::hide_card(&card_panel.window, (0.0, 0.0));

    let history_window = Rc::new(RefCell::new(HistoryWindow::default()));
    let opener = dock.clone();
    cx.subscribe(&dock, move |_, event, cx| match event {
        DockEvent::OpenClipboardHistory => {
            if let Err(err) = open_clipboard_history(&opener, &history_window, cx) {
                eprintln!("sidekick: couldn't open Clipboard History: {err}");
            }
        }
    })
    .detach();

    let handler = dock.clone();
    cx.on_action(move |action: &RemoveItem, cx| {
        handler.update(cx, |dock, cx| dock.remove(&action.id, cx));
    });
    let handler = dock.clone();
    cx.on_action(move |action: &OpenItem, cx| {
        handler.update(cx, |dock, cx| dock.open_item(&action.id, cx));
    });
    let handler = dock.clone();
    cx.on_action(move |action: &RevealItem, cx| {
        handler.read(cx).reveal_in_finder(&action.id);
    });

    let status = status_menu::StatusMenu::install({
        let cx = cx.to_async();
        move |command| match command {
            status_menu::MenuCommand::OpenConfig => {
                // Opens in the user's default text editor.
                let opened = std::process::Command::new("/usr/bin/open")
                    .arg("-t")
                    .arg(Config::path())
                    .spawn();
                if let Err(err) = opened {
                    eprintln!("sidekick: couldn't open the settings file: {err}");
                }
            }
            status_menu::MenuCommand::Reload => {
                relaunch();
                cx.update(|cx| cx.quit());
            }
            status_menu::MenuCommand::Quit => {
                cx.update(|cx| cx.quit());
            }
        }
    });

    let mut panels = Panels {
        dock: dock_panel,
        card: card_panel,
        chrome,
        shown: false,
        dock_frame: None,
        card_placement: None,
        reduce_transparency: false,
        _status: status,
    };
    panels.sync(&dock, cx);
    cx.observe(&dock, move |dock, cx| panels.sync(&dock, cx))
        .detach();
    Ok(())
}

/// Starts a fresh copy once this one has quit, so edits to the settings
/// file take effect.
fn relaunch() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let spawned = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.6; exec \"$0\"")
        .arg(exe)
        .spawn();
    if let Err(err) = spawned {
        eprintln!("sidekick: couldn't relaunch: {err}");
    }
}

fn main() {
    gpui_kit::application().with_assets(AppAssets).run(|cx| {
        if let Err(err) = run(cx) {
            eprintln!("sidekick: {err}");
            cx.quit();
        }
    });
}
