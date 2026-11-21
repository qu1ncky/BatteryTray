//! HyperX devices that `headset.rs` doesn't have: three more headset families and the Pulsefire mice, which are
//! here and not with the other mice because they speak the same frames as the headsets. Four protocols:
//!
//! Cloud III S Wireless. Output report 0C on the dongle's 64-byte collection (Windows lists it as 01C0:0001, not
//! as a vendor page): `0C 02 03 01 00 CC`, where the 01 means "read". The reply is input report 0C with CC in
//! byte 5 and the value in byte 6, FF for "no value": CC 02 the link (2 = headset connected), 06 the level in
//! percent, 48 the charger (1 charging, 2 full).
//!
//! NGENUITY's "universal" protocol: Cloud Alpha 2 Wireless, Pulsefire Haste 2 Wireless / Core Wireless / Mini.
//! 64-byte reports whose report ID is the command: `CC SS` is answered by input report `CC+1 SS`. `52 01` asks
//! for the radio link (`53 01 LL`, 0 = off, asleep or out of range), `50 02` for the battery (`51 02 PP ST ..`:
//! PP percent, ST 1 charging, 2 full, then temperature and voltage). The receiver also acknowledges every request
//! (FF ..) and sends events (FB ..), and the vendor's software polls the same collection, so only the report that
//! echoes the request counts.
//!
//! NGENUITY2: Pulsefire Haste Wireless, Pulsefire Dart. 64-byte reports without report IDs whose first byte is
//! the command, echoed by the reply: `46` the connection (`46 00 00 01 AA`: through the receiver, AA 1 = awake;
//! `46 00 00 02 ..`: on its cable), `51` the "heartbeat" (`51 00 00 nn PP ST`: PP percent, ST 1 charging, 2 full).
//!
//! Cloud Stinger Core Wireless: feature report FF of the receiver's FF00:0001 collection. A request is written,
//! and the answer is read back from the same report a few tens of milliseconds later; until then, and again soon
//! after, the receiver hands out an idle report `FF 01 00` with the old bytes behind it. The battery answer starts
//! `FF 12 05` and carries a charge code in byte 9 (3 on battery, 5 charging, 6 full) and the cell voltage in
//! bytes 11-12; there is no percentage, the vendor's software works it out from the voltage.
//!
//! Protocol facts from HyperHeadset (github.com/LennardKittner/HyperHeadset: src/devices/cloud_iii_s_wireless.rs,
//! issue #59), HeadsetControl (github.com/Sapd/HeadsetControl: lib/devices/hyperx_cloud_alpha_2_wireless.hpp and
//! its test), ngenuity-linux-cli (github.com/PMarinov1994/ngenuity-linux-cli: docs/protocol/universal.md,
//! docs/devices/pulsefire-haste-2-wireless.md), HyperBatteryMonitor (github.com/davidsantanaalves/
//! HyperBatteryMonitor: Devices/CloudStingerCoreWirelessDevice.cs, its PR #6), the reverse-engineering notes for
//! the Pulsefire Dart (github.com/santeri3700/hyperx_pulsefire_dart_reverse_engineering: protocol/index.md,
//! captures) and for the Pulsefire Haste (github.com/evan-razzaque/open-pulsefire-haste: protocol/README.md),
//! OpenMouse (github.com/OpenMouse-Project/mouse-protocol: src/hyperx/index.ts) and Wireless-Device-Battery-Monitor
//! (github.com/krisgfx9/Wireless-Device-Battery-Monitor: src/drivers/hyperx.cpp, docs/hyperx_haste_2.md, issues
//! #112, #376 and #401, pull requests #295, #477, #656, #3667, #3964 and #3988), where owners report which
//! Windows collection answers and what it says. Own implementation. Every request here only reads. Not yet verified on hardware by this project,
//! which is why the first exchange with each device goes to the log.

use super::{off, off_because, on, Io, Spec, State};
use crate::hid::HidInfo;
use crate::providers::{pad, sleep_ms};
use crate::win;
use std::sync::Mutex;

const HASTE_2_CABLE: u16 = 0x0D97;
const HASTE_CABLE: u16 = 0x048E;

pub static SPECS: &[Spec] = &[
    // ---- Headsets ----
    Spec {
        key: "hyperx-cloud-iii-s-wireless",
        name: "HyperX Cloud III S Wireless",
        vid: 0x03F0,
        pids: &[0x06BE, 0x02CC],
        accept: Some(is_cloud_iii_s_collection),
        query: cloud_iii_s,
        ..Spec::HEADSET
    },
    // The base station is two USB devices; the other one, 03F0:0ABE, only carries audio
    Spec {
        key: "hyperx-cloud-alpha-2-wireless",
        name: "HyperX Cloud Alpha 2 Wireless",
        vid: 0x03F0,
        pids: &[0x08BE],
        page: 0xFF13,
        usage: 0xFF00,
        query: cloud_alpha_2,
        ..Spec::HEADSET
    },
    // The PlayStation version's receiver, under HP's and under Kingston's vendor ID
    Spec {
        key: "hyperx-cloud-stinger-core-wireless",
        name: "HyperX Cloud Stinger Core Wireless",
        vid: 0x03F0,
        pids: &[0x0A93],
        interface: 3,
        accept: Some(is_stinger_collection),
        query: stinger_core,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-stinger-core-wireless",
        name: "HyperX Cloud Stinger Core Wireless",
        vid: 0x0951,
        pids: &[0x1713],
        interface: 3,
        accept: Some(is_stinger_collection),
        query: stinger_core,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-stinger-core-wireless-dts",
        name: "HyperX Cloud Stinger Core Wireless DTS",
        vid: 0x03F0,
        pids: &[0x0A8E],
        interface: 3,
        accept: Some(is_stinger_collection),
        query: stinger_core,
        ..Spec::HEADSET
    },
    // ---- Mice ----
    // The mouse on its cable leaves the receiver and shows up under the cable ID; both share a key, and the
    // reading that reaches the mouse wins (see the engine's poll)
    Spec {
        key: "hyperx-pulsefire-haste-2-wireless",
        name: "HyperX Pulsefire Haste 2 Wireless",
        vid: 0x03F0,
        pids: &[0x0F98, HASTE_2_CABLE],
        cable_pids: &[HASTE_2_CABLE],
        accept: Some(is_universal_collection),
        query: pulsefire_haste_2,
        ..Spec::MOUSE
    },
    Spec {
        key: "hyperx-pulsefire-haste-2-core-wireless",
        name: "HyperX Pulsefire Haste 2 Core Wireless",
        vid: 0x03F0,
        pids: &[0x0AB5],
        accept: Some(is_universal_collection),
        query: pulsefire_haste_2,
        ..Spec::MOUSE
    },
    Spec {
        key: "hyperx-pulsefire-haste-2-mini",
        name: "HyperX Pulsefire Haste 2 Mini",
        vid: 0x03F0,
        pids: &[0x0DA0],
        accept: Some(is_universal_collection),
        query: pulsefire_haste_2,
        ..Spec::MOUSE
    },
    Spec {
        key: "hyperx-pulsefire-haste-wireless",
        name: "HyperX Pulsefire Haste Wireless",
        vid: 0x03F0,
        pids: &[0x028E, HASTE_CABLE],
        cable_pids: &[HASTE_CABLE],
        page: 0xFF00,
        usage: 0x0001,
        interface: 2,
        query: |io| {
            let on_cable = io.product_id() == HASTE_CABLE;
            pulsefire_ngenuity2(io, on_cable)
        },
        ..Spec::MOUSE
    },
    Spec {
        key: "hyperx-pulsefire-dart",
        name: "HyperX Pulsefire Dart",
        vid: 0x0951,
        pids: &[0x16E1],
        page: 0xFF00,
        usage: 0x0001,
        interface: 2,
        query: |io| pulsefire_ngenuity2(io, false),
        ..Spec::MOUSE
    },
];

/// How long a reply is waited for, as the sources do. The radio hop to the device is inside it.
const TIMEOUT_MS: u64 = 1000;

/// On, but without a level the reading can stand for.
fn level_unknown() -> State {
    State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, false) }
}

// ---- Cloud III S Wireless ----

const CLOUD_III_S_LINK: u8 = 0x02;
const CLOUD_III_S_LEVEL: u8 = 0x06;
const CLOUD_III_S_CHARGER: u8 = 0x48;

/// The dongle has six collections on one interface. Report 0C belongs to the only one with 64-byte reports both
/// ways, which Windows lists under the usage page 01C0; the vendor-page ones (FF13 with 62 bytes, FFC0 with 32)
/// refuse it.
fn is_cloud_iii_s_collection(info: &HidInfo) -> bool {
    info.input_length == 64 && info.output_length == 64
}

/// Three requests a poll and no more: the dongle has been seen to stop answering after bursts of writes.
fn cloud_iii_s(io: &mut Io) -> win::Result<State> {
    io.set_timeout(TIMEOUT_MS);
    // The dongle ignores the same bytes sent as a feature report, so there is no falling back to one
    let ask = |io: &mut Io, value: u8| -> win::Result<Option<u8>> {
        let reply = io.ask(&cloud_iii_s_request(value), |d| is_cloud_iii_s_reply(d, value), 16, false)?;
        Ok(reply.map(|r| r[6]))
    };
    if ask(io, CLOUD_III_S_LINK)? != Some(0x02) {
        return Ok(off());
    }
    let level = ask(io, CLOUD_III_S_LEVEL)?;
    let charger = ask(io, CLOUD_III_S_CHARGER)?;
    Ok(cloud_iii_s_state(level, charger))
}

/// The request that reads `value`. Byte 3 is what makes it a read: with 00 there the same frame sets the value.
fn cloud_iii_s_request(value: u8) -> Vec<u8> {
    pad(64, &[0x0C, 0x02, 0x03, 0x01, 0x00, value])
}

/// The answer to a read of `value`. The mute switch (05 ..), the buttons (0F ..) and the dongle's own
/// notifications (0D ..) come as other reports.
fn is_cloud_iii_s_reply(d: &[u8], value: u8) -> bool {
    d.len() >= 7 && d[0] == 0x0C && d[5] == value
}

/// `level`: percent, FF when the headset has none to give. `charger`: 1 charging, 2 full with the cable in.
fn cloud_iii_s_state(level: Option<u8>, charger: Option<u8>) -> State {
    let (charging, full) = (charger == Some(1), charger == Some(2));
    match level {
        Some(level) if level <= 100 => State { full, ..on(level as i32, charging) },
        _ => State { full, charging, ..level_unknown() },
    }
}

// ---- NGENUITY's "universal" protocol ----

const UNIVERSAL_LINK: (u8, u8) = (0x52, 0x01);
const UNIVERSAL_BATTERY: (u8, u8) = (0x50, 0x02);

/// With the vendor's software running the collection carries dozens of reports a second (it streams the lighting
/// and every frame is acknowledged), so the time limit ends the wait and not the number of reports.
const UNIVERSAL_READS: usize = 128;

/// The mice's vendor collection: FF00:0001 on the Haste 2 Wireless, FF90:FF00 on the Mini, 64-byte reports on
/// both. The Mini also has a vendor collection with nothing but a 65-byte feature report, which isn't it.
fn is_universal_collection(info: &HidInfo) -> bool {
    info.usage_page >= 0xFF00 && info.input_length == 64 && info.output_length == 64
}

fn universal_ask(io: &mut Io, (command, sub): (u8, u8)) -> win::Result<Option<Vec<u8>>> {
    io.ask(&pad(64, &[command, sub]), |d| is_universal_reply(d, command, sub), UNIVERSAL_READS, false)
}

/// The report that answers `command sub`: acknowledgements (FF ..), events (FB ..) and the answers to what other
/// software asked don't count.
fn is_universal_reply(d: &[u8], command: u8, sub: u8) -> bool {
    d.len() >= 4 && d[0] == command + 1 && d[1] == sub
}

/// What a battery reply `51 02 PP ST` says. ST: 0 not charging, 1 charging, 2 full, 3 charger error, FF unknown.
fn universal_state(d: &[u8]) -> State {
    let (charging, full) = (d[3] == 1, d[3] == 2);
    if d[2] <= 100 {
        State { full, ..on(d[2] as i32, charging) }
    } else {
        State { full, charging, ..level_unknown() }
    }
}

fn cloud_alpha_2(io: &mut Io) -> win::Result<State> {
    io.set_timeout(TIMEOUT_MS);
    // The base station answers the link query by itself, so without that answer nothing else is asked
    match universal_ask(io, UNIVERSAL_LINK)? {
        Some(link) if link[2] != 0 => {}
        _ => return Ok(off()),
    }
    Ok(universal_ask(io, UNIVERSAL_BATTERY)?.map_or_else(level_unknown, |b| universal_state(&b)))
}

/// For a few seconds after it is switched on a Haste 2 reports about 40% whatever its charge
/// (Wireless-Device-Battery-Monitor measured 40% for 2-5 s and then 70% on a Core Wireless).
const HASTE_2_SETTLE_MS: u64 = 6000;

/// How far the level may be from the one of the poll before and still be taken as it comes.
const HASTE_2_STEP: i32 = 5;

/// The level each Haste 2 had at the poll before, by product ID; no entry while it was off or not yet polled.
/// The table engine keeps nothing between polls, and telling a mouse that has just come on takes that.
static HASTE_2_LEVELS: Mutex<Vec<(u16, i32)>> = Mutex::new(Vec::new());

fn haste_2_level_before(pid: u16) -> Option<i32> {
    let levels = HASTE_2_LEVELS.lock().unwrap_or_else(|e| e.into_inner());
    levels.iter().find(|l| l.0 == pid).map(|l| l.1)
}

fn haste_2_remember(pid: u16, level: Option<i32>) {
    let mut levels = HASTE_2_LEVELS.lock().unwrap_or_else(|e| e.into_inner());
    levels.retain(|l| l.0 != pid);
    if let Some(level) = level {
        levels.push((pid, level));
    }
}

/// Whether a level can be shown as it came: only next to the one of the poll before. After the mouse was off or
/// asleep, at the first poll, and after a jump no battery makes in a minute, it may be the made-up 40%.
fn haste_2_is_settled(before: Option<i32>, level: i32) -> bool {
    before.is_some_and(|b| (level - b).abs() <= HASTE_2_STEP)
}

fn pulsefire_haste_2(io: &mut Io) -> win::Result<State> {
    io.set_timeout(TIMEOUT_MS);
    let pid = io.product_id();
    // On its cable the mouse answers for itself, and the link query is about the radio
    let on_cable = pid == HASTE_2_CABLE;
    let link = if on_cable { None } else { universal_ask(io, UNIVERSAL_LINK)?.map(|l| l[2] != 0) };
    if link == Some(false) {
        // Nothing more is asked: the receiver would keep the request and deliver it when the mouse wakes
        haste_2_remember(pid, None);
        return Ok(off_because("d.asleep"));
    }
    // After a reconnect the receiver sends so much that the link answer can be missed. The battery answer comes
    // from the mouse itself, never from the receiver, so it settles whether the mouse is there.
    let Some(reply) = universal_ask(io, UNIVERSAL_BATTERY)? else {
        haste_2_remember(pid, None);
        return Ok(match link {
            Some(_) => level_unknown(),
            None if on_cable => off_because("d.noLink"),
            None => off(),
        });
    };
    let mut state = universal_state(&reply);
    if state.level.is_some_and(|level| !haste_2_is_settled(haste_2_level_before(pid), level)) {
        // Asked again once the mouse has had the time to measure
        sleep_ms(HASTE_2_SETTLE_MS);
        state = universal_ask(io, UNIVERSAL_BATTERY)?.map_or_else(level_unknown, |b| universal_state(&b));
    }
    haste_2_remember(pid, state.level);
    Ok(state)
}

// ---- NGENUITY2: Pulsefire Haste Wireless, Pulsefire Dart ----

const NGENUITY2_CONNECTION: u8 = 0x46;
const NGENUITY2_HEARTBEAT: u8 = 0x51;

/// The mouse reports every click and every sleep and wake as an event (FF ..) on the same collection.
const NGENUITY2_READS: usize = 64;

fn ngenuity2_ask(io: &mut Io, command: u8) -> win::Result<Option<Vec<u8>>> {
    // No report IDs: the 00 in front is Windows' and the 64 bytes of the frame follow it
    io.ask(&pad(65, &[0x00, command]), |d| is_ngenuity2_reply(d, command), NGENUITY2_READS, false)
}

/// The report that echoes `command`, with the two zero bytes every answer has after it.
fn is_ngenuity2_reply(d: &[u8], command: u8) -> bool {
    d.len() >= 6 && d[0] == command && d[1] == 0x00 && d[2] == 0x00
}

/// What a connection reply `46 00 00 TT AA` says about a mouse behind its receiver: Some(true) awake and linked,
/// Some(false) asleep or off, None when the reply is about something else (TT 2 is a mouse on its cable).
fn ngenuity2_awake(d: &[u8]) -> Option<bool> {
    match (d[3], d[4]) {
        (0x01, 0x01) => Some(true),
        (0x01, 0x00) => Some(false),
        _ => None,
    }
}

/// What a heartbeat `51 00 00 nn PP ST` says. A mouse that has no level to give sends 0 or FF for it, which
/// some firmware does all the time on the cable.
fn ngenuity2_state(d: &[u8]) -> State {
    let (charging, full) = (d[5] == 1, d[5] == 2);
    if (1..=100).contains(&d[4]) {
        State { full, ..on(d[4] as i32, charging) }
    } else {
        State { full, charging, ..level_unknown() }
    }
}

fn pulsefire_ngenuity2(io: &mut Io, on_cable: bool) -> win::Result<State> {
    io.set_timeout(TIMEOUT_MS);
    // The sources don't say what a receiver answers to the heartbeat while its mouse sleeps, so the connection
    // is asked first and the heartbeat only of a mouse that is awake
    if !on_cable {
        match ngenuity2_ask(io, NGENUITY2_CONNECTION)?.and_then(|c| ngenuity2_awake(&c)) {
            Some(true) => {}
            Some(false) => return Ok(off_because("d.asleep")),
            None => return Ok(off()),
        }
    }
    Ok(match ngenuity2_ask(io, NGENUITY2_HEARTBEAT)? {
        Some(heartbeat) => ngenuity2_state(&heartbeat),
        None if on_cable => off_because("d.noLink"),
        None => level_unknown(),
    })
}

// ---- Cloud Stinger Core Wireless ----

const STINGER_REPORT: u8 = 0xFF;

/// The two status requests the vendor's software sends, byte for byte as HyperBatteryMonitor has them and as
/// owners ran them. What follows 82 in the first is not part of the request (it looks like the tail of an earlier
/// battery answer left in the buffer), but this is the frame that was tried on the hardware.
const STINGER_LINK: [u8; 20] = [
    0xFF, 0x07, 0x00, 0xFD, 0x04, 0x00, 0x00, 0x02, 0x82, 0x03, 0x05, 0x29, 0x0F, 0x00, 0x00, 0x03, 0x68, 0x10, 0x00,
    0x00,
];
const STINGER_BATTERY: [u8; 12] = [0xFF, 0x07, 0x00, 0xFD, 0x04, 0x0C, 0xF1, 0x02, 0x01, 0x04, 0xF0, 0x0C];

/// The answer is ready 20-30 ms after the request and is gone again soon after: the report is read every 20 ms,
/// ten times at most.
const STINGER_READ_EVERY_MS: u64 = 20;
const STINGER_READS: usize = 10;

/// Cell voltages (mV) at 0%, 10% ... 100% on battery, as HyperBatteryMonitor has them for this headset.
const STINGER_MV: [i32; 11] = [3517, 3651, 3700, 3738, 3763, 3785, 3838, 3905, 3954, 4042, 4121];

/// The receiver's third collection on interface 3; the other two are the media keys and the mute switch.
fn is_stinger_collection(info: &HidInfo) -> bool {
    info.usage_page == 0xFF00 && info.usage == 0x0001 && info.feature_length >= 32
}

fn stinger_core(io: &mut Io) -> win::Result<State> {
    // Byte 11 of the link answer: 1 = the headset is on (3 was seen with it off)
    match stinger_exchange(io, &STINGER_LINK, is_stinger_answer) {
        Some(link) if link[11] == 0x01 => {}
        _ => return Ok(off()),
    }
    Ok(stinger_exchange(io, &STINGER_BATTERY, is_stinger_battery)
        .and_then(|battery| stinger_state(&battery))
        .unwrap_or_else(level_unknown))
}

/// Writes a request and reads the report until it holds what `accepts` takes for the answer.
fn stinger_exchange(io: &mut Io, request: &[u8], accepts: fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    if !io.set_feature(request) {
        return None;
    }
    for _ in 0..STINGER_READS {
        sleep_ms(STINGER_READ_EVERY_MS);
        let report = io.get_feature(STINGER_REPORT)?;
        if accepts(&report) {
            return Some(report);
        }
    }
    None
}

/// An answer, and not the idle report `FF 01 00` the receiver hands out between answers (with the bytes of the
/// last answer still behind the header, which is why it must not be read as one).
fn is_stinger_answer(r: &[u8]) -> bool {
    r.len() >= 13 && r[0] == STINGER_REPORT && !(r[1] == 0x01 && r[2] == 0x00)
}

fn is_stinger_battery(r: &[u8]) -> bool {
    r.len() >= 13 && r[0] == STINGER_REPORT && r[1] == 0x12 && r[2] == 0x05
}

/// What a battery answer says; None when it has no charge code or no voltage a cell can have.
fn stinger_state(r: &[u8]) -> Option<State> {
    let mv = u16::from_le_bytes([r[11], r[12]]) as i32;
    match r[9] {
        // The level is worked out here from the voltage, and is marked so wherever it is shown
        0x03 if (3000..=4400).contains(&mv) => {
            Some(State { detail: Some("d.estimate".into()), estimate: true, ..on(stinger_percent(mv), false) })
        }
        // The charger lifts the voltage at once, and the sources say their curve for a charging headset reads
        // high on these receivers: no level until the cable is out
        0x05 => Some(State { detail: Some("d.cableHidden".into()), ..State::new(true, None, true) }),
        0x06 => Some(State { full: true, ..on(100, false) }),
        _ => None,
    }
}

/// The level for a voltage on battery: a straight line between the two points of the curve around it.
fn stinger_percent(mv: i32) -> i32 {
    let Some(above) = STINGER_MV.iter().position(|&point| mv < point) else { return 100 };
    if above == 0 {
        return 0;
    }
    let (low, high) = (STINGER_MV[above - 1], STINGER_MV[above]);
    (above as i32 - 1) * 10 + (mv - low) * 10 / (high - low)
}

#[cfg(test)]
mod tests {
    //! Frames as the devices send them, report ID first. The captured ones are from HeadsetControl's test for the
    //! Cloud Alpha 2, ngenuity-linux-cli's notes for the Haste 2 Wireless, the Pulsefire Dart captures, an
    //! owner's report in Wireless-Device-Battery-Monitor (issue #112) for the Cloud III S, and owners' reports in
    //! HyperHeadset (issue #59) and Wireless-Device-Battery-Monitor (pull request #477) for the battery answer of
    //! the Stinger Core's receiver family.
    use super::*;

    fn h(text: &str) -> Vec<u8> {
        pad(64, &text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect::<Vec<_>>())
    }

    fn state(level: i32, charging: bool) -> State {
        State::new(true, Some(level), charging)
    }

    fn unknown(charging: bool) -> State {
        State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, charging) }
    }

    /// A collection as Windows lists it: interface, usage page, usage and the three report lengths.
    fn collection(mi: u8, page: u16, usage: u16, lengths: (usize, usize, usize)) -> HidInfo {
        HidInfo {
            path: format!(r"\\?\hid#vid_03f0&pid_0000&mi_{mi:02x}&col{page:04x}{usage:04x}#8&1&0&0000#{{4d1e55b2}}"),
            vendor_id: 0x03F0,
            usage_page: page,
            usage,
            input_length: lengths.0,
            output_length: lengths.1,
            feature_length: lengths.2,
            ..HidInfo::default()
        }
    }

    // ---- Cloud III S Wireless ----

    #[test]
    fn cloud_iii_s_requests_are_reads() {
        let request = cloud_iii_s_request(CLOUD_III_S_LEVEL);
        assert_eq!(request.len(), 64);
        assert_eq!(request[..7], [0x0C, 0x02, 0x03, 0x01, 0x00, 0x06, 0x00]);
        assert_eq!(cloud_iii_s_request(CLOUD_III_S_LINK)[3], 0x01);
        assert_eq!(cloud_iii_s_request(CLOUD_III_S_CHARGER)[3], 0x01);
    }

    #[test]
    fn cloud_iii_s_level_as_an_owner_captured_it() {
        // 39%, with what looks like the voltage after it
        let reply = h("0C 02 03 01 00 06 27 0E B8");
        assert!(is_cloud_iii_s_reply(&reply, CLOUD_III_S_LEVEL));
        assert_eq!(cloud_iii_s_state(Some(reply[6]), Some(0)), state(39, false));
        // The same owner's link answer: 2 = connected
        let link = h("0C 02 03 01 00 02 02");
        assert!(is_cloud_iii_s_reply(&link, CLOUD_III_S_LINK));
        assert_eq!(link[6], 0x02);
    }

    #[test]
    fn cloud_iii_s_answer_to_another_read_is_not_the_reply() {
        assert!(!is_cloud_iii_s_reply(&h("0C 02 03 01 00 48 01"), CLOUD_III_S_LEVEL));
        assert!(!is_cloud_iii_s_reply(&h("0D 00 00 00 01 37"), CLOUD_III_S_LEVEL)); // the dongle's own notification
        assert!(!is_cloud_iii_s_reply(&h("0F 06"), CLOUD_III_S_LEVEL)); // a button
        assert!(!is_cloud_iii_s_reply(&[0x0C, 0x02, 0x03, 0x01, 0x00, 0x06], CLOUD_III_S_LEVEL));
        // cut short
    }

    #[test]
    fn cloud_iii_s_charging_and_full() {
        assert_eq!(cloud_iii_s_state(Some(38), Some(1)), state(38, true));
        assert_eq!(cloud_iii_s_state(Some(100), Some(2)), State { full: true, ..state(100, false) });
        assert_eq!(cloud_iii_s_state(Some(80), None), state(80, false));
    }

    #[test]
    fn cloud_iii_s_without_a_value_has_no_level() {
        assert_eq!(cloud_iii_s_state(Some(0xFF), Some(1)), unknown(true));
        assert_eq!(cloud_iii_s_state(Some(101), Some(0)), unknown(false));
        assert_eq!(cloud_iii_s_state(None, Some(0xFF)), unknown(false));
    }

    #[test]
    fn cloud_iii_s_talks_to_the_64_byte_collection_only() {
        // The six collections of 03F0:06BE, as two owners' dumps list them
        let all = [
            collection(3, 0x170F, 0x0202, (8, 8, 0)),
            collection(3, 0x000C, 0x0001, (2, 0, 0)),
            collection(3, 0xFF13, 0x0001, (62, 62, 0)),
            collection(3, 0x000B, 0x0005, (2, 2, 0)),
            collection(3, 0xFFC0, 0x0202, (32, 32, 0)),
            collection(3, 0x01C0, 0x0001, (64, 64, 0)),
        ];
        let picked: Vec<u16> = all.iter().filter(|i| is_cloud_iii_s_collection(i)).map(|i| i.usage_page).collect();
        assert_eq!(picked, [0x01C0]);
    }

    // ---- NGENUITY's "universal" protocol ----

    #[test]
    fn cloud_alpha_2_on_battery_and_charging() {
        // 93%, 25 degrees, 4193 mV; then 91% on the charger
        let reply = h("51 02 5D 00 19 00 61 10 01 01 0A");
        assert!(is_universal_reply(&reply, 0x50, 0x02));
        assert_eq!(universal_state(&reply), state(93, false));
        assert_eq!(universal_state(&h("51 02 5B 01 1C 00 EA 10 01 01 0A")), state(91, true));
    }

    #[test]
    fn haste_2_wireless_on_battery() {
        // 54%, 26 degrees, 3840 mV
        assert_eq!(universal_state(&h("51 02 36 00 1A 00 00 0F 01 00 00")), state(54, false));
    }

    #[test]
    fn universal_full_on_the_charger_is_full() {
        assert_eq!(universal_state(&h("51 02 64 02")), State { full: true, ..state(100, false) });
    }

    #[test]
    fn universal_charger_error_and_unknown_are_not_charging() {
        assert_eq!(universal_state(&h("51 02 32 03")), state(50, false));
        assert_eq!(universal_state(&h("51 02 32 FF")), state(50, false));
    }

    #[test]
    fn universal_level_over_a_hundred_is_no_level() {
        assert_eq!(universal_state(&h("51 02 65 00")), unknown(false));
        assert_eq!(universal_state(&h("51 02 FF 01")), unknown(true));
    }

    #[test]
    fn universal_link_answer() {
        let (command, sub) = UNIVERSAL_LINK;
        assert!(is_universal_reply(&h("53 01 01"), command, sub));
        assert!(is_universal_reply(&h("53 01 00"), command, sub));
        assert!(!is_universal_reply(&h("53 01 01"), 0x50, 0x02));
    }

    #[test]
    fn universal_traffic_of_others_is_not_the_reply() {
        let (command, sub) = UNIVERSAL_BATTERY;
        // The receiver's acknowledgement: FF 01, its ID, then the request echoed
        let mut ack = h("FF 01");
        ack[14] = command;
        ack[15] = sub;
        assert!(!is_universal_reply(&ack, command, sub));
        assert!(!is_universal_reply(&h("45 02 01"), command, sub)); // the answer to the vendor software's lighting
        assert!(!is_universal_reply(&h("FB 13 00 5D"), command, sub)); // an event
        assert!(!is_universal_reply(&h("FB 0A 01"), command, sub)); // the mouse woke up
        assert!(!is_universal_reply(&h("51 01"), command, sub)); // the answer to a write of the alert level
        assert!(!is_universal_reply(&[0x51, 0x02, 0x36], command, sub)); // cut short
    }

    #[test]
    fn universal_requests_are_the_two_reads() {
        assert_eq!(UNIVERSAL_LINK, (0x52, 0x01));
        // 50 01 would set the low-battery alert
        assert_eq!(UNIVERSAL_BATTERY, (0x50, 0x02));
    }

    #[test]
    fn haste_2_collections() {
        // Haste 2 Wireless (0F98) and Haste 2 Mini (0DA0) as owners' dumps list them
        assert!(is_universal_collection(&collection(2, 0xFF00, 0x0001, (64, 64, 64))));
        assert!(is_universal_collection(&collection(2, 0xFF90, 0xFF00, (64, 64, 64))));
        assert!(!is_universal_collection(&collection(1, 0xFFFF, 0x0001, (0, 0, 65))));
        assert!(!is_universal_collection(&collection(0, 0x0001, 0x0002, (12, 0, 0))));
        assert!(!is_universal_collection(&collection(1, 0x000C, 0x0001, (3, 0, 0))));
    }

    #[test]
    fn haste_2_level_after_it_was_off_is_asked_again() {
        assert!(!haste_2_is_settled(None, 40));
        assert!(!haste_2_is_settled(None, 70));
    }

    #[test]
    fn haste_2_level_next_to_the_last_one_is_taken() {
        assert!(haste_2_is_settled(Some(70), 70));
        assert!(haste_2_is_settled(Some(70), 69));
        assert!(haste_2_is_settled(Some(70), 75));
    }

    #[test]
    fn haste_2_level_is_remembered_until_the_mouse_goes_off() {
        // A product ID no row has, so no other test's mouse is in the way
        let pid = 0xFFF0;
        assert_eq!(haste_2_level_before(pid), None);
        haste_2_remember(pid, Some(70));
        haste_2_remember(pid, Some(69));
        assert_eq!(haste_2_level_before(pid), Some(69));
        // Each mouse has its own
        assert_eq!(haste_2_level_before(pid + 1), None);
        // Off, asleep or without a level: the next one is asked again
        haste_2_remember(pid, None);
        assert_eq!(haste_2_level_before(pid), None);
    }

    #[test]
    fn haste_2_jump_is_asked_again() {
        // Switched off and on between two polls: 70% before, the made-up 40% now
        assert!(!haste_2_is_settled(Some(70), 40));
        assert!(!haste_2_is_settled(Some(20), 40));
    }

    // ---- NGENUITY2 ----

    #[test]
    fn dart_heartbeat_as_captured() {
        // 57% and 98%, both while charging
        let reply = h("51 00 00 03 39 01 0F 84 0F 02 40");
        assert!(is_ngenuity2_reply(&reply, NGENUITY2_HEARTBEAT));
        assert_eq!(ngenuity2_state(&reply), state(57, true));
        assert_eq!(ngenuity2_state(&h("51 00 00 03 62 01 0F 66 10 00 40")), state(98, true));
    }

    #[test]
    fn haste_heartbeat() {
        assert_eq!(ngenuity2_state(&h("51 00 00 09 49 00 0A 1D 00")), state(73, false));
        assert_eq!(ngenuity2_state(&h("51 00 00 09 40 01 0A 1D 00")), state(64, true));
        assert_eq!(ngenuity2_state(&h("51 00 00 09 64 02 0A 1D 00")), State { full: true, ..state(100, false) });
    }

    #[test]
    fn ngenuity2_level_of_zero_or_ff_is_no_level() {
        assert_eq!(ngenuity2_state(&h("51 00 00 09 00 01")), unknown(true));
        assert_eq!(ngenuity2_state(&h("51 00 00 09 FF 00")), unknown(false));
        assert_eq!(ngenuity2_state(&h("51 00 00 09 65 00")), unknown(false));
    }

    #[test]
    fn ngenuity2_connection_as_captured() {
        let awake = h("46 00 00 01 01");
        assert!(is_ngenuity2_reply(&awake, NGENUITY2_CONNECTION));
        assert_eq!(ngenuity2_awake(&awake), Some(true));
        assert_eq!(ngenuity2_awake(&h("46 00 00 01 00")), Some(false));
        // A mouse on its cable answers about the cable
        assert_eq!(ngenuity2_awake(&h("46 00 00 02 EC AC")), None);
        assert_eq!(ngenuity2_awake(&h("46 00 00 00 00")), None);
    }

    #[test]
    fn ngenuity2_events_and_other_answers_are_not_the_reply() {
        assert!(!is_ngenuity2_reply(&h("FF 03 00 00 01"), NGENUITY2_HEARTBEAT)); // woke up
        assert!(!is_ngenuity2_reply(&h("46 00 00 01 01"), NGENUITY2_HEARTBEAT));
        assert!(!is_ngenuity2_reply(&h("50 00 00 1D E2 16 51 09"), NGENUITY2_HEARTBEAT)); // hardware information
        assert!(!is_ngenuity2_reply(&[0x51, 0x00, 0x00, 0x03, 0x39], NGENUITY2_HEARTBEAT));
        // cut short
    }

    // ---- Cloud Stinger Core Wireless ----

    #[test]
    fn stinger_battery_answer_as_captured() {
        // From the receiver of the Cloud Core Wireless, which answers the same request: 3602 mV and 4085 mV on
        // battery, then 4102 mV on the charger
        let estimate = |level| Some(State { detail: Some("d.estimate".into()), estimate: true, ..state(level, false) });
        let reply = h("FF 12 05 FE 0C 04 1F 0B 01 03 05 12 0E 00 00 03 68 10");
        assert!(is_stinger_battery(&reply));
        assert_eq!(stinger_state(&reply), estimate(6));
        assert_eq!(stinger_state(&h("FF 12 05 FE 0C 04 1F 0B 01 03 05 F5 0F 00 00 03")), estimate(95));
        let charging = h("FF 12 05 FE 0C 04 1F 0B 01 05 03 06 10 5B 00 03");
        assert!(is_stinger_battery(&charging));
        assert_eq!(stinger_state(&charging).map(|s| (s.level, s.charging)), Some((None, true)));
    }

    #[test]
    fn stinger_idle_report_is_not_an_answer() {
        // The header of an idle receiver in front of the bytes of the last answer
        let idle = h("FF 01 00 FE 0C 04 1F 0B 01 03 05 12 0E 00 00 03 68 10");
        assert!(!is_stinger_answer(&idle));
        assert!(!is_stinger_battery(&idle));
        assert!(is_stinger_answer(&h("FF 12 05 FE 0C 04 1F 0B 01 03 05 12 0E")));
        assert!(!is_stinger_answer(&[0xFF, 0x12, 0x05]));
    }

    #[test]
    fn stinger_other_answers_are_not_the_battery() {
        assert!(!is_stinger_battery(&h("FF 12 06 FE 0C 04 1F 0B 01 03 05 12 0E")));
        assert!(!is_stinger_battery(&h("FF 11 05 FE 0C 04 1F 0B 01 03 05 12 0E")));
        assert!(!is_stinger_battery(&[0xFF, 0x12, 0x05]));
    }

    fn stinger_reply(code: u8, mv: u16) -> Vec<u8> {
        let mut reply = h("FF 12 05 FE 0C 04 1F 0B 01");
        reply[9] = code;
        reply[11..13].copy_from_slice(&mv.to_le_bytes());
        reply
    }

    #[test]
    fn stinger_charging_has_no_level() {
        assert_eq!(
            stinger_state(&stinger_reply(0x05, 4152)),
            Some(State { detail: Some("d.cableHidden".into()), ..State::new(true, None, true) })
        );
    }

    #[test]
    fn stinger_full_is_a_hundred() {
        assert_eq!(stinger_state(&stinger_reply(0x06, 4130)), Some(State { full: true, ..state(100, false) }));
    }

    #[test]
    fn stinger_unknown_code_or_voltage_is_no_reading() {
        assert_eq!(stinger_state(&stinger_reply(0x00, 3954)), None);
        assert_eq!(stinger_state(&stinger_reply(0xFF, 3954)), None);
        assert_eq!(stinger_state(&stinger_reply(0x03, 0)), None);
        assert_eq!(stinger_state(&stinger_reply(0x03, 0xFFFF)), None);
    }

    #[test]
    fn stinger_curve_points_are_the_tens() {
        for (i, &mv) in STINGER_MV.iter().enumerate() {
            assert_eq!(stinger_percent(mv), i as i32 * 10, "{mv} mV");
        }
    }

    #[test]
    fn stinger_levels_between_the_points() {
        // What owners' receivers showed with this curve: 81% at 3967 mV (03F0:0A8E), 20% at 3702 mV (0951:1713)
        assert_eq!(stinger_percent(3967), 81);
        assert_eq!(stinger_percent(3702), 20);
        assert_eq!(stinger_percent(3584), 5);
        assert_eq!(stinger_percent(4120), 99);
    }

    #[test]
    fn stinger_curve_ends() {
        assert_eq!(stinger_percent(3400), 0);
        assert_eq!(stinger_percent(3516), 0);
        assert_eq!(stinger_percent(4200), 100);
    }

    #[test]
    fn stinger_requests_are_the_two_status_reads() {
        assert_eq!(STINGER_LINK[..9], [0xFF, 0x07, 0x00, 0xFD, 0x04, 0x00, 0x00, 0x02, 0x82]);
        assert_eq!(STINGER_BATTERY[..9], [0xFF, 0x07, 0x00, 0xFD, 0x04, 0x0C, 0xF1, 0x02, 0x01]);
    }

    #[test]
    fn stinger_talks_to_the_feature_collection_only() {
        // MI_03 of the receiver: media keys, the mute switch, and the one with 64-byte reports of every kind
        assert!(is_stinger_collection(&collection(3, 0xFF00, 0x0001, (64, 64, 64))));
        assert!(!is_stinger_collection(&collection(3, 0xFFC0, 0x0001, (2, 0, 0))));
        assert!(!is_stinger_collection(&collection(3, 0x000C, 0x0001, (2, 0, 0))));
        assert!(!is_stinger_collection(&collection(3, 0xFF00, 0x0001, (2, 0, 2))));
    }

    #[test]
    fn a_mouse_on_its_cable_shares_the_key_of_its_receiver() {
        for (cable, receiver) in [(HASTE_2_CABLE, 0x0F98), (HASTE_CABLE, 0x028E)] {
            let row = SPECS.iter().find(|s| s.pids.contains(&cable)).unwrap();
            assert!(row.pids.contains(&receiver) && row.cable_pids == [cable], "{}", row.key);
        }
    }
}
