//! Hidden windows receive shell and hotkey messages on GPUI's UI thread.
//! Callbacks are cloned before invocation so handlers may change registrations.

use std::{cell::RefCell, collections::HashMap, io, ptr::null_mut, rc::Rc};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WNDCLASSW, WS_POPUP,
    },
};

type Handler = Rc<dyn Fn(u32, WPARAM, LPARAM) -> Option<LRESULT>>;

thread_local! {
    static HANDLERS: RefCell<HashMap<isize, Handler>> = RefCell::default();
}

pub fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

pub struct MessageWindow {
    hwnd: HWND,
    // HWNDs must be destroyed on their creating thread.
    _main_thread: std::marker::PhantomData<Rc<()>>,
}

impl MessageWindow {
    pub fn new(
        handler: impl Fn(u32, WPARAM, LPARAM) -> Option<LRESULT> + 'static,
    ) -> io::Result<Self> {
        let class = wide("SidedoorMessages");
        // SAFETY: the class name is valid throughout registration and creation;
        // the window procedure has process lifetime. An existing class is reused.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let description = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&description);
            // A hidden top-level window also receives Explorer restart broadcasts.
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        HANDLERS.with(|handlers| {
            handlers
                .borrow_mut()
                .insert(hwnd as isize, Rc::new(handler))
        });
        Ok(Self {
            hwnd,
            _main_thread: std::marker::PhantomData,
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        HANDLERS.with(|handlers| handlers.borrow_mut().remove(&(self.hwnd as isize)));
        // SAFETY: this object owns the window, on its creating thread.
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handler = HANDLERS.with(|handlers| handlers.borrow().get(&(hwnd as isize)).cloned());
    if let Some(handler) = handler {
        // Do not unwind through the system window procedure.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(message, wparam, lparam)
        })) {
            Ok(Some(result)) => return result,
            Ok(None) => {}
            Err(_) => eprintln!("sidedoor: native message handler panicked"),
        }
    }
    // SAFETY: unchanged arguments received from the Windows message dispatcher.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
