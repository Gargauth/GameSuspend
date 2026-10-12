//! Hotkeys: the AutoHotkey-style strings stored in settings.ini (`^!Pause`, `#F9`,
//! `+vk13`), `RegisterHotKey` arguments, readable names (`Ctrl+Alt+Pause`) and
//! key presses captured by the settings window.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MAPVK_VK_TO_CHAR, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, MapVirtualKeyW, VkKeyScanW,
};

const VK_CANCEL: u32 = 0x03;
const VK_PAUSE: u32 = 0x13;

/// Shift, Ctrl, Alt (generic and left/right) and both Win keys.
const MODIFIER_KEYS: &[u32] = &[
    0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C,
];

/// Modifiers in display and storage order: (MOD_*, AutoHotkey prefix, name).
const MODIFIERS: [(u32, char, &str); 4] = [
    (MOD_CONTROL, '^', "Ctrl"),
    (MOD_ALT, '!', "Alt"),
    (MOD_SHIFT, '+', "Shift"),
    (MOD_WIN, '#', "Win"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    /// `MOD_*` flags.
    pub modifiers: u32,
    pub vk: u32,
    /// Second virtual key to register for the same physical key. With Ctrl held,
    /// the Pause key arrives as VK_CANCEL (Ctrl+Break), so `^Pause` needs both.
    pub alt_vk: Option<u32>,
}

impl Hotkey {
    pub fn new(modifiers: u32, vk: u32) -> Hotkey {
        let alt_vk = (vk == VK_PAUSE && modifiers & MOD_CONTROL != 0).then_some(VK_CANCEL);
        Hotkey {
            modifiers,
            vk,
            alt_vk,
        }
    }

    /// AutoHotkey-style string for settings.ini, e.g. `^!Pause`.
    pub fn to_spec(self) -> String {
        let mut spec: String = MODIFIERS
            .iter()
            .filter(|(m, _, _)| self.modifiers & m != 0)
            .map(|&(_, prefix, _)| prefix)
            .collect();
        spec.push_str(&key_name(self.vk));
        spec
    }

    /// Readable form for menus and notifications, e.g. `Ctrl+Alt+Pause`.
    pub fn display(self) -> String {
        let mut text = modifiers_display(self.modifiers);
        text.push_str(&display_key_name(self.vk));
        text
    }

    /// Hotkey from a key press with `modifiers` (`MOD_*`) held; `None` while
    /// only modifier keys are down.
    pub fn captured(modifiers: u32, vk: u32) -> Option<Hotkey> {
        if vk == 0 || MODIFIER_KEYS.contains(&vk) {
            return None;
        }
        // With Ctrl held, the Pause key arrives as Break; it is the same physical key.
        let vk = if vk == VK_CANCEL { VK_PAUSE } else { vk };
        Some(Hotkey::new(modifiers, vk))
    }
}

/// Held modifiers as a prefix, e.g. `Ctrl+Alt+`; empty when none.
pub fn modifiers_display(modifiers: u32) -> String {
    MODIFIERS
        .iter()
        .filter(|(m, _, _)| modifiers & m != 0)
        .map(|&(_, _, name)| format!("{name}+"))
        .collect()
}

/// Readable form of a stored hotkey; unparsable strings are shown as they are.
pub fn display(spec: &str) -> String {
    parse(spec).map_or_else(|_| spec.to_string(), |key| key.display())
}

pub fn parse(spec: &str) -> Result<Hotkey, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("the hotkey is empty".into());
    }
    if spec.contains(" & ") {
        return Err("two-key combinations with & are not supported".into());
    }
    if spec.to_lowercase().ends_with(" up") {
        return Err("key-release hotkeys (Up) are not supported".into());
    }

    let mut modifiers = 0;
    let mut rest = spec;
    // A single remaining character is always the key itself, so "^+" means Ctrl and "+".
    while rest.chars().count() > 1 {
        let Some(c) = rest.chars().next() else { break };
        match MODIFIERS.iter().find(|(_, prefix, _)| *prefix == c) {
            Some(&(m, _, _)) => modifiers |= m,
            // Left/right variants, wildcard, pass-through and hook prefixes have no
            // RegisterHotKey equivalent; the hotkey still works for either side.
            None if matches!(c, '<' | '>' | '*' | '~' | '$') => {}
            None => break,
        }
        rest = &rest[c.len_utf8()..];
    }

    let vk = key_code(rest).ok_or_else(|| format!("unknown key \"{rest}\""))?;
    Ok(Hotkey::new(modifiers, vk))
}

fn key_code(name: &str) -> Option<u32> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return char_key(c);
    }
    let lower = name.to_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        return (1..=24).contains(&n).then(|| 0x70 + n - 1);
    }
    if let Some(n) = lower
        .strip_prefix("numpad")
        .and_then(|n| n.parse::<u32>().ok())
    {
        return (n <= 9).then(|| 0x60 + n);
    }
    if let Some(hex) = lower.strip_prefix("vk") {
        return u32::from_str_radix(hex, 16)
            .ok()
            .filter(|vk| (1..=0xFE).contains(vk));
    }
    NAMED_KEYS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|&(_, vk)| vk)
}

fn char_key(c: char) -> Option<u32> {
    match c {
        'a'..='z' | 'A'..='Z' => Some(c.to_ascii_uppercase() as u32),
        '0'..='9' => Some(c as u32),
        _ => {
            let mut buf = [0u16; 2];
            let [unit] = c.encode_utf16(&mut buf) else {
                return None;
            };
            // Low byte = virtual key on the current layout, -1 = not on this layout.
            let scan = unsafe { VkKeyScanW(*unit) };
            (scan != -1).then_some((scan & 0xFF) as u32)
        }
    }
}

/// Layout-independent name that [`parse`] reads back.
fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        _ => NAMED_KEYS
            .iter()
            .find(|&&(_, v)| v == vk)
            .map_or_else(|| format!("vk{vk:02X}"), |(name, _)| name.to_string()),
    }
}

/// Like [`key_name`], but punctuation keys show their character on the current layout.
fn display_key_name(vk: u32) -> String {
    let name = key_name(vk);
    if !name.starts_with("vk") {
        return name;
    }
    let ch = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0xFFFF;
    char::from_u32(ch)
        .filter(|c| !c.is_control() && *c != '\0')
        .map_or(name, |c| c.to_uppercase().to_string())
}

/// Canonical names first: the first entry for a key is used when writing.
const NAMED_KEYS: &[(&str, u32)] = &[
    ("Pause", VK_PAUSE),
    ("CtrlBreak", VK_CANCEL),
    ("ScrollLock", 0x91),
    ("CapsLock", 0x14),
    ("NumLock", 0x90),
    ("PrintScreen", 0x2C),
    ("Insert", 0x2D),
    ("Ins", 0x2D),
    ("Delete", 0x2E),
    ("Del", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PgUp", 0x21),
    ("PgDn", 0x22),
    ("Up", 0x26),
    ("Down", 0x28),
    ("Left", 0x25),
    ("Right", 0x27),
    ("Space", 0x20),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Esc", 0x1B),
    ("Escape", 0x1B),
    ("Backspace", 0x08),
    ("BS", 0x08),
    ("AppsKey", 0x5D),
    ("Sleep", 0x5F),
    ("Help", 0x2F),
    ("NumpadDot", 0x6E),
    ("NumpadDiv", 0x6F),
    ("NumpadMult", 0x6A),
    ("NumpadAdd", 0x6B),
    ("NumpadSub", 0x6D),
    ("NumpadEnter", 0x0D),
    ("Browser_Back", 0xA6),
    ("Browser_Forward", 0xA7),
    ("Browser_Refresh", 0xA8),
    ("Browser_Stop", 0xA9),
    ("Browser_Search", 0xAA),
    ("Browser_Favorites", 0xAB),
    ("Browser_Home", 0xAC),
    ("Volume_Mute", 0xAD),
    ("Volume_Down", 0xAE),
    ("Volume_Up", 0xAF),
    ("Media_Next", 0xB0),
    ("Media_Prev", 0xB1),
    ("Media_Stop", 0xB2),
    ("Media_Play_Pause", 0xB3),
    ("Launch_Mail", 0xB4),
    ("Launch_Media", 0xB5),
    ("Launch_App1", 0xB6),
    ("Launch_App2", 0xB7),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(modifiers: u32, vk: u32) -> Hotkey {
        Hotkey::new(modifiers, vk)
    }

    #[test]
    fn plain_pause_registers_only_pause() {
        assert_eq!(parse("Pause").unwrap().alt_vk, None);
    }

    #[test]
    fn ctrl_pause_also_registers_ctrl_break() {
        let key = parse("^!Pause").unwrap();
        assert_eq!(key.modifiers, MOD_CONTROL | MOD_ALT);
        assert_eq!(key.vk, VK_PAUSE);
        assert_eq!(key.alt_vk, Some(VK_CANCEL));
    }

    #[test]
    fn all_modifiers_and_case_insensitive_names() {
        assert_eq!(
            parse("^!+#scrolllock"),
            Ok(hk(MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_WIN, 0x91))
        );
        assert_eq!(parse("  <^>!F12 "), Ok(hk(MOD_CONTROL | MOD_ALT, 0x7B)));
    }

    #[test]
    fn function_numpad_letter_digit_and_vk_keys() {
        assert_eq!(parse("F1"), Ok(hk(0, 0x70)));
        assert_eq!(parse("F24"), Ok(hk(0, 0x87)));
        assert_eq!(parse("Numpad7"), Ok(hk(0, 0x67)));
        assert_eq!(parse("#g"), Ok(hk(MOD_WIN, 0x47)));
        assert_eq!(parse("!5"), Ok(hk(MOD_ALT, 0x35)));
        assert_eq!(parse("vk13"), Ok(hk(0, 0x13)));
    }

    #[test]
    fn single_remaining_char_is_the_key_not_a_modifier() {
        let key = parse("^+").unwrap();
        assert_eq!(key.modifiers, MOD_CONTROL);
        assert_ne!(key.vk, 0);
    }

    #[test]
    fn rejects_unknown_and_unsupported_syntax() {
        for spec in [
            "", "  ", "F0", "F25", "Numpad10", "vk0", "vkFF", "Pauza", "a & b", "F1 up",
        ] {
            assert!(parse(spec).is_err(), "{spec:?} should be rejected");
        }
    }

    #[test]
    fn spec_round_trips_through_parse() {
        for spec in [
            "Pause", "^!Pause", "+#F9", "!Numpad0", "^Insert", "G", "7", "vkBA",
        ] {
            assert_eq!(parse(spec).unwrap().to_spec(), spec);
        }
        // Aliases are written in their canonical form.
        assert_eq!(parse("^del").unwrap().to_spec(), "^Delete");
    }

    #[test]
    fn display_is_readable() {
        assert_eq!(display("^!Pause"), "Ctrl+Alt+Pause");
        assert_eq!(display("+#F9"), "Shift+Win+F9");
        assert_eq!(display("nonsense key"), "nonsense key");
    }

    #[test]
    fn captured_key_presses() {
        // Modifier keys alone are not a hotkey yet.
        assert_eq!(Hotkey::captured(MOD_CONTROL, 0x11), None);
        assert_eq!(Hotkey::captured(MOD_WIN, 0x5B), None);
        assert_eq!(Hotkey::captured(0, 0x13), parse("Pause").ok());
        assert_eq!(
            Hotkey::captured(MOD_SHIFT | MOD_WIN, 0x78),
            parse("+#F9").ok()
        );
        // Ctrl+Pause arrives as Ctrl+Break.
        assert_eq!(
            Hotkey::captured(MOD_CONTROL | MOD_ALT, VK_CANCEL),
            parse("^!Pause").ok()
        );
    }

    #[test]
    fn modifiers_prefix() {
        assert_eq!(modifiers_display(MOD_ALT | MOD_CONTROL), "Ctrl+Alt+");
        assert_eq!(modifiers_display(0), "");
    }
}
