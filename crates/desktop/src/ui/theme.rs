//! AppKit semantic colors and text sizes, so the UI reads as native macOS.

use crate::app::host::Accessibility;
use gpui_kit::{App, Hsla, Window, WindowAppearance, component::Theme, rgba};

/// macOS text styles (points).
pub mod text {
    pub const TITLE3: f32 = 15.0;
    pub const BODY: f32 = 13.0;
    pub const CALLOUT: f32 = 12.0;
    pub const SUBHEADLINE: f32 = 11.0;
    pub const CAPTION: f32 = 10.0;
    pub const DISPLAY: f32 = 28.0;
}

fn is_dark(window: &Window) -> bool {
    matches!(
        ::platform::native::appearance(window),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// Points GPUI Kit's theme at the window's light or dark appearance, with
/// the accent-colored caret and selection macOS text fields use. Call when a
/// window opens and whenever its appearance changes.
pub fn sync_kit_theme(window: &mut Window, cx: &mut App) {
    let dark = is_dark(window);
    if !cx.has_global::<Theme>() || Theme::global(cx).is_dark() != dark {
        Theme::change(::platform::native::appearance(window), Some(window), cx);
    }
    let caret = if dark {
        Palette::dark()
    } else {
        Palette::light()
    }
    .blue;
    if Theme::global(cx).caret == caret {
        return;
    }
    Theme::update(cx, |theme| {
        theme.colors.caret = caret;
        // `selectedTextBackgroundColor`
        theme.colors.selection = color(if dark { 0x3f638bff } else { 0xb3d7ffff });
    });
}

#[derive(Clone, Copy)]
pub struct Palette {
    /// `labelColor`
    pub label: Hsla,
    /// `secondaryLabelColor`
    pub secondary: Hsla,
    /// `tertiaryLabelColor`
    pub tertiary: Hsla,
    /// Hairline around floating panels.
    pub stroke: Hsla,
    /// `separatorColor`
    pub separator: Hsla,
    /// Track behind gauges.
    pub track: Hsla,
    /// Row highlight and placeholder tiles.
    pub fill: Hsla,
    /// Panel background when the material is replaced by paint (Reduce
    /// Transparency), otherwise transparent so the material shows.
    pub surface: Hsla,
    /// Drop-target highlight.
    pub accent_fill: Hsla,
    /// Selected segment of a segmented control.
    pub segment: Hsla,
    /// Text and glyphs on an accent-colored background.
    pub on_accent: Hsla,
    /// A key drawn as a keycap; also the face of push buttons.
    pub keycap: Hsla,
    /// Background of a grouped form section.
    pub group: Hsla,
    pub blue: Hsla,
    pub green: Hsla,
    pub orange: Hsla,
    pub red: Hsla,
    pub purple: Hsla,
    pub purple_deep: Hsla,
}

fn color(value: u32) -> Hsla {
    rgba(value).into()
}

impl Palette {
    pub fn new(window: &Window, accessibility: Accessibility) -> Self {
        let dark = is_dark(window);
        let mut palette = if dark { Self::dark() } else { Self::light() };
        if accessibility.increase_contrast {
            palette.secondary = palette.label.opacity(0.8);
            palette.tertiary = palette.label.opacity(0.6);
            palette.stroke = palette.label.opacity(0.55);
            palette.separator = palette.label.opacity(0.35);
        }
        if accessibility.reduce_transparency || !::platform::native::has_material() {
            palette.group = if dark {
                color(0x2a2a2aff)
            } else {
                color(0xffffffff)
            };
        } else {
            palette.surface = color(0x00000000);
        }
        palette
    }

    fn light() -> Self {
        Self {
            label: color(0x000000d9),
            secondary: color(0x00000080),
            tertiary: color(0x00000042),
            stroke: color(0x0000001f),
            separator: color(0x0000001a),
            track: color(0x0000000f),
            fill: color(0x0000000d),
            surface: color(0xecececff),
            accent_fill: color(0x007aff33),
            segment: color(0xffffffff),
            on_accent: color(0xffffffff),
            keycap: color(0xffffffff),
            group: color(0xffffffb3),
            blue: color(0x007affff),
            green: color(0x28cd41ff),
            orange: color(0xff9500ff),
            red: color(0xff3b30ff),
            purple: color(0x8e7cffff),
            purple_deep: color(0x5a44e6ff),
        }
    }

    fn dark() -> Self {
        Self {
            label: color(0xffffffd9),
            secondary: color(0xffffff8c),
            tertiary: color(0xffffff40),
            stroke: color(0xffffff26),
            separator: color(0xffffff1a),
            track: color(0xffffff1a),
            fill: color(0xffffff14),
            surface: color(0x1e1e1eff),
            accent_fill: color(0x0a84ff40),
            segment: color(0xffffff2e),
            on_accent: color(0xffffffff),
            keycap: color(0xffffff1f),
            group: color(0xffffff0d),
            blue: color(0x0a84ffff),
            green: color(0x32d74bff),
            orange: color(0xff9f0aff),
            red: color(0xff453aff),
            purple: color(0x8e7cffff),
            purple_deep: color(0x5a44e6ff),
        }
    }
}
