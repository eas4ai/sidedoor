//! Global shortcuts as passive key grabs on the X root window.

use crate::linux::{X, x};
use crate::shortcut::Shortcut;
use std::cell::RefCell;
use x11rb::{
    connection::Connection as _,
    protocol::xproto::{ConnectionExt as _, GrabMode, ModMask},
    x11_utils::X11Error,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterError {
    TakenByAnotherApp,
    UnsupportedKey,
    Failed(i32),
}

/// Modifiers that don't change a shortcut: Caps Lock and Num Lock.
const IGNORED: [u16; 4] = [0, 0x02, 0x10, 0x12];
const RELEVANT: u16 = 0x01 | 0x04 | 0x08 | 0x40;

struct Grab {
    index: usize,
    keycode: u8,
    modifiers: u16,
}

type Handler = Box<dyn Fn(usize)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = RefCell::new(None);
    static GRABS: RefCell<Vec<Grab>> = const { RefCell::new(Vec::new()) };
}

/// Delivers shortcuts pressed since the last call. Runs from the native
/// pump, outside any other borrow.
pub fn dispatch() {
    let presses = crate::linux::take_key_presses();
    if presses.is_empty() {
        return;
    }
    let pressed: Vec<usize> = GRABS.with(|grabs| {
        let grabs = grabs.borrow();
        presses
            .iter()
            .filter_map(|&(keycode, state)| {
                grabs
                    .iter()
                    .find(|grab| grab.keycode == keycode && grab.modifiers == state & RELEVANT)
                    .map(|grab| grab.index)
            })
            .collect()
    });
    HANDLER.with(|handler| {
        if let Some(handler) = &*handler.borrow() {
            for index in pressed {
                handler(index);
            }
        }
    });
}

pub struct HotKeys {
    wanted: Vec<Shortcut>,
    suspended: bool,
}

impl HotKeys {
    pub fn install(on_press: impl Fn(usize) + 'static) -> Self {
        HANDLER.with(|handler| *handler.borrow_mut() = Some(Box::new(on_press)));
        Self {
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
        let Some(x) = x() else {
            return (0..self.wanted.len())
                .map(|index| (index, RegisterError::Failed(0)))
                .collect();
        };
        let mut failures = Vec::new();
        for (index, shortcut) in self.wanted.iter().enumerate() {
            match grab(&x, shortcut) {
                Ok((keycode, modifiers)) => GRABS.with(|grabs| {
                    grabs.borrow_mut().push(Grab {
                        index,
                        keycode,
                        modifiers,
                    })
                }),
                Err(error) => failures.push((index, error)),
            }
        }
        failures
    }

    fn clear(&mut self) {
        // At exit the thread's storage may already be gone; the X server
        // releases the grabs with the connection anyway.
        let grabs: Vec<Grab> = GRABS
            .try_with(|grabs| grabs.borrow_mut().drain(..).collect())
            .unwrap_or_default();
        if let Some(x) = x() {
            for grab in grabs {
                ungrab(&x, grab.keycode, grab.modifiers);
            }
            let _ = x.conn.flush();
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
        let x = x().ok_or(RegisterError::Failed(0))?;
        let already = GRABS.with(|grabs| {
            let keycode = keycode(&x, shortcut);
            grabs
                .borrow()
                .iter()
                .any(|grab| Some(grab.keycode) == keycode && grab.modifiers == modifiers(shortcut))
        });
        if already {
            return Ok(());
        }
        let (keycode, modifiers) = grab(&x, shortcut)?;
        ungrab(&x, keycode, modifiers);
        let _ = x.conn.flush();
        Ok(())
    }
}

impl Drop for HotKeys {
    fn drop(&mut self) {
        self.clear();
        // The handler may be what is being dropped (it can own `self`), so
        // take it out only when it isn't already borrowed.
        let _ = HANDLER.try_with(|handler| {
            if let Ok(mut handler) = handler.try_borrow_mut() {
                drop(handler.take());
            }
        });
    }
}

fn modifiers(shortcut: &Shortcut) -> u16 {
    [
        (shortcut.shift, 0x01),
        (shortcut.control, 0x04),
        (shortcut.option, 0x08),
        (shortcut.command, 0x40),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, bit)| bit)
    .sum()
}

fn keycode(x: &X, shortcut: &Shortcut) -> Option<u8> {
    let keysym = shortcut.key_code()?;
    let setup = x.conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let mapping = x
        .conn
        .get_keyboard_mapping(min, max - min + 1)
        .ok()?
        .reply()
        .ok()?;
    let per = usize::from(mapping.keysyms_per_keycode);
    mapping
        .keysyms
        .chunks(per.max(1))
        .position(|syms| syms.contains(&keysym))
        .map(|offset| min + offset as u8)
}

fn grab(x: &X, shortcut: &Shortcut) -> Result<(u8, u16), RegisterError> {
    let keycode = keycode(x, shortcut).ok_or(RegisterError::UnsupportedKey)?;
    let modifiers = modifiers(shortcut);
    let mut grabbed = Vec::new();
    for ignored in IGNORED {
        let cookie = x.conn.grab_key(
            true,
            x.root,
            ModMask::from(modifiers | ignored),
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        );
        let result = match cookie {
            Ok(cookie) => cookie.check(),
            Err(_) => return Err(RegisterError::Failed(2)),
        };
        match result {
            Ok(()) => grabbed.push(ignored),
            Err(error) => {
                for ignored in grabbed {
                    let _ = x
                        .conn
                        .ungrab_key(keycode, x.root, ModMask::from(modifiers | ignored));
                }
                return Err(match error {
                    x11rb::errors::ReplyError::X11Error(X11Error {
                        error_kind: x11rb::protocol::ErrorKind::Access,
                        ..
                    }) => RegisterError::TakenByAnotherApp,
                    _ => RegisterError::Failed(1),
                });
            }
        }
    }
    let _ = x.conn.flush();
    Ok((keycode, modifiers))
}

fn ungrab(x: &X, keycode: u8, modifiers: u16) {
    for ignored in IGNORED {
        let _ = x
            .conn
            .ungrab_key(keycode, x.root, ModMask::from(modifiers | ignored));
    }
}
