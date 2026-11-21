//! Razer wireless headsets. None of them speaks the 90-byte report of Razer's mice and keyboards; there are four
//! protocols here, each read-only as used.
//!
//! Report 02 - BlackShark V2 HyperSpeed, Kraken V4, Kraken V4 Pro, Kraken Kitty V3 Pro, BlackShark V3. An output
//! report of 64 bytes on a vendor collection with 64-byte reports (FF14:0001; FF00:0001 on the Kraken V4 Pro's hub),
//! answered by input report 02:
//!   request  02 00 SS 00 00 00 04 00 00 RR CC ..            [62] XOR of bytes 0..61
//!   reply    02 TT SS 00 00 00 LL 00 xx RR CC 01 NN VV ..   [62] XOR of bytes 1..61 (BlackShark V3: of 0..61)
//! SS is 60; the BlackShark V2 HyperSpeed counts requests in its low five bits and the reply echoes it. RR is 80 for
//! the headset behind a receiver, 00 for a headset on its own cable and for the receiver itself. CC: 21 the battery
//! in percent, 2A charging (0 / 1). TT 02 = done (01, 03 busy), NN the number of value bytes, LL 4 + NN. With 02 in
//! place of the 01 the frame is the headset reporting by itself, which is not an answer. A receiver whose headset is
//! off stays silent or answers FF. Commands with the top bit set write settings; none is sent. The BlackShark V3
//! loses its 2.4 GHz link when it is asked every few seconds, so it has a provider of its own that asks every five
//! minutes (see `BlackSharkV3Provider`).
//! Facts: OpenRazer pull requests #2974 (driver/razerblacksharkv2_protocol.h and its validation record), #2794
//! (driver/razerkraken_driver.c) and #2882 (kraken_v4.py); razer-blackshark-v2-hyperspeed-webhid (README);
//! razer-kraken-v4-pro-linux (docs/PROTOCOL.md); HeadsetControl pull request #587 (RAZER_KRAKEN_V4_PROTOCOL.md,
//! frames captured from Synapse 4); HaloBattery (providers/blackshark_v3.py, a Synapse capture of the V3);
//! Wireless-Device-Battery-Monitor (src/drivers/razer_headset.cpp, and the collections its users reported).
//!
//! Feature report 07 - BlackShark V3 X HyperSpeed. 64 bytes on the vendor collection with a 64-byte feature report
//! (FF82:0001 on the receiver), set and then read back:
//!   07 TT II 00 00 00 NN 07 CC AA AA ..   [62] XOR of bytes 3..60
//! II is a number the reply echoes, NN the argument count (2), CC 80 battery / 84 charging, the value comes as the
//! second argument. TT 02 = done; a receiver without its headset answers with FF in every argument.
//! Facts: OpenRazer pull request #2899 (driver/razerblackshark_driver.c), Wireless-Device-Battery-Monitor.
//!
//! Feature report FF - BlackShark V2 Pro (2020). 64 bytes on FF00:0001. `FF 0A 00 FD 04 12 F1 02 05` asks (FD is
//! "get"); a fresh reply starts `FF 0F 05 FE 12 04 1F 08 05`, then [11] the power state, [12..13] the battery
//! voltage in millivolts (big-endian), [14] the level. The receiver keeps the last reply under the header
//! `FF 01 00 ..` while the headset is off.
//! Facts: HaloBattery (providers/blackshark2020.py, with frames read on a receiver), OpenRazer issue #1280,
//! Wireless-Device-Battery-Monitor.
//!
//! Airoha bridge - BlackShark V2 Pro (2023) and Barracuda Pro. The receiver bridges HID to the headset's Airoha
//! chip, as the Barracuda X's does (`barracuda.rs`). A command is `50 41 <type> <sequence> <body>`, wrapped as
//! `02 80 <length> 00 00 ..` (V2 Pro) or `01 80 <length> ..` (Barracuda Pro); replies start `50 49` after
//! `02 <length>` or `01 80 <length>`. Type 08 with the body `03 <id> 00 00` reads a value of the headset: 21 the
//! battery in percent, 2A charging; the reply carries `<id> 01 <count> <value>` after its 10-byte header (02 instead
//! of the 01: reported by itself). The receiver passes these on only in "remote mode" (type 0E, body `02 E1 01`),
//! which Razer Synapse also switches on around every such query and off again (`02 E1 00`); so does this, on every
//! path.
//! Facts: OpenRazer pull request #2862 (driver/razerblackshark_driver.c), HaloBattery (providers/blackshark.py,
//! providers/barracuda.py: a capture of Synapse reading a Barracuda Pro), Wireless-Device-Battery-Monitor
//! (razer_headset.cpp, barracuda_pro.cpp), HeadsetControl (the reply layout, razer_blackshark_v2_pro_ps.hpp).
//!
//! Own implementation. Not yet verified on hardware by this project, which is why the first exchange with each
//! headset goes to the log.

use super::{off, off_because, on, Io, Spec, State};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::providers::{no_link_reason, secs, sleep_ms, Provider};
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RAZER: u16 = 0x1532;

pub static SPECS: &[Spec] = &[
    // ---- Report 02 ----
    Spec {
        key: "razer-blackshark-v2-hyperspeed",
        name: "Razer BlackShark V2 HyperSpeed",
        vid: RAZER,
        pids: &[0x0565, 0x056E],
        cable_pids: &[0x056E],
        accept: Some(is_report_02_collection),
        query: |io| {
            let cable = io.product_id() == 0x056E;
            report_02(io, cable, true, false)
        },
        ..Spec::HEADSET
    },
    // The hub and the headphones on their own cable share a key: while the cable is in, the hub has no link and
    // the reading that reaches the headphones wins (see poll)
    Spec {
        key: "razer-kraken-v4-pro",
        name: "Razer Kraken V4 Pro",
        vid: RAZER,
        pids: &[0x0568, 0x0567],
        cable_pids: &[0x0567],
        accept: Some(is_report_02_collection),
        query: |io| {
            let cable = io.product_id() == 0x0567;
            report_02(io, cable, false, cable)
        },
        ..Spec::HEADSET
    },
    Spec {
        key: "razer-kraken-v4",
        name: "Razer Kraken V4",
        vid: RAZER,
        pids: &[0x056C],
        accept: Some(is_report_02_collection),
        query: |io| report_02(io, false, false, false),
        ..Spec::HEADSET
    },
    Spec {
        key: "razer-kraken-kitty-v3-pro",
        name: "Razer Kraken Kitty V3 Pro",
        vid: RAZER,
        pids: &[0x0588, 0x0587],
        cable_pids: &[0x0587],
        accept: Some(is_report_02_collection),
        query: |io| {
            let cable = io.product_id() == 0x0587;
            report_02(io, cable, false, false)
        },
        ..Spec::HEADSET
    },
    // ---- Feature report 07 ----
    Spec {
        key: "razer-blackshark-v3-x",
        name: "Razer BlackShark V3 X HyperSpeed",
        vid: RAZER,
        pids: &[0x057D, 0x057C],
        cable_pids: &[0x057C],
        accept: Some(|i| i.usage_page >= 0xFF00 && i.feature_length == 64),
        query: blackshark_v3_x,
        ..Spec::HEADSET
    },
    // ---- Feature report FF ----
    Spec {
        key: "razer-blackshark-v2-pro-2020",
        name: "Razer BlackShark V2 Pro (2020)",
        vid: RAZER,
        pids: &[0x0528],
        accept: Some(|i| i.usage_page == 0xFF00 && i.usage == 0x0001 && i.feature_length == 64),
        query: blackshark_v2_pro_2020,
        ..Spec::HEADSET
    },
    // ---- Airoha bridge ----
    Spec {
        key: "razer-blackshark-v2-pro-2023",
        name: "Razer BlackShark V2 Pro (2023)",
        vid: RAZER,
        pids: &[0x0555],
        accept: Some(is_bridge_collection),
        query: blackshark_v2_pro_2023,
        ..Spec::HEADSET
    },
    Spec {
        key: "razer-barracuda-pro",
        name: "Razer Barracuda Pro",
        vid: RAZER,
        pids: &[0x053A],
        accept: Some(is_bridge_collection),
        query: barracuda_pro,
        ..Spec::HEADSET
    },
];

/// A number that differs from call to call and from run to run, for the fields a reply echoes: an old reply that
/// is still around can't pass for the new one.
fn counter() -> u8 {
    static NEXT: OnceLock<AtomicU8> = OnceLock::new();
    let next = NEXT.get_or_init(|| {
        let start = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_micros());
        AtomicU8::new(start as u8)
    });
    next.fetch_add(1, Ordering::Relaxed)
}

fn xor(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |sum, b| sum ^ b)
}

// ---- Report 02 ----

const HEADSET: u8 = 0x80; // through the receiver or the hub
const DIRECT: u8 = 0x00; // the receiver itself, or the headset on its cable
const FIRMWARE: u8 = 0x02;
const BATTERY: u8 = 0x21;
const CHARGING: u8 = 0x2A;
const DONE: u8 = 0x02;
const NOT_READY: u8 = 0xFF;
/// How long an answer is waited for: it comes within 50 ms, OpenRazer and the WebHID tool give it 800.
const REPORT_02_MS: u64 = 800;

/// The collection report 02 lives on. The same interface has FF13:0001 with 62-byte reports, which is not it.
fn is_report_02_collection(i: &HidInfo) -> bool {
    matches!(i.usage_page, 0xFF14 | 0xFF00) && i.usage == 0x0001 && i.input_length == 64 && i.output_length == 64
}

/// A read request: `route` is HEADSET or DIRECT, `sequence` goes into the low five bits of byte 2.
pub fn frame(route: u8, command: u8, sequence: u8) -> Vec<u8> {
    let mut frame = vec![0u8; 64];
    frame[0] = 0x02;
    frame[2] = 0x60 | (sequence & 0x1F);
    frame[6] = 0x04;
    frame[9] = route;
    frame[10] = command;
    frame[62] = xor(&frame[..62]);
    frame
}

/// Whether `d` answers `request`: the sequence, the route and the command come back, byte 11 says "answer" and
/// the checksum holds. The checksum leaves the report ID out, except on the BlackShark V3, which counts it in.
pub fn is_answer(d: &[u8], request: &[u8]) -> bool {
    if d.len() < 64 || d[0] != 0x02 || d[2] != request[2] || d[10] != request[10] || d[11] != 0x01 {
        return false;
    }
    // A BlackShark V2 HyperSpeed on its cable (firmware 1.7) answers as "the headset" to a request that named none
    if d[9] != request[9] && !(request[9] == DIRECT && d[9] == HEADSET) {
        return false;
    }
    let sum = xor(&d[1..62]);
    d[62] == sum || d[62] == sum ^ d[0]
}

/// The first value byte of an answer, when the device says it carried the request out.
pub fn answer_value(d: &[u8]) -> Option<u8> {
    (d[1] == DONE && d[12] >= 1 && d[6].checked_sub(4) == Some(d[12])).then(|| d[13])
}

fn get(io: &mut Io, route: u8, command: u8, counting: bool) -> win::Result<Option<u8>> {
    for _ in 0..2 {
        let request = frame(route, command, if counting { counter() } else { 0 });
        // Volume keys, the mute switch and the headset's own reports arrive on the same collection
        let Some(answer) = io.ask(&request, |d| is_answer(d, &request), 16, false)? else { return Ok(None) };
        match answer[1] {
            0x01 | 0x03 => sleep_ms(50), // busy: once more
            _ => return Ok(answer_value(&answer)),
        }
    }
    Ok(None)
}

/// `level`: the answer to 21. A hub or receiver without its headset answers 255 where it answers at all.
/// `charging`: the answer to 2A, if there was one. `cable_charges`: this is the Kraken V4 Pro on its own cable,
/// which gives no charging state there while the cable charges it.
pub fn report_02_state(level: u8, charging: Option<u8>, cable_charges: bool) -> State {
    if level > 100 {
        return off();
    }
    let charging = match charging {
        Some(c) => c != 0 && c != NOT_READY,
        None => cable_charges && level < 100,
    };
    on(level as i32, charging)
}

fn report_02(io: &mut Io, cable: bool, counting: bool, cable_charges: bool) -> win::Result<State> {
    io.set_timeout(REPORT_02_MS);
    let route = if cable { DIRECT } else { HEADSET };
    let Some(level) = get(io, route, BATTERY, counting)?.filter(|&level| level <= 100) else { return Ok(off()) };
    let charging = get(io, route, CHARGING, counting)?;
    Ok(report_02_state(level, charging, cable_charges))
}

/// The four frames Synapse sends on a connection, in its order. The first two go to the receiver itself (its
/// firmware version, its idea of the charging state) and only wake the channel: without them the receiver doesn't
/// pass the others on. The third is the charging state of the headset, the fourth its battery. All are reads, and
/// no more than these go over the radio.
fn blackshark_v3(io: &mut Io) -> win::Result<State> {
    for command in [FIRMWARE, CHARGING] {
        if !io.send(&frame(DIRECT, command, 0))? {
            return Ok(off());
        }
        sleep_ms(3); // Synapse's pace; a long gap may make the receiver start over
    }
    io.set_timeout(300);
    let charging = get(io, HEADSET, CHARGING, false)?;
    io.set_timeout(600);
    let mut level = get(io, HEADSET, BATTERY, false)?;
    if level == Some(NOT_READY) {
        // The link is still coming up: once more after the pause OpenRazer's driver leaves
        sleep_ms(700);
        level = get(io, HEADSET, BATTERY, false)?;
    }
    let Some(level) = level.filter(|&level| level <= 100) else { return Ok(off()) };
    Ok(report_02_state(level, charging, false))
}

const V3_PID: u16 = 0x057A;
const V3_NAME: &str = "Razer BlackShark V3";

/// The BlackShark V3 on its receiver (1532:057A). The frames are the report 02 ones above, but this headset must
/// not be asked like the others: OpenRazer's daemon asked for the battery every 2 s and the 2.4 GHz link kept
/// dropping, the headset switching itself off and on. Its driver now asks once per connection and the two Windows
/// tools that read it ask every five minutes, so this asks every five minutes too, and a poll that comes sooner
/// ("refresh now") gets the last reading back, with the time it was taken. That is also how long a headset that
/// was switched off can stay listed with its last level. A receiver that didn't answer is asked again after a
/// minute, as Wireless-Device-Battery-Monitor does.
/// The V3 Pro, the Xbox editions and the cable IDs are left out: the sources disagree on their request.
#[derive(Default)]
pub struct BlackSharkV3Provider {
    asked: Option<(Instant, DeviceReading)>,
    logged: HashSet<bool>,
}

impl BlackSharkV3Provider {
    /// How long to leave the headset alone after an answer, or after none.
    fn interval(online: bool) -> Duration {
        secs(if online { 5 * 60 } else { 60 })
    }

    /// What is left of that after the last exchange.
    fn wait(&self) -> Duration {
        self.asked.as_ref().map_or(Duration::ZERO, |(at, r)| Self::interval(r.online).saturating_sub(at.elapsed()))
    }

    fn log_once(&mut self, collections: &[HidInfo], asked: Option<&HidInfo>, trace: &[String], online: bool) {
        if !self.logged.insert(online) {
            return;
        }
        log::write(&format!(
            "{V3_NAME} {RAZER:04x}:{V3_PID:04x}: found {}, {} HID collections",
            if online { "on" } else { "off" },
            collections.len()
        ));
        for info in collections {
            let what = if asked.is_some_and(|a| a.path == info.path) { trace.join("  ") } else { "not asked".into() };
            log::write(&format!("  mi={} {info}: {what}", info.interface_number()));
        }
    }
}

impl Provider for BlackSharkV3Provider {
    fn name(&self) -> &'static str {
        V3_NAME
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let collections = hid::enumerate(|v, p| v == RAZER && p == V3_PID);
        if collections.is_empty() {
            self.asked = None; // receiver unplugged
            return Vec::new();
        }
        if let Some((_, reading)) = self.asked.as_ref().filter(|_| !self.wait().is_zero()) {
            return vec![reading.clone()];
        }

        let collection = collections.iter().find(|i| !i.access_denied && is_report_02_collection(i));
        let mut trace = Vec::new();
        let state = match collection {
            None => off_because(no_link_reason(&collections)),
            Some(info) => {
                let result = HidDevice::open(info).and_then(|dev| blackshark_v3(&mut Io::new(dev, &mut trace)));
                result.unwrap_or_else(|e| {
                    trace.push(format!("error: {e}"));
                    off_because(tf("d.error", &[&e]))
                })
            }
        };
        self.log_once(&collections, collection, &trace, state.online);

        let reading = DeviceReading {
            online: state.online,
            level: state.level,
            charging: state.charging,
            detail: state.detail.as_deref().map(t),
            ..DeviceReading::new("headset:razer-blackshark-v3", V3_NAME, DeviceKind::Headset, t("via.24"))
        };
        self.asked = Some((Instant::now(), reading.clone()));
        vec![reading]
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        match self.asked {
            Some(_) => self.wait().max(secs(1)),
            None => secs(30), // no receiver: look for one as the other providers do
        }
    }
}

// ---- Feature report 07: BlackShark V3 X ----

const V3X_REPORT: u8 = 0x07;
const V3X_POWER: u8 = 0x07; // the command class
const V3X_BATTERY: u8 = 0x80;
const V3X_CHARGING: u8 = 0x84;

/// A read of the power class: two zero arguments, as OpenRazer's driver sends.
pub fn v3x_request(transaction: u8, command: u8) -> Vec<u8> {
    let mut request = vec![0u8; 64];
    request[0] = V3X_REPORT;
    request[2] = transaction;
    request[6] = 0x02;
    request[7] = V3X_POWER;
    request[8] = command;
    request[62] = xor(&request[3..=60]);
    request
}

/// The status of `r` when it is the reply to `request`: the transaction, class and command come back, the checksum
/// holds and there are the two arguments. The value is then `r[10]`. The request itself read back (the firmware
/// hasn't got to it yet) passes too, with the status 00.
pub fn v3x_status(r: &[u8], request: &[u8]) -> Option<u8> {
    let ours = r.len() >= 64
        && r[0] == V3X_REPORT
        && r[2] == request[2]
        && r[7] == request[7]
        && r[8] == request[8]
        && r[6] >= 2
        && r[62] == xor(&r[3..=60]);
    ours.then(|| r[1])
}

fn v3x_get(io: &mut Io, command: u8) -> Option<u8> {
    for _ in 0..4 {
        let request = v3x_request(counter(), command);
        if !io.set_feature(&request) {
            return None;
        }
        sleep_ms(60);
        let reply = io.get_feature(V3X_REPORT)?;
        match v3x_status(&reply, &request) {
            Some(DONE) => return Some(reply[10]),
            // Not ours yet, not started (00) or busy (01): the value isn't there, look again
            None | Some(0x00) | Some(0x01) => sleep_ms(50),
            Some(_) => return None, // failed, timed out, not supported
        }
    }
    None
}

/// A receiver without its headset answers FF; 0 is what it holds before it has heard from one (its serial number
/// reads as zeros then), and no headset that is on reports 0.
pub fn v3x_state(level: Option<u8>, charging: Option<u8>) -> State {
    match level {
        Some(level @ 1..=100) => on(level as i32, charging == Some(1)),
        _ => off(),
    }
}

fn blackshark_v3_x(io: &mut Io) -> win::Result<State> {
    let level = v3x_get(io, V3X_BATTERY);
    if !matches!(level, Some(1..=100)) {
        return Ok(off());
    }
    Ok(v3x_state(level, v3x_get(io, V3X_CHARGING)))
}

// ---- Feature report FF: BlackShark V2 Pro (2020) ----

const V2_PRO_2020_REQUEST: [u8; 9] = [0xFF, 0x0A, 0x00, 0xFD, 0x04, 0x12, 0xF1, 0x02, 0x05];
const V2_PRO_2020_FRESH: [u8; 9] = [0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05];

/// What a feature report FF says, or None when it isn't a fresh reply: the receiver keeps the last one under
/// another header (FF 01 00 ..) for a headset that no longer answers.
/// The level comes in steps (100, 80, 50, 30, 10 were seen), and the moment the charger goes in it jumps to 80
/// whatever it was, so it isn't shown while charging. Power states: 01 on battery, 02 low, 03 very low,
/// 09 charging, 06 charged with the cable still in.
pub fn v2_pro_2020_state(r: &[u8]) -> Option<State> {
    if r.len() < 15 || r[..9] != V2_PRO_2020_FRESH {
        return None;
    }
    if r[12] == 0 && r[13] == 0 {
        return Some(off()); // no voltage: no headset
    }
    let level = (r[14] <= 100).then_some(r[14] as i32);
    let (power, known) = (r[11], level.is_some());
    Some(State {
        detail: (!known && power != 0x09).then(|| "d.levelUnknown".to_string()),
        full: power == 0x06,
        ..State::new(true, level.filter(|_| power != 0x09), power == 0x09)
    })
}

fn blackshark_v2_pro_2020(io: &mut Io) -> win::Result<State> {
    if !io.set_feature(&V2_PRO_2020_REQUEST) {
        return Ok(off());
    }
    // The receiver fetches the answer from the headset: Synapse reads it about 100 ms later
    for _ in 0..5 {
        sleep_ms(100);
        if let Some(state) = io.get_feature(0xFF).and_then(|r| v2_pro_2020_state(&r)) {
            return Ok(state);
        }
    }
    Ok(off())
}

// ---- Airoha bridge: BlackShark V2 Pro (2023), Barracuda Pro ----

const V2_PRO: u8 = 0x02; // report IDs, which also tell the two framings apart
const BARRACUDA_PRO: u8 = 0x01;
const BRIDGE_READ: u8 = 0x08; // a value of the headset
const BRIDGE_LINK: u8 = 0x0E; // the receiver's own settings

/// FF00:0001 on the V2 Pro's receiver, FF00:0000 on the Barracuda Pro's; the other collection is consumer control.
fn is_bridge_collection(i: &HidInfo) -> bool {
    i.usage_page == 0xFF00 && i.input_length == 64 && i.output_length == 64
}

/// A command for the headset's chip in the receiver's wrapping. The V2 Pro's has two checksum bytes in front of
/// the command, which every source for this receiver leaves at zero; the sequence numbers are the ones their
/// captures show.
pub fn bridge_frame(report_id: u8, kind: u8, body: &[u8]) -> Vec<u8> {
    let (at, sequence) = if report_id == V2_PRO { (5, 0x00) } else { (3, 0x08) };
    let mut frame = vec![0u8; 64];
    frame[0] = report_id;
    frame[1] = 0x80;
    frame[2] = 4 + body.len() as u8;
    frame[at..at + 4].copy_from_slice(&[0x50, 0x41, kind, sequence]);
    frame[at + 4..at + 4 + body.len()].copy_from_slice(body);
    frame
}

fn bridge_read(report_id: u8, id: u8) -> Vec<u8> {
    bridge_frame(report_id, BRIDGE_READ, &[0x03, id, 0x00, 0x00])
}

/// The value of the headset's answer to a read of `id`: a `50 49` frame whose payload starts `<id> 01 <count>`.
/// With 02 there the headset is reporting by itself, and other frames carry something else at that place.
pub fn bridge_value(d: &[u8], report_id: u8, id: u8) -> Option<u8> {
    let at = if report_id == V2_PRO { 2 } else { 3 };
    let framed = d.len() > at + 13
        && d[0] == report_id
        && (report_id == V2_PRO || d[1] == 0x80)
        && d[at] == 0x50
        && d[at + 1] == 0x49;
    (framed && d[at + 10] == id && d[at + 11] == 0x01 && d[at + 12] >= 1).then(|| d[at + 13])
}

/// Remote mode on or off; the receiver takes a moment to switch.
fn bridge_remote(io: &mut Io, report_id: u8, on: bool) -> win::Result<bool> {
    let sent = io.send(&bridge_frame(report_id, BRIDGE_LINK, &[0x02, 0xE1, on as u8]))?;
    sleep_ms(if report_id == V2_PRO { 35 } else { 50 });
    Ok(sent)
}

fn bridge_ask(io: &mut Io, report_id: u8, id: u8) -> win::Result<Option<u8>> {
    let answer = io.ask(&bridge_read(report_id, id), |d| bridge_value(d, report_id, id).is_some(), 8, false)?;
    Ok(answer.and_then(|d| bridge_value(&d, report_id, id)))
}

/// 21 answers 0-100; 2A is 0 on battery and something else on the charger (2 was seen on the V2 Pro).
pub fn bridge_state(level: Option<u8>, charging: Option<u8>) -> State {
    match level {
        Some(level @ 0..=100) => on(level as i32, charging.is_some_and(|c| c != 0)),
        _ => off(),
    }
}

/// One value of the V2 Pro (2023), the way Synapse and OpenRazer's driver get it: remote mode on, the read, remote
/// mode off. The link dozes off after about 0.3 s without traffic and the first frame after that only wakes it,
/// so one "remote on" goes ahead and every attempt repeats it; the wait for the reply stays under those 0.3 s.
fn v2_pro_2023_get(io: &mut Io, id: u8) -> win::Result<Option<u8>> {
    let asked = (|| -> win::Result<Option<u8>> {
        bridge_remote(io, V2_PRO, true)?;
        for _ in 0..3 {
            bridge_remote(io, V2_PRO, true)?;
            if let Some(value) = bridge_ask(io, V2_PRO, id)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    })();
    // Back to local mode however that went, an error included
    let restored = bridge_remote(io, V2_PRO, false);
    let value = asked?;
    restored?;
    Ok(value)
}

fn blackshark_v2_pro_2023(io: &mut Io) -> win::Result<State> {
    io.set_timeout(150);
    let level = v2_pro_2023_get(io, BATTERY)?;
    if !matches!(level, Some(0..=100)) {
        return Ok(off());
    }
    Ok(bridge_state(level, v2_pro_2023_get(io, CHARGING)?))
}

/// The Barracuda Pro as Synapse reads it: remote mode on, both reads, remote mode off.
fn barracuda_pro(io: &mut Io) -> win::Result<State> {
    io.set_timeout(300);
    io.drain()?;
    let asked = (|| -> win::Result<(Option<u8>, Option<u8>)> {
        if !bridge_remote(io, BARRACUDA_PRO, true)? {
            return Ok((None, None));
        }
        let mut level = None;
        for _ in 0..3 {
            level = bridge_ask(io, BARRACUDA_PRO, BATTERY)?;
            if level.is_some() {
                break;
            }
        }
        if !matches!(level, Some(0..=100)) {
            return Ok((None, None)); // no headset: the second read would only wait as long again
        }
        Ok((level, bridge_ask(io, BARRACUDA_PRO, CHARGING)?))
    })();
    // Back to local mode however that went, an error included
    let restored = bridge_remote(io, BARRACUDA_PRO, false);
    let (level, charging) = asked?;
    restored?;
    Ok(bridge_state(level, charging))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(text: &str) -> Vec<u8> {
        text.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// A 64-byte report that starts with `head` and has `checksum` at byte 62.
    fn report(head: &str, checksum: u8) -> Vec<u8> {
        let mut r = h(head);
        r.resize(64, 0);
        r[62] = checksum;
        r
    }

    fn state(online: bool, level: Option<i32>, charging: bool) -> State {
        State::new(online, level, charging)
    }

    // ---- Report 02: frames captured from Synapse 4 with a Kraken V4 (HeadsetControl pull request #587) and a
    // BlackShark V3 (HaloBattery, providers/blackshark_v3.py) ----

    const KRAKEN_V4_44: &str = "02 02 60 00 00 00 05 00 80 80 21 01 01 2c";

    #[test]
    fn battery_and_charging_requests_are_the_ones_synapse_sends() {
        assert_eq!(frame(HEADSET, BATTERY, 0), report("02 00 60 00 00 00 04 00 00 80 21", 0xC7));
        assert_eq!(frame(HEADSET, CHARGING, 0), report("02 00 60 00 00 00 04 00 00 80 2a", 0xCC));
    }

    #[test]
    fn a_headset_on_its_cable_is_asked_without_a_route() {
        assert_eq!(frame(DIRECT, BATTERY, 0), report("02 00 60 00 00 00 04 00 00 00 21", 0x47));
    }

    #[test]
    fn blackshark_v3_wake_frames_are_the_captured_ones() {
        assert_eq!(frame(DIRECT, FIRMWARE, 0), report("02 00 60 00 00 00 04 00 00 00 02", 0x64));
        assert_eq!(frame(DIRECT, CHARGING, 0), report("02 00 60 00 00 00 04 00 00 00 2a", 0x4C));
    }

    #[test]
    fn the_sequence_goes_into_five_bits() {
        assert_eq!(frame(HEADSET, BATTERY, 0x25)[2], 0x65);
        assert_eq!(frame(HEADSET, BATTERY, 0x1F)[2], 0x7F);
    }

    #[test]
    fn kraken_v4_at_44_percent() {
        let answer = report(KRAKEN_V4_44, 0x6A);
        assert!(is_answer(&answer, &frame(HEADSET, BATTERY, 0)));
        assert_eq!(answer_value(&answer), Some(44));
    }

    #[test]
    fn kraken_v4_on_and_off_the_charger() {
        let request = frame(HEADSET, CHARGING, 0);
        let on_battery = report("02 02 60 00 00 00 05 00 80 80 2a 01 01 00", 0x4D);
        let charging = report("02 02 60 00 00 00 05 00 80 80 2a 01 01 01", 0x4C);
        assert!(is_answer(&on_battery, &request) && is_answer(&charging, &request));
        assert_eq!(report_02_state(44, answer_value(&on_battery), false), state(true, Some(44), false));
        assert_eq!(report_02_state(45, answer_value(&charging), false), state(true, Some(45), true));
    }

    #[test]
    fn blackshark_v3_counts_the_report_id_into_its_checksum() {
        let answer = report("02 02 60 00 00 00 05 00 00 80 21 01 01 61", 0xA5);
        assert!(is_answer(&answer, &frame(HEADSET, BATTERY, 0)));
        assert_eq!(answer_value(&answer), Some(97));
    }

    #[test]
    fn what_the_headset_reports_by_itself_is_not_the_answer() {
        let pushed = report("02 0a 60 56 00 00 05 00 80 80 2a 02 01 01", 0x11);
        assert!(!is_answer(&pushed, &frame(HEADSET, CHARGING, 0)));
    }

    #[test]
    fn the_answer_to_another_request_is_not_ours() {
        let answer = report(KRAKEN_V4_44, 0x6A);
        assert!(!is_answer(&answer, &frame(HEADSET, CHARGING, 0)));
        assert!(!is_answer(&answer, &frame(HEADSET, BATTERY, 1)));
        assert!(!is_answer(&answer, &frame(DIRECT, FIRMWARE, 0)));
    }

    #[test]
    fn a_headset_on_its_cable_may_answer_as_the_headset() {
        let answer = report(KRAKEN_V4_44, 0x6A);
        assert!(is_answer(&answer, &frame(DIRECT, BATTERY, 0)));
        // Not the other way round: the receiver answering for itself is not the headset
        let own = report("02 02 60 00 00 00 05 00 80 00 21 01 01 2c", 0xEA);
        assert!(is_answer(&own, &frame(DIRECT, BATTERY, 0)));
        assert!(!is_answer(&own, &frame(HEADSET, BATTERY, 0)));
    }

    #[test]
    fn a_wrong_checksum_or_a_short_report_is_not_an_answer() {
        let request = frame(HEADSET, BATTERY, 0);
        assert!(!is_answer(&report(KRAKEN_V4_44, 0x6B), &request));
        let mut changed = report(KRAKEN_V4_44, 0x6A);
        changed[13] = 0x64;
        assert!(!is_answer(&changed, &request));
        assert!(!is_answer(&h(KRAKEN_V4_44), &request));
    }

    #[test]
    fn busy_unsupported_or_empty_carries_no_value() {
        assert_eq!(answer_value(&report("02 01 60 00 00 00 05 00 80 80 21 01 01 2c", 0)), None);
        assert_eq!(answer_value(&report("02 05 60 00 00 00 05 00 80 80 21 01 01 2c", 0)), None);
        assert_eq!(answer_value(&report("02 02 60 00 00 00 04 00 80 80 21 01 00 2c", 0)), None);
        assert_eq!(answer_value(&report("02 02 60 00 00 00 09 00 80 80 21 01 01 2c", 0)), None);
        assert_eq!(answer_value(&report("02 02 60 00 00 00 03 00 80 80 21 01 ff 2c", 0)), None);
    }

    #[test]
    fn a_hub_without_its_headset_is_off_not_255_percent() {
        assert_eq!(report_02_state(255, None, false), off());
        assert_eq!(report_02_state(101, Some(0), false), off());
        assert_eq!(report_02_state(0, Some(0), false), state(true, Some(0), false));
    }

    #[test]
    fn charging_that_is_not_ready_is_not_charging() {
        assert_eq!(report_02_state(80, Some(NOT_READY), false), state(true, Some(80), false));
        assert_eq!(report_02_state(80, None, false), state(true, Some(80), false));
    }

    #[test]
    fn kraken_v4_pro_on_its_cable_charges_without_saying_so() {
        assert_eq!(report_02_state(60, None, true), state(true, Some(60), true));
        assert_eq!(report_02_state(100, None, true), state(true, Some(100), false));
    }

    #[test]
    fn only_the_collection_with_64_byte_reports_is_asked() {
        let collection = |page, length| HidInfo {
            usage_page: page,
            usage: 0x0001,
            input_length: length,
            output_length: length,
            ..HidInfo::default()
        };
        assert!(is_report_02_collection(&collection(0xFF14, 64)));
        assert!(is_report_02_collection(&collection(0xFF00, 64)));
        assert!(!is_report_02_collection(&collection(0xFF13, 62)));
        assert!(!is_report_02_collection(&collection(0x000C, 2)));
    }

    #[test]
    fn blackshark_v3_is_left_alone_for_five_minutes_after_an_answer() {
        assert_eq!(BlackSharkV3Provider::interval(true), secs(300));
        assert_eq!(BlackSharkV3Provider::interval(false), secs(60));
        let reading = DeviceReading { online: true, ..DeviceReading::default() };
        let provider = BlackSharkV3Provider { asked: Some((Instant::now(), reading)), ..Default::default() };
        assert!(provider.wait() > secs(290));
        assert!(provider.delay(&[]) > secs(290));
        assert_eq!(BlackSharkV3Provider::default().wait(), Duration::ZERO);
        assert_eq!(BlackSharkV3Provider::default().delay(&[]), secs(30));
    }

    // ---- BlackShark V3 X ----

    fn v3x_reply(request: &[u8], status: u8, arguments: [u8; 2]) -> Vec<u8> {
        let mut r = request.to_vec();
        r[1] = status;
        r[9..11].copy_from_slice(&arguments);
        r[62] = xor(&r[3..=60]);
        r
    }

    #[test]
    fn v3_x_battery_request() {
        assert_eq!(v3x_request(0x41, V3X_BATTERY), report("07 00 41 00 00 00 02 07 80", 0x85));
        assert_eq!(v3x_request(0x42, V3X_CHARGING)[62], 0x81);
    }

    #[test]
    fn v3_x_level_is_the_second_argument() {
        let request = v3x_request(0x41, V3X_BATTERY);
        let reply = v3x_reply(&request, 0x02, [0x00, 73]);
        assert_eq!(v3x_status(&reply, &request), Some(DONE));
        assert_eq!(v3x_state(Some(reply[10]), Some(1)), state(true, Some(73), true));
        assert_eq!(v3x_state(Some(reply[10]), Some(0)), state(true, Some(73), false));
        assert_eq!(v3x_state(Some(reply[10]), None), state(true, Some(73), false));
    }

    #[test]
    fn v3_x_request_read_back_is_not_done() {
        let request = v3x_request(0x41, V3X_BATTERY);
        assert_eq!(v3x_status(&request, &request), Some(0x00));
    }

    #[test]
    fn v3_x_reply_to_an_earlier_request_is_not_ours() {
        let request = v3x_request(0x41, V3X_BATTERY);
        assert_eq!(v3x_status(&v3x_reply(&v3x_request(0x40, V3X_BATTERY), 0x02, [0, 73]), &request), None);
        assert_eq!(v3x_status(&v3x_reply(&v3x_request(0x41, V3X_CHARGING), 0x02, [0, 1]), &request), None);
    }

    #[test]
    fn v3_x_wrong_checksum_or_missing_arguments_is_no_reply() {
        let request = v3x_request(0x41, V3X_BATTERY);
        let mut reply = v3x_reply(&request, 0x02, [0x00, 73]);
        reply[10] = 99;
        assert_eq!(v3x_status(&reply, &request), None);
        let mut bare = v3x_reply(&request, 0x02, [0x00, 73]);
        bare[6] = 0x01;
        bare[62] = xor(&bare[3..=60]);
        assert_eq!(v3x_status(&bare, &request), None);
    }

    #[test]
    fn v3_x_receiver_without_its_headset_is_off() {
        assert_eq!(v3x_state(Some(0xFF), Some(0xFF)), off());
        assert_eq!(v3x_state(Some(0), Some(0)), off());
        assert_eq!(v3x_state(None, None), off());
    }

    // ---- BlackShark V2 Pro (2020): feature reports read on a receiver (HaloBattery, providers/blackshark2020.py) ----

    fn feature(head: &str) -> Vec<u8> {
        let mut r = h(head);
        r.resize(64, 0);
        r
    }

    #[test]
    fn v2_pro_2020_on_battery() {
        let r = feature("ff 0f 05 fe 12 04 1f 08 05 03 05 01 0e c0 50");
        assert_eq!(v2_pro_2020_state(&r), Some(state(true, Some(80), false)));
    }

    #[test]
    fn v2_pro_2020_low_and_very_low() {
        let low = feature("ff 0f 05 fe 12 04 1f 08 05 03 05 02 0e 08 1e");
        assert_eq!(v2_pro_2020_state(&low), Some(state(true, Some(30), false)));
        let very_low = feature("ff 0f 05 fe 12 04 1f 08 05 03 05 03 0d a0 0a");
        assert_eq!(v2_pro_2020_state(&very_low), Some(state(true, Some(10), false)));
    }

    #[test]
    fn v2_pro_2020_level_is_hidden_while_charging() {
        let r = feature("ff 0f 05 fe 12 04 1f 08 05 05 03 09 10 80 50");
        assert_eq!(v2_pro_2020_state(&r), Some(state(true, None, true)));
    }

    #[test]
    fn v2_pro_2020_charged_with_the_cable_in() {
        let r = feature("ff 0f 05 fe 12 04 1f 08 05 06 05 06 10 88 64");
        assert_eq!(v2_pro_2020_state(&r), Some(State { full: true, ..state(true, Some(100), false) }));
        let unplugged = feature("ff 0f 05 fe 12 04 1f 08 05 03 05 01 10 38 64");
        assert_eq!(v2_pro_2020_state(&unplugged), Some(state(true, Some(100), false)));
    }

    #[test]
    fn v2_pro_2020_reply_kept_for_a_headset_that_is_off_is_not_fresh() {
        assert_eq!(v2_pro_2020_state(&feature("ff 01 00 fe 12 04 1f 08 05 05 03 09 10 88 50")), None);
        // The request read back, and a report cut short
        assert_eq!(v2_pro_2020_state(&feature("ff 0a 00 fd 04 12 f1 02 05")), None);
        assert_eq!(v2_pro_2020_state(&h("ff 0f 05 fe 12 04 1f 08 05 03 05 01 0e c0")), None);
    }

    #[test]
    fn v2_pro_2020_without_a_voltage_is_off() {
        assert_eq!(v2_pro_2020_state(&feature("ff 0f 05 fe 12 04 1f 08 05 03 05 01 00 00 50")), Some(off()));
    }

    #[test]
    fn v2_pro_2020_level_out_of_range_is_not_known() {
        let r = feature("ff 0f 05 fe 12 04 1f 08 05 03 05 01 0e c0 c8");
        assert_eq!(
            v2_pro_2020_state(&r),
            Some(State { detail: Some("d.levelUnknown".into()), ..state(true, None, false) })
        );
    }

    // ---- Airoha bridge: the request bytes of OpenRazer's driver and of HaloBattery's Synapse capture; the reply
    // frames were captured from the BlackShark V2 Pro for PlayStation's receiver (HeadsetControl's tests), which
    // lays them out as the 2023 model's sources describe ----

    #[test]
    fn v2_pro_2023_requests() {
        assert_eq!(
            bridge_frame(V2_PRO, BRIDGE_LINK, &[0x02, 0xE1, 0x01]),
            feature("02 80 07 00 00 50 41 0e 00 02 e1 01")
        );
        assert_eq!(
            bridge_frame(V2_PRO, BRIDGE_LINK, &[0x02, 0xE1, 0x00]),
            feature("02 80 07 00 00 50 41 0e 00 02 e1 00")
        );
        assert_eq!(bridge_read(V2_PRO, BATTERY), feature("02 80 08 00 00 50 41 08 00 03 21"));
        assert_eq!(bridge_read(V2_PRO, CHARGING), feature("02 80 08 00 00 50 41 08 00 03 2a"));
    }

    #[test]
    fn barracuda_pro_requests_are_the_ones_synapse_sends() {
        assert_eq!(
            bridge_frame(BARRACUDA_PRO, BRIDGE_LINK, &[0x02, 0xE1, 0x01]),
            feature("01 80 07 50 41 0e 08 02 e1 01")
        );
        assert_eq!(bridge_read(BARRACUDA_PRO, BATTERY), feature("01 80 08 50 41 08 08 03 21"));
    }

    #[test]
    fn bridge_answers_with_level_and_charging() {
        let level = feature("02 0e 50 49 08 e6 f7 0e 2d 00 04 00 21 01 01 3b");
        let charging = feature("02 0e 50 49 08 e5 30 0a 2d 00 04 00 2a 01 01 01");
        assert_eq!(bridge_value(&level, V2_PRO, BATTERY), Some(59));
        assert_eq!(bridge_value(&charging, V2_PRO, CHARGING), Some(1));
        assert_eq!(bridge_state(Some(59), Some(1)), state(true, Some(59), true));
        assert_eq!(bridge_state(Some(59), Some(0)), state(true, Some(59), false));
        assert_eq!(bridge_state(Some(59), None), state(true, Some(59), false));
    }

    #[test]
    fn bridge_answer_to_another_read_is_not_ours() {
        let level = feature("02 0e 50 49 08 e6 f7 0e 2d 00 04 00 21 01 01 3b");
        assert_eq!(bridge_value(&level, V2_PRO, CHARGING), None);
        assert_eq!(bridge_value(&level, BARRACUDA_PRO, BATTERY), None);
    }

    #[test]
    fn what_the_headset_reports_through_the_bridge_by_itself_is_not_the_answer() {
        let pushed = feature("02 0e 50 49 08 d7 b6 43 1b 00 04 00 2a 02 01 00");
        assert_eq!(bridge_value(&pushed, V2_PRO, CHARGING), None);
    }

    #[test]
    fn reply_to_the_mode_switch_is_not_a_value() {
        // 02 LL 50 49 01 .. [12] type [13] 80 | sequence [14] status
        let switched = feature("02 0d 50 49 01 00 00 00 00 00 03 00 0e 81 00");
        assert_eq!(bridge_value(&switched, V2_PRO, BATTERY), None);
        assert_eq!(bridge_value(&feature("02 80 08 00 00 50 41 08 00 03 21"), V2_PRO, BATTERY), None);
    }

    #[test]
    fn barracuda_pro_answer_sits_one_byte_further() {
        let level = feature("01 80 0e 50 49 08 00 00 00 00 00 04 00 21 01 01 22");
        assert_eq!(bridge_value(&level, BARRACUDA_PRO, BATTERY), Some(34));
        assert_eq!(bridge_value(&level, V2_PRO, BATTERY), None);
    }

    #[test]
    fn bridge_level_over_a_hundred_is_no_reading() {
        assert_eq!(bridge_state(Some(101), Some(0)), off());
        assert_eq!(bridge_state(None, None), off());
    }

    #[test]
    fn counter_moves_on() {
        let first = counter();
        assert_eq!(counter(), first.wrapping_add(1));
    }
}
