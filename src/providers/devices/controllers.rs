//! Game controllers on their 2.4 GHz receivers or a cable: 8BitDo Ultimate 2 Wireless, Flydigi Vader 5 Pro, Apex 5
//! and Apex 6, GameSir Nova 2 / Nova 2 Lite and Super Nova, BEITONG KP40D, NVIDIA SHIELD Controller (2017) as table
//! rows, and the Steam Controller (2026 and 2015) as a provider of its own at the end of the file, because its
//! receiver carries up to four controllers that are only listened to.
//!
//! None of them is listed twice. The receivers of the 8BitDo, Flydigi, GameSir and BEITONG pads show up in XInput as
//! a wired Xbox 360 pad (battery type "wired", which the XInput provider skips): GamesirBatteryTray's README says so
//! for the Nova 2, HaloBattery (providers/xinput.py, flydigi.py, issue 191) for the Vader 5 Pro and the KP40D, WDBM
//! (eightbitdo.cpp, gamesir_sapjoy.cpp) for the Ultimate 2 Wireless and the Super Nova; for the Flydigi Apex 5 and
//! Apex 6 nobody says, they are taken to be as the Vader. The SHIELD and Steam controllers are plain HID devices
//! that XInput doesn't see. Windows lists a battery only for Bluetooth devices, and every row here is a USB device:
//! the SHIELD controller is read on its cable only, and of the Steam Controller only its receivers are claimed,
//! neither its Bluetooth ID nor its cable's, since a controller on a cable goes on reporting through its puck.
//!
//! The protocols, each a read-only status request (or nothing sent at all):
//!   8BitDo Ultimate 2 Wireless: vendor collection FF7A:0001 (FFA0:0001 while the receiver is idle), output report
//!     81, input report 02: `81 04 cmd(2) sub(2) len(2) ..` is answered by `02 04 04 00 cmd(2) len(2) crc(2)
//!     total(4) offset(4) data`. 20 01 = is a controller linked, 21 01 with length 6 = the controller behind the
//!     receiver (data[0] the level in percent), 00 01 = device info (data[4..5] the product the firmware reports).
//!   Flydigi: vendor collection FFA0:0001, 32-byte reports. `5A A5 01 02 00` (after report ID 03 on the Apex 5, as
//!     an unnumbered report on the others) is answered by `5A A5 01 ..`: [5] model, [6] connection, [11] battery,
//!     high nibble 0 on battery / 1 charging / 2 charged, low nibble the level in steps (0-5, Apex 6: 0-10).
//!   GameSir Nova 2: vendor collection on page FF7A, unnumbered 64-byte reports. `01 01` is answered by a report
//!     starting 01 with the level at [18]: 0-100, or FF while charging on the cable.
//!   GameSir Super Nova: vendor collection on page FFA0, output report 07, input report 06. `07 03 0A 01` is
//!     answered by `06 len 0A 01 id fw(2) address(6) level type`, the level as above.
//!   BEITONG KP40D: vendor collection 00FF:0003 on interface 1, report 02 both ways. `02 15` is answered by
//!     `02 15 level xx state`, state 50 on battery, 51 charging.
//!   NVIDIA SHIELD Controller: output report 04 `04 cmd 00`, input report 03 `03 cmd 00 payload`. Command 07 = the
//!     battery (the level in percent at [14]), 3A = the charger (connected, type, state).
//!   Steam Controller: see `SteamControllerProvider`.
//!
//! Protocol facts from SDL (src/joystick/hidapi/SDL_hidapi_flydigi.c, SDL_hidapi_steam_triton.c,
//! steam/controller_structs.h), the Linux kernel (drivers/hid/hid-steam.c, hid-nvidia-shield.c), HaloBattery
//! (providers/steam.py, beitong.py, flydigi.py, shield.py, docs/protocols.md), Wireless-Device-Battery-Monitor
//! (src/drivers/eightbitdo.cpp, flydigi.cpp, gamesir.cpp, gamesir_sapjoy.cpp, steam_controller.cpp,
//! docs/gamesir_sapjoy.md), 8bitdo-ultimate2-linux (docs/PROTOCOL.md), GamesirBatteryTray (README.md,
//! GameSirDevice.cs), Gamesir-Nova-2-Lite-Battery-Detection (NovaBatteryReader.cs) and openflydigi (PROTOCOL.md,
//! docs/findings-apex6.md, flydigi/motion.py). Own implementation. Not yet verified on hardware by this project,
//! which is why the first exchange with each controller goes to the log.
//!
//! Left alone on purpose: anything that switches a controller's mode. 8BitDo's D-input pads send their level only
//! after feature report 06 switched them to the enhanced report, the Switch Pro Controller needs a handshake on USB,
//! the GameSir Cyclone 2 starts streaming its state when asked: none of that is sent, so those aren't here.

use super::{off, on, Io, Spec, State};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::providers::{hex_trimmed as hex, remaining_ms, secs, Provider};
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub static SPECS: &[Spec] = &[
    // 310B is the receiver with the controller linked (and the controller on its cable), 6013 the receiver while
    // the controller sits in its dock with the radio off
    Spec {
        key: "8bitdo-ultimate-2-wireless",
        name: "8BitDo Ultimate 2 Wireless",
        vid: 0x2DC8,
        pids: &[0x310B, 0x6013],
        accept: Some(is_ultimate2_channel),
        query: ultimate2,
        ..Spec::GAMEPAD
    },
    // Receiver and cable share the product ID. The FFEE collection next to this one is never written to.
    Spec {
        key: "flydigi-vader-5-pro",
        name: "Flydigi Vader 5 Pro",
        vid: 0x37D7,
        pids: &[0x2401],
        page: 0xFFA0,
        usage: 0x0001,
        query: |io| flydigi(io, 0x00, &[130], 5),
        ..Spec::GAMEPAD
    },
    Spec {
        key: "flydigi-apex-5",
        name: "Flydigi Apex 5",
        vid: 0x37D7,
        pids: &[0x2501],
        page: 0xFFA0,
        usage: 0x0001,
        query: |io| flydigi(io, 0x03, &[128, 129, 133, 134, 135], 5),
        ..Spec::GAMEPAD
    },
    Spec {
        key: "flydigi-apex-6",
        name: "Flydigi Apex 6",
        vid: 0x37D7,
        pids: &[0x2502],
        page: 0xFFA0,
        usage: 0x0001,
        query: |io| flydigi(io, 0x00, &[149, 150, 152], 10),
        ..Spec::GAMEPAD
    },
    // 1098 is the receiver while a controller is linked to it (it takes another ID when the controller is off),
    // 100F the Nova 2 Lite on its cable
    Spec {
        key: "gamesir-nova-2",
        name: "GameSir Nova 2 / Nova 2 Lite",
        vid: 0x3537,
        pids: &[0x1098, 0x100F],
        cable_pids: &[0x100F],
        accept: Some(|i| i.usage_page == 0xFF7A && i.output_length >= 3 && i.input_length >= 20),
        query: gamesir_nova,
        ..Spec::GAMEPAD
    },
    // The receiver of the Super Nova (NovaPro) and of the Tarantula Pro: the product string says which
    Spec {
        key: "gamesir-super-nova",
        name: "GameSir Super Nova / Tarantula Pro",
        vid: 0x3537,
        pids: &[0x103E],
        accept: Some(|i| {
            i.usage_page == 0xFFA0 && i.output_length == 32 && i.input_length == 64 && i.feature_length == 0
        }),
        query: gamesir_super_nova,
        ..Spec::GAMEPAD
    },
    Spec {
        key: "beitong-kp40d",
        name: "BEITONG KP40D",
        vid: 0x20BC,
        pids: &[0x515C],
        page: 0x00FF,
        usage: 0x0003,
        interface: 1,
        query: beitong_kp40d,
        ..Spec::GAMEPAD
    },
    // On its cable only: over Bluetooth Windows may list the controller itself, and it would be here twice. So the
    // row counts only while a USB collection is there.
    Spec {
        key: "nvidia-shield-controller",
        name: "NVIDIA SHIELD Controller",
        vid: 0x0955,
        pids: &[0x7214],
        cable_pids: &[0x7214],
        accept: Some(|i| is_usb_path(&i.path) && i.output_length >= 33 && i.input_length >= 33),
        shared: true,
        query: shield,
        ..Spec::GAMEPAD
    },
];

/// A charging controller that doesn't say its level.
fn charging_unknown() -> State {
    State { detail: Some("d.cableHidden".into()), ..State::new(true, None, true) }
}

/// Whether a device path is a USB one (`hid#vid_0955&pid_7214..`) and not a Bluetooth one
/// (`hid#{00001124-..}_vid&00020955_pid&7214..`).
fn is_usb_path(path: &str) -> bool {
    path.to_ascii_lowercase().contains("hid#vid_")
}

// ---- 8BitDo Ultimate 2 Wireless ----

const U2_LINK: [u8; 2] = [0x20, 0x01];
const U2_BATTERY: [u8; 2] = [0x21, 0x01];
const U2_INFO: [u8; 2] = [0x00, 0x01];
/// Where the data of an answer starts, after its header.
const U2_DATA: usize = 18;
/// The product the controller's firmware reports for itself; the receiver reports 6013.
const U2_CONTROLLER: u16 = 0x6012;

fn is_ultimate2_channel(info: &HidInfo) -> bool {
    matches!(info.usage_page, 0xFF7A | 0xFFA0)
        && info.usage == 0x0001
        && info.input_length >= 64
        && info.output_length >= 64
}

/// A request without data: the command and how many bytes are asked for.
fn u2_request(command: [u8; 2], length: u8) -> [u8; 8] {
    [0x81, 0x04, command[0], command[1], 0x00, 0x00, length, 0x00]
}

/// Whether `d` answers `command` and is long enough for the device info's product ID.
fn u2_is_reply(d: &[u8], command: [u8; 2]) -> bool {
    d.len() >= U2_DATA + 6 && d[..4] == [0x02, 0x04, 0x04, 0x00] && d[4..6] == command
}

/// The level and the charging flag from the answer about the controller behind the receiver. Bit 7 as the charging
/// flag is what WDBM reads there, by the layout of the same byte in 8BitDo's input reports.
fn u2_battery(d: &[u8]) -> Option<(i32, bool)> {
    if !u2_is_reply(d, U2_BATTERY) || d[6..8] != [0x06, 0x00] {
        return None;
    }
    let level = (d[U2_DATA] & 0x7F) as i32;
    (1..=100).contains(&level).then_some((level, d[U2_DATA] & 0x80 != 0 && level < 100))
}

/// Whether the device info comes from the controller itself, which is how it answers on its cable.
fn u2_is_controller(d: &[u8]) -> bool {
    u2_is_reply(d, U2_INFO) && u16::from_le_bytes([d[U2_DATA + 4], d[U2_DATA + 5]]) == U2_CONTROLLER
}

fn ultimate2(io: &mut Io) -> win::Result<State> {
    // Three questions at most, so none may wait long
    io.set_timeout(400);
    let link = io.ask(&u2_request(U2_LINK, 0), |d| u2_is_reply(d, U2_LINK), 8, false)?;
    if link.is_some_and(|d| d[U2_DATA] == 0) {
        return Ok(off());
    }
    let battery = io.ask(&u2_request(U2_BATTERY, 6), |d| u2_is_reply(d, U2_BATTERY), 8, false)?;
    if let Some((level, charging)) = battery.and_then(|d| u2_battery(&d)) {
        return Ok(on(level, charging));
    }
    // Nobody answers for the controller behind the receiver: it is off, or this is the controller on its cable,
    // where it doesn't give the level
    let info = io.ask(&u2_request(U2_INFO, 0), |d| u2_is_reply(d, U2_INFO), 8, false)?;
    Ok(if info.is_some_and(|d| u2_is_controller(&d)) { charging_unknown() } else { off() })
}

// ---- Flydigi ----

const FLYDIGI_INFO: u8 = 0x01;

/// The device info request, as SDL sends it. `report_id`: 03 on the Apex 5, 00 where the reports are unnumbered.
fn flydigi_request(report_id: u8) -> [u8; 6] {
    [report_id, 0x5A, 0xA5, FLYDIGI_INFO, 0x02, 0x00]
}

/// The answer to `command` without the input report ID 04 that the Apex 5 puts in front of it. The controller's
/// own input stream shares the collection and starts the same way, with another command byte.
fn flydigi_reply(d: &[u8], command: u8) -> Option<&[u8]> {
    let body = if d.first() == Some(&0x04) { &d[1..] } else { d };
    (body.len() >= 12 && body[0] == 0x5A && body[1] == 0xA5 && body[2] == command).then_some(body)
}

/// `models`: the model numbers this product ID is known to report, so a reply about anything else isn't read.
/// `steps`: the highest level step.
fn flydigi_state(body: &[u8], models: &[u8], steps: u8) -> Option<State> {
    let (state, step) = (body[11] >> 4, body[11] & 0x0F);
    if !models.contains(&body[5]) || body[6] > 2 || step > steps {
        return None;
    }
    match state {
        // One step is a fifth or a tenth of the battery: an estimate
        0 => Some(State { estimate: true, ..on(step as i32 * 100 / steps as i32, false) }),
        // SDL shows the step while charging, Flydigi's own software shows no level then: no number
        1 => Some(State::new(true, None, true)),
        2 => Some(State { full: true, ..State::new(true, Some(100), false) }),
        _ => None,
    }
}

fn flydigi(io: &mut Io, report_id: u8, models: &[u8], steps: u8) -> win::Result<State> {
    // While a game or Steam uses the controller its state arrives here a thousand times a second
    io.set_timeout(600);
    let reply = io.ask(&flydigi_request(report_id), |d| flydigi_reply(d, FLYDIGI_INFO).is_some(), 1000, false)?;
    let state =
        reply.as_deref().and_then(|d| flydigi_reply(d, FLYDIGI_INFO)).and_then(|b| flydigi_state(b, models, steps));
    Ok(state.unwrap_or_else(off))
}

// ---- GameSir ----

/// A level byte of the GameSir replies: FF = charging, no level given. Zero is what an empty reply holds, and a
/// controller that could send a real zero is off, so it is no reading; neither is anything above 100.
fn gamesir_level(level: u8) -> Option<State> {
    match level {
        0xFF => Some(charging_unknown()),
        1..=100 => Some(on(level as i32, false)),
        _ => None,
    }
}

/// The device info reply. `F4 00 ..` reports are the controller's state, which arrives unasked.
fn gamesir_nova_state(d: &[u8]) -> Option<State> {
    if d.len() < 19 || d[0] != 0x01 {
        return None;
    }
    gamesir_level(d[18])
}

fn gamesir_nova(io: &mut Io) -> win::Result<State> {
    // The controller's state frames share the collection: the wait is what ends the listening, not their number
    io.set_timeout(1000);
    let reply = io.ask(&[0x00, 0x01, 0x01], |d| d.len() >= 19 && d[0] == 0x01, 1000, false)?;
    Ok(reply.and_then(|d| gamesir_nova_state(&d)).unwrap_or_else(off))
}

/// The reply to GetDeviceInfo: `06 len 0A 01 ..`, the length counting itself and reaching the level at least.
fn is_super_nova_reply(d: &[u8]) -> bool {
    d.len() >= 15 && d[0] == 0x06 && d[2] == 0x0A && d[3] == 0x01 && d[1] >= 14 && (d[1] as usize) < d.len()
}

fn gamesir_super_nova_state(d: &[u8]) -> Option<State> {
    if !is_super_nova_reply(d) {
        return None;
    }
    gamesir_level(d[13])
}

/// The product string when it says which of the two controllers this is.
fn super_nova_name(product: Option<&str>) -> Option<String> {
    let product = product?.trim();
    let lower = product.to_ascii_lowercase();
    (lower.contains("nova") || lower.contains("tarantula")).then(|| {
        if lower.contains("gamesir") {
            product.to_string()
        } else {
            format!("GameSir {product}")
        }
    })
}

fn gamesir_super_nova(io: &mut Io) -> win::Result<State> {
    // The controller's state frames share the collection
    io.set_timeout(900);
    let reply = io.ask(&[0x07, 0x03, 0x0A, 0x01], is_super_nova_reply, 400, false)?;
    let name = super_nova_name(io.info().product.as_deref());
    Ok(reply.and_then(|d| gamesir_super_nova_state(&d)).map_or_else(off, |state| State { name, ..state }))
}

// ---- BEITONG ----

/// The status reply. `02 25` and `02 21` reports appear while BEITONG's own software is open and are not it. The
/// level is the controller's estimate; a zero is what an empty reply holds, so it is no reading.
fn beitong_state(d: &[u8]) -> Option<State> {
    if d.len() < 5 || d[0] != 0x02 || d[1] != 0x15 || !matches!(d[4], 0x50 | 0x51) {
        return None;
    }
    (1..=100).contains(&d[2]).then(|| on(d[2] as i32, d[4] == 0x51))
}

fn beitong_kp40d(io: &mut Io) -> win::Result<State> {
    io.set_timeout(800);
    let reply = io.ask(&[0x02, 0x15], |d| d.len() >= 5 && d[0] == 0x02 && d[1] == 0x15, 20, false)?;
    Ok(reply.and_then(|d| beitong_state(&d)).unwrap_or_else(off))
}

// ---- NVIDIA SHIELD Controller (2017) ----

const SHIELD_BATTERY: u8 = 0x07;
const SHIELD_CHARGER: u8 = 0x3A;

fn shield_is_reply(d: &[u8], command: u8) -> bool {
    d.len() >= 15 && d[0] == 0x03 && d[1] == command && d[2] == 0x00
}

/// The level from the battery reply: after the header come the average voltage, a reserved byte, the thermistor
/// and three more voltages, then the level. A reply without a voltage is an empty one.
fn shield_level(d: &[u8]) -> Option<i32> {
    if !shield_is_reply(d, SHIELD_BATTERY) {
        return None;
    }
    let voltage = u16::from_le_bytes([d[3], d[4]]);
    (voltage != 0 && d[14] <= 100).then_some(d[14] as i32)
}

/// (charging, full) from the charger reply: connected, type, state, the state 2 = charging, 3 = full.
fn shield_charger(d: &[u8]) -> (bool, bool) {
    if !shield_is_reply(d, SHIELD_CHARGER) || d[3] != 1 {
        return (false, false);
    }
    (d[5] == 2, d[5] == 3)
}

fn shield(io: &mut Io) -> win::Result<State> {
    // The collection may be the one the controller's state arrives on: the wait ends the listening, not a count
    io.set_timeout(1000);
    let battery = io.ask(&[0x04, SHIELD_BATTERY, 0x00], |d| shield_is_reply(d, SHIELD_BATTERY), 1000, false)?;
    let Some(level) = battery.and_then(|d| shield_level(&d)) else { return Ok(off()) };
    let charger = io.ask(&[0x04, SHIELD_CHARGER, 0x00], |d| shield_is_reply(d, SHIELD_CHARGER), 1000, false)?;
    let (charging, full) = charger.map_or((false, false), |d| shield_charger(&d));
    Ok(State { full, ..on(level, charging) })
}

// ---- Steam Controller ----

const VALVE_VID: u16 = 0x28DE;
/// How long a slot is listened to before anything is heard on it: a controller that is on sends its state every
/// few milliseconds.
const QUIET_MS: u64 = 400;
/// How long a slot with a controller is listened to for its battery report, which comes about every 2.5 s
/// (HaloBattery, issue 58: a 0.4 s window missed it).
const LIVE_MS: u64 = 5000;
/// Five seconds of a state stream are some 1300 reports; this only stops a stream gone wrong.
const MAX_REPORTS: usize = 4096;

/// What one input report of a controller slot says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Heard {
    Battery {
        level: i32,
        charging: bool,
        full: bool,
    },
    /// The controller's state: it is on.
    State,
    Connected,
    Disconnected,
    Other,
}

impl Heard {
    fn label(&self) -> &'static str {
        match self {
            Heard::Battery { .. } => "battery",
            Heard::State => "state",
            Heard::Connected => "connected",
            Heard::Disconnected => "disconnected",
            Heard::Other => "other",
        }
    }
}

/// What a slot turned out to hold after listening to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// Nothing was heard: no controller, or it is off.
    Quiet,
    /// The receiver said the controller left.
    Gone,
    /// A controller is on; its battery report when one came in time.
    On(Option<Heard>),
}

struct SteamModel {
    pid: u16,
    name: &'static str,
    heard: fn(&[u8]) -> Heard,
}

/// The receivers only. The 2026 controller on its cable (28DE:1302) sends the same reports, but a controller that
/// charges on a cable was seen to go on reporting through its puck (HaloBattery, issue 58), so it would be here
/// twice, under two keys.
static STEAM_MODELS: &[SteamModel] = &[
    // The puck of the 2026 controller, and the receiver built into the Steam Machine
    SteamModel { pid: 0x1304, name: "Steam Controller", heard: heard_2026 },
    SteamModel { pid: 0x1305, name: "Steam Controller", heard: heard_2026 },
    // The wireless adapter of the 2015 controller
    SteamModel { pid: 0x1142, name: "Steam Controller (2015)", heard: heard_2015 },
];

fn steam_model(pid: u16) -> Option<&'static SteamModel> {
    STEAM_MODELS.iter().find(|m| m.pid == pid)
}

/// A report of the 2026 controller: 43 = battery (charge state 1 discharging / 2 charging / 4 charging done, then
/// the level in percent), 79 and 46 = the receiver's word on the link (1 gone, 2 connected), 42, 45 and 47 = state.
/// Windows pads the battery report to the collection's longest, so it is never shorter than its 15 bytes.
fn heard_2026(d: &[u8]) -> Heard {
    match d.first() {
        Some(0x43) if d.len() >= 15 && d[2] <= 100 => {
            let level = d[2] as i32;
            match d[1] {
                1 => Heard::Battery { level, charging: false, full: false },
                2 => Heard::Battery { level, charging: true, full: false },
                4 => Heard::Battery { level, charging: false, full: true },
                _ => Heard::Other,
            }
        }
        Some(0x79 | 0x46) if d.len() >= 2 => match d[1] {
            1 => Heard::Disconnected,
            2 => Heard::Connected,
            _ => Heard::Other,
        },
        Some(0x42 | 0x45 | 0x47) => Heard::State,
        _ => Heard::Other,
    }
}

/// A report of the 2015 controller's adapter: `01 00 type length payload`. Type 04 with 11 bytes = status (the
/// battery voltage in mV at [12..13], the level in percent at [14]), 03 = the link (1 gone, 2 connected), 01 =
/// state. The controller runs on AA cells, so nothing ever charges.
fn heard_2015(d: &[u8]) -> Heard {
    if d.len() < 15 || d[0] != 0x01 || d[1] != 0x00 {
        return Heard::Other;
    }
    match d[2] {
        0x04 if d[3] == 0x0B && d[14] <= 100 && (d[12] | d[13]) != 0 => {
            Heard::Battery { level: d[14] as i32, charging: false, full: false }
        }
        0x03 if d[4] == 0x01 => Heard::Disconnected,
        0x03 if d[4] == 0x02 => Heard::Connected,
        0x01 => Heard::State,
        _ => Heard::Other,
    }
}

/// The collection a controller's reports arrive on. Puck: one controller per interface 2-5, on FF00:0001
/// (interface 6 with FF00:0002 is the charging contacts). 2015 adapter: one controller per interface 1-4, interface
/// 0 is its keyboard.
fn is_steam_slot(info: &HidInfo) -> bool {
    if info.access_denied || info.usage_page != 0xFF00 || info.input_length < 15 {
        return false;
    }
    let interface = info.interface_number();
    match info.product_id {
        0x1304 | 0x1305 => (2..=5).contains(&interface) && info.usage == 0x0001,
        0x1142 => (1..=4).contains(&interface),
        _ => false,
    }
}

/// Listens to one slot. `read` returns the next input report, or None when none came in the given milliseconds.
/// `seen` gets, per kind of report, how many came and the first of them.
fn listen(
    mut read: impl FnMut(u32) -> win::Result<Option<Vec<u8>>>,
    heard: fn(&[u8]) -> Heard,
    seen: &mut Vec<(&'static str, usize, String)>,
) -> win::Result<Slot> {
    let start = Instant::now();
    let mut deadline = start + Duration::from_millis(QUIET_MS);
    let mut on = false;
    for _ in 0..MAX_REPORTS {
        if Instant::now() >= deadline {
            break;
        }
        let Some(raw) = read(remaining_ms(deadline))? else { break };
        let data = Io::data(&raw);
        let what = heard(data);
        match seen.iter_mut().find(|s| s.0 == what.label()) {
            Some(s) => s.1 += 1,
            None => seen.push((what.label(), 1, hex(data))),
        }
        match what {
            Heard::Battery { .. } => return Ok(Slot::On(Some(what))),
            Heard::Disconnected => return Ok(Slot::Gone),
            Heard::State | Heard::Connected => {
                on = true;
                deadline = start + Duration::from_millis(LIVE_MS);
            }
            Heard::Other => {}
        }
    }
    Ok(if on { Slot::On(None) } else { Slot::Quiet })
}

/// Steam Controllers behind their receivers: the 2026 controller on its puck and the 2015 one on its wireless
/// adapter. A receiver has four slots, one HID interface each, and a controller sends its battery
/// level there by itself. So this only listens: Steam and SDL talk to the controller over the same collection
/// (rumble, and a setting that turns its mouse and keyboard emulation off), and nothing of that may come from
/// here. The collections are opened for reading only, next to Steam.
/// A slot that was never heard holds no controller as far as anyone can tell and is not listed; one that went
/// quiet after its controller was seen is listed as off, without the level it had.
#[derive(Default)]
pub struct SteamControllerProvider {
    /// The slots a controller was heard on in this session.
    known: HashSet<String>,
    /// What this session's log already has: see `log_once`.
    logged: HashSet<String>,
}

impl Provider for SteamControllerProvider {
    fn name(&self) -> &'static str {
        "Steam Controller"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result: Vec<DeviceReading> = Vec::new();
        let mut slots: Vec<HidInfo> = hid::enumerate(|v, p| v == VALVE_VID && steam_model(p).is_some())
            .into_iter()
            .filter(is_steam_slot)
            .collect();
        slots.sort_by_key(|i| (i.product_id, i.interface_number()));
        let mut asked: Vec<String> = Vec::new();
        for info in &slots {
            let Some(model) = steam_model(info.product_id) else { continue };
            let id = format!("steam:{:04x}:{}", info.product_id, info.interface_number().max(0));
            if asked.contains(&id) {
                continue;
            }
            asked.push(id.clone());

            let mut seen = Vec::new();
            let slot = HidDevice::open_read_only(info).and_then(|dev| {
                dev.flush();
                listen(|ms| dev.read(ms), model.heard, &mut seen)
            });
            let online = matches!(slot, Ok(Slot::On(_)));
            self.log_once(&id, info, &slot, &seen);
            if online {
                self.known.insert(id.clone());
            } else if !self.known.contains(&id) {
                continue;
            }

            // Several controllers get #2, #3... in the order of their slots
            let numbered = format!("{} #", model.name);
            let same = result.iter().filter(|r| r.name == model.name || r.name.starts_with(&numbered)).count();
            let name = if same == 0 { model.name.to_string() } else { format!("{} #{}", model.name, same + 1) };
            let reading = DeviceReading::new(id, name, DeviceKind::Gamepad, t("via.24"));
            result.push(match slot {
                Ok(Slot::On(Some(Heard::Battery { level, charging, full }))) => {
                    DeviceReading { online: true, level: Some(level), charging, full_on_charger: full, ..reading }
                }
                Ok(Slot::On(_)) => DeviceReading { online: true, detail: Some(t("d.levelUnknown")), ..reading },
                Ok(_) => DeviceReading { detail: Some(t("d.offOrAway")), ..reading },
                Err(e) => DeviceReading { detail: Some(tf("d.error", &[&e])), ..reading },
            });
        }
        result
    }

    fn delay(&self, last: &[DeviceReading]) -> Duration {
        if last.iter().any(|r| r.online) {
            secs(60)
        } else {
            secs(30)
        }
    }
}

impl SteamControllerProvider {
    /// Logs what a slot sent the first time it is found quiet and the first time a controller is found on it in a
    /// session.
    fn log_once(&mut self, id: &str, info: &HidInfo, slot: &win::Result<Slot>, seen: &[(&'static str, usize, String)]) {
        let online = matches!(slot, Ok(Slot::On(_)));
        if !self.logged.insert(format!("{id}{online}")) {
            return;
        }
        let outcome = match slot {
            Ok(slot) => format!("{slot:?}"),
            Err(e) => format!("error: {e}"),
        };
        log::write(&format!("Steam Controller {id}: {outcome}, mi={} {info}", info.interface_number()));
        for (kind, count, first) in seen {
            log::write(&format!("  {kind} x{count}, the first: {first}"));
        }
    }
}

#[cfg(test)]
mod tests {
    //! Frames as the sources give them: the Ultimate 2 Wireless answers from WDBM's eightbitdo.cpp and
    //! 8bitdo-ultimate2-linux, the Apex 5 replies captured in openflydigi's PROTOCOL.md, the Nova 2 reply from
    //! GamesirBatteryTray's README, the Super Nova layout from WDBM's docs/gamesir_sapjoy.md, the KP40D reply from
    //! HaloBattery's capture, the Steam Controller reports from HaloBattery's hardware run (issue 58).
    use super::*;
    use crate::providers::pad;

    fn state(online: bool, level: Option<i32>, charging: bool) -> State {
        State::new(online, level, charging)
    }

    fn at(length: usize, head: &[u8], index: usize, value: u8) -> Vec<u8> {
        let mut r = pad(length, head);
        r[index] = value;
        r
    }

    #[test]
    fn usb_and_bluetooth_paths_are_told_apart() {
        assert!(is_usb_path(r"\\?\HID#VID_0955&PID_7214&MI_00#8&1&0&0000#{4d1e55b2}"));
        assert!(!is_usb_path(r"\\?\hid#{00001124-0000-1000-8000-00805f9b34fb}_vid&00020955_pid&7214#9&1&0#{4d1e55b2}"));
        assert!(!is_usb_path(r"\\?\hid#{00001812-0000-1000-8000-00805f9b34fb}&dev&vid&020955&pid&7214#9&1#{4d1e55b2}"));
    }

    #[test]
    fn ultimate_2_requests_name_the_command_and_the_length() {
        assert_eq!(u2_request(U2_LINK, 0), [0x81, 0x04, 0x20, 0x01, 0, 0, 0, 0]);
        assert_eq!(u2_request(U2_BATTERY, 6), [0x81, 0x04, 0x21, 0x01, 0, 0, 0x06, 0]);
        assert_eq!(u2_request(U2_INFO, 0), [0x81, 0x04, 0x00, 0x01, 0, 0, 0, 0]);
    }

    #[test]
    fn ultimate_2_level_is_the_first_data_byte() {
        let head = [0x02, 0x04, 0x04, 0x00, 0x21, 0x01, 0x06, 0x00];
        assert_eq!(u2_battery(&at(64, &head, 18, 73)), Some((73, false)));
        assert_eq!(u2_battery(&at(64, &head, 18, 0x80 | 41)), Some((41, true)));
        assert_eq!(u2_battery(&at(64, &head, 18, 100)), Some((100, false)));
        assert_eq!(u2_battery(&at(64, &head, 18, 0x80 | 100)), Some((100, false)), "full is not charging");
    }

    #[test]
    fn ultimate_2_empty_or_impossible_levels_are_no_reading() {
        let head = [0x02, 0x04, 0x04, 0x00, 0x21, 0x01, 0x06, 0x00];
        assert_eq!(u2_battery(&pad(64, &head)), None);
        assert_eq!(u2_battery(&at(64, &head, 18, 101)), None);
        assert_eq!(u2_battery(&at(64, &head, 18, 0x7F)), None);
    }

    #[test]
    fn ultimate_2_answers_to_other_commands_are_not_the_battery() {
        // The link answer, with a controller linked
        let link = at(64, &[0x02, 0x04, 0x04, 0x00, 0x20, 0x01, 0x01, 0x00], 18, 1);
        assert!(u2_is_reply(&link, U2_LINK));
        assert!(!u2_is_reply(&link, U2_BATTERY));
        assert_eq!(u2_battery(&link), None);
        // Another length than the six bytes asked for
        assert_eq!(u2_battery(&at(64, &[0x02, 0x04, 0x04, 0x00, 0x21, 0x01, 0x01, 0x00], 18, 50)), None);
        // The fixed short queries are answered with 02 05 / 02 22
        assert!(!u2_is_reply(&pad(64, &[0x02, 0x22, 0x04, 0x00, 0x21, 0x01]), U2_BATTERY));
        assert!(!u2_is_reply(&[0x02, 0x04, 0x04, 0x00, 0x21, 0x01, 0x06, 0x00], U2_BATTERY), "too short");
    }

    #[test]
    fn ultimate_2_on_its_cable_reports_its_own_product() {
        let mut info = pad(64, &[0x02, 0x04, 0x04, 0x00, 0x00, 0x01, 0x08, 0x00]);
        info[22..24].copy_from_slice(&[0x12, 0x60]);
        assert!(u2_is_controller(&info));
        info[22..24].copy_from_slice(&[0x13, 0x60]); // the receiver
        assert!(!u2_is_controller(&info));
    }

    #[test]
    fn flydigi_request_takes_the_report_id_or_none() {
        assert_eq!(flydigi_request(0x03), [0x03, 0x5A, 0xA5, 0x01, 0x02, 0x00]);
        assert_eq!(flydigi_request(0x00), [0x00, 0x5A, 0xA5, 0x01, 0x02, 0x00]);
    }

    /// The Apex 5 on its receiver, firmware 7.0.4.5 (openflydigi, PROTOCOL.md section 11).
    const APEX5: [u8; 32] = [
        0x04, 0x5A, 0xA5, 0x01, 0x01, 0x00, 0x80, 0x02, 0x00, 0x00, 0x00, 0x00, 0x05, 0x45, 0x01, 0x00, 0x70, 0x45,
        0x21, 0x31, 0x45, 0x25, 0x00, 0x00, 0x01, 0x28, 0x00, 0x00, 0x11, 0x31, 0x1F, 0x00,
    ];
    const APEX5_MODELS: &[u8] = &[128, 129, 133, 134, 135];

    fn flydigi_parsed(reply: &[u8], models: &[u8], steps: u8) -> Option<State> {
        flydigi_reply(reply, FLYDIGI_INFO).and_then(|b| flydigi_state(b, models, steps))
    }

    /// A level counted in steps: on battery, and an estimate.
    fn step(level: i32) -> State {
        State { estimate: true, ..state(true, Some(level), false) }
    }

    #[test]
    fn apex_5_on_its_receiver_is_full() {
        assert_eq!(flydigi_parsed(&APEX5, APEX5_MODELS, 5), Some(step(100)));
    }

    #[test]
    fn vader_5_pro_on_its_receiver_and_on_its_cable() {
        // What its owner's log has (HaloBattery, issue 191): device 130, connection 2 and battery 05 on the receiver,
        // connection 1 and battery 15 on the cable; unnumbered reports
        let reply = |connection, battery| at(32, &[0x5A, 0xA5, 0x01, 0x01, 0x00, 130, connection], 11, battery);
        assert_eq!(flydigi_parsed(&reply(2, 0x05), &[130], 5), Some(step(100)));
        assert_eq!(flydigi_parsed(&reply(1, 0x15), &[130], 5), Some(state(true, None, true)));
    }

    #[test]
    fn flydigi_level_comes_in_steps() {
        let mut r = APEX5;
        r[12] = 0x02;
        assert_eq!(flydigi_parsed(&r, APEX5_MODELS, 5), Some(step(40)));
        r[12] = 0x00;
        assert_eq!(flydigi_parsed(&r, APEX5_MODELS, 5), Some(step(0)));
        // The Apex 6 counts to ten, in unnumbered reports
        let mut apex6 = pad(32, &[0x5A, 0xA5, 0x01, 0x01, 0x00, 150, 0x01]);
        apex6[11] = 0x07;
        assert_eq!(flydigi_parsed(&apex6, &[149, 150, 152], 10), Some(step(70)));
    }

    #[test]
    fn flydigi_charging_has_no_number_and_charged_is_full() {
        let mut r = APEX5;
        r[12] = 0x13;
        assert_eq!(flydigi_parsed(&r, APEX5_MODELS, 5), Some(state(true, None, true)));
        r[12] = 0x25;
        assert_eq!(flydigi_parsed(&r, APEX5_MODELS, 5), Some(State { full: true, ..state(true, Some(100), false) }));
    }

    #[test]
    fn flydigi_replies_that_do_not_fit_are_no_reading() {
        let mut r = APEX5;
        r[12] = 0x06; // a step the Apex 5 doesn't have
        assert_eq!(flydigi_parsed(&r, APEX5_MODELS, 5), None);
        r[12] = 0x35; // an unknown state
        assert_eq!(flydigi_parsed(&r, APEX5_MODELS, 5), None);
        assert_eq!(flydigi_parsed(&APEX5, &[130], 5), None, "another model than the product ID stands for");
        let mut r = APEX5;
        r[7] = 0x09; // no such connection
        assert_eq!(flydigi_parsed(&r, APEX5_MODELS, 5), None);
    }

    #[test]
    fn flydigi_input_stream_and_other_answers_are_skipped() {
        let mut stream = APEX5;
        stream[3] = 0xEF;
        assert!(flydigi_reply(&stream, FLYDIGI_INFO).is_none());
        let mut uid = APEX5;
        uid[3] = 0x04;
        assert!(flydigi_reply(&uid, FLYDIGI_INFO).is_none());
        assert!(flydigi_reply(&APEX5[..8], FLYDIGI_INFO).is_none());
        // The Apex 5 on its cable (openflydigi, section 7): the same answer with connection 1
        let cable = pad(32, &[0x04, 0x5A, 0xA5, 0x01, 0x01, 0x00, 0x80, 0x01]);
        assert!(flydigi_reply(&cable, FLYDIGI_INFO).is_some());
    }

    /// `01 F4 96 50 03 25 .. 49`: 73% (GamesirBatteryTray, README).
    fn nova_reply(level: u8) -> Vec<u8> {
        at(64, &[0x01, 0xF4, 0x96, 0x50, 0x03, 0x25], 18, level)
    }

    #[test]
    fn nova_2_level_is_byte_18() {
        assert_eq!(gamesir_nova_state(&nova_reply(0x49)), Some(state(true, Some(73), false)));
        assert_eq!(gamesir_nova_state(&nova_reply(100)), Some(state(true, Some(100), false)));
    }

    #[test]
    fn nova_2_charging_on_the_cable_gives_no_level() {
        assert_eq!(gamesir_nova_state(&nova_reply(0xFF)), Some(charging_unknown()));
        assert_eq!(charging_unknown().level, None);
        assert!(charging_unknown().charging);
    }

    #[test]
    fn nova_2_state_frames_and_empty_replies_are_no_reading() {
        assert_eq!(gamesir_nova_state(&at(64, &[0xF4, 0x00], 18, 50)), None);
        assert_eq!(gamesir_nova_state(&nova_reply(0)), None);
        assert_eq!(gamesir_nova_state(&nova_reply(101)), None);
        assert_eq!(gamesir_nova_state(&nova_reply(50)[..18]), None);
    }

    /// `06 0E 0A 01 id fw fw address(6) level type`.
    fn super_nova_reply(level: u8) -> Vec<u8> {
        pad(64, &[0x06, 0x0E, 0x0A, 0x01, 0x05, 0x01, 0x02, 0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, level, 0x03])
    }

    #[test]
    fn super_nova_level_is_byte_13() {
        // 0x60 was read while GameSir's own software showed 96%
        assert_eq!(gamesir_super_nova_state(&super_nova_reply(0x60)), Some(state(true, Some(96), false)));
        assert_eq!(gamesir_super_nova_state(&super_nova_reply(0xFF)), Some(charging_unknown()));
        assert_eq!(gamesir_super_nova_state(&super_nova_reply(0)), None);
        assert_eq!(gamesir_super_nova_state(&super_nova_reply(120)), None);
    }

    #[test]
    fn super_nova_state_frames_are_not_the_reply() {
        let mut r = super_nova_reply(0x60);
        r[2] = 0x0B;
        assert!(!is_super_nova_reply(&r));
        let mut r = super_nova_reply(0x60);
        r[0] = 0x05;
        assert!(!is_super_nova_reply(&r));
        let mut r = super_nova_reply(0x60);
        r[1] = 0x05; // says it ends before the level
        assert!(!is_super_nova_reply(&r));
        assert!(!is_super_nova_reply(&super_nova_reply(0x60)[..14]));
    }

    #[test]
    fn super_nova_name_comes_from_the_product_string_when_it_names_the_model() {
        assert_eq!(super_nova_name(Some("GameSir-Super Nova")), Some("GameSir-Super Nova".into()));
        assert_eq!(super_nova_name(Some("Tarantula Pro")), Some("GameSir Tarantula Pro".into()));
        assert_eq!(super_nova_name(Some("Controller")), None);
        assert_eq!(super_nova_name(None), None);
    }

    #[test]
    fn kp40d_reply_carries_the_level_and_the_charger() {
        // 02 15 58 00 50 ..: 88% on battery
        let captured = pad(64, &[0x02, 0x15, 0x58, 0x00, 0x50, 0x01, 0x00, 0xF0, 0x0F, 0x01]);
        assert_eq!(beitong_state(&captured), Some(state(true, Some(88), false)));
        assert_eq!(beitong_state(&pad(64, &[0x02, 0x15, 0x60, 0x00, 0x51])), Some(state(true, Some(96), true)));
    }

    #[test]
    fn kp40d_other_frames_and_odd_values_are_no_reading() {
        assert_eq!(beitong_state(&pad(64, &[0x02, 0x25, 0x58, 0x00, 0x50])), None);
        assert_eq!(beitong_state(&pad(64, &[0x02, 0x15, 0x58, 0x00, 0x52])), None, "an unknown charger state");
        assert_eq!(beitong_state(&pad(64, &[0x02, 0x15, 0x65, 0x00, 0x50])), None, "101%");
        assert_eq!(beitong_state(&pad(64, &[0x02, 0x15])), None, "an empty reply");
        assert_eq!(beitong_state(&[0x02, 0x15, 0x58]), None);
    }

    fn shield_battery_reply(voltage: u16, level: u8) -> Vec<u8> {
        let mut r = pad(33, &[0x03, 0x07, 0x00]);
        r[3..5].copy_from_slice(&voltage.to_le_bytes());
        r[14] = level;
        r
    }

    #[test]
    fn shield_level_follows_the_voltages() {
        assert_eq!(shield_level(&shield_battery_reply(3950, 81)), Some(81));
        // Windows pads the reply to the collection's longest report
        let mut padded = shield_battery_reply(3950, 81);
        padded.resize(65, 0);
        assert_eq!(shield_level(&padded), Some(81));
        assert_eq!(shield_level(&shield_battery_reply(3950, 101)), None);
        assert_eq!(shield_level(&shield_battery_reply(0, 0)), None, "an empty reply");
        assert_eq!(shield_level(&pad(33, &[0x03, 0x3A, 0x00, 0x01, 0x02, 0x02])), None, "the charger reply");
    }

    #[test]
    fn shield_charger_states() {
        let reply = |connected, state| pad(33, &[0x03, 0x3A, 0x00, connected, 0x02, state]);
        assert_eq!(shield_charger(&reply(1, 2)), (true, false));
        assert_eq!(shield_charger(&reply(1, 3)), (false, true));
        assert_eq!(shield_charger(&reply(1, 1)), (false, false));
        assert_eq!(shield_charger(&reply(0, 2)), (false, false), "no charger connected");
        assert_eq!(shield_charger(&shield_battery_reply(3950, 81)), (false, false));
    }

    fn battery(level: i32, charging: bool, full: bool) -> Heard {
        Heard::Battery { level, charging, full }
    }

    #[test]
    fn steam_controller_battery_report() {
        // 43 01 5f: 95% while in use; 43 04 64: 100%, charged, on the cable
        assert_eq!(heard_2026(&pad(15, &[0x43, 0x01, 0x5F])), battery(95, false, false));
        assert_eq!(heard_2026(&pad(64, &[0x43, 0x04, 0x64])), battery(100, false, true));
        assert_eq!(heard_2026(&pad(15, &[0x43, 0x02, 0x30])), battery(48, true, false));
    }

    #[test]
    fn steam_controller_battery_report_that_says_nothing_is_skipped() {
        assert_eq!(heard_2026(&pad(15, &[0x43, 0x01, 0x65])), Heard::Other, "101%");
        assert_eq!(heard_2026(&pad(15, &[0x43, 0x03, 0x50])), Heard::Other, "the charger is being checked");
        assert_eq!(heard_2026(&pad(15, &[0x43])), Heard::Other, "an empty report");
        assert_eq!(heard_2026(&[0x43, 0x01, 0x5F]), Heard::Other, "too short");
    }

    #[test]
    fn steam_controller_link_and_state_reports() {
        assert_eq!(heard_2026(&[0x79, 0x02]), Heard::Connected);
        assert_eq!(heard_2026(&[0x46, 0x01]), Heard::Disconnected);
        assert_eq!(heard_2026(&[0x79, 0x03]), Heard::Other, "pairing");
        assert_eq!(heard_2026(&pad(54, &[0x42])), Heard::State);
        assert_eq!(heard_2026(&pad(46, &[0x45])), Heard::State);
        assert_eq!(heard_2026(&[]), Heard::Other);
    }

    fn status_2015(voltage: u16, level: u8) -> Vec<u8> {
        let mut r = pad(64, &[0x01, 0x00, 0x04, 0x0B]);
        r[12..14].copy_from_slice(&voltage.to_le_bytes());
        r[14] = level;
        r
    }

    #[test]
    fn steam_controller_2015_status_report() {
        assert_eq!(heard_2015(&status_2015(2850, 74)), battery(74, false, false));
        assert_eq!(heard_2015(&status_2015(2850, 101)), Heard::Other);
        assert_eq!(heard_2015(&status_2015(0, 0)), Heard::Other, "no voltage: an empty report");
        assert_eq!(heard_2015(&pad(64, &[0x01, 0x00, 0x03, 0x01, 0x02])), Heard::Connected);
        assert_eq!(heard_2015(&pad(64, &[0x01, 0x00, 0x03, 0x01, 0x01])), Heard::Disconnected);
        assert_eq!(heard_2015(&pad(64, &[0x01, 0x00, 0x01, 0x3C])), Heard::State);
        assert_eq!(heard_2015(&pad(64, &[0x02, 0x00, 0x04, 0x0B])), Heard::Other);
    }

    /// Reports handed out one by one, then silence.
    fn reader(reports: Vec<Vec<u8>>) -> impl FnMut(u32) -> win::Result<Option<Vec<u8>>> {
        let mut left = reports.into_iter();
        move |_| Ok(left.next())
    }

    #[test]
    fn a_quiet_slot_holds_no_controller() {
        let mut seen = Vec::new();
        assert_eq!(listen(reader(vec![]), heard_2026, &mut seen).unwrap(), Slot::Quiet);
        assert!(seen.is_empty());
    }

    #[test]
    fn the_battery_report_ends_the_listening() {
        let reports = vec![pad(54, &[0x42]), pad(54, &[0x42]), pad(15, &[0x43, 0x01, 0x5F]), pad(54, &[0x42])];
        let mut seen = Vec::new();
        assert_eq!(listen(reader(reports), heard_2026, &mut seen).unwrap(), Slot::On(Some(battery(95, false, false))));
        assert_eq!(seen, [("state", 2, "42".to_string()), ("battery", 1, "43 01 5f".to_string())]);
    }

    #[test]
    fn a_controller_without_a_battery_report_is_on_with_no_level() {
        let reports = vec![vec![0x79, 0x02], pad(54, &[0x42])];
        assert_eq!(listen(reader(reports), heard_2026, &mut Vec::new()).unwrap(), Slot::On(None));
    }

    #[test]
    fn a_controller_that_left_is_gone() {
        let reports = vec![pad(54, &[0x42]), vec![0x79, 0x01], pad(15, &[0x43, 0x01, 0x5F])];
        assert_eq!(listen(reader(reports), heard_2026, &mut Vec::new()).unwrap(), Slot::Gone);
    }

    #[test]
    fn the_2015_adapter_reports_lose_the_zero_windows_puts_in_front() {
        let mut raw = vec![0x00];
        raw.extend(status_2015(2850, 74));
        assert_eq!(
            listen(reader(vec![raw]), heard_2015, &mut Vec::new()).unwrap(),
            Slot::On(Some(battery(74, false, false)))
        );
    }

    fn slot(pid: u16, mi: u8, usage: u16) -> HidInfo {
        HidInfo {
            path: format!(r"\\?\hid#vid_28de&pid_{pid:04x}&mi_{mi:02x}#8&1&0&0000#{{4d1e55b2}}"),
            vendor_id: VALVE_VID,
            product_id: pid,
            usage_page: 0xFF00,
            usage,
            input_length: 64,
            ..HidInfo::default()
        }
    }

    #[test]
    fn puck_slots_are_the_vendor_collections_of_interfaces_2_to_5() {
        assert!(is_steam_slot(&slot(0x1304, 2, 0x0001)));
        assert!(is_steam_slot(&slot(0x1305, 5, 0x0001)));
        assert!(!is_steam_slot(&slot(0x1304, 6, 0x0002)), "the charging contacts");
        assert!(!is_steam_slot(&slot(0x1304, 1, 0x0001)));
        assert!(!is_steam_slot(&HidInfo { usage_page: 0x0001, usage: 0x0002, ..slot(0x1304, 2, 0) }), "the mouse");
        assert!(!is_steam_slot(&HidInfo { access_denied: true, ..slot(0x1304, 2, 0x0001) }));
    }

    #[test]
    fn adapter_slots_are_interfaces_1_to_4() {
        assert!(is_steam_slot(&slot(0x1142, 1, 0x0001)));
        assert!(is_steam_slot(&slot(0x1142, 4, 0x0001)));
        assert!(!is_steam_slot(&slot(0x1142, 0, 0x0001)), "the adapter's keyboard");
        assert!(!is_steam_slot(&slot(0x1205, 2, 0x0001)), "the Steam Deck isn't claimed");
    }

    #[test]
    fn the_controller_on_its_cable_is_not_claimed_next_to_its_puck() {
        assert!(steam_model(0x1302).is_none());
        assert!(!is_steam_slot(&slot(0x1302, 0, 0x0001)));
    }

    #[test]
    fn ultimate_2_channel_is_the_64_byte_vendor_collection() {
        let channel = |page, usage, length| HidInfo {
            usage_page: page,
            usage,
            input_length: length,
            output_length: length,
            ..HidInfo::default()
        };
        assert!(is_ultimate2_channel(&channel(0xFF7A, 0x0001, 64)));
        assert!(is_ultimate2_channel(&channel(0xFFA0, 0x0001, 64)));
        assert!(!is_ultimate2_channel(&channel(0xFF7A, 0x0001, 32)));
        assert!(!is_ultimate2_channel(&channel(0x0001, 0x0005, 64)));
    }
}
