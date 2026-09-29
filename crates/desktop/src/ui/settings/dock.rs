use super::*;

pub(super) fn dock_page(
    dock_entity: &Entity<Dock>,
    dock: &Dock,
    palette: Palette,
) -> Vec<AnyElement> {
    let edges = [
        (Edge::Left, "Left"),
        (Edge::Right, "Right"),
        (Edge::Bottom, "Bottom"),
    ];
    let edge_picker =
        div()
            .flex()
            .gap(px(10.0))
            .children(edges.into_iter().map(|(edge, label)| {
                let handler = dock_entity.clone();
                choice(
                    SharedString::from(format!("edge:{label}")).into(),
                    label,
                    dock.edge == edge,
                    edge_art(edge, palette),
                    palette,
                    move |cx| handler.update(cx, |dock, cx| dock.set_edge(edge, cx)),
                )
            }));

    let themes = [
        (Appearance::System, "Automatic"),
        (Appearance::Light, "Light"),
        (Appearance::Dark, "Dark"),
    ];
    let theme_picker =
        div()
            .flex()
            .gap(px(10.0))
            .children(themes.into_iter().map(|(appearance, label)| {
                let handler = dock_entity.clone();
                choice(
                    SharedString::from(format!("theme:{label}")).into(),
                    label,
                    dock.appearance() == appearance,
                    theme_art(appearance, palette),
                    palette,
                    move |cx| handler.update(cx, |dock, cx| dock.set_appearance(appearance, cx)),
                )
            }));

    vec![
        section(
            Some("Position"),
            vec![row(
                "Screen edge",
                Some("Move the pointer to this edge to bring up the dock.".into()),
                edge_picker,
                palette,
            )],
            None,
            palette,
        ),
        section(
            Some("Appearance"),
            vec![row(
                "Theme",
                Some(
                    if cfg!(windows) {
                        "Automatic follows Windows."
                    } else if cfg!(target_os = "linux") {
                        "Automatic follows the system."
                    } else {
                        "Automatic follows macOS."
                    }
                    .into(),
                ),
                theme_picker,
                palette,
            )],
            None,
            palette,
        ),
    ]
}
