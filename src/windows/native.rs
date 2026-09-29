//! Native presentation of GPUI windows. Geometry and timing come from the
//! same modules as macOS; this boundary converts to Windows desktop pixels.

use crate::windows::{message_window::wide, services::primary_display};
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
    ptr::null_mut,
    rc::Rc,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, POINT, RECT},
    Graphics::{Dwm::*, Gdi::*},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    UI::{Shell::SetCurrentProcessExplicitAppUserModelID, WindowsAndMessaging::*},
};

pub use crate::windows::{services::WindowsPlatform as NativePlatform, system::open_config};

#[derive(Clone)]
pub struct NativeWindow {
    hwnd: HWND,
    state: Rc<State>,
}

struct State {
    alpha: Cell<f64>,
    previous: Cell<Option<ForegroundApp>>,
}

pub type NativeMaterial = NativeWindow;

#[derive(Clone, Copy)]
pub struct ForegroundApp(isize);

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
    static MATERIAL_AVAILABLE: Cell<bool> = const { Cell::new(false) };
    static APPEARANCE: Cell<Appearance> = const { Cell::new(Appearance::System) };
    static ANIMATIONS: RefCell<HashMap<isize, Animation>> = RefCell::default();
}

struct Animation {
    window: NativeWindow,
    start: Instant,
    motion: Motion,
    from: Rect,
    to: Rect,
    from_alpha: f64,
    to_alpha: f64,
}

const ANIMATION_TIMER: usize = 0x5344;

pub fn set_accessory_policy() {
    // Windows uses tool-window styles instead of an application activation policy.
    // The explicit identity groups taskbar entries and notifications consistently.
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide("app.sidedoor.desktop").as_ptr());
    }
}

pub fn set_appearance(appearance: Appearance) {
    APPEARANCE.with(|value| value.set(appearance));
    // Update native captions and backdrops alongside the shared GPUI palette.
    unsafe {
        EnumWindows(Some(update_appearance), 0);
    }
}

unsafe extern "system" fn update_appearance(hwnd: HWND, _: isize) -> i32 {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if pid == std::process::id() {
        apply_appearance(hwnd);
    }
    1
}

fn apply_appearance(hwnd: HWND) {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let mut light = 1u32;
    let mut size = 4;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
            wide("AppsUseLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut light).cast(),
            &mut size,
        );
    }
    let dark: i32 = APPEARANCE.with(|appearance| match appearance.get() {
        Appearance::System => i32::from(light == 0),
        Appearance::Light => 0,
        Appearance::Dark => 1,
    });
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const dark).cast(),
            4,
        );
    }
}

pub fn appearance(window: &gpui_kit::Window) -> gpui_kit::WindowAppearance {
    APPEARANCE.with(|value| match value.get() {
        Appearance::System => window.appearance(),
        Appearance::Light => gpui_kit::WindowAppearance::Light,
        Appearance::Dark => gpui_kit::WindowAppearance::Dark,
    })
}

pub fn frontmost_app() -> Option<ForegroundApp> {
    // SAFETY: query-only APIs; reject this process so close cannot activate itself.
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        (!hwnd.is_null() && pid != std::process::id()).then_some(ForegroundApp(hwnd as isize))
    }
}

pub fn activate_app(app: ForegroundApp) {
    // SAFETY: validate the stored window because its owner may have closed it.
    unsafe {
        if IsWindow(app.0 as HWND) != 0 {
            SetForegroundWindow(app.0 as HWND);
        }
    }
}

pub fn window_handle(window: &gpui_kit::Window) -> Option<NativeWindow> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(NativeWindow {
        hwnd: handle.hwnd.get() as HWND,
        state: Rc::new(State {
            alpha: Cell::new(1.0),
            previous: Cell::new(None),
        }),
    })
}

pub fn configure_panel(
    window: &NativeWindow,
    corner_radius: f64,
    backdrop: Backdrop,
) -> Option<NativeMaterial> {
    // SAFETY: GPUI owns the live HWND on this thread. Only presentation styles change.
    unsafe {
        let mut style = GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) as u32;
        style = (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
        if matches!(backdrop, Backdrop::Dock) {
            style |= WS_EX_NOACTIVATE;
        }
        SetWindowLongPtrW(window.hwnd, GWL_EXSTYLE, style as isize);
        SetWindowPos(
            window.hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED,
        );
        let mut bounds = RECT::default();
        GetClientRect(window.hwnd, &mut bounds);
        let scale = primary_display().map_or(1.0, |(_, scale)| scale);
        let radius = (corner_radius * scale * 2.0).round() as i32;
        let region = CreateRoundRectRgn(0, 0, bounds.right + 1, bounds.bottom + 1, radius, radius);
        if SetWindowRgn(window.hwnd, region, 1) == 0 {
            DeleteObject(region);
        }
    }
    set_material_hidden(window, false);
    Some(window.clone())
}

pub fn set_material_hidden(window: &NativeMaterial, hidden: bool) {
    apply_appearance(window.hwnd);
    // Windows 11 transient backdrop follows the same role as the macOS popover material.
    // The renderer supplies an opaque fallback on systems where DWM rejects it.
    let backdrop = if hidden { 1u32 } else { 3u32 };
    let result = unsafe {
        DwmSetWindowAttribute(
            window.hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&raw const backdrop).cast(),
            4,
        )
    };
    MATERIAL_AVAILABLE.with(|available| available.set(result >= 0));
}

pub fn has_material() -> bool {
    MATERIAL_AVAILABLE.with(Cell::get)
}

pub fn add_window_material(window: &NativeWindow) {
    set_material_hidden(window, false);
    unsafe {
        let style = GetWindowLongPtrW(window.hwnd, GWL_STYLE);
        SetWindowLongPtrW(window.hwnd, GWL_STYLE, style | WS_CAPTION as isize);
        SendMessageW(
            window.hwnd,
            WM_SETICON,
            ICON_SMALL as usize,
            crate::windows::windows_icon() as isize,
        );
        SendMessageW(
            window.hwnd,
            WM_SETICON,
            ICON_BIG as usize,
            crate::windows::windows_icon() as isize,
        );
        SetWindowPos(
            window.hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

pub fn shape_card(material: &NativeMaterial, placement: &CardPlacement) {
    let scale = primary_display().map_or(1.0, |(_, scale)| scale);
    let mut points = Vec::new();
    let mut cursor = Point::default();
    let mut push = |point: Point| {
        points.push(POINT {
            x: (point.x * scale).round() as i32,
            y: (point.y * scale).round() as i32,
        })
    };
    for step in placement.outline() {
        match step {
            PathStep::Move(to) | PathStep::Line(to) => {
                push(to);
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
                    push(Point {
                        x: coordinate(cursor.x, control_a.x, control_b.x, to.x),
                        y: coordinate(cursor.y, control_a.y, control_b.y, to.y),
                    });
                }
                cursor = to;
            }
            PathStep::Close => {}
        }
    }
    // SAFETY: the point array lives through creation. On success Windows owns the region.
    unsafe {
        let region = CreatePolygonRgn(points.as_ptr(), points.len() as i32, WINDING);
        if SetWindowRgn(material.hwnd, region, 1) == 0 {
            DeleteObject(region);
        }
    }
}

fn frame(window: &NativeWindow) -> Rect {
    let Some((display, scale)) = primary_display() else {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    };
    let mut bounds = RECT::default();
    unsafe {
        GetWindowRect(window.hwnd, &mut bounds);
    }
    Rect::new(
        f64::from(bounds.left - display.rcMonitor.left) / scale,
        f64::from(display.rcMonitor.bottom - bounds.bottom) / scale,
        f64::from(bounds.right - bounds.left) / scale,
        f64::from(bounds.bottom - bounds.top) / scale,
    )
}

fn place(window: &NativeWindow, frame: Rect) {
    let Some((display, scale)) = primary_display() else {
        return;
    };
    unsafe {
        SetWindowPos(
            window.hwnd,
            null_mut(),
            display.rcMonitor.left + (frame.x * scale).round() as i32,
            display.rcMonitor.bottom - (frame.max_y() * scale).round() as i32,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn alpha(window: &NativeWindow, value: f64) {
    window.state.alpha.set(value);
    unsafe {
        let style = GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(window.hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED as isize);
        SetLayeredWindowAttributes(
            window.hwnd,
            0,
            (value.clamp(0.0, 1.0) * 255.0).round() as u8,
            LWA_ALPHA,
        );
    }
}

fn interactive(window: &NativeWindow, enabled: bool) {
    unsafe {
        let style = GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) as u32;
        let style = if enabled {
            style & !WS_EX_TRANSPARENT
        } else {
            style | WS_EX_TRANSPARENT
        };
        SetWindowLongPtrW(window.hwnd, GWL_EXSTYLE, style as isize);
    }
}

fn cancel(window: &NativeWindow) {
    unsafe {
        KillTimer(window.hwnd, ANIMATION_TIMER);
    }
    ANIMATIONS.with(|animations| animations.borrow_mut().remove(&(window.hwnd as isize)));
}

fn animate(window: &NativeWindow, target: Rect, opacity: f64, motion: Motion) {
    cancel(window);
    let animation = Animation {
        window: window.clone(),
        start: Instant::now(),
        motion,
        from: frame(window),
        to: target,
        from_alpha: window.state.alpha.get(),
        to_alpha: opacity,
    };
    ANIMATIONS.with(|animations| {
        animations
            .borrow_mut()
            .insert(window.hwnd as isize, animation)
    });
    // SAFETY: the timer dispatches on this UI thread; it owns no borrowed pointers.
    if unsafe { SetTimer(window.hwnd, ANIMATION_TIMER, 16, Some(tick)) } == 0 {
        cancel(window);
        place(window, target);
        alpha(window, opacity);
    }
}

unsafe extern "system" fn tick(hwnd: HWND, _: u32, _: usize, _: u32) {
    // Remove before calling Windows, which may synchronously dispatch other events.
    let Some(animation) = ANIMATIONS.with(|all| all.borrow_mut().remove(&(hwnd as isize))) else {
        return;
    };
    if unsafe { IsWindow(hwnd) } == 0 {
        return;
    }
    let t = (animation.start.elapsed().as_secs_f32() / animation.motion.duration.as_secs_f32())
        .min(1.0);
    let eased = f64::from(motion::sample(animation.motion.curve, t));
    let lerp = |a, b| a + (b - a) * eased;
    let current = Rect::new(
        lerp(animation.from.x, animation.to.x),
        lerp(animation.from.y, animation.to.y),
        animation.to.width,
        animation.to.height,
    );
    place(&animation.window, current);
    alpha(
        &animation.window,
        lerp(animation.from_alpha, animation.to_alpha),
    );
    if t < 1.0 {
        ANIMATIONS.with(|all| all.borrow_mut().insert(hwnd as isize, animation));
    } else {
        unsafe {
            KillTimer(hwnd, ANIMATION_TIMER);
        }
        if animation.to_alpha == 0.0 {
            dismiss_if_invisible(&animation.window);
        }
    }
}

pub fn slide_dock(window: &NativeWindow, target: Rect, visible: bool, animated: bool) {
    if let Some((_, scale)) = primary_display() {
        let radius = (crate::geometry::DOCK_RADIUS * scale * 2.0).round() as i32;
        unsafe {
            let region = CreateRoundRectRgn(
                0,
                0,
                (target.width * scale).round() as i32 + 1,
                (target.height * scale).round() as i32 + 1,
                radius,
                radius,
            );
            if SetWindowRgn(window.hwnd, region, 1) == 0 {
                DeleteObject(region);
            }
        }
    }
    interactive(window, visible);
    if visible {
        unsafe {
            ShowWindow(window.hwnd, SW_SHOWNOACTIVATE);
        }
    }
    let opacity = if visible { 1.0 } else { 0.0 };
    if animated {
        animate(
            window,
            target,
            opacity,
            if visible {
                motion::DOCK_IN
            } else {
                motion::DOCK_OUT
            },
        );
    } else {
        cancel(window);
        place(window, target);
        alpha(window, opacity);
        if !visible {
            dismiss_if_invisible(window);
        }
    }
}

pub fn show_card(window: &NativeWindow, target: Rect, entry: CardEntry) {
    if let Some(previous) = frontmost_app() {
        window.state.previous.set(Some(previous));
    }
    cancel(window);
    interactive(window, true);
    match entry {
        CardEntry::Snap => {
            place(window, target);
            alpha(window, 1.0);
        }
        CardEntry::Glide { from } => {
            place(window, from);
            alpha(window, 1.0);
            animate(window, target, 1.0, motion::CARD_MOVE);
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
            animate(window, target, 1.0, motion::CARD_IN);
        }
    }
    unsafe {
        ShowWindow(window.hwnd, SW_SHOWNOACTIVATE);
    }
}

pub fn hide_card(window: &NativeWindow, anchor: Option<Point>) {
    interactive(window, false);
    if anchor.is_some() {
        animate(window, frame(window), 0.0, motion::CARD_OUT);
    } else {
        cancel(window);
        alpha(window, 0.0);
        dismiss_if_invisible(window);
    }
}

pub fn is_key_window(window: &NativeWindow) -> bool {
    unsafe { GetForegroundWindow() == window.hwnd }
}

pub fn dismiss_if_invisible(window: &NativeWindow) {
    if window.state.alpha.get() >= 0.01 {
        return;
    }
    let restore = is_key_window(window);
    unsafe {
        ShowWindow(window.hwnd, SW_HIDE);
    }
    if restore && let Some(previous) = window.state.previous.take() {
        activate_app(previous);
    }
}

pub fn fade_in(window: &NativeWindow) {
    alpha(window, 0.0);
    animate(window, frame(window), 1.0, motion::WINDOW_IN);
}

pub fn relaunch(exe: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(exe)
        .args(["--wait-for-process", &std::process::id().to_string()])
        .creation_flags(0x08000000)
        .spawn()
        .map(|_| ())
}

pub fn wait_for_previous_process() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--wait-for-process") {
        return;
    }
    let Some(pid) = args.get(2).and_then(|value| value.parse().ok()) else {
        return;
    };
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !process.is_null() {
            WaitForSingleObject(process, 10000);
            CloseHandle(process);
        }
    }
}
