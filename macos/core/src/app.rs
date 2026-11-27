//! The app's state and decisions on a Mac, as app.rs has them on Windows: the polling, the reminders, the "charged"
//! notices, the settings and how a change spreads, the updates. The Swift app calls in from its main thread and does
//! what comes back: it shows, plays and opens; the decisions are made here.

use crate::demo;
use crate::keys;
use batterytray::autostart;
use batterytray::hid;
use batterytray::loc::{self, t, tf};
use batterytray::log;
use batterytray::monitor::{Monitor, Pusher};
use batterytray::providers::{self, Provider};
use batterytray::reading::{DeviceKind, DeviceReading};
use batterytray::rows::{self, AlertItem, AlertManager, ChargeDoneWatcher, DeviceRow};
use batterytray::settings::{hotkey_valid, AppSettings, DEFAULT_HOTKEY};
use batterytray::sounds::{self, Choice};
use batterytray::state::StateStore;
use batterytray::update_service::{Event, Stage, Status, UpdateService};
use batterytray::updater::{ReleaseInfo, Version};
use batterytray::view::{self, DeviceView};
use batterytray::{feedback, sys};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tells the Swift app (on any thread) that there is news: it then calls "pump" on its main thread.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

static DEVICES_PENDING: AtomicBool = AtomicBool::new(false);

/// Readings the Swift app reports, polled like any provider's, by source: "controllers" (the game controllers macOS
/// knows) and "ble" (the Battery Service of the Bluetooth LE devices connected to the Mac).
#[derive(Clone, Default)]
pub struct External {
    readings: Arc<Mutex<std::collections::BTreeMap<String, Vec<DeviceReading>>>>,
    push: Arc<Mutex<Option<Pusher>>>,
}

impl External {
    fn set(&self, source: &str, readings: Vec<DeviceReading>) {
        self.readings.lock().unwrap_or_else(|e| e.into_inner()).insert(source.to_string(), readings);
        if let Some(push) = self.push.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            push();
        }
    }
}

struct ExternalProvider(External);

impl Provider for ExternalProvider {
    fn name(&self) -> &'static str {
        "GameController/Bluetooth LE"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        self.0.readings.lock().unwrap_or_else(|e| e.into_inner()).values().flatten().cloned().collect()
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        Duration::from_secs(60)
    }

    fn set_pusher(&mut self, push: Pusher) {
        *self.0.push.lock().unwrap_or_else(|e| e.into_inner()) = Some(push);
    }
}

pub struct MacApp {
    pub settings: AppSettings,
    settings_path: PathBuf,
    state: Arc<StateStore>,
    monitor: Option<Monitor>,
    external: External,
    charge_done: ChargeDoneWatcher,
    alerts: AlertManager,
    updates: Option<UpdateService>,
    update_events: Receiver<Event>,
    /// What the reminder shows (None: it is closed)
    alert: Option<Vec<AlertItem>>,
    main_open: bool,
    overlay_timed: bool,
    stopping: bool,
}

/// What the Swift app is to do: shown in order.
pub type Effects = Vec<Value>;

impl MacApp {
    /// Startup as on Windows: the settings (and the version that ran last), the language, the state, the polling and
    /// the update checks. `demo`: the devices of the pictures, with settings of their own.
    pub fn start(notify: Notify, demo: bool) -> (Self, Effects) {
        let folder = if demo { demo::folder() } else { sys::app_data_folder() };
        if demo {
            demo::prepare(&folder);
            log::set_folder(folder.clone());
        }
        if !demo {
            autostart::follow_this_copy();
        }
        let settings_path = folder.join("settings.json");
        let mut settings = AppSettings::load_from(&settings_path);
        batterytray::updater::clean_up();
        let version = Version::current().to_string();
        let updated_from = settings.last_run_version.clone().filter(|v| *v != version);
        settings.last_run_version = Some(version.clone());
        settings.save_to(&settings_path);
        loc::set(loc::resolve(Some(&settings.language)));
        let state = Arc::new(StateStore::load_from(folder.join("state.json")));

        let external = External::default();
        let mut list: Vec<Box<dyn Provider>> =
            if demo { vec![Box::new(demo::DemoProvider)] } else { providers::create(&state) };
        list.push(Box::new(ExternalProvider(external.clone())));
        let devices_notify = notify.clone();
        let monitor = Monitor::start(list, state.clone(), move || {
            // One update for any number of polls that finish before the main thread gets to it
            if !DEVICES_PENDING.swap(true, Ordering::AcqRel) {
                devices_notify();
            }
        });
        let (sender, update_events) = channel();
        let updates = UpdateService::start(settings.auto_update && !demo, move |event| {
            let _ = sender.send(event);
            notify();
        });
        log::write(&format!(
            "started v{version} on macOS {}{}",
            sys::macos_version(),
            updated_from.as_ref().map(|v| format!(", updated from v{v}")).unwrap_or_default()
        ));
        let app = Self {
            settings,
            settings_path,
            state,
            monitor: Some(monitor),
            external,
            charge_done: ChargeDoneWatcher::default(),
            alerts: AlertManager::default(),
            updates: Some(updates),
            update_events,
            alert: None,
            main_open: false,
            overlay_timed: false,
            stopping: false,
        };
        let mut effects = Vec::new();
        if updated_from.is_some() {
            effects.push(notice(&t("notice.updated.title"), &tf("notice.updated.text", &[&version]), false));
        }
        (app, effects)
    }

    /// Every device known: what the providers report now, plus devices only remembered from before.
    pub fn rows(&self) -> Vec<DeviceRow> {
        let current = self.monitor.as_ref().map(|m| m.current()).unwrap_or_default();
        let remembered: Vec<(String, Option<DeviceReading>)> =
            self.state.ids().into_iter().map(|id| (id.clone(), self.state.get(&id))).collect();
        rows::rows(current, &remembered, &self.settings)
    }

    fn update_status(&self) -> Status {
        self.updates.as_ref().map(|u| u.status()).unwrap_or(Status {
            stage: Stage::NotChecked,
            latest: None,
            last_check: None,
            progress: 0,
            error: None,
        })
    }

    pub fn save(&self) {
        self.settings.save_to(&self.settings_path);
    }

    /// A settings change: applied, shown everywhere, saved half a second after the last one (the Swift app's timer).
    pub fn edit(&mut self, change: impl FnOnce(&mut AppSettings)) -> Effects {
        change(&mut self.settings);
        self.settings_changed()
    }

    fn settings_changed(&mut self) -> Effects {
        if let Some(u) = &self.updates {
            u.set_auto_update(self.settings.auto_update);
        }
        let mut effects = vec![json!({ "type": "save_later" })];
        effects.extend(self.evaluate_alerts());
        effects
    }

    /// News from the polling threads and the update service.
    pub fn pump(&mut self) -> Effects {
        let mut effects = Vec::new();
        if DEVICES_PENDING.swap(false, Ordering::AcqRel) {
            effects.extend(self.on_devices_changed());
        }
        while let Ok(event) = self.update_events.try_recv() {
            match event {
                Event::Changed => {}
                // Announced once per version when it can't install by itself
                Event::Announce(release) => effects.push(notice(
                    &tf("notice.available.title", &[&release.version]),
                    &t("notice.available.text"),
                    false,
                )),
                Event::Installed(release) => {
                    effects.push(self.restart_into_update(&release));
                    break;
                }
            }
        }
        effects
    }

    fn on_devices_changed(&mut self) -> Effects {
        if self.stopping {
            return Vec::new();
        }
        let mut effects = Vec::new();
        // Once the user has ordered the list, a device seen for the first time goes last in its block
        let rows = self.rows();
        if rows::adopt_new(&mut self.settings, &rows) {
            effects.push(json!({ "type": "save_later" }));
        }
        effects.extend(self.evaluate_alerts());
        effects.extend(self.announce_charged());
        self.state.save_if_dirty();
        effects
    }

    /// A plain notification: unlike a low battery, nothing needs doing right away. With the system's sound chosen for
    /// it, the sound is the notification's own, for macOS to play; any other sound is the app's to play.
    fn announce_charged(&mut self) -> Effects {
        let rows = self.rows();
        let choice = self.settings.sound_for(sounds::Event::Charged);
        let mut effects = Vec::new();
        let mut announced = false;
        for row in self.charge_done.finished(&rows) {
            let level = row.current.as_ref().and_then(|c| c.level).unwrap_or(100);
            log::write(&format!("charged: {} {level}%", row.name));
            let ds = self.settings.device(&row.id);
            if !self.settings.notify_charged || ds.hidden || !ds.alerts {
                continue;
            }
            let text = tf("notice.charged.text", &[&row.name, &level]);
            effects.push(notice(&t("notice.charged.title"), &text, choice == Choice::System));
            announced = true;
        }
        if announced && choice != Choice::System {
            effects.extend(self.sound_effect(sounds::Event::Charged));
        }
        effects
    }

    /// The sound chosen for an event, for the app to play: the system's own for the event ("system"), or a file with
    /// the seconds it sounds - one of macOS' sounds by name, or one of the user's. None for no sound. A file that is
    /// gone is the system's sound.
    fn sound_effect(&self, event: sounds::Event) -> Option<Value> {
        let system = || json!({ "type": "sound", "event": event.name(), "system": true });
        let file = |path: PathBuf, length: Duration| {
            json!({
                "type": "sound", "event": event.name(), "path": path.display().to_string(),
                "seconds": length.as_secs_f64(),
            })
        };
        Some(match self.settings.sound_for(event) {
            Choice::None => return None,
            Choice::System => system(),
            Choice::Named(id) => match sounds::named_path(id).filter(|path| path.is_file()) {
                Some(path) => {
                    let length = sounds::plays_for(&path);
                    file(path, length)
                }
                None => {
                    log::write(&format!("macOS' sound {id} is gone: the system's sound instead"));
                    system()
                }
            },
            Choice::File(name) => {
                let path = sounds::path_of(&name);
                if path.is_file() {
                    let length = sounds::plays_for(&path);
                    file(path, length)
                } else {
                    log::write(&format!("sound {name} is gone: the system's sound instead"));
                    system()
                }
            }
        })
    }

    /// A sound chosen in the settings: kept, and played once so that the choice is heard.
    pub fn set_sound(&mut self, event: sounds::Event, setting: &str) -> Effects {
        let choice = Choice::parse(setting);
        let mut effects = self.edit(|s| s.set_sound(event, &choice));
        effects.extend(self.sound_effect(event));
        effects
    }

    /// A file picked for a sound in the open panel: copied into the Sounds folder and chosen. One the app can't take
    /// (not a WAV or an MP3, too big, unreadable) leaves the sound as it was, and the app says so.
    pub fn add_sound(&mut self, event: sounds::Event, file: &str) -> Effects {
        match sounds::import(std::path::Path::new(file)) {
            Ok(name) => self.set_sound(event, &Choice::File(name).setting()),
            Err(e) => {
                log::write(&format!("sound file not taken: {e:?}"));
                vec![json!({
                    "type": "sound_refused", "title": t("sounds.error.title"), "text": t("sounds.error.text"),
                })]
            }
        }
    }

    /// The reminder: opened, changed or closed as the devices and the user's answers have it; a sound when something
    /// new is due.
    pub fn evaluate_alerts(&mut self) -> Effects {
        if self.stopping || self.monitor.is_none() {
            return Vec::new();
        }
        let due = self.alerts.due(&self.rows(), &self.settings, Instant::now());
        if due.is_empty() {
            return self.close_alert().into_iter().collect();
        }
        let shown = self.alert.as_deref().unwrap_or_default();
        let fresh =
            self.alert.is_none() || due.iter().any(|d| !shown.iter().any(|s| s.id == d.id && s.critical == d.critical));
        let mut effects = vec![alert(&due, &self.settings, false)];
        if fresh {
            let names = due.iter().map(|d| format!("{} {}%", d.name, d.level)).collect::<Vec<_>>().join(", ");
            log::write(&format!("alert: {names}"));
            let critical = due.iter().any(|d| d.critical);
            effects.extend(self.sound_effect(if critical { sounds::Event::Critical } else { sounds::Event::Low }));
        }
        self.alert = Some(due);
        self.sync_window_open();
        effects
    }

    fn close_alert(&mut self) -> Option<Value> {
        let was = self.alert.take().is_some();
        self.sync_window_open();
        was.then(|| json!({ "type": "alert_close" }))
    }

    /// The answer "remind me in N min", or the reminder's time ran out (`on_its_own`): it comes back after N minutes
    /// either way.
    pub fn snooze(&mut self, on_its_own: bool) -> Effects {
        let ids: Vec<String> = self.alert.iter().flatten().map(|a| a.id.clone()).collect();
        let minutes = self.settings.repeat_minutes;
        self.alerts.snooze(&ids, Duration::from_secs(minutes as u64 * 60));
        if on_its_own {
            log::write(&format!("alert closed on its own, back in {minutes} min"));
        } else {
            log::write(&format!("alert snoozed for {minutes} min"));
        }
        self.close_alert().into_iter().collect()
    }

    pub fn mute(&mut self) -> Effects {
        let ids: Vec<String> = self.alert.iter().flatten().map(|a| a.id.clone()).collect();
        self.alerts.mute_until_charged(&ids);
        log::write("alert muted until charged");
        self.close_alert().into_iter().collect()
    }

    /// Each device at its own levels: the first one just below its reminder level, the second one critical.
    pub fn test_alert(&self) -> Effects {
        let rows = self.rows();
        let mut sample: Vec<AlertItem> = rows
            .iter()
            .filter(|r| r.online() && r.current.as_ref().is_some_and(|c| c.level.is_some()))
            .take(2)
            .enumerate()
            .map(|(i, r)| AlertItem {
                id: r.id.clone(),
                name: r.name.clone(),
                kind: r.kind(),
                level: (if i == 0 { self.settings.low_for(&r.id) - 2 } else { self.settings.critical_for(&r.id) - 1 })
                    .max(0),
                critical: i > 0,
            })
            .collect();
        if sample.is_empty() {
            sample.push(AlertItem {
                id: "test".into(),
                name: t("name.mouse"),
                kind: DeviceKind::Mouse,
                level: (self.settings.low_percent - 2).max(0),
                critical: false,
            });
        }
        let mut effects = vec![alert(&sample, &self.settings, true)];
        let critical = sample.iter().any(|s| s.critical);
        effects.extend(self.sound_effect(if critical { sounds::Event::Critical } else { sounds::Event::Low }));
        effects
    }

    /// Which of the app's windows are open: an automatic update waits while one is, so it never vanishes from under
    /// the user.
    pub fn set_windows(&mut self, main_open: bool, overlay_timed: bool) {
        self.main_open = main_open;
        self.overlay_timed = overlay_timed;
        self.sync_window_open();
    }

    fn sync_window_open(&self) {
        if let Some(u) = &self.updates {
            u.set_window_open(self.main_open || self.alert.is_some() || self.overlay_timed);
        }
    }

    pub fn refresh_now(&self) {
        if let Some(m) = &self.monitor {
            m.refresh_now();
        }
    }

    /// The system says a device came or went: the devices are asked again right away (see Monitor::devices_changed).
    pub fn devices_changed(&self) {
        if let Some(m) = &self.monitor {
            m.devices_changed();
        }
    }

    /// A theme picked in the settings: "light", "dark", or "auto" to follow the system. The app sets its appearance
    /// from the settings it is sent.
    pub fn set_theme(&mut self, setting: &str) -> Effects {
        self.settings.theme = setting.to_string();
        self.settings_changed()
    }

    pub fn set_language(&mut self, setting: &str) -> Effects {
        self.settings.language = setting.to_string();
        loc::set(loc::resolve(Some(setting)));
        let mut effects = self.settings_changed();
        // The reminder's texts are in the language too
        if let Some(due) = &self.alert {
            effects.push(alert(due, &self.settings, false));
        }
        self.refresh_now(); // the devices' details come from the providers
        effects
    }

    /// A name typed on a card or a row, as typed: kept without the spaces around it, empty for the device's own.
    pub fn rename(&mut self, id: &str, text: &str) -> Effects {
        if self.settings.set_name(id, text) {
            self.settings_changed()
        } else {
            Vec::new()
        }
    }

    pub fn pin(&mut self, id: &str, pinned: bool) -> Effects {
        let rows = self.rows();
        if rows::set_pinned(&mut self.settings, &rows, id, pinned) {
            self.settings_changed()
        } else {
            Vec::new()
        }
    }

    /// A device dropped onto another one's place in its block; refused across blocks.
    pub fn move_onto(&mut self, id: &str, onto: &str) -> Effects {
        let rows = self.rows();
        if rows::move_onto(&mut self.settings, &rows, id, onto) {
            self.settings_changed()
        } else {
            Vec::new()
        }
    }

    pub fn set_autostart(&mut self, on: bool) -> Effects {
        autostart::set(on);
        // Switched off by hand: the user knows it is off, and the banner has nothing to tell them
        if !on && !autostart::is_registered() && !self.settings.autostart_banner_dismissed {
            return self.edit(|s| s.autostart_banner_dismissed = true);
        }
        Vec::new()
    }

    /// What the Swift app reports from one source. A device another provider already lists with a level is not
    /// listed twice (the Bluetooth provider has AirPods, the Logitech one an MX mouse, the iPhone one the phone); a
    /// Bluetooth LE device that is also a game controller is the controller's.
    pub fn set_external(&self, source: &str, readings: Vec<DeviceReading>) {
        let external = |id: &str| id.starts_with("gc:") || id.starts_with("ble:");
        let mut known: Vec<String> = self
            .monitor
            .as_ref()
            .map(|m| m.current())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| !external(&r.id) && r.level.is_some())
            .map(|r| r.name.to_lowercase())
            .collect();
        if source == "ble" {
            let ours = self.external.readings.lock().unwrap_or_else(|e| e.into_inner());
            known.extend(ours.get("controllers").into_iter().flatten().map(|r| r.name.to_lowercase()));
        }
        // The same device under two names: "MX Ergo" over Bluetooth LE, "Logitech MX Ergo Multi-Device Trackball"
        // over HID++; a name too short to tell anything ("Mouse") has to match whole
        let listed = |name: &str| {
            let name = name.to_lowercase();
            known
                .iter()
                .any(|k| *k == name || (name.len() >= 5 && k.len() >= 5 && (k.contains(&name) || name.contains(k))))
        };
        self.external.set(source, readings.into_iter().filter(|r| !listed(&r.name)).collect());
    }

    pub fn updates_install(&self) {
        if let Some(u) = &self.updates {
            u.install_now();
        }
    }

    pub fn updates_check(&self) {
        if let Some(u) = &self.updates {
            u.check_now();
        }
    }

    /// The new app is in place: the Swift app starts it and steps aside, once this one has let everything go.
    fn restart_into_update(&mut self, release: &ReleaseInfo) -> Value {
        log::write(&format!("restarting into v{}", release.version));
        let reopen = self.main_open;
        self.tear_down();
        json!({ "type": "restart", "background": !reopen })
    }

    pub fn tear_down(&mut self) {
        if self.stopping {
            return;
        }
        self.stopping = true;
        if let Some(monitor) = self.monitor.take() {
            monitor.stop();
        }
        self.updates = None;
        self.state.save();
        self.save();
        self.alert = None;
    }

    /// Everything the windows and the menu bar show.
    pub fn snapshot(&self) -> Value {
        let rows = self.rows();
        let s = &self.settings;
        let lists = view::device_lists(&rows, s, chrono::Local::now()).map(|d| device(&d));
        let tray = view::tray_view(&rows, s);
        let menu: Vec<Value> = rows::list_order(&rows, s)
            .into_iter()
            .filter(|r| r.online() && !s.device(&r.id).hidden)
            .filter_map(|r| {
                let c = r.current.as_ref()?;
                let level = match c.level {
                    Some(l) => format!("{}{l}%", if c.estimate { "≈" } else { "" }),
                    None if c.charging => String::new(),
                    None => "—".into(),
                };
                Some(json!({
                    "name": r.name,
                    "kind": kind(r.kind()),
                    "level": level,
                    "tone": view::level_tone(c.level, c.charging, true, &r.id, s).name(),
                    "charging": c.charging,
                }))
            })
            .collect();
        let update = view::update_view(&self.update_status());
        let overlay: Vec<Value> = view::overlay_entries(&rows, s)
            .into_iter()
            .map(|e| {
                json!({
                    "kind": kind(e.kind), "name": e.name, "show_name": e.show_name, "level": e.level,
                    "tone": e.tone.name(), "power": e.power,
                })
            })
            .collect();
        let bar: Vec<Value> = tray
            .bar
            .iter()
            .map(|b| json!({ "kind": kind(b.kind), "level": b.level, "tone": b.tone.name(), "power": b.power }))
            .collect();
        let needs = t("d.needsAdmin");
        let needs_permission = !hid::input_monitoring_allowed()
            && rows.iter().any(|r| r.current.as_ref().and_then(|c| c.detail.as_deref()) == Some(needs.as_str()));
        // The sounds a menu offers, in its three parts: the system's and silence, macOS' sounds by name, the user's files
        let sound_list = sounds::choices(&sounds::files());
        let sound_menu: Vec<Value> = sound_list
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let group = sounds::BREAKS.iter().filter(|&&b| i >= b).count();
                json!({ "id": c.setting(), "name": c.label(), "group": group })
            })
            .collect();
        // A file that is gone from the folder isn't in the menu: its row shows the system's sound, which it plays
        let chosen = |event: sounds::Event| {
            let choice = s.sound_for(event);
            let shown = if sound_list.contains(&choice) { choice } else { Choice::System };
            shown.setting()
        };
        // Under a row, when only the beginning of its file will sound
        let sound_notes: Vec<String> = sounds::Event::ALL
            .iter()
            .map(|&event| match s.sound_for(event) {
                Choice::File(name) if sounds::is_cut(&sounds::path_of(&name)) => t("sounds.long"),
                _ => String::new(),
            })
            .collect();
        json!({
            "lang": loc::current(),
            "version": Version::current().to_string(),
            "devices": {
                "pinned": lists.pinned, "connected": lists.connected, "offline": lists.offline,
                "hidden": lists.hidden, "none_online": lists.none_online,
            },
            "updated_text": view::updated_text(&rows),
            "tray": {
                "label": tray.label, "tone": tray.tone.name(), "tooltip": tray.tooltip, "items": menu, "bar": bar,
            },
            "settings": {
                "low": s.low_percent, "critical": s.critical_percent, "repeat": s.repeat_minutes,
                "sound_low": chosen(sounds::Event::Low), "sound_critical": chosen(sounds::Event::Critical),
                "sound_charged": chosen(sounds::Event::Charged), "sound_notes": sound_notes,
                "notify_charged": s.notify_charged, "auto_close": s.auto_close,
                "auto_close_seconds": s.auto_close_seconds, "auto_update": s.auto_update,
                "overlay_enabled": s.overlay_enabled, "overlay_mode": s.overlay_mode,
                "overlay_corner": s.overlay_corner, "overlay_names": s.overlay_show_names,
                "menu_bar_devices": s.menu_bar_devices, "tray_battery_icon": s.tray_battery_icon,
                "language": s.language, "theme": s.theme, "tray_priority": s.tray_priority, "hide_offline": s.hide_offline,
                "autostart_banner_dismissed": s.autostart_banner_dismissed,
            },
            "autostart": autostart::is_registered(),
            "update": {
                "status": update.status, "checked": update.checked, "install_visible": update.install_visible,
                "install_text": update.install_text, "check_enabled": update.check_enabled, "notes": update.notes,
                "banner_visible": update.banner_visible, "banner_text": update.banner_text,
                "banner_button_visible": update.banner_button_visible,
            },
            "overlay": { "entries": overlay, "shortcut": shortcut(s.overlay_hotkey_key, s.overlay_hotkey_modifiers) },
            "sounds": sound_menu,
            "needs_permission": needs_permission,
        })
    }

    /// A shortcut pressed in the recorder (Mac key code and modifiers): whether the overlay may have it.
    pub fn check_shortcut(&self, mac_key: u16, modifiers: i32) -> Value {
        match keys::virtual_key(mac_key).filter(|&vk| hotkey_valid(vk, modifiers)) {
            Some(vk) => json!({ "valid": true, "shortcut": shortcut(vk, modifiers) }),
            None => json!({ "valid": false, "error": t("overlay.invalid") }),
        }
    }

    /// A shortcut the Swift app has registered: kept.
    pub fn set_shortcut(&mut self, key: i32, modifiers: i32) -> Effects {
        if !hotkey_valid(key, modifiers) {
            return Vec::new();
        }
        self.edit(|s| {
            s.overlay_hotkey_key = key;
            s.overlay_hotkey_modifiers = modifiers;
        })
    }

    pub fn default_shortcut() -> Value {
        shortcut(DEFAULT_HOTKEY.0, DEFAULT_HOTKEY.1)
    }

    /// The "Suggest an improvement" link, in the app's language.
    pub fn suggestion_url() -> String {
        feedback::issue_url_macos(&Version::current().to_string(), &sys::macos_version(), loc::current())
    }
}

/// A notification; `system_sound`: with the sound macOS gives its notifications (the others are silent, as the app's
/// notices always were).
fn notice(title: &str, text: &str, system_sound: bool) -> Value {
    json!({ "type": "notice", "title": title, "text": text, "system_sound": system_sound })
}

/// The reminder with these devices; `test`: the sample, which a click only closes. "auto_close": the seconds after
/// which it closes on its own as if snoozed, null while it waits for an answer.
fn alert(due: &[AlertItem], settings: &AppSettings, test: bool) -> Value {
    let critical = due.iter().any(|d| d.critical);
    let items: Vec<Value> = due
        .iter()
        .map(|d| {
            json!({
                "id": d.id, "name": d.name, "kind": kind(d.kind), "level": format!("{}%", d.level),
                "percent": d.level, "critical": d.critical,
            })
        })
        .collect();
    json!({
        "type": "alert",
        "test": test,
        "critical": critical,
        "snooze": tf("alert.snooze", &[&settings.repeat_minutes]),
        "auto_close": settings.auto_close_after().map(|d| d.as_secs()),
        "items": items,
    })
}

fn device(d: &DeviceView) -> Value {
    json!({
        "id": d.id, "name": d.name, "kind": kind(d.kind), "subtitle": d.subtitle, "connected": d.connected,
        "pinned": d.pinned, "level_text": d.level_text, "tone": d.tone.name(), "charging": d.charging,
        "indeterminate": d.indeterminate, "fill": d.fill, "custom_name": d.custom_name, "own_name": d.own_name,
        "source": d.source, "alerts": d.alerts, "low": d.low, "critical": d.critical, "own_levels": d.own_levels,
    })
}

fn shortcut(key: i32, modifiers: i32) -> Value {
    json!({
        "key": key,
        "modifiers": modifiers,
        "mac_key": keys::mac_key(key),
        "carbon_modifiers": keys::carbon_modifiers(modifiers),
        "caps": keys::parts(key, modifiers),
        "label": keys::label(key, modifiers),
    })
}

pub fn kind(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Headset => "headset",
        DeviceKind::Mouse => "mouse",
        DeviceKind::Keyboard => "keyboard",
        DeviceKind::Gamepad => "gamepad",
        DeviceKind::Other => "other",
        DeviceKind::Phone => "phone",
        DeviceKind::Tablet => "tablet",
    }
}
