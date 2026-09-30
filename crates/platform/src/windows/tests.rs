//! Run explicitly on an isolated Windows desktop: these tests own its clipboard.

use super::services::WindowsPlatform;
use crate::Platform;

#[test]
#[ignore = "owns the Windows clipboard; run only in an isolated test session"]
fn native_clipboard_roundtrips_text() {
    let platform = WindowsPlatform::default();
    for text in [
        "Windows clipboard — æøå 日本語",
        "https://example.com/path?q=one#two",
    ] {
        platform.copy_text(text);
        assert_eq!(arboard::Clipboard::new().unwrap().get_text().unwrap(), text);
    }
    if let Ok(_guard) = clipboard_win::Clipboard::new_attempts(3) {
        let _ = clipboard_win::empty();
    }
}
