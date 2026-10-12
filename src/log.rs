//! Append-only `gamesuspend.log` next to the executable, one tab-separated line per action.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

use windows_sys::Win32::System::SystemInformation::GetLocalTime;

pub static PATH: LazyLock<PathBuf> = LazyLock::new(|| crate::APP_DIR.join("gamesuspend.log"));

/// Logging never fails the caller: a read-only directory just means no log.
pub fn write(action: &str, pid: u32, exe: &str, result: &str) {
    let exe = if exe.is_empty() { "-" } else { exe };
    let line = format!(
        "{}\t{action}\tPID={pid}\tEXE={exe}\t{result}\n",
        timestamp()
    );
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&*PATH) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn timestamp() -> String {
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}
