//! Wireless mice of enthusiast brands, each with a small protocol of its own on one vendor collection. Every request
//! here is a read the vendor's own tool sends; nothing is written to a mouse.
//!
//! VAXEE / ZYGEN (3057, collection FF05:0001): feature report 0E, 64 bytes. `0E A5 CC 01 01` reads value CC (01 =
//!   read, 01 = one byte of data); 100 ms later the same report reads back as `0E A5 CC .. 01 VV`. CC 0B is the level
//!   in steps of 5% (0-20), CC 10 the charging sign. A receiver whose mouse is away doesn't put the echo there.
//! Ninjutso Sora V2 and Sprime PM1 (Nordic's 1915, FFA0:0001): feature report 05. `05 15 00 00 01` asks for the
//!   status, which reads back as `05 15 ..`: [9] percent, [10] charging, [11] full (PM1), [12] the mouse is linked
//!   (always 0 on the cable).
//! Ninjutso Ten / Ten Air (PixArt's 093A, FF01:0001): feature report 06, 16 bytes. `06 CC 00 00 01` reads value CC,
//!   which reads back as `06 CC .. [8] VV`: 13 linked (1), 12 percent, 11 charging.
//! Endgame Gear OP1w 4k / XM2w 4k and their v2 (3367, FF01:0002): feature report A1, 64 bytes, `A1 CC TT`. The reply
//!   read back has a status in [1] (01 ok, 03 busy: read again, 08 / 11 the mouse can't be reached) and the data from
//!   [16]. 0E is the mouse's own USB IDs, B4 the level. A status of 01 can come with the reply of the command before,
//!   so the level is only taken right after a mouse-info reply that carries Endgame Gear's vendor ID: a level read
//!   from that same buffer would be 67 hex and is refused. After a failed command the dongle answers nothing until a
//!   probe (0F) gets through; the probe is sent only then.
//! Cooler Master MM712 / MM731 (2516, interface 3, FF0A:0001): output `46 81 00`, input `47 81 MM ..` with [10]
//!   percent, [11] charging. The mouse also sends that report by itself when the level changes.
//! BenQ ZOWIE EC-CW (04A5): output `A5 03` on the vendor collection with 64-byte reports, input `LL HH PP EC`: the
//!   battery voltage in mV and the mouse's own percent. A voltage of 0 is a receiver without its mouse.
//! ROCCAT Kone Pro Air (1E7D:2C8E, interface 2, FF00:0001): output `90 0A`, input `90 0A 04 00 00 PP .. [10] PP` when
//!   the mouse is awake, `90 0A 03 ..` with nothing behind it when it sleeps.
//! Finalmouse UltralightX (361D:0100, FF00:0001): output report 04 `04 02 80|CC 00` reads value CC, the answer is
//!   input report 05 `05 LL CC NN <NN bytes>`. 36 link (0 = no mouse), 37 charging, 38 percent and voltage (not
//!   answered by every firmware), 5 voltage in mV, which XPanel turns into a percent with a curve of its own.
//!
//! Where the facts come from. VAXEE: stuffz/mouse-battery-monitor (docs/VAXEE.md), HaloBattery (providers/vaxee.py,
//! docs/protocols.md), Wireless-Device-Battery-Monitor (docs/vaxee.md), the product IDs also from OpenMouse
//! (src/vaxee/index.ts). Sora V2 and PM1: Wireless-Device-Battery-Monitor (src/drivers/ninjutso.cpp, sprime.cpp),
//! which took them from the vendors' web drivers. Ten: DrStopor/ninjutso-battery-tray (docs/hid-protocol.md) and
//! OpenMouse (src/ninjutso/index.ts). Endgame Gear: johanneszab/vole (re/PROTOCOL.md), stuffz/mouse-battery-monitor
//! (docs/ENDGAME.md), Janlhin/Endgame_Gear_Web_Configurator (src/session.ts, src/transport.ts). Cooler Master:
//! syntactic-spice/coolermaster-usb-protocol (docs/PROTOCOL.md), riVFerd/MM731-Controller. ZOWIE:
//! yleshchuk/zowie-battery (README.md, zowie_daemon.py), the Windows collection from Wireless-Device-Battery-Monitor
//! (src/drivers/zowie.cpp). Kone Pro Air: OpenMouse (docs/roccat-kone-pro-air-testing.md, captures/roccat-kone-pro-air)
//! and eruption (roccat_kone_pro_air.rs). Finalmouse: HaloBattery (providers/finalmouse.py),
//! saintordevil/ulx-finalmouse-battery-tray (native_hid.py), OpenMouse (src/finalmouse/index.ts).
//! Own implementation. None of it is verified on hardware by this project yet, which is why the first exchange with
//! each mouse goes to the log.

use super::{off, off_because, on, Io, Spec, State};
use crate::hid::HidInfo;
use crate::loc::tf;
use crate::providers::{pad, sleep_ms};
use crate::win;

// Mice on their cable. The receiver IDs are in the rows.
const VAXEE_CABLE: &[u16] =
    &[0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008, 0x1009, 0x1010, 0x1011, 0x1012, 0x1013, 0x1014, 0x1015];
const SORA_V2_CABLE: &[u16] = &[0xAE11];
const PM1_CABLE: &[u16] = &[0xAC11, 0xAC12, 0xAC13];
const TEN_CABLE: &[u16] = &[0xE020];
const ENDGAME_CABLE: &[u16] = &[0x1972, 0x1984, 0x1968, 0x1982];
const COOLER_MASTER_CABLE: &[u16] = &[0x0169, 0x0167];

pub static SPECS: &[Spec] = &[
    // The receiver doesn't say which mouse is paired, so all of them are one device: a mouse charging on its cable
    // next to its receiver is then one line, not two
    Spec {
        key: "vaxee",
        name: "VAXEE",
        vid: 0x3057,
        pids: &[
            0x0005, 0x1001, 0x1002, 0x2001, 0x2002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008, 0x1009, 0x1010,
            0x1011, 0x1012, 0x1013, 0x1014, 0x1015,
        ],
        cable_pids: VAXEE_CABLE,
        page: 0xFF05,
        usage: 0x0001,
        query: vaxee,
        ..Spec::MOUSE
    },
    // 1915 is Nordic Semiconductor's vendor ID, which many brands ship under: only these products, and only with the
    // collection the status report lives on
    Spec {
        key: "ninjutso-sora-v2",
        name: "Ninjutso Sora V2",
        vid: 0x1915,
        pids: &[0xAE1C, 0xAE11],
        cable_pids: SORA_V2_CABLE,
        accept: Some(status_15_collection),
        shared: true,
        query: |io| status_15(io, false),
        ..Spec::MOUSE
    },
    Spec {
        key: "sprime-pm1",
        name: "Sprime PM1",
        vid: 0x1915,
        pids: &[0xAC1C, 0xAC11, 0xAC12, 0xAC13],
        cable_pids: PM1_CABLE,
        accept: Some(status_15_collection),
        shared: true,
        query: |io| status_15(io, true),
        ..Spec::MOUSE
    },
    // 093A is PixArt's vendor ID, shared the same way
    Spec {
        key: "ninjutso-ten",
        name: "Ninjutso Ten / Ten Air",
        vid: 0x093A,
        pids: &[0xEB01, 0xE020],
        cable_pids: TEN_CABLE,
        accept: Some(ten_collection),
        shared: true,
        query: ninjutso_ten,
        ..Spec::MOUSE
    },
    // One dongle (1970) for four mice; the mouse says which it is. The other IDs are the mice on their cable
    Spec {
        key: "endgame-gear-4k",
        name: "Endgame Gear OP1w 4k / XM2w 4k",
        vid: 0x3367,
        pids: &[0x1970, 0x1972, 0x1984, 0x1968, 0x1982],
        cable_pids: ENDGAME_CABLE,
        page: 0xFF01,
        usage: 0x0002,
        query: endgame_4k,
        ..Spec::MOUSE
    },
    // One dongle (01B7) for both mice. Interface 4 carries the same usage on the dongle and is never used by Cooler
    // Master's software, so the interface is given
    Spec {
        key: "cooler-master-mm712-mm731",
        name: "Cooler Master MM712 / MM731",
        vid: 0x2516,
        pids: &[0x01B7, 0x0169, 0x0167],
        cable_pids: COOLER_MASTER_CABLE,
        page: 0xFF0A,
        usage: 0x0001,
        interface: 3,
        query: cooler_master,
        ..Spec::MOUSE
    },
    Spec {
        key: "zowie-ec-cw",
        name: "ZOWIE EC-CW",
        vid: 0x04A5,
        pids: &[0x8006, 0x8005],
        cable_pids: &[0x8005],
        accept: Some(zowie_cw_collection),
        query: zowie_ec_cw,
        ..Spec::MOUSE
    },
    Spec {
        key: "roccat-kone-pro-air",
        name: "ROCCAT Kone Pro Air",
        vid: 0x1E7D,
        pids: &[0x2C8E],
        page: 0xFF00,
        usage: 0x0001,
        interface: 2,
        query: kone_pro_air,
        ..Spec::MOUSE
    },
    Spec {
        key: "finalmouse-ultralightx",
        name: "Finalmouse UltralightX",
        vid: 0x361D,
        pids: &[0x0100],
        page: 0xFF00,
        usage: 0x0001,
        query: finalmouse_ulx,
        ..Spec::MOUSE
    },
];

/// The mouse is on, but it sent no level, or one that can't be right.
fn level_unknown(charging: bool) -> State {
    State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, charging) }
}

/// The mouse is on its cable, where its level byte follows the charger and not the battery.
fn charging_without_level() -> State {
    State { detail: Some("d.cableHidden".into()), ..State::new(true, None, true) }
}

/// The receiver answered and says its mouse isn't there.
fn asleep() -> State {
    off_because("d.asleep")
}

// ---- VAXEE / ZYGEN ----

const VAXEE_REPORT: u8 = 0x0E;
const VAXEE_HEADER: u8 = 0xA5;
const VAXEE_LEVEL: u8 = 0x0B;
const VAXEE_CHARGING: u8 = 0x10;

fn vaxee(io: &mut Io) -> win::Result<State> {
    let name = Some(vaxee_name(io.product_id()));
    // A mouse that has just woken can let the first exchange pass: the sources try twice, 300 ms apart
    for attempt in 0..2 {
        if attempt > 0 {
            sleep_ms(300);
        }
        // The charging sign is asked for even when the level didn't come: the report then never keeps a level reply
        // that the next poll could take for a new one
        let level = vaxee_get(io, VAXEE_LEVEL);
        let charging = vaxee_get(io, VAXEE_CHARGING);
        if let Some(state) = vaxee_state(level, charging) {
            return Ok(State { name, ..state });
        }
    }
    Ok(State { name, ..off() })
}

fn vaxee_get(io: &mut Io, command: u8) -> Option<u8> {
    if !io.set_feature(&vaxee_request(command)) {
        return None;
    }
    sleep_ms(100); // what VAXEE's own web panel waits before reading the report back
    io.get_feature(VAXEE_REPORT).and_then(|r| vaxee_value(&r, command))
}

/// Reads one value: 01 = read (02 would write), 01 = one byte of data.
pub fn vaxee_request(command: u8) -> Vec<u8> {
    pad(64, &[VAXEE_REPORT, VAXEE_HEADER, command, 0x01, 0x01])
}

/// The value of a reply to `command`, report ID first. The report keeps what was last put there, so a reply that
/// echoes another command is an old one.
pub fn vaxee_value(reply: &[u8], command: u8) -> Option<u8> {
    let fits = reply.len() >= 6
        && reply[0] == VAXEE_REPORT
        && reply[1] == VAXEE_HEADER
        && reply[2] == command
        && reply[4] == 0x01;
    fits.then(|| reply[5])
}

/// Both values have to be there: a level without the charging sign is a mouse that went away in between. The level
/// comes in steps of 5%, 0-20.
pub fn vaxee_state(level: Option<u8>, charging: Option<u8>) -> Option<State> {
    match (level, charging) {
        (Some(steps), Some(charging)) if steps <= 20 => Some(on(steps as i32 * 5, charging != 0)),
        _ => None,
    }
}

/// A mouse on its cable has an ID of its own. A receiver has the same ID whichever mouse is paired to it.
pub fn vaxee_name(pid: u16) -> String {
    let name = match pid {
        0x1003 => "VAXEE XE Wireless",
        0x1004 => "ZYGEN NP-01S Wireless",
        0x1005 => "VAXEE AX Wireless",
        0x1006 => "ZYGEN NP-01 Wireless",
        0x1007 => "VAXEE XE-S Wireless",
        0x1008 => "VAXEE XE-S-L Wireless",
        0x1009 => "VAXEE x NINJUTSO Sora Wireless",
        0x1010 => "VAXEE E1 Wireless",
        0x1011 => "ZYGEN NP-01S V2 Wireless",
        0x1012 => "VAXEE XE V2 Wireless",
        0x1013 => "ZYGEN NP-01S Ergo Wireless",
        0x1014 => "ZYGEN NP-01S V3 Wireless",
        0x1015 => "ZYGEN NP-01 Ergo Wireless",
        _ => return tf("name.brandMouse", &[&"VAXEE"]),
    };
    name.to_string()
}

// ---- Ninjutso Sora V2, Sprime PM1 ----

const STATUS_15_REPORT: u8 = 0x05;
const STATUS_15_COMMAND: u8 = 0x15;

/// The Sora V2 declares 704 bytes of feature report on this collection (its settings are report 04), of which the
/// status takes 32.
fn status_15_collection(info: &HidInfo) -> bool {
    info.usage_page == 0xFFA0 && info.usage == 0x0001 && info.feature_length >= 32
}

/// `has_full_flag`: byte 11 says the battery is full, as the PM1's web driver reads it.
fn status_15(io: &mut Io, has_full_flag: bool) -> win::Result<State> {
    let pid = io.product_id();
    let cable = SORA_V2_CABLE.contains(&pid) || PM1_CABLE.contains(&pid);
    let mut state = off();
    // The first answers after a pause can still say "not linked": the source asks up to four times
    for attempt in 0..4 {
        if attempt > 0 {
            sleep_ms(90);
        }
        if !io.set_feature(&status_15_request()) {
            break;
        }
        sleep_ms(15);
        let Some(read) = io.get_feature(STATUS_15_REPORT).and_then(|r| status_15_state(&r, cable, has_full_flag))
        else {
            continue;
        };
        state = read;
        if state.online {
            break;
        }
    }
    Ok(state)
}

pub fn status_15_request() -> Vec<u8> {
    pad(32, &[STATUS_15_REPORT, STATUS_15_COMMAND, 0x00, 0x00, 0x01])
}

/// What a status reply says, report ID first; None when it isn't one. On the cable the "linked" byte stays 0 and
/// doesn't count. A level of 0 is shown as not known: Ninjutso's web driver is said to show it as 100%, which no
/// source has seen on a full battery, and a mouse that is on can't be empty either.
pub fn status_15_state(reply: &[u8], cable: bool, has_full_flag: bool) -> Option<State> {
    if reply.len() < 13 || reply[0] != STATUS_15_REPORT || reply[1] != STATUS_15_COMMAND || reply[9] > 100 {
        return None;
    }
    if !cable && reply[12] == 0 {
        return Some(asleep());
    }
    let charging = reply[10] != 0;
    Some(if has_full_flag && reply[11] != 0 {
        State { full: charging, ..on(100, false) }
    } else if reply[9] == 0 {
        level_unknown(charging)
    } else {
        on(reply[9] as i32, charging)
    })
}

// ---- Ninjutso Ten / Ten Air ----

const TEN_REPORT: u8 = 0x06;
const TEN_CHARGING: u8 = 0x11;
const TEN_LEVEL: u8 = 0x12;
const TEN_LINKED: u8 = 0x13;

fn ten_collection(info: &HidInfo) -> bool {
    info.usage_page == 0xFF01 && info.usage == 0x0001 && info.feature_length == 16
}

fn ninjutso_ten(io: &mut Io) -> win::Result<State> {
    let Some(linked) = ten_get(io, TEN_LINKED) else { return Ok(off()) };
    if linked != 1 {
        return Ok(asleep());
    }
    let (Some(level), Some(charging)) = (ten_get(io, TEN_LEVEL), ten_get(io, TEN_CHARGING)) else {
        return Ok(off());
    };
    Ok(if level <= 100 { on(level as i32, charging != 0) } else { level_unknown(charging != 0) })
}

fn ten_get(io: &mut Io, command: u8) -> Option<u8> {
    if !io.set_feature(&ten_request(command)) {
        return None;
    }
    for _ in 0..3 {
        sleep_ms(30);
        if let Some(value) = io.get_feature(TEN_REPORT).and_then(|r| ten_value(&r, command)) {
            return Some(value);
        }
    }
    None
}

/// Reads one value; the 01 in byte 4 is what the mouse expects in every request.
pub fn ten_request(command: u8) -> Vec<u8> {
    pad(16, &[TEN_REPORT, command, 0x00, 0x00, 0x01])
}

/// The value of a reply to `command`, report ID first.
pub fn ten_value(reply: &[u8], command: u8) -> Option<u8> {
    (reply.len() >= 9 && reply[0] == TEN_REPORT && reply[1] == command).then(|| reply[8])
}

// ---- Endgame Gear OP1w 4k / XM2w 4k ----

const ENDGAME_REPORT: u8 = 0xA1;
const ENDGAME_MOUSE_INFO: u8 = 0x0E;
const ENDGAME_PROBE: u8 = 0x0F;
const ENDGAME_LEVEL: u8 = 0xB4;
const ENDGAME_TO_MOUSE: u8 = 0x0F;
const ENDGAME_OK: u8 = 0x01;
const ENDGAME_BUSY: u8 = 0x03;
const ENDGAME_DATA: usize = 16;

fn endgame_4k(io: &mut Io) -> win::Result<State> {
    let cable = ENDGAME_CABLE.contains(&io.product_id());
    let mut mouse = endgame_command(io, ENDGAME_MOUSE_INFO, 0x00).as_deref().and_then(endgame_mouse);
    if mouse.is_none() && !cable {
        // The mouse sleeps, or an earlier failure has left the dongle answering nothing: only a probe that gets
        // through clears that. The one addressed to the mouse is answered by the dongle whether the mouse is awake
        // or not, and Endgame Gear's tool sends it before every command; a cabled mouse refuses it
        let cleared = endgame_command(io, ENDGAME_PROBE, ENDGAME_TO_MOUSE).is_some_and(|r| r[1] == ENDGAME_OK);
        if cleared {
            mouse = endgame_command(io, ENDGAME_MOUSE_INFO, 0x00).as_deref().and_then(endgame_mouse);
        }
    }
    let Some(pid) = mouse else { return Ok(if cable { off() } else { asleep() }) };
    let level = endgame_command(io, ENDGAME_LEVEL, 0x00).as_deref().and_then(endgame_level);
    Ok(State { name: endgame_name(pid).map(str::to_string), ..endgame_state(level, cable) })
}

/// Sends a command and reads its reply, waiting while the dongle says it is busy the way the vendor's tool does.
fn endgame_command(io: &mut Io, command: u8, target: u8) -> Option<Vec<u8>> {
    if !io.set_feature(&endgame_request(command, target)) {
        return None;
    }
    for wait in [150, 200, 400, 600] {
        sleep_ms(wait);
        let reply = io.get_feature(ENDGAME_REPORT)?;
        if reply.len() < ENDGAME_DATA + 4 || reply[0] != ENDGAME_REPORT {
            return None;
        }
        if reply[1] != ENDGAME_BUSY {
            return Some(reply);
        }
    }
    None
}

/// `A1 command target`, no data: every command sent here is a read.
pub fn endgame_request(command: u8, target: u8) -> Vec<u8> {
    pad(64, &[ENDGAME_REPORT, command, target])
}

/// The product ID of the mouse from a mouse-info reply. The reply doesn't echo the command, and "ok" can come with
/// what the command before left in the buffer: only Endgame Gear's vendor ID in front makes it this one.
pub fn endgame_mouse(reply: &[u8]) -> Option<u16> {
    let d = reply.get(ENDGAME_DATA..ENDGAME_DATA + 4)?;
    (reply[1] == ENDGAME_OK && d[0] == 0x67 && d[1] == 0x33).then(|| u16::from_le_bytes([d[2], d[3]]))
}

/// The level byte of a battery reply. Asked right after mouse info, a reply that is still that one has 67 hex here.
pub fn endgame_level(reply: &[u8]) -> Option<u8> {
    let level = *reply.get(ENDGAME_DATA)?;
    (reply[1] == ENDGAME_OK && level <= 100).then_some(level)
}

/// Through the dongle the byte is the percent. On the cable it follows the charger (a mouse at 95% read 55 a minute
/// after it was plugged in), and the vendor's tool shows "Charging" until it reads 100.
pub fn endgame_state(level: Option<u8>, cable: bool) -> State {
    match (level, cable) {
        (Some(100), true) => State { full: true, ..on(100, false) },
        (_, true) => charging_without_level(),
        (Some(level), false) => on(level as i32, false),
        (None, false) => asleep(),
    }
}

pub fn endgame_name(pid: u16) -> Option<&'static str> {
    match pid {
        0x1972 => Some("Endgame Gear OP1w 4k"),
        0x1984 => Some("Endgame Gear OP1w 4k v2"),
        0x1968 => Some("Endgame Gear XM2w 4k"),
        0x1982 => Some("Endgame Gear XM2w 4k v2"),
        _ => None,
    }
}

// ---- Cooler Master MM712 / MM731 ----

fn cooler_master(io: &mut Io) -> win::Result<State> {
    let pid = io.product_id();
    // A sleeping mouse doesn't answer through the dongle; an awake one does at once
    io.set_timeout(600);
    let status = io.ask(&[0x00, 0x46, 0x81, 0x00], is_cooler_master_status, 8, false)?;
    Ok(status.map_or_else(off, |r| State {
        name: cooler_master_name(pid, r[2]).map(str::to_string),
        ..cooler_master_state(&r)
    }))
}

/// A status report, asked for or sent by the mouse itself: both are current.
pub fn is_cooler_master_status(d: &[u8]) -> bool {
    d.len() >= 13 && d[0] == 0x47 && d[1] == 0x81
}

pub fn cooler_master_state(d: &[u8]) -> State {
    let charging = d[11] != 0;
    if (1..=100).contains(&d[10]) {
        on(d[10] as i32, charging)
    } else {
        level_unknown(charging)
    }
}

/// Byte 2 of the status report is 50 on an MM712 and 70 on an MM731, which is all that tells them apart on the
/// dongle they share.
pub fn cooler_master_name(pid: u16, model: u8) -> Option<&'static str> {
    match (pid, model) {
        (0x0169, _) | (0x01B7, 0x50) => Some("Cooler Master MM712"),
        (0x0167, _) | (0x01B7, 0x70) => Some("Cooler Master MM731"),
        _ => None,
    }
}

// ---- BenQ ZOWIE EC-CW ----

/// The collection with 64-byte reports and no report IDs, which Windows counts as 65. The Enhanced Receiver declares
/// it on page 00FF.
fn zowie_cw_collection(info: &HidInfo) -> bool {
    (info.usage_page >= 0xFF00 || info.usage_page == 0x00FF) && info.output_length == 65 && info.input_length >= 5
}

fn zowie_ec_cw(io: &mut Io) -> win::Result<State> {
    io.set_timeout(1000);
    // A5 03 and nothing else: other A5 commands restart the receiver and unpair the mouse
    let reply = io.ask(&[0x00, 0xA5, 0x03], is_zowie_cw_reply, 4, false)?;
    Ok(reply.map_or_else(off, |r| zowie_cw_state(&r)))
}

fn zowie_cw_millivolts(d: &[u8]) -> u16 {
    u16::from_le_bytes([d[0], d[1]])
}

/// The reply doesn't echo the request. What marks it is the EC behind the three bytes and a voltage a lithium cell
/// can have, or none at all.
pub fn is_zowie_cw_reply(d: &[u8]) -> bool {
    d.len() >= 4 && d[3] == 0xEC && (zowie_cw_millivolts(d) == 0 || (3000..=4500).contains(&zowie_cw_millivolts(d)))
}

pub fn zowie_cw_state(d: &[u8]) -> State {
    if zowie_cw_millivolts(d) == 0 {
        return asleep();
    }
    if (1..=100).contains(&d[2]) {
        on(d[2] as i32, false)
    } else {
        level_unknown(false)
    }
}

// ---- ROCCAT Kone Pro Air ----

fn kone_pro_air(io: &mut Io) -> win::Result<State> {
    io.set_timeout(500);
    let reply = io.ask(&[0x00, 0x90, 0x0A], is_kone_status, 8, false)?;
    Ok(reply.map_or_else(off, |r| kone_state(&r)))
}

pub fn is_kone_status(d: &[u8]) -> bool {
    d.len() >= 11 && d[0] == 0x90 && d[1] == 0x0A
}

/// Byte 2 is 04 when the mouse answered and 03, with nothing after it, when it sleeps. The two sources read the
/// level from different bytes: byte 5 followed what ROCCAT's own software showed, another project reads byte 10.
/// They are equal in one captured session and not in another, so a level counts only when they agree.
pub fn kone_state(d: &[u8]) -> State {
    if d[2] != 0x04 {
        return asleep();
    }
    if d[5] == d[10] && (1..=100).contains(&d[5]) {
        on(d[5] as i32, false)
    } else {
        level_unknown(false)
    }
}

// ---- Finalmouse UltralightX ----

const FINALMOUSE_OUT: u8 = 0x04;
const FINALMOUSE_IN: u8 = 0x05;
const FINALMOUSE_VOLTAGE: u8 = 5;
const FINALMOUSE_LINK: u8 = 36;
const FINALMOUSE_CHARGING: u8 = 37;
const FINALMOUSE_CHARGE: u8 = 38;

fn finalmouse_ulx(io: &mut Io) -> win::Result<State> {
    io.set_timeout(350);
    match finalmouse_get(io, FINALMOUSE_LINK, 1)? {
        Some(link) if link[0] != 0 => {}
        Some(_) => return Ok(asleep()),
        None => return Ok(off()),
    }
    let charging = finalmouse_get(io, FINALMOUSE_CHARGING, 1)?.is_some_and(|c| c[0] == 1);
    // The mouse's own percent, which the UltralightX firmware the sources tested doesn't answer
    let percent = finalmouse_get(io, FINALMOUSE_CHARGE, 3)?.map(|c| c[0]).filter(|&p| p <= 100);
    let millivolts = if percent.is_none() && !charging {
        finalmouse_get(io, FINALMOUSE_VOLTAGE, 2)?.map(|v| u16::from_le_bytes([v[0], v[1]]))
    } else {
        None
    };
    Ok(finalmouse_state(percent, charging, millivolts))
}

/// The data of the answer to a read of `command`, at least `least` bytes of it.
fn finalmouse_get(io: &mut Io, command: u8, least: usize) -> win::Result<Option<Vec<u8>>> {
    let data = |d: &[u8]| finalmouse_data(d, command).filter(|p| p.len() >= least).map(<[u8]>::to_vec);
    Ok(io.ask(&finalmouse_request(command), |d| data(d).is_some(), 16, false)?.and_then(|r| data(&r)))
}

/// A request without data is a read: length of what follows, the command with its high bit set, length of the data.
pub fn finalmouse_request(command: u8) -> Vec<u8> {
    pad(64, &[FINALMOUSE_OUT, 0x02, 0x80 | command, 0x00])
}

/// The data of an input report that answers `command`; the dongle streams other values on the same report.
pub fn finalmouse_data(d: &[u8], command: u8) -> Option<&[u8]> {
    if d.len() < 4 || d[0] != FINALMOUSE_IN || d[2] & 0x7F != command {
        return None;
    }
    d.get(4..4 + d[3] as usize)
}

/// The percent XPanel shows for a battery voltage: straight lines between its seven points, rounded.
pub fn finalmouse_percent(millivolts: u16) -> Option<i32> {
    const CURVE: [(f64, f64); 7] =
        [(3000.0, 0.2), (3620.0, 5.0), (3660.0, 10.0), (3740.0, 25.0), (3880.0, 50.0), (4170.0, 75.0), (4380.0, 100.0)];
    if millivolts == 0 || millivolts >= 5000 {
        return None;
    }
    let mv = millivolts as f64;
    if mv < CURVE[0].0 {
        return Some(0);
    }
    for pair in CURVE.windows(2) {
        let ((low, from), (high, to)) = (pair[0], pair[1]);
        if mv <= high {
            return Some((from + (to - from) * (mv - low) / (high - low) + 0.5).floor() as i32);
        }
    }
    Some(100)
}

/// `percent`: the mouse's own, when its firmware has it. Without it the level is worked out from the voltage, which
/// says nothing while the charger holds it up.
pub fn finalmouse_state(percent: Option<u8>, charging: bool, millivolts: Option<u16>) -> State {
    if let Some(percent) = percent {
        return on(percent as i32, charging);
    }
    if charging {
        return charging_without_level();
    }
    match millivolts.and_then(finalmouse_percent) {
        Some(level) => State { detail: Some("d.estimate".into()), ..on(level, false) },
        None => level_unknown(false),
    }
}

#[cfg(test)]
mod tests {
    //! Replies as the sources publish them: the Ninjutso Ten's from ninjutso-battery-tray (docs/hid-protocol.md), the
    //! Endgame Gear mouse-info and battery data from vole (re/PROTOCOL.md), the Cooler Master status report from
    //! coolermaster-usb-protocol (docs/PROTOCOL.md), the ZOWIE replies from zowie-battery (test_zowie_daemon.py), the
    //! Kone Pro Air's from OpenMouse (captures/roccat-kone-pro-air). The others are put together from the layouts
    //! the sources describe.
    use super::*;

    fn h(text: &str) -> Vec<u8> {
        text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    fn collection(page: u16, usage: u16, input: usize, output: usize, feature: usize) -> HidInfo {
        HidInfo {
            usage_page: page,
            usage,
            input_length: input,
            output_length: output,
            feature_length: feature,
            ..HidInfo::default()
        }
    }

    fn unknown(charging: bool) -> State {
        State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, charging) }
    }

    fn sleeping() -> State {
        State { detail: Some("d.asleep".into()), ..State::new(false, None, false) }
    }

    fn on_cable() -> State {
        State { detail: Some("d.cableHidden".into()), ..State::new(true, None, true) }
    }

    #[test]
    fn cable_ids_of_a_row_are_the_ones_its_query_takes_for_cable() {
        let cable = |key: &str| SPECS.iter().find(|s| s.key == key).unwrap().cable_pids;
        assert_eq!(cable("vaxee"), VAXEE_CABLE);
        assert_eq!(cable("ninjutso-sora-v2"), SORA_V2_CABLE);
        assert_eq!(cable("sprime-pm1"), PM1_CABLE);
        assert_eq!(cable("endgame-gear-4k"), ENDGAME_CABLE);
        for spec in SPECS {
            assert!(spec.cable_pids.iter().all(|p| spec.pids.contains(p)), "{}", spec.key);
        }
    }

    // ---- VAXEE ----

    #[test]
    fn vaxee_requests_are_reads_of_one_byte() {
        assert_eq!(vaxee_request(0x0B), pad(64, &h("0E A5 0B 01 01")));
        assert_eq!(vaxee_request(0x10), pad(64, &h("0E A5 10 01 01")));
        assert_eq!(vaxee_request(0x0B).len(), 64);
    }

    #[test]
    fn vaxee_level_comes_in_steps_of_five_percent() {
        let level = vaxee_value(&pad(64, &h("0E A5 0B 01 01 0E")), 0x0B);
        let charging = vaxee_value(&pad(64, &h("0E A5 10 01 01 00")), 0x10);
        assert_eq!((level, charging), (Some(14), Some(0)));
        assert_eq!(vaxee_state(level, charging), Some(State::new(true, Some(70), false)));
        assert_eq!(vaxee_state(Some(20), Some(1)), Some(State::new(true, Some(100), true)));
        assert_eq!(vaxee_state(Some(0), Some(0)), Some(State::new(true, Some(0), false)));
    }

    #[test]
    fn vaxee_reply_to_another_command_is_an_old_one() {
        let charging_reply = pad(64, &h("0E A5 10 01 01 01"));
        assert_eq!(vaxee_value(&charging_reply, 0x0B), None);
        assert_eq!(vaxee_value(&charging_reply, 0x10), Some(1));
    }

    #[test]
    fn vaxee_report_without_the_echo_is_no_reply() {
        assert_eq!(vaxee_value(&pad(64, &[0x0E]), 0x0B), None); // the receiver put nothing there
        assert_eq!(vaxee_value(&pad(64, &h("0E 00 0B 01 01 0E")), 0x0B), None); // no header
        assert_eq!(vaxee_value(&pad(64, &h("0E A5 0B 01 02 0E")), 0x0B), None); // not one byte of data
        assert_eq!(vaxee_value(&h("0E A5 0B 01 01"), 0x0B), None); // cut short
    }

    #[test]
    fn vaxee_needs_both_values_and_a_level_in_range() {
        assert_eq!(vaxee_state(Some(14), None), None);
        assert_eq!(vaxee_state(None, Some(1)), None);
        assert_eq!(vaxee_state(Some(21), Some(0)), None);
        assert_eq!(vaxee_state(Some(0x5F), Some(0)), None);
    }

    #[test]
    fn vaxee_mouse_on_its_cable_has_its_own_name_and_a_receiver_has_none() {
        assert_eq!(vaxee_name(0x1011), "ZYGEN NP-01S V2 Wireless");
        assert_eq!(vaxee_name(0x1010), "VAXEE E1 Wireless");
        let plain = tf("name.brandMouse", &[&"VAXEE"]);
        for pid in VAXEE_CABLE {
            assert_ne!(vaxee_name(*pid), plain, "{pid:04X} has no name of its own");
        }
        for pid in [0x0005, 0x1001, 0x1002, 0x2001, 0x2002] {
            assert_eq!(vaxee_name(pid), plain);
        }
    }

    // ---- Sora V2, PM1 ----

    /// A status reply: level, charging, full, linked.
    fn status_15_reply(level: u8, charging: u8, full: u8, linked: u8) -> Vec<u8> {
        let mut r = pad(32, &[0x05, 0x15]);
        (r[9], r[10], r[11], r[12]) = (level, charging, full, linked);
        r
    }

    #[test]
    fn status_15_request_is_the_one_read_the_web_drivers_send() {
        assert_eq!(status_15_request(), pad(32, &h("05 15 00 00 01")));
    }

    #[test]
    fn status_15_collection_is_told_by_its_feature_report() {
        assert!(status_15_collection(&collection(0xFFA0, 0x0001, 0, 0, 704))); // Sora V2
        assert!(status_15_collection(&collection(0xFFA0, 0x0001, 0, 0, 32)));
        assert!(!status_15_collection(&collection(0xFFA0, 0x0001, 0, 0, 16)));
        assert!(!status_15_collection(&collection(0xFF00, 0x0001, 64, 64, 64)));
    }

    #[test]
    fn sora_v2_through_its_receiver() {
        assert_eq!(
            status_15_state(&status_15_reply(32, 0, 0, 1), false, false),
            Some(State::new(true, Some(32), false))
        );
        assert_eq!(
            status_15_state(&status_15_reply(87, 1, 0, 1), false, false),
            Some(State::new(true, Some(87), true))
        );
    }

    #[test]
    fn receiver_without_its_mouse_is_off_whatever_level_it_still_holds() {
        assert_eq!(status_15_state(&status_15_reply(32, 0, 0, 0), false, false), Some(sleeping()));
        assert_eq!(status_15_state(&status_15_reply(0, 0, 0, 0), false, true), Some(sleeping()));
    }

    #[test]
    fn on_the_cable_the_linked_byte_does_not_count() {
        assert_eq!(status_15_state(&status_15_reply(64, 1, 0, 0), true, false), Some(State::new(true, Some(64), true)));
    }

    #[test]
    fn level_of_zero_is_not_shown_as_a_level() {
        assert_eq!(status_15_state(&status_15_reply(0, 0, 0, 1), false, false), Some(unknown(false)));
        assert_eq!(status_15_state(&status_15_reply(0, 1, 0, 0), true, false), Some(unknown(true)));
    }

    #[test]
    fn pm1_says_when_it_is_full() {
        let full = State { full: true, ..State::new(true, Some(100), false) };
        assert_eq!(status_15_state(&status_15_reply(97, 1, 1, 0), true, true), Some(full));
        // Off the charger there is nothing to unplug
        assert_eq!(
            status_15_state(&status_15_reply(99, 0, 1, 1), false, true),
            Some(State::new(true, Some(100), false))
        );
        // The Sora V2's byte 11 isn't known to mean that
        assert_eq!(
            status_15_state(&status_15_reply(97, 1, 1, 1), false, false),
            Some(State::new(true, Some(97), true))
        );
    }

    #[test]
    fn other_reports_are_not_the_status() {
        assert_eq!(status_15_state(&pad(32, &[0x05, 0x0D]), false, false), None); // reply to another command
        assert_eq!(status_15_state(&pad(32, &[0x04, 0x15]), false, false), None);
        assert_eq!(status_15_state(&status_15_reply(101, 0, 0, 1), false, false), None);
        let cut_short = [0x05, 0x15, 0, 0, 0, 0, 0, 0, 0, 50, 0, 0];
        assert_eq!(status_15_state(&cut_short, false, false), None);
    }

    // ---- Ten ----

    #[test]
    fn ten_requests() {
        assert_eq!(ten_request(0x13), h("06 13 00 00 01 00 00 00 00 00 00 00 00 00 00 00"));
        assert_eq!(ten_request(0x12), h("06 12 00 00 01 00 00 00 00 00 00 00 00 00 00 00"));
        assert_eq!(ten_request(0x11), h("06 11 00 00 01 00 00 00 00 00 00 00 00 00 00 00"));
    }

    #[test]
    fn ten_at_93_percent_on_its_receiver() {
        assert_eq!(ten_value(&h("06 13 00 00 00 00 01 00 01 00 00 00 00 00 00 00"), 0x13), Some(1));
        assert_eq!(ten_value(&h("06 12 00 00 00 00 01 00 5D 00 00 00 00 00 00 00"), 0x12), Some(93));
        assert_eq!(ten_value(&h("06 11 00 00 00 00 01 00 00 00 00 00 00 00 00 00"), 0x11), Some(0));
    }

    #[test]
    fn ten_reply_to_another_command_is_not_the_answer() {
        let level = h("06 12 00 00 00 00 01 00 5D 00 00 00 00 00 00 00");
        assert_eq!(ten_value(&level, 0x13), None);
        assert_eq!(ten_value(&level, 0x11), None);
        assert_eq!(ten_value(&h("06 12 00 00 00 00 01 00"), 0x12), None); // without the value
        assert_eq!(ten_value(&h("05 12 00 00 00 00 01 00 5D"), 0x12), None);
    }

    #[test]
    fn ten_collection_is_the_one_with_the_16_byte_feature_report() {
        assert!(ten_collection(&collection(0xFF01, 0x0001, 0, 0, 16)));
        assert!(!ten_collection(&collection(0xFF01, 0x0001, 0, 0, 64)));
        assert!(!ten_collection(&collection(0xFF01, 0x0002, 0, 0, 16)));
    }

    // ---- Endgame Gear ----

    /// A reply with this status and data, as the dongle fills report A1.
    fn endgame_reply(status: u8, data: &[u8]) -> Vec<u8> {
        let mut r = pad(64, &[0xA1, status]);
        r[16..16 + data.len()].copy_from_slice(data);
        r
    }

    #[test]
    fn endgame_requests_carry_no_data() {
        assert_eq!(endgame_request(0x0E, 0x00), pad(64, &h("A1 0E")));
        assert_eq!(endgame_request(0xB4, 0x00), pad(64, &h("A1 B4")));
        assert_eq!(endgame_request(0x0F, 0x0F), pad(64, &h("A1 0F 0F")));
    }

    #[test]
    fn mouse_info_names_the_mouse_behind_the_dongle() {
        let v1 = endgame_reply(0x01, &h("67 33 72 19 71 19 01 08"));
        let v2 = endgame_reply(0x01, &h("67 33 84 19 83 19 01 07"));
        assert_eq!(endgame_mouse(&v1), Some(0x1972));
        assert_eq!(endgame_mouse(&v2), Some(0x1984));
        assert_eq!(endgame_name(0x1972), Some("Endgame Gear OP1w 4k"));
        assert_eq!(endgame_name(0x1984), Some("Endgame Gear OP1w 4k v2"));
        assert_eq!(endgame_name(0x1970), None);
    }

    #[test]
    fn sleeping_mouse_gives_no_mouse_info() {
        // 11 from mouse info, 08 from the battery; the data is what the last good reply left
        assert_eq!(endgame_mouse(&endgame_reply(0x11, &h("67 33 84 19"))), None);
        assert_eq!(endgame_level(&endgame_reply(0x08, &h("41 B4 0F"))), None);
        assert_eq!(endgame_mouse(&endgame_reply(0x03, &h("67 33 84 19"))), None);
    }

    #[test]
    fn ok_with_another_reply_in_the_buffer_is_not_mouse_info() {
        // The battery reply of the poll before: 65%, 4020 mV
        assert_eq!(endgame_mouse(&endgame_reply(0x01, &h("41 B4 0F 19"))), None);
    }

    #[test]
    fn level_through_the_dongle_is_the_percent() {
        let reply = endgame_reply(0x01, &h("41 B4 0F"));
        assert_eq!(endgame_level(&reply), Some(65));
        assert_eq!(endgame_state(Some(65), false), State::new(true, Some(65), false));
    }

    #[test]
    fn mouse_info_still_in_the_buffer_is_not_a_level() {
        assert_eq!(endgame_level(&endgame_reply(0x01, &h("67 33 84 19 83 19 01 07"))), None);
        assert_eq!(endgame_state(None, false), sleeping());
    }

    #[test]
    fn on_the_cable_only_a_full_battery_has_a_level() {
        assert_eq!(endgame_state(Some(55), true), on_cable());
        assert_eq!(endgame_state(None, true), on_cable());
        assert_eq!(endgame_state(Some(100), true), State { full: true, ..State::new(true, Some(100), false) });
    }

    // ---- Cooler Master ----

    #[test]
    fn mm712_through_the_dongle_at_25_percent() {
        let r = pad(32, &h("47 81 50 03 00 00 F2 9B 1E 00 19 00 01 00 02 FF"));
        assert!(is_cooler_master_status(&r));
        assert_eq!(cooler_master_state(&r), State::new(true, Some(25), false));
        assert_eq!(cooler_master_name(0x01B7, r[2]), Some("Cooler Master MM712"));
    }

    #[test]
    fn mm712_charging_on_its_cable() {
        let r = pad(64, &h("47 81 50 03 00 00 F2 9B 1E 00 2A 01 00 00 02 FF"));
        assert_eq!(cooler_master_state(&r), State::new(true, Some(42), true));
        assert_eq!(cooler_master_name(0x0169, r[2]), Some("Cooler Master MM712"));
    }

    #[test]
    fn the_shared_dongle_tells_the_mice_apart_by_the_status_report() {
        assert_eq!(cooler_master_name(0x01B7, 0x70), Some("Cooler Master MM731"));
        assert_eq!(cooler_master_name(0x0167, 0x70), Some("Cooler Master MM731"));
        assert_eq!(cooler_master_name(0x01B7, 0x51), None);
    }

    #[test]
    fn replies_to_other_commands_are_not_the_status() {
        assert!(!is_cooler_master_status(&pad(32, &h("45 81 02 01")))); // active profile
        assert!(!is_cooler_master_status(&pad(32, &h("59 81 01 01 CD 55 0F")))); // device info
        assert!(!is_cooler_master_status(&h("47 81 50 03 00 00 F2 9B 1E 00 19 00")));
    }

    #[test]
    fn level_outside_1_to_100_is_not_shown() {
        let empty = pad(32, &h("47 81 50 03 00 00 F2 9B 1E 00 00 01 01"));
        assert_eq!(cooler_master_state(&empty), unknown(true));
        let over = pad(32, &h("47 81 50 03 00 00 F2 9B 1E 00 65 00 01"));
        assert_eq!(cooler_master_state(&over), unknown(false));
    }

    // ---- ZOWIE EC-CW ----

    #[test]
    fn ec_cw_at_85_percent() {
        let r = pad(64, &h("D0 0F 55 EC"));
        assert!(is_zowie_cw_reply(&r));
        assert_eq!(zowie_cw_millivolts(&r), 4048);
        assert_eq!(zowie_cw_state(&r), State::new(true, Some(85), false));
    }

    #[test]
    fn ec_cw_receiver_without_its_mouse() {
        let r = pad(64, &h("00 00 00 EC"));
        assert!(is_zowie_cw_reply(&r));
        assert_eq!(zowie_cw_state(&r), sleeping());
    }

    #[test]
    fn reports_that_are_not_the_battery_reply() {
        assert!(!is_zowie_cw_reply(&pad(64, &h("D0 0F 55 00")))); // no marker
        assert!(!is_zowie_cw_reply(&pad(64, &h("A5 03 00 EC")))); // 933 mV
        assert!(!is_zowie_cw_reply(&pad(64, &h("FF FF 55 EC"))));
        assert!(!is_zowie_cw_reply(&h("D0 0F 55")));
    }

    #[test]
    fn ec_cw_percent_out_of_range_is_not_shown() {
        assert_eq!(zowie_cw_state(&pad(64, &h("D0 0F 00 EC"))), unknown(false));
        assert_eq!(zowie_cw_state(&pad(64, &h("D0 0F C8 EC"))), unknown(false));
    }

    #[test]
    fn ec_cw_collection_is_the_vendor_one_with_64_byte_reports() {
        assert!(zowie_cw_collection(&collection(0xFF00, 0x0001, 65, 65, 0)));
        assert!(zowie_cw_collection(&collection(0x00FF, 0x0001, 65, 65, 0))); // Enhanced Receiver
        assert!(!zowie_cw_collection(&collection(0x0001, 0x0002, 8, 0, 0)));
        assert!(!zowie_cw_collection(&collection(0xFF03, 0x0001, 0, 16, 0))); // the DW series' command collection
    }

    // ---- Kone Pro Air ----

    #[test]
    fn kone_pro_air_awake_with_both_level_bytes_agreeing() {
        let r = pad(64, &h("90 0A 04 00 00 1E 00 00 A0 0A 1E"));
        assert!(is_kone_status(&r));
        assert_eq!(kone_state(&r), State::new(true, Some(30), false));
    }

    #[test]
    fn kone_pro_air_level_bytes_that_disagree_give_no_level() {
        let r = pad(64, &h("90 0A 04 00 00 14 01 00 44 0B 1C"));
        assert_eq!(kone_state(&r), unknown(false));
    }

    #[test]
    fn kone_pro_air_asleep_is_not_zero_percent() {
        let r = pad(64, &h("90 0A 03 00 EC FA 0D 00"));
        assert!(is_kone_status(&r));
        assert_eq!(kone_state(&r), sleeping());
    }

    #[test]
    fn kone_pro_air_settings_echo_is_not_the_status() {
        assert!(!is_kone_status(&pad(64, &h("10 50 03 00"))));
        assert!(!is_kone_status(&pad(64, &h("90 70 00 00 EC"))));
        assert!(!is_kone_status(&h("90 0A 04 00 00 1E")));
    }

    // ---- Finalmouse ----

    #[test]
    fn finalmouse_request_is_a_read() {
        assert_eq!(finalmouse_request(36), pad(64, &h("04 02 A4 00")));
        assert_eq!(finalmouse_request(5), pad(64, &h("04 02 85 00")));
        assert_eq!(finalmouse_request(38), pad(64, &h("04 02 A6 00")));
    }

    #[test]
    fn finalmouse_answer_carries_the_command_and_its_data() {
        let voltage = pad(64, &h("05 04 05 02 B9 0F"));
        assert_eq!(finalmouse_data(&voltage, 5), Some(&[0xB9, 0x0F][..]));
        assert_eq!(finalmouse_data(&voltage, 36), None);
        assert_eq!(finalmouse_data(&pad(64, &h("05 03 A4 01 01")), 36), Some(&[0x01][..])); // high bit kept
        assert_eq!(finalmouse_data(&pad(64, &h("03 04 05 02 B9 0F")), 5), None); // the dongle's own report
        assert_eq!(finalmouse_data(&h("05 04 05 02 B9"), 5), None); // cut short
    }

    #[test]
    fn finalmouse_curve_goes_through_the_points_of_the_vendor_tool() {
        assert_eq!(finalmouse_percent(3620), Some(5));
        assert_eq!(finalmouse_percent(3660), Some(10));
        assert_eq!(finalmouse_percent(3740), Some(25));
        assert_eq!(finalmouse_percent(3880), Some(50));
        assert_eq!(finalmouse_percent(4170), Some(75));
        assert_eq!(finalmouse_percent(4380), Some(100));
        assert_eq!(finalmouse_percent(4025), Some(63)); // halfway between 50 and 75, rounded up
        assert_eq!(finalmouse_percent(3000), Some(0));
        assert_eq!(finalmouse_percent(2900), Some(0));
        assert_eq!(finalmouse_percent(4400), Some(100));
    }

    #[test]
    fn finalmouse_voltage_that_is_no_voltage_gives_no_level() {
        assert_eq!(finalmouse_percent(0), None);
        assert_eq!(finalmouse_percent(0xFFFF), None);
        assert_eq!(finalmouse_state(None, false, Some(0)), unknown(false));
        assert_eq!(finalmouse_state(None, false, None), unknown(false));
    }

    #[test]
    fn finalmouse_level_from_the_voltage_is_marked_as_an_estimate() {
        let estimate = State { detail: Some("d.estimate".into()), ..State::new(true, Some(50), false) };
        assert_eq!(finalmouse_state(None, false, Some(3880)), estimate);
    }

    #[test]
    fn finalmouse_own_percent_wins_and_charging_hides_the_voltage() {
        assert_eq!(finalmouse_state(Some(81), false, Some(3880)), State::new(true, Some(81), false));
        assert_eq!(finalmouse_state(Some(81), true, None), State::new(true, Some(81), true));
        assert_eq!(finalmouse_state(None, true, Some(4300)), on_cable());
    }
}
