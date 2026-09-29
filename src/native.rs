//! Native window operations. The application shell owns GPUI handles; this
//! module keeps operating-system types and focus restoration out of it.

pub use crate::macos::{
    Backdrop, CardEntry, MacPlatform as NativePlatform, activate_app, add_window_material,
    configure_panel, fade_in, frontmost_app, hide_card, ns_window as window_handle,
    set_accessory_policy, set_appearance, shape_card, show_card, slide_dock,
};

pub type NativeWindow = objc2::rc::Retained<objc2_app_kit::NSWindow>;
pub type NativeMaterial = objc2::rc::Retained<objc2_app_kit::NSView>;
pub type ForegroundApp = i32;

pub fn appearance(window: &gpui_kit::Window) -> gpui_kit::WindowAppearance {
    window.appearance()
}

pub fn set_material_hidden(material: &NativeMaterial, hidden: bool) {
    material.setHidden(hidden);
}

pub fn is_key_window(window: &NativeWindow) -> bool {
    window.isKeyWindow()
}

pub fn dismiss_if_invisible(window: &NativeWindow) {
    if window.alphaValue() < 0.01 {
        window.orderOut(None);
    }
}

pub fn open_config(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("/usr/bin/open")
        .arg("-t")
        .arg(path)
        .spawn()
        .map(|_| ())
}

pub fn relaunch(exe: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.6; exec \"$0\"")
        .arg(exe)
        .spawn()
        .map(|_| ())
}

pub fn has_material() -> bool {
    true
}
