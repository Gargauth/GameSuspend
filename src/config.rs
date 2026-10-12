//! `settings.ini` next to the executable: defaults, parsing, validation and saving.
//! The settings window writes it; manual edits are read on the next start.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

pub const MAX_DELAY_MS: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub toggle_key: String,
    pub emergency_key: String,
    pub minimize_delay_ms: u32,
    pub resume_delay_ms: u32,
    pub minimize_window: bool,
    pub show_tray_tips: bool,
    /// Lowercased executable names.
    pub extra_blacklist: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            toggle_key: "Pause".into(),
            emergency_key: "^!Pause".into(),
            minimize_delay_ms: 350,
            resume_delay_ms: 250,
            minimize_window: true,
            show_tray_tips: true,
            extra_blacklist: Vec::new(),
        }
    }
}

impl Config {
    /// Loads `path`, creating it with the defaults when missing.
    /// An unreadable file yields the defaults so the app always starts.
    pub fn load_or_create(path: &Path) -> Config {
        match std::fs::read(path) {
            Ok(bytes) => Config::parse(&decode(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                let _ = config.save(path);
                config
            }
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_ini())
    }

    pub fn to_ini(&self) -> String {
        let mut ini = String::from(
            "; GameSuspend settings. Easier to change in the Settings window\r\n\
             ; (right-click the GameSuspend icon next to the clock).\r\n\
             ; Hotkeys: ^ = Ctrl, ! = Alt, + = Shift, # = Win, e.g. ^!Pause.\r\n",
        );
        let flag = |b: bool| u8::from(b);
        let _ = write!(
            ini,
            "\r\n[Hotkeys]\r\nToggleKey={}\r\nEmergencyKey={}\r\n\
             \r\n[Timing]\r\nMinimizeDelayMs={}\r\nResumeDelayMs={}\r\n\
             \r\n[Behavior]\r\nMinimizeWindow={}\r\nShowTrayTips={}\r\n\
             \r\n[Blacklist]\r\nExtra={}\r\n",
            self.toggle_key,
            self.emergency_key,
            self.minimize_delay_ms,
            self.resume_delay_ms,
            flag(self.minimize_window),
            flag(self.show_tray_tips),
            self.extra_blacklist.join(", "),
        );
        ini
    }

    /// Missing or invalid values fall back to their defaults individually.
    pub fn parse(text: &str) -> Config {
        let ini = Ini::parse(text);
        let def = Config::default();
        let text_or = |section, key, default: String| match ini.get(section, key) {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => default,
        };
        let delay_or = |section, key, default| {
            ini.get(section, key)
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|&v| v <= MAX_DELAY_MS)
                .unwrap_or(default)
        };
        let flag_or = |section, key, default| {
            ini.get(section, key)
                .and_then(|v| v.parse::<i64>().ok())
                .map_or(default, |v| v != 0)
        };

        Config {
            toggle_key: text_or("hotkeys", "togglekey", def.toggle_key),
            emergency_key: text_or("hotkeys", "emergencykey", def.emergency_key),
            minimize_delay_ms: delay_or("timing", "minimizedelayms", def.minimize_delay_ms),
            resume_delay_ms: delay_or("timing", "resumedelayms", def.resume_delay_ms),
            minimize_window: flag_or("behavior", "minimizewindow", def.minimize_window),
            show_tray_tips: flag_or("behavior", "showtraytips", def.show_tray_tips),
            extra_blacklist: parse_blacklist(ini.get("blacklist", "extra").unwrap_or("")),
        }
    }
}

/// Program names separated by commas or semicolons, lowercased and deduplicated.
/// A name without an extension gets `.exe`, so "discord" works as expected.
pub fn parse_blacklist(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in text.split([',', ';']) {
        let mut name = name.trim().to_lowercase();
        if name.is_empty() {
            continue;
        }
        if !name.contains('.') {
            name.push_str(".exe");
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Accepts UTF-8 (with or without BOM) and UTF-16 LE with BOM, which is what
/// Notepad produces for "Unicode".
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Minimal INI reader with `GetPrivateProfileString` semantics: case-insensitive
/// section and key names, trimmed values, first occurrence wins, `;`/`#` comments.
struct Ini(HashMap<(String, String), String>);

impl Ini {
    fn parse(text: &str) -> Ini {
        let mut map = HashMap::new();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_lowercase();
            } else if let Some((key, value)) = line.split_once('=') {
                map.entry((section.clone(), key.trim().to_lowercase()))
                    .or_insert_with(|| value.trim().to_string());
            }
        }
        Ini(map)
    }

    fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.0
            .get(&(section.to_string(), key.to_string()))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_ini_parses_back_to_the_same_config() {
        let custom = Config {
            toggle_key: "^F9".into(),
            emergency_key: "+#vkBA".into(),
            minimize_delay_ms: 0,
            resume_delay_ms: 10_000,
            minimize_window: false,
            show_tray_tips: false,
            extra_blacklist: vec!["discord.exe".into(), "obs64.exe".into()],
        };
        for config in [Config::default(), custom] {
            assert_eq!(Config::parse(&config.to_ini()), config);
        }
    }

    #[test]
    fn reads_values_case_insensitively_and_trimmed() {
        let cfg = Config::parse(
            "[HOTKEYS]\nTOGGLEKEY =  F9 \n[timing]\nminimizedelayms=0\nResumeDelayMs=10000\n\
             [Behavior]\nMinimizeWindow=0\nShowTrayTips=0\n",
        );
        assert_eq!(cfg.toggle_key, "F9");
        assert_eq!(cfg.emergency_key, "^!Pause");
        assert_eq!(cfg.minimize_delay_ms, 0);
        assert_eq!(cfg.resume_delay_ms, 10_000);
        assert!(!cfg.minimize_window);
        assert!(!cfg.show_tray_tips);
    }

    #[test]
    fn invalid_or_out_of_range_values_fall_back_to_defaults() {
        let cfg = Config::parse(
            "[Hotkeys]\nToggleKey=\n[Timing]\nMinimizeDelayMs=-5\nResumeDelayMs=10001\n\
             [Behavior]\nMinimizeWindow=yes\n",
        );
        assert_eq!(cfg.toggle_key, "Pause");
        assert_eq!(cfg.minimize_delay_ms, 350);
        assert_eq!(cfg.resume_delay_ms, 250);
        assert!(cfg.minimize_window);
    }

    #[test]
    fn keys_outside_their_section_are_ignored_and_first_occurrence_wins() {
        let cfg = Config::parse("ToggleKey=F1\n[Hotkeys]\nToggleKey=F2\nToggleKey=F3\n");
        assert_eq!(cfg.toggle_key, "F2");
    }

    #[test]
    fn blacklist_is_split_normalized_and_deduplicated() {
        assert_eq!(
            parse_blacklist(" Discord.exe ,, OBS64.EXE; steam ,discord.EXE,"),
            ["discord.exe", "obs64.exe", "steam.exe"]
        );
    }

    #[test]
    fn decodes_utf8_bom_and_utf16le() {
        let text = "[Hotkeys]\r\nToggleKey=F8\r\n";
        let mut utf8 = vec![0xEF, 0xBB, 0xBF];
        utf8.extend_from_slice(text.as_bytes());
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        for bytes in [utf8, utf16] {
            assert_eq!(Config::parse(&decode(&bytes)).toggle_key, "F8");
        }
    }
}
