//! User settings in %LOCALAPPDATA%\BatteryTray\settings.json: the same file and names as the C#
//! app, read as forgivingly. Hand edits tend to leave a number in quotes, a comma after the last value or a //
//! comment, which are all taken. A value of the wrong type costs only itself: the rest is read value by value, and
//! the file as it was is kept as settings.json.bak before the next save writes over it. Values this version doesn't
//! know are kept as they are, so going back and forth between versions loses nothing.

use crate::log;
use crate::sounds::{Choice, Event};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

pub const AUTO: &str = "auto";

/// Win32 hotkey modifiers, without the registration-only MOD_NOREPEAT flag. A shortcut is kept as a Win32 virtual key
/// and these on macOS too, where Alt is ⌥ (Option), Ctrl is ⌃ (Control) and MOD_WIN is ⌘ (Command).
pub const MOD_ALT: i32 = 0x0001;
pub const MOD_CONTROL: i32 = 0x0002;
pub const MOD_SHIFT: i32 = 0x0004;
pub const MOD_WIN: i32 = 0x0008;
/// Ctrl+Alt+V (⌃⌥V on macOS)
pub const DEFAULT_HOTKEY: (i32, i32) = (0x56, MOD_ALT | MOD_CONTROL);
/// What the fields in Settings > Reminders take (RemindersPage in ui/settings.slint, ReminderSettings in
/// macos/BatteryTray/SettingsWindow.swift; a device's levels in its menu too). Their sliders cover the usual part of it
/// (reminder level 10-50%, critical 5-45%, repeat 1-60 min, closing 3-15 s) and stand at an end for a typed value past
/// it. The critical level is never above the reminder level. The repeat and the closing have no real end: 99 999
/// minutes or seconds is the most a field holds.
pub const LEVEL_PERCENT: (i32, i32) = (1, 100);
pub const REPEAT_MINUTES: (i32, i32) = (1, 99_999);
pub const AUTO_CLOSE_SECONDS: (i32, i32) = (1, 99_999);
/// How many devices the menu bar item on macOS may show (Settings > General > Menu bar).
pub const MENU_BAR_DEVICES: (i32, i32) = (1, 5);
/// A sound setting's default: the system's own sound, as the app always had.
const SYSTEM_SOUND: &str = "system";

// A `&String`: what serde hands to `skip_serializing_if`
#[allow(clippy::ptr_arg)]
fn is_system_sound(setting: &String) -> bool {
    setting == SYSTEM_SOUND
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeviceSettings {
    /// Custom display name; None = the name the device reports.
    pub name: Option<String>,
    pub hidden: bool,
    pub alerts: bool,
    /// Reminder level for this device; None = the global one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low_percent: Option<i32>,
    /// Critical level for this device; None = the global one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical_percent: Option<i32>,
    /// Kept in the top block of the list, connected or not. Written only when set, as the levels are: a device
    /// nobody pinned looks in the file as it always did.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            name: None,
            hidden: false,
            alerts: true,
            low_percent: None,
            critical_percent: None,
            pinned: false,
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AppSettings {
    pub low_percent: i32,
    pub critical_percent: i32,
    pub repeat_minutes: i32,
    /// Up to 2.3.0 the switch of the reminder's sound. Since then each sound is chosen by itself (below); this is
    /// written for those versions, false when the reminder has no sound at either level, and read from a file that
    /// doesn't name the sounds yet.
    pub sound: bool,
    /// What sounds when the reminder comes at the reminder level, when it comes at the critical level, and with the
    /// notice that a device has finished charging: "system", "none", one of the system's sounds by name or "file:<name>" for
    /// a file in the Sounds folder (sounds.rs). Written only when it isn't the system's sound: a file of someone who
    /// never chose a sound looks as it always did.
    #[serde(skip_serializing_if = "is_system_sound")]
    pub sound_low: String,
    #[serde(skip_serializing_if = "is_system_sound")]
    pub sound_critical: String,
    #[serde(skip_serializing_if = "is_system_sound")]
    pub sound_charged: String,
    /// "ru", "en", "zh", or "auto" to follow Windows.
    pub language: String,
    /// "light", "dark", or "auto" to follow the system's mode.
    pub theme: String,
    /// Which device kind the tray icon prefers when no device is low: "mouse", "keyboard", "bluetooth", "headset", or "lowest".
    pub tray_priority: String,
    pub tray_battery_icon: bool,
    /// Hide disconnected / asleep (inactive) devices from the device list.
    pub hide_offline: bool,
    /// A Windows notification when a device finishes charging.
    pub notify_charged: bool,
    /// The low battery reminder closes on its own after `auto_close_seconds`, as if snoozed (the default); off, it
    /// waits for an answer.
    pub auto_close: bool,
    pub auto_close_seconds: i32,
    pub auto_update: bool,
    pub overlay_enabled: bool,
    pub overlay_hotkey_key: i32,
    pub overlay_hotkey_modifiers: i32,
    /// 0 = shown for 5 seconds, 1 = toggled by the shortcut.
    pub overlay_mode: i32,
    /// 0 top left, 1 top right, 2 bottom left, 3 bottom right.
    pub overlay_corner: i32,
    pub overlay_show_names: bool,
    /// macOS: how many devices the menu bar item shows (MENU_BAR_DEVICES); fewer when fewer are on the air.
    pub menu_bar_devices: i32,
    /// Version that ran last time; a different one means the app was just updated.
    pub last_run_version: Option<String>,
    pub devices: BTreeMap<String, DeviceSettings>,
    /// The banner that offers to start with Windows has been answered ("don't show again", or autostart turned off
    /// by hand): it stays away.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub autostart_banner_dismissed: bool,
    /// The banner that offers admin rights when Windows keeps a device from the app: "don't show again".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub admin_banner_dismissed: bool,
    /// The devices in the order the user put them in; the list shows the pinned ones of them first. Empty until the
    /// first pin or drag: until then the list goes by kind and name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub device_order: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            low_percent: 20,
            critical_percent: 10,
            repeat_minutes: 10,
            sound: true,
            sound_low: SYSTEM_SOUND.into(),
            sound_critical: SYSTEM_SOUND.into(),
            sound_charged: SYSTEM_SOUND.into(),
            language: AUTO.into(),
            theme: AUTO.into(),
            tray_priority: "mouse".into(),
            tray_battery_icon: true,
            hide_offline: false,
            notify_charged: true,
            auto_close: true,
            auto_close_seconds: 10,
            auto_update: true,
            overlay_enabled: false,
            overlay_hotkey_key: DEFAULT_HOTKEY.0,
            overlay_hotkey_modifiers: DEFAULT_HOTKEY.1,
            overlay_mode: 0,
            overlay_corner: 1,
            overlay_show_names: false,
            menu_bar_devices: 1,
            last_run_version: None,
            devices: BTreeMap::new(),
            autostart_banner_dismissed: false,
            admin_banner_dismissed: false,
            device_order: Vec::new(),
            extra: Map::new(),
        }
    }
}

/// A shortcut the overlay may use: Ctrl or Alt with an ordinary key, or a function key alone (not F12); the
/// clipboard shortcuts and Alt+F4 stay with the focused app.
pub fn hotkey_valid(key: i32, modifiers: i32) -> bool {
    if cfg!(target_os = "macos") {
        macos_hotkey_valid(key, modifiers)
    } else {
        windows_hotkey_valid(key, modifiers)
    }
}

/// Windows: an ordinary key needs Ctrl or Alt (an F key goes alone); the Windows keys, F12 (the debugger's), Ctrl+C,
/// Ctrl+V, Ctrl+X and Alt+F4 stay the system's.
pub fn windows_hotkey_valid(key: i32, modifiers: i32) -> bool {
    (modifiers & !(MOD_ALT | MOD_CONTROL | MOD_SHIFT)) == 0
        && (0x08..=0xFE).contains(&key)
        && ![0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0x5D, 0x7B, 0x90, 0x91].contains(&key)
        && !(0xA0..=0xA5).contains(&key)
        && ((modifiers & (MOD_ALT | MOD_CONTROL)) != 0 || (0x70..=0x87).contains(&key))
        && !(modifiers == MOD_CONTROL && [0x43, 0x56, 0x58].contains(&key))
        && !(modifiers == MOD_ALT && key == 0x73)
}

/// macOS: an ordinary key needs ⌘, ⌃ or ⌥ (an F key goes alone, F12 too); what every Mac app and the system answer
/// stays theirs - ⌘Q, ⌘W, ⌘H, ⌘M, ⌘C, ⌘V, ⌘X, ⌘Z, ⌘A, ⌘Tab, ⌘`, ⌘Space, ⌃Space, ⇧⌘3/4/5 and ⌥⌘Esc.
pub fn macos_hotkey_valid(key: i32, modifiers: i32) -> bool {
    const COMMAND_KEYS: [i32; 12] = [0x51, 0x57, 0x48, 0x4D, 0x43, 0x56, 0x58, 0x5A, 0x41, 0x09, 0xC0, 0x20];
    (modifiers & !(MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN)) == 0
        && (0x08..=0xFE).contains(&key)
        && ![0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0x5D, 0x90, 0x91].contains(&key)
        && !(0xA0..=0xA5).contains(&key)
        && ((modifiers & (MOD_ALT | MOD_CONTROL | MOD_WIN)) != 0 || (0x70..=0x87).contains(&key))
        && !(modifiers == MOD_WIN && COMMAND_KEYS.contains(&key))
        && !(modifiers == MOD_CONTROL && key == 0x20)
        && !(modifiers == MOD_WIN | MOD_SHIFT && [0x33, 0x34, 0x35].contains(&key))
        && !(modifiers == MOD_WIN | MOD_ALT && key == 0x1B)
}

impl AppSettings {
    pub fn path() -> PathBuf {
        crate::sys::app_data_folder().join("settings.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &PathBuf) -> Self {
        let mut s = match fs::read_to_string(path) {
            Err(_) if !path.exists() => Self::default(),
            Err(e) => {
                log::write(&format!("settings.json unreadable, using defaults: {e}"));
                keep_copy(path);
                Self::default()
            }
            Ok(text) => match parse_lenient(text.trim_start_matches('\u{FEFF}')) {
                None => {
                    log::write("settings.json unreadable, using defaults: not JSON");
                    keep_copy(path);
                    Self::default()
                }
                Some(value) => {
                    let (s, clean) = Self::from_value(&value);
                    if !clean {
                        keep_copy(path);
                    }
                    s
                }
            },
        };
        s.normalize();
        s
    }

    /// The settings in a JSON value, and whether every value in it was read.
    pub fn from_value(value: &Value) -> (Self, bool) {
        let mut s = Self::default();
        let Value::Object(root) = value else {
            log::write("settings.json: no settings object in the file");
            return (s, false);
        };
        let mut clean = true;
        for (key, v) in root {
            let ok = match key.as_str() {
                "LowPercent" => set(&mut s.low_percent, int(v)),
                "CriticalPercent" => set(&mut s.critical_percent, int(v)),
                "RepeatMinutes" => set(&mut s.repeat_minutes, int(v)),
                "Sound" => set(&mut s.sound, v.as_bool()),
                "SoundLow" => set(&mut s.sound_low, v.as_str().map(str::to_string)),
                "SoundCritical" => set(&mut s.sound_critical, v.as_str().map(str::to_string)),
                "SoundCharged" => set(&mut s.sound_charged, v.as_str().map(str::to_string)),
                "Language" => set(&mut s.language, v.as_str().map(str::to_string)),
                "Theme" => set(&mut s.theme, v.as_str().map(str::to_string)),
                "TrayPriority" => set(&mut s.tray_priority, v.as_str().map(str::to_string)),
                "TrayBatteryIcon" => set(&mut s.tray_battery_icon, v.as_bool()),
                "HideOffline" => set(&mut s.hide_offline, v.as_bool()),
                "NotifyCharged" => set(&mut s.notify_charged, v.as_bool()),
                "AutoClose" => set(&mut s.auto_close, v.as_bool()),
                "AutoCloseSeconds" => set(&mut s.auto_close_seconds, int(v)),
                "AutoUpdate" => set(&mut s.auto_update, v.as_bool()),
                "OverlayEnabled" => set(&mut s.overlay_enabled, v.as_bool()),
                "OverlayHotkeyKey" => set(&mut s.overlay_hotkey_key, int(v)),
                "OverlayHotkeyModifiers" => set(&mut s.overlay_hotkey_modifiers, int(v)),
                "OverlayMode" => set(&mut s.overlay_mode, int(v)),
                "OverlayCorner" => set(&mut s.overlay_corner, int(v)),
                "OverlayShowNames" => set(&mut s.overlay_show_names, v.as_bool()),
                "MenuBarDevices" => set(&mut s.menu_bar_devices, int(v)),
                "LastRunVersion" => match v {
                    Value::Null => true,
                    _ => set(&mut s.last_run_version, v.as_str().map(|t| Some(t.to_string()))),
                },
                "Devices" => match v {
                    Value::Object(devices) => {
                        for (id, d) in devices {
                            // A null or a number: skipped
                            let Value::Object(fields) = d else {
                                clean = false;
                                continue;
                            };
                            let (device, device_clean) = device_from(fields);
                            clean &= device_clean;
                            s.devices.insert(id.clone(), device);
                        }
                        true
                    }
                    _ => false,
                },
                "AutostartBannerDismissed" => set(&mut s.autostart_banner_dismissed, v.as_bool()),
                "AdminBannerDismissed" => set(&mut s.admin_banner_dismissed, v.as_bool()),
                "DeviceOrder" => match v {
                    Value::Array(ids) => {
                        for id in ids {
                            // A null or a number: skipped
                            match id.as_str() {
                                Some(id) => s.device_order.push(id.to_string()),
                                None => clean = false,
                            }
                        }
                        true
                    }
                    _ => false,
                },
                _ => {
                    s.extra.insert(key.clone(), v.clone());
                    true
                }
            };
            if !ok {
                // A null or a value of the wrong type for this setting: its default stays
                log::write(&format!("settings.json: {key} ignored"));
                clean = false;
            }
        }
        // A file from before the sounds could be chosen, with the reminder's sound switched off: the reminder stays
        // silent at both levels. The "charged" notice keeps its sound: the switch never was about it.
        if !s.sound && !root.contains_key("SoundLow") && !root.contains_key("SoundCritical") {
            s.sound_low = Choice::None.setting();
            s.sound_critical = Choice::None.setting();
        }
        (s, clean)
    }

    /// Keeps every level and choice within what the app offers (as the C# app does on load).
    pub fn normalize(&mut self) {
        self.low_percent = self.low_percent.clamp(LEVEL_PERCENT.0, LEVEL_PERCENT.1);
        self.critical_percent = self.critical_percent.clamp(LEVEL_PERCENT.0, self.low_percent);
        self.repeat_minutes = self.repeat_minutes.clamp(REPEAT_MINUTES.0, REPEAT_MINUTES.1);
        self.auto_close_seconds = self.auto_close_seconds.clamp(AUTO_CLOSE_SECONDS.0, AUTO_CLOSE_SECONDS.1);
        if !hotkey_valid(self.overlay_hotkey_key, self.overlay_hotkey_modifiers) {
            (self.overlay_hotkey_key, self.overlay_hotkey_modifiers) = DEFAULT_HOTKEY;
        }
        if !(0..=1).contains(&self.overlay_mode) {
            self.overlay_mode = 0;
        }
        if !(0..=3).contains(&self.overlay_corner) {
            self.overlay_corner = 1;
        }
        // A sound is one the app knows; anything else (a hand edit, a later version's sound) is the system's
        for sound in [&mut self.sound_low, &mut self.sound_critical, &mut self.sound_charged] {
            *sound = Choice::parse(sound).setting();
        }
        self.sound = self.reminder_sounds();
        self.menu_bar_devices = self.menu_bar_devices.clamp(MENU_BAR_DEVICES.0, MENU_BAR_DEVICES.1);
        if !matches!(self.tray_priority.as_str(), "mouse" | "keyboard" | "bluetooth" | "headset" | "lowest") {
            self.tray_priority = "mouse".into();
        }
        // A device has both of its own levels or none: one set alone (by hand) gets the other from the global ones
        let (low, critical) = (self.low_percent, self.critical_percent);
        for d in self.devices.values_mut() {
            if d.low_percent.is_none() && d.critical_percent.is_none() {
                continue;
            }
            let own_low = d.low_percent.unwrap_or(low).clamp(LEVEL_PERCENT.0, LEVEL_PERCENT.1);
            d.low_percent = Some(own_low);
            d.critical_percent = Some(d.critical_percent.unwrap_or(critical).clamp(LEVEL_PERCENT.0, own_low));
        }
        // A device has one place in the user's order: named twice (by hand), the first one counts
        let mut seen = HashSet::new();
        self.device_order.retain(|id| seen.insert(id.clone()));
    }

    pub fn save(&self) {
        self.save_to(&Self::path());
    }

    pub fn save_to(&self, path: &PathBuf) {
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
            let tmp = path.with_extension("json.tmp");
            fs::write(&tmp, text)?;
            fs::rename(&tmp, path)
        })();
        if let Err(e) = result {
            log::write(&format!("cannot save settings: {e}"));
        }
    }

    /// A device's settings, or the defaults when it has none.
    pub fn device(&self, id: &str) -> DeviceSettings {
        self.devices.get(id).cloned().unwrap_or_default()
    }

    pub fn device_mut(&mut self, id: &str) -> &mut DeviceSettings {
        self.devices.entry(id.to_string()).or_default()
    }

    /// The reminder level a device actually alerts at: its own when set, the global one otherwise.
    pub fn low_for(&self, id: &str) -> i32 {
        self.devices.get(id).and_then(|d| d.low_percent).unwrap_or(self.low_percent)
    }

    /// The critical level a device actually turns red at: its own when set, the global one otherwise. Never above the
    /// level it alerts at, so a low custom level can't fire critical first.
    pub fn critical_for(&self, id: &str) -> i32 {
        self.devices.get(id).and_then(|d| d.critical_percent).unwrap_or(self.critical_percent).min(self.low_for(id))
    }

    pub fn display_name(&self, id: &str, fallback: &str) -> String {
        match self.devices.get(id).and_then(|d| d.name.as_deref()).map(str::trim) {
            Some(custom) if !custom.is_empty() => custom.to_string(),
            _ => fallback.to_string(),
        }
    }

    /// Asked of every row at every refresh, so it never adds an entry for a device, as `device` doesn't.
    pub fn is_pinned(&self, id: &str) -> bool {
        self.devices.get(id).is_some_and(|d| d.pinned)
    }

    /// The name the user gives a device, without the spaces around it; an empty one gives the device's own name back.
    /// False when that is the name it has already: a rename left as it was writes nothing, not even an entry for a
    /// device the file doesn't name yet.
    pub fn set_name(&mut self, id: &str, text: &str) -> bool {
        let name = text.trim();
        let stored = self.devices.get(id).and_then(|d| d.name.as_deref()).unwrap_or("");
        if name == stored {
            return false;
        }
        self.device_mut(id).name = (!name.is_empty()).then(|| name.to_string());
        true
    }

    /// The general reminder level; the critical one goes no higher, pulled down along.
    pub fn set_low(&mut self, value: i32) {
        self.low_percent = value.clamp(LEVEL_PERCENT.0, LEVEL_PERCENT.1);
        self.critical_percent = self.critical_percent.min(self.low_percent);
    }

    /// The general critical level; the reminder level goes up along when it would be lower.
    pub fn set_critical(&mut self, value: i32) {
        self.critical_percent = value.clamp(LEVEL_PERCENT.0, LEVEL_PERCENT.1);
        self.low_percent = self.low_percent.max(self.critical_percent);
    }

    pub fn set_repeat(&mut self, minutes: i32) {
        self.repeat_minutes = minutes.clamp(REPEAT_MINUTES.0, REPEAT_MINUTES.1);
    }

    pub fn set_auto_close_seconds(&mut self, seconds: i32) {
        self.auto_close_seconds = seconds.clamp(AUTO_CLOSE_SECONDS.0, AUTO_CLOSE_SECONDS.1);
    }

    /// The sound chosen for an event.
    pub fn sound_for(&self, event: Event) -> Choice {
        Choice::parse(match event {
            Event::Low => &self.sound_low,
            Event::Critical => &self.sound_critical,
            Event::Charged => &self.sound_charged,
        })
    }

    pub fn set_sound(&mut self, event: Event, choice: &Choice) {
        let setting = choice.setting();
        match event {
            Event::Low => self.sound_low = setting,
            Event::Critical => self.sound_critical = setting,
            Event::Charged => self.sound_charged = setting,
        }
        self.sound = self.reminder_sounds();
    }

    /// What the versions up to 2.3.0 read as the reminder's sound switch: off when neither level has a sound.
    fn reminder_sounds(&self) -> bool {
        self.sound_for(Event::Low) != Choice::None || self.sound_for(Event::Critical) != Choice::None
    }

    /// How long the reminder stays before it closes on its own; None while it waits for an answer.
    pub fn auto_close_after(&self) -> Option<std::time::Duration> {
        self.auto_close.then(|| std::time::Duration::from_secs(self.auto_close_seconds as u64))
    }

    /// A reminder level entered for one device. It becomes the device's own, and the critical level with it: the one
    /// in effect until now, pulled down when it would be higher. Entering the level the device has already changes
    /// nothing, so a device that follows the general levels goes on following them when the focus only passes through
    /// its field.
    pub fn set_device_low(&mut self, id: &str, value: i32) {
        let (was, critical) = (self.low_for(id), self.critical_for(id));
        let low = value.clamp(LEVEL_PERCENT.0, LEVEL_PERCENT.1);
        if low == was {
            return;
        }
        let d = self.device_mut(id);
        d.low_percent = Some(low);
        d.critical_percent = Some(critical.min(low));
    }

    /// A critical level entered for one device: no higher than its reminder level, which becomes the device's own with
    /// it. Entering the level the device has already changes nothing.
    pub fn set_device_critical(&mut self, id: &str, value: i32) {
        let (low, was) = (self.low_for(id), self.critical_for(id));
        let critical = value.min(low).max(LEVEL_PERCENT.0);
        if critical == was {
            return;
        }
        let d = self.device_mut(id);
        d.low_percent = Some(low);
        d.critical_percent = Some(critical);
    }

    /// Back to the general levels.
    pub fn reset_device_levels(&mut self, id: &str) {
        let d = self.device_mut(id);
        d.low_percent = None;
        d.critical_percent = None;
    }
}

fn device_from(fields: &Map<String, Value>) -> (DeviceSettings, bool) {
    let mut d = DeviceSettings::default();
    let mut clean = true;
    for (key, v) in fields {
        let ok = match key.as_str() {
            "Name" => match v {
                Value::Null => true,
                _ => set(&mut d.name, v.as_str().map(|t| Some(t.to_string()))),
            },
            "Hidden" => set(&mut d.hidden, v.as_bool()),
            "Alerts" => set(&mut d.alerts, v.as_bool()),
            "LowPercent" => v.is_null() || set(&mut d.low_percent, int(v).map(Some)),
            "CriticalPercent" => v.is_null() || set(&mut d.critical_percent, int(v).map(Some)),
            "Pinned" => set(&mut d.pinned, v.as_bool()),
            _ => {
                d.extra.insert(key.clone(), v.clone());
                true
            }
        };
        if !ok {
            log::write(&format!("settings.json: a device's {key} ignored"));
            clean = false;
        }
    }
    (d, clean)
}

fn set<T>(target: &mut T, value: Option<T>) -> bool {
    match value {
        Some(v) => {
            *target = v;
            true
        }
        None => false,
    }
}

/// A whole number, also one written in quotes.
fn int(v: &Value) -> Option<i32> {
    match v {
        Value::Number(n) => n.as_i64().and_then(|n| i32::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The file as it was, kept before the next save writes over it, to fix by hand.
fn keep_copy(path: &PathBuf) {
    let backup = PathBuf::from(format!("{}.bak", path.display()));
    match fs::copy(path, &backup) {
        Ok(_) => log::write("settings.json as it was: settings.json.bak"),
        Err(e) => log::write(&format!("cannot keep settings.json.bak: {e}")),
    }
}

/// JSON, also with // and /* */ comments and a comma after the last value.
pub fn parse_lenient(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok().or_else(|| serde_json::from_str(&strip_extras(text)).ok())
}

/// Comments out first, then the commas they may have stood between and a closing bracket.
fn strip_extras(text: &str) -> String {
    drop_trailing_commas(&strip_comments(text))
}

/// Copies JSON text, letting `outside` decide what happens to what isn't inside a string: it gets the characters and
/// the position, writes what it keeps, and returns where to go on.
fn rewrite(text: &str, mut outside: impl FnMut(&[char], usize, &mut String) -> usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
        } else if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
        } else {
            i = outside(&chars, i, &mut out);
        }
    }
    out
}

fn strip_comments(text: &str) -> String {
    rewrite(text, |chars, mut i, out| {
        match (chars[i], chars.get(i + 1)) {
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
        i
    })
}

fn drop_trailing_commas(text: &str) -> String {
    rewrite(text, |chars, i, out| {
        // A comma with nothing but whitespace between it and a closing bracket goes
        let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
        if chars[i] != ',' || !matches!(next, Some('}') | Some(']')) {
            out.push(chars[i]);
        }
        i + 1
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_csharp_app_writes() {
        let text = r#"{
  "LowPercent": 25,
  "CriticalPercent": 10,
  "RepeatMinutes": 10,
  "Sound": true,
  "Language": "ru",
  "NotifyCharged": true,
  "AutoUpdate": true,
  "OverlayEnabled": true,
  "OverlayHotkeyKey": 66,
  "OverlayHotkeyModifiers": 3,
  "OverlayMode": 1,
  "OverlayCorner": 3,
  "OverlayShowNames": false,
  "LastRunVersion": "1.8.6",
  "Devices": {
    "razer:barracuda-x": { "Name": "Уши", "Hidden": false, "Alerts": true, "LowPercent": 30, "CriticalPercent": 15 },
    "ry:3151:402d:mouse": { "Name": null, "Hidden": true, "Alerts": false }
  }
}"#;
        let (s, clean) = AppSettings::from_value(&parse_lenient(text).unwrap());
        assert!(clean);
        assert_eq!((s.low_percent, s.language.as_str(), s.overlay_corner), (25, "ru", 3));
        assert_eq!(s.last_run_version.as_deref(), Some("1.8.6"));
        assert_eq!(s.display_name("razer:barracuda-x", "x"), "Уши");
        assert_eq!((s.low_for("razer:barracuda-x"), s.critical_for("razer:barracuda-x")), (30, 15));
        assert!(s.device("ry:3151:402d:mouse").hidden);

        // Written back the same way: names, null name, no per-device levels where there were none
        let back = serde_json::to_string_pretty(&s).unwrap();
        let (again, _) = AppSettings::from_value(&serde_json::from_str(&back).unwrap());
        assert_eq!(again, s);
        assert!(back.contains(r#""Name": null"#) && !back.contains(r#""LowPercent": null"#), "{back}");
    }

    #[test]
    fn hand_edits_are_forgiven() {
        let text = "{ // my settings\n \"LowPercent\": \"30\", \"Sound\": 1, \"Future\": [1, 2], }";
        let (s, clean) = AppSettings::from_value(&parse_lenient(text).unwrap());
        assert_eq!(s.low_percent, 30);
        assert!(s.sound, "a value of the wrong type keeps its default");
        assert!(!clean);
        assert_eq!(s.extra.get("Future"), Some(&serde_json::json!([1, 2])), "unknown values are kept");
    }

    #[test]
    fn a_comma_before_a_comment_and_a_byte_order_mark() {
        let text = "\u{FEFF}{ \"LowPercent\": 30, // the last one\n \"Language\": \"a // not a comment\", /* x */ }";
        let value = parse_lenient(text.trim_start_matches('\u{FEFF}')).expect("read as JSON");
        let (s, clean) = AppSettings::from_value(&value);
        assert!(clean);
        assert_eq!((s.low_percent, s.language.as_str()), (30, "a // not a comment"));
    }

    #[test]
    fn the_menu_bar_shows_one_to_five_devices() {
        assert_eq!(AppSettings::default().menu_bar_devices, 1);
        for (written, read) in [(0, 1), (4, 4), (9, 5)] {
            let (mut s, clean) = AppSettings::from_value(&serde_json::json!({ "MenuBarDevices": written }));
            s.normalize();
            assert!(clean);
            assert_eq!(s.menu_bar_devices, read);
        }
    }

    #[test]
    fn levels_stay_in_range() {
        let mut s = AppSettings { low_percent: 80, critical_percent: 90, repeat_minutes: 0, ..Default::default() };
        s.device_mut("a").low_percent = Some(12);
        s.normalize();
        assert_eq!((s.low_percent, s.critical_percent, s.repeat_minutes), (80, 80, 1), "critical no higher");
        assert_eq!((s.low_for("a"), s.critical_for("a")), (12, 12));
        let mut s =
            AppSettings { low_percent: 0, critical_percent: 500, repeat_minutes: 1_000_000, ..Default::default() };
        s.normalize();
        assert_eq!((s.low_percent, s.critical_percent, s.repeat_minutes), (1, 1, 99_999));
    }

    #[test]
    fn shortcuts() {
        let valid = windows_hotkey_valid;
        assert!(valid(0x56, MOD_ALT | MOD_CONTROL));
        assert!(valid(0x71, 0), "F2 alone");
        assert!(!valid(0x7B, 0), "F12 is the debugger's");
        assert!(!valid(0x41, 0), "an ordinary key needs Ctrl or Alt");
        assert!(!valid(0x43, MOD_CONTROL), "Ctrl+C");
        assert!(!valid(0x56, MOD_WIN | MOD_ALT), "the Windows key is the system's");
    }

    #[test]
    fn shortcuts_on_a_mac() {
        let valid = macos_hotkey_valid;
        assert!(valid(DEFAULT_HOTKEY.0, DEFAULT_HOTKEY.1), "⌃⌥V");
        assert!(valid(0x42, MOD_WIN | MOD_SHIFT), "⇧⌘B");
        assert!(valid(0x56, MOD_WIN | MOD_ALT), "⌥⌘V");
        assert!(valid(0x7B, 0), "F12 alone: no debugger there");
        assert!(!valid(0x41, MOD_SHIFT), "an ordinary key needs ⌘, ⌃ or ⌥");
        assert!(!valid(0x43, MOD_WIN), "⌘C");
        assert!(!valid(0x20, MOD_WIN), "⌘Space, Spotlight");
        assert!(!valid(0x20, MOD_CONTROL), "⌃Space, the input source");
        assert!(!valid(0x34, MOD_WIN | MOD_SHIFT), "⇧⌘4, a screenshot");
        assert!(!valid(0x1B, MOD_WIN | MOD_ALT), "⌥⌘Esc, Force Quit");
        assert!(!valid(0x5B, MOD_WIN), "a modifier alone");
    }

    #[test]
    fn the_pin_the_order_and_the_banner_are_written_only_when_set() {
        // Without them the file is what it was before they existed, and reads as nothing pinned or ordered
        let mut s = AppSettings::default();
        s.device_mut("a");
        let text = serde_json::to_string_pretty(&s).unwrap();
        for key in ["Pinned", "DeviceOrder", "AutostartBannerDismissed", "AdminBannerDismissed"] {
            assert!(!text.contains(key), "{text}");
        }
        assert_eq!(AppSettings::from_value(&serde_json::from_str(&text).unwrap()), (s.clone(), true));

        s.device_mut("a").pinned = true;
        s.device_order = vec!["b".into(), "a".into()];
        s.autostart_banner_dismissed = true;
        s.admin_banner_dismissed = true;
        let value = serde_json::to_value(&s).unwrap();
        let pinned = serde_json::json!({ "Name": null, "Hidden": false, "Alerts": true, "Pinned": true });
        assert_eq!(value["Devices"]["a"], pinned);
        assert_eq!(value["DeviceOrder"], serde_json::json!(["b", "a"]));
        assert_eq!(value["AutostartBannerDismissed"], serde_json::json!(true));
        assert_eq!(value["AdminBannerDismissed"], serde_json::json!(true));
        assert_eq!(AppSettings::from_value(&value), (s, true));
    }

    #[test]
    fn a_wrong_value_in_the_order_costs_only_itself() {
        let text = r#"{ "DeviceOrder": ["b", 5, null, "a", ["c"]], "AutostartBannerDismissed": "yes",
            "Devices": { "a": { "Pinned": 1, "Hidden": true } } }"#;
        let (s, clean) = AppSettings::from_value(&parse_lenient(text).unwrap());
        assert!(!clean);
        assert_eq!(s.device_order, ["b", "a"]);
        assert!(!s.autostart_banner_dismissed && !s.device("a").pinned && s.device("a").hidden);
        let (s, clean) = AppSettings::from_value(&serde_json::json!({ "DeviceOrder": "a" }));
        assert!(!clean && s.device_order.is_empty());
        let (s, clean) = AppSettings::from_value(&serde_json::json!({ "DeviceOrder": ["a", 5] }));
        assert!(!clean, "what was left out is worth the copy of the file");
        assert_eq!(s.device_order, ["a"]);
    }

    #[test]
    fn the_order_names_a_device_once() {
        let mut s =
            AppSettings { device_order: ["b", "a", "b", "c", "a"].map(String::from).to_vec(), ..Default::default() };
        s.normalize();
        assert_eq!(s.device_order, ["b", "a", "c"]);
    }

    #[test]
    fn a_level_entered_for_a_device_becomes_its_own() {
        let levels = |s: &AppSettings, id: &str| (s.device(id).low_percent, s.device(id).critical_percent);
        let mut s = AppSettings::default(); // 20% and 10%
        s.set_device_low("a", 30);
        assert_eq!(levels(&s, "a"), (Some(30), Some(10)), "the other one as it was shown");
        // Lowering it under the critical level pulls that along
        s.set_device_low("a", 7);
        assert_eq!(levels(&s, "a"), (Some(7), Some(7)));
        // The critical level goes no higher than the reminder level, which it doesn't move
        s.set_device_critical("b", 40);
        assert_eq!(levels(&s, "b"), (Some(20), Some(20)));
        s.set_device_critical("b", 6);
        assert_eq!(levels(&s, "b"), (Some(20), Some(6)));
        // Out of range: the nearest that is in it
        s.set_device_low("c", 199);
        s.set_device_critical("c", 0);
        assert_eq!(levels(&s, "c"), (Some(100), Some(1)));
        s.set_device_low("c", -3);
        assert_eq!(levels(&s, "c"), (Some(1), Some(1)));
        // The general levels and the reminders' switch are someone else's
        assert_eq!((s.low_percent, s.critical_percent, s.device("a").alerts), (20, 10, true));
        let entered = s.clone();
        s.normalize();
        assert_eq!(s, entered, "nothing left to put right");
    }

    #[test]
    fn entering_the_level_a_device_has_changes_nothing() {
        let mut s = AppSettings::default();
        s.set_device_low("a", 20);
        s.set_device_critical("a", 10);
        assert_eq!(s, AppSettings::default(), "it still follows the general levels");
        s.set_low(1);
        s.set_device_low("a", -3);
        s.set_device_critical("a", 99);
        assert!(s.devices.is_empty(), "nor when the nearest level in range is the one it has");
        // A device with its own levels keeps them, also when they equal the general ones again
        s.set_device_low("a", 30);
        s.set_device_low("a", 1);
        assert_eq!((s.device("a").low_percent, s.device("a").critical_percent), (Some(1), Some(1)));
    }

    #[test]
    fn a_device_goes_back_to_the_general_levels() {
        let mut s = AppSettings::default();
        s.set_device_low("a", 30);
        s.set_device_critical("a", 25);
        s.device_mut("a").alerts = false;
        assert_eq!((s.low_for("a"), s.critical_for("a")), (30, 25), "its own levels stay while its reminders are off");
        s.reset_device_levels("a");
        assert_eq!((s.device("a").low_percent, s.device("a").critical_percent), (None, None));
        s.set_low(40);
        assert_eq!((s.low_for("a"), s.critical_for("a")), (40, 10));
        assert!(!s.device("a").alerts);
    }

    #[test]
    fn the_reminder_closes_on_its_own_unless_told_not_to() {
        let s = AppSettings::default();
        assert_eq!(s.auto_close_after(), Some(std::time::Duration::from_secs(10)));
        let (s, _) = AppSettings::from_value(&serde_json::json!({ "AutoClose": false }));
        assert_eq!(s.auto_close_after(), None);
        // A typed value past the slider's end is kept, up to what the field holds
        let (mut s, clean) = AppSettings::from_value(&serde_json::json!({ "AutoClose": true, "AutoCloseSeconds": 60 }));
        assert!(clean);
        s.normalize();
        assert_eq!(s.auto_close_after(), Some(std::time::Duration::from_secs(60)));
        let (mut s, _) = AppSettings::from_value(&serde_json::json!({ "AutoCloseSeconds": 1_000_000 }));
        s.normalize();
        assert_eq!(s.auto_close_after(), Some(std::time::Duration::from_secs(99_999)));
    }

    #[test]
    fn the_critical_level_is_never_above_the_reminder_level() {
        let mut s = AppSettings::default();
        s.set_device_low("own", 30);
        s.set_low(7);
        assert_eq!((s.low_percent, s.critical_percent), (7, 7), "the critical level pulled down along");
        s.set_critical(30);
        assert_eq!((s.low_percent, s.critical_percent), (30, 30), "the reminder level pushed up along");
        s.set_low(199);
        s.set_critical(199);
        assert_eq!((s.low_percent, s.critical_percent), (100, 100));
        s.set_critical(0);
        s.set_low(0);
        assert_eq!((s.low_percent, s.critical_percent), (1, 1));
        s.set_repeat(0);
        assert_eq!(s.repeat_minutes, 1);
        s.set_repeat(500);
        assert_eq!(s.repeat_minutes, 500, "the field takes more than the slider's 60");
        s.set_repeat(1_000_000);
        assert_eq!(s.repeat_minutes, 99_999);
        s.set_auto_close_seconds(0);
        assert_eq!(s.auto_close_seconds, 1);
        s.set_auto_close_seconds(45);
        assert_eq!(s.auto_close_seconds, 45, "the field takes more than the slider's 15");
        s.set_auto_close_seconds(1_000_000);
        assert_eq!(s.auto_close_seconds, 99_999);
        assert_eq!((s.low_for("own"), s.critical_for("own")), (30, 10), "a device's own levels stay out of it");
    }

    #[test]
    fn a_name_is_kept_without_the_spaces_around_it() {
        let mut s = AppSettings::default();
        assert!(s.set_name("a", "  Mine "));
        assert_eq!(s.device("a").name.as_deref(), Some("Mine"));
        assert!(!s.set_name("a", "Mine  "), "the same name");
        assert!(s.set_name("a", "   "));
        assert_eq!(s.device("a").name, None);
        assert_eq!(s.display_name("a", "Own"), "Own");
        // Nothing typed for a device without a name of its own: nothing to keep, not even its entry
        assert!(!s.set_name("b", ""));
        assert!(!s.set_name("b", "  "));
        assert!(!s.devices.contains_key("b"));
    }

    #[test]
    fn the_new_keys_never_land_in_extra() {
        // Each read by its own arm: a key left to `extra` as well would be written twice
        let text = r#"{ "AutostartBannerDismissed": true, "DeviceOrder": ["b", "a"],
            "Devices": { "a": { "Name": null, "Hidden": false, "Alerts": true, "Pinned": true } } }"#;
        let (s, clean) = AppSettings::from_value(&parse_lenient(text).unwrap());
        assert!(clean);
        assert!(s.extra.is_empty() && s.device("a").extra.is_empty());
        assert!(s.autostart_banner_dismissed && s.is_pinned("a") && !s.is_pinned("b"));
        assert_eq!(s.device_order, ["b", "a"]);
        let written = serde_json::to_string_pretty(&s).unwrap();
        for key in ["\"Pinned\"", "\"DeviceOrder\"", "\"AutostartBannerDismissed\""] {
            assert_eq!(written.matches(key).count(), 1, "{key} in {written}");
        }
    }

    #[test]
    fn each_sound_is_chosen_by_itself() {
        // Never chosen: the system's sound, and the file as it always was
        let mut s = AppSettings::default();
        assert_eq!(s.sound_for(Event::Critical), Choice::System);
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("SoundLow") && !text.contains("SoundCharged"), "{text}");

        let named = crate::sounds::NAMED_SOUNDS[0].id;
        s.set_sound(Event::Low, &Choice::Named(named));
        s.set_sound(Event::Charged, &Choice::File("done.mp3".into()));
        let value = serde_json::to_value(&s).unwrap();
        assert_eq!(value["SoundLow"], serde_json::json!(named));
        assert_eq!(value["SoundCharged"], serde_json::json!("file:done.mp3"));
        assert!(value.get("SoundCritical").is_none());
        let (again, clean) = AppSettings::from_value(&value);
        assert!(clean && again == s && again.extra.is_empty());

        // The switch of the versions before follows the reminder's two sounds
        s.set_sound(Event::Low, &Choice::None);
        assert!(s.sound, "the critical level still sounds");
        s.set_sound(Event::Critical, &Choice::None);
        assert!(!s.sound);
        s.set_sound(Event::Charged, &Choice::None);
        s.set_sound(Event::Critical, &Choice::System);
        assert!(s.sound);
    }

    #[test]
    fn a_sound_switched_off_before_stays_off() {
        // 2.3.0 and before: one switch, for the reminder's sound
        let (mut s, clean) = AppSettings::from_value(&serde_json::json!({ "Sound": false }));
        s.normalize();
        assert!(clean);
        assert_eq!((s.sound_for(Event::Low), s.sound_for(Event::Critical)), (Choice::None, Choice::None));
        assert_eq!(s.sound_for(Event::Charged), Choice::System, "the switch never was about the notice");
        assert!(!s.sound);
        // Written and read again: as it is
        let (mut again, _) = AppSettings::from_value(&serde_json::to_value(&s).unwrap());
        again.normalize();
        assert_eq!(again, s);

        // Once a sound has been chosen, the old switch decides nothing
        let named = crate::sounds::NAMED_SOUNDS[0].id;
        let (mut s, _) = AppSettings::from_value(&serde_json::json!({ "Sound": false, "SoundLow": named }));
        s.normalize();
        assert_eq!((s.sound_for(Event::Low), s.sound_for(Event::Critical)), (Choice::Named(named), Choice::System));
        assert!(s.sound);

        // What isn't a sound is the system's; a value of the wrong type costs only itself
        let (mut s, clean) = AppSettings::from_value(&serde_json::json!({ "SoundLow": "gong", "SoundCritical": 5 }));
        s.normalize();
        assert!(!clean);
        assert_eq!((s.sound_low.as_str(), s.sound_critical.as_str()), ("system", "system"));
    }
}
