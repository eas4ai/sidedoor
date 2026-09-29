//! Settings › Plugins: create a plugin, change each plugin's settings, read
//! its logs, and add installed plugins after a trust prompt.

use crate::{
    dock::Dock,
    plugin::{Manifest, SettingKind, SettingSpec},
    settings_window::{push_button, row, section},
    style::{Palette, text},
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, PromptLevel, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, TestSupportExt as _, Window,
    component::{
        Sizable as _,
        input::{Input, InputEvent, InputState},
        switch::Switch,
    },
    div,
    prelude::FluentBuilder as _,
    px,
};
use serde_json::Value;

/// How many recent log lines the page shows per plugin.
const SHOWN_LOG_LINES: usize = 60;

/// A text field kept across renders by the window.
struct Field {
    state: Entity<InputState>,
    _subscription: Option<Subscription>,
}

/// The name typed for a new plugin.
fn name_field(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    window
        .use_keyed_state("new-plugin-name", cx, |window, cx| Field {
            state: cx.new(|cx| InputState::new(window, cx).placeholder("Widget name")),
            _subscription: None,
        })
        .read(cx)
        .state
        .clone()
}

/// A field for a text, secret or number setting, saving as you type.
fn setting_field(
    dock: &Entity<Dock>,
    manifest: &Manifest,
    spec: &SettingSpec,
    current: &Value,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let key = SharedString::from(format!("plugin-setting:{}:{}", manifest.id, spec.key));
    let initial = match current {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let (dock, manifest, spec) = (dock.clone(), manifest.clone(), spec.clone());
    window
        .use_keyed_state(key, cx, move |window, cx: &mut Context<Field>| {
            let state = cx.new(|cx| {
                let mut state = InputState::new(window, cx)
                    .masked(spec.kind == SettingKind::Secret)
                    .placeholder(spec.title.clone());
                state.set_value(initial, window, cx);
                state
            });
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |_, state, event: &InputEvent, _, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let text = state.read(cx).unmask_value().to_string();
                    let value = if spec.kind == SettingKind::Number {
                        match text.trim().parse::<f64>() {
                            Ok(number) => Value::from(number),
                            Err(_) => return,
                        }
                    } else {
                        Value::from(text)
                    };
                    dock.update(cx, |dock, cx| {
                        dock.set_plugin_setting(&manifest, &spec.key, value, cx)
                    });
                },
            );
            Field {
                state,
                _subscription: Some(subscription),
            }
        })
        .read(cx)
        .state
        .clone()
}

fn field_box(
    id: impl Into<SharedString>,
    state: &Entity<InputState>,
    palette: Palette,
) -> impl IntoElement {
    div()
        .id(id.into())
        .test_support()
        .w(px(200.0))
        .h(px(26.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .bg(palette.fill)
        .flex()
        .items_center()
        .child(Input::new(state).appearance(false).small())
}

/// Asks before adding a plugin: it will run with this app's access.
pub fn confirm_add(dock: Entity<Dock>, manifest: Manifest, window: &mut Window, cx: &mut App) {
    if crate::builtins::contains(&manifest.id) {
        dock.update(cx, |dock, cx| {
            dock.add_plugin(manifest, cx);
        });
        return;
    }
    let answer = window.prompt(
        PromptLevel::Warning,
        &format!("Add “{}” to the dock?", manifest.name),
        Some(
            "Plugins run with the same access as this app: your files, the network and \
             other programs. Only add plugins from people you trust.",
        ),
        &["Add Plugin", "Cancel"],
        cx,
    );
    cx.spawn(async move |cx| {
        if answer.await == Ok(0) {
            cx.update(|cx| {
                dock.update(cx, |dock, cx| {
                    dock.add_plugin(manifest, cx);
                })
            });
        }
    })
    .detach();
}

fn setting_row(
    dock: &Entity<Dock>,
    manifest: &Manifest,
    spec: &SettingSpec,
    current: &Value,
    palette: Palette,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let detail = spec.description.clone().map(SharedString::from);
    let id = format!("{}:{}", manifest.id, spec.key);
    let control = match spec.kind {
        SettingKind::Toggle => {
            let (dock, manifest, key) = (dock.clone(), manifest.clone(), spec.key.clone());
            Switch::new(SharedString::from(format!("plugin-toggle:{id}")))
                .small()
                .color(palette.blue)
                .checked(current.as_bool().unwrap_or(false))
                .on_click(move |checked, _, cx| {
                    dock.update(cx, |dock, cx| {
                        dock.set_plugin_setting(&manifest, &key, Value::from(*checked), cx)
                    });
                })
                .into_any_element()
        }
        SettingKind::Choice => {
            let chosen = current.as_str().unwrap_or_default().to_string();
            let segments = spec.options.iter().enumerate().map(|(index, option)| {
                let (dock, manifest, key) = (dock.clone(), manifest.clone(), spec.key.clone());
                let value = option.clone();
                let selected = *option == chosen;
                div()
                    .id(SharedString::from(format!("plugin-choice:{id}:{index}")))
                    .test_support()
                    .px(px(9.0))
                    .py(px(3.0))
                    .rounded(px(5.0))
                    .text_size(px(text::CALLOUT))
                    .when_else(
                        selected,
                        |segment| segment.bg(palette.segment).font_weight(FontWeight::MEDIUM),
                        |segment| segment.text_color(palette.secondary),
                    )
                    .on_click(move |_, _, cx| {
                        dock.update(cx, |dock, cx| {
                            dock.set_plugin_setting(&manifest, &key, Value::from(value.clone()), cx)
                        });
                    })
                    .child(option.clone())
            });
            div()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(7.0))
                .bg(palette.fill)
                .children(segments)
                .into_any_element()
        }
        SettingKind::Text | SettingKind::Secret | SettingKind::Number => {
            let state = setting_field(dock, manifest, spec, current, window, cx);
            field_box(format!("plugin-field:{id}"), &state, palette).into_any_element()
        }
    };
    row(spec.title.clone(), detail, control, palette)
}

fn logs_row(
    dock: &Entity<Dock>,
    id: &str,
    logs: &[SharedString],
    problem: Option<SharedString>,
    palette: Palette,
) -> AnyElement {
    let clear = dock.clone();
    let plugin = id.to_string();
    let lines = logs
        .iter()
        .rev()
        .take(SHOWN_LOG_LINES)
        .rev()
        .map(|line| div().child(line.clone()));
    div()
        .px(px(12.0))
        .py(px(8.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(format!("Log · {} lines", logs.len()))
                .child(push_button(
                    SharedString::from(format!("clear-logs:{id}")),
                    "Clear",
                    palette,
                    !logs.is_empty(),
                    false,
                    move |_, cx| clear.update(cx, |dock, cx| dock.clear_plugin_logs(&plugin, cx)),
                )),
        )
        .children(problem.map(|problem| div().text_color(palette.orange).child(problem)))
        .child(
            div()
                .id(SharedString::from(format!("plugin-logs:{id}")))
                .test_support()
                .max_h(px(140.0))
                .overflow_y_scroll()
                .p(px(8.0))
                .rounded(px(6.0))
                .bg(palette.fill)
                .font_family("Menlo")
                .text_size(px(text::SUBHEADLINE))
                .text_color(palette.secondary)
                .when(logs.is_empty(), |log| {
                    log.child("Nothing yet. console.log output shows up here.")
                })
                .children(lines),
        )
        .into_any_element()
}

/// The page's sections. Built with the window at hand, for its text fields.
pub fn plugins_page(
    dock_entity: &Entity<Dock>,
    palette: Palette,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let mut sections = Vec::new();

    let name = name_field(window, cx);
    let (dock, field) = (dock_entity.clone(), name.clone());
    let create = div()
        .flex()
        .gap(px(8.0))
        .child(field_box("new-plugin-name", &name, palette))
        .child(push_button(
            "create-plugin",
            "Create",
            palette,
            true,
            false,
            move |window, cx| {
                let name = field.read(cx).value().to_string();
                let result = dock.update(cx, |dock, cx| dock.create_plugin(&name, cx));
                match result {
                    Ok(_) => field.update(cx, |field, cx| field.set_value("", window, cx)),
                    Err(message) => {
                        let shown = window.prompt(
                            PromptLevel::Critical,
                            "Couldn't create the plugin",
                            Some(&message),
                            &["OK"],
                            cx,
                        );
                        // Only an OK button; nothing to do with the answer.
                        cx.spawn(async move |_| {
                            shown.await.ok();
                        })
                        .detach();
                    }
                }
            },
        ));
    sections.push(section(
        Some("New Plugin"),
        vec![row(
            "Widget",
            Some("Creates a starter widget, adds it to the dock and opens its code.".into()),
            create,
            palette,
        )],
        Some(
            format!(
                "Plugins live in {}.",
                display(&crate::plugin::plugins_dir())
            )
            .into(),
        ),
        palette,
    ));

    // What each running plugin needs, copied out so the fields below can
    // use the window.
    struct Running {
        manifest: Manifest,
        values: serde_json::Map<String, Value>,
        logs: Vec<SharedString>,
        problem: Option<SharedString>,
    }
    let (running, available): (Vec<Running>, Vec<Manifest>) = {
        let dock = dock_entity.read(cx);
        let running = dock
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                crate::dock::ItemKind::Plugin(manifest) => Some(manifest),
                _ => None,
            })
            .map(|manifest| {
                let state = dock.plugin(&manifest.id);
                Running {
                    manifest: manifest.clone(),
                    values: dock.plugin_values(manifest),
                    logs: state
                        .map(|state| state.logs.iter().cloned().collect())
                        .unwrap_or_default(),
                    problem: state.and_then(|state| state.problem.clone()),
                }
            })
            .collect();
        (running, dock.available_plugins())
    };

    for plugin in running {
        let manifest = &plugin.manifest;
        let (reveal, open) = (dock_entity.clone(), dock_entity.clone());
        let (folder, main) = (manifest.dir.clone(), manifest.dir.join(&manifest.main));
        let mut rows = vec![row(
            "Code",
            Some(display(&manifest.dir).into()),
            div()
                .flex()
                .gap(px(8.0))
                .child(push_button(
                    SharedString::from(format!("reveal-plugin:{}", manifest.id)),
                    "Show in Finder",
                    palette,
                    true,
                    false,
                    move |_, cx| reveal.read(cx).reveal_path(&folder),
                ))
                .child(push_button(
                    SharedString::from(format!("open-plugin:{}", manifest.id)),
                    "Open",
                    palette,
                    true,
                    false,
                    move |_, cx| open.read(cx).open_path(&main),
                )),
            palette,
        )];
        if crate::builtins::contains(&manifest.id) {
            rows = vec![row(
                "Built-in",
                Some("Included with Sidedoor.".into()),
                div(),
                palette,
            )];
        }
        for spec in &manifest.settings {
            let current = plugin.values.get(&spec.key).cloned().unwrap_or(Value::Null);
            rows.push(setting_row(
                dock_entity,
                manifest,
                spec,
                &current,
                palette,
                window,
                cx,
            ));
        }
        rows.push(logs_row(
            dock_entity,
            &manifest.id,
            &plugin.logs,
            plugin.problem.clone(),
            palette,
        ));
        sections.push(section_named(manifest.name.clone(), rows, palette));
    }

    if !available.is_empty() {
        let rows = available
            .into_iter()
            .map(|manifest| {
                let dock = dock_entity.clone();
                let id = SharedString::from(format!("install-plugin:{}", manifest.id));
                row(
                    manifest.name.clone(),
                    Some(if crate::builtins::contains(&manifest.id) {
                        "Included with Sidedoor.".into()
                    } else {
                        display(&manifest.dir).into()
                    }),
                    push_button(
                        id,
                        if crate::builtins::contains(&manifest.id) {
                            "Add"
                        } else {
                            "Add…"
                        },
                        palette,
                        true,
                        false,
                        move |window, cx| confirm_add(dock.clone(), manifest.clone(), window, cx),
                    ),
                    palette,
                )
            })
            .collect();
        sections.push(section(Some("Not in the Dock"), rows, None, palette));
    }
    sections
}

/// A section whose title is a plugin's name.
fn section_named(title: String, rows: Vec<AnyElement>, palette: Palette) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .px(px(4.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(section(None, rows, None, palette))
        .into_any_element()
}

/// A path with the home folder written as `~`.
fn display(path: &std::path::Path) -> String {
    let path = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => path.replacen(&home, "~", 1),
        _ => path,
    }
}
