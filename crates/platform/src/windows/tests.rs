//! Run explicitly on an isolated Windows desktop: these tests own its clipboard.

use super::services::WindowsPlatform;
use crate::{Platform, clipboard::ClipKind};

#[test]
#[ignore = "owns the Windows clipboard; run only in an isolated test session"]
fn native_clipboard_roundtrips_and_respects_history_exclusion() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Ok(_guard) = clipboard_win::Clipboard::new_attempts(3) {
                let _ = clipboard_win::empty();
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = std::env::temp_dir().join(format!("sidedoor-native-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let _cleanup = Cleanup(directory.clone());
    let platform = WindowsPlatform::default();
    let before = platform.pasteboard_change_count();
    for kind in [
        ClipKind::Text {
            text: "Windows clipboard — æøå 日本語".into(),
        },
        ClipKind::Link {
            url: "https://example.com/path?q=one#two".into(),
        },
    ] {
        platform.write_pasteboard(&kind);
        assert_eq!(platform.read_pasteboard(&directory).unwrap().kind, kind);
    }
    assert_ne!(platform.pasteboard_change_count(), before);

    let file = directory.join("a file #1.txt");
    std::fs::write(&file, b"test").unwrap();
    let kind = ClipKind::File { path: file };
    platform.write_pasteboard(&kind);
    assert_eq!(platform.read_pasteboard(&directory).unwrap().kind, kind);
    assert!(
        arboard::Clipboard::new().unwrap().get_text().is_err(),
        "file copy must clear the previous text"
    );

    let path = directory.join("input.png");
    let pixels = [255, 0, 0, 255, 0, 128, 255, 255];
    image::save_buffer(&path, &pixels, 2, 1, image::ColorType::Rgba8).unwrap();
    platform.write_pasteboard(&ClipKind::Image {
        path,
        width: 2,
        height: 1,
    });
    let ClipKind::Image {
        path,
        width,
        height,
    } = platform.read_pasteboard(&directory).unwrap().kind
    else {
        panic!("expected image");
    };
    assert_eq!((width, height), (2, 1));
    assert_eq!(image::open(path).unwrap().into_rgba8().as_raw(), &pixels);

    platform.write_pasteboard(&ClipKind::Text {
        text: "excluded".into(),
    });
    {
        let _guard = clipboard_win::Clipboard::new_attempts(3).unwrap();
        let format = unsafe {
            windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW(
                super::message_window::wide("CanIncludeInClipboardHistory").as_ptr(),
            )
        };
        clipboard_win::raw::set_without_clear(format, &[0, 0, 0, 0]).unwrap();
    }
    assert_eq!(platform.read_pasteboard(&directory), None);
}
