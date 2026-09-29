//! Keep one dock per desktop session, including simultaneous double-clicks.

use super::message_window::wide;
use std::{
    io,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, FindWindowExW, GetWindowThreadProcessId, PostMessageW, WM_APP,
    },
};

pub const OPEN_SETTINGS: u32 = WM_APP + 2;

pub struct Instance(HANDLE);

impl Instance {
    pub fn acquire() -> io::Result<Option<Self>> {
        // A named kernel handle makes the check atomic across processes. It is
        // released by Windows on crash too, so no stale lockfile can block launch.
        let handle = unsafe { CreateMutexW(null(), 0, wide("Local\\Sidedoor.Desktop").as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            let class = wide("SidedoorMessages");
            let mut previous = null_mut();
            loop {
                let window = unsafe { FindWindowExW(null_mut(), previous, class.as_ptr(), null()) };
                if window.is_null() {
                    break;
                }
                unsafe {
                    let mut pid = 0;
                    GetWindowThreadProcessId(window, &mut pid);
                    AllowSetForegroundWindow(pid);
                    PostMessageW(window, OPEN_SETTINGS, 0, 0);
                }
                previous = window;
            }
            Ok(None)
        } else {
            Ok(Some(Self(handle)))
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: this guard owns exactly one kernel reference.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
