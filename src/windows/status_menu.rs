//! Windows notification-area icon, menu, and plugin notification balloons.

use crate::{
    platform::LoginItem,
    windows::{
        message_window::{MessageWindow, wide},
        system,
    },
};
use std::{cell::Cell, ptr::null_mut, rc::Rc};
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    UI::{Shell::*, WindowsAndMessaging::*},
};

const ICON_ID: u32 = 1;
const TRAY_MESSAGE: u32 = WM_APP + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    OpenSettings,
    Reload,
    Quit,
}

thread_local! { static TRAY: Cell<HWND> = const { Cell::new(null_mut()) }; }

pub struct StatusMenu {
    window: Option<MessageWindow>,
}

impl StatusMenu {
    pub fn install(handler: impl Fn(MenuCommand) + 'static) -> Self {
        let handle = Rc::new(Cell::new(null_mut()));
        let callback_handle = handle.clone();
        // SAFETY: a null-terminated string valid for the duration of the call.
        let restarted = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
        let window = MessageWindow::new(move |message, _, event| {
            let hwnd = callback_handle.get();
            if restarted != 0 && message == restarted {
                add_icon(hwnd);
                return Some(0);
            }
            if message != TRAY_MESSAGE {
                return None;
            }
            match event as u32 {
                WM_LBUTTONUP => handler(MenuCommand::OpenSettings),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd, &handler),
                _ => {}
            }
            Some(0)
        })
        .map_err(|error| eprintln!("sidedoor: couldn't create tray icon: {error}"))
        .ok();
        if let Some(window) = &window {
            handle.set(window.hwnd());
            TRAY.with(|tray| tray.set(window.hwnd()));
            add_icon(window.hwnd());
        }
        Self { window }
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    // SAFETY: zero is the documented initialization for unused fields.
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        ..unsafe { std::mem::zeroed() }
    }
}

fn copy_wide(target: &mut [u16], text: &str) {
    // Keep a trailing NUL even if Windows truncates a long notification.
    let count = target.len().saturating_sub(1);
    for (out, unit) in target.iter_mut().take(count).zip(text.encode_utf16()) {
        *out = unit;
    }
}

fn add_icon(hwnd: HWND) {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = TRAY_MESSAGE;
    // SAFETY: the shared system icon does not need to be destroyed.
    data.hIcon = crate::windows::windows_icon();
    copy_wide(&mut data.szTip, "Sidedoor");
    // SAFETY: fully initialized notification structure, owned HWND.
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
        eprintln!("sidedoor: Windows couldn't add the notification-area icon");
    }
}

fn show_menu(hwnd: HWND, handler: &impl Fn(MenuCommand)) {
    // SAFETY: all menu handles are locally owned and destroyed after tracking.
    unsafe {
        let previous = GetForegroundWindow();
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, 1, wide("Settings…").as_ptr());
        let enabled = system::login_item() == LoginItem::On;
        AppendMenuW(
            menu,
            MF_STRING | if enabled { MF_CHECKED } else { MF_UNCHECKED },
            2,
            wide("Launch at login").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, 3, wide("Reload").as_ptr());
        AppendMenuW(menu, MF_STRING, 4, wide("Quit Sidedoor").as_ptr());
        let mut pointer = POINT::default();
        GetCursorPos(&mut pointer);
        SetForegroundWindow(hwnd);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pointer.x,
            pointer.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        // Restore before invoking the handler so Settings records the actual prior app.
        if !previous.is_null() {
            SetForegroundWindow(previous);
        }
        match selected {
            1 => handler(MenuCommand::OpenSettings),
            2 => {
                if let Err(error) = system::set_launch_at_login(!enabled) {
                    show_notification("Sidedoor", "Couldn't change launch at login", &error);
                }
            }
            3 => handler(MenuCommand::Reload),
            4 => handler(MenuCommand::Quit),
            _ => {}
        }
    }
}

pub fn show_notification(source: &str, title: &str, body: &str) {
    TRAY.with(|tray| {
        if tray.get().is_null() {
            return;
        }
        let mut data = icon_data(tray.get());
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, &format!("{source}\n{body}"));
        // SAFETY: modifies this application's existing tray icon.
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    });
}

impl Drop for StatusMenu {
    fn drop(&mut self) {
        if let Some(window) = &self.window {
            // SAFETY: remove the icon before its callback window is destroyed.
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &icon_data(window.hwnd()));
            }
            TRAY.with(|tray| tray.set(null_mut()));
        }
    }
}
