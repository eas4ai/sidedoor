//! What the dock needs from the operating system, behind one seam so the
//! model and views can run against a fake in tests.

use domain::{
    clipboard::ClipKind,
    config::Appearance,
    geometry::{Point, Screen},
};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

pub const REVEAL_LABEL: &str = if cfg!(windows) {
    "Show in File Explorer"
} else if cfg!(target_os = "linux") {
    "Show in Files"
} else {
    "Show in Finder"
};
pub const COMPUTER_NAME: &str = if cfg!(windows) {
    "PC"
} else if cfg!(target_os = "linux") {
    "computer"
} else {
    "Mac"
};

/// An installed app, as shown in the dock.
#[derive(Clone, Debug, PartialEq)]
pub struct AppInfo {
    pub bundle_id: String,
    pub name: String,
    pub path: PathBuf,
    /// A PNG rendering of the app icon, if one could be made.
    pub icon: Option<PathBuf>,
}

/// Something new on the pasteboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Copied {
    pub kind: ClipKind,
    /// Name of the app that was frontmost when it was copied.
    pub source: Option<String>,
}

/// System display preferences the UI honors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
    pub reduce_transparency: bool,
    pub increase_contrast: bool,
    pub reduce_motion: bool,
}

/// Whether the app starts when the user logs in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoginItem {
    On,
    #[default]
    Off,
    /// Registered, but the user must allow it in System Settings.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))] // macOS login-item approval state
    NeedsApproval,
    /// Only an app bundle can be a login item.
    Unavailable,
}

pub trait Platform {
    /// The display with the menu bar.
    fn main_screen(&self) -> Option<Screen>;
    fn pointer(&self) -> Point;
    fn running_bundle_ids(&self) -> HashSet<String>;
    fn accessibility(&self) -> Accessibility;
    /// Seconds east of UTC for local dates.
    fn utc_offset(&self) -> i64;

    fn app_by_bundle_id(&self, bundle_id: &str) -> Option<AppInfo>;
    /// The app bundle at `path`, e.g. one dropped from Finder.
    fn app_at(&self, path: &Path) -> Option<AppInfo>;
    fn open(&self, path: &Path) -> io::Result<()>;
    fn reveal_in_finder(&self, path: &Path);
    /// Moves a file or folder to the Trash (the Recycle Bin on Windows).
    fn trash(&self, path: &Path) -> io::Result<()>;

    /// Increments whenever anything is copied.
    fn pasteboard_change_count(&self) -> isize;
    /// Reads the pasteboard, skipping content marked private or transient.
    /// Copied images are saved into `image_dir`.
    fn read_pasteboard(&self, image_dir: &Path) -> Option<Copied>;
    fn write_pasteboard(&self, kind: &ClipKind);

    /// Shows a notification in Notification Center; `source` names the
    /// plugin that sent it.
    fn notify(&self, source: &str, title: &str, body: &str);

    /// Forces light or dark for every window, or follows the system.
    fn set_appearance(&self, appearance: Appearance);
    fn login_item(&self) -> LoginItem;
    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String>;
}
