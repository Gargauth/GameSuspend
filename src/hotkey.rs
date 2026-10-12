//! AutoHotkey-style hotkey strings (`^!Pause`, `#F9`, `+vk13`) to `RegisterHotKey` arguments.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VkKeyScanW,
};

const VK_CANCEL: u32 = 0x03;
const VK_PAUSE: u32 = 0x13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    /// `MOD_*` flags.
    pub modifiers: u32,
    pub vk: u32,
    /// Second virtual key to register for the same physical key. With Ctrl held,
    /// the Pause key arrives as VK_CANCEL (Ctrl+Break), so `^Pause` needs both.
    pub alt_vk: Option<u32>,
}

pub fn parse(spec: &str) -> Result<Hotkey, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("zkratka je prázdná".into());
    }
    if spec.contains(" & ") {
        return Err("kombinace dvou kláves přes & nejsou podporované".into());
    }
    if spec.to_lowercase().ends_with(" up") {
        return Err("zkratky na uvolnění klávesy (Up) nejsou podporované".into());
    }

    let mut modifiers = 0;
    let mut rest = spec;
    // A single remaining character is always the key itself, so "^+" means Ctrl and "+".
    while rest.chars().count() > 1 {
        let Some(c) = rest.chars().next() else { break };
        match c {
            '^' => modifiers |= MOD_CONTROL,
            '!' => modifiers |= MOD_ALT,
            '+' => modifiers |= MOD_SHIFT,
            '#' => modifiers |= MOD_WIN,
            // Left/right variants, wildcard, pass-through and hook prefixes have no
            // RegisterHotKey equivalent; the hotkey still works for either side.
            '<' | '>' | '*' | '~' | '$' => {}
            _ => break,
        }
        rest = &rest[c.len_utf8()..];
    }

    let vk = key_code(rest).ok_or_else(|| format!("neznámá klávesa „{rest}“"))?;
    let alt_vk = (vk == VK_PAUSE && modifiers & MOD_CONTROL != 0).then_some(VK_CANCEL);
    Ok(Hotkey {
        modifiers,
        vk,
        alt_vk,
    })
}

fn key_code(name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return char_key(c);
    }
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
        .find(|(n, _)| *n == lower)
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

const NAMED_KEYS: &[(&str, u32)] = &[
    ("pause", VK_PAUSE),
    ("ctrlbreak", VK_CANCEL),
    ("scrolllock", 0x91),
    ("capslock", 0x14),
    ("numlock", 0x90),
    ("printscreen", 0x2C),
    ("insert", 0x2D),
    ("ins", 0x2D),
    ("delete", 0x2E),
    ("del", 0x2E),
    ("home", 0x24),
    ("end", 0x23),
    ("pgup", 0x21),
    ("pgdn", 0x22),
    ("up", 0x26),
    ("down", 0x28),
    ("left", 0x25),
    ("right", 0x27),
    ("space", 0x20),
    ("tab", 0x09),
    ("enter", 0x0D),
    ("escape", 0x1B),
    ("esc", 0x1B),
    ("backspace", 0x08),
    ("bs", 0x08),
    ("appskey", 0x5D),
    ("sleep", 0x5F),
    ("help", 0x2F),
    ("numpaddot", 0x6E),
    ("numpaddiv", 0x6F),
    ("numpadmult", 0x6A),
    ("numpadadd", 0x6B),
    ("numpadsub", 0x6D),
    ("numpadenter", 0x0D),
    ("browser_back", 0xA6),
    ("browser_forward", 0xA7),
    ("browser_refresh", 0xA8),
    ("browser_stop", 0xA9),
    ("browser_search", 0xAA),
    ("browser_favorites", 0xAB),
    ("browser_home", 0xAC),
    ("volume_mute", 0xAD),
    ("volume_down", 0xAE),
    ("volume_up", 0xAF),
    ("media_next", 0xB0),
    ("media_prev", 0xB1),
    ("media_stop", 0xB2),
    ("media_play_pause", 0xB3),
    ("launch_mail", 0xB4),
    ("launch_media", 0xB5),
    ("launch_app1", 0xB6),
    ("launch_app2", 0xB7),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(modifiers: u32, vk: u32) -> Hotkey {
        Hotkey {
            modifiers,
            vk,
            alt_vk: None,
        }
    }

    #[test]
    fn plain_pause_registers_only_pause() {
        assert_eq!(parse("Pause"), Ok(hk(0, VK_PAUSE)));
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
}
