//! Sidedoor desktop entry point.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod app;
mod builtins;
mod ui;
fn main() {
    app::run();
}
