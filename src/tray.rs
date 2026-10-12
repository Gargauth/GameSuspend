//! Notification-area icon, balloon notifications and the context menu.

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_LARGE_ICON, NIIF_USER, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetSystemMetrics, HICON, IMAGE_ICON,
    LR_DEFAULTCOLOR, LoadImageW, MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW, SM_CXICON,
    SM_CXSMICON, SM_CYICON, SM_CYSMICON, SYSTEM_METRICS_INDEX, SetForegroundWindow,
    SetMenuDefaultItem, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP,
    WM_NULL,
};

use crate::wide;

/// Resource IDs assigned in build.rs.
const ICON_IDLE: u16 = 1;
const ICON_SUSPENDED: u16 = 2;

/// Callback message for mouse events on the tray icon.
pub const WM_TRAY: u32 = WM_APP + 1;

pub struct Tray {
    hwnd: HWND,
    idle: Icons,
    suspended: Icons,
}

/// Small for the notification area, large for notifications.
struct Icons {
    small: HICON,
    large: HICON,
}

impl Icons {
    fn load(id: u16) -> Icons {
        Icons {
            small: load_icon(id, SM_CXSMICON, SM_CYSMICON),
            large: load_icon(id, SM_CXICON, SM_CYICON),
        }
    }
}

pub struct MenuItem {
    /// 0 = separator.
    pub id: u32,
    pub text: String,
    pub enabled: bool,
    pub default: bool,
}

impl MenuItem {
    pub fn separator() -> MenuItem {
        MenuItem {
            id: 0,
            text: String::new(),
            enabled: false,
            default: false,
        }
    }

    pub fn new(id: u32, text: impl Into<String>) -> MenuItem {
        MenuItem {
            id,
            text: text.into(),
            enabled: true,
            default: false,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> MenuItem {
        self.enabled = enabled;
        self
    }

    pub fn default_item(mut self, default: bool) -> MenuItem {
        self.default = default;
        self
    }
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        Tray {
            hwnd,
            idle: Icons::load(ICON_IDLE),
            suspended: Icons::load(ICON_SUSPENDED),
        }
    }

    fn icons(&self, suspended: bool) -> &Icons {
        if suspended {
            &self.suspended
        } else {
            &self.idle
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = 1;
        data
    }

    /// Adds the icon or updates it. Adding again after a failed modify also
    /// recovers the icon when Explorer restarts or starts after this app.
    pub fn show(&self, suspended: bool, tip: &str) {
        let mut data = self.data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.icons(suspended).small;
        copy_wide(&mut data.szTip, tip);
        unsafe {
            if Shell_NotifyIconW(NIM_MODIFY, &data) == 0 {
                Shell_NotifyIconW(NIM_ADD, &data);
            }
        }
    }

    /// Shows a notification carrying the icon of the given state.
    pub fn notify(&self, title: &str, text: &str, suspended: bool) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
        data.hBalloonIcon = self.icons(suspended).large;
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, text);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    pub fn remove(&self) {
        unsafe { Shell_NotifyIconW(NIM_DELETE, &self.data()) };
    }
}

fn load_icon(id: u16, cx: SYSTEM_METRICS_INDEX, cy: SYSTEM_METRICS_INDEX) -> HICON {
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

/// Copies `text` into a fixed, NUL-terminated UTF-16 buffer, truncating if needed.
fn copy_wide(dst: &mut [u16], text: &str) {
    let max = dst.len() - 1;
    let mut len = 0;
    for (slot, unit) in dst[..max].iter_mut().zip(text.encode_utf16()) {
        *slot = unit;
        len += 1;
    }
    dst[len] = 0;
}

/// Shows the menu at the cursor and returns the chosen item ID, 0 when dismissed.
pub fn popup_menu(hwnd: HWND, items: &[MenuItem]) -> u32 {
    unsafe {
        let menu = CreatePopupMenu();
        for item in items {
            if item.id == 0 {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                continue;
            }
            let flags = if item.enabled {
                MF_STRING
            } else {
                MF_STRING | MF_GRAYED
            };
            AppendMenuW(menu, flags, item.id as usize, wide(&item.text).as_ptr());
            if item.default {
                SetMenuDefaultItem(menu, item.id, 0);
            }
        }
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Without these two calls the menu does not close when clicking elsewhere.
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        cmd as u32
    }
}
