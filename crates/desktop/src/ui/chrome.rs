//! Window title bars. Every platform keeps its own: macOS a transparent
//! title bar whose traffic lights sit in the window's toolbar, Windows and
//! Linux the system caption above the content. Views ask here whether to
//! leave room for the traffic lights and draw their own title.

/// Whether the title bar is drawn over the content, as on macOS. Elsewhere
/// the system caption sits above it and shows the window's title.
pub const INSET_TITLE_BAR: bool = cfg!(target_os = "macos");

/// A title strip's height: its full height where the title bar is inset
/// into the content, and nothing where the system caption shows the title.
pub const fn title_strip(height: f32) -> f32 {
    if INSET_TITLE_BAR { height } else { 0.0 }
}
