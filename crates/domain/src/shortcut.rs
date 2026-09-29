//! Global keyboard shortcuts as the user sees and stores them.
//!
//! Stored like GPUI keystrokes ("ctrl-cmd-v"), shown the macOS way (⌃⌘V),
//! and registered by virtual key code, which follows the key's position on
//! an ANSI keyboard.

use std::fmt;

/// Whether keys are named as on a PC keyboard (Ctrl+Alt+V) rather than
/// with the Mac symbols (⌃⌥V). Ctrl is then also the primary modifier.
pub const PC_KEYS: bool = !cfg!(target_os = "macos");

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub control: bool,
    pub option: bool,
    pub shift: bool,
    pub command: bool,
    /// GPUI's key name: "v", "f5", "space", "left", …
    pub key: String,
}

/// Why a key combination can't be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NeedsModifier,
    UnsupportedKey,
    ReservedBySystem,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NeedsModifier => {
                if cfg!(windows) {
                    "Add Ctrl, Alt or Win, or use an F-key on its own."
                } else if cfg!(target_os = "linux") {
                    "Add Ctrl, Alt or Super, or use an F-key on its own."
                } else {
                    "Add ⌘, ⌥ or ⌃, or use an F-key on its own."
                }
            }
            Self::UnsupportedKey => "That key can't be used in a shortcut.",
            Self::ReservedBySystem => {
                if cfg!(windows) {
                    "Windows already uses this shortcut."
                } else if cfg!(target_os = "linux") {
                    "The system already uses this shortcut."
                } else {
                    "macOS already uses this shortcut."
                }
            }
        })
    }
}

/// Shortcuts macOS keeps for itself or that every app relies on.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const RESERVED: &[&str] = &[
    "cmd-space",
    "ctrl-space",
    "cmd-tab",
    "shift-cmd-tab",
    "cmd-q",
    "cmd-w",
    "cmd-h",
    "cmd-m",
    "cmd-c",
    "cmd-v",
    "cmd-x",
    "cmd-z",
    "cmd-a",
    "cmd-s",
    "shift-cmd-3",
    "shift-cmd-4",
    "shift-cmd-5",
    "alt-cmd-escape",
];

/// Shortcuts Linux desktops (GNOME, KDE, Xfce) keep for themselves or
/// that every app relies on.
#[cfg(target_os = "linux")]
const RESERVED: &[&str] = &[
    "alt-tab",
    "alt-shift-tab",
    "alt-f2",
    "alt-f4",
    "ctrl-alt-delete",
    "ctrl-alt-t",
    "ctrl-alt-left",
    "ctrl-alt-right",
    "cmd-a",
    "cmd-d",
    "cmd-l",
    "cmd-v",
    "cmd-tab",
    "cmd-space",
    "ctrl-c",
    "ctrl-v",
    "ctrl-x",
    "ctrl-z",
    "ctrl-a",
    "ctrl-s",
    "ctrl-w",
    "ctrl-q",
];

#[cfg(target_os = "windows")]
const RESERVED: &[&str] = &[
    "alt-tab",
    "alt-shift-tab",
    "alt-f4",
    "ctrl-alt-delete",
    "ctrl-shift-escape",
    "cmd-l",
    "cmd-d",
    "cmd-e",
    "cmd-r",
    "cmd-v",
    "cmd-tab",
    "cmd-space",
    "ctrl-c",
    "ctrl-v",
    "ctrl-x",
    "ctrl-z",
    "ctrl-a",
    "ctrl-s",
    "ctrl-w",
];

impl Shortcut {
    /// Parses "ctrl-alt-shift-cmd-k" style text; modifiers in any order.
    pub fn parse(text: &str) -> Option<Self> {
        let mut shortcut = Self {
            control: false,
            option: false,
            shift: false,
            command: false,
            key: String::new(),
        };
        let text = text.trim();
        // "cmd--" names the minus key, which is also the separator.
        let (modifiers, key) = match text.strip_suffix("--") {
            Some(rest) => (rest, "-"),
            None => match text.rsplit_once('-') {
                Some((modifiers, key)) => (modifiers, key),
                None => ("", text),
            },
        };
        for modifier in modifiers.split('-').filter(|part| !part.is_empty()) {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => shortcut.control = true,
                "alt" | "opt" | "option" => shortcut.option = true,
                "shift" => shortcut.shift = true,
                "cmd" | "command" | "super" | "win" => shortcut.command = true,
                _ => return None,
            }
        }
        if key.is_empty() {
            return None;
        }
        shortcut.key = key.to_ascii_lowercase();
        Some(shortcut)
    }

    /// Canonical text for the settings file, modifiers in macOS order.
    pub fn to_config(&self) -> String {
        let mut text = String::new();
        for (on, name) in [
            (self.control, "ctrl-"),
            (self.option, "alt-"),
            (self.shift, "shift-"),
            (self.command, "cmd-"),
        ] {
            if on {
                text.push_str(name);
            }
        }
        text.push_str(&self.key);
        text
    }

    /// One symbol per keycap, as macOS menus show them: ⌃ ⌥ ⇧ ⌘ then the key.
    pub fn symbols(&self) -> Vec<String> {
        let mut symbols: Vec<String> = [
            (self.control, if PC_KEYS { "Ctrl" } else { "⌃" }),
            (self.option, if PC_KEYS { "Alt" } else { "⌥" }),
            (self.shift, if PC_KEYS { "Shift" } else { "⇧" }),
            (
                self.command,
                if cfg!(windows) {
                    "Win"
                } else if PC_KEYS {
                    "Super"
                } else {
                    "⌘"
                },
            ),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, symbol)| (*symbol).to_string())
        .collect();
        symbols.push(key_symbol(&self.key));
        symbols
    }

    pub fn is_function_key(&self) -> bool {
        function_key_number(&self.key).is_some()
    }

    pub fn validate(&self) -> Result<(), Problem> {
        if key_code(&self.key).is_none() {
            return Err(Problem::UnsupportedKey);
        }
        if !(self.command || self.option || self.control || self.is_function_key()) {
            return Err(Problem::NeedsModifier);
        }
        if RESERVED.contains(&self.to_config().as_str()) {
            return Err(Problem::ReservedBySystem);
        }
        Ok(())
    }

    /// The virtual key code Carbon registers.
    pub fn key_code(&self) -> Option<u32> {
        key_code(&self.key)
    }

    /// Carbon's modifier mask.
    #[cfg(target_os = "macos")]
    pub fn carbon_modifiers(&self) -> u32 {
        const CMD: u32 = 1 << 8;
        const SHIFT: u32 = 1 << 9;
        const OPTION: u32 = 1 << 11;
        const CONTROL: u32 = 1 << 12;
        [
            (self.command, CMD),
            (self.shift, SHIFT),
            (self.option, OPTION),
            (self.control, CONTROL),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, bit)| bit)
        .sum()
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.symbols().join(if PC_KEYS { "+" } else { "" }))
    }
}

fn function_key_number(key: &str) -> Option<u32> {
    let number: u32 = key.strip_prefix('f')?.parse().ok()?;
    (1..=20).contains(&number).then_some(number)
}

fn key_symbol(key: &str) -> String {
    match key {
        "space" => "Space".into(),
        "enter" => "↩".into(),
        "tab" => "⇥".into(),
        "backspace" => "⌫".into(),
        "delete" => "⌦".into(),
        "escape" => "⎋".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "home" => "↖".into(),
        "end" => "↘".into(),
        "pageup" => "⇞".into(),
        "pagedown" => "⇟".into(),
        key if function_key_number(key).is_some() => key.to_ascii_uppercase(),
        key => key.to_uppercase(),
    }
}

/// Virtual key codes (kVK_*) for the keys GPUI names.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn key_code(key: &str) -> Option<u32> {
    let code = match key {
        "a" => 0x00,
        "s" => 0x01,
        "d" => 0x02,
        "f" => 0x03,
        "h" => 0x04,
        "g" => 0x05,
        "z" => 0x06,
        "x" => 0x07,
        "c" => 0x08,
        "v" => 0x09,
        "b" => 0x0B,
        "q" => 0x0C,
        "w" => 0x0D,
        "e" => 0x0E,
        "r" => 0x0F,
        "y" => 0x10,
        "t" => 0x11,
        "1" => 0x12,
        "2" => 0x13,
        "3" => 0x14,
        "4" => 0x15,
        "6" => 0x16,
        "5" => 0x17,
        "=" => 0x18,
        "9" => 0x19,
        "7" => 0x1A,
        "-" => 0x1B,
        "8" => 0x1C,
        "0" => 0x1D,
        "]" => 0x1E,
        "o" => 0x1F,
        "u" => 0x20,
        "[" => 0x21,
        "i" => 0x22,
        "p" => 0x23,
        "enter" => 0x24,
        "l" => 0x25,
        "j" => 0x26,
        "'" => 0x27,
        "k" => 0x28,
        ";" => 0x29,
        "\\" => 0x2A,
        "," => 0x2B,
        "/" => 0x2C,
        "n" => 0x2D,
        "m" => 0x2E,
        "." => 0x2F,
        "tab" => 0x30,
        "space" => 0x31,
        "`" => 0x32,
        "backspace" => 0x33,
        "escape" => 0x35,
        "f17" => 0x40,
        "f18" => 0x4F,
        "f19" => 0x50,
        "f20" => 0x5A,
        "f5" => 0x60,
        "f6" => 0x61,
        "f7" => 0x62,
        "f3" => 0x63,
        "f8" => 0x64,
        "f9" => 0x65,
        "f11" => 0x67,
        "f13" => 0x69,
        "f16" => 0x6A,
        "f14" => 0x6B,
        "f10" => 0x6D,
        "f12" => 0x6F,
        "f15" => 0x71,
        "home" => 0x73,
        "pageup" => 0x74,
        "delete" => 0x75,
        "f4" => 0x76,
        "end" => 0x77,
        "f2" => 0x78,
        "pagedown" => 0x79,
        "f1" => 0x7A,
        "left" => 0x7B,
        "right" => 0x7C,
        "down" => 0x7D,
        "up" => 0x7E,
        _ => return None,
    };
    Some(code)
}

/// X keysyms: the key server-side grabs resolve through the keymap.
#[cfg(any(target_os = "linux", test))]
fn x11_keysym(key: &str) -> Option<u32> {
    if let Some(number) = function_key_number(key) {
        return Some(0xffbe + number - 1);
    }
    if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() {
        // Latin-1 keysyms equal their code points; letters use lowercase.
        return Some(u32::from(key.as_bytes()[0].to_ascii_lowercase()));
    }
    Some(match key {
        "space" => 0x0020,
        "backspace" => 0xff08,
        "tab" => 0xff09,
        "enter" => 0xff0d,
        "escape" => 0xff1b,
        "home" => 0xff50,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "pageup" => 0xff55,
        "pagedown" => 0xff56,
        "end" => 0xff57,
        "delete" => 0xffff,
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
fn key_code(key: &str) -> Option<u32> {
    x11_keysym(key)
}

// RegisterHotKey uses virtual keys, not Carbon's physical key codes.
#[cfg(target_os = "windows")]
fn key_code(key: &str) -> Option<u32> {
    windows_key_code(key)
}

#[cfg(any(target_os = "windows", test))]
fn windows_key_code(key: &str) -> Option<u32> {
    if let Some(number) = function_key_number(key) {
        return Some(0x70 + number - 1);
    }
    if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(u32::from(key.as_bytes()[0].to_ascii_uppercase()));
    }
    Some(match key {
        "backspace" => 0x08,
        "tab" => 0x09,
        "enter" => 0x0d,
        "escape" => 0x1b,
        "space" => 0x20,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "delete" => 0x2e,
        ";" => 0xba,
        "=" => 0xbb,
        "," => 0xbc,
        "-" => 0xbd,
        "." => 0xbe,
        "/" => 0xbf,
        "`" => 0xc0,
        "[" => 0xdb,
        "\\" => 0xdc,
        "]" => 0xdd,
        "'" => 0xde,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(text: &str) -> Shortcut {
        Shortcut::parse(text).unwrap()
    }

    #[test]
    fn windows_virtual_keys_and_aliases() {
        assert_eq!(windows_key_code("v"), Some(0x56));
        assert_eq!(windows_key_code("f1"), Some(0x70));
        assert_eq!(windows_key_code("f20"), Some(0x83));
        assert_eq!(windows_key_code("left"), Some(0x25));
        assert_eq!(windows_key_code("-"), Some(0xbd));
        assert_eq!(windows_key_code("§"), None);
        assert_eq!(shortcut("win-k"), shortcut("cmd-k"));
    }

    #[test]
    fn x11_keysyms() {
        assert_eq!(x11_keysym("v"), Some(0x76));
        assert_eq!(x11_keysym("f1"), Some(0xffbe));
        assert_eq!(x11_keysym("f12"), Some(0xffc9));
        assert_eq!(x11_keysym("space"), Some(0x20));
        assert_eq!(x11_keysym("left"), Some(0xff51));
        assert_eq!(x11_keysym("-"), Some(0x2d));
        assert_eq!(x11_keysym("§"), None);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_shortcuts_use_native_labels_and_reservations() {
        assert_eq!(shortcut("ctrl-alt-v").to_string(), "Ctrl+Alt+V");
        assert_eq!(shortcut("win-k").symbols(), ["Win", "K"]);
        assert_eq!(shortcut("ctrl-alt-v").validate(), Ok(()));
        assert_eq!(shortcut("win-v").validate(), Err(Problem::ReservedBySystem));
        assert_eq!(
            shortcut("alt-f4").validate(),
            Err(Problem::ReservedBySystem)
        );
    }

    #[test]
    fn parses_in_any_order_and_writes_canonically() {
        let parsed = shortcut("cmd-ctrl-V");
        assert!(parsed.control && parsed.command && !parsed.option && !parsed.shift);
        assert_eq!(parsed.key, "v");
        assert_eq!(parsed.to_config(), "ctrl-cmd-v");
        assert_eq!(shortcut("shift-opt-cmd-f5").to_config(), "alt-shift-cmd-f5");
        assert_eq!(shortcut("cmd--").key, "-");
        assert_eq!(Shortcut::parse("hyper-k"), None);
        assert_eq!(Shortcut::parse(""), None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_shortcuts_use_pc_labels_and_reservations() {
        assert_eq!(shortcut("ctrl-alt-v").to_string(), "Ctrl+Alt+V");
        assert_eq!(shortcut("super-k").symbols(), ["Super", "K"]);
        assert_eq!(shortcut("ctrl-alt-v").validate(), Ok(()));
        assert_eq!(
            shortcut("alt-f4").validate(),
            Err(Problem::ReservedBySystem)
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn shows_macos_symbols_in_menu_order() {
        assert_eq!(shortcut("cmd-alt-ctrl-shift-k").to_string(), "⌃⌥⇧⌘K");
        assert_eq!(shortcut("f5").symbols(), ["F5"]);
        assert_eq!(shortcut("alt-space").to_string(), "⌥Space");
        assert_eq!(shortcut("cmd-left").to_string(), "⌘←");
    }

    #[test]
    fn needs_a_modifier_unless_it_is_a_function_key() {
        assert_eq!(shortcut("k").validate(), Err(Problem::NeedsModifier));
        assert_eq!(shortcut("shift-k").validate(), Err(Problem::NeedsModifier));
        assert_eq!(shortcut("f6").validate(), Ok(()));
        assert_eq!(shortcut("ctrl-cmd-v").validate(), Ok(()));
        assert_eq!(
            shortcut(if PC_KEYS { "cmd-v" } else { "cmd-space" }).validate(),
            Err(Problem::ReservedBySystem)
        );
        assert_eq!(shortcut("cmd-§").validate(), Err(Problem::UnsupportedKey));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn maps_to_carbon_codes_and_masks() {
        let s = shortcut("ctrl-cmd-v");
        assert_eq!(s.key_code(), Some(0x09));
        assert_eq!(s.carbon_modifiers(), (1 << 12) | (1 << 8));
        assert_eq!(shortcut("f1").key_code(), Some(0x7A));
        assert_eq!(
            shortcut("alt-shift-9").carbon_modifiers(),
            (1 << 11) | (1 << 9)
        );
    }
}
