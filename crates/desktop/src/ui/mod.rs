//! Shared desktop UI, used on macOS, Windows and Linux.
pub(crate) mod chrome;
pub(crate) mod clipboard;
pub(crate) mod dock;
pub(crate) mod menu;
pub(crate) mod plugins;
pub(crate) mod remote_image;
pub(crate) mod settings;
pub(crate) mod shortcut_recorder;
pub(crate) mod slider;
pub(crate) mod switch;
#[cfg(test)]
mod tests;
pub(crate) mod theme;
