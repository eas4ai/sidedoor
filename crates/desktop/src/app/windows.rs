use super::*;

pub(super) fn panel_options(width: f64, height: f64) -> WindowOptions {
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

pub(super) struct Panel {
    pub(super) window: NativeWindow,
    pub(super) handle: AnyWindowHandle,
    pub(super) material: Option<NativeMaterial>,
}

impl Panel {
    /// Resizes through GPUI so its layout follows, then runs `after` once
    /// the resize has landed (both are queued on the main thread in order).
    pub(super) fn resize_then(
        &self,
        width: f64,
        height: f64,
        cx: &mut App,
        after: impl FnOnce() + 'static,
    ) {
        self.handle
            .update(cx, |_, window, _| {
                window.resize(size(px(width as f32), px(height as f32)));
            })
            .ok();
        cx.spawn(async move |_| after()).detach();
    }

    pub(super) fn set_material_hidden(&self, hidden: bool) {
        if let Some(material) = &self.material {
            native::set_material_hidden(material, hidden);
        }
    }
}

/// Keeps the native dock and card windows in step with the [`Dock`] model.
pub(super) struct Panels {
    pub(super) dock: Panel,
    pub(super) card: Panel,
    pub(super) chrome: Entity<CardChrome>,
    pub(super) shown: bool,
    pub(super) dock_frame: Option<Rect>,
    pub(super) edge: Option<geometry::Edge>,
    pub(super) card_placement: Option<CardPlacement>,
    pub(super) reduce_transparency: bool,
    pub(super) _status: status_menu::StatusMenu,
}

impl Panels {
    pub(super) fn sync(&mut self, dock: &Entity<Dock>, cx: &mut App) {
        // Tooltips are sized to their text: the app name and its shortcut.
        let labels: Vec<String> = {
            let model = dock.read(cx);
            model
                .card()
                .and_then(|(_, item)| tooltip_text(item))
                .into_iter()
                .chain(
                    model
                        .card()
                        .and_then(|(_, item)| model.shortcut_for(&item.id))
                        .map(ToString::to_string),
                )
                .collect()
        };
        let widths: Vec<(String, f64)> = labels
            .into_iter()
            .map(|label| {
                let width = measure(&self.card, &label, cx);
                (label, width)
            })
            .collect();
        let text_width = |text: &str| {
            widths
                .iter()
                .find(|(label, _)| label == text)
                .map_or(0.0, |(_, width)| *width)
        };
        let (shown, frame, hidden_frame, edge, accessibility, placement) = {
            let model = dock.read(cx);
            let frame = model.frame();
            let hidden = geometry::hidden_dock_frame(model.screen(), model.edge, model.items.len());
            let placement = model.card().map(|(index, item)| {
                let (width, height) = views::card_size(item, model, text_width);
                geometry::card_placement(model.screen(), frame, model.edge, index, width, height)
            });
            (
                model.is_shown(),
                frame,
                hidden,
                model.edge,
                model.accessibility,
                placement,
            )
        };

        if accessibility.reduce_transparency != self.reduce_transparency {
            self.reduce_transparency = accessibility.reduce_transparency;
            self.dock.set_material_hidden(self.reduce_transparency);
            self.card.set_material_hidden(self.reduce_transparency);
        }

        // The dock grows or shrinks as items are added and removed, and
        // moves when its edge changes.
        let target = if shown { frame } else { hidden_frame };
        let moved = self.edge.is_some_and(|previous| previous != edge);
        self.edge = Some(edge);
        if self.dock_frame != Some(frame) {
            let window = self.dock.window.clone();
            let first = self.dock_frame.is_none();
            let animate = !accessibility.reduce_motion;
            self.dock_frame = Some(frame);
            self.dock
                .resize_then(frame.width, frame.height, cx, move || {
                    if moved && shown {
                        // Tuck it into the new edge, then slide it out there.
                        native::slide_dock(&window, hidden_frame, false, false);
                        native::slide_dock(&window, frame, true, animate);
                    } else {
                        native::slide_dock(&window, target, shown && !first, false);
                    }
                });
            if moved {
                self.shown = shown;
            }
        }
        if shown != self.shown {
            self.shown = shown;
            native::slide_dock(
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
                    _ if !motion => native::CardEntry::Snap,
                    Some(previous) => native::CardEntry::Glide {
                        from: geometry::glide_start(previous.frame, frame, placement.side),
                    },
                    None => native::CardEntry::Pop {
                        anchor: placement.arrow_tip(),
                    },
                };
                let window = self.card.window.clone();
                let material = self.card.material.clone();
                self.card
                    .resize_then(frame.width, frame.height, cx, move || {
                        if let Some(material) = &material {
                            native::shape_card(material, &placement);
                        }
                        native::show_card(&window, frame, entry);
                    });
            }
            None => {
                let anchor = previous
                    .filter(|_| motion)
                    .map(|previous| previous.arrow_tip());
                native::hide_card(&self.card.window, anchor);
                // A card that took the keyboard, for a plugin's text field,
                // hands it back once it has faded, so typing returns to the
                // app underneath.
                if native::is_key_window(&self.card.window) {
                    let window = self.card.window.clone();
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(motion::CARD_OUT.duration + std::time::Duration::from_millis(30))
                            .await;
                        native::dismiss_if_invisible(&window);
                    })
                    .detach();
                }
            }
        }
    }
}

pub(super) fn tooltip_text(item: &dock::DockItem) -> Option<String> {
    match &item.kind {
        dock::ItemKind::App(app) => Some(app.name.clone()),
        _ => None,
    }
}

/// Width of a tooltip label as the card window will draw it.
pub(super) fn measure(panel: &Panel, label: &str, cx: &mut App) -> f64 {
    let family = crate::ui::theme::ui_font(cx);
    panel
        .handle
        .update(cx, |_, window, _| {
            let run = TextRun {
                len: label.len(),
                font: gpui_kit::Font {
                    weight: FontWeight::NORMAL,
                    ..font(family)
                },
                color: gpui_kit::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window.text_system().layout_line(
                label,
                px(crate::ui::theme::text::CALLOUT),
                &[run],
                None,
            );
            f64::from(f32::from(line.width))
        })
        .unwrap_or(label.chars().count() as f64 * 6.6)
}

pub(super) fn open_panel<V: gpui_kit::Render>(
    cx: &mut App,
    width: f64,
    height: f64,
    corner_radius: f64,
    backdrop: native::Backdrop,
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
            native::window_handle(window)
        })
        .ok()
        .flatten()
        .ok_or("couldn't reach the native window")?;
    let material = native::configure_panel(&window, corner_radius, backdrop);
    Ok(Panel {
        window,
        handle,
        material,
    })
}

/// The Clipboard History window, while it is open, and the app to hand
/// focus back to when it closes.
#[derive(Default)]
pub(super) struct HistoryWindow {
    pub(super) handle: Option<AnyWindowHandle>,
    pub(super) previous_app: Option<ForegroundApp>,
}

pub(super) fn open_clipboard_history(
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

    state.borrow_mut().previous_app = native::frontmost_app();
    cx.activate(true);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(780.0), px(500.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Clipboard History".into()),
            appears_transparent: crate::ui::chrome::INSET_TITLE_BAR,
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
            if let Some(native) = native::window_handle(window) {
                native::add_window_material(&native);
                if !cx.reduce_motion() {
                    native::fade_in(&native);
                }
            }
            // GPUI's application-level activation is a no-op on Windows and X11.
            #[cfg(not(target_os = "macos"))]
            window.activate_window();
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
                native::activate_app(pid);
            }
        }
    })
    .detach();
    state.borrow_mut().handle = Some(handle);
    Ok(())
}

/// Plugin windows on screen, by plugin id and window key, with the app
/// that had focus before each opened.
pub(super) type PluginWindows =
    Rc<RefCell<HashMap<(String, String), (AnyWindowHandle, Option<ForegroundApp>)>>>;

/// Removes a plugin window; the keyboard goes back to the app before it.
pub(super) fn close_plugin_window(windows: &PluginWindows, id: &(String, String), cx: &mut App) {
    let Some((handle, previous)) = windows.borrow_mut().remove(id) else {
        return;
    };
    handle
        .update(cx, |_, window, _| window.remove_window())
        .ok();
    if let Some(pid) = previous {
        native::activate_app(pid);
    }
}

pub(super) fn open_plugin_window(
    dock: &Entity<Dock>,
    windows: &PluginWindows,
    plugin: &str,
    key: &str,
    cx: &mut App,
) -> Result<(), String> {
    let id = (plugin.to_string(), key.to_string());
    let existing = windows.borrow().get(&id).map(|(handle, _)| *handle);
    if let Some(handle) = existing
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(());
    }
    let declared = dock
        .read(cx)
        .items
        .iter()
        .find_map(|item| match &item.kind {
            dock::ItemKind::Plugin(manifest) if manifest.id == plugin => manifest
                .windows
                .iter()
                .find(|window| window.key == key)
                .cloned(),
            _ => None,
        });
    let declared = declared.ok_or("the plugin declares no such window")?;

    let previous = native::frontmost_app();
    cx.activate(true);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(declared.width as f32), px(declared.height as f32)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(declared.title.clone().into()),
            appears_transparent: crate::ui::chrome::INSET_TITLE_BAR,
            traffic_light_position: Some(point(px(14.0), px(plugin_window::TITLE_BAR / 2.0 - 7.0))),
        }),
        window_min_size: Some(size(px(280.0), px(200.0))),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let (handle, view) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            plugin_window::PluginWindow::new(
                dock.clone(),
                plugin.to_string().into(),
                key.to_string().into(),
                declared.title.clone().into(),
                window,
                cx,
            )
        })
    })
    .map_err(|err| err.to_string())?;
    handle
        .update(cx, |_, window, cx| {
            Root::update(window, cx, |root, _, _| {
                root.style()
                    .refine(&StyleRefinement::default().bg(transparent_black()));
            });
            if let Some(native) = native::window_handle(window) {
                native::add_window_material(&native);
                if !cx.reduce_motion() {
                    native::fade_in(&native);
                }
            }
            // GPUI's application-level activation is a no-op on Windows and X11.
            #[cfg(not(target_os = "macos"))]
            window.activate_window();
        })
        .ok();

    let (dock, closing, id_for_close) = (dock.clone(), windows.clone(), id.clone());
    cx.subscribe(&view, move |_, _: &plugin_window::PluginWindowEvent, cx| {
        // The close button already closes the window.
        let previous = closing
            .borrow_mut()
            .remove(&id_for_close)
            .and_then(|(_, previous)| previous);
        let (plugin, key) = &id_for_close;
        dock.update(cx, |dock, _| dock.plugin_window_closed(plugin, key));
        if let Some(pid) = previous {
            native::activate_app(pid);
        }
    })
    .detach();
    windows.borrow_mut().insert(id, (handle, previous));
    Ok(())
}

/// The Settings window, while it is open, and the app that had focus.
#[derive(Default)]
pub(super) struct SettingsState {
    pub(super) handle: Option<AnyWindowHandle>,
    pub(super) previous_app: Option<ForegroundApp>,
}

pub(super) fn open_settings(
    dock: &Entity<Dock>,
    state: &Rc<RefCell<SettingsState>>,
    cx: &mut App,
) -> Result<(), String> {
    let existing = state.borrow().handle;
    if let Some(handle) = existing
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(());
    }

    state.borrow_mut().previous_app = native::frontmost_app();
    cx.activate(true);
    let (width, height) = settings_window::WINDOW_SIZE;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(width), px(height)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(settings_window::Tab::General.title().into()),
            appears_transparent: crate::ui::chrome::INSET_TITLE_BAR,
            traffic_light_position: None,
        }),
        is_resizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let lookup: settings_window::PlaceLookup = std::sync::Arc::new(weather::search);
    let (handle, view) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| SettingsWindow::new(dock.clone(), lookup, window, cx))
    })
    .map_err(|err| err.to_string())?;
    handle
        .update(cx, |_, window, cx| {
            Root::update(window, cx, |root, _, _| {
                root.style()
                    .refine(&StyleRefinement::default().bg(transparent_black()));
            });
            if let Some(native) = native::window_handle(window) {
                native::add_window_material(&native);
                if !cx.reduce_motion() {
                    native::fade_in(&native);
                }
            }
            // GPUI's application-level activation is a no-op on Windows and X11.
            #[cfg(not(target_os = "macos"))]
            window.activate_window();
        })
        .ok();

    let state_for_events = state.clone();
    cx.subscribe(&view, move |_, event: &SettingsEvent, cx| {
        let (handle, previous) = {
            let mut state = state_for_events.borrow_mut();
            (state.handle.take(), state.previous_app.take())
        };
        // The close button already closes the window.
        if *event == SettingsEvent::Dismiss
            && let Some(handle) = handle
        {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
        if let Some(pid) = previous {
            native::activate_app(pid);
        }
    })
    .detach();
    state.borrow_mut().handle = Some(handle);
    Ok(())
}

/// Opens the settings file in the user's default text editor.
pub(super) fn open_config_file() {
    let opened = native::open_config(&services::storage::config_path());
    if let Err(err) = opened {
        eprintln!("sidedoor: couldn't open the settings file: {err}");
    }
}
