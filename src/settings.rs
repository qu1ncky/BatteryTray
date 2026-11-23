//! User settings in %LOCALAPPDATA%\BatteryTray\settings.json: the same file and names as the C#
//! app, read as forgivingly. Hand edits tend to leave a number in quotes, a comma after the last value or a //
//! comment, which are all taken. A value of the wrong type costs only itself: the rest is read value by value, and
//! the file as it was is kept as settings.json.bak before the next save writes over it. Values this version doesn't
//! know are kept as they are, so going back and forth between versions loses nothing.

use crate::log;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

pub const AUTO: &str = "auto";

/// Win32 hotkey modifiers, without the registration-only MOD_NOREPEAT flag.
pub const MOD_ALT: i32 = 0x0001;
pub const MOD_CONTROL: i32 = 0x0002;
pub const MOD_SHIFT: i32 = 0x0004;
/// Ctrl+Alt+V
pub const DEFAULT_HOTKEY: (i32, i32) = (0x56, MOD_ALT | MOD_CONTROL);

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
    pub sound: bool,
    /// "ru", "en", "zh", or "auto" to follow Windows.
    pub language: String,
    /// A Windows notification when a device finishes charging.
    pub notify_charged: bool,
    pub auto_update: bool,
    pub overlay_enabled: bool,
    pub overlay_hotkey_key: i32,
    pub overlay_hotkey_modifiers: i32,
    /// 0 = shown for 5 seconds, 1 = toggled by the shortcut.
    pub overlay_mode: i32,
    /// 0 top left, 1 top right, 2 bottom left, 3 bottom right.
    pub overlay_corner: i32,
    pub overlay_show_names: bool,
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
            language: AUTO.into(),
            notify_charged: true,
            auto_update: true,
            overlay_enabled: false,
            overlay_hotkey_key: DEFAULT_HOTKEY.0,
            overlay_hotkey_modifiers: DEFAULT_HOTKEY.1,
            overlay_mode: 0,
            overlay_corner: 1,
            overlay_show_names: false,
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
    (modifiers & !(MOD_ALT | MOD_CONTROL | MOD_SHIFT)) == 0
        && (0x08..=0xFE).contains(&key)
        && ![0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0x5D, 0x7B, 0x90, 0x91].contains(&key)
        && !(0xA0..=0xA5).contains(&key)
        && ((modifiers & (MOD_ALT | MOD_CONTROL)) != 0 || (0x70..=0x87).contains(&key))
        && !(modifiers == MOD_CONTROL && [0x43, 0x56, 0x58].contains(&key))
        && !(modifiers == MOD_ALT && key == 0x73)
}

impl AppSettings {
    pub fn path() -> PathBuf {
        crate::win::app_data_folder().join("settings.json")
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
                "Language" => set(&mut s.language, v.as_str().map(str::to_string)),
                "NotifyCharged" => set(&mut s.notify_charged, v.as_bool()),
                "AutoUpdate" => set(&mut s.auto_update, v.as_bool()),
                "OverlayEnabled" => set(&mut s.overlay_enabled, v.as_bool()),
                "OverlayHotkeyKey" => set(&mut s.overlay_hotkey_key, int(v)),
                "OverlayHotkeyModifiers" => set(&mut s.overlay_hotkey_modifiers, int(v)),
                "OverlayMode" => set(&mut s.overlay_mode, int(v)),
                "OverlayCorner" => set(&mut s.overlay_corner, int(v)),
                "OverlayShowNames" => set(&mut s.overlay_show_names, v.as_bool()),
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
        (s, clean)
    }

    /// Keeps every level and choice within what the app offers (as the C# app does on load).
    pub fn normalize(&mut self) {
        self.low_percent = self.low_percent.clamp(10, 50);
        self.critical_percent = self.critical_percent.clamp(5, self.low_percent - 5);
        self.repeat_minutes = self.repeat_minutes.clamp(1, 120);
        if !hotkey_valid(self.overlay_hotkey_key, self.overlay_hotkey_modifiers) {
            (self.overlay_hotkey_key, self.overlay_hotkey_modifiers) = DEFAULT_HOTKEY;
        }
        if !(0..=1).contains(&self.overlay_mode) {
            self.overlay_mode = 0;
        }
        if !(0..=3).contains(&self.overlay_corner) {
            self.overlay_corner = 1;
        }
        // A device has both of its own levels or none: one set alone (by hand) gets the other from the global ones
        let (low, critical) = (self.low_percent, self.critical_percent);
        for d in self.devices.values_mut() {
            if d.low_percent.is_none() && d.critical_percent.is_none() {
                continue;
            }
            let own_low = d.low_percent.unwrap_or(low).clamp(10, 50);
            d.low_percent = Some(own_low);
            d.critical_percent = Some(d.critical_percent.unwrap_or(critical).clamp(5, own_low - 5));
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

    /// The critical level a device actually turns red at: its own when set, the global one otherwise. Never closer
    /// than 5 below the level it alerts at, so a low custom level can't fire critical first.
    pub fn critical_for(&self, id: &str) -> i32 {
        self.devices.get(id).and_then(|d| d.critical_percent).unwrap_or(self.critical_percent).min(self.low_for(id) - 5)
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

    /// The general reminder level; the critical one stays at least 5 below it.
    pub fn set_low(&mut self, value: i32) {
        self.low_percent = value.clamp(10, 50);
        self.critical_percent = self.critical_percent.min(self.low_percent - 5);
    }

    /// The general critical level; the reminder level stays at least 5 above it.
    pub fn set_critical(&mut self, value: i32) {
        self.critical_percent = value.clamp(5, 45);
        self.low_percent = self.low_percent.max(self.critical_percent + 5);
    }

    pub fn set_repeat(&mut self, minutes: i32) {
        self.repeat_minutes = minutes.clamp(1, 120);
    }

    /// A reminder level entered for one device. It becomes the device's own, and the critical level with it: the one
    /// in effect until now, pulled down when it would be closer than 5. Entering the level the device has already
    /// changes nothing, so a device that follows the general levels goes on following them when the focus only
    /// passes through its field.
    pub fn set_device_low(&mut self, id: &str, value: i32) {
        let (was, critical) = (self.low_for(id), self.critical_for(id));
        let low = value.clamp(10, 50);
        if low == was {
            return;
        }
        let d = self.device_mut(id);
        d.low_percent = Some(low);
        d.critical_percent = Some(critical.min(low - 5));
    }

    /// A critical level entered for one device: no higher than 5 below its reminder level, which becomes the
    /// device's own with it. Entering the level the device has already changes nothing.
    pub fn set_device_critical(&mut self, id: &str, value: i32) {
        let (low, was) = (self.low_for(id), self.critical_for(id));
        let critical = value.min(low - 5).max(5);
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
    fn levels_stay_in_range() {
        let mut s = AppSettings { low_percent: 80, critical_percent: 90, repeat_minutes: 0, ..Default::default() };
        s.device_mut("a").low_percent = Some(12);
        s.normalize();
        assert_eq!((s.low_percent, s.critical_percent, s.repeat_minutes), (50, 45, 1));
        assert_eq!((s.low_for("a"), s.critical_for("a")), (12, 7));
    }

    #[test]
    fn shortcuts() {
        assert!(hotkey_valid(0x56, MOD_ALT | MOD_CONTROL));
        assert!(hotkey_valid(0x71, 0), "F2 alone");
        assert!(!hotkey_valid(0x7B, 0), "F12 is the debugger's");
        assert!(!hotkey_valid(0x41, 0), "an ordinary key needs Ctrl or Alt");
        assert!(!hotkey_valid(0x43, MOD_CONTROL), "Ctrl+C");
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
        // Lowering it pulls the critical level along, 5 below
        s.set_device_low("a", 12);
        assert_eq!(levels(&s, "a"), (Some(12), Some(7)));
        // The critical level goes no higher than 5 below the reminder level, which it doesn't move
        s.set_device_critical("b", 40);
        assert_eq!(levels(&s, "b"), (Some(20), Some(15)));
        s.set_device_critical("b", 6);
        assert_eq!(levels(&s, "b"), (Some(20), Some(6)));
        // Out of range: the nearest that is in it
        s.set_device_low("c", 99);
        s.set_device_critical("c", 0);
        assert_eq!(levels(&s, "c"), (Some(50), Some(5)));
        s.set_device_low("c", 3);
        assert_eq!(levels(&s, "c"), (Some(10), Some(5)));
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
        s.set_low(10);
        s.set_device_low("a", 3);
        s.set_device_critical("a", 99);
        assert!(s.devices.is_empty(), "nor when the nearest level in range is the one it has");
        // A device with its own levels keeps them, also when they equal the general ones again
        s.set_device_low("a", 30);
        s.set_device_low("a", 10);
        assert_eq!((s.device("a").low_percent, s.device("a").critical_percent), (Some(10), Some(5)));
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
    fn the_general_levels_keep_their_distance() {
        let mut s = AppSettings::default();
        s.set_device_low("own", 30);
        s.set_low(12);
        assert_eq!((s.low_percent, s.critical_percent), (12, 7));
        s.set_critical(30);
        assert_eq!((s.low_percent, s.critical_percent), (35, 30));
        s.set_low(99);
        s.set_critical(99);
        assert_eq!((s.low_percent, s.critical_percent), (50, 45));
        s.set_critical(0);
        s.set_low(0);
        assert_eq!((s.low_percent, s.critical_percent), (10, 5));
        s.set_repeat(0);
        assert_eq!(s.repeat_minutes, 1);
        s.set_repeat(500);
        assert_eq!(s.repeat_minutes, 120);
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
}
