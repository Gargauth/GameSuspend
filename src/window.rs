//! Foreground window queries, minimize/restore and message boxes.
//!
//! Window state changes use `ShowWindowAsync`: a synchronous `ShowWindow` on a
//! window of another process waits until that process handles the message, which
//! a suspended or hung game never does.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsZoomed,
    MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MessageBoxW, SW_MAXIMIZE, SW_MINIMIZE,
    SW_RESTORE, SetForegroundWindow, ShowWindowAsync,
};

use crate::wide;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Minimized,
    Normal,
    Maximized,
}

impl Placement {
    pub fn of(hwnd: HWND) -> Placement {
        unsafe {
            if IsIconic(hwnd) != 0 {
                Placement::Minimized
            } else if IsZoomed(hwnd) != 0 {
                Placement::Maximized
            } else {
                Placement::Normal
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Placement::Minimized => "minimalizované",
            Placement::Normal => "normální",
            Placement::Maximized => "maximalizované",
        }
    }
}

pub fn foreground() -> Option<HWND> {
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_null()).then_some(hwnd)
}

pub fn pid(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

pub fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

pub fn minimize(hwnd: HWND) {
    unsafe { ShowWindowAsync(hwnd, SW_MINIMIZE) };
}

/// Returns the window to the state it had before suspending and activates it.
/// A window that was already minimized stays minimized.
pub fn restore(hwnd: HWND, placement: Placement) -> bool {
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return false;
    }
    let cmd = match placement {
        Placement::Minimized => return true,
        Placement::Maximized => SW_MAXIMIZE,
        Placement::Normal => SW_RESTORE,
    };
    unsafe {
        ShowWindowAsync(hwnd, cmd);
        SetForegroundWindow(hwnd);
    }
    true
}

pub fn message_box(text: &str, error: bool) {
    let icon = if error { MB_ICONERROR } else { MB_ICONWARNING };
    let text = wide(text);
    let caption = wide("GameSuspend");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | icon | MB_SETFOREGROUND,
        )
    };
}
