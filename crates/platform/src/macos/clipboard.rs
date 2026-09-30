use super::*;

pub(super) fn copy_text(text: &str) {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: AppKit's pasteboard type constants are always initialized.
    let string = unsafe { NSPasteboardTypeString };
    pasteboard.setString_forType(&NSString::from_str(text), string);
}
