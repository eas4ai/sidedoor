//! Draws what a plugin renders. Lowercase nodes are GPUI's own elements
//! (`div`, `svg`, `img`); capitalized ones are this app's native
//! components, drawn by the same code as the built-in widgets. Every node
//! takes GPUI style props (`flex_col`, `gap`, `text_color`, …), applied on
//! top of its native look.

use crate::{
    dock::Dock,
    plugin::{Node, icon_path},
    style::{Palette, text},
    views,
};
use gpui_kit::{
    AnyElement, App, DefiniteLength, ElementId, Entity, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, Length, ObjectFit, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled, StyledImage as _, TestSupportExt as _, auto,
    component::{Disableable as _, Sizable as _, switch::Switch},
    div, img,
    prelude::FluentBuilder as _,
    px, relative, rgba, svg,
};
use serde_json::{Map, Value};

type Props = Map<String, Value>;

/// Where a plugin's nodes are drawn, and who hears its events.
#[derive(Clone)]
pub struct Surface {
    pub dock: Entity<Dock>,
    pub plugin: SharedString,
    pub palette: Palette,
}

impl Surface {
    /// Draws `nodes`; `root` names the surface, e.g. `"card"`.
    pub fn render(&self, nodes: &[Node], root: &str) -> Vec<AnyElement> {
        nodes
            .iter()
            .enumerate()
            .map(|(index, node)| self.node(node, &format!("{root}/{index}")))
            .collect()
    }

    fn node(&self, node: &Node, path: &str) -> AnyElement {
        let Node::Element {
            kind,
            props,
            children,
        } = node
        else {
            let Node::Text(text) = node else {
                unreachable!()
            };
            return text.clone().into_any_element();
        };
        let children = || self.render(children, path);
        let palette = self.palette;
        match kind.as_str() {
            "div" => self.div(props, path, children()),
            "svg" => style(
                svg()
                    .path(icon_path(string(props, "path").unwrap_or("circle")))
                    .size(px(14.0))
                    .text_color(palette.label),
                props,
                palette,
            )
            .into_any_element(),
            "img" => {
                let fit = match string(props, "object_fit") {
                    Some("cover") => ObjectFit::Cover,
                    Some("fill") => ObjectFit::Fill,
                    _ => ObjectFit::Contain,
                };
                style(
                    img(std::path::PathBuf::from(
                        string(props, "src").unwrap_or_default(),
                    ))
                    .object_fit(fit),
                    props,
                    palette,
                )
                .into_any_element()
            }
            "Card" => {
                let heading = string(props, "title").map(|title| {
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(views::title(title.to_string()))
                        .children(string(props, "accessory").map(|accessory| {
                            div()
                                .text_size(px(text::CALLOUT))
                                .text_color(palette.secondary)
                                .child(accessory.to_string())
                        }))
                });
                style(
                    views::card_body()
                        .gap(px(8.0))
                        .children(heading)
                        .children(children()),
                    props,
                    palette,
                )
                .into_any_element()
            }
            "Title" => {
                style(views::title("").children(children()), props, palette).into_any_element()
            }
            "Text" => {
                let (size, weight) = match string(props, "variant") {
                    Some("callout") => (text::CALLOUT, FontWeight::NORMAL),
                    Some("caption") => (text::CAPTION, FontWeight::NORMAL),
                    Some("headline") => (text::BODY, FontWeight::SEMIBOLD),
                    Some("title") => (text::TITLE3, FontWeight::SEMIBOLD),
                    Some("display") => (text::DISPLAY, FontWeight::SEMIBOLD),
                    _ => (text::BODY, FontWeight::NORMAL),
                };
                let color = if flag(props, "tertiary") {
                    palette.tertiary
                } else if flag(props, "secondary") {
                    palette.secondary
                } else {
                    palette.label
                };
                style(
                    div()
                        .text_size(px(size))
                        .font_weight(weight)
                        .text_color(color)
                        .children(children()),
                    props,
                    palette,
                )
                .into_any_element()
            }
            "Icon" => {
                let size = number(props, "icon_size").unwrap_or(14.0);
                let color = color(props.get("color"), palette).unwrap_or(palette.label);
                style(
                    svg()
                        .path(icon_path(string(props, "name").unwrap_or("circle")))
                        .size(px(size))
                        .flex_shrink_0()
                        .text_color(color),
                    props,
                    palette,
                )
                .into_any_element()
            }
            "Button" => self.button(props, path, children()),
            "Switch" => {
                let (dock, plugin) = (self.dock.clone(), self.plugin.clone());
                let handler = handler(props, "on_change");
                let switch = Switch::new(self.id(props, path))
                    .small()
                    .color(palette.blue)
                    .checked(flag(props, "checked"))
                    .disabled(flag(props, "disabled"))
                    .on_click(move |checked, _, cx| {
                        if let Some(handler) = &handler {
                            send(&dock, &plugin, handler, Value::Bool(*checked), cx);
                        }
                    });
                style(div().flex_shrink_0().child(switch), props, palette).into_any_element()
            }
            "Segmented" => self.segmented(props, path),
            "Meter" => style(
                div().child(views::meter(
                    string(props, "icon").map(|name| icon_path(name).into()),
                    string(props, "label").unwrap_or_default().to_string(),
                    string(props, "value").unwrap_or_default().to_string(),
                    number(props, "fraction").unwrap_or(0.0),
                    color(props.get("color"), palette).unwrap_or(palette.blue),
                    palette,
                )),
                props,
                palette,
            )
            .into_any_element(),
            "ListRow" => self.list_row(props, path),
            "Sparkline" => {
                let bar = color(props.get("color"), palette).unwrap_or(palette.tertiary);
                let values: Vec<f32> = props
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_f64)
                            .map(|value| value as f32)
                            .collect()
                    })
                    .unwrap_or_default();
                style(
                    div().h(px(18.0)).flex().items_end().gap(px(2.0)).children(
                        values.into_iter().map(move |value| {
                            div()
                                .w(px(3.0))
                                .h(relative(value.clamp(0.12, 1.0)))
                                .rounded(px(1.0))
                                .bg(bar)
                        }),
                    ),
                    props,
                    palette,
                )
                .into_any_element()
            }
            "Footer" => style(
                div()
                    .mt_auto()
                    .pt(px(8.0))
                    .border_t_1()
                    .border_color(palette.separator)
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(text::SUBHEADLINE))
                    .text_color(palette.tertiary)
                    .children(children()),
                props,
                palette,
            )
            .into_any_element(),
            "Keycap" => style(
                div()
                    .min_w(px(20.0))
                    .h(px(20.0))
                    .px(px(5.0))
                    .rounded(px(5.0))
                    .bg(palette.keycap)
                    .border_1()
                    .border_color(palette.stroke)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(text::SUBHEADLINE))
                    .font_weight(FontWeight::MEDIUM)
                    .children(children()),
                props,
                palette,
            )
            .into_any_element(),
            "Divider" => style(
                div()
                    .h(px(1.0))
                    .w_full()
                    .flex_shrink_0()
                    .bg(palette.separator),
                props,
                palette,
            )
            .into_any_element(),
            "Spacer" => style(div().flex_1(), props, palette).into_any_element(),
            // An unknown tag still shows its children.
            _ => style(div(), props, palette)
                .children(children())
                .into_any_element(),
        }
    }

    /// An element id that stays put across renders: the plugin's own `id`
    /// prop when it gave one, else the node's place in the tree.
    fn id(&self, props: &Props, path: &str) -> ElementId {
        let local = match props.get("id") {
            Some(Value::String(id)) => id.clone(),
            Some(Value::Number(id)) => id.to_string(),
            _ => path.to_string(),
        };
        ElementId::Name(format!("plugin:{}:{local}", self.plugin).into())
    }

    fn div(&self, props: &Props, path: &str, children: Vec<AnyElement>) -> AnyElement {
        let palette = self.palette;
        let click = handler(props, "on_click");
        let hover = handler(props, "on_hover");
        let hover_style = props.get("hover").and_then(Value::as_object).cloned();
        let active_style = props.get("active").and_then(Value::as_object).cloned();
        let base = style(div(), props, palette).children(children);
        if click.is_none() && hover.is_none() && hover_style.is_none() && active_style.is_none() {
            return base.into_any_element();
        }
        let (dock, plugin) = (self.dock.clone(), self.plugin.clone());
        let (hover_dock, hover_plugin) = (dock.clone(), plugin.clone());
        base.id(self.id(props, path))
            .test_support()
            .when_some(hover_style, |el, hovered| {
                el.hover(move |s| style(s, &hovered, palette))
            })
            .when_some(active_style, |el, pressed| {
                el.active(move |s| style(s, &pressed, palette))
            })
            .when_some(click, |el, handler| {
                el.on_click(move |_, _, cx| send(&dock, &plugin, &handler, Value::Null, cx))
            })
            .when_some(hover, |el, handler| {
                el.on_hover(move |hovered: &bool, _, cx| {
                    send(
                        &hover_dock,
                        &hover_plugin,
                        &handler,
                        Value::Bool(*hovered),
                        cx,
                    )
                })
            })
            .into_any_element()
    }

    fn button(&self, props: &Props, path: &str, children: Vec<AnyElement>) -> AnyElement {
        let palette = self.palette;
        let disabled = flag(props, "disabled");
        let label = string(props, "label").map(|label| label.to_string());
        let glyph = string(props, "icon").map(icon_path);
        let variant = string(props, "variant").unwrap_or("push");
        let base = div()
            .id(self.id(props, path))
            .test_support()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .gap(px(5.0))
            .text_size(px(text::BODY));
        let (base, tint) = match variant {
            "link" => (
                base.px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(5.0))
                    .text_color(palette.blue)
                    .hover(|s| s.bg(palette.fill)),
                palette.blue,
            ),
            "primary" => (
                base.h(px(24.0))
                    .px(px(12.0))
                    .rounded(px(6.0))
                    .bg(palette.blue)
                    .text_color(palette.on_accent)
                    .font_weight(FontWeight::MEDIUM),
                palette.on_accent,
            ),
            other => {
                let tint = if other == "destructive" {
                    palette.red
                } else {
                    palette.label
                };
                (
                    base.h(px(24.0))
                        .px(px(12.0))
                        .rounded(px(6.0))
                        .bg(palette.keycap)
                        .border_1()
                        .border_color(palette.stroke)
                        .text_color(tint),
                    tint,
                )
            }
        };
        let base = style(base, props, palette)
            .children(glyph.map(|glyph| views::icon(glyph.into(), 13.0, tint)))
            .children(label)
            .children(children);
        if disabled {
            return base.opacity(0.45).into_any_element();
        }
        let click = handler(props, "on_click");
        let (dock, plugin) = (self.dock.clone(), self.plugin.clone());
        base.active(|s| s.opacity(0.7))
            .when_some(click, |el, handler| {
                el.on_click(move |_, _, cx| send(&dock, &plugin, &handler, Value::Null, cx))
            })
            .into_any_element()
    }

    fn segmented(&self, props: &Props, path: &str) -> AnyElement {
        let palette = self.palette;
        let selected = number(props, "selected").unwrap_or(0.0) as usize;
        let change = handler(props, "on_change");
        let options: Vec<String> = props
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| option.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let id = self.id(props, path);
        let segments = options.into_iter().enumerate().map(|(index, label)| {
            let (dock, plugin, change) = (self.dock.clone(), self.plugin.clone(), change.clone());
            let chosen = index == selected;
            div()
                .id(ElementId::NamedChild(
                    std::sync::Arc::new(id.clone()),
                    index.to_string().into(),
                ))
                .test_support()
                .flex_1()
                .px(px(9.0))
                .py(px(3.0))
                .rounded(px(5.0))
                .flex()
                .justify_center()
                .text_size(px(text::CALLOUT))
                .when_else(
                    chosen,
                    |segment| segment.bg(palette.segment).font_weight(FontWeight::MEDIUM),
                    |segment| {
                        segment
                            .text_color(palette.secondary)
                            .hover(|s| s.text_color(palette.label))
                    },
                )
                .on_click(move |_, _, cx| {
                    if let Some(handler) = &change {
                        send(&dock, &plugin, handler, Value::from(index), cx);
                    }
                })
                .child(label)
        });
        style(
            div()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(7.0))
                .bg(palette.fill)
                .children(segments),
            props,
            palette,
        )
        .into_any_element()
    }

    fn list_row(&self, props: &Props, path: &str) -> AnyElement {
        let palette = self.palette;
        let leading = string(props, "icon").map(|name| {
            div()
                .size(px(28.0))
                .flex_shrink_0()
                .rounded(px(6.0))
                .bg(palette.fill)
                .flex()
                .items_center()
                .justify_center()
                .child(views::icon(icon_path(name).into(), 14.0, palette.secondary))
        });
        let row = div()
            .id(self.id(props, path))
            .test_support()
            .min_h(px(40.0))
            .py(px(4.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(8.0))
            .children(leading)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(text::BODY))
                            .child(string(props, "title").unwrap_or_default().to_string()),
                    )
                    .children(string(props, "subtitle").map(|subtitle| {
                        div()
                            .truncate()
                            .text_size(px(text::SUBHEADLINE))
                            .text_color(palette.secondary)
                            .child(subtitle.to_string())
                    })),
            )
            .children(string(props, "accessory").map(|accessory| {
                div()
                    .flex_shrink_0()
                    .text_size(px(text::CALLOUT))
                    .text_color(palette.secondary)
                    .child(accessory.to_string())
            }));
        let row = style(row, props, palette);
        match handler(props, "on_click") {
            Some(handler) => {
                let (dock, plugin) = (self.dock.clone(), self.plugin.clone());
                row.hover(|s| s.bg(palette.fill))
                    .active(|s| s.opacity(0.7))
                    .on_click(move |_, _, cx| send(&dock, &plugin, &handler, Value::Null, cx))
                    .into_any_element()
            }
            None => row.into_any_element(),
        }
    }
}

fn send(dock: &Entity<Dock>, plugin: &str, handler: &str, value: Value, cx: &mut App) {
    dock.update(cx, |dock, _| dock.plugin_event(plugin, handler, value));
}

fn handler(props: &Props, name: &str) -> Option<SharedString> {
    let key = props.get(name)?.get("$h")?.as_str()?;
    Some(key.to_string().into())
}

fn string<'a>(props: &'a Props, name: &str) -> Option<&'a str> {
    props.get(name)?.as_str()
}

fn number(props: &Props, name: &str) -> Option<f32> {
    Some(props.get(name)?.as_f64()? as f32)
}

fn flag(props: &Props, name: &str) -> bool {
    props.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// A palette token or a hex color.
fn color(value: Option<&Value>, palette: Palette) -> Option<Hsla> {
    let name = value?.as_str()?;
    Some(match name {
        "label" => palette.label,
        "secondary" => palette.secondary,
        "tertiary" => palette.tertiary,
        "separator" => palette.separator,
        "stroke" => palette.stroke,
        "fill" => palette.fill,
        "track" => palette.track,
        "accent" | "blue" => palette.blue,
        "on_accent" => palette.on_accent,
        "green" => palette.green,
        "orange" => palette.orange,
        "red" => palette.red,
        "purple" => palette.purple,
        "transparent" => gpui_kit::transparent_black(),
        hex => return parse_hex(hex),
    })
}

fn parse_hex(text: &str) -> Option<Hsla> {
    let hex = text.strip_prefix('#')?;
    let expanded: String = match hex.len() {
        3 | 4 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => hex.to_string(),
        _ => return None,
    };
    let value = u32::from_str_radix(&expanded, 16).ok()?;
    let value = if expanded.len() == 6 {
        (value << 8) | 0xff
    } else {
        value
    };
    Some(rgba(value).into())
}

fn length(value: &Value) -> Option<Length> {
    match value {
        Value::Number(number) => Some(px(number.as_f64()? as f32).into()),
        Value::String(text) if text == "full" => Some(relative(1.0).into()),
        Value::String(text) if text == "auto" => Some(auto()),
        Value::String(text) => {
            let percent: f32 = text.strip_suffix('%')?.trim().parse().ok()?;
            Some(relative(percent / 100.0).into())
        }
        _ => None,
    }
}

fn definite(value: &Value) -> Option<DefiniteLength> {
    match length(value)? {
        Length::Definite(length) => Some(length),
        Length::Auto => None,
    }
}

/// Applies GPUI style props by the names of the `Styled` methods.
pub fn style<S: Styled>(element: S, props: &Props, palette: Palette) -> S {
    props.iter().fold(element, |element, (name, value)| {
        apply(element, name, value, palette)
    })
}

fn apply<S: Styled>(el: S, name: &str, value: &Value, palette: Palette) -> S {
    macro_rules! flags {
        ($($method:ident),* $(,)?) => {
            match name {
                $(stringify!($method) => {
                    return if value.as_bool() == Some(true) { el.$method() } else { el };
                })*
                _ => {}
            }
        };
    }
    macro_rules! points {
        ($($method:ident),* $(,)?) => {
            match name {
                $(stringify!($method) => {
                    return match value.as_f64() {
                        Some(n) => el.$method(px(n as f32)),
                        None => el,
                    };
                })*
                _ => {}
            }
        };
    }
    macro_rules! lengths {
        ($($method:ident),* $(,)?) => {
            match name {
                $(stringify!($method) => {
                    return match length(value) {
                        Some(length) => el.$method(length),
                        None => el,
                    };
                })*
                _ => {}
            }
        };
    }
    macro_rules! insets {
        ($($method:ident),* $(,)?) => {
            match name {
                $(stringify!($method) => {
                    return match definite(value) {
                        Some(length) => el.$method(length),
                        None => el,
                    };
                })*
                _ => {}
            }
        };
    }
    flags!(
        flex,
        flex_col,
        flex_row,
        flex_wrap,
        flex_1,
        flex_auto,
        flex_none,
        flex_shrink_0,
        items_start,
        items_center,
        items_end,
        items_baseline,
        justify_start,
        justify_center,
        justify_end,
        justify_between,
        justify_around,
        size_full,
        w_full,
        h_full,
        min_w_0,
        mx_auto,
        mt_auto,
        ml_auto,
        relative,
        absolute,
        inset_0,
        overflow_hidden,
        rounded_full,
        border_1,
        border_t_1,
        border_b_1,
        shadow_sm,
        shadow_md,
        shadow_lg,
        italic,
        text_center,
        text_right,
        truncate,
        whitespace_nowrap,
        cursor_pointer,
    );
    points!(
        gap, gap_x, gap_y, p, px, py, pt, pr, pb, pl, m, mx, my, mt, mr, mb, ml, rounded,
        text_size,
    );
    lengths!(size, w, h, min_w, min_h, max_w, max_h);
    insets!(top, right, bottom, left);
    match name {
        "bg" => match color(Some(value), palette) {
            Some(color) => el.bg(color),
            None => el,
        },
        "text_color" => match color(Some(value), palette) {
            Some(color) => el.text_color(color),
            None => el,
        },
        "border_color" => match color(Some(value), palette) {
            Some(color) => el.border_color(color),
            None => el,
        },
        "opacity" => match value.as_f64() {
            Some(opacity) => el.opacity(opacity as f32),
            None => el,
        },
        // `true` is a factor of 1, as in CSS's `flex-grow: 1`.
        "flex_grow" | "flex_shrink" => {
            let factor = match value {
                Value::Bool(true) => 1.0,
                Value::Number(number) => number.as_f64().unwrap_or(0.0) as f32,
                _ => return el,
            };
            if name == "flex_grow" {
                el.flex_grow(factor)
            } else {
                el.flex_shrink(factor)
            }
        }
        "border" => match value.as_f64() {
            Some(width) if width >= 4.0 => el.border_4(),
            Some(width) if width >= 2.0 => el.border_2(),
            Some(width) if width > 0.0 => el.border_1(),
            _ => el,
        },
        "line_height" => match value.as_f64() {
            // Small numbers are multiples of the font size, as in CSS.
            Some(n) if n <= 4.0 => el.line_height(relative(n as f32)),
            Some(n) => el.line_height(px(n as f32)),
            None => el,
        },
        "line_clamp" => match value.as_u64() {
            Some(lines) => el.line_clamp(lines as usize),
            None => el,
        },
        "font_family" => match value.as_str() {
            Some(family) => el.font_family(SharedString::from(family.to_string())),
            None => el,
        },
        "font_weight" => {
            let weight = match value {
                Value::String(name) => match name.as_str() {
                    "medium" => FontWeight::MEDIUM,
                    "semibold" => FontWeight::SEMIBOLD,
                    "bold" => FontWeight::BOLD,
                    _ => FontWeight::NORMAL,
                },
                Value::Number(number) => FontWeight(number.as_f64().unwrap_or(400.0) as f32),
                _ => return el,
            };
            el.font_weight(weight)
        }
        _ => el,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_tokens_or_hex() {
        assert_eq!(parse_hex("#fff"), Some(rgba(0xffffffff).into()));
        assert_eq!(parse_hex("#ff000080"), Some(rgba(0xff000080).into()));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("red"), None);
    }

    #[test]
    fn lengths_are_points_fractions_or_keywords() {
        assert_eq!(length(&Value::from(12)), Some(px(12.0).into()));
        assert_eq!(length(&Value::from("50%")), Some(relative(0.5).into()));
        assert_eq!(length(&Value::from("full")), Some(relative(1.0).into()));
        assert_eq!(length(&Value::from("auto")), Some(auto()));
        assert_eq!(length(&Value::from("wide")), None);
    }
}
