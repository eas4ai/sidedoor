use super::*;

/// What a plugin's card shows, copied out of the dock so it can be drawn
/// with the window at hand.
struct PluginCard {
    manifest: plugin_host::Manifest,
    tree: Option<Vec<plugin_host::Node>>,
    problem: Option<SharedString>,
}

impl PluginCard {
    fn of(dock: &Dock) -> Option<Self> {
        let (_, item) = dock.card()?;
        let ItemKind::Plugin(manifest) = &item.kind else {
            return None;
        };
        let state = dock.plugin(&manifest.id);
        Some(Self {
            manifest: manifest.clone(),
            tree: state.and_then(|state| state.card.clone()),
            problem: state.and_then(|state| state.problem.clone()),
        })
    }

    /// What the plugin drew, or why it can't draw yet. A card without a
    /// fixed height reports its content's height so the window can fit it.
    fn render(
        self,
        dock_entity: &Entity<Dock>,
        palette: Palette,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let name = self.manifest.name.clone();
        let content = match (&self.problem, &self.tree) {
            (Some(problem), _) => message_card(&name, problem, palette),
            (None, None) => message_card(&name, "Starting…", palette),
            (None, Some(tree)) => {
                let surface = crate::ui::plugins::renderer::Surface {
                    dock: dock_entity.clone(),
                    plugin: self.manifest.id.clone().into(),
                    palette,
                };
                // Whatever doesn't fit is cut off at the card's edge rather
                // than drawn over the arrow.
                div()
                    .w_full()
                    .when(self.manifest.height.is_some(), |el| el.h_full())
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .children(surface.render(tree, "card", window, cx))
                    .into_any_element()
            }
        };
        if self.manifest.height.is_some() {
            return content;
        }
        let (dock, id) = (dock_entity.clone(), self.manifest.id.clone());
        div()
            .w_full()
            .flex()
            .flex_col()
            .on_children_prepainted(move |bounds, _, cx| {
                let top = bounds.iter().map(|b| b.top()).min();
                let bottom = bounds.iter().map(|b| b.bottom()).max();
                if let (Some(top), Some(bottom)) = (top, bottom) {
                    let height = f64::from(f32::from(bottom - top));
                    dock.update(cx, |dock, cx| dock.set_plugin_height(&id, height, cx));
                }
            })
            .child(content)
            .into_any_element()
    }
}

// MARK: Card

/// Where the card window currently sits, shared by the native side (which
/// places it) and [`CardView`] (which draws inside it).
#[derive(Default)]
pub struct CardChrome {
    pub placement: Option<CardPlacement>,
}

pub struct CardView {
    dock: Entity<Dock>,
    chrome: Entity<CardChrome>,
    _subscriptions: Vec<Subscription>,
}

impl CardView {
    pub fn new(
        dock: Entity<Dock>,
        chrome: Entity<CardChrome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe(&chrome, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        Self {
            dock,
            chrome,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for CardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let animate = !cx.reduce_motion();
        let palette = Palette::new(window, self.dock.read(cx).accessibility);
        // Plugin cards draw with the window, which the dock borrow below
        // would block.
        let mut plugin_card = PluginCard::of(self.dock.read(cx))
            .map(|card| card.render(&self.dock, palette, window, cx));
        let dock = self.dock.read(cx);
        let placement = self.chrome.read(cx).placement;
        let content = dock.card().map(|(_, item)| {
            let content = match &item.kind {
                ItemKind::App(app) => tooltip(
                    &app.name,
                    dock.shortcut_for(&item.id).map(ToString::to_string),
                    palette,
                ),
                ItemKind::Plugin(_) => plugin_card
                    .take()
                    .unwrap_or_else(|| div().into_any_element()),
            };
            let content = div().relative().size_full().child(content);
            if !animate {
                return content.into_any_element();
            }
            // Each item's content fades up as the card arrives or glides over.
            // A tooltip is a single word: it only needs a quick cross-fade.
            // Tooltip text starts part-way in, so the pill never shows empty.
            let (duration, rise_by, floor) = match item.kind {
                ItemKind::App(_) => (Duration::from_millis(90), 0.0, 0.4),
                _ => (Duration::from_millis(200), 3.0, 0.0),
            };
            content
                .with_animation(
                    SharedString::from(format!("card-content:{}", item.id)),
                    Animation::new(duration),
                    move |content, t| {
                        let rise = motion::sample(motion::CARD_IN.curve, t);
                        content
                            .opacity(floor + (1.0 - floor) * ease_out(t))
                            .top(px((1.0 - rise) * rise_by))
                    },
                )
                .into_any_element()
        });

        let body = placement.map_or_else(
            || domain::geometry::Rect::new(0.0, 0.0, 0.0, 0.0),
            |placement| placement.body(),
        );
        let hover = self.dock.clone();
        let card = div()
            .id("card")
            .test_support()
            .absolute()
            .left(px(body.x as f32))
            .top(px(body.y as f32))
            .w(px(body.width as f32))
            .h(px(body.height as f32))
            .text_color(palette.label)
            .on_hover(move |hovered: &bool, _, cx: &mut App| {
                hover.update(cx, |dock, cx| dock.set_card_hovered(*hovered, cx));
            })
            .children(content);

        div()
            .size_full()
            .relative()
            .children(
                placement.map(|placement| silhouette(placement, palette.surface, palette.stroke)),
            )
            .child(card)
    }
}

/// The card's silhouette: filled when the material is replaced by an opaque
/// surface (Reduce Transparency), and always edged with a hairline that runs
/// around the arrow too, so the arrow reads as part of the card.
fn silhouette(placement: CardPlacement, fill: Hsla, stroke: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let origin = bounds.origin;
            let (width, height) = (placement.frame.width, placement.frame.height);
            // Filled shapes use the outline as is; the 1-point stroke is pulled
            // in by half its width so no side is clipped by the window edge.
            let place = |p: geometry::Point, inset: f64| {
                let x = inset + p.x * (width - 2.0 * inset) / width;
                let y = inset + p.y * (height - 2.0 * inset) / height;
                point(origin.x + px(x as f32), origin.y + px(y as f32))
            };
            let trace = |mut builder: PathBuilder, inset: f64| {
                let at = |p| place(p, inset);
                for step in placement.outline() {
                    match step {
                        PathStep::Move(to) => builder.move_to(at(to)),
                        PathStep::Line(to) => builder.line_to(at(to)),
                        PathStep::Cubic {
                            control_a,
                            control_b,
                            to,
                        } => builder.cubic_bezier_to(at(to), at(control_a), at(control_b)),
                        PathStep::Close => builder.close(),
                    }
                }
                builder.build().ok()
            };
            if fill.a > 0.0
                && let Some(path) = trace(PathBuilder::fill(), 0.0)
            {
                window.paint_path(path, fill);
            }
            if let Some(path) = trace(PathBuilder::stroke(px(1.0)), 0.5) {
                window.paint_path(path, stroke);
            }
        },
    )
    .absolute()
    .size_full()
}

fn tooltip(name: &str, shortcut: Option<String>, palette: Palette) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(TOOLTIP_HINT_GAP as f32))
        .text_size(px(text::CALLOUT))
        .child(name.to_string())
        .children(shortcut.map(|shortcut| div().text_color(palette.secondary).child(shortcut)))
        .into_any_element()
}

pub(crate) fn card_body() -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .px(px(14.0))
        .py(px(12.0))
}

pub(crate) fn title(label: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(text::TITLE3))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

fn message_card(heading: &str, message: &str, palette: Palette) -> AnyElement {
    card_body()
        .gap(px(4.0))
        .child(title(heading.to_string()))
        .child(
            div()
                .text_size(px(text::CALLOUT))
                .text_color(palette.secondary)
                .child(message.to_string()),
        )
        .into_any_element()
}

pub(crate) fn gauge(fraction: f32, color: Hsla, palette: Palette) -> impl IntoElement {
    div()
        .h(px(6.0))
        .w_full()
        .rounded_full()
        .bg(palette.track)
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(color),
        )
}

pub(crate) fn meter(
    glyph: Option<SharedString>,
    label: impl Into<SharedString>,
    value: String,
    fraction: f32,
    color: Hsla,
    palette: Palette,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .children(glyph.map(|glyph| icon(glyph, 13.0, palette.secondary)))
                .child(
                    div()
                        .text_size(px(text::CALLOUT))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.into()),
                )
                .child(
                    div()
                        .ml_auto()
                        .text_size(px(text::CALLOUT))
                        .text_color(palette.secondary)
                        .child(value),
                ),
        )
        .child(gauge(fraction, color, palette))
}
