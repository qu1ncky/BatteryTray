//! The core of BatteryTray for macOS, as a static library for the Swift app (macos/BatteryTray): the C functions it
//! calls, declared in macos/BatteryTray/core.h. The app sends a command as JSON and gets JSON back - the state to show,
//! or the effects to carry out (a notice, a sound, the reminder, a restart). All of it from the app's main thread;
//! the polling threads only tell it, through the function given to `bt_init`, that there is news.

mod app;
mod demo;
mod keys;

use app::{MacApp, Notify};
use batterytray::reading::{DeviceKind, DeviceReading};
use batterytray::settings::MENU_BAR_DEVICES;
use batterytray::{diag, hid, loc, log, sounds, updater};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::sync::{Arc, Mutex, OnceLock};

static APP: Mutex<Option<MacApp>> = Mutex::new(None);
static NOTIFY: OnceLock<Notify> = OnceLock::new();

/// Gives the core the function that tells the app there is news (called on any thread; the app answers with "pump"
/// on its main thread). Called once, first.
///
/// # Safety
/// `notify` must stay callable for as long as the process runs.
#[no_mangle]
pub unsafe extern "C" fn bt_init(notify: extern "C" fn()) {
    std::panic::set_hook(Box::new(|info| log::write(&format!("panic: {info}"))));
    let _ = NOTIFY.set(Arc::new(move || notify()));
}

/// Runs a command (JSON) and returns the answer (JSON), which the caller frees with `bt_free`.
///
/// # Safety
/// `command` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn bt_call(command: *const c_char) -> *mut c_char {
    let text = if command.is_null() { "" } else { CStr::from_ptr(command).to_str().unwrap_or("") };
    let answer = match serde_json::from_str::<Value>(text) {
        Ok(command) => std::panic::catch_unwind(|| call(&command))
            .unwrap_or_else(|_| json!({ "error": "the core failed on this command; see the log" })),
        Err(e) => json!({ "error": format!("not JSON: {e}") }),
    };
    CString::new(answer.to_string()).unwrap_or_default().into_raw()
}

/// Frees an answer of `bt_call`.
///
/// # Safety
/// `text` must be an answer of `bt_call`, freed once.
#[no_mangle]
pub unsafe extern "C" fn bt_free(text: *mut c_char) {
    if !text.is_null() {
        drop(CString::from_raw(text));
    }
}

fn str_of<'a>(command: &'a Value, key: &str) -> &'a str {
    command.get(key).and_then(Value::as_str).unwrap_or("")
}

fn int_of(command: &Value, key: &str) -> i32 {
    command.get(key).and_then(Value::as_i64).unwrap_or(0) as i32
}

fn bool_of(command: &Value, key: &str) -> bool {
    command.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn effects(list: Vec<Value>) -> Value {
    json!({ "effects": list })
}

fn call(command: &Value) -> Value {
    let cmd = str_of(command, "cmd");
    // What needs no running app: the command-line switches and the texts
    match cmd {
        "update_now" => return json!({ "code": update_now() }),
        "dump" => {
            let file = command
                .get("file")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| log::folder().join("dump.txt"));
            let ok = std::fs::write(file, diag::dump_lines().join("\n") + "\n").is_ok();
            return json!({ "code": if ok { 0 } else { 1 } });
        }
        "texts" => {
            let language = loc::current();
            let texts: serde_json::Map<String, Value> =
                loc::all_in(language).into_iter().map(|(k, v)| (k.to_string(), Value::String(v))).collect();
            return json!({ "lang": language, "texts": texts });
        }
        "log" => {
            log::write(str_of(command, "text"));
            return json!({});
        }
        "default_shortcut" => return MacApp::default_shortcut(),
        "suggestion_url" => return json!({ "url": MacApp::suggestion_url() }),
        "log_folder" => return json!({ "path": log::folder().display().to_string() }),
        // Where the user's sounds are kept: shown in Finder, which is where one is removed
        "sounds_folder" => return json!({ "path": sounds::folder().display().to_string() }),
        "input_monitoring" => return json!({ "allowed": hid::input_monitoring_allowed() }),
        "request_input_monitoring" => return json!({ "allowed": hid::request_input_monitoring() }),
        _ => {}
    }

    let mut guard = APP.lock().unwrap_or_else(|e| e.into_inner());
    if cmd == "start" {
        if guard.is_some() {
            return effects(Vec::new());
        }
        let notify = NOTIFY.get().cloned().unwrap_or_else(|| Arc::new(|| {}));
        let (app, list) = MacApp::start(notify, bool_of(command, "demo"));
        *guard = Some(app);
        return effects(list);
    }
    let Some(app) = guard.as_mut() else { return json!({ "error": "not started" }) };
    let id = str_of(command, "id");
    let list = match cmd {
        "snapshot" => return app.snapshot(),
        "pump" => app.pump(),
        "tick" => app.evaluate_alerts(),
        "save" => {
            app.save();
            Vec::new()
        }
        "refresh" => {
            app.refresh_now();
            Vec::new()
        }
        "devices_changed" => {
            app.devices_changed();
            Vec::new()
        }
        "windows" => {
            app.set_windows(bool_of(command, "main_open"), bool_of(command, "overlay_timed"));
            Vec::new()
        }
        "language" => app.set_language(str_of(command, "value")),
        "theme" => app.set_theme(str_of(command, "value")),
        "set" => {
            let (n, on) = (int_of(command, "value"), bool_of(command, "value"));
            match str_of(command, "key") {
                "low" => app.edit(|s| s.set_low(n)),
                "critical" => app.edit(|s| s.set_critical(n)),
                "repeat" => app.edit(|s| s.set_repeat(n)),
                "notify_charged" => app.edit(|s| s.notify_charged = on),
                "auto_close" => app.edit(|s| s.auto_close = on),
                "auto_close_seconds" => app.edit(|s| s.set_auto_close_seconds(n)),
                "auto_update" => app.edit(|s| s.auto_update = on),
                "overlay_enabled" => app.edit(|s| s.overlay_enabled = on),
                "overlay_mode" => app.edit(|s| s.overlay_mode = n.clamp(0, 1)),
                "overlay_corner" => app.edit(|s| s.overlay_corner = n.clamp(0, 3)),
                "overlay_names" => app.edit(|s| s.overlay_show_names = on),
                "menu_bar_devices" => {
                    app.edit(|s| s.menu_bar_devices = n.clamp(MENU_BAR_DEVICES.0, MENU_BAR_DEVICES.1))
                }
                "tray_battery_icon" => app.edit(|s| s.tray_battery_icon = on),
                "tray_priority" => app.edit(|s| {
                    s.tray_priority = str_of(command, "value").to_string();
                    s.normalize();
                }),
                "hide_offline" => app.edit(|s| s.hide_offline = on),
                other => return json!({ "error": format!("no setting {other}") }),
            }
        }
        // A sound ("low", "critical" or "charged") chosen from its menu, or a file picked for it in the open panel
        "set_sound" | "add_sound" => {
            let Some(event) = sounds::Event::from_name(str_of(command, "event")) else {
                return json!({ "error": format!("no sound {}", str_of(command, "event")) });
            };
            if cmd == "set_sound" {
                app.set_sound(event, str_of(command, "value"))
            } else {
                app.add_sound(event, str_of(command, "path"))
            }
        }
        "rename" => app.rename(id, str_of(command, "text")),
        "pin" => app.pin(id, bool_of(command, "pinned")),
        "hide" => app.edit(|s| s.device_mut(id).hidden = bool_of(command, "hidden")),
        "move" => app.move_onto(id, str_of(command, "onto")),
        "device_alerts" => app.edit(|s| s.device_mut(id).alerts = bool_of(command, "on")),
        "device_low" => app.edit(|s| s.set_device_low(id, int_of(command, "value"))),
        "device_critical" => app.edit(|s| s.set_device_critical(id, int_of(command, "value"))),
        "device_levels_reset" => app.edit(|s| s.reset_device_levels(id)),
        "autostart" => app.set_autostart(bool_of(command, "on")),
        "dismiss_autostart_banner" => app.edit(|s| s.autostart_banner_dismissed = true),
        "check_shortcut" => {
            let mac_key = command.get("mac_key").and_then(Value::as_u64).unwrap_or(u64::MAX);
            let modifiers = keys::modifiers(
                bool_of(command, "command"),
                bool_of(command, "control"),
                bool_of(command, "option"),
                bool_of(command, "shift"),
            );
            return match u16::try_from(mac_key) {
                Ok(key) => app.check_shortcut(key, modifiers),
                Err(_) => json!({ "valid": false, "error": loc::t("overlay.invalid") }),
            };
        }
        "set_shortcut" => app.set_shortcut(int_of(command, "key"), int_of(command, "modifiers")),
        "snooze" => app.snooze(bool_of(command, "auto")),
        "mute" => app.mute(),
        "test_alert" => app.test_alert(),
        "update_install" => {
            app.updates_install();
            Vec::new()
        }
        "update_check" => {
            app.updates_check();
            Vec::new()
        }
        "external" => {
            let source = command.get("source").and_then(Value::as_str).unwrap_or("controllers");
            let readings = command.get("readings").and_then(Value::as_array).cloned().unwrap_or_default();
            app.set_external(source, readings.iter().filter_map(external_reading).collect());
            Vec::new()
        }
        "quit" => {
            log::write("exit");
            app.tear_down();
            Vec::new()
        }
        other => return json!({ "error": format!("no command {other}") }),
    };
    effects(list)
}

/// A game controller as the Swift app reports it: {"id", "name", "level" (0-100), "charging", "full"}.
fn external_reading(value: &Value) -> Option<DeviceReading> {
    let id = value.get("id")?.as_str()?;
    let name = value.get("name").and_then(Value::as_str).unwrap_or(id);
    let level = value.get("level").and_then(Value::as_i64).map(|l| l.clamp(0, 100) as i32);
    // A controller is a gamepad; a Bluetooth LE device is what its name says
    let kind = match value.get("kind").and_then(Value::as_str) {
        Some("gamepad") | None => DeviceKind::Gamepad,
        Some(_) => batterytray::providers::bluetooth::kind_by_name(name),
    };
    Some(DeviceReading {
        online: true,
        level,
        charging: value.get("charging").and_then(Value::as_bool).unwrap_or(false),
        full_on_charger: value.get("full").and_then(Value::as_bool).unwrap_or(false),
        ..DeviceReading::new(id, name, kind, "Bluetooth")
    })
}

/// `--update`: installs the latest release over this app; 0 = updated, 2 = already up to date, 1 = failed (the app
/// runs it as root when its folder is an administrator's).
fn update_now() -> i32 {
    match updater::get_latest() {
        Ok(Some(latest)) if latest.version > updater::Version::current() => match updater::install(&latest, &|_| {}) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use batterytray::sys;

    fn run(command: Value) -> Value {
        call(&command)
    }

    /// The commands the Swift app sends, on a core started with the demo's devices.
    #[test]
    fn the_app_s_commands_on_the_demo() {
        assert!(run(json!({ "cmd": "snapshot" }))["error"].is_string(), "nothing before start");
        run(json!({ "cmd": "start", "demo": true }));
        // The first poll of the demo provider
        let mut snapshot = run(json!({ "cmd": "snapshot" }));
        for _ in 0..100 {
            if snapshot["devices"]["connected"].as_array().is_some_and(|c| !c.is_empty()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            run(json!({ "cmd": "pump" }));
            snapshot = run(json!({ "cmd": "snapshot" }));
        }
        let names = |block: &str| -> Vec<String> {
            snapshot["devices"][block].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap().into()).collect()
        };
        assert_eq!(names("pinned"), ["Razer Barracuda X", "Xbox Wireless Controller"]);
        assert_eq!(names("connected"), ["Attack Shark R3", "Rainy 75"]);
        assert_eq!(names("offline"), ["Logitech MX Keys"]);
        assert_eq!(names("hidden"), ["AirPods Pro"]);
        assert_eq!(snapshot["tray"]["label"], "15");
        assert_eq!(snapshot["overlay"]["shortcut"]["label"], "⌃⌥V");

        // Nothing is due: the keyboard at 15% has its reminders off, the headset at 37% is above its own 30%
        assert!(run(json!({ "cmd": "tick" }))["effects"].as_array().unwrap().is_empty());
        let due = run(json!({ "cmd": "device_low", "id": demo::BARRACUDA, "value": 40 }));
        let alert = due["effects"].as_array().unwrap().iter().find(|e| e["type"] == "alert").cloned().unwrap();
        assert_eq!(alert["items"][0]["name"], "Razer Barracuda X");
        assert_eq!(alert["items"][0]["percent"], 37, "the level bar's share");
        assert_eq!(alert["auto_close"], 10, "closes on its own after 10 seconds unless told otherwise");
        // Waiting for an answer instead: the open reminder hears it at once
        let waits = run(json!({ "cmd": "set", "key": "auto_close", "value": false }));
        let alert = waits["effects"].as_array().unwrap().iter().find(|e| e["type"] == "alert").cloned().unwrap();
        assert!(alert["auto_close"].is_null());
        run(json!({ "cmd": "set", "key": "auto_close", "value": true }));
        run(json!({ "cmd": "set", "key": "auto_close_seconds", "value": 45 }));
        assert_eq!(run(json!({ "cmd": "snapshot" }))["settings"]["auto_close_seconds"], 45, "past the slider's 15");
        run(json!({ "cmd": "set", "key": "auto_close_seconds", "value": 1_000_000 }));
        assert_eq!(run(json!({ "cmd": "snapshot" }))["settings"]["auto_close_seconds"], 99_999, "what the field holds");
        let closed = run(json!({ "cmd": "snooze", "auto": true }));
        assert_eq!(closed["effects"][0]["type"], "alert_close");

        // Settings: kept within range, saved half a second later by the app
        let changed = run(json!({ "cmd": "set", "key": "low", "value": 180 }));
        assert_eq!(changed["effects"][0]["type"], "save_later");
        assert_eq!(run(json!({ "cmd": "snapshot" }))["settings"]["low"], 100);

        // Sounds: the system's until one is chosen; silence plays nothing, a sound chosen is played once - one of
        // macOS' own from the system's folder
        let snapshot = run(json!({ "cmd": "snapshot" }));
        assert_eq!(snapshot["settings"]["sound_low"], "system");
        let named = run(json!({ "cmd": "set_sound", "event": "low", "value": "hero" }));
        let sound = named["effects"].as_array().unwrap().iter().find(|e| e["type"] == "sound").cloned().unwrap();
        assert!(sound["path"].as_str().is_some_and(|p| p.ends_with("/Hero.aiff")), "{sound}");
        assert_eq!(
            (snapshot["sounds"][0]["id"].clone(), snapshot["sounds"][1]["id"].clone()),
            (json!("system"), json!("none"))
        );
        assert_eq!(
            (snapshot["sounds"][1]["group"].clone(), snapshot["sounds"][2]["group"].clone()),
            (json!(0), json!(1))
        );
        let silent = run(json!({ "cmd": "set_sound", "event": "critical", "value": "none" }));
        assert!(silent["effects"].as_array().unwrap().iter().all(|e| e["type"] != "sound"));
        assert_eq!(run(json!({ "cmd": "snapshot" }))["settings"]["sound_critical"], "none");
        let system = run(json!({ "cmd": "set_sound", "event": "critical", "value": "system" }));
        let sound = system["effects"].as_array().unwrap().iter().find(|e| e["type"] == "sound").cloned().unwrap();
        assert_eq!((sound["event"].clone(), sound["system"].clone()), (json!("critical"), json!(true)));
        assert!(run(json!({ "cmd": "set_sound", "event": "bell", "value": "none" }))["error"].is_string());
        let refused = run(json!({ "cmd": "add_sound", "event": "low", "path": "/nowhere/notes.txt" }));
        assert_eq!(refused["effects"][0]["type"], "sound_refused");

        // Shortcuts: ⌘C is the system's, ⇧⌘B is free
        let copy = run(json!({ "cmd": "check_shortcut", "mac_key": 0x08, "command": true }));
        assert_eq!(copy["valid"], false);
        let free = run(json!({ "cmd": "check_shortcut", "mac_key": 0x0B, "command": true, "shift": true }));
        assert_eq!((free["valid"].clone(), free["shortcut"]["label"].clone()), (json!(true), json!("⇧⌘B")));

        run(json!({ "cmd": "language", "value": "en" }));
        let texts = run(json!({ "cmd": "texts" }));
        assert_eq!((texts["lang"].clone(), texts["texts"]["autostart"].clone()), (json!("en"), json!("Open at login")));
        run(json!({ "cmd": "quit" }));
    }

    #[test]
    fn the_command_line_and_bad_input() {
        assert!(run(json!({ "cmd": "no-such-thing-before-start" }))["error"].is_string());
        let url = run(json!({ "cmd": "suggestion_url" }));
        assert!(url["url"].as_str().unwrap().contains("macOS"));
        let answer = unsafe { bt_call(c"not json".as_ptr()) };
        let text = unsafe { CStr::from_ptr(answer) }.to_str().unwrap().to_string();
        unsafe { bt_free(answer) };
        assert!(text.contains("not JSON"));
        assert_eq!(sys::app_data_folder().file_name().unwrap(), "BatteryTray");
    }
}
