//! Shell operations and per-user login registration.

use super::message_window::wide;
use crate::LoginItem;
use std::{
    io,
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    },
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

pub fn open(path: &Path) -> io::Result<()> {
    let path = wide(path);
    // SAFETY: all strings remain live through this synchronous shell call.
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            null(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Windows could not open the item (shell error {result})"
        )))
    }
}

pub fn open_config(path: &Path) -> io::Result<()> {
    open(path).or_else(|_| {
        std::process::Command::new("notepad.exe")
            .arg(path)
            .spawn()
            .map(|_| ())
    })
}

pub fn login_item() -> LoginItem {
    let key = wide(RUN);
    let name = wide("Sidedoor");
    let mut command = [0u16; 32768];
    let mut size = std::mem::size_of_val(&command) as u32;
    // SAFETY: command is a writable buffer of the specified size.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            command.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return LoginItem::Off;
    }
    if result != ERROR_SUCCESS {
        return LoginItem::Unavailable;
    }
    let length = command
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(command.len());
    let command = String::from_utf16_lossy(&command[..length]);
    let expected = std::env::current_exe()
        .ok()
        .map(|exe| format!("\"{}\"", exe.display()));
    if expected
        .as_deref()
        .is_some_and(|expected| command.eq_ignore_ascii_case(expected))
    {
        LoginItem::On
    } else {
        // A portable installation moved; enabling again repairs its path.
        LoginItem::Off
    }
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let key = wide(RUN);
    let name = wide("Sidedoor");
    let result = if enabled {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let command = wide(format!("\"{}\"", exe.display()));
        // SAFETY: the UTF-16 command is null terminated and its byte size includes the terminator.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        }
    } else {
        // SAFETY: deletes only this application's named value under the current user.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) }
    };
    if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32).to_string())
    }
}
