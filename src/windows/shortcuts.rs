//! Resolve a shell shortcut only for process matching. Launching still opens
//! the .lnk itself, preserving its arguments, working directory and elevation.

use super::message_window::wide;
use std::{
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ},
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{Interface, PCWSTR},
};

pub fn target(path: &Path) -> Option<PathBuf> {
    if !path.extension()?.eq_ignore_ascii_case("lnk") {
        return Some(path.to_path_buf());
    }
    // GPUI initializes the UI thread's OLE apartment before any platform calls.
    // COM interface owners release their references automatically on this thread.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(PCWSTR(wide(path).as_ptr()), STGM_READ).ok()?;
        let mut buffer = [0u16; 32768];
        link.GetPath(&mut buffer, std::ptr::null_mut(), 0).ok()?;
        let length = buffer.iter().position(|&unit| unit == 0)?;
        (length > 0).then(|| std::ffi::OsString::from_wide(&buffer[..length]).into())
    }
}
