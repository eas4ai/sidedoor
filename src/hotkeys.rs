//! System-wide hotkeys through Carbon's `RegisterEventHotKey`, which works
//! from any app without Accessibility or Input Monitoring access.

use crate::shortcut::Shortcut;
use std::{cell::RefCell, ffi::c_void, rc::Rc};

type OsStatus = i32;
type EventTargetRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerProc = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OsStatus;

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Default)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerProc,
        type_count: u32,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out_handler: *mut EventHandlerRef,
    ) -> OsStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: EventTargetRef,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> OsStatus;
    fn UnregisterEventHotKey(hotkey: EventHotKeyRef) -> OsStatus;
    fn GetEventParameter(
        event: EventRef,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> OsStatus;
}

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SIGNATURE: u32 = four_cc(b"SKCL");
const EVENT_CLASS_KEYBOARD: u32 = four_cc(b"keyb");
const EVENT_HOTKEY_PRESSED: u32 = 5;
const PARAM_DIRECT_OBJECT: u32 = four_cc(b"----");
const TYPE_HOTKEY_ID: u32 = four_cc(b"hkid");
/// Another app (or this one) already registered the combination.
const HOTKEY_EXISTS: OsStatus = -9878;

/// Why a shortcut couldn't be registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterError {
    TakenByAnotherApp,
    UnsupportedKey,
    Failed(i32),
}

/// Called with a hotkey's id when it is pressed.
type PressHandler = Rc<dyn Fn(u32)>;

thread_local! {
    static ON_PRESS: RefCell<Option<PressHandler>> = const { RefCell::new(None) };
}

extern "C" fn hotkey_pressed(_: EventHandlerCallRef, event: EventRef, _: *mut c_void) -> OsStatus {
    let mut id = EventHotKeyId::default();
    // SAFETY: the event is a hot-key event, whose direct object is its id.
    let status = unsafe {
        GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_HOTKEY_ID,
            std::ptr::null_mut(),
            size_of::<EventHotKeyId>(),
            std::ptr::null_mut(),
            (&raw mut id).cast(),
        )
    };
    if status != 0 || id.signature != SIGNATURE {
        return status;
    }
    // Clone out of the cell so the callback may re-register hotkeys.
    let callback = ON_PRESS.with(|cell| cell.borrow().clone());
    if let Some(callback) = callback {
        callback(id.id);
    }
    0
}

/// Registers one Carbon hotkey. The caller unregisters it.
fn register(shortcut: &Shortcut, id: u32) -> Result<EventHotKeyRef, RegisterError> {
    let key_code = shortcut.key_code().ok_or(RegisterError::UnsupportedKey)?;
    let mut hotkey: EventHotKeyRef = std::ptr::null_mut();
    // SAFETY: plain value arguments and a valid out pointer.
    let status = unsafe {
        RegisterEventHotKey(
            key_code,
            shortcut.carbon_modifiers(),
            EventHotKeyId {
                signature: SIGNATURE,
                id,
            },
            GetApplicationEventTarget(),
            0,
            &mut hotkey,
        )
    };
    match status {
        0 => Ok(hotkey),
        HOTKEY_EXISTS => Err(RegisterError::TakenByAnotherApp),
        other => Err(RegisterError::Failed(other)),
    }
}

/// The app's registered hotkeys. Ids index into the list given to [`set`].
///
/// [`set`]: HotKeys::set
pub struct HotKeys {
    active: Vec<(u32, EventHotKeyRef)>,
    wanted: Vec<Shortcut>,
    suspended: bool,
}

impl HotKeys {
    /// Installs the event handler; `on_press` gets the index of the
    /// shortcut that was pressed.
    pub fn install(on_press: impl Fn(usize) + 'static) -> Self {
        ON_PRESS.with(|cell| {
            *cell.borrow_mut() = Some(Rc::new(move |id| on_press(id as usize - 1)));
        });
        let spec = EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOTKEY_PRESSED,
        };
        let mut handler: EventHandlerRef = std::ptr::null_mut();
        // SAFETY: the handler is a plain function; the spec outlives the call.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                hotkey_pressed,
                1,
                &spec,
                std::ptr::null_mut(),
                &mut handler,
            )
        };
        if status != 0 {
            eprintln!("sidekick: couldn't listen for global shortcuts ({status})");
        }
        Self {
            active: Vec::new(),
            wanted: Vec::new(),
            suspended: false,
        }
    }

    /// Replaces every hotkey. Returns the shortcuts that couldn't be
    /// registered, by index.
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
            let id = index as u32 + 1;
            match register(shortcut, id) {
                Ok(hotkey) => self.active.push((id, hotkey)),
                Err(err) => failures.push((index, err)),
            }
        }
        failures
    }

    fn clear(&mut self) {
        for (_, hotkey) in self.active.drain(..) {
            // SAFETY: each ref came from a successful registration.
            unsafe { UnregisterEventHotKey(hotkey) };
        }
    }

    /// Stops listening, e.g. while the user records a new shortcut.
    pub fn suspend(&mut self) {
        self.suspended = true;
        self.clear();
    }

    pub fn resume(&mut self) -> Vec<(usize, RegisterError)> {
        self.suspended = false;
        self.apply()
    }

    /// Whether another app already owns `shortcut`. Call while suspended so
    /// this app's own hotkeys don't count.
    pub fn probe(&self, shortcut: &Shortcut) -> Result<(), RegisterError> {
        let hotkey = register(shortcut, u32::MAX)?;
        // SAFETY: the ref came from the successful registration above.
        unsafe { UnregisterEventHotKey(hotkey) };
        Ok(())
    }
}

impl Drop for HotKeys {
    fn drop(&mut self) {
        self.clear();
    }
}
