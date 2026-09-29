use super::*;

fn coordinates(location: &WeatherLocation) -> String {
    let north = if location.latitude >= 0.0 { "N" } else { "S" };
    let east = if location.longitude >= 0.0 { "E" } else { "W" };
    format!(
        "{:.2}° {north}, {:.2}° {east}",
        location.latitude.abs(),
        location.longitude.abs()
    )
}

pub(super) fn weather_page(
    view: &Entity<SettingsWindow>,
    city: &Entity<InputState>,
    places: &PlaceSearch,
    dock: &Dock,
    palette: Palette,
) -> Vec<AnyElement> {
    let location = &dock.location;
    let now = match &dock.weather {
        WeatherState::Ready { weather, .. } => format!(
            "{}° {}",
            weather.temperature.round() as i64,
            weather.condition.label()
        ),
        WeatherState::Loading => "Loading…".into(),
        WeatherState::Failed(_) => "Unavailable".into(),
    };
    let current = row(
        location.name.clone(),
        Some(coordinates(location).into()),
        div().text_color(palette.secondary).child(now),
        palette,
    );

    let field = div().px(px(10.0)).py(px(8.0)).child(
        div()
            .h(px(28.0))
            .px(px(8.0))
            .rounded(px(7.0))
            .bg(palette.fill)
            .flex()
            .items_center()
            .child(
                Input::new(city)
                    .appearance(false)
                    .cleanable(true)
                    .prefix(glyph(IconName::Search, 14.0, palette.secondary)),
            ),
    );
    let mut rows = vec![field.into_any_element()];
    let note = |message: SharedString, color: Hsla| {
        div()
            .px(px(12.0))
            .py(px(10.0))
            .text_color(color)
            .child(message)
            .into_any_element()
    };
    match places {
        PlaceSearch::Idle => {}
        PlaceSearch::Searching => rows.push(note("Searching…".into(), palette.secondary)),
        PlaceSearch::Failed(message) => rows.push(note(message.clone(), palette.orange)),
        PlaceSearch::Found { query, places } if places.is_empty() => rows.push(note(
            format!("No places match “{query}”.").into(),
            palette.secondary,
        )),
        PlaceSearch::Found { places, .. } => {
            rows.extend(places.iter().enumerate().map(|(index, place)| {
                let current = place.location() == *location;
                let (view, chosen) = (view.clone(), place.clone());
                let detail = place.detail();
                div()
                    .id(("place", index))
                    .test_support()
                    .mx(px(4.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .hover(|style| style.bg(palette.fill))
                    .child(glyph(IconName::MapPin, 14.0, palette.secondary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(place.name.clone())
                            .when(!detail.is_empty(), |text| {
                                text.child(
                                    div()
                                        .text_size(px(text::SUBHEADLINE))
                                        .text_color(palette.secondary)
                                        .child(detail),
                                )
                            }),
                    )
                    .when(current, |row| {
                        row.child(glyph(IconName::Check, 14.0, palette.blue))
                    })
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.choose_place(&chosen, window, cx));
                    })
                    .into_any_element()
            }));
            // Breathing room under the last result.
            rows.push(div().h(px(4.0)).into_any_element());
        }
    }

    vec![
        section(Some("Location"), vec![current], None, palette),
        // One block: the field and its results, without hairlines between.
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .px(px(4.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Change Location"),
            )
            .child(
                div()
                    .rounded(px(10.0))
                    .bg(palette.group)
                    .border_1()
                    .border_color(palette.separator)
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .child(
                div()
                    .px(px(4.0))
                    .text_size(px(text::SUBHEADLINE))
                    .text_color(palette.secondary)
                    .child("Forecasts come from Open-Meteo and refresh every 20 minutes."),
            )
            .into_any_element(),
    ]
}
