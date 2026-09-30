//! Motion vocabulary shared by the native window animations and the GPUI
//! views, so the dock, its icons and its cards move as one system.

use std::time::Duration;

/// A cubic Bézier timing curve, as in `CAMediaTimingFunction`. `y` values
/// above 1 overshoot the target and settle back, which reads as a spring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// A duration paired with its curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub duration: Duration,
    pub curve: Curve,
}

const fn motion(millis: u64, x1: f32, y1: f32, x2: f32, y2: f32) -> Motion {
    Motion {
        duration: Duration::from_millis(millis),
        curve: Curve { x1, y1, x2, y2 },
    }
}

/// The dock sliding out of its edge: quick, with a soft overshoot.
pub const DOCK_IN: Motion = motion(420, 0.2, 1.22, 0.32, 1.0);
/// The dock tucking away: accelerates out, no bounce.
pub const DOCK_OUT: Motion = motion(220, 0.4, 0.0, 0.85, 0.45);
/// A card appearing beside its item, settling with a hint of overshoot.
pub const CARD_IN: Motion = motion(220, 0.25, 1.04, 0.3, 1.0);
/// A card gliding from one item to the next: a plain ease-out, so quick
/// sweeps along the dock don't wobble.
pub const CARD_MOVE: Motion = motion(170, 0.25, 0.8, 0.25, 1.0);
/// The scale a card grows from as it appears, like an `NSPopover`.
pub const POPOVER_SCALE: f64 = 0.86;
/// The scale a card shrinks to as it leaves.
#[cfg(target_os = "macos")]
pub const POPOVER_EXIT_SCALE: f64 = 0.95;
/// A card leaving.
pub const CARD_OUT: Motion = motion(140, 0.4, 0.0, 1.0, 1.0);

/// A window fading in.
pub const WINDOW_IN: Motion = motion(180, 0.25, 0.1, 0.25, 1.0);

/// How far a dock icon travels in from the edge as the dock appears.
pub const ICON_TRAVEL: f32 = 16.0;
/// Delay between neighboring icons as they arrive.
pub const ICON_STAGGER: Duration = Duration::from_millis(24);
/// Each icon's own arrival.
pub const ICON_IN: Motion = motion(460, 0.2, 1.3, 0.3, 1.0);
/// Largest extra scale under the pointer, as a fraction (0.14 → 114%).
pub const MAGNIFY: f32 = 0.14;
/// How far magnification reaches, in slots.
const MAGNIFY_REACH: f32 = 1.1;
/// A switch's thumb sliding across, settling with a hint of spring.
pub const SWITCH: Motion = motion(260, 0.3, 1.12, 0.4, 1.0);
/// Dock items sliding apart to make room for one being dragged, and into
/// place after a drop: a spring, so a gap that moves with the pointer
/// redirects smoothly. Its response, and a damping just under critical for
/// a slight settle.
pub const REORDER: Duration = Duration::from_millis(300);
pub const REORDER_DAMPING: f32 = 0.86;
/// Scale of an item while it is pressed.
pub const PRESSED: f32 = 0.86;

/// Dock-style magnification of an item whose center is `center`, given the
/// pointer's position along the dock (same units). Falls off smoothly with
/// distance and is 1.0 without a pointer.
pub fn magnification(pointer: Option<f32>, center: f32, slot: f32) -> f32 {
    let Some(pointer) = pointer else {
        return 1.0;
    };
    let distance = (pointer - center).abs() / (slot * MAGNIFY_REACH);
    1.0 + MAGNIFY * (-distance * distance * 2.2).exp()
}

/// Samples a curve at `t` (0–1) for GPUI-side transitions, so views and
/// native windows share one feel.
pub fn sample(curve: Curve, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // Solve x(s) = t for s by Newton's method, then return y(s).
    let bezier = |a: f32, b: f32, s: f32| {
        let inv = 1.0 - s;
        3.0 * inv * inv * s * a + 3.0 * inv * s * s * b + s * s * s
    };
    let slope = |a: f32, b: f32, s: f32| {
        let inv = 1.0 - s;
        3.0 * inv * inv * a + 6.0 * inv * s * (b - a) + 3.0 * s * s * (1.0 - b)
    };
    let mut s = t;
    for _ in 0..8 {
        let error = bezier(curve.x1, curve.x2, s) - t;
        let d = slope(curve.x1, curve.x2, s);
        if error.abs() < 1e-5 || d.abs() < 1e-6 {
            break;
        }
        s = (s - error / d).clamp(0.0, 1.0);
    }
    bezier(curve.y1, curve.y2, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnification_peaks_under_the_pointer_and_fades_with_distance() {
        let slot = 52.0;
        let under = magnification(Some(100.0), 100.0, slot);
        let near = magnification(Some(100.0), 152.0, slot);
        let far = magnification(Some(100.0), 360.0, slot);
        assert!((under - (1.0 + MAGNIFY)).abs() < 1e-6);
        assert!(near > 1.0 && near < under);
        assert!(far < 1.001);
        assert_eq!(magnification(None, 100.0, slot), 1.0);
    }

    #[test]
    fn curves_start_and_end_on_their_endpoints() {
        for motion in [
            DOCK_IN, DOCK_OUT, CARD_IN, CARD_MOVE, CARD_OUT, ICON_IN, SWITCH,
        ] {
            assert!(sample(motion.curve, 0.0).abs() < 1e-4);
            assert!((sample(motion.curve, 1.0) - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn spring_curves_overshoot_before_settling() {
        let peak = (1..100)
            .map(|step| sample(ICON_IN.curve, step as f32 / 100.0))
            .fold(0.0_f32, f32::max);
        assert!(peak > 1.02, "expected an overshoot, peak was {peak}");
    }
}
