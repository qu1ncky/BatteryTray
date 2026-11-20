//! Devices as the app shows them and the order the list has them in, and the two decisions made from them: a
//! finished charge to announce and low battery reminders due.

use crate::reading::{DeviceKind, DeviceReading};
use crate::settings::AppSettings;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// A device as the UI shows it: what the provider reports now, plus the last level we ever saw.
#[derive(Clone, Debug)]
pub struct DeviceRow {
    pub id: String,
    pub current: Option<DeviceReading>,
    pub last: Option<DeviceReading>,
    pub name: String,
}

impl DeviceRow {
    pub fn any(&self) -> &DeviceReading {
        self.current.as_ref().or(self.last.as_ref()).expect("a row has a current or a last reading")
    }

    pub fn kind(&self) -> DeviceKind {
        self.any().kind
    }

    pub fn online(&self) -> bool {
        self.current.as_ref().is_some_and(|c| c.online)
    }
}

/// Every device known: what the providers report now, plus devices only remembered from before; by kind, then name.
pub fn rows(
    current: Vec<DeviceReading>,
    remembered: &[(String, Option<DeviceReading>)],
    settings: &AppSettings,
) -> Vec<DeviceRow> {
    let mut rows: Vec<DeviceRow> = Vec::new();
    for r in current {
        match rows.iter_mut().find(|row| row.id == r.id) {
            Some(row) => row.current = Some(r),
            None => rows.push(DeviceRow { id: r.id.clone(), current: Some(r), last: None, name: String::new() }),
        }
    }
    for (id, last) in remembered {
        match rows.iter_mut().find(|row| &row.id == id) {
            Some(row) => row.last = last.clone(),
            None if last.is_some() => {
                rows.push(DeviceRow { id: id.clone(), current: None, last: last.clone(), name: String::new() })
            }
            None => {}
        }
    }
    for row in &mut rows {
        row.name = settings.display_name(&row.id, &row.any().name);
    }
    rows.sort_by_key(|r| (r.kind() as u8, r.name.to_lowercase()));
    rows
}

/// The rows in the order of the list, which the overlay and the tray's tooltip follow: the pinned devices first, and
/// each of the two groups in the order the user put its devices in. A device without a place there comes after those
/// that have one, by kind, then name - which is every device until the first pin or drag.
pub fn list_order<'a>(rows: &'a [DeviceRow], settings: &AppSettings) -> Vec<&'a DeviceRow> {
    let mut ordered: Vec<&DeviceRow> = rows.iter().collect();
    ordered.sort_by_cached_key(|r| {
        let place = settings.device_order.iter().position(|id| *id == r.id).unwrap_or(usize::MAX);
        // The id last: two devices of one kind and name keep their order from one poll to the next
        (!settings.is_pinned(&r.id), place, r.kind() as u8, r.name.to_lowercase(), r.id.clone())
    });
    ordered
}

/// The part of the list a device is shown in. A drag stays within one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    /// A card at the top, connected or not.
    Pinned,
    /// A card: on the air.
    Connected,
    /// A row under "Not connected".
    Offline,
    /// A row under "Hidden", whatever else is true of the device.
    Hidden,
}

/// Where the list shows a device.
pub fn block(row: &DeviceRow, settings: &AppSettings) -> Block {
    let device = settings.device(&row.id);
    if device.hidden {
        Block::Hidden
    } else if device.pinned {
        Block::Pinned
    } else if row.online() {
        Block::Connected
    } else {
        Block::Offline
    }
}

/// Gives every device a place in the user's order, if it has none yet: after the last one, which is where the list
/// has it already. So the first pin or drag keeps the order it finds. True when a device was added.
fn adopt(settings: &mut AppSettings, rows: &[DeviceRow]) -> bool {
    let new: Vec<String> = list_order(rows, settings)
        .into_iter()
        .filter(|r| !settings.device_order.contains(&r.id))
        .map(|r| r.id.clone())
        .collect();
    settings.device_order.extend_from_slice(&new);
    !new.is_empty()
}

/// Once the user has put the list in order, a device seen for the first time gets its place for good: the end of its
/// block. True when there was such a device, and the settings are worth saving.
pub fn adopt_new(settings: &mut AppSettings, rows: &[DeviceRow]) -> bool {
    !settings.device_order.is_empty() && adopt(settings, rows)
}

/// Pins a device, which makes it the last of the pinned ones, or unpins it, which makes it the first of the others.
/// True when that changed anything.
pub fn set_pinned(settings: &mut AppSettings, rows: &[DeviceRow], id: &str, pinned: bool) -> bool {
    if settings.is_pinned(id) == pinned {
        return false;
    }
    adopt(settings, rows);
    settings.device_order.retain(|other| other != id);
    // The pinned devices are shown in the order they are named in, wherever the others stand between them, and so are
    // the others: the end of the whole order is the end of the pinned ones, and its start the start of the rest
    if pinned {
        settings.device_order.push(id.to_string());
    } else {
        settings.device_order.insert(0, id.to_string());
    }
    settings.device_mut(id).pinned = pinned;
    true
}

/// Moves a device to the place of another one in its block, which makes room: dragged down it lands right below that
/// one, dragged up right above it. No other device changes its place among the rest, in this block or in any other.
/// Refused (false) unless the list shows the two in the same block, and for hidden ones: a drag never leaves its
/// block.
pub fn move_onto(settings: &mut AppSettings, rows: &[DeviceRow], id: &str, onto: &str) -> bool {
    let block_of = |id: &str| rows.iter().find(|r| r.id == id).map(|r| block(r, settings));
    let (Some(own), Some(other)) = (block_of(id), block_of(onto)) else { return false };
    if id == onto || own != other || own == Block::Hidden {
        return false;
    }
    adopt(settings, rows);
    let order = &mut settings.device_order;
    let place = |order: &[String], id: &str| order.iter().position(|other| other == id);
    if let (Some(from), Some(to)) = (place(order, id), place(order, onto)) {
        // Taken out first, the device goes in at the index the other one had. If the other one stood further down,
        // it has moved up by one meanwhile and the device lands right below it; if further up, right above it
        let moved = order.remove(from);
        order.insert(to, moved);
    }
    true
}

/// Spots the moment a charge finishes: a device that was charging reports itself full with the charger still in.
/// Only devices that say so count - many report nothing new until the cable comes out. One announcement per charge:
/// chargers top a full battery up now and then, which must not repeat it.
#[derive(Default)]
pub struct ChargeDoneWatcher {
    was_charging: HashMap<String, bool>,
    announced: HashSet<String>,
}

impl ChargeDoneWatcher {
    const REARM_PERCENT: i32 = 90; // below this a new charge may be announced again

    /// The rows whose charge finished since the last call.
    pub fn finished<'a>(&mut self, rows: &'a [DeviceRow]) -> Vec<&'a DeviceRow> {
        let mut done = Vec::new();
        for row in rows {
            let Some(r) = &row.current else { continue };
            if !r.online {
                // Turned off or out of range: whatever it does meanwhile wasn't seen
                self.was_charging.remove(&row.id);
                continue;
            }
            if r.level.is_some_and(|l| l <= Self::REARM_PERCENT) {
                self.announced.remove(&row.id);
            }
            let was = self.was_charging.insert(row.id.clone(), r.charging).unwrap_or(false);
            // Already full when first seen (e.g. the app started overnight) is no news
            if was && r.full_on_charger && self.announced.insert(row.id.clone()) {
                done.push(row);
            }
        }
        done
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertItem {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub level: i32,
    pub critical: bool,
}

#[derive(Default)]
struct AlertState {
    snoozed_until: Option<Instant>,
    muted: bool,
    critical_seen: bool,
}

/// Decides which devices need a low battery reminder right now. Each device alerts at its own level when one is set,
/// and at the global one otherwise. A reminder stays due until the user snoozes it (it comes back after the repeat
/// interval) or mutes it (quiet until the device is charged). Reaching the critical level breaks through a snooze or
/// mute once. Charging, or a level clearly above the threshold, re-arms everything.
#[derive(Default)]
pub struct AlertManager {
    states: HashMap<String, AlertState>,
}

impl AlertManager {
    const REARM_MARGIN: i32 = 5;

    pub fn due(&mut self, rows: &[DeviceRow], settings: &AppSettings, now: Instant) -> Vec<AlertItem> {
        let mut due = Vec::new();
        for row in rows {
            let Some(r) = &row.current else { continue }; // provider lost the device: keep its state as is
            let ds = settings.device(&row.id);
            let low = settings.low_for(&row.id);
            if ds.hidden || !ds.alerts || r.charging || r.level.is_some_and(|l| l > low + Self::REARM_MARGIN) {
                self.states.remove(&row.id);
                continue;
            }
            let Some(level) = r.level.filter(|&l| r.online && l <= low) else { continue };

            let st = self.states.entry(row.id.clone()).or_default();
            let critical = level <= settings.critical_for(&row.id);
            if critical && !st.critical_seen {
                st.critical_seen = true;
                st.muted = false;
                st.snoozed_until = None;
            }
            if st.muted || st.snoozed_until.is_some_and(|until| now < until) {
                continue;
            }
            due.push(AlertItem { id: row.id.clone(), name: row.name.clone(), kind: row.kind(), level, critical });
        }
        due
    }

    pub fn snooze(&mut self, ids: &[String], duration: Duration) {
        for id in ids {
            self.states.entry(id.clone()).or_default().snoozed_until = Some(Instant::now() + duration);
        }
    }

    pub fn mute_until_charged(&mut self, ids: &[String]) {
        for id in ids {
            self.states.entry(id.clone()).or_default().muted = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(level: i32, charging: bool, full: bool) -> DeviceRow {
        let r = DeviceReading {
            online: true,
            level: Some(level),
            charging,
            full_on_charger: full,
            ..DeviceReading::new("m", "Mouse", DeviceKind::Mouse, "2.4 GHz")
        };
        DeviceRow { id: "m".into(), current: Some(r), last: None, name: "Mouse".into() }
    }

    fn device(id: &str, kind: DeviceKind, name: &str, online: bool) -> DeviceReading {
        DeviceReading { online, ..DeviceReading::new(id, name, kind, "test") }
    }

    /// A headset, a mouse and a keyboard on the air, a second keyboard and a gamepad that are off: by kind and name
    /// that is headset, mouse, old, keys, pad.
    fn devices() -> Vec<DeviceReading> {
        vec![
            device("pad", DeviceKind::Gamepad, "Xbox", false),
            device("keys", DeviceKind::Keyboard, "Rainy 75", true),
            device("old", DeviceKind::Keyboard, "MX Keys", false),
            device("mouse", DeviceKind::Mouse, "Attack Shark R3", true),
            device("headset", DeviceKind::Headset, "Barracuda X", true),
        ]
    }

    /// The whole list, top to bottom.
    fn list(devices: &[DeviceReading], settings: &AppSettings) -> Vec<String> {
        list_order(&rows(devices.to_vec(), &[], settings), settings).iter().map(|r| r.id.clone()).collect()
    }

    /// What the list shows in one block, top to bottom.
    fn shown(devices: &[DeviceReading], settings: &AppSettings, in_block: Block) -> Vec<String> {
        let rows = rows(devices.to_vec(), &[], settings);
        list_order(&rows, settings).iter().filter(|r| block(r, settings) == in_block).map(|r| r.id.clone()).collect()
    }

    fn pin(settings: &mut AppSettings, devices: &[DeviceReading], id: &str, pinned: bool) -> bool {
        set_pinned(settings, &rows(devices.to_vec(), &[], settings), id, pinned)
    }

    fn drag(settings: &mut AppSettings, devices: &[DeviceReading], id: &str, onto: &str) -> bool {
        move_onto(settings, &rows(devices.to_vec(), &[], settings), id, onto)
    }

    /// What the app does after every poll.
    fn poll(settings: &mut AppSettings, devices: &[DeviceReading]) -> bool {
        adopt_new(settings, &rows(devices.to_vec(), &[], settings))
    }

    #[test]
    fn the_list_goes_by_kind_then_name_until_the_user_orders_it() {
        let mut settings = AppSettings::default();
        assert_eq!(list(&devices(), &settings), ["headset", "mouse", "old", "keys", "pad"]);
        // By the name the user gave, whatever its case
        settings.set_name("old", "Sofa keyboard");
        assert_eq!(list(&devices(), &settings), ["headset", "mouse", "keys", "old", "pad"]);
        assert!(settings.device_order.is_empty(), "nothing is kept for it");
    }

    #[test]
    fn two_devices_of_one_kind_and_name_go_by_id() {
        let twins =
            [device("ry:b", DeviceKind::Mouse, "Mouse", true), device("ry:a", DeviceKind::Mouse, "mouse", true)];
        let mut settings = AppSettings::default();
        assert_eq!(list(&twins, &settings), ["ry:a", "ry:b"]);
        let swapped = [twins[1].clone(), twins[0].clone()];
        assert_eq!(list(&swapped, &settings), ["ry:a", "ry:b"], "whichever the providers report first");
        // And that is the order the first pin keeps, not the one the readings came in
        let all = [twins[0].clone(), twins[1].clone(), device("pad", DeviceKind::Gamepad, "Xbox", true)];
        pin(&mut settings, &all, "pad", true);
        assert_eq!(list(&all, &settings), ["pad", "ry:a", "ry:b"]);
    }

    #[test]
    fn a_device_is_shown_in_one_block() {
        let devices = devices();
        let mut settings = AppSettings::default();
        pin(&mut settings, &devices, "pad", true);
        settings.device_mut("old").hidden = true;
        assert_eq!(shown(&devices, &settings, Block::Pinned), ["pad"], "pinned while off: still at the top");
        assert_eq!(shown(&devices, &settings, Block::Connected), ["headset", "mouse", "keys"]);
        assert!(shown(&devices, &settings, Block::Offline).is_empty());
        assert_eq!(shown(&devices, &settings, Block::Hidden), ["old"]);
        // A hidden device is under "Hidden" and nowhere else, pinned or not
        settings.device_mut("pad").hidden = true;
        assert!(shown(&devices, &settings, Block::Pinned).is_empty());
        assert_eq!(shown(&devices, &settings, Block::Hidden), ["pad", "old"]);
    }

    #[test]
    fn the_first_pin_keeps_the_order_it_finds() {
        let devices = devices();
        let mut settings = AppSettings::default();
        assert!(pin(&mut settings, &devices, "pad", true));
        assert_eq!(list(&devices, &settings), ["pad", "headset", "mouse", "old", "keys"]);
        // From here on a name moves nothing: by name this keyboard would now come after the other one
        settings.set_name("old", "Sofa keyboard");
        assert_eq!(list(&devices, &settings), ["pad", "headset", "mouse", "old", "keys"]);
        let pinned = settings.clone();
        assert!(!pin(&mut settings, &devices, "pad", true), "pinned already");
        assert!(!pin(&mut settings, &devices, "keys", false), "not pinned anyway");
        assert_eq!(settings, pinned);
    }

    #[test]
    fn a_pinned_device_goes_last_of_the_pinned_and_an_unpinned_one_first_of_the_rest() {
        let devices = devices();
        let mut settings = AppSettings::default();
        pin(&mut settings, &devices, "pad", true);
        pin(&mut settings, &devices, "headset", true);
        assert_eq!(shown(&devices, &settings, Block::Pinned), ["pad", "headset"]);
        assert_eq!(shown(&devices, &settings, Block::Connected), ["mouse", "keys"]);
        assert_eq!(shown(&devices, &settings, Block::Offline), ["old"]);
        // Unpinned: the first of its block, on the air or not
        pin(&mut settings, &devices, "pad", false);
        assert_eq!(shown(&devices, &settings, Block::Offline), ["pad", "old"]);
        pin(&mut settings, &devices, "headset", false);
        assert_eq!(shown(&devices, &settings, Block::Connected), ["headset", "mouse", "keys"]);
        assert!(shown(&devices, &settings, Block::Pinned).is_empty());
    }

    #[test]
    fn a_drag_moves_one_device_within_its_block() {
        let devices = devices();
        let mut settings = AppSettings::default();
        // Dragged up: above the one it is dropped on
        assert!(drag(&mut settings, &devices, "keys", "headset"));
        assert_eq!(shown(&devices, &settings, Block::Connected), ["keys", "headset", "mouse"]);
        assert_eq!(shown(&devices, &settings, Block::Offline), ["old", "pad"], "the other block is as it was");
        // Dragged down: below it
        assert!(drag(&mut settings, &devices, "keys", "mouse"));
        assert_eq!(shown(&devices, &settings, Block::Connected), ["headset", "mouse", "keys"]);
        assert!(drag(&mut settings, &devices, "old", "pad"));
        assert_eq!(shown(&devices, &settings, Block::Offline), ["pad", "old"]);
        assert_eq!(shown(&devices, &settings, Block::Connected), ["headset", "mouse", "keys"]);
        // The pinned ones among themselves, connected or not
        pin(&mut settings, &devices, "pad", true);
        pin(&mut settings, &devices, "mouse", true);
        assert!(drag(&mut settings, &devices, "mouse", "pad"));
        assert_eq!(shown(&devices, &settings, Block::Pinned), ["mouse", "pad"]);
        assert_eq!(shown(&devices, &settings, Block::Connected), ["headset", "keys"]);
    }

    #[test]
    fn a_drag_passes_no_more_devices_than_it_has_to() {
        // a, c and e are on the air, b and d between them are off
        let mut devices: Vec<DeviceReading> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| device(id, DeviceKind::Mouse, id, *id != "b" && *id != "d"))
            .collect();
        let mut settings = AppSettings::default();
        assert!(drag(&mut settings, &devices, "e", "c"));
        // e went over d only because d stood between it and c; b, further up, is still above it
        assert_eq!(settings.device_order, ["a", "b", "e", "c", "d"]);
        assert!(drag(&mut settings, &devices, "a", "e"));
        assert_eq!(settings.device_order, ["b", "e", "a", "c", "d"]);
        // Which shows once they are all on the air
        for d in &mut devices {
            d.online = true;
        }
        assert_eq!(shown(&devices, &settings, Block::Connected), ["b", "e", "a", "c", "d"]);
    }

    #[test]
    fn a_drag_never_leaves_its_block() {
        let devices = devices();
        let mut settings = AppSettings::default();
        pin(&mut settings, &devices, "mouse", true);
        settings.device_mut("keys").hidden = true;
        settings.device_mut("pad").hidden = true;
        let before = settings.clone();
        for (id, onto) in [
            ("headset", "old"),     // on the air onto one that is off
            ("old", "headset"),     // and the other way round
            ("headset", "mouse"),   // onto a pinned one
            ("mouse", "headset"),   // a pinned one out of its block
            ("keys", "headset"),    // a hidden one
            ("headset", "keys"),    // onto a hidden one
            ("keys", "pad"),        // under "Hidden" nothing is dragged at all
            ("headset", "headset"), // onto itself
            ("headset", "gone"),    // onto a device the list doesn't have
            ("gone", "headset"),
        ] {
            assert!(!drag(&mut settings, &devices, id, onto), "{id} onto {onto}");
        }
        assert_eq!(settings, before);
        // A drag that is refused is no reason to stop going by kind and name
        let mut untouched = AppSettings::default();
        assert!(!drag(&mut untouched, &devices, "headset", "old"));
        assert_eq!(untouched, AppSettings::default());
    }

    #[test]
    fn a_device_seen_for_the_first_time_goes_last_in_its_block() {
        let mut devices = devices();
        let mut settings = AppSettings::default();
        // Nothing ordered yet: it goes where its kind and name put it, and nothing is kept
        devices.push(device("new", DeviceKind::Headset, "Aaa", true));
        assert_eq!(shown(&devices, &settings, Block::Connected), ["new", "headset", "mouse", "keys"]);
        assert!(!poll(&mut settings, &devices));
        assert!(settings.device_order.is_empty());

        // Ordered by the user before it turns up: last, also before it is given its place for good
        devices.pop();
        drag(&mut settings, &devices, "keys", "headset");
        devices.push(device("new", DeviceKind::Headset, "Aaa", true));
        assert_eq!(shown(&devices, &settings, Block::Connected), ["keys", "headset", "mouse", "new"]);
        assert!(poll(&mut settings, &devices));
        assert!(!poll(&mut settings, &devices), "once");
        assert_eq!(settings.device_order.last().map(String::as_str), Some("new"));
        // The next one comes after it, though its name would put it first
        devices.push(device("newer", DeviceKind::Headset, "Aa", true));
        devices.push(device("off", DeviceKind::Headset, "A", false));
        assert!(poll(&mut settings, &devices));
        assert_eq!(shown(&devices, &settings, Block::Connected), ["keys", "headset", "mouse", "new", "newer"]);
        assert_eq!(shown(&devices, &settings, Block::Offline), ["old", "pad", "off"]);
    }

    #[test]
    fn a_device_keeps_its_place_while_it_is_hidden_off_or_gone() {
        let mut devices = devices();
        let mut settings = AppSettings::default();
        drag(&mut settings, &devices, "mouse", "headset");
        assert_eq!(shown(&devices, &settings, Block::Connected), ["mouse", "headset", "keys"]);

        settings.device_mut("headset").hidden = true;
        assert_eq!(shown(&devices, &settings, Block::Connected), ["mouse", "keys"]);
        settings.device_mut("headset").hidden = false;
        assert_eq!(shown(&devices, &settings, Block::Connected), ["mouse", "headset", "keys"]);

        // Off: among the rows under "Not connected" by the same order; back on the air, where it was
        devices.iter_mut().find(|d| d.id == "headset").unwrap().online = false;
        assert_eq!(shown(&devices, &settings, Block::Connected), ["mouse", "keys"]);
        assert_eq!(shown(&devices, &settings, Block::Offline), ["headset", "old", "pad"]);
        devices.iter_mut().find(|d| d.id == "headset").unwrap().online = true;

        // Gone (its dongle unplugged) while the others are moved, and back
        let without: Vec<DeviceReading> = devices.iter().filter(|d| d.id != "mouse").cloned().collect();
        assert!(drag(&mut settings, &without, "keys", "headset"));
        assert_eq!(shown(&without, &settings, Block::Connected), ["keys", "headset"]);
        assert!(!poll(&mut settings, &devices), "it is no new device");
        assert_eq!(shown(&devices, &settings, Block::Connected), ["mouse", "keys", "headset"]);
    }

    #[test]
    fn pins_set_in_the_file_by_hand_work_without_an_order() {
        let devices = devices();
        let mut settings = AppSettings::default();
        settings.device_mut("pad").pinned = true;
        settings.device_mut("keys").pinned = true;
        // Both halves by kind and name, and still no order to add a new device to
        assert_eq!(list(&devices, &settings), ["keys", "pad", "headset", "mouse", "old"]);
        assert!(!poll(&mut settings, &devices));
        // The first drag or pin keeps that picture
        assert!(drag(&mut settings, &devices, "pad", "keys"));
        assert_eq!(list(&devices, &settings), ["pad", "keys", "headset", "mouse", "old"]);
        assert!(pin(&mut settings, &devices, "headset", true));
        assert!(pin(&mut settings, &devices, "keys", false));
        assert_eq!(list(&devices, &settings), ["pad", "headset", "keys", "mouse", "old"]);
    }

    #[test]
    fn the_users_order_is_the_lists_alone() {
        // The reminder, which goes by the rows as they come, lists its devices as it always did
        let devices = devices();
        let mut settings = AppSettings::default();
        let by_kind_and_name: Vec<String> = rows(devices.clone(), &[], &settings).into_iter().map(|r| r.id).collect();
        pin(&mut settings, &devices, "pad", true);
        drag(&mut settings, &devices, "keys", "headset");
        assert_eq!(list(&devices, &settings), ["pad", "keys", "headset", "mouse", "old"]);
        let rows: Vec<String> = rows(devices, &[], &settings).into_iter().map(|r| r.id).collect();
        assert_eq!(rows, by_kind_and_name);
        assert_eq!(rows, ["headset", "mouse", "old", "keys", "pad"]);
    }

    #[test]
    fn a_finished_charge_is_announced_once() {
        let mut w = ChargeDoneWatcher::default();
        assert!(w.finished(&[row(100, false, true)]).is_empty(), "already full when first seen");
        assert!(w.finished(&[row(80, true, false)]).is_empty());
        assert_eq!(w.finished(&[row(100, false, true)]).len(), 1);
        assert!(w.finished(&[row(99, true, false)]).is_empty());
        assert!(w.finished(&[row(100, false, true)]).is_empty(), "a top-up is no news");
    }

    #[test]
    fn reminder_snooze_and_critical() {
        let settings = AppSettings::default(); // 20% and 10%
        let mut alerts = AlertManager::default();
        let now = Instant::now();
        assert!(alerts.due(&[row(25, false, false)], &settings, now).is_empty());
        assert_eq!(alerts.due(&[row(18, false, false)], &settings, now).len(), 1);
        alerts.snooze(&["m".into()], Duration::from_secs(600));
        assert!(alerts.due(&[row(17, false, false)], &settings, Instant::now()).is_empty(), "snoozed");
        let critical = alerts.due(&[row(9, false, false)], &settings, Instant::now());
        assert!(critical.len() == 1 && critical[0].critical, "critical breaks through the snooze");
        alerts.mute_until_charged(&["m".into()]);
        assert!(alerts.due(&[row(8, false, false)], &settings, Instant::now()).is_empty(), "muted");
        assert!(alerts.due(&[row(8, true, false)], &settings, Instant::now()).is_empty(), "charging re-arms");
        assert_eq!(alerts.due(&[row(8, false, false)], &settings, Instant::now()).len(), 1);
    }
}
