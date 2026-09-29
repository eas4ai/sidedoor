//! Linux workspace, clipboard and system data for the shared dock model.

use super::{apps, clipboard, primary_display, system, x};
use crate::{
    api::{Accessibility, AppInfo, Copied, LoginItem, Platform},
    clipboard::ClipKind,
    config::Appearance,
    geometry::{Point, Rect, Screen},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use x11rb::protocol::xproto::ConnectionExt as _;

/// How long a slow system query is reused. The dock polls several times a
/// second; these settings change rarely.
const SCREEN_TTL: Duration = Duration::from_secs(1);
const SETTINGS_TTL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct LinuxPlatform {
    running: RefCell<apps::RunningIndex>,
    screen: Cell<Option<(Instant, Screen)>>,
    accessibility: Cell<Option<(Instant, Accessibility)>>,
}

impl LinuxPlatform {
    fn app_info(&self, id: String, path: PathBuf) -> Option<AppInfo> {
        let info = apps::app_info(id, path)?;
        self.running
            .borrow_mut()
            .remember(&info.bundle_id, &info.path);
        Some(info)
    }
}

/// The class name of the window the user is in, such as "Firefox".
pub fn active_app_name() -> Option<String> {
    let x = x()?;
    let window = super::active_window()?;
    let reply = x
        .conn
        .get_property(
            false,
            window,
            x.atoms.WM_CLASS,
            x11rb::protocol::xproto::AtomEnum::STRING,
            0,
            256,
        )
        .ok()?
        .reply()
        .ok()?;
    // WM_CLASS holds the instance and then the class, NUL-separated.
    let mut parts = reply
        .value
        .split(|&b| b == 0)
        .filter(|part| !part.is_empty());
    let instance = parts.next();
    let class = parts.next().or(instance)?;
    let name = String::from_utf8_lossy(class).into_owned();
    (!name.is_empty()).then_some(name)
}

fn gsetting(schema: &str, key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .trim_matches('\'')
            .to_string()
    })
}

fn read_accessibility() -> Accessibility {
    let increase_contrast = gsetting("org.gnome.desktop.a11y.interface", "high-contrast")
        .is_some_and(|value| value == "true")
        || std::env::var("GTK_THEME").is_ok_and(|theme| theme.contains("HighContrast"));
    let reduce_motion = gsetting("org.gnome.desktop.interface", "enable-animations")
        .is_some_and(|value| value == "false");
    Accessibility {
        // X11 compositors have no per-window blur to reduce, and the panels
        // already draw opaque surfaces; follow contrast as Windows does.
        reduce_transparency: increase_contrast,
        increase_contrast,
        reduce_motion,
    }
}

impl Platform for LinuxPlatform {
    fn main_screen(&self) -> Option<Screen> {
        if let Some((at, screen)) = self.screen.get()
            && at.elapsed() < SCREEN_TTL
        {
            return Some(screen);
        }
        let display = primary_display()?;
        let scale = display.scale;
        let (wx, wy, ww, wh) = display.work;
        let screen = Screen {
            frame: Rect::new(
                0.0,
                0.0,
                f64::from(display.width) / scale,
                f64::from(display.height) / scale,
            ),
            visible: Rect::new(
                f64::from(wx - display.x) / scale,
                f64::from(display.bottom() - (wy + wh)) / scale,
                f64::from(ww) / scale,
                f64::from(wh) / scale,
            ),
        };
        self.screen.set(Some((Instant::now(), screen)));
        Some(screen)
    }

    fn pointer(&self) -> Point {
        let (Some(x), Some(display)) = (x(), primary_display()) else {
            return Point::default();
        };
        let Some(reply) = x
            .conn
            .query_pointer(x.root)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return Point::default();
        };
        Point {
            x: f64::from(i32::from(reply.root_x) - display.x) / display.scale,
            y: f64::from(display.bottom() - i32::from(reply.root_y)) / display.scale,
        }
    }

    fn running_bundle_ids(&self) -> HashSet<String> {
        self.running.borrow().running(&apps::running_programs())
    }

    fn accessibility(&self) -> Accessibility {
        if let Some((at, value)) = self.accessibility.get()
            && at.elapsed() < SETTINGS_TTL
        {
            return value;
        }
        let value = read_accessibility();
        self.accessibility.set(Some((Instant::now(), value)));
        value
    }

    fn utc_offset(&self) -> i64 {
        // SAFETY: localtime_r fills the caller's tm from a valid time_t.
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&now, &mut tm).is_null() {
                return 0;
            }
            tm.tm_gmtoff
        }
    }

    fn app_by_bundle_id(&self, id: &str) -> Option<AppInfo> {
        let path = apps::find_entry(id)?;
        self.app_info(id.to_string(), path)
    }

    fn app_at(&self, path: &Path) -> Option<AppInfo> {
        if path.extension().is_none_or(|ext| ext != "desktop") || !path.is_file() {
            return None;
        }
        let id = apps::entry_id(path)?;
        // Prefer the installed copy, so the saved ID opens the same app.
        let path = apps::find_entry(&id).unwrap_or_else(|| path.to_path_buf());
        self.app_info(id, path)
    }

    fn installed_apps(&self) -> Vec<AppInfo> {
        apps::installed()
    }

    fn open(&self, path: &Path) -> io::Result<()> {
        if path.extension().is_some_and(|ext| ext == "desktop") {
            apps::launch(path)
        } else {
            system::open(path)
        }
    }

    fn reveal_in_finder(&self, path: &Path) {
        system::reveal(path);
    }

    fn trash(&self, path: &Path) -> io::Result<()> {
        system::trash(path)
    }

    fn pasteboard_change_count(&self) -> isize {
        clipboard::change_count()
    }

    fn read_pasteboard(&self, image_dir: &Path) -> Option<Copied> {
        clipboard::read(image_dir)
    }

    fn write_pasteboard(&self, kind: &ClipKind) {
        clipboard::write(kind);
    }

    fn notify(&self, source: &str, title: &str, body: &str) {
        system::notify(source, title, body);
    }

    fn set_appearance(&self, appearance: Appearance) {
        crate::native::set_appearance(appearance);
    }

    fn login_item(&self) -> LoginItem {
        system::login_item()
    }

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        system::set_launch_at_login(enabled)
    }
}
