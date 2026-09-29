//! Settings › Plugins: install plugins from GitHub or start a new one, then
//! manage each: put it in the dock, change its settings, read its logs,
//! update it, reload it or delete it.

use crate::app::dock::{Dock, InstallState, UpdateState};
use crate::app::host::REVEAL_LABEL;
use crate::ui::settings::{push_button, row, section};
use crate::ui::switch::mac_switch;
use crate::ui::theme::{Palette, text};
use domain::config::MAX_ITEMS;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, AsyncWindowContext, Context,
    Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, PromptLevel,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, TestSupportExt as _,
    Window,
    assets::IconName,
    component::{
        Sizable as _,
        input::{Input, InputEvent, InputState},
    },
    div,
    prelude::FluentBuilder as _,
    px, svg,
};
use plugin_host::{GitHub, Manifest, SettingKind, SettingSpec};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};

/// How many recent log lines the page shows per plugin.
const SHOWN_LOG_LINES: usize = 60;
/// How long a plugin's details take to fade in when it's expanded.
const EXPAND_FADE: Duration = Duration::from_millis(160);
/// Details sit under the plugin's name, past its chevron and icon.
const DETAIL_INDENT: f32 = 44.0;

/// A text field kept across renders by the window.
struct Field {
    state: Entity<InputState>,
    _subscription: Option<Subscription>,
}

/// Which plugins show their details.
#[derive(Default)]
struct Expanded(HashSet<String>);

/// What the page keeps between renders.
#[derive(Clone)]
struct Page {
    dock: Entity<Dock>,
    new_name: Entity<InputState>,
    github: Entity<InputState>,
    expanded: Entity<Expanded>,
}

impl Page {
    fn new(dock: &Entity<Dock>, window: &mut Window, cx: &mut App) -> Self {
        let new_name = window
            .use_keyed_state("new-plugin-name", cx, |window, cx| Field {
                state: cx.new(|cx| InputState::new(window, cx).placeholder("Widget name")),
                _subscription: None,
            })
            .read(cx)
            .state
            .clone();
        let expanded = window.use_keyed_state("plugins-expanded", cx, |_, _| Expanded::default());
        let github_dock = dock.clone();
        let github_expanded = expanded.clone();
        let github = window
            .use_keyed_state("github-url", cx, move |window, cx: &mut Context<Field>| {
                let state =
                    cx.new(|cx| InputState::new(window, cx).placeholder("github.com/owner/repo"));
                // Return installs, like the button beside it.
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |_, state, event: &InputEvent, window, cx| match event {
                        InputEvent::PressEnter { .. } => install_from_github(
                            github_dock.clone(),
                            state.clone(),
                            github_expanded.clone(),
                            window,
                            cx,
                        ),
                        // A new link clears the last error.
                        InputEvent::Change => github_dock.update(cx, |dock, cx| {
                            if matches!(dock.install, InstallState::Failed(_)) {
                                dock.set_install(InstallState::Idle, cx);
                            }
                        }),
                        _ => {}
                    },
                );
                Field {
                    state,
                    _subscription: Some(subscription),
                }
            })
            .read(cx)
            .state
            .clone();
        Self {
            dock: dock.clone(),
            new_name,
            github,
            expanded,
        }
    }

    fn toggle(&self, id: &str, cx: &mut App) {
        self.expanded.update(cx, |expanded, cx| {
            if !expanded.0.remove(id) {
                expanded.0.insert(id.to_string());
            }
            cx.notify();
        });
    }
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
    width: f32,
    palette: Palette,
) -> impl IntoElement {
    div()
        .id(id.into())
        .test_support()
        .w(px(width))
        .h(px(26.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .bg(palette.fill)
        .flex()
        .items_center()
        .child(Input::new(state).appearance(false).small())
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
            mac_switch(SharedString::from(format!("plugin-toggle:{id}")), palette)
                .checked(current.as_bool().unwrap_or(false))
                .on_change(move |checked, _, cx| {
                    dock.update(cx, |dock, cx| {
                        dock.set_plugin_setting(&manifest, &key, Value::from(checked), cx)
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
            field_box(format!("plugin-field:{id}"), &state, 200.0, palette).into_any_element()
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

/// Adds a plugin to the dock, first asking whether to trust one that
/// hasn't run before: it will run with this app's access.
pub fn confirm_add(dock: Entity<Dock>, manifest: Manifest, window: &mut Window, cx: &mut App) {
    if dock.read(cx).is_trusted(&manifest.id) {
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

/// Downloads the plugin linked in `field`, asks whether to trust it, then
/// installs it and puts it in the dock.
fn install_from_github(
    dock: Entity<Dock>,
    field: Entity<InputState>,
    expanded: Entity<Expanded>,
    window: &mut Window,
    cx: &mut App,
) {
    if matches!(dock.read(cx).install, InstallState::Downloading(_)) {
        return;
    }
    let source = match GitHub::parse(&field.read(cx).value()) {
        Ok(source) => source,
        Err(message) => {
            dock.update(cx, |dock, cx| {
                dock.set_install(InstallState::Failed(message.into()), cx)
            });
            return;
        }
    };
    let installer = dock.read(cx).installer();
    dock.update(cx, |dock, cx| {
        dock.set_install(InstallState::Downloading(source.label().into()), cx)
    });
    window
        .spawn(cx, async move |cx| {
            let request = source.clone();
            let fetched = cx
                .background_executor()
                .spawn(async move { installer.fetch(&request) })
                .await;
            let staged = match fetched {
                Ok(staged) => staged,
                Err(message) => {
                    cx.update(|_, cx| {
                        dock.update(cx, |dock, cx| {
                            dock.set_install(InstallState::Failed(message.into()), cx)
                        })
                    })
                    .ok();
                    return;
                }
            };
            let answer = cx.prompt(
                PromptLevel::Warning,
                &format!("Install “{}”?", staged.manifest.name),
                Some(&format!(
                    "From github.com/{}. Plugins run with the same access as this app: your \
                     files, the network and other programs. Only install plugins from people \
                     you trust.",
                    source.label()
                )),
                &["Install", "Cancel"],
            );
            let accepted = answer.await == Ok(0);
            cx.update(|window, cx| {
                if !accepted {
                    dock.update(cx, |dock, cx| dock.set_install(InstallState::Idle, cx));
                    return;
                }
                match dock.update(cx, |dock, cx| dock.finish_install(staged, cx)) {
                    Ok(manifest) => {
                        field.update(cx, |field, cx| field.set_value("", window, cx));
                        expanded.update(cx, |expanded, cx| {
                            expanded.0.insert(manifest.id);
                            cx.notify();
                        });
                    }
                    Err(message) => dock.update(cx, |dock, cx| {
                        dock.set_install(InstallState::Failed(message.into()), cx)
                    }),
                }
            })
            .ok();
        })
        .detach();
}

/// Asks, then deletes a plugin: its folder goes to the Trash.
fn confirm_delete(dock: Entity<Dock>, manifest: Manifest, window: &mut Window, cx: &mut App) {
    let answer = window.prompt(
        PromptLevel::Warning,
        &format!("Delete “{}”?", manifest.name),
        Some("Its folder moves to the Trash, and its settings and saved data are deleted."),
        &["Delete", "Cancel"],
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            if answer.await != Ok(0) {
                return;
            }
            let deleted = cx
                .update(|_, cx| dock.update(cx, |dock, cx| dock.delete_plugin(&manifest.id, cx)))
                .unwrap_or(Ok(()));
            if let Err(message) = deleted {
                alert(cx, "Couldn't delete the plugin", &message).await;
            }
        })
        .detach();
}

async fn alert(cx: &mut AsyncWindowContext, title: &str, message: &str) {
    cx.prompt(PromptLevel::Critical, title, Some(message), &["OK"])
        .await
        .ok();
}

fn create_plugin(page: &Page, window: &mut Window, cx: &mut App) {
    let name = page.new_name.read(cx).value().to_string();
    match page
        .dock
        .update(cx, |dock, cx| dock.create_plugin(&name, cx))
    {
        Ok(manifest) => {
            page.new_name
                .update(cx, |field, cx| field.set_value("", window, cx));
            page.expanded.update(cx, |expanded, cx| {
                expanded.0.insert(manifest.id);
                cx.notify();
            });
        }
        Err(message) => {
            window
                .spawn(cx, async move |cx| {
                    alert(cx, "Couldn't create the plugin", &message).await
                })
                .detach();
        }
    }
}

/// The page's sections. Built with the window at hand, for its text fields.
pub fn plugins_page(
    dock_entity: &Entity<Dock>,
    palette: Palette,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let page = Page::new(dock_entity, window, cx);
    let mut sections = vec![get_plugins(&page, palette, cx)];

    // Copied out first, so the fields below can use the window.
    struct Listed {
        manifest: Manifest,
        in_dock: bool,
        values: serde_json::Map<String, Value>,
        logs: Vec<SharedString>,
        problem: Option<SharedString>,
        update: Option<UpdateState>,
    }
    let (listed, room, reduce_motion) = {
        let dock = dock_entity.read(cx);
        let listed: Vec<Listed> = dock
            .installed_plugins()
            .into_iter()
            .map(|manifest| {
                let state = dock.plugin(&manifest.id);
                Listed {
                    in_dock: dock.in_dock(&manifest.id),
                    values: dock.plugin_values(&manifest),
                    logs: state
                        .map(|state| state.logs.iter().cloned().collect())
                        .unwrap_or_default(),
                    problem: state.and_then(|state| state.problem.clone()),
                    update: dock.updates.get(&manifest.id).cloned(),
                    manifest,
                }
            })
            .collect();
        (
            listed,
            dock.items.len() < MAX_ITEMS,
            dock.accessibility.reduce_motion,
        )
    };
    let expanded = page.expanded.read(cx).0.clone();

    let mut rows = Vec::new();
    for plugin in listed {
        let manifest = &plugin.manifest;
        let open = expanded.contains(&manifest.id);
        rows.push(plugin_row(
            &page,
            manifest,
            plugin.in_dock,
            room,
            open,
            plugin.problem.clone(),
            palette,
        ));
        if !open {
            continue;
        }
        let mut details = Vec::new();
        let builtin = crate::builtins::contains(&manifest.id);
        if builtin {
            details.push(row(
                "Built-in",
                Some("Included with Sidedoor. Its settings are in the other tabs.".into()),
                div(),
                palette,
            ));
        }
        if let Some(source) = &manifest.source {
            details.push(source_row(
                &page,
                manifest,
                source,
                plugin.update.as_ref(),
                palette,
            ));
        }
        if !builtin {
            details.push(code_row(&page, manifest, palette));
        }
        if plugin.in_dock {
            for spec in &manifest.settings {
                let current = plugin.values.get(&spec.key).cloned().unwrap_or(Value::Null);
                details.push(setting_row(
                    dock_entity,
                    manifest,
                    spec,
                    &current,
                    palette,
                    window,
                    cx,
                ));
            }
            details.push(logs_row(
                dock_entity,
                &manifest.id,
                &plugin.logs,
                plugin.problem.clone(),
                palette,
            ));
        } else if !builtin {
            details.push(row(
                "Not in the Dock",
                Some("Its settings and logs show once it's in the dock.".into()),
                div(),
                palette,
            ));
        }
        if let Some(actions) = actions_row(&page, manifest, plugin.in_dock, palette) {
            details.push(actions);
        }
        let details = div()
            .pl(px(DETAIL_INDENT))
            .flex()
            .flex_col()
            .children(details);
        rows.push(if reduce_motion {
            details.into_any_element()
        } else {
            details
                .with_animation(
                    SharedString::from(format!("plugin-details:{}", manifest.id)),
                    Animation::new(EXPAND_FADE),
                    |details, t| details.opacity(1.0 - (1.0 - t).powi(3)),
                )
                .into_any_element()
        });
    }
    let used = format!(
        "{} of {MAX_ITEMS} places in the dock used.",
        dock_entity.read(cx).items.len()
    );
    sections.push(section(
        Some("Installed"),
        rows,
        Some(if room {
            "Switch a plugin on to put it in the dock. Click its name for settings, logs and more."
                .into()
        } else {
            format!("{used} Remove an item to add another.").into()
        }),
        palette,
    ));
    sections
}

/// Install from GitHub, start a new plugin, or open the plugins folder.
fn get_plugins(page: &Page, palette: Palette, cx: &mut App) -> AnyElement {
    let install = page.dock.read(cx).install.clone();
    let downloading = matches!(install, InstallState::Downloading(_));
    let detail: SharedString = match &install {
        InstallState::Idle => "A repository, or a folder in one that holds a plugin.".into(),
        InstallState::Downloading(label) => format!("Downloading {label}…").into(),
        InstallState::Failed(message) => message.clone(),
    };
    let detail = div()
        .text_size(px(text::SUBHEADLINE))
        .when_else(
            matches!(install, InstallState::Failed(_)),
            |detail| detail.text_color(palette.orange),
            |detail| detail.text_color(palette.secondary),
        )
        .child(detail);
    let github = page.clone();
    let install_row = div()
        .min_h(px(40.0))
        .px(px(12.0))
        .py(px(8.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(div().flex_1().child("Install from GitHub"))
                .child(field_box("github-url", &page.github, 240.0, palette))
                .child(push_button(
                    "install-github",
                    if downloading {
                        "Installing…"
                    } else {
                        "Install"
                    },
                    palette,
                    !downloading,
                    false,
                    move |window, cx| {
                        install_from_github(
                            github.dock.clone(),
                            github.github.clone(),
                            github.expanded.clone(),
                            window,
                            cx,
                        )
                    },
                )),
        )
        .child(detail)
        .into_any_element();

    let create = page.clone();
    let new_row = row(
        "New Plugin",
        Some("Creates a starter widget, adds it to the dock and opens its code.".into()),
        div()
            .flex()
            .gap(px(8.0))
            .child(field_box("new-plugin-name", &page.new_name, 200.0, palette))
            .child(push_button(
                "create-plugin",
                "Create",
                palette,
                true,
                false,
                move |window, cx| create_plugin(&create, window, cx),
            )),
        palette,
    );

    let folder = plugin_host::plugins_dir();
    let reveal = page.dock.clone();
    let folder_row = row(
        "Plugins Folder",
        Some(display(&folder).into()),
        push_button("reveal-plugins", REVEAL_LABEL, palette, true, false, {
            let folder = folder.clone();
            move |_, cx| {
                if let Err(err) = std::fs::create_dir_all(&folder) {
                    eprintln!("sidedoor: couldn't create {}: {err}", folder.display());
                }
                reveal.read(cx).open_path(&folder);
            }
        }),
        palette,
    );
    section(
        Some("Get Plugins"),
        vec![install_row, new_row, folder_row],
        Some("Plugins you drop into the plugins folder show up below.".into()),
        palette,
    )
}

/// A plugin's icon, tinted like its dock item in Settings › Items.
pub(crate) fn plugin_icon(manifest: &Manifest, palette: Palette) -> AnyElement {
    let fill = match manifest.id.as_str() {
        crate::builtins::WEATHER => palette.blue,
        crate::builtins::CLIPBOARD => palette.purple,
        crate::builtins::STATS => palette.green,
        _ => palette.orange,
    };
    div()
        .size(px(22.0))
        .m(px(1.0))
        .flex_shrink_0()
        .rounded(px(6.0))
        .bg(fill)
        .flex()
        .items_center()
        .justify_center()
        .child(
            svg()
                .path(manifest.icon_path())
                .size(px(13.0))
                .text_color(palette.on_accent),
        )
        .into_any_element()
}

/// Where a plugin comes from, in a few words.
fn origin(manifest: &Manifest) -> String {
    if crate::builtins::contains(&manifest.id) {
        "Built-in".into()
    } else if let Some(source) = &manifest.source {
        format!("GitHub · {}", source.github.label())
    } else {
        "Local".into()
    }
}

/// A plugin's line in the list: click it for details, switch it into the dock.
fn plugin_row(
    page: &Page,
    manifest: &Manifest,
    in_dock: bool,
    room: bool,
    open: bool,
    problem: Option<SharedString>,
    palette: Palette,
) -> AnyElement {
    let toggle = page.clone();
    let id = manifest.id.clone();
    let summary = div()
        .id(SharedString::from(format!("plugin-row:{}", manifest.id)))
        .test_support()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .on_click(move |_, _, cx| toggle.toggle(&id, cx))
        .child(
            svg()
                .path(if open {
                    IconName::ChevronDown.path()
                } else {
                    IconName::ChevronRight.path()
                })
                .size(px(12.0))
                .flex_shrink_0()
                .text_color(palette.secondary),
        )
        .child(plugin_icon(manifest, palette))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(manifest.name.clone())
                .child(
                    div()
                        .text_size(px(text::SUBHEADLINE))
                        .truncate()
                        .when_else(
                            in_dock && problem.is_some(),
                            |detail| detail.text_color(palette.orange),
                            |detail| detail.text_color(palette.secondary),
                        )
                        .child(match problem.filter(|_| in_dock) {
                            Some(problem) => problem,
                            None => origin(manifest).into(),
                        }),
                ),
        );
    let (dock, manifest_for_switch) = (page.dock.clone(), manifest.clone());
    let switch = mac_switch(
        SharedString::from(format!("plugin-dock:{}", manifest.id)),
        palette,
    )
    .checked(in_dock)
    .disabled(!in_dock && !room)
    .on_change(move |checked, window, cx| {
        if checked {
            confirm_add(dock.clone(), manifest_for_switch.clone(), window, cx);
        } else {
            let id = format!("plugin:{}", manifest_for_switch.id);
            dock.update(cx, |dock, cx| dock.remove(&id, cx));
        }
    });
    div()
        .min_h(px(44.0))
        .px(px(12.0))
        .py(px(6.0))
        .flex()
        .items_center()
        .gap(px(16.0))
        .child(summary)
        .child(switch)
        .into_any_element()
}

/// Where a GitHub plugin came from, and checking it for updates.
fn source_row(
    page: &Page,
    manifest: &Manifest,
    source: &plugin_host::Source,
    update: Option<&UpdateState>,
    palette: Palette,
) -> AnyElement {
    let status = match update {
        None => None,
        Some(UpdateState::Checking) => Some("Checking for updates…".to_string()),
        Some(UpdateState::Updating) => Some("Downloading the update…".to_string()),
        Some(UpdateState::UpToDate) => Some("Up to date.".to_string()),
        Some(UpdateState::Updated) => Some("Updated.".to_string()),
        Some(UpdateState::Failed(message)) => Some(message.to_string()),
    };
    let detail = match status {
        Some(status) => format!(
            "{} · {} · {status}",
            source.github.label(),
            source.short_commit()
        ),
        None => format!("{} · {}", source.github.label(), source.short_commit()),
    };
    let busy = matches!(update, Some(UpdateState::Checking | UpdateState::Updating));
    let (open, check) = (page.dock.clone(), page.dock.clone());
    let url = source.github.url();
    let id = manifest.id.clone();
    row(
        "GitHub",
        Some(detail.into()),
        div()
            .flex()
            .gap(px(8.0))
            .child(push_button(
                SharedString::from(format!("plugin-github:{}", manifest.id)),
                "View",
                palette,
                true,
                false,
                move |_, cx| open.read(cx).open_path(std::path::Path::new(&url)),
            ))
            .child(push_button(
                SharedString::from(format!("update-plugin:{}", manifest.id)),
                "Check for Updates",
                palette,
                !busy,
                false,
                move |_, cx| check.update(cx, |dock, cx| dock.update_plugin(&id, cx)),
            )),
        palette,
    )
}

/// Where a plugin's code is, and opening it.
fn code_row(page: &Page, manifest: &Manifest, palette: Palette) -> AnyElement {
    let (reveal, open) = (page.dock.clone(), page.dock.clone());
    let (folder, main) = (manifest.dir.clone(), manifest.dir.join(&manifest.main));
    row(
        "Code",
        Some(display(&manifest.dir).into()),
        div()
            .flex()
            .gap(px(8.0))
            .child(push_button(
                SharedString::from(format!("reveal-plugin:{}", manifest.id)),
                REVEAL_LABEL,
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
    )
}

/// Reload and Delete, on the trailing side.
fn actions_row(
    page: &Page,
    manifest: &Manifest,
    in_dock: bool,
    palette: Palette,
) -> Option<AnyElement> {
    let deletable = !crate::builtins::contains(&manifest.id);
    if !in_dock && !deletable {
        return None;
    }
    let (reload, delete) = (page.dock.clone(), page.dock.clone());
    let (id, doomed) = (manifest.id.clone(), manifest.clone());
    Some(
        div()
            .px(px(12.0))
            .py(px(8.0))
            .flex()
            .justify_end()
            .gap(px(8.0))
            .when(in_dock, |actions| {
                actions.child(push_button(
                    SharedString::from(format!("reload-plugin:{}", manifest.id)),
                    "Reload",
                    palette,
                    true,
                    false,
                    move |_, cx| reload.update(cx, |dock, cx| dock.reload_plugin(&id, cx)),
                ))
            })
            .when(deletable, |actions| {
                actions.child(push_button(
                    SharedString::from(format!("delete-plugin:{}", manifest.id)),
                    "Delete…",
                    palette,
                    true,
                    true,
                    move |window, cx| confirm_delete(delete.clone(), doomed.clone(), window, cx),
                ))
            })
            .into_any_element(),
    )
}

/// A path with the home folder written as `~`.
fn display(path: &std::path::Path) -> String {
    let path = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => path.replacen(&home, "~", 1),
        _ => path,
    }
}
