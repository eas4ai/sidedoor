use super::*;
use crate::app::updates::{State as AppUpdateState, VERSION};

pub(super) fn general_page(
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    palette: Palette,
    cx: &App,
) -> Vec<AnyElement> {
    let login = dock.login_item();
    let detail: Option<SharedString> = match login {
        LoginItem::NeedsApproval => {
            Some("Allow Sidedoor in System Settings › General › Login Items.".into())
        }
        LoginItem::Unavailable => Some("Only the installed app can open at login.".into()),
        LoginItem::On | LoginItem::Off => None,
    };
    let handler = dock_entity.clone();
    let launch = crate::ui::switch::mac_switch("launch-at-login", palette)
        .checked(matches!(login, LoginItem::On | LoginItem::NeedsApproval))
        .disabled(login == LoginItem::Unavailable)
        .on_change(move |checked, _, cx| {
            handler.update(cx, |dock, cx| dock.set_launch_at_login(checked, cx));
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

    let path = services::storage::config_path();
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
            crate::app::host::REVEAL_LABEL,
            palette,
            true,
            false,
            move |_, cx| {
                handler
                    .read(cx)
                    .reveal_path(&services::storage::config_path())
            },
        ))
        .child(push_button(
            "open-config",
            "Open",
            palette,
            true,
            false,
            |window, cx| window.dispatch_action(Box::new(OpenConfigFile), cx),
        ));

    let updates = dock.app_updates.clone();
    let state = &updates.read(cx).state;
    let (status, label, enabled) = match state {
        AppUpdateState::Idle => (format!("Version {VERSION}"), "Check for Updates", true),
        AppUpdateState::Checking => ("Checking for updates…".into(), "Checking…", false),
        AppUpdateState::Current => (
            format!("Sidedoor {VERSION} is up to date."),
            "Check for Updates",
            true,
        ),
        AppUpdateState::Available(release) => (
            format!("Version {} is available.", release.version),
            "Download Update",
            true,
        ),
        AppUpdateState::Downloading(release) => (
            format!("Downloading and verifying version {}…", release.version),
            "Downloading…",
            false,
        ),
        AppUpdateState::Ready(release) => (
            if release.installation.needs_admin() {
                format!(
                    "Version {} is ready. System authentication is required to install.",
                    release.version
                )
            } else {
                format!(
                    "Version {} is ready. Sidedoor will close and reopen.",
                    release.version
                )
            },
            "Install and Restart",
            true,
        ),
        AppUpdateState::Installing => ("Preparing to restart…".into(), "Installing…", false),
        AppUpdateState::Failed(error) => (error.clone(), "Try Again", true),
    };
    let handler = updates.clone();
    let action = push_button(
        "app-update-action",
        label,
        palette,
        enabled,
        false,
        move |_, cx| {
            handler.update(cx, |updates, cx| match updates.state {
                AppUpdateState::Available(_) => updates.download(cx),
                AppUpdateState::Ready(_) => updates.restart(cx),
                _ => updates.check(false, cx),
            })
        },
    );
    let ready = matches!(state, AppUpdateState::Ready(_));
    let handler = updates.clone();
    let actions = div()
        .flex()
        .gap(px(8.0))
        .child(action)
        .when(ready, |buttons| {
            buttons.child(push_button(
                "discard-app-update",
                "Discard Download",
                palette,
                true,
                false,
                move |_, cx| handler.update(cx, |updates, cx| updates.discard(cx)),
            ))
        });
    let handler = dock_entity.clone();
    let automatic = crate::ui::switch::mac_switch("automatic-app-updates", palette)
        .accessibility_label("Check for updates automatically")
        .checked(dock.automatic_update_checks)
        .on_change(move |enabled, _, cx| {
            handler.update(cx, |dock, cx| dock.set_automatic_update_checks(enabled, cx))
        });

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
                    1 => format!("1 item kept on this {}.", crate::app::host::COMPUTER_NAME).into(),
                    count => format!(
                        "{count} items kept on this {}.",
                        crate::app::host::COMPUTER_NAME
                    )
                    .into(),
                }),
                clear,
                palette,
            )],
            None,
            palette,
        ),
        section(
            Some("Updates"),
            vec![
                row("Sidedoor", Some(status.into()), actions, palette),
                row(
                    "Check automatically",
                    Some("At startup and every six hours. You choose when to install.".into()),
                    automatic,
                    palette,
                ),
            ],
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
            .child(format!("Sidedoor {} · Weather by Open-Meteo", VERSION))
            .into_any_element(),
    ]
}
