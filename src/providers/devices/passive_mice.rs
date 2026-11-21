//! Mice that send their battery by themselves. Nothing is ever written to them: each status collection is read on
//! its own GENERIC_READ handle by a listener that stays between polls, because the reports can be minutes apart.
//! That is why these are not table rows, and `SPECS` is empty.
//!
//! Rapoo VT mice (vendor ID 24AE; every model has its own receiver ID and cable ID). The status comes on the vendor
//! collection FF00:0002:
//!   Nordic 54L firmware (the VT0 .. VT9 lines Rapoo Hub drives): input report 07, 13 to 19 bytes with the ID:
//!     07 SL GG XX XX YY YY ST PP .. .. .. .. II II .. .. .. RF
//!     L (low nibble) the link: 0 receiver, 1 Bluetooth, 2 cable. ST: 1 on battery, 2 or 3 charging, 0 no reading.
//!     PP percent. II II the mouse's own (cable) product ID, in reports of 15 bytes and more. RF, in reports of 19
//!     bytes: bit 7 = the mouse is linked to the receiver, the rest the signal strength.
//!   Older firmware (VT9 Pro, VT3 Pro, VT3 Pro Max, VT1 Pro Max): input report BB, 7 bytes with the ID:
//!     BB B0 MM XX XX ST PP    ST: 1 on battery, 2 charging, 0 in between. PP percent, or 7F while charging for
//!     "not known". Other firmware of that age gives these bytes another meaning, so only the IDs below are read.
//!   A receiver sends the report about every 3 s while the mouse is awake and goes quiet when it sleeps; on the
//!   cable it comes only now and then (on a DPI change, once in 16 s in a capture).
//!   A VT9 on its cable with the power switch off reports "charging, 1%" whatever the level is, so that pair is
//!   shown as charging without a level.
//!
//! Incott mice on PixArt's 8K receiver 093A:522C, and on their cable 093A:622C (G23, G23 V2, G24, Ghero, Zero 29,
//! Zero 39): input report 09 on the vendor collection FF05:0001:
//!     09 BB SP ..    BB: bit 7 charging, the rest percent; SP the DPI stage and the polling rate.
//!   The mouse sends it only while it is being used, and no request returns it (the feature reads that look like a
//!   battery query return constants). On the cable there are two such collections and only one is alive, so both
//!   are listened to. Wraith's W1 uses the same IDs with another command set and sends this report only to its own
//!   software, so the receiver is listed once a report of ours was seen and is named by its product string.
//!
//! Silence from a receiver is the mouse asleep or off. On the cable the mouse is there and on power, but the level
//! it last sent goes out of date: after a few minutes it is shown as charging without a level.
//!
//! Protocol facts from Wireless-Device-Battery-Monitor (docs/rapoo.md, src/drivers/rapoo.cpp, rapoo_protocol.h,
//! tests/rapoo_protocol_test.cpp, src/drivers/misc_mice.cpp; issues 136, 584, 1396), Rapoo Hub (hub.rapoo.cn: the
//! VT_nrf54L report decoder and the hardware manifests, which is where the Nordic product IDs come from),
//! Device-Battery-Info (Sources/Rapoo/RapooProtocol.cs), zbndev/rapoo_battery, OpenMouse (mouse-protocol:
//! docs/rapoo-vt9-pro-testing.md, captures/rapoo-vt9-pro, docs/incott-testing.md,
//! captures/incott-8k-wireless/input-report-battery.hex, src/incott/index.ts) and Incott-Manager (device.go).
//! Own implementation. Not yet verified on hardware by this project, which is why the collections, the first status
//! report and the reports that weren't understood go to the log.

use super::Spec;
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::monitor::{no_push, Pusher};
use crate::providers::{
    group_by, hex_trimmed as hex, no_link_reason, receiver_name, secs, sleep_ms, ClearOnDrop, Provider,
};
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Nothing here answers a request, so there are no rows: see `PassiveMiceProvider`.
pub static SPECS: &[Spec] = &[];

const RAPOO: u16 = 0x24AE;
const PIXART: u16 = 0x093A;
const STATUS_REPORT: u8 = 0x07;
const OLD_STATUS_REPORT: u8 = 0xBB;
const INCOTT_REPORT: u8 = 0x09;
/// A Rapoo receiver sends every 3 s or so while the mouse is awake; this much silence means it's asleep or off.
const HEARTBEAT_QUIET: Duration = secs(30);
/// An Incott mouse sends only while it's being used: after this long without a report it is taken for asleep.
const IN_USE_QUIET: Duration = secs(600);
/// On the cable the reports are rare and the level rises meanwhile: how long the last one is still shown.
const CABLE_QUIET: Duration = secs(300);
/// How long a poll waits for a new receiver's first report, so the mouse doesn't show up as asleep for a moment.
const FIRST_REPORT_WAIT: Duration = secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    Receiver,
    Cable,
}

/// Which status report a device sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
    Nordic,
    Old,
    Incott,
}

impl Frame {
    /// Whether this is the collection the report comes on. Rapoo's older mice have a second FF00:0002 collection
    /// with 10-byte reports, and the Nordic ones differ in length by receiver (13, 16 or 19 bytes).
    fn fits(self, info: &HidInfo) -> bool {
        let (page, usage, length) = (info.usage_page, info.usage, info.input_length);
        match self {
            Frame::Nordic => page == 0xFF00 && usage == 0x0002 && (9..=64).contains(&length),
            Frame::Old => page == 0xFF00 && usage == 0x0002 && length == 7,
            Frame::Incott => page == 0xFF05 && usage == 0x0001 && length >= 8,
        }
    }

    fn parser(self) -> fn(&[u8]) -> Option<Battery> {
        match self {
            Frame::Nordic => parse_nordic,
            Frame::Old => parse_old,
            Frame::Incott => parse_incott,
        }
    }

    /// How long a report counts as the current state.
    fn keep(self, link: Link) -> Duration {
        match (self, link) {
            (_, Link::Cable) => CABLE_QUIET,
            (Frame::Incott, Link::Receiver) => IN_USE_QUIET,
            (_, Link::Receiver) => HEARTBEAT_QUIET,
        }
    }

    /// The receiver sends by itself all the time the mouse is awake, so a first report is worth a short wait.
    fn heartbeat(self) -> bool {
        self != Frame::Incott
    }

    /// Other brands' mice carry the same USB IDs.
    fn shared(self) -> bool {
        self == Frame::Incott
    }
}

/// What one status report says. `level` is None when the report has none to give. `mouse`: the product ID of the
/// mouse itself, when the report carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Battery {
    level: Option<i32>,
    charging: bool,
    mouse: Option<u16>,
}

/// Rapoo's report 07, with the report ID first. None when it isn't one or has no reading in it.
fn parse_nordic(report: &[u8]) -> Option<Battery> {
    if report.len() < 9 || report[0] != STATUS_REPORT {
        return None;
    }
    let link = report[1] & 0x0F;
    let (state, percent) = (report[7], report[8] as i32);
    if link > 2 || !(1..=3).contains(&state) || percent > 100 {
        return None;
    }
    // A receiver that says the mouse isn't linked has nothing current to tell about it
    if link == 0 && report.len() >= 19 && report[18] & 0x80 == 0 {
        return None;
    }
    let charging = state != 1;
    let level = match percent {
        // 0 is what a mouse sends before it has measured: unknown, not empty
        0 => None,
        // The VT9 on its cable with the power switch off
        1 if link == 2 && charging => None,
        _ => Some(percent),
    };
    let mouse = (report.len() >= 15).then(|| u16::from_le_bytes([report[13], report[14]])).filter(|&pid| pid != 0);
    Some(Battery { level, charging, mouse })
}

/// The older Rapoo report BB, with the report ID first.
fn parse_old(report: &[u8]) -> Option<Battery> {
    if report.len() < 7 || report[0] != OLD_STATUS_REPORT || report[1] != 0xB0 {
        return None;
    }
    let charging = match report[5] {
        1 => false,
        2 => true,
        _ => return None,
    };
    let level = match report[6] as i32 {
        0 => None,
        percent @ 1..=100 => Some(percent),
        0x7F if charging => None,
        _ => return None,
    };
    Some(Battery { level, charging, mouse: None })
}

/// Incott's report 09, with the report ID first. A level of 0 from a mouse that isn't charging is what the receiver
/// has when the mouse is off.
fn parse_incott(report: &[u8]) -> Option<Battery> {
    if report.len() < 3 || report[0] != INCOTT_REPORT {
        return None;
    }
    let charging = report[1] & 0x80 != 0;
    let percent = (report[1] & 0x7F) as i32;
    if percent > 100 || (percent == 0 && !charging) {
        return None;
    }
    Some(Battery { level: (percent != 0).then_some(percent), charging, mouse: None })
}

/// `key`: the reading's ID is "dev:" and this, so it must stay as it is. `name`: as sold, or empty when only the
/// product string can say. `cable`: 0 when the ID of the mouse on its cable isn't known.
struct Model {
    key: &'static str,
    name: &'static str,
    vid: u16,
    receiver: u16,
    cable: u16,
    frame: Frame,
}

const fn nordic(key: &'static str, name: &'static str, receiver: u16, cable: u16) -> Model {
    Model { key, name, vid: RAPOO, receiver, cable, frame: Frame::Nordic }
}

const fn old(key: &'static str, name: &'static str, receiver: u16, cable: u16) -> Model {
    Model { key, name, vid: RAPOO, receiver, cable, frame: Frame::Old }
}

static MODELS: &[Model] = &[
    // The older firmware. VT9 Pro: OpenMouse's captures; VT3 Pro: Device-Battery-Info; VT1 Pro Max: its owner's
    // USB captures in Wireless-Device-Battery-Monitor's issue 136; VT3 Pro Max: zbndev/rapoo_battery
    old("rapoo-vt9-pro", "Rapoo VT9 Pro", 0x1205, 0x4405),
    old("rapoo-vt3-pro", "Rapoo VT3 Pro", 0x1215, 0x4415),
    old("rapoo-vt1-pro-max", "Rapoo VT1 Pro Max", 0x1233, 0x4433),
    old("rapoo-vt3-pro-max", "Rapoo VT3 Pro Max", 0x1244, 0),
    // Every mouse Rapoo Hub drives with its VT_nrf54L driver, named as its manifest names it. Owners' captures
    // exist for the VT3 MAX (1417 / 4617) and the VT9 (1414 / 4614); the rest is the manifests' word
    nordic("rapoo-vt0", "Rapoo VT0", 0x1410, 0x4610),
    nordic("rapoo-vt0-air-max", "Rapoo VT0 Air MAX", 0x14A1, 0x46A1),
    nordic("rapoo-vt0-max", "Rapoo VT0 MAX", 0x1420, 0x4620),
    nordic("rapoo-vt0-max-master", "Rapoo VT0 MAX MASTER", 0x144A, 0x464A),
    nordic("rapoo-vt1", "Rapoo VT1", 0x1415, 0x4615),
    nordic("rapoo-vt1-max", "Rapoo VT1 MAX", 0x1444, 0x4644),
    nordic("rapoo-vt1s-max-v2", "Rapoo VT1S MAX V2", 0x1B0A, 0x4B0A),
    nordic("rapoo-vt1s-v2", "Rapoo VT1S V2", 0x146A, 0x466A),
    nordic("rapoo-vt2", "Rapoo VT2", 0x1422, 0x4622),
    nordic("rapoo-vt2-air-max-v2", "Rapoo VT2 Air MAX V2", 0x1B12, 0x4B12),
    nordic("rapoo-vt2-max", "Rapoo VT2 MAX", 0x1423, 0x4623),
    nordic("rapoo-vt2-max-master-v2", "Rapoo VT2 MAX MASTER V2", 0x1475, 0x4675),
    nordic("rapoo-vt2-max-v2", "Rapoo VT2 MAX V2", 0x1B09, 0x4B09),
    nordic("rapoo-vt2-v2", "Rapoo VT2 V2", 0x1468, 0x4668),
    nordic("rapoo-vt2k-max", "Rapoo VT2K MAX", 0x14A4, 0x46A4),
    nordic("rapoo-vt2s-air-max-v2", "Rapoo VT2S Air MAX V2", 0x1B13, 0x4B13),
    nordic("rapoo-vt2s-max-master-v2", "Rapoo VT2S MAX MASTER V2", 0x1476, 0x4676),
    nordic("rapoo-vt2s-max-v2", "Rapoo VT2S MAX V2", 0x1B04, 0x4B04),
    nordic("rapoo-vt2s-v2", "Rapoo VT2S V2", 0x145D, 0x465D),
    nordic("rapoo-vt3", "Rapoo VT3", 0x1412, 0x4612),
    nordic("rapoo-vt3-air-max-v2", "Rapoo VT3 Air MAX V2", 0x1B0C, 0x4B0C),
    nordic("rapoo-vt3-max", "Rapoo VT3 MAX", 0x1417, 0x4617),
    nordic("rapoo-vt3-max-master-v2", "Rapoo VT3 MAX MASTER V2", 0x146D, 0x466D),
    nordic("rapoo-vt3-max-v2", "Rapoo VT3 MAX V2", 0x1B06, 0x4B06),
    nordic("rapoo-vt3-pro-v2", "Rapoo VT3 PRO V2", 0x14A5, 0x46A5),
    nordic("rapoo-vt3-v2", "Rapoo VT3 V2", 0x1462, 0x4662),
    nordic("rapoo-vt3s", "Rapoo VT3S", 0x1411, 0x4611),
    nordic("rapoo-vt3s-air-max", "Rapoo VT3S Air Max", 0x14A2, 0x46A2),
    nordic("rapoo-vt3s-air-max-v2", "Rapoo VT3S Air MAX V2", 0x1B0D, 0x4B0D),
    nordic("rapoo-vt3s-max", "Rapoo VT3S MAX", 0x1421, 0x4621),
    nordic("rapoo-vt3s-max-master", "Rapoo VT3S MAX MASTER", 0x144B, 0x464B),
    nordic("rapoo-vt3s-max-master-v2", "Rapoo VT3S MAX MASTER V2", 0x146E, 0x466E),
    nordic("rapoo-vt3s-max-v2", "Rapoo VT3S MAX V2", 0x1B07, 0x4B07),
    nordic("rapoo-vt3s-pro-v2", "Rapoo VT3S PRO V2", 0x14A6, 0x46A6),
    nordic("rapoo-vt3s-v2", "Rapoo VT3S V2", 0x1464, 0x4664),
    nordic("rapoo-vt4-air-max-v2", "Rapoo VT4 Air MAX V2", 0x1B14, 0x4B14),
    nordic("rapoo-vt4-max-master-v2", "Rapoo VT4 MAX MASTER V2", 0x1477, 0x4677),
    nordic("rapoo-vt4-max-v2", "Rapoo VT4 MAX V2", 0x1B03, 0x4B03),
    nordic("rapoo-vt4-v2", "Rapoo VT4 V2", 0x145B, 0x465B),
    nordic("rapoo-vt6", "Rapoo VT6", 0x1451, 0x4651),
    nordic("rapoo-vt6-max", "Rapoo VT6 MAX", 0x1452, 0x4652),
    nordic("rapoo-vt7", "Rapoo VT7", 0x1413, 0x4613),
    nordic("rapoo-vt7-air-max-v2", "Rapoo VT7 Air MAX V2", 0x1B0E, 0x4B0E),
    nordic("rapoo-vt7-max", "Rapoo VT7 MAX", 0x1416, 0x4616),
    nordic("rapoo-vt7-max-master-v2", "Rapoo VT7 MAX MASTER V2", 0x1470, 0x4670),
    nordic("rapoo-vt7-max-v2", "Rapoo VT7 MAX V2", 0x1B05, 0x4B05),
    nordic("rapoo-vt7-pro-v2", "Rapoo VT7 PRO V2", 0x14A7, 0x46A7),
    nordic("rapoo-vt7-v2", "Rapoo VT7 V2", 0x1460, 0x4660),
    nordic("rapoo-vt7s-air-max-v2", "Rapoo VT7S Air MAX V2", 0x1B0F, 0x4B0F),
    nordic("rapoo-vt7s-max-master-v2", "Rapoo VT7S MAX MASTER V2", 0x1471, 0x4671),
    nordic("rapoo-vt7s-max-v2", "Rapoo VT7S MAX V2", 0x1B02, 0x4B02),
    nordic("rapoo-vt7s-pro-v2", "Rapoo VT7S PRO V2", 0x14A8, 0x46A8),
    nordic("rapoo-vt7s-v2", "Rapoo VT7S V2", 0x1459, 0x4659),
    nordic("rapoo-vt8-air-max-v2", "Rapoo VT8 Air MAX V2", 0x1B15, 0x4B15),
    nordic("rapoo-vt8-max-master-v2", "Rapoo VT8 MAX MASTER V2", 0x1478, 0x4678),
    nordic("rapoo-vt8-max-v2", "Rapoo VT8 MAX V2", 0x1B0B, 0x4B0B),
    nordic("rapoo-vt8-v2", "Rapoo VT8 V2", 0x146C, 0x466C),
    nordic("rapoo-vt9", "Rapoo VT9", 0x1414, 0x4614),
    nordic("rapoo-vt9-air-max-v2", "Rapoo VT9 Air MAX V2", 0x1B10, 0x4B10),
    nordic("rapoo-vt9-max", "Rapoo VT9 MAX", 0x1418, 0x4618),
    nordic("rapoo-vt9-max-master-v2", "Rapoo VT9 MAX MASTER V2", 0x1473, 0x4673),
    nordic("rapoo-vt9-max-v2", "Rapoo VT9 MAX V2", 0x1B08, 0x4B08),
    nordic("rapoo-vt9-pro-v2", "Rapoo VT9 PRO V2", 0x14A9, 0x46A9),
    nordic("rapoo-vt9-v2", "Rapoo VT9 V2", 0x1466, 0x4666),
    nordic("rapoo-vt9s-air-max-v2", "Rapoo VT9S Air MAX V2", 0x1B11, 0x4B11),
    nordic("rapoo-vt9s-max-master-v2", "Rapoo VT9S MAX MASTER V2", 0x1474, 0x4674),
    nordic("rapoo-vt9s-max-v2", "Rapoo VT9S MAX V2", 0x1B01, 0x4B01),
    nordic("rapoo-vt9s-pro-v2", "Rapoo VT9S PRO V2", 0x14AA, 0x46AA),
    nordic("rapoo-vt9s-v2", "Rapoo VT9S V2", 0x1453, 0x4653),
    // 093A is PixArt's vendor ID and these two products are whatever mouse is built on its 8K reference design
    Model { key: "incott-8k-mouse", name: "", vid: PIXART, receiver: 0x522C, cable: 0x622C, frame: Frame::Incott },
];

fn find(vid: u16, pid: u16) -> Option<(&'static Model, Link)> {
    MODELS.iter().filter(|m| m.vid == vid && pid != 0).find_map(|m| {
        if m.receiver == pid {
            Some((m, Link::Receiver))
        } else if m.cable == pid {
            Some((m, Link::Cable))
        } else {
            None
        }
    })
}

/// The name a product string gives: "incott 8K wireless mouse" is an Incott mouse, "incott Esports G23V2Pro mouse"
/// (the same mouse on its cable) the Incott Esports G23V2Pro. Incott spells itself in lower case there.
fn product_name(product: Option<&str>) -> String {
    let tidy = product.map(|p| match p.get(..6) {
        Some(head) if head.eq_ignore_ascii_case("incott") => format!("Incott{}", &p[6..]),
        _ => p.to_string(),
    });
    receiver_name::mouse(tidy.as_deref(), None)
}

/// What the status collections of one device said so far. `battery`: the last report, while it still counts.
/// `heard`: a report was read at least once. `mouse`: the mouse's own product ID, from the last report that had it.
/// `error`: why none of the collections can be read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Status {
    battery: Option<Battery>,
    heard: bool,
    mouse: Option<u16>,
    error: Option<String>,
}

/// One status for a device with several status collections (an Incott mouse on its cable has two, one of them
/// dead). None when it has no collection to listen on.
fn combine(statuses: &[Status]) -> Option<Status> {
    let first = statuses.first()?;
    Some(Status {
        battery: statuses.iter().find_map(|s| s.battery),
        heard: statuses.iter().any(|s| s.heard),
        mouse: statuses.iter().find_map(|s| s.mouse),
        error: if statuses.iter().all(|s| s.error.is_some()) { first.error.clone() } else { None },
    })
}

/// The reading for one USB device. `status`: None when it has no status collection that could be opened, and
/// `missing` is then the text key that says why.
fn reading(
    model: &'static Model,
    link: Link,
    product: Option<&str>,
    status: Option<&Status>,
    missing: &str,
) -> Option<DeviceReading> {
    // With shared IDs this is somebody else's mouse until it has sent a report of ours
    if model.frame.shared() && !status.is_some_and(|s| s.heard) {
        return None;
    }
    // A receiver paired with a mouse of another model says so in its reports
    let reported = status.and_then(|s| s.mouse).and_then(|pid| find(model.vid, pid)).map(|found| found.0);
    let model = reported.filter(|m| m.frame == model.frame).unwrap_or(model);
    let name = if model.name.is_empty() { product_name(product) } else { model.name.to_string() };
    let via = t(if link == Link::Cable { "via.cable" } else { "via.24" });
    let mut r = DeviceReading::new(format!("dev:{}", model.key), name, DeviceKind::Mouse, via);
    let Some(status) = status else {
        r.detail = Some(t(missing));
        return Some(r);
    };
    if let Some(error) = &status.error {
        r.detail = Some(tf("d.error", &[error]));
        return Some(r);
    }
    match status.battery {
        Some(b) => {
            r.online = true;
            r.charging = b.charging;
            r.level = b.level;
        }
        // On its cable the mouse is there and on power, whatever it last said
        None if link == Link::Cable => {
            r.online = true;
            r.charging = true;
        }
        // Silence is the mouse asleep, switched off or out of range - never an empty battery
        None => {
            r.detail = Some(t("d.asleep"));
            return Some(r);
        }
    }
    if r.level.is_none() {
        r.detail = Some(t("d.levelUnknown"));
    }
    Some(r)
}

/// One reading per mouse out of its receiver's and its cable's: the one that knows more, the cable's when they
/// know the same.
fn merge(mut readings: Vec<(Link, DeviceReading)>) -> Vec<DeviceReading> {
    fn rank(r: &DeviceReading) -> u8 {
        match (r.online, r.level) {
            (true, Some(_)) => 2,
            (true, None) => 1,
            _ => 0,
        }
    }
    readings.sort_by_key(|(link, _)| *link != Link::Cable);
    let mut result: Vec<DeviceReading> = Vec::new();
    for (_, r) in readings {
        match result.iter_mut().find(|o| o.id == r.id) {
            Some(other) => {
                if rank(&r) > rank(other) {
                    *other = r;
                }
            }
            None => result.push(r),
        }
    }
    result
}

fn describe(b: &Battery) -> String {
    let level = b.level.map(|l| format!("{l}%")).unwrap_or_else(|| "no level".into());
    format!("{level}, {}", if b.charging { "charging" } else { "on battery" })
}

/// What the reports of one collection said so far. No I/O and no clock of its own, so it can be fed by hand. The
/// first report it reads goes to the log, and the first few it can't read, so that a user's log shows what a mouse
/// that isn't read sends.
struct Tracker {
    parse: fn(&[u8]) -> Option<Battery>,
    keep: Duration,
    last_report: Option<Instant>,
    unread: HashSet<String>,
    battery: Option<Battery>,
    heard: bool,
    mouse: Option<u16>,
}

impl Tracker {
    const LOGGED_REPORTS_MAX: usize = 8;

    fn new(parse: fn(&[u8]) -> Option<Battery>, keep: Duration) -> Self {
        Self { parse, keep, last_report: None, unread: HashSet::new(), battery: None, heard: false, mouse: None }
    }

    /// Takes one input report; true when what a poll would show has changed. Lines worth logging go to `log`.
    fn feed(&mut self, report: &[u8], now: Instant, log: &mut Vec<String>) -> bool {
        let Some(battery) = (self.parse)(report) else {
            let text = hex(report);
            if self.unread.len() < Self::LOGGED_REPORTS_MAX && self.unread.insert(text.clone()) {
                log.push(format!("report {text} (not a reading)"));
            }
            return false;
        };
        if !self.heard {
            log.push(format!("first status report {}: {}", hex(report), describe(&battery)));
        }
        self.heard = true;
        self.last_report = Some(now);
        self.mouse = battery.mouse.or(self.mouse);
        let news = self.battery != Some(battery);
        self.battery = Some(battery);
        news
    }

    /// True when the last report has just stopped counting.
    fn tick(&mut self, now: Instant) -> bool {
        if self.battery.is_none() || self.last_report.is_some_and(|t| now.saturating_duration_since(t) < self.keep) {
            return false;
        }
        self.battery = None;
        true
    }
}

struct ListenerShared {
    inner: Mutex<(Tracker, Option<String>)>, // the tracker and the error that stopped the listener
    stopped: AtomicBool,
    running: AtomicBool,
}

/// Reads one status collection on its own thread into a Tracker.
struct Listener {
    label: String, // for the log: the mouse and its USB IDs
    shared: Arc<ListenerShared>,
}

impl Listener {
    /// A listener that stops after running this long (the device was unplugged) asks for a poll at once. One that
    /// fails right away waits for the next poll, or polls would follow each other without a pause.
    const PUSH_ON_STOP_AFTER: Duration = secs(10);

    fn start(info: HidInfo, label: String, tracker: Tracker, changed: Pusher) -> Self {
        let shared = Arc::new(ListenerShared {
            inner: Mutex::new((tracker, None)),
            stopped: AtomicBool::new(false),
            running: AtomicBool::new(true),
        });
        let (s, l) = (shared.clone(), label.clone());
        let thread = std::thread::Builder::new().name("Mouse status reports".into());
        if thread.spawn(move || Self::listen(info, l, s, changed)).is_err() {
            shared.running.store(false, Ordering::Relaxed);
        }
        Self { label, shared }
    }

    fn running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    fn current(&self) -> Status {
        let inner = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        Status { battery: inner.0.battery, heard: inner.0.heard, mouse: inner.0.mouse, error: inner.1.clone() }
    }

    /// The listener has something to show: a report, or the reason it stopped.
    fn settled(&self) -> bool {
        !self.running() || self.current().battery.is_some()
    }

    /// The listening thread owns the handle and closes it itself, so no read is ever left pending on a closed handle.
    fn listen(info: HidInfo, label: String, shared: Arc<ListenerShared>, changed: Pusher) {
        // However it ends, a panic included: a listener that isn't running is started again by the next poll
        let _running = ClearOnDrop(&shared.running);
        let started = Instant::now();
        let result = (|| -> win::Result<()> {
            let dev = HidDevice::open_read_only(&info)?;
            let mut lines = Vec::new();
            while !shared.stopped.load(Ordering::Relaxed) {
                let report = dev.read(1000)?;
                let news = {
                    let mut inner = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
                    let fed = report.is_some_and(|r| inner.0.feed(&r, Instant::now(), &mut lines));
                    let quiet = inner.0.tick(Instant::now());
                    fed || quiet
                };
                for line in lines.drain(..) {
                    log::write(&format!("{label}: {line}"));
                }
                if news {
                    changed();
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let mut inner = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            // Windows' own text ("Access is denied.") rather than "Cannot open <a long device path>"
            inner.1 = Some(e.code.map(win::system_message).unwrap_or(e.message));
            inner.0.battery = None;
        }
        shared.running.store(false, Ordering::Relaxed);
        if !shared.stopped.load(Ordering::Relaxed) && started.elapsed() > Self::PUSH_ON_STOP_AFTER {
            changed();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Relaxed);
    }
}

/// A status collection to listen on.
struct Wanted {
    info: HidInfo,
    model: &'static Model,
    link: Link,
}

pub struct PassiveMiceProvider {
    push: Pusher,
    listeners: HashMap<String, Listener>, // by collection path
    /// What this session's log already has: the USB IDs that were described and the errors of the listeners.
    logged: HashSet<String>,
}

impl Default for PassiveMiceProvider {
    fn default() -> Self {
        Self { push: no_push(), listeners: HashMap::new(), logged: HashSet::new() }
    }
}

impl PassiveMiceProvider {
    /// Starts a listener for every new status collection and drops the ones that are gone.
    fn track(&mut self, wanted: &[Wanted]) {
        let paths: Vec<String> = self.listeners.keys().cloned().collect();
        for path in paths {
            let present = wanted.iter().any(|w| w.info.path == path);
            let listener = &self.listeners[&path];
            if listener.running() && present {
                continue;
            }
            // A listener stops by itself when its device is unplugged; a collection still there that can't be read
            // is worth one line in the log (it gets a new listener right below and is tried again)
            if present {
                if let Some(error) = listener.current().error {
                    if self.logged.insert(format!("{path} {error}")) {
                        log::write(&format!("{}: cannot read the status collection: {error}", listener.label));
                    }
                }
            }
            self.listeners.remove(&path); // dropping it stops it
        }

        let mut awaited = Vec::new();
        for w in wanted {
            if self.listeners.contains_key(&w.info.path) {
                continue;
            }
            let label = format!("{} {:04x}:{:04x}", log_name(w.model), w.info.vendor_id, w.info.product_id);
            let tracker = Tracker::new(w.model.frame.parser(), w.model.frame.keep(w.link));
            self.listeners
                .insert(w.info.path.clone(), Listener::start(w.info.clone(), label, tracker, self.push.clone()));
            if w.model.frame.heartbeat() && w.link == Link::Receiver {
                awaited.push(w.info.path.clone());
            }
        }
        let until = Instant::now() + FIRST_REPORT_WAIT;
        while awaited.iter().any(|p| !self.listeners[p].settled()) && Instant::now() < until {
            sleep_ms(100);
        }
    }

    /// Once per USB ID and session: its HID collections and which of them are listened to, enough to see from a
    /// user's log whether a mouse that isn't read has the status collection at all.
    fn log_once(&mut self, model: &Model, link: Link, collections: &[HidInfo]) {
        let id = format!("{:04x}:{:04x}", collections[0].vendor_id, collections[0].product_id);
        if !self.logged.insert(id.clone()) {
            return;
        }
        log::write(&format!(
            "{} {id}: {}, {} HID collections",
            log_name(model),
            if link == Link::Cable { "the mouse on its cable" } else { "a receiver" },
            collections.len()
        ));
        for info in collections {
            let role = if !info.access_denied && model.frame.fits(info) { "listening" } else { "not used" };
            log::write(&format!("  mi={} {info}: {role}", info.interface_number()));
        }
    }
}

fn log_name(model: &Model) -> &'static str {
    if model.name.is_empty() {
        "Incott"
    } else {
        model.name
    }
}

impl Provider for PassiveMiceProvider {
    fn name(&self) -> &'static str {
        "Passive mice"
    }

    fn set_pusher(&mut self, push: Pusher) {
        self.push = push;
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let found = hid::enumerate(|v, p| find(v, p).is_some());
        let devices: Vec<(&'static Model, Link, Vec<HidInfo>)> = group_by(found, |i| (i.vendor_id, i.product_id))
            .into_iter()
            .filter_map(|((vid, pid), collections)| find(vid, pid).map(|(model, link)| (model, link, collections)))
            .collect();

        let mut wanted = Vec::new();
        for &(model, link, ref collections) in &devices {
            self.log_once(model, link, collections);
            for info in collections.iter().filter(|i| !i.access_denied && model.frame.fits(i)) {
                wanted.push(Wanted { info: info.clone(), model, link });
            }
        }
        self.track(&wanted);

        let mut readings = Vec::new();
        for &(model, link, ref collections) in &devices {
            let statuses: Vec<Status> =
                collections.iter().filter_map(|i| self.listeners.get(&i.path)).map(Listener::current).collect();
            let product = collections.iter().find_map(|i| i.product.as_deref());
            let status = combine(&statuses);
            if let Some(r) = reading(model, link, product, status.as_ref(), no_link_reason(collections)) {
                readings.push((link, r));
            }
        }
        merge(readings)
    }

    fn delay(&self, last: &[DeviceReading]) -> Duration {
        // The listeners push every change; polling is for receivers and cables that come and go
        if last.iter().any(|r| r.online) {
            secs(30)
        } else {
            secs(15)
        }
    }
}

#[cfg(test)]
mod tests {
    //! The Rapoo reports are the ones their owners captured for Wireless-Device-Battery-Monitor (docs/rapoo.md:
    //! a VT3 MAX at 25%, a VT9 at 100%, a VT1 Pro Max at 100% and at 95% on the charger) and the VT9 Pro's from
    //! OpenMouse (98%); the Incott ones are OpenMouse's capture of a G23 V2 Pro across a charge. All of them have
    //! the report ID in front, as Windows hands them over.
    use super::*;

    const VT3_MAX_RECEIVER: [u8; 19] = [
        0x07, 0x20, 0x03, 0x20, 0x03, 0x20, 0x03, 0x01, 0x19, 0x00, 0x04, 0x84, 0x00, 0x17, 0x46, 0x00, 0x02, 0x03,
        0xBC,
    ];
    const VT3_MAX_CABLE: [u8; 19] = [
        0x07, 0x22, 0x03, 0x20, 0x03, 0x20, 0x03, 0x02, 0x19, 0x00, 0x04, 0x84, 0x00, 0x17, 0x46, 0x00, 0x02, 0x03,
        0x00,
    ];
    const VT9_RECEIVER: [u8; 16] =
        [0x07, 0x10, 0x03, 0x40, 0x06, 0x40, 0x06, 0x01, 0x64, 0x00, 0x03, 0x01, 0x00, 0x14, 0x46, 0x01];
    const VT9_CABLE: [u8; 19] = [
        0x07, 0x12, 0x04, 0x80, 0x0C, 0x80, 0x0C, 0x02, 0x64, 0x00, 0x03, 0x01, 0x00, 0x14, 0x46, 0x01, 0x02, 0x02,
        0x00,
    ];
    const INCOTT_95: [u8; 8] = [0x09, 0x5F, 0x10, 0x04, 0x00, 0x0F, 0x0F, 0x10];
    const INCOTT_97_CHARGING: [u8; 8] = [0x09, 0xE1, 0x10, 0x04, 0x00, 0x0F, 0x0F, 0x10];

    fn battery(level: Option<i32>, charging: bool, mouse: Option<u16>) -> Option<Battery> {
        Some(Battery { level, charging, mouse })
    }

    fn model(key: &str) -> &'static Model {
        MODELS.iter().find(|m| m.key == key).expect("no such model")
    }

    fn collection(page: u16, usage: u16, input: usize) -> HidInfo {
        HidInfo { usage_page: page, usage, input_length: input, ..HidInfo::default() }
    }

    fn heard(battery: Option<Battery>) -> Status {
        Status { battery, heard: true, mouse: battery.and_then(|b| b.mouse), error: None }
    }

    #[test]
    fn a_nordic_receiver_reports_level_state_and_the_mouse() {
        assert_eq!(parse_nordic(&VT3_MAX_RECEIVER), battery(Some(25), false, Some(0x4617)));
        assert_eq!(parse_nordic(&VT9_RECEIVER), battery(Some(100), false, Some(0x4614)));
    }

    #[test]
    fn a_nordic_mouse_on_its_cable_is_charging() {
        assert_eq!(parse_nordic(&VT3_MAX_CABLE), battery(Some(25), true, Some(0x4617)));
        assert_eq!(parse_nordic(&VT9_CABLE), battery(Some(100), true, Some(0x4614)));
    }

    #[test]
    fn a_short_nordic_report_has_the_level_but_not_the_mouse() {
        // The 13-byte collection some VT3 MAX receivers have
        assert_eq!(parse_nordic(&VT3_MAX_RECEIVER[..13]), battery(Some(25), false, None));
        assert_eq!(parse_nordic(&VT3_MAX_RECEIVER[..9]), battery(Some(25), false, None));
        assert_eq!(parse_nordic(&VT3_MAX_RECEIVER[..8]), None);
    }

    #[test]
    fn a_nordic_report_without_a_reading_is_none() {
        let with = |at: usize, value: u8| {
            let mut report = VT3_MAX_RECEIVER;
            report[at] = value;
            parse_nordic(&report)
        };
        assert_eq!(with(7, 0), None, "no reading yet");
        assert_eq!(with(7, 4), None, "unknown state");
        assert_eq!(with(8, 101), None, "not a percentage");
        assert_eq!(with(8, 0x7F), None, "the older firmware's mark means nothing here");
        assert_eq!(with(1, 0x23), None, "unknown link");
        assert_eq!(with(0, 0x08), None, "the CH584 mice's report");
        assert_eq!(with(7, 3), battery(Some(25), true, Some(0x4617)), "Rapoo Hub shows 3 as charging too");
        assert_eq!(with(8, 0), battery(None, false, Some(0x4617)), "0 is not measured, not empty");
    }

    #[test]
    fn a_receiver_whose_mouse_is_not_linked_has_no_reading() {
        let mut report = VT3_MAX_RECEIVER;
        report[18] = 0x3C; // the link bit cleared, the signal strength left
        assert_eq!(parse_nordic(&report), None);
        // On the cable that byte is 0 and means nothing
        assert!(parse_nordic(&VT3_MAX_CABLE).is_some());
    }

    #[test]
    fn a_nordic_mouse_switched_off_on_its_cable_charges_without_a_level() {
        let mut report = VT9_CABLE;
        report[8] = 1;
        assert_eq!(parse_nordic(&report), battery(None, true, Some(0x4614)));
        // The same 1% from a mouse on battery is a level
        let mut report = VT9_RECEIVER;
        report[8] = 1;
        assert_eq!(parse_nordic(&report), battery(Some(1), false, Some(0x4614)));
    }

    #[test]
    fn the_older_report_has_state_and_level_at_the_end() {
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x71, 0x84, 0x03, 0x01, 0x64]), battery(Some(100), false, None));
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x71, 0x84, 0x03, 0x02, 0x5F]), battery(Some(95), true, None));
        // The VT9 Pro on its receiver at 98%
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x51, 0x20, 0x03, 0x01, 0x62]), battery(Some(98), false, None));
    }

    #[test]
    fn the_older_report_can_charge_without_a_level() {
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x81, 0x20, 0x03, 0x02, 0x7F]), battery(None, true, None));
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x81, 0x20, 0x03, 0x01, 0x7F]), None, "only while charging");
    }

    #[test]
    fn other_older_reports_are_not_readings() {
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x81, 0x20, 0x03, 0x00, 0x32]), None, "between two states");
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x81, 0x20, 0x03, 0x03, 0x32]), None, "unknown state");
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x81, 0x20, 0x03, 0x01, 0x65]), None, "not a percentage");
        assert_eq!(parse_old(&[0xBC, 0xB0, 0x81, 0x20, 0x03, 0x01, 0x32, 0, 0, 0]), None, "the cable report");
        assert_eq!(parse_old(&[0xBB, 0x00, 0x81, 0x20, 0x03, 0x01, 0x32]), None);
        assert_eq!(parse_old(&[0xBB, 0xB0, 0x81, 0x20, 0x03, 0x01]), None, "too short");
        assert_eq!(parse_old(&VT3_MAX_RECEIVER), None);
        assert_eq!(parse_nordic(&[0xBB, 0xB0, 0x71, 0x84, 0x03, 0x01, 0x64, 0x00, 0x00]), None);
    }

    #[test]
    fn an_incott_report_has_the_level_with_charging_in_the_top_bit() {
        assert_eq!(parse_incott(&INCOTT_95), battery(Some(95), false, None));
        assert_eq!(parse_incott(&INCOTT_97_CHARGING), battery(Some(97), true, None));
        assert_eq!(parse_incott(&[0x09, 0x64, 0x54]), battery(Some(100), false, None));
        assert_eq!(parse_incott(&[0x09, 0xE4, 0x54]), battery(Some(100), true, None));
    }

    #[test]
    fn an_incott_report_without_a_level_is_no_reading() {
        assert_eq!(parse_incott(&[0x09, 0x00, 0x10]), None, "the mouse is off");
        assert_eq!(parse_incott(&[0x09, 0x65, 0x10]), None, "not a percentage");
        assert_eq!(parse_incott(&[0x09, 0xE5, 0x10]), None, "not a percentage while charging");
        assert_eq!(parse_incott(&[0x08, 0x5F, 0x10]), None, "another report");
        assert_eq!(parse_incott(&[0x09, 0x5F]), None, "too short");
        assert_eq!(parse_incott(&[0x09, 0x80, 0x10]), battery(None, true, None));
    }

    #[test]
    fn each_kind_listens_on_its_own_collection() {
        for length in [13, 16, 19] {
            assert!(Frame::Nordic.fits(&collection(0xFF00, 0x0002, length)));
        }
        assert!(!Frame::Nordic.fits(&collection(0xFF00, 0x000E, 33)), "the configuration channel");
        assert!(!Frame::Nordic.fits(&collection(0xFF00, 0x0002, 513)), "the firmware collection");
        assert!(Frame::Old.fits(&collection(0xFF00, 0x0002, 7)));
        assert!(!Frame::Old.fits(&collection(0xFF00, 0x0002, 10)), "its 10-byte sibling");
        assert!(!Frame::Old.fits(&collection(0xFF0B, 0x0104, 61)));
        assert!(Frame::Incott.fits(&collection(0xFF05, 0x0001, 33)));
        assert!(!Frame::Incott.fits(&collection(0xFF00, 0x0001, 0)), "the receiver's other vendor collections");
        assert!(!Frame::Incott.fits(&collection(0x0001, 0x0002, 8)));
    }

    #[test]
    fn models_have_their_own_ids_and_keys() {
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for m in MODELS {
            assert!(keys.insert(m.key), "{}: the key is used twice", m.key);
            assert!(m.key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'), "{}", m.key);
            for pid in [m.receiver, m.cable].into_iter().filter(|&pid| pid != 0) {
                assert!(ids.insert((m.vid, pid)), "{:04X}:{pid:04X} is listed twice ({})", m.vid, m.key);
                assert!(!super::super::handled(m.vid, pid), "{:04X}:{pid:04X} is a table row too", m.vid);
            }
        }
        assert!(SPECS.is_empty());
    }

    #[test]
    fn a_product_id_says_the_model_and_the_link() {
        let (m, link) = find(RAPOO, 0x1417).unwrap();
        assert_eq!((m.key, link), ("rapoo-vt3-max", Link::Receiver));
        let (m, link) = find(RAPOO, 0x4617).unwrap();
        assert_eq!((m.key, link), ("rapoo-vt3-max", Link::Cable));
        let (m, link) = find(PIXART, 0x622C).unwrap();
        assert_eq!((m.key, link), ("incott-8k-mouse", Link::Cable));
        assert!(find(RAPOO, 0).is_none(), "a model without a known cable ID");
        assert!(find(PIXART, 0x1417).is_none());
        assert!(find(RAPOO, 0x1A11).is_none(), "the VT1 SE sends another report");
    }

    #[test]
    fn a_report_counts_until_the_mouse_has_been_quiet_for_too_long() {
        let mut tracker = Tracker::new(parse_nordic, HEARTBEAT_QUIET);
        let mut log = Vec::new();
        let start = Instant::now();
        assert!(tracker.feed(&VT3_MAX_RECEIVER, start, &mut log));
        assert!(!tracker.feed(&VT3_MAX_RECEIVER, start + secs(3), &mut log), "the same again is no news");
        assert!(!tracker.tick(start + secs(32)));
        assert!(tracker.tick(start + secs(33)));
        assert_eq!(tracker.battery, None);
        assert!(tracker.heard);
        assert_eq!(tracker.mouse, Some(0x4617), "who it was is remembered");
        assert!(!tracker.tick(start + secs(40)), "it went quiet once");
        assert!(tracker.feed(&VT3_MAX_RECEIVER, start + secs(60), &mut log), "and is back");
        assert_eq!(log.len(), 1, "only the first report is logged: {log:?}");
        assert!(log[0].ends_with("25%, on battery"), "{}", log[0]);
    }

    #[test]
    fn a_changed_level_is_news() {
        let mut tracker = Tracker::new(parse_incott, IN_USE_QUIET);
        let now = Instant::now();
        assert!(tracker.feed(&INCOTT_95, now, &mut Vec::new()));
        assert!(tracker.feed(&INCOTT_97_CHARGING, now, &mut Vec::new()));
        assert_eq!(tracker.battery, battery(Some(97), true, None));
    }

    #[test]
    fn reports_that_are_not_readings_change_nothing_and_are_logged_once() {
        let mut tracker = Tracker::new(parse_old, HEARTBEAT_QUIET);
        let mut log = Vec::new();
        let now = Instant::now();
        let transition = [0xBB, 0xB0, 0x81, 0x20, 0x03, 0x00, 0x32];
        assert!(!tracker.feed(&transition, now, &mut log));
        assert!(!tracker.feed(&transition, now, &mut log));
        assert!(!tracker.heard && tracker.battery.is_none());
        assert_eq!(log, vec!["report bb b0 81 20 03 00 32 (not a reading)".to_string()]);
        // Not more than a few of them, however many differ
        for i in 0..100u8 {
            tracker.feed(&[0xBC, i], now, &mut log);
        }
        assert_eq!(log.len(), Tracker::LOGGED_REPORTS_MAX);
    }

    #[test]
    fn a_quiet_receiver_is_a_mouse_asleep() {
        let r = reading(model("rapoo-vt3-max"), Link::Receiver, None, Some(&heard(None)), "d.noLink").unwrap();
        assert_eq!((r.id.as_str(), r.name.as_str()), ("dev:rapoo-vt3-max", "Rapoo VT3 MAX"));
        assert!(!r.online && r.level.is_none() && !r.charging);
        assert_eq!(r.detail, Some(t("d.asleep")));
        assert_eq!(r.via, t("via.24"));
        // Before any report too: these IDs are nobody else's
        let r = reading(model("rapoo-vt3-max"), Link::Receiver, None, Some(&Status::default()), "d.noLink").unwrap();
        assert_eq!(r.detail, Some(t("d.asleep")));
    }

    #[test]
    fn a_report_is_the_reading() {
        let status = heard(parse_nordic(&VT3_MAX_RECEIVER));
        let r = reading(model("rapoo-vt3-max"), Link::Receiver, None, Some(&status), "d.noLink").unwrap();
        assert!(r.online && !r.charging);
        assert_eq!((r.level, r.detail), (Some(25), None));
        let status = heard(battery(None, true, None));
        let r = reading(model("rapoo-vt3-pro"), Link::Receiver, None, Some(&status), "d.noLink").unwrap();
        assert!(r.online && r.charging);
        assert_eq!((r.level, r.detail), (None, Some(t("d.levelUnknown"))));
    }

    #[test]
    fn a_quiet_mouse_on_its_cable_is_charging_without_a_level() {
        let r = reading(model("rapoo-vt9"), Link::Cable, None, Some(&heard(None)), "d.noLink").unwrap();
        assert!(r.online && r.charging);
        assert_eq!((r.level, r.detail), (None, Some(t("d.levelUnknown"))));
        assert_eq!(r.via, t("via.cable"));
    }

    #[test]
    fn a_receiver_paired_with_another_model_is_that_model() {
        let status = heard(parse_nordic(&VT9_RECEIVER));
        let r = reading(model("rapoo-vt3-max"), Link::Receiver, None, Some(&status), "d.noLink").unwrap();
        assert_eq!((r.id.as_str(), r.name.as_str()), ("dev:rapoo-vt9", "Rapoo VT9"));
        // An ID that is no model of ours changes nothing
        let status = Status { mouse: Some(0x1234), ..status };
        let r = reading(model("rapoo-vt3-max"), Link::Receiver, None, Some(&status), "d.noLink").unwrap();
        assert_eq!(r.id, "dev:rapoo-vt3-max");
    }

    #[test]
    fn a_device_without_its_status_collection_says_why() {
        let r = reading(model("rapoo-vt9"), Link::Receiver, None, None, "d.needsAdmin").unwrap();
        assert!(!r.online);
        assert_eq!(r.detail, Some(t("d.needsAdmin")));
        let status = Status { error: Some("Access is denied.".into()), ..Status::default() };
        let r = reading(model("rapoo-vt9"), Link::Cable, None, Some(&status), "d.noLink").unwrap();
        assert!(!r.online && !r.charging);
        assert_eq!(r.detail, Some(tf("d.error", &[&"Access is denied."])));
    }

    #[test]
    fn shared_ids_are_listed_only_after_a_report_of_ours() {
        let incott = model("incott-8k-mouse");
        let product = Some("incott 8K wireless mouse");
        assert!(reading(incott, Link::Receiver, product, None, "d.noLink").is_none());
        assert!(reading(incott, Link::Receiver, product, Some(&Status::default()), "d.noLink").is_none());
        assert!(reading(incott, Link::Cable, product, Some(&Status::default()), "d.noLink").is_none());
        let r = reading(incott, Link::Receiver, product, Some(&heard(parse_incott(&INCOTT_95))), "d.noLink").unwrap();
        assert_eq!((r.id.as_str(), r.level, r.online), ("dev:incott-8k-mouse", Some(95), true));
        let r = reading(incott, Link::Receiver, product, Some(&heard(None)), "d.noLink").unwrap();
        assert_eq!((r.online, r.detail), (false, Some(t("d.asleep"))));
    }

    #[test]
    fn the_product_string_names_a_mouse_on_shared_ids() {
        assert_eq!(product_name(Some("incott 8K wireless mouse")), receiver_name::mouse(Some("Incott"), None));
        assert_eq!(product_name(Some("incott Esports G23V2Pro mouse")), "Incott Esports G23V2Pro");
        assert_eq!(product_name(Some("incott Esports G23 Pro mouse")), "Incott Esports G23 Pro");
        assert_eq!(product_name(Some("2.4G Wireless Mouse")), t("name.mouse"));
        assert_eq!(product_name(None), t("name.mouse"));
    }

    #[test]
    fn several_collections_of_one_device_give_one_status() {
        assert_eq!(combine(&[]), None);
        let dead = Status::default();
        let live = heard(parse_incott(&INCOTT_97_CHARGING));
        assert_eq!(combine(&[dead.clone(), live.clone()]), Some(live.clone()));
        let failed = Status { error: Some("Access is denied.".into()), ..Status::default() };
        assert_eq!(combine(&[failed.clone(), live.clone()]), Some(live), "one that reads is enough");
        assert_eq!(combine(&[failed.clone(), failed.clone()]), Some(failed));
    }

    #[test]
    fn receiver_and_cable_of_one_mouse_are_one_reading() {
        let vt9 = model("rapoo-vt9");
        let asleep = reading(vt9, Link::Receiver, None, Some(&heard(None)), "d.noLink").unwrap();
        let plugged = reading(vt9, Link::Cable, None, Some(&heard(None)), "d.noLink").unwrap();
        let on_cable = reading(vt9, Link::Cable, None, Some(&heard(parse_nordic(&VT9_CABLE))), "d.noLink").unwrap();
        let on_air = reading(vt9, Link::Receiver, None, Some(&heard(parse_nordic(&VT9_RECEIVER))), "d.noLink").unwrap();

        // The cable beats a receiver that has gone quiet
        let merged = merge(vec![(Link::Receiver, asleep.clone()), (Link::Cable, plugged.clone())]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].online && merged[0].charging && merged[0].via == t("via.cable"));
        // A receiver that still hears the mouse knows more than a cable that hasn't said anything yet
        let merged = merge(vec![(Link::Receiver, on_air.clone()), (Link::Cable, plugged)]);
        assert_eq!((merged[0].level, merged[0].via.clone()), (Some(100), t("via.24")));
        // Both know the level: the cable's reading
        let merged = merge(vec![(Link::Receiver, on_air), (Link::Cable, on_cable)]);
        assert!(merged[0].charging && merged[0].via == t("via.cable"));
        // Another mouse stays its own reading
        let other = reading(model("rapoo-vt3"), Link::Receiver, None, Some(&heard(None)), "d.noLink").unwrap();
        assert_eq!(merge(vec![(Link::Receiver, asleep), (Link::Receiver, other)]).len(), 2);
    }
}
