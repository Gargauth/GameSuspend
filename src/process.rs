//! Process suspend/resume via ntdll, process lookup and the safety blacklist.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_SUSPEND_RESUME,
};

#[link(name = "ntdll.dll", kind = "raw-dylib", modifiers = "+verbatim")]
unsafe extern "system" {
    fn NtSuspendProcess(process: HANDLE) -> i32;
    fn NtResumeProcess(process: HANDLE) -> i32;
}

/// Never suspended, regardless of settings.ini. explorer.exe freezes the desktop
/// and taskbar, dwm.exe the whole screen, csrss/winlogon/lsass mean a BSOD or logoff.
const HARD_BLACKLIST: &[&str] = &[
    "explorer.exe",
    "dwm.exe",
    "csrss.exe",
    "winlogon.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "smss.exe",
    "system",
    "registry",
    "memory compression",
    "sihost.exe",
    "ctfmon.exe",
    "taskmgr.exe",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtOp {
    Suspend,
    Resume,
}

impl NtOp {
    fn name(self) -> &'static str {
        match self {
            NtOp::Suspend => "NtSuspendProcess",
            NtOp::Resume => "NtResumeProcess",
        }
    }
}

/// Opens `pid`; the handle closes on drop, so no error path can leak it.
pub fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Result<OwnedHandle, u32> {
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        Err(unsafe { GetLastError() })
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

/// Suspends or resumes every thread of `pid`. Needs only PROCESS_SUSPEND_RESUME.
pub fn nt_call(pid: u32, op: NtOp) -> Result<(), String> {
    if pid == 0 {
        return Err("neplatný PID (0)".into());
    }
    let process = open(pid, PROCESS_SUSPEND_RESUME)
        .map_err(|e| format!("OpenProcess selhalo, WinErr {e} ({})", win_error_text(e)))?;
    let handle = process.as_raw_handle();
    let status = unsafe {
        match op {
            NtOp::Suspend => NtSuspendProcess(handle),
            NtOp::Resume => NtResumeProcess(handle),
        }
    };
    if status != 0 {
        return Err(format!(
            "{} vrátilo NTSTATUS 0x{:08X}",
            op.name(),
            status as u32
        ));
    }
    Ok(())
}

fn win_error_text(code: u32) -> &'static str {
    match code {
        5 => "přístup odepřen - chybí práva správce nebo to blokuje anticheat",
        6 => "neplatný handle",
        87 => "neplatný parametr - proces už pravděpodobně neexistuje",
        299 => "částečně dokončeno",
        _ => "neznámá chyba",
    }
}

/// Executable name of a running process, `None` when no such process exists.
/// Uses a Toolhelp snapshot, which needs no access rights to the process itself.
pub fn exe_name(pid: u32) -> Option<String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut ok = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
    while ok != 0 {
        if entry.th32ProcessID == pid {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
            return Some(String::from_utf16_lossy(&entry.szExeFile[..len]));
        }
        ok = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
    }
    None
}

pub fn exists(pid: u32) -> bool {
    exe_name(pid).is_some()
}

pub fn current_pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

/// Why `pid` must not be suspended, or `None` when it is allowed.
pub fn block_reason(pid: u32, own_pid: u32, exe: &str, extra: &[String]) -> Option<&'static str> {
    if pid == own_pid {
        return Some("to je GameSuspend sám");
    }
    if pid <= 4 {
        return Some("systémový proces");
    }
    let exe = exe.to_lowercase();
    if HARD_BLACKLIST.contains(&exe.as_str()) {
        return Some("systémový proces na pevném blacklistu");
    }
    // A suspended process holding a low-level keyboard hook (AutoHotkey) stalls
    // keyboard input for the whole system.
    if exe.contains("autohotkey") {
        return Some("AutoHotkey proces");
    }
    if extra.contains(&exe) {
        return Some("proces na uživatelském blacklistu v settings.ini");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::time::Duration;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const OWN: u32 = 1000;

    #[test]
    fn blocks_self_low_pids_and_system_processes() {
        assert!(block_reason(OWN, OWN, "game.exe", &[]).is_some());
        assert!(block_reason(4, OWN, "System", &[]).is_some());
        for exe in ["explorer.exe", "DWM.EXE", "csrss.exe", "Memory Compression"] {
            assert!(block_reason(500, OWN, exe, &[]).is_some(), "{exe}");
        }
    }

    #[test]
    fn blocks_any_autohotkey_process() {
        assert!(block_reason(500, OWN, "AutoHotkey64_UIA.exe", &[]).is_some());
    }

    #[test]
    fn user_blacklist_extends_hard_blacklist() {
        let extra = vec!["discord.exe".to_string()];
        assert!(block_reason(500, OWN, "Discord.exe", &extra).is_some());
        assert_eq!(block_reason(500, OWN, "ACValhalla.exe", &extra), None);
        assert_eq!(block_reason(500, OWN, "notepad.exe", &[]), None);
    }

    #[test]
    fn rejects_invalid_and_missing_pids() {
        assert!(nt_call(0, NtOp::Suspend).is_err());
        let err = nt_call(0xFFFF_FFF0, NtOp::Suspend).unwrap_err();
        assert!(err.starts_with("OpenProcess selhalo"), "{err}");
        assert_eq!(exe_name(0xFFFF_FFF0), None);
    }

    /// Busy loop in cmd.exe: burns a full core until killed.
    struct BusyChild(Child);

    impl BusyChild {
        fn spawn() -> BusyChild {
            let child = Command::new("cmd.exe")
                .raw_arg("/d /c for /l %i in (0,0,1) do @rem")
                .spawn()
                .expect("spawn cmd.exe");
            BusyChild(child)
        }

        fn pid(&self) -> u32 {
            self.0.id()
        }

        /// User + kernel CPU time in milliseconds.
        fn cpu_ms(&self) -> u64 {
            let p = open(self.pid(), PROCESS_QUERY_LIMITED_INFORMATION).unwrap();
            let mut t = [FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            }; 4];
            let [created, exited, kernel, user] = &mut t;
            let h = p.as_raw_handle();
            assert_ne!(
                unsafe { GetProcessTimes(h, created, exited, kernel, user) },
                0
            );
            let ft =
                |f: &FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
            (ft(kernel) + ft(user)) / 10_000
        }

        fn cpu_delta_ms(&self, wait: Duration) -> u64 {
            let before = self.cpu_ms();
            std::thread::sleep(wait);
            self.cpu_ms() - before
        }
    }

    impl Drop for BusyChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn suspend_stops_cpu_time_and_resume_restarts_it() {
        let child = BusyChild::spawn();
        let window = Duration::from_millis(800);
        assert_eq!(exe_name(child.pid()).as_deref(), Some("cmd.exe"));
        assert!(
            child.cpu_delta_ms(window) > 200,
            "busy child is not running"
        );

        nt_call(child.pid(), NtOp::Suspend).unwrap();
        assert!(
            child.cpu_delta_ms(window) < 50,
            "suspended child kept running"
        );

        nt_call(child.pid(), NtOp::Resume).unwrap();
        assert!(
            child.cpu_delta_ms(window) > 200,
            "resumed child did not run"
        );
    }
}
