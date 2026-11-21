//! Razer wireless mice and keyboards, through their receiver or cable, with the protocol OpenRazer uses
//! (github.com/openrazer/openrazer, driver/razercommon.c, razerchromacommon.c):
//! a 90-byte feature report (report ID 0) on interface 0 (mice; 3 on the newest ones) or 2/3 (keyboards):
//!   [0] status  [1] transaction id  [2..3] remaining packets  [4] protocol 0  [5] data size  [6] class  [7] id
//!   [8..87] arguments  [88] crc = XOR of bytes 2..87  [89] reserved
//! Battery: class 07 id 80 -> arguments[1] = 0..255; charging: class 07 id 84 -> arguments[1] = 0/1.
//! Reply status 02 = ok, 01 = busy, 04 = timeout (mouse off), 05 = not supported. The transaction id and the wait
//! between set and get differ per device; the table is OpenRazer's, and for the models it has no release for yet,
//! what the pull requests of their owners use. Read-only; not yet verified on hardware here.

use super::{group_by, one_per_id, secs, sleep_ms, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::reading::{DeviceKind, DeviceReading};
use std::time::Duration;

const RAZER_VID: u16 = 0x1532;
const REPORT_LENGTH: usize = 91; // report ID + 90 bytes

/// `charging: false` = the device runs on AA batteries; OpenRazer never asks it for a charging state.
struct Model {
    key: &'static str,
    name: &'static str, // without "Razer "
    kind: DeviceKind,
    wireless: bool,
    tid: u8,
    interface: i32,
    wait_ms: u64,
    query_charging: bool,
}

const fn mouse(key: &'static str, name: &'static str, wireless: bool, tid: u8, wait_us: u64, charging: bool) -> Model {
    Model {
        key,
        name,
        kind: DeviceKind::Mouse,
        wireless,
        tid,
        interface: 0,
        wait_ms: wait_us.div_ceil(1000),
        query_charging: charging,
    }
}

const fn m(key: &'static str, name: &'static str, wireless: bool, tid: u8, wait_us: u64) -> Model {
    mouse(key, name, wireless, tid, wait_us, true)
}

/// A mouse that takes the report on interface 3, a consumer-control collection, and has none on its mouse
/// collection: the Viper V4 Pro and the mice that came after it.
const fn m3(key: &'static str, name: &'static str, wireless: bool, tid: u8, wait_us: u64) -> Model {
    Model { interface: 3, ..mouse(key, name, wireless, tid, wait_us, true) }
}

const fn k(key: &'static str, name: &'static str, wireless: bool, tid: u8, interface: i32, wait_us: u64) -> Model {
    Model {
        key,
        name,
        kind: DeviceKind::Keyboard,
        wireless,
        tid,
        interface,
        wait_ms: wait_us.div_ceil(1000),
        query_charging: true,
    }
}

static MODELS: &[(u16, Model)] = &[
    (0x001F, m("naga-epic", "Naga Epic", true, 0xFF, 600)),
    (0x0024, m("mamba-2012", "Mamba 2012", false, 0xFF, 600)),
    (0x0025, m("mamba-2012", "Mamba 2012", true, 0xFF, 600)),
    (0x0032, m("ouroboros", "Ouroboros", true, 0xFF, 600)),
    (0x003E, m("naga-epic-chroma", "Naga Epic Chroma", true, 0xFF, 600)),
    (0x003F, m("naga-epic-chroma", "Naga Epic Chroma", true, 0xFF, 600)),
    (0x0044, m("mamba", "Mamba", false, 0xFF, 600)),
    (0x0045, m("mamba", "Mamba", true, 0xFF, 600)),
    (0x0059, m("lancehead", "Lancehead", false, 0x3F, 600)),
    (0x005A, m("lancehead", "Lancehead", true, 0x3F, 600)),
    (0x0062, mouse("atheris", "Atheris", true, 0x1F, 400000, false)),
    (0x006F, m("lancehead-wireless", "Lancehead Wireless", true, 0x1F, 31000)),
    (0x0070, m("lancehead-wireless", "Lancehead Wireless", false, 0x1F, 31000)),
    (0x0072, m("mamba-wireless", "Mamba Wireless", true, 0x3F, 31000)),
    (0x0073, m("mamba-wireless", "Mamba Wireless", false, 0x3F, 31000)),
    (0x0077, m("pro-click", "Pro Click", true, 0x1F, 31000)),
    (0x0080, m("pro-click", "Pro Click", false, 0x1F, 31000)),
    (0x007A, m("viper-ultimate", "Viper Ultimate", false, 0xFF, 59900)),
    (0x007B, m("viper-ultimate", "Viper Ultimate", true, 0xFF, 59900)),
    (0x007C, m("deathadder-v2-pro", "DeathAdder V2 Pro", false, 0x3F, 59900)),
    (0x007D, m("deathadder-v2-pro", "DeathAdder V2 Pro", true, 0x3F, 59900)),
    (0x0083, mouse("basilisk-x-hyperspeed", "Basilisk X HyperSpeed", true, 0xFF, 31000, false)),
    (0x0086, m("basilisk-ultimate", "Basilisk Ultimate", false, 0x1F, 31000)),
    (0x0088, m("basilisk-ultimate", "Basilisk Ultimate", true, 0x1F, 31000)),
    (0x008F, m("naga-pro", "Naga Pro", false, 0x1F, 600)),
    (0x0090, m("naga-pro", "Naga Pro", true, 0x1F, 31000)),
    (0x0094, mouse("orochi-v2", "Orochi V2", true, 0x1F, 400000, false)),
    (0x0095, mouse("orochi-v2", "Orochi V2", true, 0x1F, 400000, false)),
    (0x009A, m("pro-click-mini", "Pro Click Mini", true, 0x1F, 31000)),
    (0x009C, mouse("deathadder-v2-x", "DeathAdder V2 X HyperSpeed", true, 0x1F, 31000, false)),
    (0x009E, m("viper-mini-se", "Viper Mini SE", false, 0x1F, 59900)),
    (0x009F, m("viper-mini-se", "Viper Mini SE", true, 0x1F, 59900)),
    (0x00A5, m("viper-v2-pro", "Viper V2 Pro", false, 0x1F, 31000)),
    (0x00A6, m("viper-v2-pro", "Viper V2 Pro", true, 0x1F, 31000)),
    (0x00A7, m("naga-v2-pro", "Naga V2 Pro", false, 0x1F, 31000)),
    (0x00A8, m("naga-v2-pro", "Naga V2 Pro", true, 0x1F, 31000)),
    (0x00AA, m("basilisk-v3-pro", "Basilisk V3 Pro", false, 0x1F, 31000)),
    (0x00AB, m("basilisk-v3-pro", "Basilisk V3 Pro", true, 0x1F, 31000)),
    (0x00AF, m("cobra-pro", "Cobra Pro", false, 0x1F, 31000)),
    (0x00B0, m("cobra-pro", "Cobra Pro", true, 0x1F, 31000)),
    (0x00B3, m("hyperpolling", "HyperPolling Mouse", true, 0x1F, 59900)),
    (0x00B4, mouse("naga-v2-hyperspeed", "Naga V2 HyperSpeed", true, 0x1F, 31000, false)),
    (0x00B6, m("deathadder-v3-pro", "DeathAdder V3 Pro", false, 0x1F, 31000)),
    (0x00B7, m("deathadder-v3-pro", "DeathAdder V3 Pro", true, 0x1F, 31000)),
    (0x00C2, m("deathadder-v3-pro", "DeathAdder V3 Pro", false, 0x1F, 31000)),
    (0x00C3, m("deathadder-v3-pro", "DeathAdder V3 Pro", true, 0x1F, 31000)),
    (0x00B8, mouse("viper-v3-hyperspeed", "Viper V3 HyperSpeed", true, 0x1F, 59900, false)),
    (0x00B9, mouse("basilisk-v3-x", "Basilisk V3 X HyperSpeed", true, 0x1F, 31000, false)),
    (0x00BE, m("deathadder-v4-pro", "DeathAdder V4 Pro", false, 0x1F, 31000)),
    (0x00BF, m("deathadder-v4-pro", "DeathAdder V4 Pro", true, 0x1F, 31000)),
    (0x00C0, m("viper-v3-pro", "Viper V3 Pro", false, 0x1F, 31000)),
    (0x00C1, m("viper-v3-pro", "Viper V3 Pro", true, 0x1F, 59900)),
    (0x00C4, m("deathadder-v3-hyperspeed", "DeathAdder V3 HyperSpeed", false, 0x1F, 31000)),
    (0x00C5, m("deathadder-v3-hyperspeed", "DeathAdder V3 HyperSpeed", true, 0x1F, 31000)),
    (0x00C7, m("pro-click-v2-vertical", "Pro Click V2 Vertical", false, 0x1F, 31000)),
    (0x00C8, m("pro-click-v2-vertical", "Pro Click V2 Vertical", true, 0x1F, 31000)),
    (0x00CC, m("basilisk-v3-pro-35k", "Basilisk V3 Pro 35K", false, 0x1F, 31000)),
    (0x00CD, m("basilisk-v3-pro-35k", "Basilisk V3 Pro 35K", true, 0x1F, 31000)),
    (0x00D0, m("pro-click-v2", "Pro Click V2", false, 0x1F, 31000)),
    (0x00D1, m("pro-click-v2", "Pro Click V2", true, 0x1F, 31000)),
    (0x00D3, mouse("basilisk-mobile", "Basilisk Mobile", false, 0x1F, 31000, false)),
    (0x00D4, mouse("basilisk-mobile", "Basilisk Mobile", true, 0x1F, 31000, false)),
    (0x00D6, m("basilisk-v3-pro-35k-pg", "Basilisk V3 Pro 35K Phantom Green", false, 0x1F, 31000)),
    (0x00D7, m("basilisk-v3-pro-35k-pg", "Basilisk V3 Pro 35K Phantom Green", true, 0x1F, 31000)),
    (0x00DA, m("cobra-hyperspeed", "Cobra HyperSpeed", false, 0x1F, 31000)),
    (0x00DB, m("cobra-hyperspeed", "Cobra HyperSpeed", true, 0x1F, 31000)),
    (0x00DE, m("viper-v3-pro-se", "Viper V3 Pro SE", false, 0x1F, 31000)),
    (0x00DF, m("viper-v3-pro-se", "Viper V3 Pro SE", true, 0x1F, 59900)),
    (0x00E0, mouse("orochi-v2", "Orochi V2", true, 0x1F, 400000, false)),
    (0x00E2, m3("basilisk-v4-pro", "Basilisk V4 Pro", false, 0x1F, 31000)),
    (0x00E3, m3("basilisk-v4-pro", "Basilisk V4 Pro", true, 0x1F, 31000)),
    (0x00E5, m3("viper-v4-pro", "Viper V4 Pro", false, 0x1F, 59900)),
    (0x00E6, m3("viper-v4-pro", "Viper V4 Pro", true, 0x1F, 59900)),
    // On its cable the Naga V3 Pro answers on the mouse interface, as the older mice do; its receiver needs a
    // longer wait right after it is plugged in
    (0x00E7, m("naga-v3-pro", "Naga V3 Pro", false, 0x1F, 31000)),
    (0x00E8, m3("naga-v3-pro", "Naga V3 Pro", true, 0x1F, 99900)),
    // With a shorter wait this receiver answers "busy" and no level
    (0x00EA, m3("basilisk-v4-hyperspeed", "Basilisk V4 HyperSpeed", true, 0x1F, 31000)),
    (0x00EB, m3("basilisk-v4-hyperspeed", "Basilisk V4 HyperSpeed", false, 0x1F, 31000)),
    (0x00EF, m("deathadder-v4-pro", "DeathAdder V4 Pro", false, 0x1F, 31000)),
    (0x00F0, m("deathadder-v4-pro", "DeathAdder V4 Pro", true, 0x1F, 31000)),
    (0x0258, k("blackwidow-v3-mini", "BlackWidow V3 Mini HyperSpeed", false, 0x1F, 3, 600)),
    (0x0271, k("blackwidow-v3-mini", "BlackWidow V3 Mini HyperSpeed", true, 0x9F, 3, 4900)),
    (0x025A, k("blackwidow-v3-pro", "BlackWidow V3 Pro", false, 0x3F, 2, 600)),
    (0x025C, k("blackwidow-v3-pro", "BlackWidow V3 Pro", true, 0x9F, 2, 4900)),
    (0x0292, k("deathstalker-v2-pro", "DeathStalker V2 Pro", false, 0x1F, 3, 600)),
    (0x0290, k("deathstalker-v2-pro", "DeathStalker V2 Pro", true, 0x9F, 2, 4900)),
    (0x0298, k("deathstalker-v2-pro-tkl", "DeathStalker V2 Pro TKL", false, 0x1F, 3, 600)),
    (0x0296, k("deathstalker-v2-pro-tkl", "DeathStalker V2 Pro TKL", true, 0x9F, 2, 4900)),
    (0x02B9, k("blackwidow-v4-mini", "BlackWidow V4 Mini HyperSpeed", false, 0x1F, 3, 600)),
    (0x02BA, k("blackwidow-v4-mini", "BlackWidow V4 Mini HyperSpeed", true, 0x9F, 3, 4900)),
    (0x02D7, k("blackwidow-v4-tkl", "BlackWidow V4 TKL HyperSpeed", false, 0x1F, 3, 600)),
    (0x02D5, k("blackwidow-v4-tkl", "BlackWidow V4 TKL HyperSpeed", true, 0x9F, 2, 4900)),
    (0x02CD, k("joro", "Joro", true, 0x1F, 3, 600)),
    (0x02CC, k("blackwidow-v4-low-profile", "BlackWidow V4 Low-Profile HyperSpeed", false, 0x1F, 3, 600)),
    (0x02C9, k("blackwidow-v4-low-profile", "BlackWidow V4 Low-Profile HyperSpeed", true, 0x9F, 2, 4900)),
    (0x02D4, k("blackwidow-v4-low-profile-tkl", "BlackWidow V4 Low-Profile TKL HyperSpeed", false, 0x1F, 3, 600)),
    (0x02D2, k("blackwidow-v4-low-profile-tkl", "BlackWidow V4 Low-Profile TKL HyperSpeed", true, 0x9F, 2, 4900)),
    (0x0277, k("pro-type-ultra", "Pro Type Ultra", false, 0x1F, 2, 600)),
    (0x027B, k("pro-type-ultra", "Pro Type Ultra", true, 0x9F, 2, 4900)),
    (0x02C4, k("pro-type-ergo", "Pro Type Ergo", false, 0x1F, 3, 600)),
    (0x02C2, k("pro-type-ergo", "Pro Type Ergo", true, 0x9F, 2, 4900)),
];

fn model(pid: u16) -> Option<&'static Model> {
    MODELS.iter().find(|(p, _)| *p == pid).map(|(_, m)| m)
}

pub struct RazerPeripheralProvider;

impl Provider for RazerPeripheralProvider {
    fn name(&self) -> &'static str {
        "Razer"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        // A mouse on its cable and on its receiver at once shows up twice: keep the reading that has a level
        let found = hid::enumerate(|v, p| v == RAZER_VID && model(p).is_some());
        let mut result = Vec::new();
        for (pid, collections) in group_by(found, |i| i.product_id) {
            let Some(model) = model(pid) else { continue };
            let candidates: Vec<&HidInfo> =
                collections.iter().filter(|i| i.interface_number().max(0) == model.interface).collect();
            result.push(query(model, &candidates));
        }
        one_per_id(result, |r| r.level.is_some())
    }

    fn delay(&self, last: &[DeviceReading]) -> Duration {
        if last.iter().any(|r| r.online) {
            secs(60)
        } else {
            secs(20)
        }
    }
}

fn query(model: &Model, candidates: &[&HidInfo]) -> DeviceReading {
    let mut reading = DeviceReading {
        detail: Some(t("d.asleep")),
        ..DeviceReading::new(
            format!("razer:{}", model.key),
            format!("Razer {}", model.name),
            model.kind,
            t(if model.wireless { "via.24" } else { "via.cable" }),
        )
    };
    for info in candidates {
        if info.access_denied {
            reading.detail = Some(t("d.needsAdmin"));
            continue;
        }
        if info.feature_length != REPORT_LENGTH {
            continue;
        }
        let dev = match HidDevice::open_features_only(info) {
            Ok(dev) => dev,
            Err(e) => {
                reading.detail = Some(tf("d.error", &[&e]));
                continue;
            }
        };
        let Some(level) = command(&dev, model, 0x07, 0x80) else { continue };
        let percent = (level[1] as f64 * 100.0 / 255.0).round() as i32;
        if percent == 0 {
            continue; // OpenRazer treats 0 as a bogus reading from a sleeping mouse
        }
        let charging = model.query_charging && command(&dev, model, 0x07, 0x84).is_some_and(|c| c[1] != 0);
        reading.online = true;
        reading.level = Some(percent);
        reading.charging = charging;
        reading.detail = None;
        return reading;
    }
    reading
}

/// Sends one command and returns its reply arguments, or None (no reply, timeout, not supported).
fn command(dev: &HidDevice, model: &Model, class: u8, id: u8) -> Option<Vec<u8>> {
    for attempt in 0..3 {
        if attempt > 0 {
            sleep_ms(10);
        }
        let mut request = vec![0u8; REPORT_LENGTH];
        request[2] = model.tid;
        request[6] = 0x02; // data size
        request[7] = class;
        request[8] = id;
        request[89] = request[3..=88].iter().fold(0, |crc, &b| crc ^ b); // report bytes 2..87
        if !dev.set_feature(&request) {
            return None;
        }

        sleep_ms(model.wait_ms.max(1));
        let reply = dev.get_feature(0)?;
        if reply.len() < REPORT_LENGTH {
            return None;
        }
        if reply[7] != class || reply[8] != id {
            continue; // someone else's reply (Synapse)
        }
        // OpenRazer: "some commands respond with 'busy' but succeed" - busy counts as success
        return matches!(reply[1], 0x02 | 0x01).then(|| reply[9..89].to_vec());
    }
    None
}
