//! Application state and actions: suspend, resume, emergency resume, settings, exit.
//!
//! CRITICAL ORDER OF OPERATIONS:
//!   suspend = minimize window -> wait -> suspend process
//!   resume  = resume process  -> wait -> restore window
//! A suspended process cannot handle window messages, so the window is
//! minimized while the game still runs and restored only after it runs again.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};

use crate::config::Config;
use crate::process::{self, NtOp};
use crate::tray::{MenuItem, Tray};
use crate::window::{self, Placement};
use crate::{autostart, hotkey, log};

/// PID of the currently suspended process, readable without the app state so
/// that the panic hook and exit fallbacks can always resume it.
pub static SUSPENDED_PID: AtomicU32 = AtomicU32::new(0);

// `*_ALT` = second registration of the same hotkey, see [`hotkey::Hotkey::alt_vk`].
const HK_TOGGLE: i32 = 1;
const HK_TOGGLE_ALT: i32 = 2;
const HK_EMERGENCY: i32 = 3;
const HK_EMERGENCY_ALT: i32 = 4;

const ID_STATUS: u32 = 1;
const ID_TOGGLE: u32 = 2;
const ID_EMERGENCY: u32 = 3;
const ID_SETTINGS: u32 = 4;
const ID_HISTORY: u32 = 5;
const ID_EXIT: u32 = 6;

struct Suspended {
    pid: u32,
    hwnd: HWND,
    exe: String,
    placement: Placement,
}

/// Follow-up work that must run after the app state is released (it opens windows).
pub enum Effect {
    None,
    Settings,
    History,
    Exit,
}

pub struct App {
    hwnd: HWND,
    ini: PathBuf,
    cfg: Config,
    first_run: bool,
    state: Option<Suspended>,
    /// Last suspended process for the emergency hotkey; kept after a normal resume.
    last: Option<(u32, String)>,
    tray: Tray,
}

impl App {
    pub fn new(hwnd: HWND) -> App {
        let ini = crate::APP_DIR.join("settings.ini");
        App {
            hwnd,
            first_run: !ini.exists(),
            cfg: Config::load_or_create(&ini),
            ini,
            state: None,
            last: None,
            tray: Tray::new(hwnd),
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Registers hotkeys and shows the tray icon. Asks for the settings window
    /// on the first run and when a hotkey cannot be used.
    pub fn start(&mut self) -> Effect {
        let hotkeys = self.register_hotkeys(&self.cfg);
        self.update_tray();
        log::write(
            "START",
            process::current_pid(),
            "GameSuspend.exe",
            &format!(
                "OK (toggle={}, emergency={})",
                self.cfg.toggle_key, self.cfg.emergency_key
            ),
        );
        match &hotkeys {
            Ok(()) => self.notify(
                "GameSuspend is running",
                &format!(
                    "{} = suspend/resume the foreground game\n{} = emergency resume",
                    hotkey::display(&self.cfg.toggle_key),
                    hotkey::display(&self.cfg.emergency_key)
                ),
            ),
            Err(e) => window::message_box(
                std::ptr::null_mut(),
                &format!("Some hotkeys can't be used:\n\n{e}\n\nPick different ones in Settings."),
                false,
            ),
        }
        if self.first_run || hotkeys.is_err() {
            Effect::Settings
        } else {
            Effect::None
        }
    }

    /// Registers both hotkeys of `cfg`; the error lists every one that failed.
    fn register_hotkeys(&self, cfg: &Config) -> Result<(), String> {
        let errors: Vec<String> = [
            (
                "Suspend / resume",
                &cfg.toggle_key,
                HK_TOGGLE,
                HK_TOGGLE_ALT,
            ),
            (
                "Emergency resume",
                &cfg.emergency_key,
                HK_EMERGENCY,
                HK_EMERGENCY_ALT,
            ),
        ]
        .into_iter()
        .filter_map(|(what, spec, id, alt_id)| {
            self.register_hotkey(id, alt_id, spec)
                .err()
                .map(|e| format!("{what} ({}): {e}", hotkey::display(spec)))
        })
        .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    fn register_hotkey(&self, id: i32, alt_id: i32, spec: &str) -> Result<(), String> {
        let key = hotkey::parse(spec)?;
        let modifiers = key.modifiers | MOD_NOREPEAT;
        if unsafe { RegisterHotKey(self.hwnd, id, modifiers, key.vk) } == 0 {
            return Err("another app already uses this hotkey".into());
        }
        if let Some(alt) = key.alt_vk {
            // Best effort: the primary registration already works.
            unsafe { RegisterHotKey(self.hwnd, alt_id, modifiers, alt) };
        }
        Ok(())
    }

    fn unregister_hotkeys(&self) {
        for id in [HK_TOGGLE, HK_TOGGLE_ALT, HK_EMERGENCY, HK_EMERGENCY_ALT] {
            unsafe { UnregisterHotKey(self.hwnd, id) };
        }
    }

    /// While the settings window is open its hotkey fields must receive the keys.
    pub fn pause_hotkeys(&self) {
        self.unregister_hotkeys();
    }

    pub fn resume_hotkeys(&self) -> Result<(), String> {
        self.register_hotkeys(&self.cfg)
            .map_err(|e| format!("Some hotkeys couldn't be turned back on:\n\n{e}"))
    }

    /// Applies and saves settings from the settings window. On error nothing
    /// changes and the hotkeys stay paused, so the window can stay open.
    pub fn apply_settings(&mut self, cfg: Config, autostart: bool) -> Result<(), String> {
        self.unregister_hotkeys();
        if let Err(e) = self.register_hotkeys(&cfg) {
            self.unregister_hotkeys();
            return Err(format!(
                "These hotkeys can't be used:\n\n{e}\n\nPick different ones."
            ));
        }
        let saved = cfg
            .save(&self.ini)
            .map_err(|e| format!("Can't save settings to {}:\n{e}", self.ini.display()));
        if let Err(e) = saved.and_then(|()| autostart::set(autostart)) {
            self.unregister_hotkeys();
            return Err(e);
        }
        self.cfg = cfg;
        self.update_tray();
        log::write(
            "SETTINGS",
            0,
            "",
            &format!(
                "OK (toggle={}, emergency={})",
                self.cfg.toggle_key, self.cfg.emergency_key
            ),
        );
        Ok(())
    }

    pub fn on_hotkey(&mut self, id: i32) {
        match id {
            HK_TOGGLE | HK_TOGGLE_ALT => self.toggle(),
            HK_EMERGENCY | HK_EMERGENCY_ALT => self.emergency(),
            _ => {}
        }
    }

    fn toggle(&mut self) {
        if self.state.is_some() {
            self.resume();
        } else {
            self.suspend();
        }
    }

    fn suspend(&mut self) {
        let Some(hwnd) = window::foreground() else {
            self.notify("Suspend", "No active window found.");
            log::write("SUSPEND", 0, "", "ERROR: no active window");
            return;
        };
        let pid = window::pid(hwnd);
        let Some(exe) = process::exe_name(pid) else {
            self.notify("Suspend", "Can't determine the window's process.");
            log::write(
                "SUSPEND",
                pid,
                "",
                "ERROR: can't determine the window's process",
            );
            return;
        };

        // Safety checks before anything touches the window.
        let own_pid = process::current_pid();
        if let Some(reason) = process::block_reason(pid, own_pid, &exe, &self.cfg.extra_blacklist) {
            self.notify("Suspend blocked", &format!("{exe}\n{reason}"));
            log::write("SUSPEND", pid, &exe, &format!("BLOCKED: {reason}"));
            return;
        }

        let placement = Placement::of(hwnd);
        let title = window::title(hwnd);

        // Step 1: minimize while the process still handles window messages.
        if self.cfg.minimize_window {
            window::minimize(hwnd);
            sleep(Duration::from_millis(self.cfg.minimize_delay_ms.into()));
        }

        // Step 2: only now suspend.
        if let Err(err) = process::nt_call(pid, NtOp::Suspend) {
            // Never leave the user with a minimized window and nothing suspended.
            if self.cfg.minimize_window {
                window::restore(hwnd, placement);
            }
            self.notify(
                "Suspend failed",
                &format!(
                    "{exe}\n{err}\nThe window was restored. Try running GameSuspend as administrator."
                ),
            );
            log::write("SUSPEND", pid, &exe, &format!("ERROR: {err}"));
            return;
        }

        SUSPENDED_PID.store(pid, Ordering::SeqCst);
        self.last = Some((pid, exe.clone()));
        self.state = Some(Suspended {
            pid,
            hwnd,
            exe: exe.clone(),
            placement,
        });
        self.update_tray();
        log::write(
            "SUSPEND",
            pid,
            &exe,
            &format!("OK (window: {})", placement.label()),
        );
        self.notify(
            "Suspended",
            &format!(
                "{exe}\n{title}\n{} = resume",
                hotkey::display(&self.cfg.toggle_key)
            ),
        );
    }

    fn resume(&mut self) {
        let Some(st) = self.state.take() else { return };

        // Step 1: resume so the process can handle window messages again.
        if let Err(err) = process::nt_call(st.pid, NtOp::Resume) {
            log::write("RESUME", st.pid, &st.exe, &format!("ERROR: {err}"));
            self.notify("Resume failed", &format!("{}\n{err}", st.exe));
            if process::exists(st.pid) {
                self.state = Some(st);
            } else {
                // The process ended meanwhile; there is nothing left to hold.
                self.clear_suspended();
            }
            return;
        }
        self.clear_suspended();

        // Step 2: give threads time to restart the render loop, then the window.
        sleep(Duration::from_millis(self.cfg.resume_delay_ms.into()));
        if self.cfg.minimize_window {
            window::restore(st.hwnd, st.placement);
        }
        log::write(
            "RESUME",
            st.pid,
            &st.exe,
            &format!("OK (window: {})", st.placement.label()),
        );
        self.notify("Resumed", &st.exe);
    }

    /// Resumes the last known PID regardless of the internal state: the rescue
    /// when the state and reality diverge. Resuming a running process is a no-op.
    fn emergency(&mut self) {
        let target = match (&self.state, &self.last) {
            (Some(st), _) => Some((st.pid, st.exe.clone())),
            (None, last) => last.clone(),
        };
        let Some((pid, exe)) = target else {
            self.notify("Emergency resume", "There is no suspended game to resume.");
            log::write("EMERGENCY", 0, "", "ERROR: no remembered PID");
            return;
        };

        match process::nt_call(pid, NtOp::Resume) {
            Ok(()) => {
                log::write("EMERGENCY", pid, &exe, "OK");
                if let Some(st) = self.state.take() {
                    sleep(Duration::from_millis(self.cfg.resume_delay_ms.into()));
                    window::restore(st.hwnd, st.placement);
                }
                self.clear_suspended();
                self.notify("Resumed (emergency)", &exe);
            }
            Err(err) => {
                log::write("EMERGENCY", pid, &exe, &format!("ERROR: {err}"));
                self.notify("Emergency resume failed", &format!("{exe}\n{err}"));
                if !process::exists(pid) {
                    self.state = None;
                    self.clear_suspended();
                }
            }
        }
    }

    fn clear_suspended(&mut self) {
        SUSPENDED_PID.store(0, Ordering::SeqCst);
        self.update_tray();
    }

    /// Tray double-click: resume a suspended game, otherwise open the settings.
    /// Suspending from the tray would target the taskbar, the foreground window then.
    pub fn on_tray_double_click(&mut self) -> Effect {
        if self.state.is_some() {
            self.resume();
            Effect::None
        } else {
            Effect::Settings
        }
    }

    /// Always resumes a suspended process: otherwise it would stay frozen with
    /// nothing left to wake it up.
    pub fn shutdown(&mut self, reason: &str) {
        if let Some(st) = self.state.take() {
            let result = match process::nt_call(st.pid, NtOp::Resume) {
                Ok(()) => format!("OK (exit reason: {reason})"),
                Err(err) => format!("ERROR: {err} (exit reason: {reason})"),
            };
            SUSPENDED_PID.store(0, Ordering::SeqCst);
            log::write("EXIT-RESUME", st.pid, &st.exe, &result);
        }
        self.unregister_hotkeys();
        self.tray.remove();
    }

    pub fn update_tray(&self) {
        let tip = match &self.state {
            Some(st) => format!("GameSuspend – suspended: {}", st.exe),
            None => format!(
                "GameSuspend – {} suspends the foreground game",
                hotkey::display(&self.cfg.toggle_key)
            ),
        };
        self.tray.show(self.state.is_some(), &tip);
    }

    /// Windows already shows the app name and icon above the title.
    fn notify(&self, title: &str, text: &str) {
        if self.cfg.show_tray_tips {
            self.tray.notify(title, text, self.state.is_some());
        }
    }

    pub fn menu_items(&self) -> Vec<MenuItem> {
        let toggle_key = hotkey::display(&self.cfg.toggle_key);
        let suspended = self.state.is_some();
        let (status, toggle) = match &self.state {
            Some(st) => (
                format!("Suspended: {}", st.exe),
                MenuItem::new(ID_TOGGLE, format!("Resume game\t{toggle_key}")).default_item(true),
            ),
            None => (
                "Ready – press the hotkey in a game".to_string(),
                MenuItem::new(ID_TOGGLE, format!("Suspend foreground game\t{toggle_key}"))
                    .enabled(false),
            ),
        };
        vec![
            MenuItem::new(ID_STATUS, status).enabled(false),
            MenuItem::separator(),
            toggle,
            MenuItem::new(
                ID_EMERGENCY,
                format!(
                    "Emergency resume\t{}",
                    hotkey::display(&self.cfg.emergency_key)
                ),
            ),
            MenuItem::separator(),
            MenuItem::new(ID_SETTINGS, "Settings…").default_item(!suspended),
            MenuItem::new(ID_HISTORY, "History…"),
            MenuItem::separator(),
            MenuItem::new(ID_EXIT, "Exit"),
        ]
    }

    pub fn on_menu(&mut self, id: u32) -> Effect {
        match id {
            ID_TOGGLE => self.toggle(),
            ID_EMERGENCY => self.emergency(),
            ID_SETTINGS => return Effect::Settings,
            ID_HISTORY => return Effect::History,
            ID_EXIT => return Effect::Exit,
            _ => {}
        }
        Effect::None
    }
}
