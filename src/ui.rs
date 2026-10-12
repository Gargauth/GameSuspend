//! Settings and history windows: modeless dialogs laid out in assets/dialogs.rc.
//!
//! While the settings window is open, the global hotkeys are unregistered so the
//! hotkey fields can capture them; closing or saving registers them again.

use std::cell::{Cell, RefCell};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, CheckDlgButton, EM_LIMITTEXT, EM_SETCUEBANNER, ICC_LISTVIEW_CLASSES,
    ICC_STANDARD_CLASSES, ICC_UPDOWN_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx,
    IsDlgButtonChecked, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS,
    LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMTEXTW, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_EX_LABELTIP,
    LVSCW_AUTOSIZE_USEHEADER, UDM_SETRANGE32,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN,
    VK_MENU, VK_RWIN, VK_SHIFT, VK_TAB,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateDialogParamW, DLGC_HASSETSEL, DLGC_WANTALLKEYS, DestroyWindow, GetDlgCtrlID, GetDlgItem,
    GetDlgItemInt, GetDlgItemTextW, GetWindowTextLengthW, ICON_BIG, ICON_SMALL, IDCANCEL, IDOK,
    IsDialogMessageW, MSG, MapDialogRect, SW_RESTORE, SW_SHOW, SendDlgItemMessageW, SendMessageW,
    SetDlgItemInt, SetDlgItemTextW, SetForegroundWindow, SetWindowTextW, ShowWindow, WM_CHAR,
    WM_CLEAR, WM_COMMAND, WM_CONTEXTMENU, WM_CUT, WM_DESTROY, WM_GETDLGCODE, WM_INITDIALOG,
    WM_KEYDOWN, WM_KEYUP, WM_PASTE, WM_SETICON, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_UNDO,
};

use crate::config::{self, Config, MAX_DELAY_MS};
use crate::hotkey::{self, Hotkey};
use crate::ids::*;
use crate::{autostart, history, log, wide, window, with_app};

/// Rows shown in the history window.
const HISTORY_LIMIT: usize = 500;

thread_local! {
    static SETTINGS: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
    static HISTORY: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
    static SETTINGS_STATE: RefCell<Option<SettingsState>> = const { RefCell::new(None) };
}

/// The settings window's starting values and the hotkeys captured so far.
struct SettingsState {
    config: Config,
    toggle: Option<Hotkey>,
    emergency: Option<Hotkey>,
}

pub fn init() {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES | ICC_LISTVIEW_CLASSES | ICC_UPDOWN_CLASS,
    };
    unsafe { InitCommonControlsEx(&icc) };
}

/// Lets open dialogs handle Tab, Enter and Esc. Call for every queued message.
pub fn is_dialog_message(msg: &MSG) -> bool {
    [&SETTINGS, &HISTORY].into_iter().any(|key| {
        let dlg = key.with(Cell::get);
        !dlg.is_null() && unsafe { IsDialogMessageW(dlg, msg) } != 0
    })
}

pub fn open_settings() {
    if bring_to_front(&SETTINGS) {
        return;
    }
    let Some(config) = with_app(|app| {
        app.pause_hotkeys();
        app.config().clone()
    }) else {
        return;
    };
    let state = SettingsState {
        toggle: hotkey::parse(&config.toggle_key).ok(),
        emergency: hotkey::parse(&config.emergency_key).ok(),
        config,
    };
    SETTINGS_STATE.with(|s| *s.borrow_mut() = Some(state));
    open_dialog(&SETTINGS, IDD_SETTINGS, settings_proc);
}

pub fn open_history() {
    if !bring_to_front(&HISTORY) {
        open_dialog(&HISTORY, IDD_HISTORY, history_proc);
    }
}

type DialogProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> isize;

fn open_dialog(slot: &'static std::thread::LocalKey<Cell<HWND>>, id: u16, proc: DialogProc) {
    let dlg = unsafe {
        CreateDialogParamW(
            GetModuleHandleW(std::ptr::null()),
            id as usize as *const u16,
            std::ptr::null_mut(),
            Some(proc),
            0,
        )
    };
    if dlg.is_null() {
        return;
    }
    slot.with(|s| s.set(dlg));
    unsafe {
        ShowWindow(dlg, SW_SHOW);
        SetForegroundWindow(dlg);
    }
}

fn bring_to_front(slot: &'static std::thread::LocalKey<Cell<HWND>>) -> bool {
    let dlg = slot.with(Cell::get);
    if dlg.is_null() {
        return false;
    }
    unsafe {
        ShowWindow(dlg, SW_RESTORE);
        SetForegroundWindow(dlg);
    }
    true
}

fn set_title_and_icon(dlg: HWND, title: &str) {
    let title = wide(&format!(
        "GameSuspend {} – {title}",
        env!("CARGO_PKG_VERSION")
    ));
    unsafe {
        SetWindowTextW(dlg, title.as_ptr());
        SendMessageW(
            dlg,
            WM_SETICON,
            ICON_BIG as WPARAM,
            window::load_icon(IDI_APP, false) as LPARAM,
        );
        SendMessageW(
            dlg,
            WM_SETICON,
            ICON_SMALL as WPARAM,
            window::load_icon(IDI_APP, true) as LPARAM,
        );
    }
}

fn send(dlg: HWND, id: i32, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    unsafe { SendDlgItemMessageW(dlg, id, msg, wparam, lparam) }
}

fn set_text(dlg: HWND, id: i32, text: &str) {
    unsafe { SetDlgItemTextW(dlg, id, wide(text).as_ptr()) };
}

fn get_text(dlg: HWND, id: i32) -> String {
    let len = unsafe { GetWindowTextLengthW(GetDlgItem(dlg, id)) }.max(0) as usize;
    let mut buf = vec![0u16; len + 1];
    let read = unsafe { GetDlgItemTextW(dlg, id, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..read as usize])
}

fn set_checked(dlg: HWND, id: i32, checked: bool) {
    unsafe { CheckDlgButton(dlg, id, u32::from(checked)) };
}

fn is_checked(dlg: HWND, id: i32) -> bool {
    unsafe { IsDlgButtonChecked(dlg, id) == BST_CHECKED }
}

fn command_parts(wparam: WPARAM) -> (i32, u32) {
    ((wparam & 0xFFFF) as i32, ((wparam >> 16) & 0xFFFF) as u32)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

unsafe extern "system" fn settings_proc(dlg: HWND, msg: u32, wparam: WPARAM, _: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            init_settings(dlg);
            1
        }
        WM_COMMAND => {
            match command_parts(wparam).0 {
                IDOK => save_settings(dlg),
                IDCANCEL => close_settings(dlg),
                IDC_SHOW_HISTORY => open_history(),
                _ => return 0,
            }
            1
        }
        WM_DESTROY => {
            SETTINGS.with(|s| s.set(std::ptr::null_mut()));
            SETTINGS_STATE.with(|s| s.borrow_mut().take());
            0
        }
        _ => 0,
    }
}

fn init_settings(dlg: HWND) {
    set_title_and_icon(dlg, "Settings");
    SETTINGS_STATE.with(|s| {
        let state = s.borrow();
        let Some(state) = state.as_ref() else { return };
        let cfg = &state.config;

        for (id, key) in [
            (IDC_HK_TOGGLE, state.toggle),
            (IDC_HK_EMERGENCY, state.emergency),
        ] {
            unsafe { SetWindowSubclass(GetDlgItem(dlg, id), Some(hotkey_field_proc), 1, 0) };
            set_text(dlg, id, &key.map(Hotkey::display).unwrap_or_default());
        }

        set_checked(dlg, IDC_MINIMIZE, cfg.minimize_window);
        set_checked(dlg, IDC_NOTIFY, cfg.show_tray_tips);
        set_checked(dlg, IDC_AUTOSTART, autostart::is_enabled());

        set_text(dlg, IDC_BLACKLIST, &cfg.extra_blacklist.join(", "));
        let cue = wide("e.g. discord.exe, obs64.exe");
        send(
            dlg,
            IDC_BLACKLIST,
            EM_SETCUEBANNER,
            1,
            cue.as_ptr() as LPARAM,
        );

        for (edit, spin, value) in [
            (IDC_MIN_DELAY, IDC_MIN_DELAY_SPIN, cfg.minimize_delay_ms),
            (IDC_RESUME_DELAY, IDC_RESUME_DELAY_SPIN, cfg.resume_delay_ms),
        ] {
            send(dlg, spin, UDM_SETRANGE32, 0, MAX_DELAY_MS as LPARAM);
            send(dlg, edit, EM_LIMITTEXT, 5, 0);
            unsafe { SetDlgItemInt(dlg, edit, value, 0) };
        }
    });
}

/// Hotkey field: an edit control that shows the pressed key combination
/// instead of typed text.
unsafe extern "system" fn hotkey_field_proc(
    field: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    match msg {
        WM_GETDLGCODE => {
            // Take every key, including Esc and Enter, except Tab to leave the field.
            let pressed = lparam as *const MSG;
            let tab = !pressed.is_null()
                && unsafe {
                    (*pressed).message == WM_KEYDOWN && (*pressed).wParam == VK_TAB as WPARAM
                };
            let default = unsafe { DefSubclassProc(field, msg, wparam, lparam) };
            if tab {
                default
            } else {
                // No DLGC_HASSETSEL: the dialog must not highlight the text on focus.
                (default & !(DLGC_HASSETSEL as LRESULT)) | DLGC_WANTALLKEYS as LRESULT
            }
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            capture_key(field, wparam as u32);
            0
        }
        WM_KEYUP | WM_SYSKEYUP => {
            // Released modifiers without a key: show the stored hotkey again.
            if held_modifiers() == 0 {
                show_stored_hotkey(field);
            }
            0
        }
        WM_CHAR | WM_SYSCHAR | WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO | WM_CONTEXTMENU => 0,
        _ => unsafe { DefSubclassProc(field, msg, wparam, lparam) },
    }
}

fn held_modifiers() -> u32 {
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    let mut modifiers = 0;
    if down(VK_CONTROL) {
        modifiers |= MOD_CONTROL;
    }
    if down(VK_MENU) {
        modifiers |= MOD_ALT;
    }
    if down(VK_SHIFT) {
        modifiers |= MOD_SHIFT;
    }
    if down(VK_LWIN) || down(VK_RWIN) {
        modifiers |= MOD_WIN;
    }
    modifiers
}

fn capture_key(field: HWND, vk: u32) {
    let modifiers = held_modifiers();
    let text = match Hotkey::captured(modifiers, vk) {
        Some(key) => {
            let id = unsafe { GetDlgCtrlID(field) };
            SETTINGS_STATE.with(|s| {
                if let Some(state) = s.borrow_mut().as_mut() {
                    match id {
                        IDC_HK_TOGGLE => state.toggle = Some(key),
                        _ => state.emergency = Some(key),
                    }
                }
            });
            key.display()
        }
        None => format!("{}…", hotkey::modifiers_display(modifiers)),
    };
    unsafe { SetWindowTextW(field, wide(&text).as_ptr()) };
}

fn show_stored_hotkey(field: HWND) {
    let id = unsafe { GetDlgCtrlID(field) };
    let key = SETTINGS_STATE.with(|s| {
        let state = s.borrow();
        let state = state.as_ref()?;
        if id == IDC_HK_TOGGLE {
            state.toggle
        } else {
            state.emergency
        }
    });
    let text = key.map(Hotkey::display).unwrap_or_default();
    unsafe { SetWindowTextW(field, wide(&text).as_ptr()) };
}

fn read_delay(dlg: HWND, id: i32) -> u32 {
    let value = unsafe { GetDlgItemInt(dlg, id, std::ptr::null_mut(), 0) };
    value.min(MAX_DELAY_MS)
}

fn read_settings(dlg: HWND) -> Result<Config, String> {
    SETTINGS_STATE.with(|s| {
        let state = s.borrow();
        let state = state.as_ref().ok_or("The settings window is not ready.")?;
        let toggle = state
            .toggle
            .ok_or("Pick a hotkey to suspend and resume the game.")?;
        let emergency = state
            .emergency
            .ok_or("Pick a hotkey for emergency resume.")?;
        if toggle == emergency {
            return Err("The suspend hotkey and the emergency hotkey must be different.".into());
        }
        Ok(Config {
            toggle_key: toggle.to_spec(),
            emergency_key: emergency.to_spec(),
            minimize_delay_ms: read_delay(dlg, IDC_MIN_DELAY),
            resume_delay_ms: read_delay(dlg, IDC_RESUME_DELAY),
            minimize_window: is_checked(dlg, IDC_MINIMIZE),
            show_tray_tips: is_checked(dlg, IDC_NOTIFY),
            extra_blacklist: config::parse_blacklist(&get_text(dlg, IDC_BLACKLIST)),
        })
    })
}

fn save_settings(dlg: HWND) {
    let result = read_settings(dlg).and_then(|config| {
        let autostart = is_checked(dlg, IDC_AUTOSTART);
        with_app(|app| app.apply_settings(config, autostart))
            .unwrap_or_else(|| Err("The app is busy, please try again.".into()))
    });
    match result {
        Ok(()) => unsafe {
            DestroyWindow(dlg);
        },
        Err(message) => window::message_box(dlg, &message, true),
    }
}

fn close_settings(dlg: HWND) {
    if let Some(Err(message)) = with_app(|app| app.resume_hotkeys()) {
        window::message_box(dlg, &message, false);
    }
    unsafe { DestroyWindow(dlg) };
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

unsafe extern "system" fn history_proc(dlg: HWND, msg: u32, wparam: WPARAM, _: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            set_title_and_icon(dlg, "History");
            init_history_list(dlg);
            fill_history(dlg);
            1
        }
        WM_COMMAND => {
            match command_parts(wparam).0 {
                IDCANCEL | IDOK => unsafe {
                    DestroyWindow(dlg);
                },
                IDC_OPEN_LOG => window::open_file(&log::PATH),
                IDC_CLEAR_LOG => {
                    if window::ask_yes_no(dlg, "Delete the whole history?") {
                        let _ = std::fs::remove_file(&*log::PATH);
                        fill_history(dlg);
                    }
                }
                _ => return 0,
            }
            1
        }
        WM_DESTROY => {
            HISTORY.with(|s| s.set(std::ptr::null_mut()));
            0
        }
        _ => 0,
    }
}

fn init_history_list(dlg: HWND) {
    let ex = (LVS_EX_FULLROWSELECT | LVS_EX_LABELTIP | LVS_EX_DOUBLEBUFFER) as LPARAM;
    send(
        dlg,
        IDC_HISTORY_LIST,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        ex as WPARAM,
        ex,
    );
    // Column widths in dialog units, so they scale with font and DPI.
    let columns = [("Time", 80), ("Event", 80), ("Program", 80), ("Result", 0)];
    for (i, (title, width_dlu)) in columns.into_iter().enumerate() {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: width_dlu,
            bottom: 0,
        };
        unsafe { MapDialogRect(dlg, &mut rect) };
        let mut title = wide(title);
        let mut column: LVCOLUMNW = unsafe { std::mem::zeroed() };
        column.mask = LVCF_TEXT | LVCF_WIDTH;
        column.cx = rect.right;
        column.pszText = title.as_mut_ptr();
        send(
            dlg,
            IDC_HISTORY_LIST,
            LVM_INSERTCOLUMNW,
            i,
            &column as *const _ as LPARAM,
        );
    }
}

fn fill_history(dlg: HWND) {
    send(dlg, IDC_HISTORY_LIST, LVM_DELETEALLITEMS, 0, 0);
    let entries = history::load(&log::PATH, HISTORY_LIMIT);
    for (row, entry) in entries.iter().enumerate() {
        let cells = [&entry.time, &entry.event, &entry.program, &entry.result];
        for (col, text) in cells.into_iter().enumerate() {
            let mut text = wide(text);
            let mut item: LVITEMW = unsafe { std::mem::zeroed() };
            item.mask = LVIF_TEXT;
            item.iItem = row as i32;
            item.iSubItem = col as i32;
            item.pszText = text.as_mut_ptr();
            let msg = if col == 0 {
                LVM_INSERTITEMW
            } else {
                LVM_SETITEMTEXTW
            };
            send(dlg, IDC_HISTORY_LIST, msg, row, &item as *const _ as LPARAM);
        }
    }
    // The last column takes the remaining width.
    send(
        dlg,
        IDC_HISTORY_LIST,
        LVM_SETCOLUMNWIDTH,
        3,
        LVSCW_AUTOSIZE_USEHEADER as LPARAM,
    );
    unsafe {
        EnableWindow(
            GetDlgItem(dlg, IDC_CLEAR_LOG),
            i32::from(!entries.is_empty()),
        )
    };
}
