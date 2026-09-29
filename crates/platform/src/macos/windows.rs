use super::*;

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
