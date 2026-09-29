//! Native presentation of GPUI windows on X11. Geometry and timing come
//! from the same modules as macOS; this boundary converts to X pixels.
//!
//! The dock and card are override-redirect windows (GPUI's pop-up kind), so
//! no window manager places, decorates or focuses them: Sidedoor moves them
//! itself, shapes where they take clicks, and fades them through the
//! compositor's window opacity.

use crate::linux::{self, X, x};
use crate::{
    config::Appearance,
    geometry::{CardPlacement, PathStep, Point, Rect},
    motion::{self, Motion},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection as _,
    protocol::{
        shape::{self, ConnectionExt as _},
        xproto::{self, ConnectionExt as _},
    },
    wrapper::ConnectionExt as _,
};

pub use crate::linux::{services::LinuxPlatform as NativePlatform, system::open_config};

#[derive(Clone)]
pub struct NativeWindow {
    id: u32,
    state: Rc<State>,
}

struct State {
    alpha: Cell<f64>,
    previous: Cell<Option<ForegroundApp>>,
    /// Whether this is one of our unmanaged panels, which we place.
    panel: Cell<bool>,
    mapped: Cell<bool>,
    /// Where the window takes clicks while it is interactive.
    input: RefCell<Option<Vec<xproto::Rectangle>>>,
}

pub type NativeMaterial = NativeWindow;

/// The window the user was in, so focus can return to it.
#[derive(Clone, Copy)]
pub struct ForegroundApp(u32);

#[derive(Clone, Copy)]
pub enum Backdrop {
    Dock,
    Card,
}

#[derive(Clone, Copy)]
pub enum CardEntry {
    Snap,
    Glide { from: Rect },
    Pop { anchor: Point },
}

thread_local! {
    static APPEARANCE: Cell<Appearance> = const { Cell::new(Appearance::System) };
    static ANIMATIONS: RefCell<HashMap<u32, Animation>> = RefCell::default();
}

struct Animation {
    window: NativeWindow,
    start: Instant,
    motion: Motion,
    from: Rect,
    to: Rect,
    moves: bool,
    from_alpha: f64,
    to_alpha: f64,
}

/// How often animations advance; about one frame at 60 Hz.
const FRAME: Duration = Duration::from_millis(16);
/// How often shortcuts and clipboard changes are read while idle.
const IDLE: Duration = Duration::from_millis(40);

/// Advances window animations and delivers pressed shortcuts. The app
/// calls this from a foreground task, waiting the returned delay between
/// calls; nothing here blocks.
pub fn pump() -> Duration {
    crate::hotkeys::dispatch();
    crate::status_menu::dispatch();
    let windows: Vec<u32> = ANIMATIONS.with(|all| all.borrow().keys().copied().collect());
    for window in windows {
        step(window);
    }
    if let Some(x) = x() {
        let _ = x.conn.flush();
    }
    let animating = ANIMATIONS.with(|all| !all.borrow().is_empty());
    if animating { FRAME } else { IDLE }
}

pub fn set_accessory_policy() {
    // Unmanaged panels never appear in a taskbar; nothing else to set.
}

pub fn set_appearance(appearance: Appearance) {
    APPEARANCE.with(|value| value.set(appearance));
}

pub fn appearance(window: &gpui_kit::Window) -> gpui_kit::WindowAppearance {
    APPEARANCE.with(|value| match value.get() {
        Appearance::System => window.appearance(),
        Appearance::Light => gpui_kit::WindowAppearance::Light,
        Appearance::Dark => gpui_kit::WindowAppearance::Dark,
    })
}

pub fn frontmost_app() -> Option<ForegroundApp> {
    let window = linux::active_window()?;
    // Never hand focus back to ourselves.
    (linux::window_pid(window) != Some(std::process::id())).then_some(ForegroundApp(window))
}

pub fn activate_app(app: ForegroundApp) {
    let Some(x) = x() else {
        return;
    };
    // Source 2 identifies a pager-like tool acting for the user, which
    // window managers honor without focus-stealing prevention.
    let message = xproto::ClientMessageEvent::new(
        32,
        app.0,
        x.atoms._NET_ACTIVE_WINDOW,
        [2, x11rb::CURRENT_TIME, 0, 0, 0],
    );
    let _ = x.conn.send_event(
        false,
        x.root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        message,
    );
    let _ = x.conn.flush();
}

pub fn window_handle(window: &gpui_kit::Window) -> Option<NativeWindow> {
    linux::set_scale(f64::from(window.scale_factor()));
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let id = match handle.as_raw() {
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        RawWindowHandle::Xlib(handle) => handle.window as u32,
        _ => return None,
    };
    Some(NativeWindow {
        id,
        state: Rc::new(State {
            alpha: Cell::new(1.0),
            previous: Cell::new(None),
            panel: Cell::new(false),
            mapped: Cell::new(false),
            input: RefCell::new(None),
        }),
    })
}

pub fn configure_panel(
    window: &NativeWindow,
    corner_radius: f64,
    _backdrop: Backdrop,
) -> Option<NativeMaterial> {
    window.state.panel.set(true);
    let x = x()?;
    if let Some(geometry) = x
        .conn
        .get_geometry(window.id)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    {
        let scale = linux::primary_display().map_or(1.0, |display| display.scale);
        let points = rounded_rect(
            f64::from(geometry.width) / scale,
            f64::from(geometry.height) / scale,
            corner_radius,
        );
        set_shape(&x, window, &points, scale);
    }
    Some(window.clone())
}

pub fn set_material_hidden(_material: &NativeMaterial, _hidden: bool) {
    // Panels paint their own surface; there is no native material to hide.
}

/// X11 has no system material to show through a window, so the shared
/// palette paints opaque surfaces, as with Reduce Transparency on macOS.
pub fn has_material() -> bool {
    false
}

/// Regular windows keep the window manager's title bar and paint their
/// own surface. Window managers place new windows by their own policy;
/// center it on the primary display's work area, as on macOS and Windows.
pub fn add_window_material(window: &NativeWindow) {
    let (Some(x), Some(display)) = (x(), linux::primary_display()) else {
        return;
    };
    let Some(geometry) = x
        .conn
        .get_geometry(window.id)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return;
    };
    let (wx, wy, ww, wh) = display.work;
    move_window(
        window,
        (
            wx + (ww - i32::from(geometry.width)) / 2,
            wy + (wh - i32::from(geometry.height)) / 2,
        ),
    );
}

/// An outline in window points: a rounded rectangle, as corner curves
/// flattened to line segments.
fn rounded_rect(width: f64, height: f64, radius: f64) -> Vec<Point> {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    let mut points = Vec::new();
    let corners = [
        (width - radius, radius, -90.0),
        (width - radius, height - radius, 0.0),
        (radius, height - radius, 90.0),
        (radius, radius, 180.0),
    ];
    for (cx, cy, start) in corners {
        for i in 0..=8 {
            let angle = (start + f64::from(i) * 90.0 / 8.0f64).to_radians();
            points.push(Point {
                x: cx + radius * angle.cos(),
                y: cy + radius * angle.sin(),
            });
        }
    }
    points
}

fn outline_points(placement: &CardPlacement) -> Vec<Point> {
    let mut points = Vec::new();
    let mut cursor = Point::default();
    for step in placement.outline() {
        match step {
            PathStep::Move(to) | PathStep::Line(to) => {
                points.push(to);
                cursor = to;
            }
            PathStep::Cubic {
                control_a,
                control_b,
                to,
            } => {
                for i in 1..=16 {
                    let t = f64::from(i) / 16.0;
                    let u = 1.0 - t;
                    let coordinate = |a, b, c, d| {
                        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                    };
                    points.push(Point {
                        x: coordinate(cursor.x, control_a.x, control_b.x, to.x),
                        y: coordinate(cursor.y, control_a.y, control_b.y, to.y),
                    });
                }
                cursor = to;
            }
            PathStep::Close => {}
        }
    }
    points
}

/// Covers a polygon (window points) with one rectangle per run of equal
/// pixel rows, sampling each row at its center like a rasterizer does.
fn polygon_rects(points: &[Point], scale: f64) -> Vec<xproto::Rectangle> {
    let points: Vec<(f64, f64)> = points.iter().map(|p| (p.x * scale, p.y * scale)).collect();
    if points.len() < 3 {
        return Vec::new();
    }
    let bottom = points.iter().map(|p| p.1).fold(0.0, f64::max).ceil() as i32;
    let mut rects: Vec<xproto::Rectangle> = Vec::new();
    let mut previous: Option<Vec<(i16, u16)>> = None;
    for row in 0..bottom.max(0) {
        let y = f64::from(row) + 0.5;
        let mut crossings: Vec<f64> = Vec::new();
        for (index, &(x0, y0)) in points.iter().enumerate() {
            let (x1, y1) = points[(index + 1) % points.len()];
            if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
                crossings.push(x0 + (y - y0) * (x1 - x0) / (y1 - y0));
            }
        }
        crossings.sort_by(f64::total_cmp);
        let spans: Vec<(i16, u16)> = crossings
            .as_chunks::<2>()
            .0
            .iter()
            .filter_map(|&[start, end]| {
                let (start, end) = (start.round(), end.round());
                (end > start).then_some((start as i16, (end - start) as u16))
            })
            .collect();
        // Extend the rectangles of the previous row when the spans match.
        if previous.as_ref() == Some(&spans) {
            let count = spans.len();
            let start = rects.len() - count;
            for rect in &mut rects[start..] {
                rect.height += 1;
            }
        } else {
            rects.extend(spans.iter().map(|&(x, width)| xproto::Rectangle {
                x,
                y: row as i16,
                width,
                height: 1,
            }));
        }
        previous = Some(spans);
    }
    rects
}

fn set_shape(x: &X, window: &NativeWindow, points: &[Point], scale: f64) {
    if !x.shape {
        return;
    }
    let rects = polygon_rects(points, scale);
    // Without a compositor there is no per-pixel alpha, so the window's
    // own shape has to cut the rounded corners and the arrow.
    if !linux::has_compositor() {
        let _ = x.conn.shape_rectangles(
            shape::SO::SET,
            shape::SK::BOUNDING,
            xproto::ClipOrdering::UNSORTED,
            window.id,
            0,
            0,
            &rects,
        );
    }
    *window.state.input.borrow_mut() = Some(rects);
    apply_input(x, window);
}

fn apply_input(x: &X, window: &NativeWindow) {
    if !x.shape {
        return;
    }
    let interactive = window.state.alpha.get() > 0.0 && window.state.mapped.get();
    let input = window.state.input.borrow();
    let rects: &[xproto::Rectangle] = match (&*input, interactive) {
        (Some(rects), true) => rects,
        (None, true) => {
            let _ = x.conn.shape_mask(
                shape::SO::SET,
                shape::SK::INPUT,
                window.id,
                0,
                0,
                x11rb::NONE,
            );
            return;
        }
        (_, false) => &[],
    };
    let _ = x.conn.shape_rectangles(
        shape::SO::SET,
        shape::SK::INPUT,
        xproto::ClipOrdering::UNSORTED,
        window.id,
        0,
        0,
        rects,
    );
}

pub fn shape_card(material: &NativeMaterial, placement: &CardPlacement) {
    let (Some(x), Some(display)) = (x(), linux::primary_display()) else {
        return;
    };
    set_shape(&x, material, &outline_points(placement), display.scale);
}

fn frame(window: &NativeWindow) -> Rect {
    let (Some(x), Some(display)) = (x(), linux::primary_display()) else {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    };
    let Some(geometry) = x
        .conn
        .get_geometry(window.id)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    };
    let Some(origin) = x
        .conn
        .translate_coordinates(window.id, x.root, 0, 0)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    };
    let scale = display.scale;
    let (left, top) = (i32::from(origin.dst_x), i32::from(origin.dst_y));
    let (width, height) = (i32::from(geometry.width), i32::from(geometry.height));
    Rect::new(
        f64::from(left - display.x) / scale,
        f64::from(display.bottom() - (top + height)) / scale,
        f64::from(width) / scale,
        f64::from(height) / scale,
    )
}

fn place(window: &NativeWindow, frame: Rect) {
    let (Some(x), Some(display)) = (x(), linux::primary_display()) else {
        return;
    };
    let scale = display.scale;
    let _ = x.conn.configure_window(
        window.id,
        &xproto::ConfigureWindowAux::new()
            .x(display.x + (frame.x * scale).round() as i32)
            .y(display.bottom() - (frame.max_y() * scale).round() as i32)
            .stack_mode(xproto::StackMode::ABOVE),
    );
}

fn show(window: &NativeWindow) {
    let Some(x) = x() else {
        return;
    };
    if !window.state.mapped.replace(true) {
        let _ = x.conn.map_window(window.id);
    }
    apply_input(&x, window);
}

fn hide(window: &NativeWindow) {
    let Some(x) = x() else {
        return;
    };
    if window.state.mapped.replace(false) {
        let _ = x.conn.unmap_window(window.id);
    }
}

fn alpha(window: &NativeWindow, value: f64) {
    let value = value.clamp(0.0, 1.0);
    let was_visible = window.state.alpha.get() > 0.0;
    window.state.alpha.set(value);
    let Some(x) = x() else {
        return;
    };
    let opacity = (value * f64::from(u32::MAX)).round() as u32;
    if value >= 1.0 {
        let _ = x
            .conn
            .delete_property(window.id, x.atoms._NET_WM_WINDOW_OPACITY);
    } else {
        let _ = x.conn.change_property32(
            xproto::PropMode::REPLACE,
            window.id,
            x.atoms._NET_WM_WINDOW_OPACITY,
            x.atoms.CARDINAL,
            &[opacity],
        );
    }
    if was_visible != (value > 0.0) {
        apply_input(&x, window);
    }
}

fn cancel(window: &NativeWindow) {
    ANIMATIONS.with(|animations| animations.borrow_mut().remove(&window.id));
}

fn animate(window: &NativeWindow, target: Option<Rect>, opacity: f64, motion: Motion) {
    cancel(window);
    let from = frame(window);
    let animation = Animation {
        window: window.clone(),
        start: Instant::now(),
        motion,
        from,
        to: target.unwrap_or(from),
        moves: target.is_some(),
        from_alpha: window.state.alpha.get(),
        to_alpha: opacity,
    };
    ANIMATIONS.with(|animations| animations.borrow_mut().insert(window.id, animation));
    step(window.id);
}

fn step(id: u32) {
    let Some(animation) = ANIMATIONS.with(|all| all.borrow_mut().remove(&id)) else {
        return;
    };
    let t = (animation.start.elapsed().as_secs_f32() / animation.motion.duration.as_secs_f32())
        .min(1.0);
    let eased = f64::from(motion::sample(animation.motion.curve, t));
    let lerp = |a: f64, b: f64| a + (b - a) * eased;
    if animation.moves {
        place(
            &animation.window,
            Rect::new(
                lerp(animation.from.x, animation.to.x),
                lerp(animation.from.y, animation.to.y),
                animation.to.width,
                animation.to.height,
            ),
        );
    }
    // Opacity overshooting a spring curve still ends at the target.
    alpha(
        &animation.window,
        lerp(animation.from_alpha, animation.to_alpha).clamp(0.0, 1.0),
    );
    if t < 1.0 {
        ANIMATIONS.with(|all| all.borrow_mut().insert(id, animation));
    } else if animation.to_alpha == 0.0 {
        dismiss_if_invisible(&animation.window);
    }
}

pub fn slide_dock(window: &NativeWindow, target: Rect, visible: bool, animated: bool) {
    if let Some(x) = x() {
        let scale = linux::primary_display().map_or(1.0, |display| display.scale);
        let points = rounded_rect(target.width, target.height, crate::geometry::DOCK_RADIUS);
        set_shape(&x, window, &points, scale);
    }
    let opacity = if visible { 1.0 } else { 0.0 };
    if visible {
        if !window.state.mapped.get() {
            // Map it where it starts, so it doesn't flash at the origin.
            place(window, if animated { frame(window) } else { target });
        }
        show(window);
    }
    if animated {
        let motion = if visible {
            motion::DOCK_IN
        } else {
            motion::DOCK_OUT
        };
        animate(window, Some(target), opacity, motion);
    } else {
        cancel(window);
        place(window, target);
        alpha(window, opacity);
        if !visible {
            dismiss_if_invisible(window);
        }
    }
    flush();
}

pub fn show_card(window: &NativeWindow, target: Rect, entry: CardEntry) {
    if let Some(previous) = frontmost_app() {
        window.state.previous.set(Some(previous));
    }
    cancel(window);
    match entry {
        CardEntry::Snap => {
            place(window, target);
            alpha(window, 1.0);
            show(window);
        }
        CardEntry::Glide { from } => {
            place(window, from);
            alpha(window, 1.0);
            show(window);
            animate(window, Some(target), 1.0, motion::CARD_MOVE);
        }
        CardEntry::Pop { anchor } => {
            // Keep the arrow anchored while the card settles into position.
            let drift = (1.0 - motion::POPOVER_SCALE) * 12.0;
            let start = Rect::new(
                target.x + (anchor.x - target.width / 2.0).signum() * drift,
                target.y,
                target.width,
                target.height,
            );
            place(window, start);
            alpha(window, 0.0);
            show(window);
            animate(window, Some(target), 1.0, motion::CARD_IN);
        }
    }
    flush();
}

pub fn hide_card(window: &NativeWindow, anchor: Option<Point>) {
    if anchor.is_some() && window.state.mapped.get() {
        animate(window, None, 0.0, motion::CARD_OUT);
    } else {
        cancel(window);
        alpha(window, 0.0);
        dismiss_if_invisible(window);
    }
    flush();
}

/// Moves a managed window's top-left to `origin`, in X root pixels.
fn move_window(window: &NativeWindow, origin: (i32, i32)) {
    let Some(x) = x() else {
        return;
    };
    // Static gravity: the coordinates are the client window's own. Source 2
    // is a tool acting for the user, which window managers honor.
    let flags = 10 | (1 << 8) | (1 << 9) | (2 << 12);
    let message = xproto::ClientMessageEvent::new(
        32,
        window.id,
        x.atoms._NET_MOVERESIZE_WINDOW,
        [flags, origin.0 as u32, origin.1 as u32, 0, 0],
    );
    let _ = x.conn.send_event(
        false,
        x.root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        message,
    );
    flush();
}

/// Gives an unmanaged pop-up window the keyboard, as a menu has while it
/// is open, and starts watching for clicks anywhere else.
pub fn begin_menu(window: &NativeWindow) {
    if let Some(previous) = frontmost_app() {
        window.state.previous.set(Some(previous));
    }
    if let Some(x) = x() {
        let _ = x
            .conn
            .set_input_focus(xproto::InputFocus::PARENT, window.id, x11rb::CURRENT_TIME);
    }
    linux::watch_presses(true);
    flush();
}

/// Whether the pointer was pressed outside `window` since the last call.
pub fn pressed_outside(window: &NativeWindow) -> bool {
    let presses = linux::take_presses();
    if presses.is_empty() {
        return false;
    }
    let Some(x) = x() else {
        return false;
    };
    let Some(geometry) = x
        .conn
        .get_geometry(window.id)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return true;
    };
    let Some(origin) = x
        .conn
        .translate_coordinates(window.id, x.root, 0, 0)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return true;
    };
    let (left, top) = (i32::from(origin.dst_x), i32::from(origin.dst_y));
    let (right, bottom) = (
        left + i32::from(geometry.width),
        top + i32::from(geometry.height),
    );
    presses.iter().any(|&(x, y)| {
        let (x, y) = (i32::from(x), i32::from(y));
        x < left || x >= right || y < top || y >= bottom
    })
}

/// Stops watching for clicks and hands the keyboard back.
pub fn end_menu(window: &NativeWindow) {
    linux::watch_presses(false);
    if let Some(previous) = window.state.previous.take() {
        activate_app(previous);
    }
    flush();
}

pub fn is_key_window(window: &NativeWindow) -> bool {
    x().and_then(|x| x.conn.get_input_focus().ok()?.reply().ok())
        .is_some_and(|focus| focus.focus == window.id)
}

pub fn dismiss_if_invisible(window: &NativeWindow) {
    if window.state.alpha.get() >= 0.01 {
        return;
    }
    let restore = is_key_window(window);
    if window.state.panel.get() {
        hide(window);
    }
    if restore && let Some(previous) = window.state.previous.take() {
        activate_app(previous);
    }
    flush();
}

pub fn fade_in(window: &NativeWindow) {
    // Only a compositor can fade a window; otherwise it simply appears.
    if !linux::has_compositor() {
        return;
    }
    window.state.mapped.set(true);
    alpha(window, 0.0);
    animate(window, None, 1.0, motion::WINDOW_IN);
    flush();
}

fn flush() {
    if let Some(x) = x() {
        let _ = x.conn.flush();
    }
}

pub fn relaunch(exe: &Path) -> io::Result<()> {
    crate::linux::system::relaunch(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_a_rounded_rectangle_into_row_runs() {
        let rects = polygon_rects(&rounded_rect(100.0, 60.0, 10.0), 1.0);
        let area: u32 = rects
            .iter()
            .map(|rect| u32::from(rect.width) * u32::from(rect.height))
            .sum();
        // A full rectangle minus four corners of (1 - π/4)·r².
        let expected = 100.0 * 60.0 - 4.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 100.0;
        assert!((f64::from(area) - expected).abs() < 60.0, "{area}");
        // The straight middle collapses into a single tall rectangle.
        assert!(
            rects
                .iter()
                .any(|rect| rect.width == 100 && rect.height >= 40)
        );
        assert!(rects.len() < 30);
    }

    #[test]
    fn scales_to_physical_pixels() {
        let rects = polygon_rects(&rounded_rect(50.0, 20.0, 0.0), 2.0);
        let rects: Vec<_> = rects
            .iter()
            .map(|rect| (rect.x, rect.y, rect.width, rect.height))
            .collect();
        assert_eq!(rects, [(0, 0, 100, 40)]);
    }
}
