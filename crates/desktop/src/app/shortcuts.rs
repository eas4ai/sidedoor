use super::*;

/// Global shortcuts, and which dock item each registered index opens.
pub(super) struct Shortcuts {
    pub(super) hotkeys: HotKeys,
    pub(super) targets: Vec<String>,
}

pub(super) type SharedShortcuts = Rc<RefCell<Option<Shortcuts>>>;

/// Registers the dock's shortcuts, replacing the previous set.
pub(super) fn register_shortcuts(dock: &Entity<Dock>, shortcuts: &SharedShortcuts, cx: &mut App) {
    let list = dock.read(cx).shortcuts();
    let mut guard = shortcuts.borrow_mut();
    let Some(shortcuts) = guard.as_mut() else {
        return;
    };
    shortcuts.targets = list.iter().map(|(id, _)| id.clone()).collect();
    let failures = shortcuts
        .hotkeys
        .set(list.iter().map(|(_, shortcut)| shortcut.clone()).collect());
    for (index, err) in failures {
        let (id, shortcut) = &list[index];
        eprintln!("sidedoor: couldn't register {shortcut} for {id}: {err:?}");
    }
}

/// The shortcut recorder, while it is open.
#[derive(Default)]
pub(super) struct RecorderWindow {
    pub(super) handle: Option<AnyWindowHandle>,
    pub(super) previous_app: Option<ForegroundApp>,
}

pub(super) fn open_shortcut_recorder(
    id: &str,
    dock: &Entity<Dock>,
    shortcuts: &SharedShortcuts,
    state: &Rc<RefCell<RecorderWindow>>,
    cx: &mut App,
) -> Result<(), String> {
    // One recorder at a time.
    if let Some(handle) = state.borrow_mut().handle.take() {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    }
    // Let the user press combinations this app already uses.
    if let Some(shortcuts) = shortcuts.borrow_mut().as_mut() {
        shortcuts.hotkeys.suspend();
    }
    state.borrow_mut().previous_app = native::frontmost_app();
    cx.activate(true);

    let (title, icon, glyph, current) = {
        let model = dock.read(cx);
        let index = model
            .index_of(id)
            .ok_or("that item is no longer in the dock")?;
        let item = &model.items[index];
        let icon = match &item.kind {
            dock::ItemKind::App(app) => app.icon.clone(),
            _ => None,
        };
        (
            model.item_name(id),
            icon,
            views::widget_glyph(&item.kind),
            model.shortcut_for(id).cloned(),
        )
    };
    let check: shortcut_recorder::ConflictCheck = {
        let (dock, shortcuts, id) = (dock.clone(), shortcuts.clone(), id.to_string());
        Rc::new(move |shortcut, cx| {
            if let Some(owner) = dock.read(cx).shortcut_owner(shortcut, &id) {
                return Err(format!("{owner} already uses {shortcut}."));
            }
            let guard = shortcuts.borrow();
            match guard
                .as_ref()
                .map(|shortcuts| shortcuts.hotkeys.probe(shortcut))
            {
                Some(Err(RegisterError::TakenByAnotherApp)) => {
                    Err(format!("Another app already uses {shortcut}."))
                }
                Some(Err(_)) => Err("That key can't be used in a shortcut.".into()),
                _ => Ok(()),
            }
        })
    };

    let (width, height) = shortcut_recorder::WINDOW_SIZE;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(width), px(height)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Assign Shortcut".into()),
            appears_transparent: cfg!(target_os = "macos"),
            traffic_light_position: Some(point(px(14.0), px(14.0))),
        }),
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let (handle, view) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| ShortcutRecorder::new(title, icon, glyph, current, check, window, cx))
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
            // GPUI's application-level activation is a no-op on Windows.
            #[cfg(target_os = "windows")]
            window.activate_window();
        })
        .ok();

    let (dock, shortcuts, state_for_events, id) = (
        dock.clone(),
        shortcuts.clone(),
        state.clone(),
        id.to_string(),
    );
    cx.subscribe(&view, move |_, event: &RecorderEvent, cx| {
        match event {
            RecorderEvent::Save(shortcut) => {
                dock.update(cx, |dock, cx| {
                    dock.set_shortcut(&id, Some(shortcut.clone()), cx)
                });
            }
            RecorderEvent::Remove => {
                dock.update(cx, |dock, cx| dock.set_shortcut(&id, None, cx));
            }
            RecorderEvent::Cancel | RecorderEvent::Closed => {}
        }
        let (handle, previous) = {
            let mut state = state_for_events.borrow_mut();
            (state.handle.take(), state.previous_app.take())
        };
        // The close button already closes the window.
        if *event != RecorderEvent::Closed
            && let Some(handle) = handle
        {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
        if let Some(shortcuts) = shortcuts.borrow_mut().as_mut() {
            for (index, err) in shortcuts.hotkeys.resume() {
                eprintln!("sidedoor: couldn't register shortcut {index}: {err:?}");
            }
        }
        if let Some(pid) = previous {
            native::activate_app(pid);
        }
    })
    .detach();
    state.borrow_mut().handle = Some(handle);
    Ok(())
}
