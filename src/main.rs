//! GameSuspend: suspends and resumes the foreground game with one hotkey.
//!
//! A hidden window receives hotkeys and tray events; all work happens on its
//! thread. Suspend/resume uses ntdll!NtSuspendProcess / NtResumeProcess.

#![windows_subsystem = "windows"]

mod app;
mod autostart;
mod config;
mod history;
mod hotkey;
mod ids;
mod log;
mod process;
mod tray;
mod ui;
mod window;

use std::cell::RefCell;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ENDSESSION_LOGOFF,
    FindWindowW, GetMessageW, GetWindowThreadProcessId, MSG, PM_REMOVE, PeekMessageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, TranslateMessage, WM_APP, WM_CLOSE,
    WM_CONTEXTMENU, WM_DESTROY, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_QUERYENDSESSION,
    WM_RBUTTONUP, WNDCLASSEXW, WS_EX_TOOLWINDOW,
};

use app::{App, Effect, SUSPENDED_PID};
use process::NtOp;

/// Directory of the executable; settings.ini and the log live here (portable app).
pub static APP_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
});

const WINDOW_CLASS: &str = "GameSuspend.HiddenWindow";
/// Sent by a newly started instance: the running one resumes its game and exits.
const WM_REPLACE: u32 = WM_APP + 2;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// NUL-terminated UTF-16 for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Runs `f` on the app state. Returns `None` before startup finished or when
/// re-entered from a nested message loop (menu, message box); such events are
/// dropped, the same way a busy hotkey ignores further presses.
pub(crate) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

fn main() {
    install_panic_hook();
    replace_running_instance();
    ui::init();

    let hwnd = create_window();
    let mut app = App::new(hwnd);
    let effect = app.start();
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    run_effect(hwnd, effect);

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        if ui::is_dialog_message(&msg) {
            continue;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Runs follow-up work outside of `with_app`, since opening a window re-enters it.
fn run_effect(hwnd: HWND, effect: Effect) {
    match effect {
        Effect::None => {}
        Effect::Settings => ui::open_settings(),
        Effect::History => ui::open_history(),
        Effect::Exit => exit(hwnd, "Menu"),
    }
}

/// A panic must not leave the game frozen: resume it before the process aborts.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let pid = SUSPENDED_PID.swap(0, Ordering::SeqCst);
        if pid != 0 {
            let _ = process::nt_call(pid, NtOp::Resume);
        }
        log::write("PANIC", pid, "", &info.to_string());
    }));
}

/// Single instance: the newest start wins, like AutoHotkey's `#SingleInstance Force`.
fn replace_running_instance() {
    let class = wide(WINDOW_CLASS);
    let old = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if old.is_null() {
        return;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(old, &mut pid) };
    let process = process::open(pid, PROCESS_SYNCHRONIZE);
    unsafe { PostMessageW(old, WM_REPLACE, 0, 0) };
    if let Ok(process) = process {
        unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) };
    }
}

fn create_window() -> HWND {
    let class = wide(WINDOW_CLASS);
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        RegisterClassExW(&wc);

        TASKBAR_CREATED.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            Ordering::Relaxed,
        );

        let title = wide("GameSuspend");
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            window::message_box(std::ptr::null_mut(), "Can't create the app window.", true);
            std::process::exit(1);
        }
        hwnd
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            with_app(|app| app.on_hotkey(wparam as i32));
            discard_queued_hotkeys(hwnd);
            0
        }
        tray::WM_TRAY => {
            match (lparam as u32) & 0xFFFF {
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                WM_LBUTTONDBLCLK => {
                    if let Some(effect) = with_app(App::on_tray_double_click) {
                        run_effect(hwnd, effect);
                    }
                }
                _ => {}
            }
            0
        }
        WM_REPLACE => {
            exit(hwnd, "Single");
            0
        }
        WM_CLOSE => {
            exit(hwnd, "Close");
            0
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wparam != 0 {
                let logoff = (lparam as u32) & ENDSESSION_LOGOFF != 0;
                shutdown(if logoff { "Logoff" } else { "Shutdown" });
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            // Explorer restarted: the tray icon is gone and must be added again.
            with_app(|app| app.update_tray());
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Presses that arrived while an action was running (it sleeps between the
/// window and process steps) are dropped instead of toggling straight back.
fn discard_queued_hotkeys(hwnd: HWND) {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { PeekMessageW(&mut msg, hwnd, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) } != 0 {}
}

fn show_menu(hwnd: HWND) {
    let Some(items) = with_app(|app| app.menu_items()) else {
        return;
    };
    let id = tray::popup_menu(hwnd, &items);
    if id == 0 {
        return;
    }
    if let Some(effect) = with_app(|app| app.on_menu(id)) {
        run_effect(hwnd, effect);
    }
}

fn exit(hwnd: HWND, reason: &str) {
    shutdown(reason);
    unsafe { DestroyWindow(hwnd) };
}

/// Resumes the suspended game before exiting. Falls back to the bare PID when
/// the app state is unavailable (re-entrant call), so the game is never left frozen.
fn shutdown(reason: &str) {
    if with_app(|app| app.shutdown(reason)).is_some() {
        return;
    }
    let pid = SUSPENDED_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        let result = match process::nt_call(pid, NtOp::Resume) {
            Ok(()) => format!("OK (exit reason: {reason})"),
            Err(err) => format!("ERROR: {err} (exit reason: {reason})"),
        };
        log::write("EXIT-RESUME", pid, "", &result);
    }
}
