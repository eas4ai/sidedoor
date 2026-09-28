//! Global keyboard shortcuts as the user sees and stores them.
//!
//! Stored like GPUI keystrokes ("ctrl-cmd-v"), shown the macOS way (⌃⌘V),
//! and registered by virtual key code, which follows the key's position on
//! an ANSI keyboard.

use std::fmt;

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
    ReservedByMacOS,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NeedsModifier => "Add ⌘, ⌥ or ⌃, or use an F-key on its own.",
            Self::UnsupportedKey => "That key can't be used in a shortcut.",
            Self::ReservedByMacOS => "macOS already uses this shortcut.",
        })
    }
}

/// Shortcuts macOS keeps for itself or that every app relies on.
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
                "cmd" | "command" | "super" => shortcut.command = true,
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
            (self.control, "⌃"),
            (self.option, "⌥"),
            (self.shift, "⇧"),
            (self.command, "⌘"),
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
            return Err(Problem::ReservedByMacOS);
        }
        Ok(())
    }

    /// The virtual key code Carbon registers.
    pub fn key_code(&self) -> Option<u32> {
        key_code(&self.key)
    }

    /// Carbon's modifier mask.
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
        f.write_str(&self.symbols().concat())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(text: &str) -> Shortcut {
        Shortcut::parse(text).unwrap()
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
            shortcut("cmd-space").validate(),
            Err(Problem::ReservedByMacOS)
        );
        assert_eq!(shortcut("cmd-§").validate(), Err(Problem::UnsupportedKey));
    }

    #[test]
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
