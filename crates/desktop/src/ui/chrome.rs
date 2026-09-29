//! Window chrome for Linux, where the app draws its own frame so windows
//! look as they do on macOS: a transparent title bar with traffic lights,
//! rounded corners, a hairline edge and a soft shadow. macOS and Windows
//! keep the system frame and never build this view. Without a compositor
//! (or a window manager that lets clients draw their shadow) the window
//! manager's frame is used instead.

use crate::app::dock::Dock;
use crate::ui::theme::{self, Palette};
use gpui_kit::{
    AnyView, App, AppContext as _, Bounds, BoxShadow, Context, CursorStyle, Entity, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, Pixels, Point, Render, ResizeEdge, Size, StatefulInteractiveElement as _,
    Styled as _, Window, WindowBounds, WindowOptions, assets::IconName, canvas, div, point,
    prelude::FluentBuilder as _, px, rgba, size, svg,
};
use std::{cell::RefCell, collections::HashSet};

/// macOS window corner radius.
pub const RADIUS: f32 = 10.0;
/// Room around the window for its shadow, which the window manager is
/// told is not part of the window.
const SHADOW: f32 = 24.0;
/// Half the width of the band around the frame's edge that resizes.
const RESIZE_BAND: f32 = 5.0;
/// The traffic lights: a 12-point circle in a 14-point frame, 20 apart.
const LIGHT_FRAME: f32 = 14.0;
const LIGHT: f32 = 12.0;
const LIGHT_PITCH: f32 = 20.0;
/// AppKit's standard position for the first light in a 28-point title bar.
const DEFAULT_LIGHTS: (f32, f32) = (8.0, 7.0);

/// Whether windows get the drawn chrome on this platform.
const DRAWN: bool = cfg!(target_os = "linux");

thread_local! {
    /// Windows whose frame the app draws.
    static FRAMED: RefCell<HashSet<gpui_kit::WindowId>> = RefCell::default();
}

/// Whether this window's frame, and so its surface, is drawn by the app.
pub fn is_drawn(window: &Window) -> bool {
    FRAMED.with(|framed| {
        framed
            .borrow()
            .contains(&window.window_handle().window_id())
    })
}

/// The inset from the window's edge to its visible frame.
fn inset(window: &Window) -> Pixels {
    if is_drawn(window) && !window.is_maximized() && !window.is_fullscreen() {
        px(SHADOW)
    } else {
        px(0.0)
    }
}

/// The size of the window's content: the viewport without the shadow.
pub fn content_size(window: &Window) -> Size<Pixels> {
    let inset = inset(window) * 2.0;
    let viewport = window.viewport_size();
    size(viewport.width - inset, viewport.height - inset)
}

/// What the title bar allows, from the window's options.
#[derive(Clone, Copy)]
struct Controls {
    lights: Point<Pixels>,
    /// Height of the strip that drags the window.
    title_height: Pixels,
    resizable: bool,
    minimizable: bool,
}

pub struct Chrome {
    dock: Entity<Dock>,
    content: AnyView,
    controls: Controls,
    /// A press in the title bar that becomes a move once the pointer moves.
    pressed: bool,
    lights_hovered: bool,
    /// The shadow width last reported to the window manager.
    extents: Option<Pixels>,
}

#[cfg(target_os = "linux")]
fn can_draw_frame() -> bool {
    ::platform::native::can_draw_frame()
}

#[cfg(not(target_os = "linux"))]
fn can_draw_frame() -> bool {
    false
}

/// Opens a titled window, with drawn chrome around `build`'s view where
/// the platform needs it. Returns the content view either way.
pub fn open_window<V: Render>(
    mut options: WindowOptions,
    dock: &Entity<Dock>,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui_kit::Result<(gpui_kit::AnyWindowHandle, Entity<V>)> {
    if !DRAWN || !can_draw_frame() {
        return gpui_kit::open_window(options, cx, build);
    }
    let lights = options
        .titlebar
        .as_ref()
        .and_then(|titlebar| titlebar.traffic_light_position)
        .unwrap_or(point(px(DEFAULT_LIGHTS.0), px(DEFAULT_LIGHTS.1)));
    let controls = Controls {
        lights,
        // The strip the lights sit in, centered vertically.
        title_height: lights.y * 2.0 + px(LIGHT_FRAME),
        resizable: options.is_resizable,
        minimizable: options.is_minimizable,
    };
    // The window grows by the shadow on every side; the content keeps the
    // size and position asked for.
    let grow = |bounds: Bounds<Pixels>| Bounds {
        origin: bounds.origin - point(px(SHADOW), px(SHADOW)),
        size: bounds.size + size(px(SHADOW * 2.0), px(SHADOW * 2.0)),
    };
    options.window_bounds = options.window_bounds.map(|bounds| match bounds {
        WindowBounds::Windowed(bounds) => WindowBounds::Windowed(grow(bounds)),
        other => other,
    });
    let origin = match options.window_bounds {
        Some(WindowBounds::Windowed(bounds)) => Some(bounds.origin),
        _ => None,
    };
    options.window_min_size = options
        .window_min_size
        .map(|min| min + size(px(SHADOW * 2.0), px(SHADOW * 2.0)));

    let mut content = None;
    let dock = dock.clone();
    let (handle, _) = gpui_kit::open_window(options, cx, |window, cx| {
        FRAMED.with(|framed| {
            framed
                .borrow_mut()
                .insert(window.window_handle().window_id())
        });
        let view = build(window, cx);
        content = Some(view.clone());
        cx.new(|cx| {
            // Forget the window once it's gone.
            let id = window.window_handle().window_id();
            cx.on_release(move |_, _| {
                FRAMED.with(|framed| framed.borrow_mut().remove(&id));
            })
            .detach();
            Chrome {
                dock,
                content: view.into(),
                controls,
                pressed: false,
                lights_hovered: false,
                extents: None,
            }
        })
    })?;
    #[cfg(target_os = "linux")]
    if let Some(origin) = origin {
        handle
            .update(cx, |_, window, _| {
                if let Some(native) = ::platform::native::window_handle(window) {
                    ::platform::native::draw_own_frame(&native, f64::from(SHADOW));
                    ::platform::native::move_window(
                        &native,
                        (
                            f64::from(f32::from(origin.x)),
                            f64::from(f32::from(origin.y)),
                        ),
                    );
                }
            })
            .ok();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = origin;
    Ok((handle, content.expect("open_window ran its build closure")))
}

impl Render for Chrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inset = inset(window);
        self.report_extents(inset, window);
        let palette = Palette::new(window, self.dock.read(cx).accessibility);
        let square = inset == px(0.0);
        let radius = if square { px(0.0) } else { px(RADIUS) };
        let controls = self.controls;
        let active = window.is_window_active();
        // Like an AppKit window: a wide, soft shadow that lifts while the
        // window is active, over a tight contact shadow.
        let shadows = if square {
            Vec::new()
        } else {
            vec![
                BoxShadow {
                    color: gpui_kit::hsla(0.0, 0.0, 0.0, if active { 0.28 } else { 0.16 }),
                    offset: point(px(0.0), px(6.0)),
                    blur_radius: px(14.0),
                    spread_radius: px(-2.0),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::hsla(0.0, 0.0, 0.0, 0.22),
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(1.0),
                    spread_radius: px(0.0),
                    inset: false,
                },
            ]
        };

        let title_bar = div()
            .id("title-bar")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(controls.title_height)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.pressed = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.pressed = false),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if std::mem::take(&mut this.pressed) {
                    window.start_window_move();
                }
            }))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.zoom_window();
                }
            })
            .on_mouse_down(MouseButton::Right, |event, window, _| {
                window.show_window_menu(event.position)
            });

        let frame = div()
            .size_full()
            .relative()
            .rounded(radius)
            .bg(theme::window_surface(window))
            .shadow(shadows)
            .when(!square, |this| this.border_1().border_color(palette.stroke))
            // Behind the content, so the content's own controls win clicks.
            .child(title_bar)
            .child(div().size_full().child(self.content.clone()))
            .child(self.traffic_lights(window, cx));

        div()
            .size_full()
            .p(inset)
            .child(frame)
            .when(!square && controls.resizable, |this| {
                this.child(resize_edges(inset))
            })
    }
}

impl Chrome {
    fn report_extents(&mut self, inset: Pixels, window: &mut Window) {
        if self.extents == Some(inset) {
            return;
        }
        self.extents = Some(inset);
        #[cfg(target_os = "linux")]
        if let Some(native) = ::platform::native::window_handle(window) {
            ::platform::native::draw_own_frame(&native, f64::from(f32::from(inset)));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = window;
    }

    fn traffic_lights(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let controls = self.controls;
        let glyphs = self.lights_hovered;
        let active = window.is_window_active();
        let inactive = if theme::is_dark(window) {
            color(0xffffff26)
        } else {
            color(0x00000026)
        };
        let light =
            |id: &'static str, fill: u32, edge: u32, glyph: IconName, ink: u32, enabled: bool| {
                let lit = enabled && (active || glyphs);
                div()
                    .id(id)
                    .size(px(LIGHT_FRAME))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(LIGHT))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if lit { color(fill) } else { inactive })
                            .border_1()
                            .border_color(if lit { color(edge) } else { inactive })
                            .when(lit && glyphs, |this| {
                                this.child(
                                    svg()
                                        .path(glyph.path())
                                        .size(px(8.0))
                                        .text_color(color(ink)),
                                )
                            }),
                    )
            };
        div()
            .id("traffic-lights")
            .absolute()
            // The border is inside the frame; AppKit measures from its edge.
            .left(controls.lights.x - px(1.0))
            .top(controls.lights.y - px(1.0))
            .flex()
            .gap(px(LIGHT_PITCH - LIGHT_FRAME))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.lights_hovered = *hovered;
                cx.notify();
            }))
            .child(
                light(
                    "close",
                    0xff5f57ff,
                    0xe0443eff,
                    IconName::Close,
                    0x4d0000ff,
                    true,
                )
                .on_click(|_, window, _| request_close(window)),
            )
            .child(
                light(
                    "minimize",
                    0xfebc2eff,
                    0xdea123ff,
                    IconName::Minus,
                    0x995700ff,
                    controls.minimizable,
                )
                .when(controls.minimizable, |this| {
                    this.on_click(|_, window, _| window.minimize_window())
                }),
            )
            .child(
                light(
                    "zoom",
                    0x28c840ff,
                    0x1aab29ff,
                    IconName::Plus,
                    0x006500ff,
                    controls.resizable,
                )
                .when(controls.resizable, |this| {
                    this.on_click(|_, window, _| window.zoom_window())
                }),
            )
    }
}

/// Closes the window the way the window manager's close does, so the
/// views hear about it as they do from the macOS close button.
fn request_close(window: &mut Window) {
    #[cfg(target_os = "linux")]
    if let Some(native) = ::platform::native::window_handle(window) {
        ::platform::native::request_close(&native);
        return;
    }
    window.remove_window();
}

fn color(value: u32) -> Hsla {
    rgba(value).into()
}

/// The edge or corner of `frame` near `position`, if it starts a resize.
fn resize_edge(frame: Bounds<Pixels>, position: Point<Pixels>) -> Option<ResizeEdge> {
    let (band, corner) = (px(RESIZE_BAND), px(RESIZE_BAND * 3.0));
    let (left, top) = (frame.origin.x, frame.origin.y);
    let (right, bottom) = (left + frame.size.width, top + frame.size.height);
    let (x, y) = (position.x, position.y);
    if x < left - band || x > right + band || y < top - band || y > bottom + band {
        return None;
    }
    let at_left = x < left + band;
    let at_right = x > right - band;
    let at_top = y < top + band;
    let at_bottom = y > bottom - band;
    let near_left = x < left + corner;
    let near_right = x > right - corner;
    let near_top = y < top + corner;
    let near_bottom = y > bottom - corner;
    Some(if (at_top && near_left) || (at_left && near_top) {
        ResizeEdge::TopLeft
    } else if (at_top && near_right) || (at_right && near_top) {
        ResizeEdge::TopRight
    } else if (at_bottom && near_left) || (at_left && near_bottom) {
        ResizeEdge::BottomLeft
    } else if (at_bottom && near_right) || (at_right && near_bottom) {
        ResizeEdge::BottomRight
    } else if at_top {
        ResizeEdge::Top
    } else if at_bottom {
        ResizeEdge::Bottom
    } else if at_left {
        ResizeEdge::Left
    } else if at_right {
        ResizeEdge::Right
    } else {
        return None;
    })
}

fn cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// Invisible handles along the frame's edges and corners that resize it.
fn resize_edges(inset: Pixels) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, gpui_kit::HitboxBehavior::Normal),
        move |bounds: Bounds<Pixels>, hitbox, window, _| {
            let frame = Bounds {
                origin: bounds.origin + point(inset, inset),
                size: bounds.size - size(inset * 2.0, inset * 2.0),
            };
            let current = resize_edge(frame, window.mouse_position());
            if let Some(edge) = current {
                window.set_cursor_style(cursor(edge), &hitbox);
            }
            // Repaint when the pointer crosses into or out of an edge, so
            // the cursor follows it.
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if phase.bubble() && resize_edge(frame, event.position) != current {
                    window.refresh();
                }
            });
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase.capture()
                    && event.button == MouseButton::Left
                    && let Some(edge) = resize_edge(frame, event.position)
                {
                    window.start_window_resize(edge);
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizes_from_a_band_around_the_frame_edge() {
        let frame = Bounds {
            origin: point(px(24.0), px(24.0)),
            size: size(px(400.0), px(300.0)),
        };
        let at = |x: f32, y: f32| resize_edge(frame, point(px(x), px(y)));
        assert_eq!(at(22.0, 150.0), Some(ResizeEdge::Left));
        assert_eq!(at(426.0, 150.0), Some(ResizeEdge::Right));
        assert_eq!(at(200.0, 25.0), Some(ResizeEdge::Top));
        assert_eq!(at(25.0, 25.0), Some(ResizeEdge::TopLeft));
        assert_eq!(at(420.0, 322.0), Some(ResizeEdge::BottomRight));
        // Inside the frame, and out in the shadow, nothing resizes.
        assert_eq!(at(200.0, 150.0), None);
        assert_eq!(at(5.0, 150.0), None);
    }
}
