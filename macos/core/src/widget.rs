//! The desktop widgets' devices (macos/Widget). The widget extension runs in a sandbox that may read one folder of the
//! user's: the app writes what the widgets show there, `widget/devices.json` in the app's folder, and asks WidgetKit to
//! draw them again. WidgetKit allows a widget some 40-70 redraws a day, so a change of a level alone, the most
//! frequent one, asks for one at most every 15 minutes; anything else that shows (a device that comes or goes, starts
//! or stops charging, runs low; a name; the language) asks at once.

use crate::app::{kind, Effects};
use batterytray::loc::{self, t};
use batterytray::log;
use batterytray::view::WidgetView;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the widgets are asked to draw again for levels that changed
const LEVELS_EVERY: Duration = Duration::from_secs(15 * 60);

pub struct WidgetFeed {
    file: PathBuf,
    /// Whether WidgetKit is asked to draw: not for the demo's devices, which are written to the demo's folder
    redraws: bool,
    /// What was written last, and the same without the levels
    written: String,
    shape: String,
    /// When the widgets were last asked to draw, and whether a level changed since
    asked: Option<Instant>,
    levels_changed: bool,
}

impl WidgetFeed {
    /// The widgets' file in the app's folder (the demo's for the demo).
    pub fn new(folder: &Path, redraws: bool) -> Self {
        Self {
            file: folder.join("widget").join("devices.json"),
            redraws,
            written: String::new(),
            shape: String::new(),
            asked: None,
            levels_changed: false,
        }
    }

    /// What the widgets show now: written when it changed, with the effect that asks WidgetKit to draw them again
    /// when the change is due. `running`: false once the app quits; the widgets then say it is closed.
    pub fn update(&mut self, view: &WidgetView, running: bool, now: Instant) -> Effects {
        let mut feed = feed(view, running);
        let text = feed.to_string();
        if text != self.written {
            if let Err(e) = write(&self.file, &text) {
                log::write(&format!("widgets: can't write {}: {e}", self.file.display()));
            }
            self.written = text;
            for device in feed["devices"].as_array_mut().into_iter().flatten() {
                if let Some(device) = device.as_object_mut() {
                    device.remove("level");
                    device.remove("fill");
                }
            }
            let shape = feed.to_string();
            if shape != self.shape {
                self.shape = shape;
                return self.redraw(now);
            }
            self.levels_changed = true;
        }
        match self.asked {
            Some(asked) if self.levels_changed && now.duration_since(asked) >= LEVELS_EVERY => self.redraw(now),
            _ => Vec::new(),
        }
    }

    fn redraw(&mut self, now: Instant) -> Effects {
        self.asked = Some(now);
        self.levels_changed = false;
        if self.redraws {
            vec![json!({ "type": "widget_reload" })]
        } else {
            Vec::new()
        }
    }
}

/// The file's contents (macos/Widget/Feed.swift reads them): the devices, the one to charge next, those a widget
/// with four places and one with six shows (indices into "devices"), and the texts the widgets take from the app, in
/// its language. A device that is off has "seen", when its last level was read, in seconds since 1970, and the date
/// to say when that is a day ago or more.
fn feed(view: &WidgetView, running: bool) -> Value {
    let devices: Vec<Value> = view
        .devices
        .iter()
        .map(|d| {
            json!({
                "id": d.id, "name": d.name, "kind": kind(d.kind), "connected": d.connected, "level": d.level_text,
                "fill": d.fill, "estimate": d.estimate, "power": d.power, "indeterminate": d.indeterminate,
                "tone": d.tone.name(), "detail": d.detail, "seen": d.seen.map(|s| s.timestamp()),
                "seen_date": d.seen.map(|s| loc::date_time(&s)),
            })
        })
        .collect();
    json!({
        "version": 1,
        "running": running,
        "lang": loc::current(),
        "devices": devices,
        "next": view.next,
        "four": view.four,
        "six": view.six,
        "texts": {
            "charging": t("state.charging"), "offline": t("state.offline"), "none_online": t("none.online"),
            "now": t("ago.now"), "min": t("ago.min"), "hours": t("ago.hours"),
        },
    })
}

/// Written whole or not at all: the widget may read it at any moment.
fn write(file: &Path, text: &str) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let part = file.with_extension("json.part");
    std::fs::write(&part, text)?;
    std::fs::rename(&part, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use batterytray::reading::{DeviceKind, DeviceReading};
    use batterytray::rows::DeviceRow;
    use batterytray::settings::AppSettings;
    use batterytray::view;

    fn rows(levels: &[(&str, Option<i32>, bool)]) -> Vec<DeviceRow> {
        levels
            .iter()
            .map(|&(id, level, charging)| {
                let reading = DeviceReading {
                    online: true,
                    level,
                    charging,
                    ..DeviceReading::new(id, id, DeviceKind::Mouse, "")
                };
                DeviceRow { id: id.into(), current: Some(reading.clone()), last: Some(reading), name: id.into() }
            })
            .collect()
    }

    #[test]
    fn levels_redraw_the_widgets_at_most_every_15_minutes_anything_else_at_once() {
        let folder = std::env::temp_dir().join(format!("batterytray-widget-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut feed = WidgetFeed::new(&folder, true);
        let s = AppSettings::default();
        let start = Instant::now();
        let at = |minutes: u64| start + Duration::from_secs(minutes * 60);
        let mut update = |levels: &[(&str, Option<i32>, bool)], running: bool, minutes: u64| {
            feed.update(&view::widget_view(&rows(levels), &s), running, at(minutes)).len()
        };

        // The first state redraws, and is in the file
        assert_eq!(update(&[("a", Some(50), false)], true, 0), 1);
        let file = folder.join("widget").join("devices.json");
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!((written["running"].as_bool(), written["devices"][0]["level"].as_str()), (Some(true), Some("50%")));
        // Nothing new: nothing; a level: written, drawn once 15 minutes have passed, the latest level
        assert_eq!(update(&[("a", Some(50), false)], true, 1), 0);
        assert_eq!(update(&[("a", Some(49), false)], true, 2), 0);
        assert_eq!(update(&[("a", Some(48), false)], true, 10), 0);
        assert!(std::fs::read_to_string(&file).unwrap().contains("\"48%\""));
        assert_eq!(update(&[("a", Some(48), false)], true, 15), 1);
        assert_eq!(update(&[("a", Some(48), false)], true, 40), 0, "drawn already");
        // Low now: at once; so is a device that starts charging, another that comes, and the app quitting
        assert_eq!(update(&[("a", Some(19), false)], true, 41), 1);
        assert_eq!(update(&[("a", Some(19), true)], true, 42), 1);
        assert_eq!(update(&[("a", Some(19), true), ("b", Some(70), false)], true, 43), 1);
        assert_eq!(update(&[("a", Some(19), true), ("b", Some(70), false)], false, 44), 1);
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written["running"].as_bool(), Some(false));

        // The demo's devices are written, never drawn
        let mut demo = WidgetFeed::new(&folder, false);
        assert!(demo.update(&view::widget_view(&rows(&[("a", Some(50), false)]), &s), true, start).is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
