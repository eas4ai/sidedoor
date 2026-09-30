//! Operating-system integration and native window presentation.
use domain::{config, geometry, motion, shortcut};
pub mod api;
pub use api::*;
pub mod paths;
pub use paths::*;
#[cfg_attr(target_os = "windows", path = "windows/hotkeys.rs")]
#[cfg_attr(target_os = "macos", path = "macos/hotkeys.rs")]
#[cfg_attr(target_os = "linux", path = "linux/hotkeys.rs")]
pub mod hotkeys;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg_attr(target_os = "windows", path = "windows/native.rs")]
#[cfg_attr(target_os = "macos", path = "macos/native.rs")]
#[cfg_attr(target_os = "linux", path = "linux/native.rs")]
pub mod native;
#[cfg(target_os = "macos")]
#[path = "macos/notifications.rs"]
mod notifications;
pub mod proxy;
#[cfg_attr(target_os = "windows", path = "windows/status_menu.rs")]
#[cfg_attr(target_os = "macos", path = "macos/status_menu.rs")]
#[cfg_attr(target_os = "linux", path = "linux/status_menu.rs")]
pub mod status_menu;
#[cfg(target_os = "windows")]
pub mod windows;
