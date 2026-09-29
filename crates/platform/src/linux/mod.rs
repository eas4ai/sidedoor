//! Linux integrations for X11 desktops (including XWayland sessions).
//!
//! The dock needs a global pointer position, absolute window placement and
//! global shortcuts. Wayland withholds all three from ordinary clients, so
//! Sidedoor asks GPUI for its X11 backend and talks to the X server through
//! one connection of its own on the UI thread. Shared geometry stays in
//! logical, bottom-left coordinates; only this boundary converts to the X
//! server's physical, top-left pixels.

pub mod apps;
pub mod clipboard;
pub mod services;
pub mod system;

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::OnceLock,
};
use x11rb::{
    connection::{Connection as _, RequestConnection as _},
    protocol::{
        Event,
        randr::ConnectionExt as _,
        xproto::{self, ConnectionExt as _},
    },
    rust_connection::RustConnection,
};

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        CLIPBOARD,
        UTF8_STRING,
        CARDINAL,
        WINDOW,
        RESOURCE_MANAGER,
        _NET_ACTIVE_WINDOW,
        _NET_WORKAREA,
        _NET_WM_PID,
        _NET_WM_NAME,
        _NET_WM_WINDOW_OPACITY,
        _NET_CLIENT_LIST,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_CM_S0,
        WM_CLASS,
        TARGETS,
        SIDEDOOR_SELECTION,
        _NET_MOVERESIZE_WINDOW,
    }
}

/// Sidedoor's own X server connection. GPUI keeps its connection private,
/// so window presentation, pointer queries and key grabs use this one.
pub struct X {
    pub conn: RustConnection,
    pub root: u32,
    pub atoms: Atoms,
    pub shape: bool,
    pub xfixes: bool,
    /// An unmapped window that receives selection conversions.
    pub helper: u32,
}

thread_local! {
    static CONNECTION: std::cell::OnceCell<Option<Rc<X>>> = const { std::cell::OnceCell::new() };
    static SCALE: Cell<Option<f64>> = const { Cell::new(None) };
    /// Grabbed keys pressed since the last pump: key code and modifier state.
    static PRESSED: RefCell<VecDeque<(u8, u16)>> = const { RefCell::new(VecDeque::new()) };
    /// Selection replies waiting for the code that asked for them.
    static SELECTIONS: RefCell<VecDeque<xproto::SelectionNotifyEvent>> =
        const { RefCell::new(VecDeque::new()) };
    /// Counts clipboard ownership changes, like `NSPasteboard.changeCount`.
    static CLIPBOARD_CHANGES: Cell<isize> = const { Cell::new(0) };
    /// Where the pointer was at each button press anywhere on screen,
    /// while something watches for them (an open menu).
    static PRESSES: RefCell<Option<Vec<(i16, i16)>>> = const { RefCell::new(None) };
}

/// The shared connection, opened on first use.
pub fn x() -> Option<Rc<X>> {
    // `try_with`: destructors running at exit may still ask for it.
    CONNECTION
        .try_with(|cell| {
            cell.get_or_init(|| match connect() {
                Ok(x) => Some(Rc::new(x)),
                Err(error) => {
                    eprintln!("sidedoor: couldn't connect to the X server: {error}");
                    None
                }
            })
            .clone()
        })
        .ok()
        .flatten()
}

fn connect() -> Result<X, Box<dyn std::error::Error>> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let atoms = Atoms::new(&conn)?.reply()?;
    let has = |name: &'static str| conn.extension_information(name).ok().flatten().is_some();
    let shape = has(x11rb::protocol::shape::X11_EXTENSION_NAME);
    let xfixes = has(x11rb::protocol::xfixes::X11_EXTENSION_NAME);
    if xfixes {
        use x11rb::protocol::xfixes::ConnectionExt as _;
        conn.xfixes_query_version(5, 0)?.reply()?;
    }
    let helper = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        helper,
        root,
        -10,
        -10,
        1,
        1,
        0,
        xproto::WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &xproto::CreateWindowAux::new(),
    )?;
    if xfixes {
        use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
        conn.xfixes_select_selection_input(
            helper,
            atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )?;
    }
    conn.flush()?;
    Ok(X {
        conn,
        root,
        atoms,
        shape,
        xfixes,
        helper,
    })
}

/// Reads every event the X server has queued for our connection without
/// waiting. Key presses and selection replies are queued for their owners;
/// nothing is dispatched from here, so callers may hold any borrow.
pub fn drain_events() {
    let Some(x) = x() else {
        return;
    };
    while let Ok(Some(event)) = x.conn.poll_for_event() {
        match event {
            Event::KeyPress(press) => {
                PRESSED.with(|queue| {
                    queue
                        .borrow_mut()
                        .push_back((press.detail, u16::from(press.state)))
                });
            }
            Event::XfixesSelectionNotify(_) => {
                CLIPBOARD_CHANGES.with(|count| count.set(count.get() + 1));
            }
            Event::SelectionNotify(reply) => {
                SELECTIONS.with(|queue| queue.borrow_mut().push_back(reply));
            }
            Event::XinputRawButtonPress(_) => {
                let at = x
                    .conn
                    .query_pointer(x.root)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .map(|reply| (reply.root_x, reply.root_y));
                PRESSES.with(|presses| {
                    if let (Some(presses), Some(at)) = (&mut *presses.borrow_mut(), at) {
                        presses.push(at);
                    }
                });
            }
            _ => {}
        }
    }
}

/// Starts or stops recording button presses anywhere on screen. XInput 2
/// raw events reach every client that asks, without grabbing the pointer
/// from the app that has it.
pub fn watch_presses(watch: bool) {
    let Some(x) = x() else {
        return;
    };
    use x11rb::protocol::xinput::{self, ConnectionExt as _};
    if watch
        && x.conn
            .xinput_xi_query_version(2, 0)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_none()
    {
        return;
    }
    let mask = if watch {
        xinput::XIEventMask::RAW_BUTTON_PRESS
    } else {
        xinput::XIEventMask::from(0u32)
    };
    let _ = x.conn.xinput_xi_select_events(
        x.root,
        &[xinput::EventMask {
            deviceid: xinput::Device::ALL_MASTER.into(),
            mask: vec![mask],
        }],
    );
    let _ = x.conn.flush();
    PRESSES.with(|presses| *presses.borrow_mut() = watch.then(Vec::new));
}

/// Button presses since the last call, as root-window pixel positions.
pub fn take_presses() -> Vec<(i16, i16)> {
    drain_events();
    PRESSES.with(|presses| {
        presses
            .borrow_mut()
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    })
}

pub fn take_key_presses() -> Vec<(u8, u16)> {
    drain_events();
    PRESSED.with(|queue| queue.borrow_mut().drain(..).collect())
}

pub fn clipboard_changes() -> isize {
    drain_events();
    CLIPBOARD_CHANGES.with(Cell::get)
}

/// Waits briefly for the reply to a selection conversion we requested.
pub fn wait_for_selection(
    x: &X,
    property: u32,
    timeout: std::time::Duration,
) -> Option<xproto::SelectionNotifyEvent> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        drain_events();
        let found = SELECTIONS.with(|queue| {
            let mut queue = queue.borrow_mut();
            let index = queue
                .iter()
                .position(|reply| reply.requestor == x.helper && reply.property == property);
            index.and_then(|index| queue.remove(index))
        });
        if found.is_some() {
            return found;
        }
        let failed = SELECTIONS.with(|queue| {
            let mut queue = queue.borrow_mut();
            let index = queue
                .iter()
                .position(|reply| reply.requestor == x.helper && reply.property == x11rb::NONE);
            index.and_then(|index| queue.remove(index)).is_some()
        });
        if failed || std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

static WAYLAND_DISPLAY: OnceLock<Option<std::ffi::OsString>> = OnceLock::new();

/// Chooses GPUI's X11 backend before it starts. On a Wayland session this
/// runs the app through XWayland, where the dock can follow the pointer.
pub fn prefer_x11() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY");
    if std::env::var_os("DISPLAY").is_some() && wayland.is_some() {
        // SAFETY: called at startup, before any other thread exists.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
    }
    WAYLAND_DISPLAY.get_or_init(|| wayland);
}

/// The Wayland session the app was started in, for the apps it launches.
pub fn original_wayland_display() -> Option<std::ffi::OsString> {
    WAYLAND_DISPLAY.get().cloned().flatten()
}

/// The primary monitor in physical pixels, with its work area (the part not
/// covered by panels) and the logical-to-physical scale.
#[derive(Clone, Copy, Debug)]
pub struct Display {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub work: (i32, i32, i32, i32),
    pub scale: f64,
}

impl Display {
    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }
}

/// Records GPUI's scale so native placement matches GPUI's layout.
pub fn set_scale(scale: f64) {
    if scale.is_finite() && scale > 0.0 {
        SCALE.with(|value| value.set(Some(scale)));
    }
}

fn scale(x: &X) -> f64 {
    if let Some(scale) = SCALE.with(Cell::get) {
        return scale;
    }
    // GPUI's X11 client uses the same sources before a window exists.
    if let Some(scale) = std::env::var("GPUI_X11_SCALE_FACTOR")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|scale| scale.is_normal() && *scale > 0.0)
    {
        return scale;
    }
    x.conn
        .get_property(
            false,
            x.root,
            x.atoms.RESOURCE_MANAGER,
            x11rb::protocol::xproto::AtomEnum::STRING,
            0,
            u32::MAX / 4,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| {
            String::from_utf8_lossy(&reply.value)
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Xft.dpi:")
                        .map(str::trim)
                        .map(String::from)
                })
        })
        .and_then(|dpi| dpi.parse::<f64>().ok())
        .map_or(1.0, |dpi| (dpi / 96.0).max(1.0))
}

pub fn primary_display() -> Option<Display> {
    let x = x()?;
    let scale = scale(&x);
    let (px, py, width, height) = x
        .conn
        .randr_get_monitors(x.root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| {
            let monitors = reply.monitors;
            monitors
                .iter()
                .find(|monitor| monitor.primary)
                .or(monitors.first())
                .map(|m| {
                    (
                        i32::from(m.x),
                        i32::from(m.y),
                        i32::from(m.width),
                        i32::from(m.height),
                    )
                })
        })
        .or_else(|| {
            let geometry = x.conn.get_geometry(x.root).ok()?.reply().ok()?;
            Some((0, 0, i32::from(geometry.width), i32::from(geometry.height)))
        })?;
    let work = x
        .conn
        .get_property(false, x.root, x.atoms._NET_WORKAREA, x.atoms.CARDINAL, 0, 4)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| {
            let values: Vec<i32> = reply.value32()?.map(|value| value as i32).collect();
            (values.len() == 4).then(|| (values[0], values[1], values[2], values[3]))
        })
        .map(|(wx, wy, ww, wh)| {
            // The work area spans every monitor; keep the part on this one.
            let left = wx.max(px);
            let top = wy.max(py);
            let right = (wx + ww).min(px + width);
            let bottom = (wy + wh).min(py + height);
            if right > left && bottom > top {
                (left, top, right - left, bottom - top)
            } else {
                (px, py, width, height)
            }
        })
        .unwrap_or((px, py, width, height));
    Some(Display {
        x: px,
        y: py,
        width,
        height,
        work,
        scale,
    })
}

/// Whether a compositing manager draws windows, so per-pixel alpha and
/// window opacity take effect.
pub fn has_compositor() -> bool {
    let Some(x) = x() else {
        return false;
    };
    x.conn
        .get_selection_owner(x.atoms._NET_WM_CM_S0)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.owner != x11rb::NONE)
}

/// The process that owns an X window, when its client says so.
pub fn window_pid(window: u32) -> Option<u32> {
    let x = x()?;
    let reply = x
        .conn
        .get_property(false, window, x.atoms._NET_WM_PID, x.atoms.CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next()
}

/// The window the window manager considers active.
pub fn active_window() -> Option<u32> {
    let x = x()?;
    let reply = x
        .conn
        .get_property(
            false,
            x.root,
            x.atoms._NET_ACTIVE_WINDOW,
            x.atoms.WINDOW,
            0,
            1,
        )
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next().filter(|&window| window != 0)
}
