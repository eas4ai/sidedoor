use super::*;

pub(super) fn general_page(
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    palette: Palette,
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
            .child(format!(
                "Sidedoor {} · Weather by Open-Meteo",
                env!("CARGO_PKG_VERSION")
            ))
            .into_any_element(),
    ]
}
