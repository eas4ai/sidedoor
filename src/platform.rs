//! What the dock needs from the operating system, behind one seam so the
//! model and views can run against a fake in tests.

use crate::{
    clipboard::{ClipKind, History},
    config::{Appearance, Config},
    geometry::{Point, Screen},
};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
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

    /// Increments whenever anything is copied.
    fn pasteboard_change_count(&self) -> isize;
    /// Reads the pasteboard, skipping content marked private or transient.
    /// Copied images are saved into `image_dir`.
    fn read_pasteboard(&self, image_dir: &Path) -> Option<Copied>;
    fn write_pasteboard(&self, kind: &ClipKind);

    fn save_config(&self, config: &Config) -> io::Result<()>;
    fn save_history(&self, history: &History) -> io::Result<()>;

    /// Forces light or dark for every window, or follows the system.
    fn set_appearance(&self, appearance: Appearance);
    fn login_item(&self) -> LoginItem;
    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String>;
}

/// Where the clone keeps its files.
pub fn support_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Library/Application Support/SidekickClone")
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Library/Caches/SidekickClone")
}

#[cfg(test)]
pub mod fake {
    //! An in-memory platform for tests.

    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct FakePlatform {
        pub screen: RefCell<Option<Screen>>,
        pub pointer: RefCell<Point>,
        pub running: RefCell<HashSet<String>>,
        pub apps: RefCell<Vec<AppInfo>>,
        pub opened: RefCell<Vec<PathBuf>>,
        pub pasteboard: RefCell<(isize, Option<Copied>)>,
        pub written: RefCell<Vec<ClipKind>>,
        pub saved_configs: RefCell<Vec<Config>>,
        pub appearance: RefCell<Option<Appearance>>,
        pub login: RefCell<LoginItem>,
    }

    impl FakePlatform {
        pub fn with_apps(apps: &[(&str, &str)]) -> Self {
            let fake = Self::default();
            *fake.screen.borrow_mut() = Some(Screen {
                frame: crate::geometry::Rect::new(0.0, 0.0, 1512.0, 982.0),
                visible: crate::geometry::Rect::new(0.0, 0.0, 1512.0, 949.0),
            });
            *fake.apps.borrow_mut() = apps
                .iter()
                .map(|(id, name)| AppInfo {
                    bundle_id: (*id).into(),
                    name: (*name).into(),
                    path: PathBuf::from(format!("/Applications/{name}.app")),
                    icon: None,
                })
                .collect();
            fake
        }

        pub fn copy(&self, kind: ClipKind) {
            let mut pasteboard = self.pasteboard.borrow_mut();
            pasteboard.0 += 1;
            pasteboard.1 = Some(Copied { kind, source: None });
        }
    }

    impl Platform for FakePlatform {
        fn main_screen(&self) -> Option<Screen> {
            *self.screen.borrow()
        }
        fn pointer(&self) -> Point {
            *self.pointer.borrow()
        }
        fn running_bundle_ids(&self) -> HashSet<String> {
            self.running.borrow().clone()
        }
        fn accessibility(&self) -> Accessibility {
            Accessibility::default()
        }
        fn utc_offset(&self) -> i64 {
            0
        }
        fn app_by_bundle_id(&self, bundle_id: &str) -> Option<AppInfo> {
            self.apps
                .borrow()
                .iter()
                .find(|app| app.bundle_id == bundle_id)
                .cloned()
        }
        fn app_at(&self, path: &Path) -> Option<AppInfo> {
            self.apps
                .borrow()
                .iter()
                .find(|app| app.path == path)
                .cloned()
        }
        fn open(&self, path: &Path) -> io::Result<()> {
            self.opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
        fn reveal_in_finder(&self, _: &Path) {}
        fn pasteboard_change_count(&self) -> isize {
            self.pasteboard.borrow().0
        }
        fn read_pasteboard(&self, _: &Path) -> Option<Copied> {
            self.pasteboard.borrow().1.clone()
        }
        fn write_pasteboard(&self, kind: &ClipKind) {
            self.written.borrow_mut().push(kind.clone());
        }
        fn save_config(&self, config: &Config) -> io::Result<()> {
            self.saved_configs.borrow_mut().push(config.clone());
            Ok(())
        }
        fn save_history(&self, _: &History) -> io::Result<()> {
            Ok(())
        }
        fn set_appearance(&self, appearance: Appearance) {
            *self.appearance.borrow_mut() = Some(appearance);
        }
        fn login_item(&self) -> LoginItem {
            *self.login.borrow()
        }
        fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
            *self.login.borrow_mut() = if enabled {
                LoginItem::On
            } else {
                LoginItem::Off
            };
            Ok(())
        }
    }
}
