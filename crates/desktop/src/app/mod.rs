//! Application startup, state, and window coordination.
pub(crate) mod dock;
pub(crate) mod host;
mod shortcuts;
pub(crate) mod updates;
mod windows;
use self::host as platform;
use crate::ui::{
    dock as views, plugins::window as plugin_window, settings as settings_window, shortcut_recorder,
};
use ::platform::{hotkeys, native, status_menu};
use dock::{Dock, DockEvent, Services};
use domain::{geometry, motion};
use geometry::{CardPlacement, Rect};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, FontWeight, Refineable as _,
    StyleRefinement, Styled as _, TextRun, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, base::Root, font, point, px, size, transparent_black,
};
use hotkeys::{HotKeys, RegisterError};
use native::{ForegroundApp, NativeMaterial, NativeWindow};
use platform::Host;
use settings_window::{SettingsEvent, SettingsWindow};
use shortcut_recorder::{RecorderEvent, ShortcutRecorder};
use shortcuts::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use views::{
    AddApps, AssignShortcut, CardChrome, CardView, DockView, OpenConfigFile, OpenItem,
    OpenSettings, RemoveItem, RemoveShortcut, RevealItem, RunPluginAction,
};
use windows::*;

fn init(cx: &mut App) -> Result<(), String> {
    gpui_kit::init(cx);
    crate::ui::theme::install_ui_font(cx);
    native::set_accessory_policy();

    let platform: Rc<dyn Host> = Rc::new(platform::NativeHost::default());
    let screen = platform.main_screen().ok_or("no display found")?;
    platform::migrate_old_name();
    let config = services::storage::load_config(|id| platform.app_by_bundle_id(id).is_some())
        .map_err(|err| {
            format!(
                "couldn't read {}: {err}",
                services::storage::config_path().display()
            )
        })?;
    native::set_appearance(config.appearance);
    let dock: Entity<Dock> =
        cx.new(|cx| Dock::new(config, platform, screen, Services { live: true }, cx));
    dock.update(cx, |dock, cx| dock.start_plugins(cx));
    let chrome = cx.new(|_| CardChrome::default());

    let frame = dock.read(cx).frame();
    let dock_panel = open_panel(
        cx,
        frame.width,
        frame.height,
        geometry::DOCK_RADIUS,
        native::Backdrop::Dock,
        |window, cx| cx.new(|cx| DockView::new(dock.clone(), window, cx)),
    )?;
    let card_panel = open_panel(
        cx,
        300.0,
        200.0,
        geometry::CARD_RADIUS,
        native::Backdrop::Card,
        |window, cx| cx.new(|cx| CardView::new(dock.clone(), chrome.clone(), window, cx)),
    )?;
    native::hide_card(&card_panel.window, None);

    // Global shortcuts: each registered index maps to a dock item.
    let shortcuts: SharedShortcuts = Rc::new(RefCell::new(None));
    let hotkeys = HotKeys::install({
        let (cx, dock, shortcuts) = (cx.to_async(), dock.clone(), shortcuts.clone());
        move |index| {
            let target = shortcuts
                .borrow()
                .as_ref()
                .and_then(|shortcuts| shortcuts.targets.get(index).cloned());
            if let Some(id) = target {
                cx.update(|cx| dock.update(cx, |dock, cx| dock.trigger_shortcut(&id, cx)));
            }
        }
    });
    *shortcuts.borrow_mut() = Some(Shortcuts {
        hotkeys,
        targets: Vec::new(),
    });
    register_shortcuts(&dock, &shortcuts, cx);

    let plugin_windows = PluginWindows::default();
    let opener = dock.clone();
    let registry = shortcuts.clone();
    cx.subscribe(&dock, move |_, event, cx| match event {
        DockEvent::ShortcutsChanged => register_shortcuts(&opener, &registry, cx),
        DockEvent::OpenPluginWindow { plugin, key } => {
            if let Err(err) = open_plugin_window(&opener, &plugin_windows, plugin, key, cx) {
                eprintln!("sidedoor: couldn't open {plugin}'s {key} window: {err}");
            }
        }
        DockEvent::ClosePluginWindow { plugin, key } => {
            close_plugin_window(&plugin_windows, &(plugin.clone(), key.clone()), cx);
        }
    })
    .detach();

    let recorder = Rc::new(RefCell::new(RecorderWindow::default()));
    let (handler, registry) = (dock.clone(), shortcuts.clone());
    cx.on_action(move |action: &AssignShortcut, cx| {
        if let Err(err) = open_shortcut_recorder(&action.id, &handler, &registry, &recorder, cx) {
            eprintln!("sidedoor: couldn't open the shortcut recorder: {err}");
        }
    });
    let handler = dock.clone();
    cx.on_action(move |action: &RemoveShortcut, cx| {
        handler.update(cx, |dock, cx| dock.set_shortcut(&action.id, None, cx));
    });

    let handler = dock.clone();
    cx.on_action(move |action: &RemoveItem, cx| {
        handler.update(cx, |dock, cx| dock.remove(&action.id, cx));
    });
    let handler = dock.clone();
    cx.on_action(move |action: &OpenItem, cx| {
        handler.update(cx, |dock, cx| dock.open_item(&action.id, cx));
    });
    let handler = dock.clone();
    cx.on_action(move |action: &RunPluginAction, cx| {
        handler.update(cx, |dock, _| {
            dock.run_plugin_action(&action.plugin, &action.key);
        });
    });
    let handler = dock.clone();
    cx.on_action(move |action: &RevealItem, cx| {
        handler.read(cx).reveal_in_finder(&action.id);
    });

    let settings = Rc::new(RefCell::new(SettingsState::default()));
    let (handler, state) = (dock.clone(), settings.clone());
    cx.on_action(move |_: &OpenSettings, cx| {
        if let Err(err) = open_settings(&handler, &state, cx) {
            eprintln!("sidedoor: couldn't open Settings: {err}");
        }
    });
    cx.on_action(|_: &OpenConfigFile, _| open_config_file());
    let (handler, picker) = (
        dock.clone(),
        Rc::new(RefCell::new(AppPickerState::default())),
    );
    cx.on_action(move |_: &AddApps, cx| {
        if let Err(err) = open_app_picker(&handler, &picker, cx) {
            eprintln!("sidedoor: couldn't open the app list: {err}");
        }
    });

    let status = status_menu::StatusMenu::install({
        let cx = cx.to_async();
        let (dock, settings) = (dock.clone(), settings.clone());
        move |command| match command {
            status_menu::MenuCommand::OpenSettings => {
                cx.update(|cx| {
                    if let Err(err) = open_settings(&dock, &settings, cx) {
                        eprintln!("sidedoor: couldn't open Settings: {err}");
                    }
                });
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

    // X11 has no run loop to hang native animations and key grabs on, so
    // a foreground task drives them.
    #[cfg(target_os = "linux")]
    cx.spawn(async move |cx| {
        loop {
            let delay = native::pump();
            cx.background_executor().timer(delay).await;
        }
    })
    .detach();

    let mut panels = Panels {
        dock: dock_panel,
        card: card_panel,
        chrome,
        shown: false,
        dock_frame: None,
        edge: None,
        card_placement: None,
        reduce_transparency: false,
        _status: status,
    };
    panels.sync(&dock, cx);
    cx.observe(&dock, move |dock, cx| panels.sync(&dock, cx))
        .detach();
    if std::env::args().any(|argument| argument == "--settings") {
        cx.dispatch_action(&OpenSettings);
    }
    Ok(())
}

/// Starts a fresh copy once this one has quit, so edits to the settings
/// file take effect.
fn relaunch() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let spawned = native::relaunch(&exe);
    if let Err(err) = spawned {
        eprintln!("sidedoor: couldn't relaunch: {err}");
    }
}

pub fn run() {
    #[cfg(target_os = "linux")]
    ::platform::linux::prefer_x11();
    #[cfg(target_os = "windows")]
    native::wait_for_previous_process();
    #[cfg(target_os = "windows")]
    let _instance = match ::platform::windows::instance::Instance::acquire() {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            eprintln!("sidedoor: couldn't acquire application instance: {error}");
            return;
        }
    };
    // Every Lucide icon, so plugins can use any of them by name.
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            if let Err(err) = init(cx) {
                eprintln!("sidedoor: {err}");
                cx.quit();
            } else {
                updates::acknowledge_startup();
            }
        });
}
