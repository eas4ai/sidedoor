//! The AppKit side: [`MacPlatform`] for workspace, pasteboard and storage,
//! plus helpers that dress and move the native windows GPUI created.
//!
//! Everything here runs on the main thread.

use crate::{
    clipboard::{ClipKind, History},
    config::{Appearance, Config},
    geometry::{CardPlacement, PathStep, Point, Rect, Screen},
    motion::{
        CARD_IN, CARD_MOVE, CARD_OUT, Curve, DOCK_IN, DOCK_OUT, Motion, POPOVER_EXIT_SCALE,
        POPOVER_SCALE, WINDOW_IN,
    },
    platform::{Accessibility, AppInfo, Copied, LoginItem, Platform, cache_dir},
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
    NSPasteboardTypeFileURL, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
    NSRunningApplication, NSScreen, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
    NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSBundle, NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString, NSTimeZone, NSURL,
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

/// Runs without a Dock icon or menu bar, like a menu-bar utility.
pub fn set_accessory_policy() {
    NSApplication::sharedApplication(mtm())
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

pub fn set_appearance(appearance: Appearance) {
    // SAFETY: AppKit's appearance name constants are always initialized.
    let name = match appearance {
        Appearance::System => None,
        Appearance::Light => Some(unsafe { NSAppearanceNameAqua }),
        Appearance::Dark => Some(unsafe { NSAppearanceNameDarkAqua }),
    };
    let appearance = name.and_then(NSAppearance::appearanceNamed);
    NSApplication::sharedApplication(mtm()).setAppearance(appearance.as_deref());
}

// MARK: Activation

/// The app the user was in, so focus can go back to it after the history
/// window takes the keyboard.
pub fn frontmost_app() -> Option<i32> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    (pid != std::process::id() as i32).then_some(pid)
}

pub fn activate_app(pid: i32) {
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

/// Fades a newly opened window in.
pub fn fade_in(window: &NSWindow) {
    window.setAlphaValue(0.0);
    animate(WINDOW_IN, || window.animator().setAlphaValue(1.0));
}

/// Adds the system sidebar material behind a regular window's content.
pub fn add_window_material(window: &NSWindow) {
    let Some(content) = window.contentView() else {
        return;
    };
    let view =
        NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm()), content.bounds());
    view.setMaterial(NSVisualEffectMaterial::Sidebar);
    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    view.setState(NSVisualEffectState::FollowsWindowActiveState);
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
}

// MARK: Launch at login

/// Whether the running app is a bundle, which login items require.
fn is_bundled() -> bool {
    NSBundle::mainBundle()
        .bundlePath()
        .to_string()
        .ends_with(".app")
}

pub fn login_item() -> LoginItem {
    if !is_bundled() {
        return LoginItem::Unavailable;
    }
    // SAFETY: querying the main app's own service has no preconditions.
    let status = unsafe { SMAppService::mainAppService().status() };
    match status {
        SMAppServiceStatus::Enabled => LoginItem::On,
        SMAppServiceStatus::RequiresApproval => LoginItem::NeedsApproval,
        _ => LoginItem::Off,
    }
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    if !is_bundled() {
        return Err("only an app bundle can launch at login".into());
    }
    // SAFETY: registering the main app as its own login item is the
    // documented use of `mainAppService`.
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|err| err.localizedDescription().to_string())?;
    if login_item() == LoginItem::NeedsApproval {
        // SAFETY: opening System Settings has no preconditions.
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }
    Ok(())
}

// MARK: Platform

#[derive(Default)]
pub struct MacPlatform {
    /// The shared Bun process that runs plugins.
    plugins: crate::plugin::Runner,
}

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
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let urls = NSArray::from_retained_slice(&[url]);
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    }

    fn pasteboard_change_count(&self) -> isize {
        NSPasteboard::generalPasteboard().changeCount()
    }

    fn read_pasteboard(&self, image_dir: &Path) -> Option<Copied> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let types: Vec<String> = pasteboard
            .types()?
            .iter()
            .map(|kind| kind.to_string())
            .collect();
        // Password managers mark secrets; nspasteboard.org documents these.
        const PRIVATE: &[&str] = &[
            "org.nspasteboard.ConcealedType",
            "org.nspasteboard.TransientType",
            "org.nspasteboard.AutoGeneratedType",
            "com.agilebits.onepassword",
        ];
        if types.iter().any(|kind| PRIVATE.contains(&kind.as_str())) {
            return None;
        }

        // SAFETY: AppKit's pasteboard type constants are always initialized.
        let (file_url, string, png, tiff) = unsafe {
            (
                NSPasteboardTypeFileURL,
                NSPasteboardTypeString,
                NSPasteboardTypePNG,
                NSPasteboardTypeTIFF,
            )
        };
        let has = |kind: &NSString| types.iter().any(|t| *t == kind.to_string());

        let kind = if has(file_url) {
            let url = pasteboard.stringForType(file_url)?;
            let path = NSURL::URLWithString(&url)?.path()?.to_string();
            ClipKind::File { path: path.into() }
        } else if let Some(text) = pasteboard
            .stringForType(string)
            .filter(|text| text.length() > 0)
        {
            ClipKind::from_text(text.to_string())
        } else if has(png) || has(tiff) {
            let data = pasteboard
                .dataForType(png)
                .or_else(|| pasteboard.dataForType(tiff))?;
            save_image(&data, image_dir)?
        } else {
            return None;
        };

        let source = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .and_then(|app| app.localizedName())
            .map(|name| name.to_string());
        Some(Copied { kind, source })
    }

    fn notify(&self, source: &str, title: &str, body: &str) {
        crate::notifications::show(source, title, body);
    }

    fn write_pasteboard(&self, kind: &ClipKind) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // SAFETY: AppKit's pasteboard type constants are always initialized.
        let (file_url, string, png) = unsafe {
            (
                NSPasteboardTypeFileURL,
                NSPasteboardTypeString,
                NSPasteboardTypePNG,
            )
        };
        match kind {
            ClipKind::Text { text } => {
                pasteboard.setString_forType(&NSString::from_str(text), string);
            }
            ClipKind::Link { url } => {
                pasteboard.setString_forType(&NSString::from_str(url), string);
            }
            ClipKind::Image { path, .. } => {
                if let Ok(bytes) = std::fs::read(path) {
                    pasteboard.setData_forType(Some(&NSData::from_vec(bytes)), png);
                }
            }
            ClipKind::File { path } => {
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                if let Some(url) = url.absoluteString() {
                    pasteboard.setString_forType(&url, file_url);
                }
            }
        }
    }

    fn save_config(&self, config: &Config) -> io::Result<()> {
        config.save()
    }

    fn save_history(&self, history: &History) -> io::Result<()> {
        let path = History::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec(history).map_err(io::Error::other)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, json)?;
        std::fs::rename(temp, path)
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

    fn plugins(&self) -> Vec<crate::plugin::Manifest> {
        crate::plugin::discover(&crate::plugin::plugins_dir())
    }

    fn start_plugin(
        &self,
        manifest: &crate::plugin::Manifest,
        settings: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<crate::plugin::Connection, String> {
        static BUN: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        let bun = BUN
            .get_or_init(crate::plugin::find_bun)
            .as_ref()
            .ok_or("Plugins need Bun. Install it from bun.sh, then reload.")?;
        self.plugins
            .start(manifest, settings, bun, &crate::plugin::sdk_dir())
            .map_err(|err| format!("Couldn't start {}: {err}", manifest.name))
    }

    fn create_plugin(&self, name: &str) -> Result<crate::plugin::Manifest, String> {
        let dir = crate::plugin::plugins_dir();
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        crate::plugin::create(&dir, name)
            .map_err(|err| format!("Couldn't create the plugin: {err}"))
    }
}

fn app_info(bundle_id: String, path: PathBuf) -> AppInfo {
    let name = path
        .file_stem()
        .map_or_else(|| bundle_id.clone(), |stem| stem.to_string_lossy().into());
    let icon = cache_dir().join("icons").join(format!("{bundle_id}.png"));
    let icon = (icon.exists() || write_icon_png(&path, &icon).is_ok()).then_some(icon);
    AppInfo {
        bundle_id,
        name,
        path,
        icon,
    }
}

fn save_image(data: &NSData, dir: &Path) -> Option<ClipKind> {
    let bitmap = NSBitmapImageRep::imageRepWithData(data)?;
    let (width, height) = (bitmap.pixelsWide(), bitmap.pixelsHigh());
    // SAFETY: an empty property dictionary is valid for PNG output.
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    std::fs::create_dir_all(dir).ok()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let path = dir.join(format!("{nanos}.png"));
    std::fs::write(&path, png.to_vec()).ok()?;
    Some(ClipKind::Image {
        path,
        width: width.max(0) as u32,
        height: height.max(0) as u32,
    })
}

/// Renders the Finder icon of `app` to a 256×256 PNG at `out`.
fn write_icon_png(app: &Path, out: &Path) -> io::Result<()> {
    const SIZE: isize = 256;
    let icon =
        NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&app.to_string_lossy()));
    // SAFETY: a null plane pointer asks AppKit to allocate the bitmap; the
    // remaining arguments describe 8-bit RGBA.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            SIZE,
            SIZE,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }
    .ok_or_else(|| io::Error::other("couldn't allocate icon bitmap"))?;

    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)
        .ok_or_else(|| io::Error::other("couldn't draw icon"))?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    icon.drawInRect(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(SIZE as f64, SIZE as f64),
    ));
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    // SAFETY: an empty property dictionary is valid for PNG output.
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or_else(|| io::Error::other("couldn't encode icon"))?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, png.to_vec())
}

// MARK: Windows

/// The AppKit window behind a GPUI window.
pub fn ns_window(window: &gpui_kit::Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI hands out a live NSView for its window on the main thread.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

/// Which backdrop a panel uses.
#[derive(Clone, Copy, Debug)]
pub enum Backdrop {
    /// A thin, dock-like material.
    Dock,
    /// The material macOS uses for popovers and HUD cards.
    Card,
}

/// Turns a GPUI pop-up window into a borderless, shadowed panel with a
/// system material behind the GPUI content, and returns that material view.
///
/// Uses Liquid Glass (`NSGlassEffectView`) where the OS has it, and
/// `NSVisualEffectView` otherwise.
pub fn configure_panel(
    window: &NSWindow,
    corner_radius: f64,
    backdrop: Backdrop,
) -> Option<Retained<NSView>> {
    window.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
    window.setMovable(false);
    window.setHasShadow(true);

    let content = window.contentView()?;
    let bounds = content.bounds();
    let material = match glass_view(bounds, corner_radius) {
        Some(glass) => glass,
        None => visual_effect_view(bounds, backdrop),
    };
    material.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    material.setWantsLayer(true);
    if let Some(layer) = material.layer() {
        layer.setCornerRadius(corner_radius);
        layer.setMasksToBounds(true);
    }
    content.addSubview_positioned_relativeTo(&material, NSWindowOrderingMode::Below, None);
    window.invalidateShadow();
    Some(material)
}

fn glass_view(bounds: NSRect, corner_radius: f64) -> Option<Retained<NSView>> {
    let class = AnyClass::get(c"NSGlassEffectView")?;
    // SAFETY: NSGlassEffectView is an NSView subclass (macOS 26+) with a
    // `cornerRadius` property.
    unsafe {
        let allocated: Allocated<NSView> = msg_send![class, alloc];
        let view: Retained<NSView> = msg_send![allocated, initWithFrame: bounds];
        let _: () = msg_send![&*view, setCornerRadius: corner_radius];
        Some(view)
    }
}

fn visual_effect_view(bounds: NSRect, backdrop: Backdrop) -> Retained<NSView> {
    let view = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm()), bounds);
    view.setMaterial(match backdrop {
        Backdrop::Dock => NSVisualEffectMaterial::Menu,
        Backdrop::Card => NSVisualEffectMaterial::Popover,
    });
    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    // The app never becomes active, so keep the material vibrant regardless.
    view.setState(NSVisualEffectState::Active);
    Retained::into_super(view)
}

/// Masks a card's material to its outline: rounded body and arrow as one
/// shape, so the glass, its shadow and the drawn hairline all agree.
pub fn shape_card(material: &NSView, placement: &CardPlacement) {
    let Some(layer) = material.layer() else {
        return;
    };
    // Liquid Glass draws its own rounded rim along its bounds, which would
    // show inside the outline as a second border. Let the material overhang
    // the window so its rim is masked away and the drawn hairline is the
    // only edge.
    const OVERHANG: f64 = 32.0;
    let (width, height) = (placement.frame.width, placement.frame.height);
    // Window points from the top-left → material points from its
    // bottom-left, which sits `OVERHANG` outside the window.
    let flip = |point: Point| NSPoint::new(point.x + OVERHANG, height - point.y + OVERHANG);
    let path = NSBezierPath::bezierPath();
    for step in placement.outline() {
        match step {
            PathStep::Move(to) => path.moveToPoint(flip(to)),
            PathStep::Line(to) => path.lineToPoint(flip(to)),
            PathStep::Cubic {
                control_a,
                control_b,
                to,
            } => path.curveToPoint_controlPoint1_controlPoint2(
                flip(to),
                flip(control_a),
                flip(control_b),
            ),
            PathStep::Close => path.closePath(),
        }
    }

    // Swap the shape in one step; Core Animation would otherwise tween the
    // old mask into the new one while the window glides.
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    material.setFrame(NSRect::new(
        NSPoint::new(-OVERHANG, -OVERHANG),
        NSSize::new(width + 2.0 * OVERHANG, height + 2.0 * OVERHANG),
    ));
    let mask = CAShapeLayer::new();
    mask.setPath(Some(&path.CGPath()));
    // The corner radius from `configure_panel` would clip the arrow.
    layer.setCornerRadius(0.0);
    layer.setMasksToBounds(false);
    // SAFETY: the mask layer is retained by the material's layer.
    unsafe { layer.setMask(Some(&mask)) };
    CATransaction::commit();
}

fn animate(motion: Motion, changes: impl FnOnce()) {
    let Curve { x1, y1, x2, y2 } = motion.curve;
    NSAnimationContext::beginGrouping();
    let context = NSAnimationContext::currentContext();
    context.setDuration(motion.duration.as_secs_f64());
    context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
        x1, y1, x2, y2,
    )));
    changes();
    NSAnimationContext::endGrouping();
}

/// Slides the dock to `frame`, springing in or tucking away.
pub fn slide_dock(window: &NSWindow, frame: Rect, visible: bool, animated: bool) {
    let alpha = if visible { 1.0 } else { 0.0 };
    window.setIgnoresMouseEvents(!visible);
    if visible {
        window.orderFrontRegardless();
    }
    if animated {
        animate(if visible { DOCK_IN } else { DOCK_OUT }, || {
            let animator = window.animator();
            animator.setFrame_display(ns_rect(frame), true);
            animator.setAlphaValue(alpha);
        });
    } else {
        window.setFrame_display(ns_rect(frame), true);
        window.setAlphaValue(alpha);
    }
}

/// Moves a window so its top-left corner matches `frame`. Size changes go
/// through GPUI (`Window::resize`), which keeps the top-left corner fixed.
pub fn set_top_left(window: &NSWindow, frame: Rect) {
    window.setFrameTopLeftPoint(NSPoint::new(frame.x, frame.max_y()));
}

/// How a card reaches its frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardEntry {
    /// Grow out of `anchor` (window points from the top-left, the arrow
    /// tip) while fading in, like an `NSPopover`.
    Pop { anchor: Point },
    /// Glide from `from`, a frame of the new size where the previous card was.
    Glide { from: Rect },
    /// Jump straight there.
    Snap,
}

/// Shows a card at `frame`. The window already has `frame`'s size.
pub fn show_card(window: &NSWindow, frame: Rect, entry: CardEntry) {
    window.setIgnoresMouseEvents(false);
    window.orderFrontRegardless();
    match entry {
        CardEntry::Pop { anchor } => {
            set_top_left(window, frame);
            window.setAlphaValue(0.0);
            scale_content(window, anchor, POPOVER_SCALE, 1.0, CARD_IN);
            animate(CARD_IN, || window.animator().setAlphaValue(1.0));
        }
        CardEntry::Glide { from } => {
            set_top_left(window, from);
            window.setAlphaValue(1.0);
            animate(CARD_MOVE, || {
                window.animator().setFrame_display(ns_rect(frame), true);
            });
        }
        CardEntry::Snap => {
            set_top_left(window, frame);
            window.setAlphaValue(1.0);
        }
    }
    window.invalidateShadow();
}

/// Fades a card out, shrinking back into `anchor` (its arrow tip) unless
/// motion is reduced.
pub fn hide_card(window: &NSWindow, anchor: Option<Point>) {
    window.setIgnoresMouseEvents(true);
    if let Some(anchor) = anchor {
        scale_content(window, anchor, 1.0, POPOVER_EXIT_SCALE, CARD_OUT);
    }
    animate(CARD_OUT, || window.animator().setAlphaValue(0.0));
}

/// Animates the window's whole content (GPUI drawing and material) from
/// `from` to `to` scale about `anchor`, in window points from the top-left.
/// Only the presentation animates; the content's own transform stays
/// identity, so nothing lingers once the animation ends.
fn scale_content(window: &NSWindow, anchor: Point, from: f64, to: f64, motion: Motion) {
    let Some(layer) = window.contentView().and_then(|view| view.layer()) else {
        return;
    };
    let bounds = layer.bounds();
    let unit = layer.anchorPoint();
    // Layer points run from the bottom-left unless the layer is flipped.
    let pivot_y = if layer.isGeometryFlipped() {
        anchor.y
    } else {
        bounds.size.height - anchor.y
    };
    let (dx, dy) = (
        anchor.x - unit.x * bounds.size.width,
        pivot_y - unit.y * bounds.size.height,
    );
    let about = |scale: f64| {
        CATransform3D::new_translation(dx, dy, 0.0)
            .scale(scale, scale, 1.0)
            .translate(-dx, -dy, 0.0)
    };
    let Curve { x1, y1, x2, y2 } = motion.curve;
    let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
    // SAFETY: `transform` animates `CATransform3D` values boxed in NSValue.
    unsafe {
        animation.setFromValue(Some(&NSValue::valueWithCATransform3D(about(from))));
        animation.setToValue(Some(&NSValue::valueWithCATransform3D(about(to))));
    }
    animation.setDuration(motion.duration.as_secs_f64());
    animation.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
        x1, y1, x2, y2,
    )));
    layer.addAnimation_forKey(&animation, Some(&NSString::from_str("popover")));
}
