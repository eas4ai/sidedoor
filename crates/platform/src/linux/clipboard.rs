//! The CLIPBOARD selection, served by arboard.

use std::cell::RefCell;

thread_local! {
    // Serves what we copy for as long as the app runs; X11 has no
    // clipboard storage of its own.
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

pub fn copy_text(text: &str) {
    CLIPBOARD.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            *cell = arboard::Clipboard::new()
                .map_err(|error| eprintln!("sidedoor: couldn't open the clipboard: {error}"))
                .ok();
        }
        if let Some(clipboard) = cell.as_mut()
            && let Err(error) = clipboard.set_text(text)
        {
            eprintln!("sidedoor: couldn't write clipboard: {error}");
        }
    });
}
