//! Application state and actions: suspend, resume, emergency resume, reload, exit.
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
use crate::{hotkey, log};

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
const ID_OPEN_LOG: u32 = 4;
const ID_OPEN_SETTINGS: u32 = 5;
const ID_RELOAD: u32 = 6;
const ID_EXIT: u32 = 7;

struct Suspended {
    pid: u32,
    hwnd: HWND,
    exe: String,
    placement: Placement,
}

pub enum MenuEffect {
    None,
    Open(PathBuf),
    Exit,
}

pub struct App {
    hwnd: HWND,
    ini: PathBuf,
    cfg: Config,
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
            cfg: Config::load_or_create(&ini),
            ini,
            state: None,
            last: None,
            tray: Tray::new(hwnd),
        }
    }

    /// Registers hotkeys and shows the tray icon. Fails only when the main
    /// hotkey cannot be registered, since the app is useless without it.
    pub fn start(&mut self) -> Result<(), String> {
        let (toggle, emergency) = self.register_hotkeys();
        if let Err(e) = toggle {
            return Err(format!(
                "Nelze zaregistrovat zkratku pro uspání „{}“:\n{e}\n\nOprav položku ToggleKey v settings.ini.",
                self.cfg.toggle_key
            ));
        }
        if let Err(e) = emergency {
            window::message_box(
                &format!(
                    "Nelze zaregistrovat nouzovou zkratku „{}“:\n{e}\n\nOprav položku EmergencyKey v settings.ini.",
                    self.cfg.emergency_key
                ),
                false,
            );
        }
        self.update_tray();
        log::write(
            "START",
            process::current_pid(),
            "GameSuspend.exe",
            &format!(
                "OK (toggle={}, nouzové={})",
                self.cfg.toggle_key, self.cfg.emergency_key
            ),
        );
        self.notify(
            "GameSuspend běží",
            &format!(
                "{} = uspat/probudit hru v popředí\n{} = nouzové probuzení",
                self.cfg.toggle_key, self.cfg.emergency_key
            ),
        );
        Ok(())
    }

    fn register_hotkeys(&self) -> (Result<(), String>, Result<(), String>) {
        (
            self.register_hotkey(HK_TOGGLE, HK_TOGGLE_ALT, &self.cfg.toggle_key),
            self.register_hotkey(HK_EMERGENCY, HK_EMERGENCY_ALT, &self.cfg.emergency_key),
        )
    }

    fn register_hotkey(&self, id: i32, alt_id: i32, spec: &str) -> Result<(), String> {
        let key = hotkey::parse(spec)?;
        let modifiers = key.modifiers | MOD_NOREPEAT;
        if unsafe { RegisterHotKey(self.hwnd, id, modifiers, key.vk) } == 0 {
            return Err("zkratku už používá jiná aplikace".into());
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
            self.notify("Uspání", "Nenalezeno žádné aktivní okno.");
            log::write("SUSPEND", 0, "", "CHYBA: žádné aktivní okno");
            return;
        };
        let pid = window::pid(hwnd);
        let Some(exe) = process::exe_name(pid) else {
            self.notify("Uspání", "Nelze zjistit proces okna.");
            log::write("SUSPEND", pid, "", "CHYBA: nelze zjistit proces okna");
            return;
        };

        // Safety checks before anything touches the window.
        let own_pid = process::current_pid();
        if let Some(reason) = process::block_reason(pid, own_pid, &exe, &self.cfg.extra_blacklist) {
            self.notify(
                "Uspání zablokováno",
                &format!("{exe} (PID {pid})\n{reason}"),
            );
            log::write("SUSPEND", pid, &exe, &format!("BLOKOVÁNO: {reason}"));
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
                "Uspání se nepovedlo",
                &format!(
                    "{exe} (PID {pid})\n{err}\nOkno bylo vráceno zpět.\nZkus spustit GameSuspend jako správce."
                ),
            );
            log::write("SUSPEND", pid, &exe, &format!("CHYBA: {err}"));
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
            &format!("OK (okno: {})", placement.label()),
        );
        self.notify(
            "Uspáno",
            &format!(
                "{exe} (PID {pid})\n{title}\n{} = probudit",
                self.cfg.toggle_key
            ),
        );
    }

    fn resume(&mut self) {
        let Some(st) = self.state.take() else { return };

        // Step 1: resume so the process can handle window messages again.
        if let Err(err) = process::nt_call(st.pid, NtOp::Resume) {
            log::write("RESUME", st.pid, &st.exe, &format!("CHYBA: {err}"));
            self.notify(
                "Probuzení selhalo",
                &format!("{} (PID {})\n{err}", st.exe, st.pid),
            );
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
            &format!("OK (okno: {})", st.placement.label()),
        );
        self.notify("Probuzeno", &format!("{} (PID {})", st.exe, st.pid));
    }

    /// Resumes the last known PID regardless of the internal state: the rescue
    /// when the state and reality diverge. Resuming a running process is a no-op.
    fn emergency(&mut self) {
        let target = match (&self.state, &self.last) {
            (Some(st), _) => Some((st.pid, st.exe.clone())),
            (None, last) => last.clone(),
        };
        let Some((pid, exe)) = target else {
            self.notify("Nouzové probuzení", "Není zapamatován žádný proces.");
            log::write("EMERGENCY", 0, "", "CHYBA: žádný zapamatovaný PID");
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
                self.notify("Nouzově probuzeno", &format!("{exe} (PID {pid})"));
            }
            Err(err) => {
                log::write("EMERGENCY", pid, &exe, &format!("CHYBA: {err}"));
                self.notify(
                    "Nouzové probuzení selhalo",
                    &format!("{exe} (PID {pid})\n{err}"),
                );
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

    /// Tray double-click: resume only. Suspending from the tray would target the
    /// taskbar, which is the foreground window at that moment.
    pub fn on_tray_double_click(&mut self) {
        if self.state.is_some() {
            self.resume();
        }
    }

    fn reload(&mut self) {
        if self.state.is_some() {
            self.notify("Nelze znovu načíst", "Nejdřív probuď uspaný proces.");
            return;
        }
        self.unregister_hotkeys();
        self.cfg = Config::load_or_create(&self.ini);
        let (toggle, emergency) = self.register_hotkeys();
        self.update_tray();

        let errors: Vec<String> = [
            ("ToggleKey", &self.cfg.toggle_key, toggle),
            ("EmergencyKey", &self.cfg.emergency_key, emergency),
        ]
        .into_iter()
        .filter_map(|(name, key, res)| res.err().map(|e| format!("{name} „{key}“: {e}")))
        .collect();

        if errors.is_empty() {
            log::write("RELOAD", 0, "", "OK");
            self.notify(
                "Nastavení načteno",
                &format!(
                    "{} = uspat/probudit\n{} = nouzové probuzení",
                    self.cfg.toggle_key, self.cfg.emergency_key
                ),
            );
        } else {
            log::write("RELOAD", 0, "", &format!("CHYBA: {}", errors.join("; ")));
            window::message_box(
                &format!(
                    "Některé zkratky nejdou zaregistrovat:\n\n{}",
                    errors.join("\n")
                ),
                true,
            );
        }
    }

    /// Always resumes a suspended process: otherwise it would stay frozen with
    /// nothing left to wake it up.
    pub fn shutdown(&mut self, reason: &str) {
        if let Some(st) = self.state.take() {
            let result = match process::nt_call(st.pid, NtOp::Resume) {
                Ok(()) => format!("OK (důvod ukončení: {reason})"),
                Err(err) => format!("CHYBA: {err} (důvod ukončení: {reason})"),
            };
            SUSPENDED_PID.store(0, Ordering::SeqCst);
            log::write("EXIT-RESUME", st.pid, &st.exe, &result);
        }
        self.unregister_hotkeys();
        self.tray.remove();
    }

    pub fn update_tray(&self) {
        let tip = match &self.state {
            Some(st) => format!("GameSuspend – USPÁNO: {} (PID {})", st.exe, st.pid),
            None => format!(
                "GameSuspend – nečinný ({} = uspat hru v popředí)",
                self.cfg.toggle_key
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
        let (status, toggle) = match &self.state {
            Some(st) => (
                format!("Stav: USPÁNO – {} (PID {})", st.exe, st.pid),
                MenuItem::new(
                    ID_TOGGLE,
                    format!("Probudit {}\t{}", st.exe, self.cfg.toggle_key),
                )
                .default_item(true),
            ),
            None => (
                "Stav: nečinný".to_string(),
                MenuItem::new(
                    ID_TOGGLE,
                    format!("Uspat hru v popředí\t{}", self.cfg.toggle_key),
                )
                .enabled(false),
            ),
        };
        vec![
            MenuItem::new(ID_STATUS, status).enabled(false),
            MenuItem::separator(),
            toggle,
            MenuItem::new(
                ID_EMERGENCY,
                format!("Nouzové probuzení\t{}", self.cfg.emergency_key),
            ),
            MenuItem::separator(),
            MenuItem::new(ID_OPEN_LOG, "Otevřít log").enabled(log::PATH.exists()),
            MenuItem::new(ID_OPEN_SETTINGS, "Otevřít nastavení"),
            MenuItem::new(ID_RELOAD, "Znovu načíst nastavení"),
            MenuItem::separator(),
            MenuItem::new(ID_EXIT, "Ukončit"),
        ]
    }

    pub fn on_menu(&mut self, id: u32) -> MenuEffect {
        match id {
            ID_TOGGLE => self.toggle(),
            ID_EMERGENCY => self.emergency(),
            ID_OPEN_LOG => return MenuEffect::Open(log::PATH.clone()),
            ID_OPEN_SETTINGS => return MenuEffect::Open(self.ini.clone()),
            ID_RELOAD => self.reload(),
            ID_EXIT => return MenuEffect::Exit,
            _ => {}
        }
        MenuEffect::None
    }
}
