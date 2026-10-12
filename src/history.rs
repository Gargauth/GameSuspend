//! Reads gamesuspend.log back as readable history entries for the history window.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub time: String,
    pub event: String,
    pub program: String,
    pub result: String,
}

/// Newest entries first, at most `limit` of them.
pub fn load(path: &Path, limit: usize) -> Vec<Entry> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .rev()
        .filter_map(parse_line)
        .take(limit)
        .collect()
}

/// `time \t ACTION \t PID=n \t EXE=name \t result`, as written by `log::write`.
fn parse_line(line: &str) -> Option<Entry> {
    let line = line.trim_start_matches('\u{feff}');
    let mut fields = line.splitn(5, '\t');
    let time = fields.next()?;
    let action = fields.next()?;
    let _pid = fields.next()?.strip_prefix("PID=")?;
    let exe = fields.next()?.strip_prefix("EXE=")?;
    let result = fields.next().unwrap_or("");
    Some(Entry {
        time: time.to_string(),
        event: event_label(action).to_string(),
        program: if exe == "-" {
            String::new()
        } else {
            exe.to_string()
        },
        result: result.to_string(),
    })
}

fn event_label(action: &str) -> &str {
    match action {
        "SUSPEND" => "Suspend",
        "RESUME" => "Resume",
        "EMERGENCY" => "Emergency resume",
        "EXIT-RESUME" => "Resume on exit",
        "START" => "Start",
        "SETTINGS" | "RELOAD" => "Settings",
        "PANIC" => "App error",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_lines_into_readable_entries() {
        let entry = parse_line(
            "\u{feff}2026-08-16 12:13:36\tSUSPEND\tPID=14348\tEXE=notepad.exe\tOK (window: normal)",
        );
        assert_eq!(
            entry,
            Some(Entry {
                time: "2026-08-16 12:13:36".into(),
                event: "Suspend".into(),
                program: "notepad.exe".into(),
                result: "OK (window: normal)".into(),
            })
        );
        let entry = parse_line("2026-08-16 12:13:36\tRELOAD\tPID=0\tEXE=-\tOK").unwrap();
        assert_eq!(
            (entry.event.as_str(), entry.program.as_str()),
            ("Settings", "")
        );
    }

    #[test]
    fn skips_malformed_lines() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("garbage"), None);
        assert_eq!(parse_line("t\tSUSPEND\tpid\tEXE=x\tOK"), None);
    }

    #[test]
    fn loads_newest_first_with_limit() {
        let dir = std::env::temp_dir().join(format!("gamesuspend-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gamesuspend.log");
        let lines: String = (1..=5)
            .map(|i| format!("2026-01-01 00:00:0{i}\tSUSPEND\tPID={i}\tEXE=g.exe\tOK\n"))
            .collect();
        std::fs::write(&path, format!("{lines}broken line\n")).unwrap();

        let times: Vec<String> = load(&path, 3).into_iter().map(|e| e.time).collect();
        assert_eq!(
            times,
            [
                "2026-01-01 00:00:05",
                "2026-01-01 00:00:04",
                "2026-01-01 00:00:03"
            ]
        );
        assert!(load(&dir.join("missing.log"), 3).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
