//! Window helpers: foreground window queries, minimize/restore, message boxes,
//! icons and opening files.
//!
//! Window state changes use `ShowWindowAsync`: a synchronous `ShowWindow` on a
//! window of another process waits until that process handles the message, which
//! a suspended or hung game never does.

use std::path::Path;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, GetWindowTextW, GetWindowThreadProcessId, HICON, IDYES,
    IMAGE_ICON, IsIconic, IsWindow, IsZoomed, LR_DEFAULTCOLOR, LoadImageW, MB_DEFBUTTON2,
    MB_ICONERROR, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_YESNO,
    MESSAGEBOX_STYLE, MessageBoxW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SW_MAXIMIZE,
    SW_MINIMIZE, SW_RESTORE, SW_SHOWNORMAL, SetForegroundWindow, ShowWindowAsync,
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
            Placement::Minimized => "minimized",
            Placement::Normal => "normal",
            Placement::Maximized => "maximized",
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

pub fn message_box(owner: HWND, text: &str, error: bool) {
    let icon = if error { MB_ICONERROR } else { MB_ICONWARNING };
    show_message(owner, text, MB_OK | icon);
}

pub fn ask_yes_no(owner: HWND, text: &str) -> bool {
    show_message(owner, text, MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES
}

fn show_message(owner: HWND, text: &str, style: MESSAGEBOX_STYLE) -> i32 {
    let text = wide(text);
    let caption = wide("GameSuspend");
    unsafe {
        MessageBoxW(
            owner,
            text.as_ptr(),
            caption.as_ptr(),
            style | MB_SETFOREGROUND,
        )
    }
}

/// Icon resource at the system size for small (title bar, tray) or large icons.
pub fn load_icon(id: u16, small: bool) -> HICON {
    let (cx, cy) = if small {
        (SM_CXSMICON, SM_CYSMICON)
    } else {
        (SM_CXICON, SM_CYICON)
    };
    unsafe {
        LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            id as usize as *const u16,
            IMAGE_ICON,
            GetSystemMetrics(cx),
            GetSystemMetrics(cy),
            LR_DEFAULTCOLOR,
        )
    }
}

/// Opens a file in its default application (Notepad for .log).
pub fn open_file(path: &Path) {
    let verb = wide("open");
    let file = wide(&path.to_string_lossy());
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}
