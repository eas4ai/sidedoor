//! Global shortcuts without a keyboard hook or elevated privileges.

use crate::{shortcut::Shortcut, windows::message_window::MessageWindow};
use windows_sys::Win32::{
    Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, GetLastError, HWND},
    UI::{
        Input::KeyboardAndMouse::{
            MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
            UnregisterHotKey,
        },
        WindowsAndMessaging::WM_HOTKEY,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterError {
    TakenByAnotherApp,
    UnsupportedKey,
    Failed(i32),
}

pub struct HotKeys {
    window: Option<MessageWindow>,
    active: Vec<i32>,
    wanted: Vec<Shortcut>,
    suspended: bool,
}

impl HotKeys {
    pub fn install(on_press: impl Fn(usize) + 'static) -> Self {
        let window = MessageWindow::new(move |message, id, _| {
            if message == WM_HOTKEY && id > 0 {
                on_press(id - 1);
                Some(0)
            } else {
                None
            }
        })
        .map_err(|error| eprintln!("sidedoor: couldn't listen for shortcuts: {error}"))
        .ok();
        Self {
            window,
            active: Vec::new(),
            wanted: Vec::new(),
            suspended: false,
        }
    }

    pub fn set(&mut self, shortcuts: Vec<Shortcut>) -> Vec<(usize, RegisterError)> {
        self.wanted = shortcuts;
        self.apply()
    }

    fn apply(&mut self) -> Vec<(usize, RegisterError)> {
        self.clear();
        if self.suspended {
            return Vec::new();
        }
        let mut failures = Vec::new();
        for (index, shortcut) in self.wanted.iter().enumerate() {
            let id = index as i32 + 1;
            match self.register(shortcut, id) {
                Ok(()) => self.active.push(id),
                Err(error) => failures.push((index, error)),
            }
        }
        failures
    }

    fn register(&self, shortcut: &Shortcut, id: i32) -> Result<(), RegisterError> {
        let window = self.window.as_ref().ok_or(RegisterError::Failed(0))?;
        register(window.hwnd(), shortcut, id)
    }

    fn clear(&mut self) {
        if let Some(window) = &self.window {
            for id in self.active.drain(..) {
                // SAFETY: id belongs to this object's window on this thread.
                unsafe {
                    UnregisterHotKey(window.hwnd(), id);
                }
            }
        }
    }

    pub fn suspend(&mut self) {
        self.suspended = true;
        self.clear();
    }

    pub fn resume(&mut self) -> Vec<(usize, RegisterError)> {
        self.suspended = false;
        self.apply()
    }

    pub fn probe(&self, shortcut: &Shortcut) -> Result<(), RegisterError> {
        // Application hotkey identifiers occupy 0x0000..=0xbfff.
        const PROBE: i32 = 0xbfff;
        self.register(shortcut, PROBE)?;
        if let Some(window) = &self.window {
            // SAFETY: this registration was just created above.
            unsafe {
                UnregisterHotKey(window.hwnd(), PROBE);
            }
        }
        Ok(())
    }
}

impl Drop for HotKeys {
    fn drop(&mut self) {
        self.clear();
    }
}

fn register(hwnd: HWND, shortcut: &Shortcut, id: i32) -> Result<(), RegisterError> {
    let key = shortcut.key_code().ok_or(RegisterError::UnsupportedKey)?;
    let mut modifiers = MOD_NOREPEAT;
    for (pressed, flag) in [
        (shortcut.control, MOD_CONTROL),
        (shortcut.option, MOD_ALT),
        (shortcut.shift, MOD_SHIFT),
        (shortcut.command, MOD_WIN),
    ] {
        if pressed {
            modifiers |= flag;
        }
    }
    // SAFETY: a live main-thread window and value-only key specification.
    if unsafe { RegisterHotKey(hwnd, id, modifiers, key) } != 0 {
        Ok(())
    } else {
        // SAFETY: read immediately after the failing call, on the same thread.
        let error = unsafe { GetLastError() };
        Err(if error == ERROR_HOTKEY_ALREADY_REGISTERED {
            RegisterError::TakenByAnotherApp
        } else {
            RegisterError::Failed(error as i32)
        })
    }
}
