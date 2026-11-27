//! What the windows show, worked out from the app's state the same way on both systems: a device as the list shows it,
//! the update status, the number and tooltip of the tray icon (the menu bar item on macOS), the overlay's entries.
//! The Windows windows (Slint) and the macOS app (SwiftUI, through macos/core) both draw from these.

use crate::loc::{self, t, tf};
use crate::reading::{DeviceKind, DeviceReading};
use crate::rows::{self, Block, DeviceRow};
use crate::settings::AppSettings;
use crate::update_service::{Stage, Status};
use crate::updater::Version;
use chrono::{DateTime, Local};

pub type Rgb = (u8, u8, u8);

/// What a level says, which picks its color: each app draws a tone in the color its theme (light or dark) has for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Good,
    Warn,
    Bad,
    Charge,
    Offline,
}

impl Tone {
    /// The Windows app's color for the tone: Windows 11's own, the dark theme's bright ones, the light theme's dark
    /// enough to read as text on a light background (4.5:1 on its cards).
    pub fn rgb(self, light: bool) -> Rgb {
        match (self, light) {
            (Tone::Good, false) => (0x6C, 0xCB, 0x5F),
            (Tone::Warn, false) => (0xF7, 0xC0, 0x4A),
            (Tone::Bad, false) => (0xFF, 0x6B, 0x6B),
            (Tone::Charge, false) => (0x60, 0xCD, 0xFF),
            (Tone::Offline, false) => (0x7A, 0x7A, 0x7A),
            (Tone::Good, true) => (0x0F, 0x7B, 0x0F),
            (Tone::Warn, true) => (0x9D, 0x5D, 0x00),
            (Tone::Bad, true) => (0xC4, 0x2B, 0x1C),
            (Tone::Charge, true) => (0x00, 0x5F, 0xB8),
            (Tone::Offline, true) => (0x8A, 0x8A, 0x8A),
        }
    }

    /// Its name for the macOS app
    pub fn name(self) -> &'static str {
        match self {
            Tone::Good => "good",
            Tone::Warn => "warn",
            Tone::Bad => "bad",
            Tone::Charge => "charge",
            Tone::Offline => "offline",
        }
    }

    /// Its number for the Windows windows (Slint has no Rust enums): the order of the variants
    pub fn index(self) -> i32 {
        self as i32
    }
}

/// The tone a device's level shows in: its own thresholds when set, the global ones otherwise.
pub fn level_tone(level: Option<i32>, charging: bool, online: bool, id: &str, s: &AppSettings) -> Tone {
    match level {
        _ if !online => Tone::Offline,
        _ if charging => Tone::Charge,
        None => Tone::Offline,
        Some(l) if l <= s.critical_for(id) => Tone::Bad,
        Some(l) if l <= s.low_for(id) => Tone::Warn,
        Some(_) => Tone::Good,
    }
}

/// One device as any part of the list shows it (a card, a "Not connected" row, a hidden row), and what its menu and
/// its name field start from.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub subtitle: String,
    pub connected: bool,
    pub pinned: bool,
    pub level_text: String,
    pub tone: Tone,
    pub charging: bool,
    /// Charging with no readable level: the bar is drawn full and faint, the lightning alone says it all
    pub indeterminate: bool,
    /// How much of the level bar is filled, 0 to 1
    pub fill: f32,
    pub custom_name: String,
    pub own_name: String,
    /// "via · own name", shown while the name is typed
    pub source: String,
    pub alerts: bool,
    pub low: i32,
    pub critical: i32,
    pub own_levels: bool,
}

/// The device list as the window shows it, block by block.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceLists<T> {
    pub pinned: Vec<T>,
    pub connected: Vec<T>,
    pub offline: Vec<T>,
    pub hidden: Vec<T>,
    /// No device of the first two blocks is connected: the window says so instead
    pub none_online: bool,
}

impl<T> DeviceLists<T> {
    pub fn map<U>(self, f: impl Fn(T) -> U) -> DeviceLists<U> {
        DeviceLists {
            pinned: self.pinned.into_iter().map(&f).collect(),
            connected: self.connected.into_iter().map(&f).collect(),
            offline: self.offline.into_iter().map(&f).collect(),
            hidden: self.hidden.into_iter().map(&f).collect(),
            none_online: self.none_online,
        }
    }
}

/// The rows in the list's order, each in the block it is shown in.
pub fn device_lists(rows: &[DeviceRow], s: &AppSettings, now: DateTime<Local>) -> DeviceLists<DeviceView> {
    let mut lists = DeviceLists {
        pinned: Vec::new(),
        connected: Vec::new(),
        offline: Vec::new(),
        hidden: Vec::new(),
        none_online: true,
    };
    for row in rows::list_order(rows, s) {
        let block = rows::block(row, s);
        // A pinned device is shown with or without a connection; a hidden one doesn't count
        if row.online() && matches!(block, Block::Pinned | Block::Connected) {
            lists.none_online = false;
        }
        let view = device_view(row, s, now);
        match block {
            Block::Pinned => lists.pinned.push(view),
            Block::Connected => lists.connected.push(view),
            Block::Offline if !s.hide_offline => lists.offline.push(view),
            Block::Offline => {}
            Block::Hidden => lists.hidden.push(view),
        }
    }
    lists
}

pub fn device_view(row: &DeviceRow, s: &AppSettings, now: DateTime<Local>) -> DeviceView {
    let mut parts = Vec::new();
    let (mut level, mut estimate, mut unknown, mut charging) = (None, false, false, false);
    let mut tone = Tone::Offline;
    if row.online() {
        let cur = row.current.as_ref().expect("online rows have a reading");
        level = cur.level;
        estimate = cur.estimate;
        unknown = cur.charging && level.is_none();
        if cur.charging {
            parts.push(t("state.charging"));
        } else if cur.full_on_charger {
            parts.push(t("state.full"));
        }
        parts.push(cur.via.clone());
        if let Some(detail) = cur.detail.as_ref().filter(|d| !d.is_empty()) {
            parts.push(detail.clone());
        }
        charging = cur.charging;
        tone = level_tone(level, cur.charging, true, &row.id, s);
    } else {
        // The reason, when the provider knows it ("asleep or off"), reads better than a bare "Not connected"
        let why = row.current.as_ref().and_then(|c| c.detail.clone()).filter(|d| !d.is_empty());
        parts.push(match why {
            Some(why) => {
                let mut chars = why.chars();
                chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
            }
            None => t("state.offline"),
        });
        if let Some(last) = &row.last {
            if let Some(was) = last.level {
                parts.push(tf("last.seen", &[&was, &ago(last.time, now)]));
            }
        }
    }
    let ds = s.device(&row.id);
    let any = row.any();
    DeviceView {
        id: row.id.clone(),
        name: row.name.clone(),
        kind: row.kind(),
        subtitle: parts.join(" · "),
        connected: row.online(),
        pinned: s.is_pinned(&row.id),
        // An estimate is marked as one; while charging with no readable level the lightning alone says it all
        level_text: match level {
            Some(l) => format!("{}{l}%", if estimate { "≈" } else { "" }),
            None if unknown => String::new(),
            None => "—".into(),
        },
        tone,
        charging,
        indeterminate: unknown,
        fill: (level.unwrap_or(0) as f32 / 100.0).clamp(0.0, 1.0),
        custom_name: ds.name.clone().unwrap_or_default(),
        own_name: any.name.clone(),
        source: format!("{} · {}", any.via, any.name),
        alerts: ds.alerts,
        low: s.low_for(&row.id),
        critical: s.critical_for(&row.id),
        own_levels: ds.low_percent.is_some() || ds.critical_percent.is_some(),
    }
}

/// "just now", "5 min ago", "3 h ago", then the day and time.
pub fn ago(time: DateTime<Local>, now: DateTime<Local>) -> String {
    let d = now - time;
    if d.num_minutes() < 1 {
        t("ago.now")
    } else if d.num_hours() < 1 {
        tf("ago.min", &[&d.num_minutes()])
    } else if d.num_days() < 1 {
        tf("ago.hours", &[&d.num_hours()])
    } else {
        loc::date_time(&time)
    }
}

/// "Updated at 15:40:53", or that the devices are still being asked.
pub fn updated_text(rows: &[DeviceRow]) -> String {
    match rows.iter().filter_map(|r| r.current.as_ref().map(|c| c.time)).max() {
        Some(time) => tf("updated.at", &[&time.format("%H:%M:%S")]),
        None => t("polling"),
    }
}

/// The updates card and the update banner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateView {
    pub status: String,
    /// When it was last asked, as a second line while there is nothing newer
    pub checked: String,
    pub install_visible: bool,
    pub install_text: String,
    pub check_enabled: bool,
    /// The release notes (Markdown, shown as plain text) of a newer version
    pub notes: String,
    pub banner_visible: bool,
    pub banner_text: String,
    pub banner_button_visible: bool,
}

pub fn update_view(u: &Status) -> UpdateView {
    let version = Version::current().to_string();
    let latest = u.latest.as_ref().map(|l| l.version.to_string()).unwrap_or_default();
    let status = match u.stage {
        Stage::Checking => t("upd.checking"),
        Stage::UpToDate => tf("upd.uptodate", &[&version]),
        Stage::Available => tf("upd.available", &[&latest, &version]),
        Stage::Installing if u.progress > 0 && u.progress < 100 => tf("upd.downloading", &[&latest, &u.progress]),
        Stage::Installing => tf("upd.installing", &[&latest]),
        Stage::Failed => tf("upd.failed", &[&u.error.clone().unwrap_or_default()]),
        Stage::NotChecked => tf("upd.idle", &[&version]),
    };
    let checked = match u.last_check {
        Some(c) if u.stage == Stage::UpToDate => tf("upd.checked", &[&c.format("%H:%M")]),
        _ => String::new(),
    };
    let newer = u.newer_available();
    let notes = if newer {
        u.latest.as_ref().map(|l| l.notes.replace("**", "").replace('`', "").trim().to_string())
    } else {
        None
    }
    .unwrap_or_default();
    let notes = if notes.chars().count() > 600 {
        notes.chars().take(600).collect::<String>().trim_end().to_string() + "…"
    } else {
        notes
    };
    UpdateView {
        checked,
        install_visible: newer && matches!(u.stage, Stage::Available | Stage::Failed),
        install_text: tf("upd.install", &[&latest]),
        check_enabled: !matches!(u.stage, Stage::Checking | Stage::Installing),
        notes,
        banner_visible: newer && matches!(u.stage, Stage::Available | Stage::Installing),
        banner_text: if u.stage == Stage::Installing { status.clone() } else { tf("upd.banner", &[&latest]) },
        banner_button_visible: u.stage == Stage::Available,
        status,
    }
}

/// The tray icon: a number in a color, and a line per device. The menu bar item on macOS shows `bar` instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayView {
    pub label: String,
    pub tone: Tone,
    pub charging: bool,
    /// One line per device on the air, in the list's order; empty when no level is known
    pub lines: Vec<String>,
    /// The lines joined, or "no data"
    pub tooltip: String,
    /// macOS: the devices in the menu bar item, as many as the settings say at most
    pub bar: Vec<BarSlot>,
}

/// One device in the menu bar item on macOS, with its kind, level and charging state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarSlot {
    pub kind: DeviceKind,
    /// "37", "≈15" for an estimate, empty when the device only says it is charging
    pub level: String,
    pub tone: Tone,
    /// Charging or full on its charger: the battery displays a lightning bolt
    pub power: bool,
}

pub fn tray_view(rows: &[DeviceRow], s: &AppSettings) -> TrayView {
    // In the order of the window's list
    let shown: Vec<&DeviceRow> =
        rows::list_order(rows, s).into_iter().filter(|r| r.online() && !s.device(&r.id).hidden).collect();
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
    let tooltip = if lines.is_empty() { t("tray.nodata") } else { lines.join("\n") };

    // The icon shows the device that needs charging most by its own levels: a critical or low non-charging one first,
    // then by the user's device-kind priority (including charging devices with no readable level), then the least
    // charged among equals.
    let live: Vec<(&DeviceRow, &DeviceReading)> = shown
        .iter()
        .filter_map(|r| r.current.as_ref().filter(|c| c.level.is_some() || c.charging).map(|c| (*r, c)))
        .collect();
    let urgency = |id: &str, charging: bool, level: Option<i32>| -> u8 {
        if charging {
            return 0;
        }
        match level {
            Some(l) if l <= s.critical_for(id) => 2,
            Some(l) if l <= s.low_for(id) => 1,
            _ => 0,
        }
    };
    let pick = live.iter().min_by_key(|(r, c)| {
        (
            std::cmp::Reverse(urgency(&r.id, c.charging, c.level)),
            tray_priority_rank(r, &s.tray_priority),
            c.charging,
            c.level.unwrap_or(100),
        )
    });
    let (label, tone) = match pick {
        None => ("--".to_string(), Tone::Offline),
        Some((r, c)) => (
            c.level.map_or_else(|| "⚡".to_string(), |level| level.to_string()),
            level_tone(c.level, c.charging, true, &r.id, s),
        ),
    };
    let charging = pick.is_some_and(|(_, c)| c.charging);
    let bar = if s.menu_bar_devices <= 1 {
        pick.map(|(r, c)| bar_slot(r, c, s)).into_iter().collect()
    } else {
        bar_slots(&live, s)
    };
    TrayView { label, tone, charging, lines, tooltip, bar }
}

/// Fills the multi-device menu bar item in device-list order, promoting low batteries into the available slots.
fn bar_slots(readable: &[(&DeviceRow, &DeviceReading)], s: &AppSettings) -> Vec<BarSlot> {
    let low = |(r, c): &(&DeviceRow, &DeviceReading)| !c.charging && c.level.is_some_and(|l| l <= s.low_for(&r.id));
    let places = s.menu_bar_devices.clamp(1, 5) as usize;
    let mut picked: Vec<_> = readable.iter().take(places).copied().collect();
    let mut left_out: Vec<_> = readable.iter().skip(places).copied().filter(low).collect();
    left_out.sort_by_key(|(_, c)| c.level);
    for device in left_out {
        let Some(place) = picked.iter().rposition(|p| !low(p)) else { break };
        picked.remove(place);
        picked.push(device);
    }
    picked.into_iter().map(|(r, c)| bar_slot(r, c, s)).collect()
}

fn bar_slot(row: &DeviceRow, reading: &DeviceReading, settings: &AppSettings) -> BarSlot {
    let power = reading.charging || reading.full_on_charger;
    BarSlot {
        kind: row.kind(),
        level: reading.level.map(|l| format!("{}{l}", if reading.estimate { "≈" } else { "" })).unwrap_or_default(),
        tone: level_tone(reading.level, power, true, &row.id, settings),
        power,
    }
}

/// Rank of a device for the tray icon when urgency is tied (lower = higher priority).
fn tray_priority_rank(row: &DeviceRow, priority: &str) -> u8 {
    match priority {
        "lowest" => 0,
        "keyboard" => match row.kind() {
            DeviceKind::Keyboard => 0,
            DeviceKind::Mouse => 1,
            _ => 2,
        },
        "bluetooth" => {
            let is_bt = row.id.starts_with("ble:")
                || row.id.starts_with("bt:")
                || row.current.as_ref().is_some_and(|c| c.via.to_lowercase().contains("bluetooth"));
            if is_bt {
                0
            } else {
                match row.kind() {
                    DeviceKind::Mouse => 1,
                    DeviceKind::Keyboard => 2,
                    _ => 3,
                }
            }
        }
        "headset" => match row.kind() {
            DeviceKind::Headset => 0,
            DeviceKind::Mouse => 1,
            DeviceKind::Keyboard => 2,
            _ => 3,
        },
        _ => match row.kind() {
            DeviceKind::Mouse => 0,
            DeviceKind::Keyboard => 1,
            _ => 2,
        },
    }
}

/// A name as the tooltip has room for it.
pub fn short(name: &str) -> String {
    if name.chars().count() > 24 {
        name.chars().take(23).collect::<String>() + "…"
    } else {
        name.to_string()
    }
}

/// The devices the overlay shows: on the air and not hidden, in the order of the list (never by level). Names are
/// shown for kinds that occur more than once, or for all when the settings say so.
pub fn overlay_select<'a>(rows: &'a [DeviceRow], settings: &AppSettings) -> Vec<(&'a DeviceRow, bool)> {
    let shown: Vec<&DeviceRow> = crate::rows::list_order(rows, settings)
        .into_iter()
        .filter(|r| r.online() && !settings.device(&r.id).hidden)
        .collect();
    let kinds: Vec<DeviceKind> = shown.iter().map(|r| r.kind()).collect();
    let duplicate = |k: DeviceKind| kinds.iter().filter(|&&x| x == k).count() > 1;
    shown.into_iter().map(|r| (r, settings.overlay_show_names || duplicate(r.kind()))).collect()
}

/// One device in the overlay.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayEntryView {
    pub kind: DeviceKind,
    pub name: String,
    pub show_name: bool,
    pub level: String,
    pub tone: Tone,
    /// Charging or full on its charger: the lightning follows the level
    pub power: bool,
}

pub fn overlay_entries(rows: &[DeviceRow], s: &AppSettings) -> Vec<OverlayEntryView> {
    overlay_select(rows, s)
        .into_iter()
        .map(|(row, show_name)| {
            let c = row.current.as_ref().expect("shown rows are on the air");
            let power = c.charging || c.full_on_charger;
            OverlayEntryView {
                kind: row.kind(),
                name: row.name.clone(),
                show_name,
                level: match c.level {
                    Some(l) => format!("{}{l}%", if c.estimate { "≈" } else { "" }),
                    None if power => String::new(),
                    None => "—".into(),
                },
                tone: level_tone(c.level, power, true, &row.id, s),
                power,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::DeviceReading;

    fn row(id: &str, kind: DeviceKind, online: bool) -> DeviceRow {
        let reading = DeviceReading { online, level: Some(50), ..DeviceReading::new(id, id, kind, "2.4 GHz") };
        DeviceRow { id: id.into(), current: Some(reading.clone()), last: Some(reading), name: id.into() }
    }

    fn ids(list: &[DeviceView]) -> Vec<&str> {
        list.iter().map(|d| d.id.as_str()).collect()
    }

    #[test]
    fn a_device_is_listed_in_one_block_in_list_order() {
        let rows = [
            row("mouse", DeviceKind::Mouse, false),
            row("hidden", DeviceKind::Headset, true),
            row("keyboard", DeviceKind::Keyboard, true),
            row("pad", DeviceKind::Gamepad, false),
            row("headset", DeviceKind::Headset, true),
            row("pinned-mouse", DeviceKind::Mouse, true),
        ];
        let mut s = AppSettings::default();
        s.device_mut("pinned-mouse").pinned = true;
        s.device_mut("pad").pinned = true;
        // Hiding a pinned device keeps its pin, and it is shown with the hidden ones only
        s.device_mut("hidden").pinned = true;
        s.device_mut("hidden").hidden = true;
        s.device_order = vec!["keyboard".into(), "headset".into()];
        let lists = device_lists(&rows, &s, Local::now());
        assert_eq!(ids(&lists.pinned), ["pinned-mouse", "pad"]);
        assert_eq!(ids(&lists.connected), ["keyboard", "headset"]);
        assert_eq!(ids(&lists.offline), ["mouse"]);
        assert_eq!(ids(&lists.hidden), ["hidden"]);
        let pad = &lists.pinned[1];
        assert_eq!((pad.connected, pad.pinned, pad.level_text.as_str()), (false, true, "—"));
        assert!(pad.subtitle.starts_with("Not connected · last: 50%"), "{}", pad.subtitle);
        let keyboard = &lists.connected[0];
        assert_eq!((keyboard.level_text.as_str(), keyboard.fill, keyboard.tone), ("50%", 0.5, Tone::Good));
    }

    #[test]
    fn no_device_online_counts_pinned_ones() {
        let mut s = AppSettings::default();
        assert!(device_lists(&[], &s, Local::now()).none_online, "no devices at all");

        let rows = [row("pinned", DeviceKind::Mouse, true), row("away", DeviceKind::Keyboard, false)];
        s.device_mut("pinned").pinned = true;
        assert!(!device_lists(&rows, &s, Local::now()).none_online, "a pinned device is connected");

        s.device_mut("pinned").hidden = true;
        assert!(device_lists(&rows, &s, Local::now()).none_online, "a hidden one doesn't count");
    }

    #[test]
    fn the_tray_shows_the_device_that_needs_charging_most() {
        let mut s = AppSettings::default();
        let at = |id: &str, level: i32, charging: bool| {
            let mut r = row(id, DeviceKind::Mouse, true);
            let c = r.current.as_mut().unwrap();
            c.level = Some(level);
            c.charging = charging;
            r
        };
        assert_eq!(tray_view(&[], &s).label, "--");
        assert_eq!(tray_view(&[], &s).tooltip, "Device Battery: no data");
        let rows = [at("a", 60, false), at("b", 15, false), at("c", 5, true)];
        let tray = tray_view(&rows, &s);
        assert_eq!((tray.label.as_str(), tray.tone), ("15", Tone::Warn));
        assert_eq!(tray.lines, ["a: 60%", "b: 15%", "c: 5% ⚡"]);
        // Its own reminder level makes the fuller one the more urgent
        s.device_mut("a").low_percent = Some(70);
        s.device_mut("a").critical_percent = Some(65);
        assert_eq!(tray_view(&rows, &s).label, "60");
        // Only charging ones left: the least charged of them
        assert_eq!(tray_view(&[at("c", 5, true), at("d", 40, true)], &s).label, "5");
    }

    #[test]
    fn the_menu_bar_shows_the_first_devices_and_one_running_out() {
        let mut s = AppSettings { menu_bar_devices: 3, ..AppSettings::default() };
        let at = |id: &str, kind: DeviceKind, level: Option<i32>, charging: bool| {
            let mut r = row(id, kind, true);
            let c = r.current.as_mut().unwrap();
            c.level = level;
            c.charging = charging;
            r
        };
        let levels = |rows: &[DeviceRow], s: &AppSettings| -> Vec<String> {
            tray_view(rows, s).bar.into_iter().map(|b| b.level).collect()
        };
        assert!(tray_view(&[], &s).bar.is_empty());

        // Fewer devices on the air, fewer shown
        let headset = at("a", DeviceKind::Headset, Some(37), false);
        assert_eq!(levels(std::slice::from_ref(&headset), &s), ["37"]);

        // In the list's order, the lightning after the level, no "%"; the phone, fifth, is below its reminder level
        // and takes the last place
        let rows = [
            headset.clone(),
            at("b", DeviceKind::Mouse, Some(62), true),
            at("c", DeviceKind::Keyboard, Some(80), false),
            at("d", DeviceKind::Gamepad, Some(54), false),
            at("e", DeviceKind::Phone, Some(15), false),
        ];
        s.device_order = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
        let bar = tray_view(&rows, &s).bar;
        assert_eq!(bar.iter().map(|b| b.level.as_str()).collect::<Vec<_>>(), ["37", "62", "15"]);
        assert_eq!((bar[1].power, bar[1].tone, bar[2].tone), (true, Tone::Charge, Tone::Warn));
        assert_eq!(bar[2].kind, DeviceKind::Phone);

        // All five fit; one place shows the one running out
        s.menu_bar_devices = 5;
        assert_eq!(levels(&rows, &s), ["37", "62", "80", "54", "15"]);
        s.menu_bar_devices = 2;
        assert_eq!(levels(&rows, &s), ["37", "15"]);
        s.menu_bar_devices = 1;
        assert_eq!(levels(&rows, &s), ["15"]);

        // Charging isn't running out, even when low; an estimate keeps its "≈"; a device without a level isn't shown
        // unless it is charging
        let mut estimate = at("f", DeviceKind::Mouse, Some(9), false);
        estimate.current.as_mut().unwrap().estimate = true;
        let rows = [
            at("a", DeviceKind::Headset, Some(37), false),
            at("b", DeviceKind::Keyboard, None, false),
            at("c", DeviceKind::Mouse, None, true),
            at("d", DeviceKind::Gamepad, Some(5), true),
            estimate,
        ];
        s.device_order = ["a", "b", "c", "d", "f"].map(String::from).to_vec();
        s.menu_bar_devices = 3;
        let bar = tray_view(&rows, &s).bar;
        assert_eq!(bar.iter().map(|b| b.level.as_str()).collect::<Vec<_>>(), ["37", "", "≈9"]);
        assert_eq!(bar[2].tone, Tone::Bad);
    }

    #[test]
    fn tray_priority_picks_preferred_kind_when_normal() {
        let mut s = AppSettings::default();
        let make = |id: &str, kind: DeviceKind, level: i32, via: &str| {
            let mut r = row(id, kind, true);
            let c = r.current.as_mut().unwrap();
            c.level = Some(level);
            c.via = via.to_string();
            r
        };
        let rows = [
            make("mouse", DeviceKind::Mouse, 85, "USB"),
            make("kb", DeviceKind::Keyboard, 60, "USB"),
            make("headset", DeviceKind::Headset, 40, "Bluetooth"),
        ];
        // Default ("mouse"): prefers mouse even though headset/keyboard have lower normal levels
        assert_eq!(tray_view(&rows, &s).label, "85");
        s.tray_priority = "keyboard".into();
        let tray = tray_view(&rows, &s);
        assert_eq!(tray.label, "60");
        assert_eq!(tray.bar.len(), 1);
        assert_eq!((tray.bar[0].kind, tray.bar[0].level.as_str()), (DeviceKind::Keyboard, "60"));
        s.tray_priority = "headset".into();
        assert_eq!(tray_view(&rows, &s).label, "40");
        s.tray_priority = "bluetooth".into();
        assert_eq!(tray_view(&rows, &s).label, "40");
        s.tray_priority = "lowest".into();
        assert_eq!(tray_view(&rows, &s).label, "40");
        // Urgency still overrides priority when a non-preferred device drops to low
        s.tray_priority = "mouse".into();
        let urgent =
            [make("mouse", DeviceKind::Mouse, 85, "USB"), make("headset", DeviceKind::Headset, 15, "Bluetooth")];
        assert_eq!(tray_view(&urgent, &s).label, "15");
        // Charging device with unknown percentage still shows in tray
        let mut charging_only = row("mouse-charging", DeviceKind::Mouse, true);
        charging_only.current.as_mut().unwrap().level = None;
        charging_only.current.as_mut().unwrap().charging = true;
        let with_kb = [charging_only, make("kb", DeviceKind::Keyboard, 80, "USB")];
        let tray = tray_view(&with_kb, &s);
        assert_eq!((tray.label.as_str(), tray.tone, tray.charging), ("⚡", Tone::Charge, true));
        assert_eq!(tray.bar.len(), 1);
        assert_eq!((tray.bar[0].kind, tray.bar[0].level.as_str(), tray.bar[0].power), (DeviceKind::Mouse, "", true));
    }

    #[test]
    fn the_update_card_by_stage() {
        let mut status = Status { stage: Stage::Checking, latest: None, last_check: None, progress: 0, error: None };
        let v = update_view(&status);
        assert!(!v.check_enabled && !v.banner_visible && !v.install_visible);
        status.stage = Stage::Available;
        status.latest = Some(crate::updater::ReleaseInfo {
            version: Version(99, 0, 0),
            tag: "v99.0.0".into(),
            notes: "**New** `thing`".into(),
            page_url: String::new(),
            exe_url: String::new(),
            exe_size: 1,
            sha256: String::new(),
        });
        let v = update_view(&status);
        assert!(v.banner_visible && v.banner_button_visible && v.install_visible && v.check_enabled);
        assert_eq!((v.notes.as_str(), v.install_text.as_str()), ("New thing", "Update to 99.0.0"));
        status.stage = Stage::Installing;
        status.progress = 40;
        let v = update_view(&status);
        assert_eq!(v.banner_text, "Downloading version 99.0.0: 40%");
        assert!(v.banner_visible && !v.banner_button_visible && !v.install_visible);
    }

    #[test]
    fn filters_offline_and_hidden_devices_without_using_remembered_levels() {
        let mut settings = AppSettings::default();
        settings.device_mut("hidden").hidden = true;
        let offline = row("offline", DeviceKind::Mouse, false);
        let remembered = DeviceRow { id: "remembered".into(), current: None, ..offline.clone() };
        let rows =
            [row("shown", DeviceKind::Keyboard, true), offline, remembered, row("hidden", DeviceKind::Mouse, true)];
        let result = overlay_select(&rows, &settings);
        assert_eq!(result.iter().map(|r| r.0.id.as_str()).collect::<Vec<_>>(), ["shown"]);
    }

    #[test]
    fn names_are_shown_only_for_duplicate_kinds_unless_enabled_for_all() {
        let rows = [
            row("m1", DeviceKind::Mouse, true),
            row("m2", DeviceKind::Mouse, true),
            row("k", DeviceKind::Keyboard, true),
        ];
        let mut settings = AppSettings::default();
        for (r, show) in overlay_select(&rows, &settings) {
            assert_eq!(show, r.kind() == DeviceKind::Mouse);
        }
        settings.overlay_show_names = true;
        assert!(overlay_select(&rows, &settings).iter().all(|r| r.1));
    }

    #[test]
    fn order_does_not_follow_battery_levels() {
        let rows: Vec<DeviceRow> = (0..20).rev().map(|i| row(&format!("{i:02}"), DeviceKind::Headset, true)).collect();
        let settings = AppSettings::default();
        let before: Vec<String> = overlay_select(&rows, &settings).iter().map(|r| r.0.id.clone()).collect();
        let changed: Vec<DeviceRow> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut r = r.clone();
                r.current.as_mut().unwrap().level = Some(i as i32);
                r
            })
            .collect();
        let after: Vec<String> = overlay_select(&changed, &settings).iter().map(|r| r.0.id.clone()).collect();
        assert_eq!(before, after);
        assert_eq!(before.len(), 20);
    }

    #[test]
    fn the_order_is_the_lists() {
        let rows = [
            row("mouse", DeviceKind::Mouse, true),
            row("off", DeviceKind::Headset, false),
            row("headset", DeviceKind::Headset, true),
            row("keys", DeviceKind::Keyboard, true),
            row("pad", DeviceKind::Gamepad, true),
        ];
        let shown = |settings: &AppSettings| -> Vec<String> {
            overlay_select(&rows, settings).iter().map(|r| r.0.id.clone()).collect()
        };
        let mut settings = AppSettings::default();
        assert_eq!(shown(&settings), ["headset", "mouse", "keys", "pad"], "by kind and name until the user orders it");
        // The pinned one first, then the others as the user put them; what is off or hidden is left out as before
        settings.device_order = ["pad", "off", "keys", "headset", "mouse"].map(String::from).to_vec();
        settings.device_mut("mouse").pinned = true;
        assert_eq!(shown(&settings), ["mouse", "pad", "keys", "headset"]);
        settings.device_mut("keys").hidden = true;
        assert_eq!(shown(&settings), ["mouse", "pad", "headset"]);
    }

    #[test]
    fn overlay_entries_mark_estimates_and_power() {
        let mut charging = row("c", DeviceKind::Mouse, true);
        charging.current.as_mut().unwrap().level = None;
        charging.current.as_mut().unwrap().charging = true;
        let mut estimate = row("e", DeviceKind::Keyboard, true);
        estimate.current.as_mut().unwrap().estimate = true;
        let entries = overlay_entries(&[charging, estimate], &AppSettings::default());
        assert_eq!((entries[0].level.as_str(), entries[0].power), ("", true));
        assert_eq!((entries[1].level.as_str(), entries[1].power), ("≈50%", false));
    }
}
