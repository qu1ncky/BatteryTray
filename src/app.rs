//! The app around the core: command-line switches, a single instance, the tray icon, the windows,
//! reminders, the overlay and updates. Everything UI runs on the main thread in Slint's (winit's) event loop; the
//! polling and update threads, and the hidden Win32 window (tray clicks, a second launch, the overlay's shortcut),
//! hand their work to it with `slint::invoke_from_event_loop`.
//! Startup is the C# app's, step for step, because installs move from one to the other in place: the same mutex, the
//! same activator window a second launch signals, the same logon task and switches.

use crate::alert_view::AlertView;
use crate::autostart::{self, BACKGROUND_SWITCH};
use crate::hotkey::Gesture;
use crate::loc::{self, t, tf};
use crate::main_view::MainView;
use crate::monitor::Monitor;
use crate::overlay::{Overlay, Rect};
use crate::reading::DeviceReading;
use crate::rows::{self, AlertManager, ChargeDoneWatcher, DeviceRow};
use crate::settings::AppSettings;
use crate::state::StateStore;
use crate::tray::{self, Tray, MENU_EXIT, MENU_OPEN, MENU_REFRESH, MENU_SETTINGS};
use crate::update_service::{Event, Status, UpdateService};
use crate::updater::{self, ReleaseInfo, Version};
use crate::{diag, gui, log, providers, win};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
    FindWindowExW, PostMessageW, RegisterClassExW, RegisterWindowMessageW, ASFW_ANY, HWND_MESSAGE, MSGFLT_ALLOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CONTEXTMENU, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP,
    WNDCLASSEXW,
};

const WM_APP_TRAY: u32 = WM_APP + 2;
const ACTIVATOR_TITLE: PCWSTR = w!("BatteryTray.Activator");
const CLASS_NAME: PCWSTR = w!("BatteryTray.Window");

static DEVICES_PENDING: AtomicBool = AtomicBool::new(false);
static OPEN_MESSAGE: OnceLock<u32> = OnceLock::new();
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// What a second launch posts to the running instance: open your window.
fn open_message() -> u32 {
    *OPEN_MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("BatteryTray.Open")) })
}

/// What Explorer broadcasts after it restarts: tray icons have to be added again.
fn taskbar_created() -> u32 {
    *TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

pub fn version() -> String {
    Version::current().to_string()
}

/// Runs the app (or a command-line switch) and returns the exit code.
pub fn run(args: &[String]) -> i32 {
    std::panic::set_hook(Box::new(|info| log::write(&format!("panic: {info}"))));
    if let Some(code) = command_line(args) {
        return code;
    }
    // Only with the user's own rights: %TEMP% is theirs, and an elevated copy deleting a folder there could be led
    // anywhere through a junction. A launch from the Start menu is one.
    if !win::is_elevated() {
        remove_dotnet_leftovers();
    }

    // Launched by hand while the app already runs in the tray: open its window instead of starting again
    if signal_running() {
        return 0;
    }
    // Launched by hand without admin rights: start the elevated copy through the logon task (no UAC prompt), so the
    // Razer headset can be read, and have it open its window
    // Twice: the task ignores a start while its own instance still runs, and the copy that just installed the app for
    // admin rights may be that instance, on its way out as this one starts
    if !win::is_elevated() && autostart::is_registered() {
        for _ in 0..2 {
            if !autostart::run_task() {
                break;
            }
            for _ in 0..30 {
                std::thread::sleep(Duration::from_millis(100));
                if signal_running() {
                    return 0;
                }
            }
        }
        // The task brought nothing up (its exe was moved or deleted): run this copy instead
    }

    let Some(single) = SingleInstance::acquire() else { return 0 };
    App::run(single, args.iter().any(|a| a == BACKGROUND_SWITCH))
}

/// Switches for the installer, the updater and for diagnostics:
///   --register-task / --unregister-task   create or remove the elevated logon task (needs admin)
///   --install                             copy this exe to Program Files, register its logon task and add a Start
///                                         menu shortcut (needs admin; "Grant rights" in the window runs it)
///   --update                              install the latest release over this exe; exit 0 = updated,
///                                         2 = already up to date, 1 = failed (the app runs it elevated)
///   --dump [file]                         poll every provider once and write the readings to a file
fn command_line(args: &[String]) -> Option<i32> {
    Some(match args.first()?.as_str() {
        "--update" => update_now(),
        "--register-task" => {
            if autostart::register() {
                0
            } else {
                1
            }
        }
        "--unregister-task" => {
            if autostart::unregister() {
                0
            } else {
                1
            }
        }
        "--install" => {
            if autostart::install_here() {
                0
            } else {
                1
            }
        }
        "--dump" => {
            let file = args.get(1).map(std::path::PathBuf::from).unwrap_or_else(|| log::folder().join("dump.txt"));
            let _ = std::fs::write(file, diag::dump_lines().join("\r\n") + "\r\n");
            0
        }
        _ => return None,
    })
}

fn update_now() -> i32 {
    match updater::get_latest() {
        Ok(Some(latest)) if latest.version > Version::current() => match updater::install(&latest, &|_| {}) {
            Ok(()) => 0,
            Err(e) => {
                log::write(&format!("update (--update) failed: {e}"));
                1
            }
        },
        Ok(_) => 2,
        Err(e) => {
            log::write(&format!("update (--update) failed: {e}"));
            1
        }
    }
}

/// Asks the running instance to open its window. False when no instance is running.
fn signal_running() -> bool {
    unsafe {
        let Ok(target) = FindWindowExW(Some(HWND_MESSAGE), None, PCWSTR::null(), ACTIVATOR_TITLE) else { return false };
        // The running instance may bring its window to the front
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        PostMessageW(Some(target), open_message(), WPARAM(0), LPARAM(0)).is_ok()
    }
}

/// The mutex the C# app used too: one instance, whichever of the two it is.
struct SingleInstance(HANDLE);

impl SingleInstance {
    fn acquire() -> Option<Self> {
        const ERROR_ALREADY_EXISTS: u32 = 183;
        unsafe {
            // Access denied: an elevated instance owns it
            let handle = CreateMutexW(None, true, w!("BatteryTray.SingleInstance")).ok()?;
            if win::last_error() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(Self(handle))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// Runs `f` on the app, on the UI thread, once the event loop gets to it.
fn later(f: impl FnOnce(&mut App) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        with_app(f);
    });
}

/// The app's state, for code on the UI thread. None while it is busy further up the stack (a bug) or gone.
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut app) => app.as_mut().map(f),
        Err(_) => {
            log::write("app state busy: a UI action was dropped");
            None
        }
    })
}

pub struct App {
    activator: HWND,
    pub settings: AppSettings,
    state: Arc<StateStore>,
    monitor: Option<Monitor>,
    tray: Option<Tray>,
    tooltip: String,
    charge_done: ChargeDoneWatcher,
    alerts: AlertManager,
    updates: Option<UpdateService>,
    update_events: Receiver<Event>,
    single: Option<SingleInstance>,
    pub overlay: Overlay,
    main: Option<MainView>,
    alert: Option<AlertView>,
    test_alert: Option<AlertView>,
    alert_timer: slint::Timer,
    save_timer: slint::Timer,
    stopping: bool,
}

impl App {
    fn run(single: SingleInstance, background: bool) -> i32 {
        if let Err(e) = gui::init() {
            log::write(&format!("cannot start the UI: {e}"));
            return 1;
        }
        tray::use_dark_menus();
        let (hwnd, activator) = match create_windows() {
            Ok(windows) => windows,
            Err(e) => {
                log::write(&format!("cannot create the app's windows: {e}"));
                return 1;
            }
        };

        let mut settings = AppSettings::load();
        updater::clean_up();
        let updated_from = settings.last_run_version.clone().filter(|v| *v != version());
        settings.last_run_version = Some(version());
        settings.save();
        loc::set(loc::resolve(Some(&settings.language)));
        let state = Arc::new(StateStore::load());

        let monitor = Monitor::start(providers::create(&state), state.clone(), || {
            // One update for any number of polls that finish before the UI thread gets to it
            if !DEVICES_PENDING.swap(true, Ordering::AcqRel) {
                later(|app| {
                    DEVICES_PENDING.store(false, Ordering::Release);
                    app.on_devices_changed();
                });
            }
        });
        let (sender, update_events) = channel();
        let updates = UpdateService::start(settings.auto_update, move |event| {
            let _ = sender.send(event);
            later(|app| app.on_update_events());
        });
        let tray = Tray::new(hwnd, WM_APP_TRAY);
        let mut overlay = Overlay::new(hwnd);
        overlay.apply_settings(&settings);

        log::write(&format!(
            "started v{}, elevated={}{}",
            version(),
            if win::is_elevated() { "True" } else { "False" },
            updated_from.as_ref().map(|v| format!(", updated from v{v}")).unwrap_or_default()
        ));
        let app = App {
            activator,
            settings,
            state,
            monitor: Some(monitor),
            tray: Some(tray),
            tooltip: String::new(),
            charge_done: ChargeDoneWatcher::default(),
            alerts: AlertManager::default(),
            updates: Some(updates),
            update_events,
            single: Some(single),
            overlay,
            main: None,
            alert: None,
            test_alert: None,
            alert_timer: slint::Timer::default(),
            save_timer: slint::Timer::default(),
            stopping: false,
        };
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        with_app(|app| {
            // Snoozes run out between polls
            app.alert_timer.start(slint::TimerMode::Repeated, Duration::from_secs(30), || {
                with_app(|app| app.evaluate_alerts());
            });
            app.update_tray();
            if updated_from.is_some() {
                app.notice(&t("notice.updated.title"), &tf("notice.updated.text", &[&version()]));
            }
        });
        // The logon task starts it with --background; started any other way, show the window
        if !background {
            later(|app| app.open());
        }

        if let Err(e) = slint::run_event_loop_until_quit() {
            log::write(&format!("event loop: {e}"));
        }
        APP.with(|cell| cell.borrow_mut().take());
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        0
    }

    /// Every device known: what the providers report now, plus devices only remembered from before.
    pub fn rows(&self) -> Vec<DeviceRow> {
        let current = self.monitor.as_ref().map(|m| m.current()).unwrap_or_default();
        let remembered: Vec<(String, Option<DeviceReading>)> = self
            .state
            .ids()
            .into_iter()
            .map(|id| {
                let last = self.state.get(&id);
                (id, last)
            })
            .collect();
        rows::rows(current, &remembered, &self.settings)
    }

    pub fn update_status(&self) -> Status {
        self.updates.as_ref().map(|u| u.status()).unwrap_or(Status {
            stage: crate::update_service::Stage::NotChecked,
            latest: None,
            last_check: None,
            progress: 0,
            error: None,
        })
    }

    /// Opens (or brings to the front) the main window, on its devices.
    pub fn open(&mut self) {
        self.show_main(false);
    }

    /// The tray menu's "Settings": the same, on the settings' first tab.
    pub fn open_settings(&mut self) {
        self.show_main(true);
    }

    fn show_main(&mut self, settings: bool) {
        if self.stopping {
            return;
        }
        if self.main.is_none() {
            match MainView::open(self) {
                Ok(view) => self.main = Some(view),
                Err(e) => {
                    log::write(&format!("cannot open the window: {e}"));
                    return;
                }
            }
        }
        // Every opening starts from the same place, with nothing left open from before (an open window may be
        // behind others, on any screen)
        if let Some(view) = &self.main {
            view.ui.invoke_reset_screen(settings);
        }
        // A name that was being typed is dropped with the rest, and the list may have held its device back
        self.refresh_main();
        if let Some(view) = &self.main {
            gui::show_and_activate(&view.ui);
        }
        self.sync_window_open();
    }

    pub fn close_main(&mut self) {
        self.main = None;
        self.overlay.suspended = false;
        self.sync_window_open();
    }

    pub fn refresh_main(&mut self) {
        if let Some(view) = &self.main {
            view.refresh(&self.rows(), &self.settings);
        }
    }

    pub fn refresh_now(&mut self) {
        if let Some(m) = &self.monitor {
            m.refresh_now();
        }
    }

    /// An automatic update waits while a window of the app is open, so it never vanishes from under the user.
    fn sync_window_open(&self) {
        if let Some(u) = &self.updates {
            u.set_window_open(self.main.is_some() || self.alert.is_some() || self.overlay.is_timed_visible());
        }
    }

    fn notice(&self, title: &str, text: &str) {
        if let Some(tray) = &self.tray {
            tray.show_notice(title, text);
        }
    }

    /// A settings change from the window: applied, shown everywhere, saved half a second after the last one.
    pub fn edit(&mut self, change: impl FnOnce(&mut AppSettings)) {
        change(&mut self.settings);
        self.settings_changed();
    }

    fn settings_changed(&mut self) {
        self.save_later();
        if let Some(u) = &self.updates {
            u.set_auto_update(self.settings.auto_update);
        }
        self.update_tray();
        self.overlay.apply_settings(&self.settings);
        let rows = self.rows();
        let obstruction = self.alert_bounds();
        self.overlay.refresh(&rows, &self.settings, obstruction);
        if let Some(view) = &self.main {
            view.load_settings(self);
            view.refresh(&rows, &self.settings);
        }
        self.evaluate_alerts();
    }

    /// Saves the settings half a second after the last change: a burst of changes is one write.
    fn save_later(&self) {
        self.save_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(500), || {
            with_app(|app| app.settings.save());
        });
    }

    /// A name typed on a card or a row, as typed: kept without the spaces around it, empty for the device's own. The
    /// list is refreshed also when nothing changed: while the name was typed, the device was held in the block it
    /// was shown in.
    pub fn rename_device(&mut self, id: &str, text: &str) {
        if self.settings.set_name(id, text) {
            self.settings_changed();
        } else {
            self.refresh_main();
        }
    }

    pub fn pin_device(&mut self, id: &str, pinned: bool) {
        let rows = self.rows();
        if rows::set_pinned(&mut self.settings, &rows, id, pinned) {
            self.settings_changed();
        }
    }

    /// A device dropped onto another one's place in its block. Refused across blocks: the window then still shows the
    /// order it had, as it never moved a row itself.
    pub fn move_device(&mut self, id: &str, onto: &str) {
        let rows = self.rows();
        if rows::move_onto(&mut self.settings, &rows, id, onto) {
            self.settings_changed();
        }
    }

    pub fn dismiss_autostart_banner(&mut self) {
        self.edit(|s| s.autostart_banner_dismissed = true);
    }

    pub fn dismiss_admin_banner(&mut self) {
        self.edit(|s| s.admin_banner_dismissed = true);
    }

    /// "Grant rights", confirmed in the window: the app installs itself into Program Files (one UAC prompt), where the
    /// logon task starts it with admin rights, and the installed copy takes over. Declined, nothing changes.
    pub fn grant_admin(&mut self) {
        if autostart::install() {
            later(|app| app.hand_over_to_installed());
        } else {
            log::write("admin rights: the install was declined or failed");
        }
    }

    /// A language picked in the settings ("auto" follows Windows): everything switches right away.
    pub fn set_language(&mut self, setting: &str) {
        self.settings.language = setting.to_string();
        loc::set(loc::resolve(Some(setting)));
        if let Some(view) = &self.main {
            view.ui.global::<gui::T>().set_lang(loc::current().into());
            view.refresh_update(&self.update_status());
        }
        for alert in [&self.alert, &self.test_alert].into_iter().flatten() {
            alert.ui.global::<gui::T>().set_lang(loc::current().into());
        }
        self.overlay.set_language();
        self.settings_changed();
        self.refresh_now(); // the devices' details come from the providers
    }

    pub fn set_autostart(&mut self, on: bool) {
        autostart::set(on);
        let registered = autostart::is_registered();
        if let Some(view) = &self.main {
            view.ui.set_autostart(registered);
        }
        // Switched off by hand: the user knows it is off, and the banner has nothing to tell them. A switch-off Windows
        // refused leaves it on, where the banner doesn't show anyway
        if !on && !registered && !self.settings.autostart_banner_dismissed {
            self.dismiss_autostart_banner();
        }
    }

    pub fn set_overlay_shortcut(&mut self, gesture: Gesture) {
        let mut settings = std::mem::take(&mut self.settings);
        let taken = self.overlay.try_set_shortcut(gesture, &mut settings);
        self.settings = settings;
        if taken {
            self.settings_changed();
            // The field gives up the focus, so its key caps show what was just recorded
            if let Some(view) = &self.main {
                view.ui.invoke_shortcut_recorded();
            }
        } else if let Some(view) = &self.main {
            view.shortcut_refused(gesture.is_valid());
        }
    }

    pub fn overlay_preview(&mut self) {
        let rows = self.rows();
        let obstruction = self.alert_bounds();
        self.overlay.preview(&rows, &self.settings, obstruction);
        self.sync_window_open();
    }

    fn on_hotkey(&mut self, id: i32) {
        // While the shortcut is being recorded, pressing it does nothing
        self.overlay.suspended = self.main.as_ref().is_some_and(|m| m.ui.get_recording_shortcut());
        let rows = self.rows();
        let obstruction = self.alert_bounds();
        self.overlay.on_hotkey(id, &rows, &self.settings, obstruction);
        self.sync_window_open();
    }

    pub fn overlay_tick(&mut self) {
        if self.overlay.tick() {
            self.sync_window_open();
        }
    }

    pub fn overlay_place(&mut self) {
        let obstruction = self.alert_bounds();
        self.overlay.place(obstruction);
    }

    fn alert_bounds(&self) -> Option<Rect> {
        self.alert.as_ref().and_then(|a| a.bounds())
    }

    fn on_devices_changed(&mut self) {
        if self.stopping {
            return;
        }
        // Once the user has ordered the list, a device seen for the first time goes last in its block
        let rows = self.rows();
        if rows::adopt_new(&mut self.settings, &rows) {
            self.save_later();
        }
        self.update_tray();
        self.refresh_main();
        self.evaluate_alerts();
        self.announce_charged();
        let rows = self.rows();
        let obstruction = self.alert_bounds();
        self.overlay.refresh(&rows, &self.settings, obstruction);
        self.state.save_if_dirty();
    }

    /// A plain Windows notification: unlike a low battery, nothing needs doing right away.
    fn announce_charged(&mut self) {
        let rows = self.rows();
        for row in self.charge_done.finished(&rows) {
            let level = row.current.as_ref().and_then(|c| c.level).unwrap_or(100);
            log::write(&format!("charged: {} {level}%", row.name));
            let ds = self.settings.device(&row.id);
            if !self.settings.notify_charged || ds.hidden || !ds.alerts {
                continue;
            }
            self.notice(&t("notice.charged.title"), &tf("notice.charged.text", &[&row.name, &level]));
        }
    }

    fn evaluate_alerts(&mut self) {
        if self.stopping || self.monitor.is_none() {
            return;
        }
        let due = self.alerts.due(&self.rows(), &self.settings, Instant::now());
        if due.is_empty() {
            self.close_alert();
            return;
        }

        let fresh = self.alert.as_ref().is_none_or(|a| due.iter().any(|d| !a.shows(d)));
        if self.alert.is_none() {
            let view = AlertView::new(
                || {
                    with_app(|app| {
                        let ids = app.alert.as_ref().map(|a| a.ids()).unwrap_or_default();
                        let minutes = app.settings.repeat_minutes;
                        app.alerts.snooze(&ids, Duration::from_secs(minutes as u64 * 60));
                        log::write(&format!("alert snoozed for {minutes} min"));
                        app.close_alert();
                    });
                },
                || {
                    with_app(|app| {
                        let ids = app.alert.as_ref().map(|a| a.ids()).unwrap_or_default();
                        app.alerts.mute_until_charged(&ids);
                        log::write("alert muted until charged");
                        app.close_alert();
                    });
                },
            );
            match view {
                Ok(view) => self.alert = Some(view),
                Err(e) => {
                    log::write(&format!("cannot open the reminder: {e}"));
                    return;
                }
            }
        }
        let names = due.iter().map(|d| format!("{} {}%", d.name, d.level)).collect::<Vec<_>>().join(", ");
        let critical = due.iter().any(|d| d.critical);
        if let Some(alert) = &mut self.alert {
            alert.set_items(due, self.settings.repeat_minutes);
            alert.show();
        }
        self.sync_window_open();
        self.overlay_place();
        if fresh {
            log::write(&format!("alert: {names}"));
            self.play_sound(critical);
        }
    }

    fn close_alert(&mut self) {
        if let Some(alert) = self.alert.take() {
            alert.close();
        }
        self.sync_window_open();
        self.overlay_place();
    }

    /// Each device at its own levels: the first one just below its reminder level, the second one critical.
    pub fn show_test_alert(&mut self) {
        let rows = self.rows();
        let mut sample: Vec<rows::AlertItem> = rows
            .iter()
            .filter(|r| r.online() && r.current.as_ref().is_some_and(|c| c.level.is_some()))
            .take(2)
            .enumerate()
            .map(|(i, r)| rows::AlertItem {
                id: r.id.clone(),
                name: r.name.clone(),
                kind: r.kind(),
                level: if i == 0 { self.settings.low_for(&r.id) - 2 } else { self.settings.critical_for(&r.id) - 1 },
                critical: i > 0,
            })
            .collect();
        if sample.is_empty() {
            sample.push(rows::AlertItem {
                id: "test".into(),
                name: t("name.mouse"),
                kind: crate::reading::DeviceKind::Mouse,
                level: self.settings.low_percent - 2,
                critical: false,
            });
        }
        let critical = sample.iter().any(|s| s.critical);
        let close = || {
            with_app(|app| {
                if let Some(a) = app.test_alert.take() {
                    a.close();
                }
            });
        };
        match AlertView::new(close, close) {
            Ok(mut view) => {
                view.set_items(sample, self.settings.repeat_minutes);
                view.show();
                if let Some(old) = self.test_alert.replace(view) {
                    old.close();
                }
                self.play_sound(critical);
            }
            Err(e) => log::write(&format!("cannot open the sample reminder: {e}")),
        }
    }

    fn play_sound(&self, critical: bool) {
        if self.settings.sound {
            unsafe {
                // The system's Critical Stop and Exclamation sounds, as the C# app's SystemSounds
                MessageBeep(if critical { 0x10 } else { 0x30 });
            }
        }
    }

    pub fn updates_install(&mut self) {
        if let Some(u) = &self.updates {
            u.install_now();
        }
    }

    pub fn updates_check(&mut self) {
        if let Some(u) = &self.updates {
            u.check_now();
        }
    }

    fn update_tray(&mut self) {
        let rows = self.rows();
        // In the order of the window's list
        let shown: Vec<&DeviceRow> = rows::list_order(&rows, &self.settings)
            .into_iter()
            .filter(|r| r.online() && !self.settings.device(&r.id).hidden)
            .collect();
        // A device charging without a readable level is listed too, without a made-up number
        let lines: Vec<String> = shown
            .iter()
            .filter_map(|r| {
                let c = r.current.as_ref()?;
                match c.level {
                    Some(l) => Some(format!(
                        "{}: {}{l}%{}",
                        short(&r.name),
                        if c.estimate { "≈" } else { "" },
                        if c.charging { " ⚡" } else { "" }
                    )),
                    None if c.charging => Some(tf("tray.charging", &[&short(&r.name)])),
                    None => None,
                }
            })
            .collect();
        self.tooltip = if lines.is_empty() { t("tray.nodata") } else { lines.join("\n") };

        // The icon shows the device that needs charging most by its own levels: a critical one first, then one
        // below its reminder level, the least charged among equals; charging ones only when nothing else is left
        let live: Vec<(&DeviceRow, &DeviceReading, i32)> =
            shown.iter().filter_map(|r| r.current.as_ref().and_then(|c| c.level.map(|l| (*r, c, l)))).collect();
        let s = &self.settings;
        let urgency = |id: &str, level: i32| {
            if level <= s.critical_for(id) {
                2
            } else if level <= s.low_for(id) {
                1
            } else {
                0
            }
        };
        let pick = live
            .iter()
            .filter(|(_, c, _)| !c.charging)
            .min_by_key(|(r, _, l)| (std::cmp::Reverse(urgency(&r.id, *l)), *l))
            .or_else(|| live.iter().min_by_key(|(_, _, l)| *l));
        let (label, color) = match pick {
            None => ("--".to_string(), (0x80, 0x80, 0x80)),
            Some((r, c, l)) => (l.to_string(), level_color(Some(*l), c.charging, true, &r.id, s)),
        };
        let tooltip = self.tooltip.clone();
        if let Some(tray) = &mut self.tray {
            tray.update(&label, color, &tooltip);
        }
    }

    fn on_update_events(&mut self) {
        while let Ok(event) = self.update_events.try_recv() {
            match event {
                Event::Changed => {
                    if let Some(view) = &self.main {
                        view.refresh_update(&self.update_status());
                    }
                }
                // Announced once per version when it can't install by itself
                Event::Announce(release) => {
                    self.notice(&tf("notice.available.title", &[&release.version]), &t("notice.available.text"))
                }
                Event::Installed(release) => {
                    self.restart_into_update(&release);
                    return;
                }
            }
        }
    }

    /// The new exe is in place: start it and step aside. The new copy can only start once this one has released the
    /// single-instance mutex and the activator window, so tear down first.
    fn restart_into_update(&mut self, release: &ReleaseInfo) {
        log::write(&format!("restarting into v{}", release.version));
        let reopen = self.main.is_some();
        let exe = std::env::current_exe().unwrap_or_default();
        self.tear_down();
        // Started directly, with this copy's rights: an elevated copy comes back elevated, and one without admin rights
        // goes through the logon task by itself if it can (run() does). Running the task from here would not do: when
        // this copy is the task's own instance - always for a user without admin rights - the task ignores a second
        // start while this process is still alive, and nothing would come back until the next sign-in.
        let mut command = std::process::Command::new(exe);
        if !reopen {
            command.arg(BACKGROUND_SWITCH);
        }
        if let Err(e) = command.spawn() {
            log::write(&format!("cannot start the new version: {e}"));
        }
        let _ = slint::quit_event_loop();
    }

    /// Installed for admin rights: the copy in Program Files takes over, with the window open. Started directly,
    /// it goes through the new logon task by itself and comes up with admin rights (run() does); an elevated copy
    /// starts it elevated anyway. Like an update, it can only start once this one has torn down.
    fn hand_over_to_installed(&mut self) {
        let Some(exe) = autostart::installed_exe() else { return };
        log::write("handing over to the installed copy");
        self.tear_down();
        if let Err(e) = std::process::Command::new(exe).spawn() {
            log::write(&format!("cannot start the installed copy: {e}"));
        }
        let _ = slint::quit_event_loop();
    }

    fn exit(&mut self) {
        log::write("exit");
        self.tear_down();
        let _ = slint::quit_event_loop();
    }

    fn tear_down(&mut self) {
        if self.stopping {
            return;
        }
        self.stopping = true;
        self.alert_timer.stop();
        self.save_timer.stop();
        self.overlay.hide();
        if let Some(monitor) = self.monitor.take() {
            monitor.stop();
        }
        self.updates = None;
        self.state.save();
        self.settings.save();
        self.close_alert();
        self.test_alert = None;
        self.main = None;
        self.tray = None;
        unsafe {
            let _ = DestroyWindow(self.activator);
        }
        self.single = None;
    }
}

/// The level color a device shows at: its own thresholds when set, the global ones otherwise.
pub fn level_color(level: Option<i32>, charging: bool, online: bool, id: &str, s: &AppSettings) -> (u8, u8, u8) {
    const GOOD: (u8, u8, u8) = (0x6C, 0xCB, 0x5F);
    const WARN: (u8, u8, u8) = (0xF7, 0xC0, 0x4A);
    const BAD: (u8, u8, u8) = (0xFF, 0x6B, 0x6B);
    const CHARGE: (u8, u8, u8) = (0x60, 0xCD, 0xFF);
    const OFFLINE: (u8, u8, u8) = (0x7A, 0x7A, 0x7A);
    match level {
        _ if !online => OFFLINE,
        _ if charging => CHARGE,
        None => OFFLINE,
        Some(l) if l <= s.critical_for(id) => BAD,
        Some(l) if l <= s.low_for(id) => WARN,
        Some(_) => GOOD,
    }
}

fn short(name: &str) -> String {
    if name.chars().count() > 24 {
        name.chars().take(23).collect::<String>() + "…"
    } else {
        name.to_string()
    }
}

/// The C# app ran as a single-file .NET exe that unpacked its native DLLs to %TEMP%\.net\BatteryTray, one folder per
/// build. Nothing uses them any more.
fn remove_dotnet_leftovers() {
    let folder = std::env::temp_dir().join(".net").join("BatteryTray");
    if folder.exists() && std::fs::remove_dir_all(&folder).is_ok() {
        log::write(&format!("removed the .NET app's leftovers in {}", folder.display()));
    }
}

/// A hidden top-level window for the tray and the overlay's shortcut (Explorer broadcasts TaskbarCreated only to
/// top-level windows), and a message-only one a second launch finds by its title.
fn create_windows() -> windows::core::Result<(HWND, HWND)> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            w!("BatteryTray"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let activator = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            ACTIVATOR_TITLE,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )?;
        // The app usually runs elevated and the launcher or Explorer don't: let their messages through UIPI
        let _ = ChangeWindowMessageFilterEx(activator, open_message(), MSGFLT_ALLOW, None);
        let _ = ChangeWindowMessageFilterEx(hwnd, taskbar_created(), MSGFLT_ALLOW, None);
        let _ = ChangeWindowMessageFilterEx(hwnd, WM_APP_TRAY, MSGFLT_ALLOW, None);
        Ok((hwnd, activator))
    }
}

/// The hidden windows' messages. Nothing here touches the app directly: the work goes to the event loop, where no
/// other part of the app is in the middle of something.
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_APP_TRAY => match lparam.0 as u32 {
            WM_LBUTTONUP => later(|app| app.open()),
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                let chosen = tray::show_menu(hwnd);
                later(move |app| match chosen {
                    MENU_OPEN => app.open(),
                    MENU_SETTINGS => app.open_settings(),
                    MENU_REFRESH => app.refresh_now(),
                    MENU_EXIT => app.exit(),
                    _ => {}
                });
            }
            _ => {}
        },
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            later(move |app| app.on_hotkey(id));
        }
        WM_ENDSESSION if wparam.0 != 0 => {
            // Windows is shutting down or signing out: what a normal exit saves, now, before this returns
            with_app(|app| app.tear_down());
        }
        m if m == open_message() && m != 0 => later(|app| app.open()),
        m if m == taskbar_created() && m != 0 => later(|app| {
            if let Some(tray) = &app.tray {
                tray.readd();
            }
        }),
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

#[link(name = "user32", kind = "raw-dylib")]
extern "system" {
    fn MessageBeep(kind: u32) -> i32;
}
