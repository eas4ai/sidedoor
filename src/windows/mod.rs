//! Windows integrations. All native handles and callbacks stay on the UI thread.

mod icons;
pub mod message_window;
pub mod services;
mod shortcuts;
pub mod system;

#[cfg(test)]
mod tests;

/// Shared application icon embedded in the executable (resource 1).
/// LoadIcon returns a shared handle whose lifetime is the loaded module.
pub fn windows_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use windows_sys::Win32::{
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{IDI_APPLICATION, LoadIconW},
    };
    unsafe {
        let icon = LoadIconW(
            GetModuleHandleW(std::ptr::null()),
            std::ptr::without_provenance::<u16>(1),
        );
        if icon.is_null() {
            LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
        } else {
            icon
        }
    }
}
