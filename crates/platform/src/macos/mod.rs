//! AppKit workspace and clipboard services,
//! plus helpers that dress and move the native windows GPUI created.
//!
//! Everything here runs on the main thread.

use crate::cache_dir;
use crate::{
    api::{Accessibility, AppInfo, LoginItem, Platform},
    config::Appearance,
    geometry::{CardPlacement, PathStep, Point, Rect, Screen},
    motion::{
        CARD_IN, CARD_MOVE, CARD_OUT, Curve, DOCK_IN, DOCK_OUT, Motion, POPOVER_EXIT_SCALE,
        POPOVER_SCALE, WINDOW_IN,
    },
};
use objc2::{
    AnyThread, MainThreadMarker, MainThreadOnly, msg_send,
    rc::{Allocated, Retained},
    runtime::AnyClass,
};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearance, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationOptions,
    NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSBezierPath, NSBitmapImageFileType,
    NSBitmapImageRep, NSDeviceRGBColorSpace, NSEvent, NSGraphicsContext, NSPasteboard,
    NSPasteboardTypeString, NSRunningApplication, NSScreen, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowOrderingMode, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{
    NSBundle, NSDictionary, NSFileManager, NSPoint, NSRect, NSSize, NSString, NSTimeZone, NSURL,
    NSValue,
};
use objc2_quartz_core::{
    CABasicAnimation, CAMediaTiming as _, CAMediaTimingFunction, CAShapeLayer, CATransaction,
    CATransform3D, NSValueCATransform3DAdditions as _,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit calls must run on the main thread")
}

fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}

fn rect(ns: NSRect) -> Rect {
    Rect::new(ns.origin.x, ns.origin.y, ns.size.width, ns.size.height)
}

// MARK: Platform

#[derive(Default)]
pub struct MacPlatform {}

impl Platform for MacPlatform {
    fn main_screen(&self) -> Option<Screen> {
        let screens = NSScreen::screens(mtm());
        let screen = screens.firstObject()?;
        Some(Screen {
            frame: rect(screen.frame()),
            visible: rect(screen.visibleFrame()),
        })
    }

    fn pointer(&self) -> Point {
        let point = NSEvent::mouseLocation();
        Point {
            x: point.x,
            y: point.y,
        }
    }

    fn running_bundle_ids(&self) -> HashSet<String> {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter_map(|app| app.bundleIdentifier().map(|id| id.to_string()))
            .collect()
    }

    fn accessibility(&self) -> Accessibility {
        let workspace = NSWorkspace::sharedWorkspace();
        Accessibility {
            reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
            increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
            reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
        }
    }

    fn utc_offset(&self) -> i64 {
        NSTimeZone::localTimeZone().secondsFromGMT() as i64
    }

    fn app_by_bundle_id(&self, bundle_id: &str) -> Option<AppInfo> {
        let url = NSWorkspace::sharedWorkspace()
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
        let path = PathBuf::from(url.path()?.to_string());
        Some(app_info(bundle_id.to_string(), path))
    }

    fn app_at(&self, path: &Path) -> Option<AppInfo> {
        if path.extension().is_none_or(|ext| ext != "app") {
            return None;
        }
        let bundle = NSBundle::bundleWithPath(&NSString::from_str(&path.to_string_lossy()))?;
        let bundle_id = bundle.bundleIdentifier()?.to_string();
        Some(app_info(bundle_id, path.to_path_buf()))
    }

    fn open(&self, path: &Path) -> io::Result<()> {
        std::process::Command::new("/usr/bin/open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }

    fn reveal_in_finder(&self, path: &Path) {
        // Not `activateFileViewerSelectingURLs`: it waits for Finder in a
        // nested run loop, which runs GPUI's queued tasks while the click
        // that called it is still borrowing the app, and that aborts.
        if let Err(err) = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .spawn()
        {
            eprintln!("sidedoor: couldn't reveal {}: {err}", path.display());
        }
    }

    fn trash(&self, path: &Path) -> std::io::Result<()> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|err| std::io::Error::other(err.localizedDescription().to_string()))
    }

    fn notify(&self, source: &str, title: &str, body: &str) {
        crate::notifications::show(source, title, body);
    }

    fn copy_text(&self, text: &str) {
        clipboard::copy_text(text)
    }

    fn set_appearance(&self, appearance: Appearance) {
        set_appearance(appearance);
    }

    fn login_item(&self) -> LoginItem {
        login_item()
    }

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        set_launch_at_login(enabled)
    }
}

mod apps;
mod clipboard;
mod system;
mod windows;
use apps::app_info;
pub use system::{login_item, set_launch_at_login};
pub use windows::*;
