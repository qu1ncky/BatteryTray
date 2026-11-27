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
use crate::sound_player::{self, Player};
use crate::sounds::{self, Choice};
use crate::state::StateStore;
use crate::tray::{self, Tray, MENU_EXIT, MENU_OPEN, MENU_REFRESH, MENU_SETTINGS};
use crate::update_service::{Event, Status, UpdateService};
use crate::updater::{self, ReleaseInfo, Version};
use crate::{diag, gui, log, providers, theme, view, win};
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
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CONTEXTMENU, WM_DEVICECHANGE, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP,
    WM_RBUTTONUP, WNDCLASSEXW,
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

fn toggle_main() {
    let panel = with_app(|app| {
        if let Some(view) = &app.main {
            return Some(view.ui.clone_strong());
        }
        app.open();
        None
    })
    .flatten();
    // Closing can apply edits through callbacks, so release the app state borrow first.
    if let Some(ui) = panel {
        MainView::dismiss(&ui);
    }
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
    sound: Player,
    /// Ends the sound that plays: when it is over, or at `sounds::MAX_PLAY`
    sound_timer: slint::Timer,
    /// The sound the settings' play button started, while it plays
    pub sound_playing: Option<sounds::Event>,
    /// The sound for which a file was picked that the app couldn't take; said under its row until the next choice
    pub sound_refused: Option<sounds::Event>,
    stopping: bool,
}

impl App {
    fn run(single: SingleInstance, background: bool) -> i32 {
        if let Err(e) = gui::init() {
            log::write(&format!("cannot start the UI: {e}"));
            return 1;
        }
        tray::set_menu_mode(theme::current().taskbar_light);
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
        // The theme picked in the settings, before any window: each one is made in the theme it shows
        theme::choose(&settings.theme);
        // New devices are always looked for on Windows: the polls when a device connects have no switch here
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
        // Light or dark: followed while the app runs, also with no window open (the tray icon's number)
        theme::watch(|| later(|app| app.on_theme_changed()));
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
            sound: Player::default(),
            sound_timer: slint::Timer::default(),
            sound_playing: None,
            sound_refused: None,
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
                app.notice(&t("notice.updated.title"), &tf("notice.updated.text", &[&version()]), false);
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
            let Some(tray) = &self.tray else { return };
            match MainView::open(self, tray.window()) {
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
            view.show();
        }
        self.sync_window_open();
    }

    pub fn close_main(&mut self) {
        self.main = None;
        self.overlay.suspended = false;
        // What the window was playing or saying about a sound goes with it
        if self.sound_playing.is_some() {
            self.stop_sound();
        }
        self.sound_refused = None;
        self.sync_window_open();
    }

    /// The main window's handle while it is open: the owner of a dialog opened from it.
    pub fn main_hwnd(&self) -> Option<HWND> {
        self.main.as_ref().and_then(|view| gui::hwnd(view.ui.window()))
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

    /// A Windows notification; `silent`: without Windows' own sound.
    fn notice(&self, title: &str, text: &str, silent: bool) {
        if let Some(tray) = &self.tray {
            tray.show_notice(title, text, silent);
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

    /// A theme picked in the settings ("auto" follows Windows): the windows switch right away. The tray icon's number
    /// and menu keep the taskbar's mode.
    pub fn set_theme(&mut self, setting: &str) {
        self.settings.theme = setting.to_string();
        if theme::choose(setting) {
            self.windows_follow_theme();
        }
        self.settings_changed();
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

    /// A plain Windows notification: unlike a low battery, nothing needs doing right away. With the system's sound
    /// chosen for it, the sound is the notification's own, as it always was; any other sound is the app's to play,
    /// and Windows shows the notification silently.
    fn announce_charged(&mut self) {
        let rows = self.rows();
        let choice = self.settings.sound_for(sounds::Event::Charged);
        let mut announced = false;
        for row in self.charge_done.finished(&rows) {
            let level = row.current.as_ref().and_then(|c| c.level).unwrap_or(100);
            log::write(&format!("charged: {} {level}%", row.name));
            let ds = self.settings.device(&row.id);
            if !self.settings.notify_charged || ds.hidden || !ds.alerts {
                continue;
            }
            let text = tf("notice.charged.text", &[&row.name, &level]);
            self.notice(&t("notice.charged.title"), &text, choice != Choice::System);
            announced = true;
        }
        // Only with a notice Windows shows: it holds them back during a full-screen game or a presentation
        if announced && !matches!(choice, Choice::System | Choice::None) && sound_player::notices_are_shown() {
            self.play(sounds::Event::Charged);
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
                        if app.alert.as_ref().is_some_and(|a| a.closed_on_its_own()) {
                            log::write(&format!("alert closed on its own, back in {minutes} min"));
                        } else {
                            log::write(&format!("alert snoozed for {minutes} min"));
                        }
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
            alert.set_items(due, self.settings.repeat_minutes, self.settings.auto_close_after());
            alert.show();
        }
        self.sync_window_open();
        self.overlay_place();
        if fresh {
            log::write(&format!("alert: {names}"));
            self.play(if critical { sounds::Event::Critical } else { sounds::Event::Low });
        }
    }

    fn close_alert(&mut self) {
        if let Some(alert) = self.alert.take() {
            alert.close();
            // The reminder's sound doesn't outlast the reminder
            self.stop_sound();
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
                level: (if i == 0 { self.settings.low_for(&r.id) - 2 } else { self.settings.critical_for(&r.id) - 1 })
                    .max(0),
                critical: i > 0,
            })
            .collect();
        if sample.is_empty() {
            sample.push(rows::AlertItem {
                id: "test".into(),
                name: t("name.mouse"),
                kind: crate::reading::DeviceKind::Mouse,
                level: (self.settings.low_percent - 2).max(0),
                critical: false,
            });
        }
        let critical = sample.iter().any(|s| s.critical);
        let close = || {
            with_app(|app| {
                if let Some(a) = app.test_alert.take() {
                    a.close();
                    app.stop_sound();
                }
            });
        };
        match AlertView::new(close, close) {
            Ok(mut view) => {
                view.set_items(sample, self.settings.repeat_minutes, self.settings.auto_close_after());
                view.show();
                if let Some(old) = self.test_alert.replace(view) {
                    old.close();
                }
                self.play(if critical { sounds::Event::Critical } else { sounds::Event::Low });
            }
            Err(e) => log::write(&format!("cannot open the sample reminder: {e}")),
        }
    }

    /// Plays the sound chosen for an event, ending the one before. It is over when it has played, at
    /// `sounds::MAX_PLAY` at the latest, and when the reminder it came with closes. False when nothing sounds.
    fn play(&mut self, event: sounds::Event) -> bool {
        self.stop_sound();
        let choice = self.settings.sound_for(event);
        let Some(length) = self.sound.play(&choice, event) else { return false };
        // Only a sound that reaches the limit is cut: a system sound's length is a guess
        let cut = length >= sounds::MAX_PLAY;
        self.sound_timer.start(slint::TimerMode::SingleShot, length.min(sounds::MAX_PLAY), move || {
            with_app(|app| {
                if cut {
                    app.sound.stop();
                }
                app.show_playing(None);
            });
        });
        true
    }

    fn stop_sound(&mut self) {
        self.sound_timer.stop();
        self.sound.stop();
        self.show_playing(None);
    }

    /// Which sound the settings' play button shows as playing.
    fn show_playing(&mut self, event: Option<sounds::Event>) {
        if self.sound_playing != event {
            self.sound_playing = event;
            self.show_sounds();
        }
    }

    /// The settings' rows of sounds, after something about them changed that isn't a setting: what plays, what was
    /// refused, what is in the Sounds folder.
    pub fn show_sounds(&self) {
        if let Some(view) = &self.main {
            view.load_sounds(self);
        }
    }

    /// The sound at a place in the settings' lists as the window shows them; None for the last line, "Custom
    /// file…", which is no sound.
    pub fn sound_at(&self, index: i32) -> Option<Choice> {
        self.main.as_ref().and_then(|view| view.sound_at(index))
    }

    /// A sound picked in the settings: kept, and played once so that the choice is heard.
    pub fn choose_sound(&mut self, event: sounds::Event, choice: Choice) {
        // A file deleted from the folder while the window still listed it: the lists are brought up to date instead
        if matches!(&choice, Choice::File(name) if !sounds::path_of(name).is_file()) {
            self.show_sounds();
            return;
        }
        self.sound_refused = None;
        // Also the sound being heard for this row: the new one starts, it isn't the button's "stop"
        self.stop_sound();
        self.edit(|s| s.set_sound(event, &choice));
        self.preview_sound(event);
    }

    /// The settings' play button of a sound: plays it, or ends it while it plays.
    pub fn preview_sound(&mut self, event: sounds::Event) {
        if self.sound_playing == Some(event) {
            self.stop_sound();
        } else if self.play(event) {
            self.show_playing(Some(event));
        }
    }

    /// A file of the user's has been taken out of the Sounds folder (the ✕ in a list): it is in no list any more, and a
    /// sound that had it is the system's again.
    pub fn sound_removed(&mut self, name: &str) {
        log::write(&format!("sound {name} removed"));
        self.stop_sound();
        let gone = Choice::File(name.to_string());
        let had: Vec<sounds::Event> =
            sounds::Event::ALL.into_iter().filter(|&event| self.settings.sound_for(event) == gone).collect();
        if had.is_empty() {
            self.show_sounds();
        } else {
            self.edit(|s| {
                for event in had {
                    s.set_sound(event, &Choice::System);
                }
            });
        }
    }

    /// A file picked for a sound in the "Open" dialog: copied into the Sounds folder and chosen. One the app can't
    /// take (not a WAV or an MP3, too big, unreadable) leaves the sound as it was, and the settings say so.
    pub fn add_sound(&mut self, event: sounds::Event, file: &std::path::Path) {
        match sounds::import(file) {
            Ok(name) => self.choose_sound(event, Choice::File(name)),
            Err(e) => {
                log::write(&format!("sound file not taken: {e:?}"));
                self.sound_refused = Some(event);
                self.show_sounds();
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
        let tray = view::tray_view(&self.rows(), &self.settings);
        self.tooltip = tray.tooltip;
        let tooltip = self.tooltip.clone();
        if let Some(tray_icon) = &mut self.tray {
            tray_icon.update(&tray.label, tray.tone, tray.charging, &tooltip, self.settings.tray_battery_icon);
        }
    }

    /// Windows' light or dark mode changed: the tray icon's number (drawn again, whatever the levels) and menu, the
    /// windows and the main window's title bar follow it at once.
    fn on_theme_changed(&mut self) {
        let Some(mode) = theme::reread() else { return };
        tray::set_menu_mode(mode.taskbar_light);
        self.update_tray();
        self.windows_follow_theme();
    }

    /// The open windows, and the main window's title bar, in the app mode as it is now.
    fn windows_follow_theme(&self) {
        if let Some(view) = &self.main {
            view.follow_theme();
        }
        for alert in [&self.alert, &self.test_alert].into_iter().flatten() {
            alert.follow_theme();
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
                    self.notice(&tf("notice.available.title", &[&release.version]), &t("notice.available.text"), false)
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

/// The C# app ran as a single-file .NET exe that unpacked its native DLLs to %TEMP%\.net\BatteryTray, one folder per
/// build. Nothing uses them any more.
fn remove_dotnet_leftovers() {
    let folder = std::env::temp_dir().join(".net").join("BatteryTray");
    if folder.exists() && std::fs::remove_dir_all(&folder).is_ok() {
        log::write(&format!("removed the .NET app's leftovers in {}", folder.display()));
    }
}

/// A hidden top-level window for the tray and the overlay's shortcut (Explorer broadcasts TaskbarCreated, and Windows
/// device changes, only to top-level windows), and a message-only one a second launch finds by its title.
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
            WM_LBUTTONUP => {
                let _ = slint::invoke_from_event_loop(toggle_main);
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                let current_priority =
                    with_app(|app| app.settings.tray_priority.clone()).unwrap_or_else(|| "mouse".into());
                let chosen = tray::show_menu(hwnd, &current_priority);
                later(move |app| match chosen {
                    MENU_OPEN => app.open(),
                    MENU_SETTINGS => app.open_settings(),
                    MENU_REFRESH => app.refresh_now(),
                    MENU_EXIT => app.exit(),
                    id => {
                        if let Some(priority) = tray::priority_for_menu(id) {
                            app.edit(|s| {
                                s.tray_priority = priority.into();
                                s.normalize();
                            });
                        }
                    }
                });
            }
            _ => {}
        },
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            later(move |app| app.on_hotkey(id));
        }
        // Windows tells every top-level window when a device comes or goes (DBT_DEVNODES_CHANGED, also for a Bluetooth
        // headset that connects): the devices are asked again right away rather than on their schedules, unless the
        // settings say to wait for the schedules (see Monitor::devices_changed)
        WM_DEVICECHANGE => {
            later(|app| {
                if let Some(m) = &app.monitor {
                    m.devices_changed();
                }
            });
            // Answered as Windows would: nothing here is a reason to refuse a device's removal
            return DefWindowProcW(hwnd, msg, wparam, lparam);
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
