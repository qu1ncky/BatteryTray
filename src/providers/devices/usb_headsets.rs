//! Headsets of other brands on their USB dongles: Audeze, AKG, Skullcandy, Alienware, Poly, Jabra, EPOS, JBL,
//! ASUS ROG, Fractal Design, MOONDROP and, as a provider of its own at the end, the Sony PULSE 3D.
//!
//! Airoha dongles (Audeze Maxwell / Maxwell 2, AKG N9 Hybrid, Skullcandy PLYR / Crusher PLYR 720): the vendor
//! collection FF13:0001 with 62-byte reports. A request is output report 06, `06 <n> 80 <message>`: n bytes of
//! message, 80 = for the headset behind the dongle. The answer is not sent on the interrupt pipe, it is fetched
//! as input report 07 over the control pipe, `07 <n> 80 <bytes>`: only the n bytes count, the rest of the report
//! is left over from older answers, and n = 0 means nothing new. The bytes are a stream of RACE messages
//! `05 <type> <length, 2 bytes LE> <id, 2 bytes LE> <payload>`, type 5A request, 5B acknowledgement, 5D result.
//! The battery is id 0CD6 with payload 00: `05 5D 05 00 D6 0C <status 00> <role> <percent>`. There is no charging
//! state. Facts from HeadsetControl (lib/devices/audeze_maxwell_device.hpp, tests/test_protocols.cpp), HaloBattery
//! (providers/audeze.py, providers/skullcandy.py) and Wireless-Device-Battery-Monitor (src/drivers/akg.cpp).
//!
//! Alienware AW920H: the same two reports with a frame of its own, `06 C0 0A` answered by
//! `07 C0 0A 09 00 <linked> <percent> <charging> <critical> .. .. <critical percent> <mode> <xor of 3..12>`
//! (Wireless-Device-Battery-Monitor, src/drivers/alienware.cpp, from Alienware Command Center).
//!
//! Poly BT600 adapter: report 07 on FFA2:0003, `07 01 01 10 <len> <addr> 00 00 <op> <group> <item> <value>`, addr
//! 00 the adapter and 20 the headset, answered from 00 and 02. The battery is GET 0A/1A:
//! `<level> <number of levels> <charging> <minutes, 2 bytes> 01` (HeadsetControl, lib/devices/plantronics_bt600.hpp;
//! HaloBattery, providers/plantronics.py).
//!
//! Jabra Link 380 / 390 and the Evolve2 65 Flex / Evolve3 75 on a cable: Jabra's GN protocol on report 05 of the
//! FF00 collection, `05 <to> <from> <seq> <type << 6 | length> <command> <sub> <payload>`. Battery is GET 12/02,
//! answered with `<flags, bit 0 charging> <percent>`; command FE is a refusal (HeadsetControl,
//! lib/devices/protocols/jabra_gnp_protocol.hpp and jabra_*.hpp; Wireless-Device-Battery-Monitor,
//! src/drivers/jabra.cpp).
//!
//! EPOS H3PRO Hybrid: report 01 on FFFF:0001, 106 bytes of 16-bit little-endian words
//! `EB FE, 0001, type, target, payload length, CRC-8, group, command, values` (HeadsetControl,
//! lib/devices/epos_h3pro_hybrid.hpp, tests/test_epos_h3pro_hybrid.cpp).
//!
//! JBL Quantum: the newer models answer a "get device info" feature report 88 with input report 86 on the FF12
//! collection; the older dongles keep the link and the level in the one-byte feature reports 68 and 49
//! (Wireless-Device-Battery-Monitor, src/drivers/jbl.cpp; Visner-org/jbl-quantum-battery for the 810 and 910;
//! Tymon3310/jbl-quantum-linux, docs/QUANTUM610_PACKETS.md, for the 610; the product IDs from
//! sidmax7/QuantumEngine, models.rs).
//!
//! ASUS ROG Pelta / Delta II / Cetra: report CC on the FF00 collection, `CC 12 <sub>` echoed with the data from
//! byte 5, FF AA for a headset that is away. ROG Strix Go 2.4: feature report FF with the battery voltage
//! (G-Helper, app/Peripherals/Headset; Wireless-Device-Battery-Monitor, src/drivers/rog.cpp; HaloBattery,
//! providers/asus.py).
//!
//! Fractal Design Scape and MOONDROP GM01 Pro: one status request each (Wireless-Device-Battery-Monitor,
//! src/drivers/fractal.cpp and moondrop.cpp).
//!
//! Sony PULSE 3D: the dongle takes no requests and reports only changes, so it is listened to (HeadsetControl,
//! lib/devices/sony_pulse_3d.hpp, tests/test_sony_pulse_3d.cpp).
//!
//! Own implementation: only the protocol facts come from those projects. Everything sent is a read, apart from
//! the "hello" that opens the BT600's sessions, which changes nothing on the headset. Not yet verified on
//! hardware by this project, which is why the first exchange with each headset goes to the log.

use super::{off, on, Io, Spec, State};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::providers::{hex_trimmed, no_link_reason, pad, secs, sleep_ms, Provider};
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

pub static SPECS: &[Spec] = &[
    // ---- Airoha dongles (RACE) ----
    // The dongle and the headset on its USB-C cable share a key: the reading that reaches the headset wins
    Spec {
        key: "audeze-maxwell",
        name: "Audeze Maxwell",
        vid: AUDEZE,
        pids: &[0x4B19, 0x4B18, MAXWELL_CABLE],
        cable_pids: &[MAXWELL_CABLE],
        page: 0xFF13,
        usage: 0x0001,
        query: |io| {
            if io.product_id() == MAXWELL_CABLE {
                return race_battery(io, RaceLink::Cable);
            }
            // On its cable the headset keeps its link to the dongle and both answer with the same level
            // (HaloBattery measured it), but only the cable tells that it is charging: the dongle leaves it to it
            if maxwell_is_on_its_cable() {
                return Ok(off());
            }
            race_battery(io, RaceLink::AudezeDongle)
        },
        ..Spec::HEADSET
    },
    Spec {
        key: "audeze-maxwell-2",
        name: "Audeze Maxwell 2",
        vid: AUDEZE,
        pids: &[0x4B29, 0x4B28],
        page: 0xFF13,
        usage: 0x0001,
        query: |io| race_battery(io, RaceLink::Dongle),
        ..Spec::HEADSET
    },
    Spec {
        key: "akg-n9-hybrid",
        name: "AKG N9 Hybrid",
        vid: 0x0ECB,
        pids: &[0x20D9],
        page: 0xFF13,
        usage: 0x0001,
        query: |io| race_battery(io, RaceLink::Dongle),
        ..Spec::HEADSET
    },
    Spec {
        key: "skullcandy-plyr",
        name: "Skullcandy PLYR",
        vid: 0x34F0,
        pids: &[0x3210],
        page: 0xFF13,
        usage: 0x0001,
        query: |io| race_battery(io, RaceLink::Dongle),
        ..Spec::HEADSET
    },
    Spec {
        key: "skullcandy-crusher-plyr-720",
        name: "Skullcandy Crusher PLYR 720",
        vid: 0x34F0,
        pids: &[0x5310],
        page: 0xFF13,
        usage: 0x0001,
        query: |io| race_battery(io, RaceLink::Dongle),
        ..Spec::HEADSET
    },
    Spec {
        key: "alienware-aw920h",
        name: "Alienware AW920H",
        vid: 0x413C,
        pids: &[0xA515],
        page: 0xFF13,
        usage: 0x0001,
        interface: 5,
        query: alienware_aw920h,
        ..Spec::HEADSET
    },
    // ---- Poly ----
    Spec {
        key: "poly-bt600",
        name: "Poly Voyager (BT600 adapter)",
        vid: 0x047F,
        pids: &[0x02EE, 0x02F7],
        page: 0xFFA2,
        usage: 0x0003,
        query: poly_bt600,
        ..Spec::HEADSET
    },
    // ---- Jabra ----
    // A Link dongle pairs with any Jabra headset, which gives its own name once it answers
    Spec {
        key: "jabra-link-380",
        name: "Jabra headset (Link 380)",
        vid: 0x0B0E,
        pids: &[0x24C7, 0x24C8, 0x24C9, 0x24CA],
        page: 0xFF00,
        usage: 0x0001,
        query: |io| jabra(io, JABRA_BEHIND_DONGLE),
        ..Spec::HEADSET
    },
    Spec {
        key: "jabra-link-390",
        name: "Jabra headset (Link 390)",
        vid: 0x0B0E,
        pids: &[0x2E50, 0x2E51, 0x2E56, 0x2E57],
        page: 0xFF00,
        usage: 0x0005,
        query: |io| jabra(io, JABRA_BEHIND_DONGLE),
        ..Spec::HEADSET
    },
    Spec {
        key: "jabra-evolve2-65-flex",
        name: "Jabra Evolve2 65 Flex",
        vid: 0x0B0E,
        pids: &[0x2517, 0x2518, 0x2519],
        cable_pids: &[0x2517, 0x2518, 0x2519],
        page: 0xFF00,
        usage: 0x0001,
        query: |io| jabra(io, JABRA_ON_USB),
        ..Spec::HEADSET
    },
    Spec {
        key: "jabra-evolve3-75",
        name: "Jabra Evolve3 75",
        vid: 0x0B0E,
        pids: &[0x2546, 0x2547, 0x2548],
        cable_pids: &[0x2546, 0x2547, 0x2548],
        page: 0xFF00,
        usage: 0x0005,
        query: |io| jabra(io, JABRA_ON_USB),
        ..Spec::HEADSET
    },
    // ---- EPOS ----
    Spec {
        key: "epos-h3pro-hybrid",
        name: "EPOS H3PRO Hybrid",
        vid: 0x1395,
        pids: &[0x0303, 0x0101],
        cable_pids: &[0x0101],
        page: 0xFFFF,
        usage: 0x0001,
        query: |io| {
            let dongle = io.product_id() == 0x0303;
            epos_h3pro(io, dongle)
        },
        ..Spec::HEADSET
    },
    // ---- JBL Quantum, the newer models: dongle, then the headset on its cable ----
    // Only the Quantum 650 dongle was read on hardware by the source; the other IDs are the vendor app's list for
    // the same code path. `shared` keeps each of them out of the list unless it really has the FF12 collection:
    // the 950, 950X and 850 dongles may carry it under an ID of their own (2186, 2187 and 2199 in
    // jbl-quantum-linux's list), which no source has read and which are not claimed.
    Spec {
        key: "jbl-quantum-650",
        name: "JBL Quantum 650 Wireless",
        vid: 0x0ECB,
        pids: &[0x2125, 0x2126],
        cable_pids: &[0x2126],
        accept: Some(is_jbl_info_collection),
        shared: true,
        query: jbl_device_info,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-650x",
        name: "JBL Quantum 650X Wireless for Xbox",
        vid: 0x0ECB,
        pids: &[0x213B, 0x213D],
        cable_pids: &[0x213D],
        accept: Some(is_jbl_info_collection),
        shared: true,
        query: jbl_device_info,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-950",
        name: "JBL Quantum 950 Wireless",
        vid: 0x0ECB,
        pids: &[0x2127, 0x2128],
        cable_pids: &[0x2128],
        accept: Some(is_jbl_info_collection),
        shared: true,
        query: jbl_device_info,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-950x",
        name: "JBL Quantum 950X Wireless for Xbox",
        vid: 0x0ECB,
        pids: &[0x213E, 0x2140],
        cable_pids: &[0x2140],
        accept: Some(is_jbl_info_collection),
        shared: true,
        query: jbl_device_info,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-520",
        name: "JBL Quantum 520 Wireless",
        vid: 0x0ECB,
        pids: &[0x218D, 0x218E],
        cable_pids: &[0x218E],
        accept: Some(is_jbl_info_collection),
        shared: true,
        query: jbl_device_info,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-520x",
        name: "JBL Quantum 520X Wireless for Xbox",
        vid: 0x0ECB,
        pids: &[0x21A7, 0x21A9],
        cable_pids: &[0x21A9],
        accept: Some(is_jbl_info_collection),
        shared: true,
        query: jbl_device_info,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-850",
        name: "JBL Quantum 850 Wireless",
        vid: 0x0ECB,
        pids: &[0x219A, 0x219B],
        cable_pids: &[0x219B],
        accept: Some(is_jbl_info_collection),
        shared: true,
        query: jbl_device_info,
        ..Spec::HEADSET
    },
    // ---- JBL Quantum, the older dongles ----
    // The Quantum 350 was read on Windows by the source, the others on Linux or not at all, where the collection
    // that carries the feature reports isn't known: any that has feature reports is tried (on interface 5 where
    // the source names it), and `shared` keeps a dongle without one out of the list.
    Spec {
        key: "jbl-quantum-350",
        name: "JBL Quantum 350 Wireless",
        vid: 0x0ECB,
        pids: &[0x206B],
        page: 0x0001,
        usage: 0x0000,
        interface: 3,
        query: jbl_feature_bytes,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-360",
        name: "JBL Quantum 360 Wireless",
        vid: 0x0ECB,
        pids: &[0x2115],
        interface: 5,
        accept: Some(has_feature_reports),
        shared: true,
        query: jbl_feature_bytes,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-610",
        name: "JBL Quantum 610 Wireless",
        vid: 0x0ECB,
        pids: &[0x205C],
        interface: 5,
        accept: Some(has_feature_reports),
        shared: true,
        query: jbl_feature_bytes,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-810",
        name: "JBL Quantum 810 Wireless",
        vid: 0x0ECB,
        pids: &[0x2069],
        accept: Some(has_feature_reports),
        shared: true,
        query: jbl_feature_bytes,
        ..Spec::HEADSET
    },
    Spec {
        key: "jbl-quantum-910",
        name: "JBL Quantum 910 Wireless",
        vid: 0x0ECB,
        pids: &[0x2088],
        accept: Some(has_feature_reports),
        shared: true,
        query: jbl_feature_bytes,
        ..Spec::HEADSET
    },
    // ---- ASUS ROG ----
    Spec {
        key: "rog-pelta",
        name: "ASUS ROG Pelta",
        vid: 0x0B05,
        pids: &[0x1B84],
        accept: Some(is_rog_collection),
        query: rog_headset,
        ..Spec::HEADSET
    },
    Spec {
        key: "rog-delta-ii",
        name: "ASUS ROG Delta II",
        vid: 0x0B05,
        pids: &[0x1AFA, 0x1D41],
        accept: Some(is_rog_collection),
        query: rog_headset,
        ..Spec::HEADSET
    },
    Spec {
        key: "rog-cetra-speednova",
        name: "ASUS ROG Cetra True Wireless SpeedNova",
        vid: 0x0B05,
        pids: &[0x1AD3],
        accept: Some(is_rog_collection),
        query: rog_cetra_speednova,
        ..Spec::HEADSET
    },
    Spec {
        key: "rog-cetra-open",
        name: "ASUS ROG Cetra Open Wireless",
        vid: 0x0B05,
        pids: &[0x1CAC],
        accept: Some(is_rog_collection),
        query: rog_cetra_open,
        ..Spec::HEADSET
    },
    Spec {
        key: "rog-strix-go-24",
        name: "ASUS ROG Strix Go 2.4",
        vid: 0x0B05,
        pids: &[0x18D6],
        accept: Some(|i| i.feature_length >= 64),
        query: rog_strix_go,
        ..Spec::HEADSET
    },
    // ---- Others ----
    // 0001 is the first product ID of a vendor that makes other USB devices too, and the source goes by the
    // collection alone: listed only while that collection is there
    Spec {
        key: "fractal-scape",
        name: "Fractal Design Scape",
        vid: 0x36BC,
        pids: &[0x0001, 0x2001],
        cable_pids: &[0x2001],
        accept: Some(|i| i.usage_page == 0xFF00 && i.usage == 0x0001 && i.output_length >= 4 && i.input_length >= 24),
        shared: true,
        query: |io| {
            let dongle = io.product_id() == 0x0001;
            fractal_scape(io, dongle)
        },
        ..Spec::HEADSET
    },
    // WalkPlay's vendor ID is on many DACs, and the headset on its cable shows up under the same product ID
    // without the control collection: listed only while that collection is there
    Spec {
        key: "moondrop-gm01-pro",
        name: "MOONDROP GM01 Pro",
        vid: 0x35D8,
        pids: &[0x0133],
        interface: 3,
        accept: Some(|i| i.usage_page == 0x000C && i.output_length >= 4 && i.input_length >= 10),
        shared: true,
        query: moondrop_gm01,
        ..Spec::HEADSET
    },
];

/// The headset answered, but with a level that is no reading.
fn level_unknown(charging: bool) -> State {
    State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, charging) }
}

// ---- Airoha dongles ----

const AUDEZE: u16 = 0x3329;
/// The first Maxwell on its USB-C cable.
const MAXWELL_CABLE: u16 = 0x4B1A;

const REPORT_OUT: u8 = 0x06;
const REPORT_IN: u8 = 0x07;
/// Both reports are 62 bytes long, the report ID included.
const REPORT_LENGTH: usize = 62;
/// "For the headset behind the dongle"; 00 would ask the dongle itself, which has no battery.
const ROUTE_HEADSET: u8 = 0x80;

const RACE_START: u8 = 0x05;
const RACE_REQUEST: u8 = 0x5A;
const RACE_ACK: u8 = 0x5B;
const RACE_RESULT: u8 = 0x5D;
const RACE_BATTERY: u16 = 0x0CD6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RaceLink {
    Dongle,
    /// The dongle of the first Maxwell, which says in its product string whether a headset is linked.
    AudezeDongle,
    /// The headset itself on its USB-C cable, which charges it.
    Cable,
}

#[derive(Debug, PartialEq, Eq)]
enum RaceAnswer {
    Level(u8),
    /// The acknowledgement came with an error status: there will be no result.
    Refused,
}

/// A request for the headset: `06 <n> 80 05 5A <length> <id> <payload>`, n counting the bytes after the 80 and
/// the length the ID and the payload.
fn race_request(id: u16, payload: &[u8]) -> Vec<u8> {
    let mut message = vec![RACE_START, RACE_REQUEST];
    message.extend_from_slice(&(payload.len() as u16 + 2).to_le_bytes());
    message.extend_from_slice(&id.to_le_bytes());
    message.extend_from_slice(payload);
    let mut report = vec![REPORT_OUT, message.len() as u8, ROUTE_HEADSET];
    report.extend_from_slice(&message);
    pad(REPORT_LENGTH, &report)
}

/// The bytes of an input report 07 that are new. The length byte decides: what follows the valid bytes is left
/// over from older answers, and a dongle whose headset is off keeps an old battery result there.
fn race_fragment(report: &[u8]) -> &[u8] {
    if report.len() < 3 || report[0] != REPORT_IN {
        return &[];
    }
    report.get(3..3 + report[1] as usize).unwrap_or(&[])
}

/// The answer to the battery request in a stream of messages, if it is complete. The acknowledgement and the
/// result often come in one report, and other messages (settings the vendor's app asked for, notifications) may
/// stand between them.
fn race_answer(stream: &[u8]) -> Option<RaceAnswer> {
    let id = RACE_BATTERY.to_le_bytes();
    let mut i = 0;
    while i + 4 <= stream.len() {
        let kind = stream[i + 1];
        if stream[i] != RACE_START || !(RACE_REQUEST..=RACE_RESULT).contains(&kind) {
            i += 1;
            continue;
        }
        let length = u16::from_le_bytes([stream[i + 2], stream[i + 3]]) as usize;
        let Some(body) = stream.get(i + 4..i + 4 + length) else { break };
        if body.len() >= 3 && body[..2] == id {
            match kind {
                RACE_RESULT if body.len() >= 5 && body[2] == 0x00 && body[4] <= 100 => {
                    return Some(RaceAnswer::Level(body[4]));
                }
                RACE_ACK if body[2] != 0x00 => return Some(RaceAnswer::Refused),
                _ => {}
            }
        }
        i += 4 + length;
    }
    None
}

/// With no headset linked the first Maxwell's dongle calls itself "Audeze Maxwell Dongle" (the Xbox one "Audeze
/// Maxwell XBOX Dongle") instead of "Audeze Maxwell HID", as HaloBattery measured.
fn names_no_headset(product: Option<&str>) -> bool {
    product.is_some_and(|p| p.trim().to_ascii_lowercase().ends_with("dongle"))
}

fn race_state(level: u8, link: RaceLink) -> State {
    match link {
        // There is no charging flag, but a Maxwell on its cable always charges
        RaceLink::Cable => on(level as i32, true),
        // A headset that was just switched on reports 0 for a moment (HaloBattery, on the Maxwell's Xbox dongle)
        _ if level == 0 => level_unknown(false),
        _ => on(level as i32, false),
    }
}

/// Whether the first Maxwell is plugged in by its cable as well, with the collection its battery is read on.
fn maxwell_is_on_its_cable() -> bool {
    hid::enumerate(|vid, pid| vid == AUDEZE && pid == MAXWELL_CABLE)
        .iter()
        .any(|i| !i.access_denied && i.usage_page == 0xFF13 && i.usage == 0x0001)
}

fn race_battery(io: &mut Io, link: RaceLink) -> win::Result<State> {
    if link == RaceLink::AudezeDongle && names_no_headset(io.info().product.as_deref()) {
        return Ok(off());
    }
    let mut stream = Vec::new();
    // The vendor's app polls the same dongle and may take the answer, so the request is sent once more
    over_control_pipe(io, &race_request(RACE_BATTERY, &[0x00]), 2, 1000, 50, |report| {
        stream.extend_from_slice(race_fragment(report));
        race_answer(&stream).map(|answer| match answer {
            RaceAnswer::Level(level) => race_state(level, link),
            RaceAnswer::Refused => off(),
        })
    })
}

/// Whether an input report 07 carries nothing: `07 00 ..`, which is also what the AW920H's dongle answers until
/// the reply is there.
fn is_empty_report(report: &[u8]) -> bool {
    report.get(1).is_none_or(|&length| length == 0)
}

/// Whether a report read after the request is news. `held` is what the dongle still handed out before the
/// request, if reading didn't take it away: the same report again is that old one. Once the dongle has shown an
/// empty report it has let go of it, and what follows is new even when it reads the same (a level that hasn't
/// moved gives the same bytes).
fn is_news(report: &[u8], held: &mut Vec<u8>) -> bool {
    if is_empty_report(report) {
        held.clear();
        return false;
    }
    held.is_empty() || report != held.as_slice()
}

/// Sends `request` as output report 06 and hands every input report 07 that follows, fetched over the control
/// pipe every `every_ms`, to `read` until it gives a state; off after `attempts` requests, each waited for
/// `wait_ms`. Reading report 07 takes it from the dongle, so what it still holds from earlier exchanges is read
/// away first; a dongle that keeps handing out the same old report is not believed.
fn over_control_pipe(
    io: &mut Io,
    request: &[u8],
    attempts: usize,
    wait_ms: u64,
    every_ms: u64,
    mut read: impl FnMut(&[u8]) -> Option<State>,
) -> win::Result<State> {
    for _ in 0..attempts {
        let mut held = Vec::new();
        for _ in 0..8 {
            let Some(report) = io.get_input_report(REPORT_IN) else { return Ok(off()) };
            if is_empty_report(&report) {
                held.clear();
                break;
            }
            held = report;
        }
        if !io.send(request)? {
            return Ok(off());
        }
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        while Instant::now() < deadline {
            sleep_ms(every_ms);
            let Some(report) = io.get_input_report(REPORT_IN) else { continue };
            if !is_news(&report, &mut held) {
                continue;
            }
            if let Some(state) = read(&report) {
                return Ok(state);
            }
        }
    }
    Ok(off())
}

// ---- Alienware ----

/// A battery reply of the AW920H, checksum and all; None for "nothing yet" (07 00 80) and anything else. Command
/// Center throws away a reply with a level of 0, and so does this.
fn alienware_state(d: &[u8]) -> Option<State> {
    if d.len() < 14 || d[..4] != [REPORT_IN, 0xC0, 0x0A, 0x09] || d[6] == 0 {
        return None;
    }
    if d[3..=12].iter().fold(0, |x, b| x ^ b) != d[13] {
        return None;
    }
    if d[5] == 0 {
        return Some(off()); // the dongle answers for a headset that isn't linked
    }
    // A critical battery has its level in another byte
    let level = if d[8] == 1 { d[11] } else { d[6] };
    (level <= 100).then(|| on(level as i32, d[7] == 1))
}

fn alienware_aw920h(io: &mut Io) -> win::Result<State> {
    // About one request in a hundred is never answered (as the source measured), hence three
    over_control_pipe(io, &pad(REPORT_LENGTH, &[REPORT_OUT, 0xC0, 0x0A]), 3, 180, 15, alienware_state)
}

// ---- Poly BT600 ----

const POLY_ADAPTER: u8 = 0x00;
const POLY_HEADSET: u8 = 0x20;
/// What a reply from the headset carries in place of 20.
const POLY_FROM_HEADSET: u8 = 0x02;
const POLY_HELLO: u8 = 0x01;
const POLY_HELLO_ACK: u8 = 0x08;
const POLY_GET: u8 = 0x02;
const POLY_GET_REPLY: u8 = 0x03;
const POLY_BATTERY: (u8, u8) = (0x0A, 0x1A);
const POLY_SESSION: (u8, u8) = (0x01, 0x02);
/// A hello is followed by a catalogue of settings in many reports, which are skipped.
const POLY_READS: usize = 24;

/// `07 01 01 10 <len> <addr> 00 00 <op> <group> <item>`: the length is 6 for a GET and 7 for a hello.
fn poly_frame(op: u8, (group, item): (u8, u8), to: u8) -> Vec<u8> {
    let length = if op == POLY_GET { 0x06 } else { 0x07 };
    pad(64, &[0x07, 0x01, 0x01, 0x10, length, to, 0x00, 0x00, op, group, item])
}

/// A reply to `op` on this setting from `from`. Catalogue reports have another header, and events of the adapter
/// carry its address.
fn is_poly_reply(d: &[u8], op: u8, (group, item): (u8, u8), from: u8) -> bool {
    d.len() > 10 && d[..4] == [0x07, 0x01, 0x01, 0x10] && d[5] == from && d[8] == op && d[9] == group && d[10] == item
}

/// `<level> <number of levels, 0 included> <charging>`: level 7 of 0B is 70%, as Poly's own software counts.
fn poly_battery(d: &[u8]) -> Option<State> {
    let (level, levels) = (*d.get(11)? as i32, *d.get(12)? as i32);
    (levels >= 2 && level < levels).then(|| on(level * 100 / (levels - 1), d.get(13).is_some_and(|&c| c != 0)))
}

fn poly_bt600(io: &mut Io) -> win::Result<State> {
    io.set_timeout(1000);
    // The adapter answers nothing until its sessions are open, which takes what Windows itself sends when the
    // adapter is plugged in: the output report 06 01, a hello to the adapter and one to the headset. It opens
    // a channel and changes no setting; the sources repeat it before every read.
    let _ = io.send(&[0x06, 0x01]);
    let hello = |io: &mut Io, to: u8, from: u8| {
        io.ask(
            &poly_frame(POLY_HELLO, POLY_SESSION, to),
            |d| is_poly_reply(d, POLY_HELLO_ACK, POLY_SESSION, from),
            POLY_READS,
            false,
        )
    };
    hello(io, POLY_ADAPTER, POLY_ADAPTER)?;
    if hello(io, POLY_HEADSET, POLY_FROM_HEADSET)?.is_none() {
        return Ok(off()); // nobody answers for a headset that is off or away
    }
    let reply = io.ask(
        &poly_frame(POLY_GET, POLY_BATTERY, POLY_HEADSET),
        |d| is_poly_reply(d, POLY_GET_REPLY, POLY_BATTERY, POLY_FROM_HEADSET),
        POLY_READS,
        false,
    )?;
    Ok(reply.map_or_else(off, |r| poly_battery(&r).unwrap_or_else(|| level_unknown(false))))
}

// ---- Jabra ----

const JABRA_HOST: u8 = 0x00;
const JABRA_BEHIND_DONGLE: u8 = 0x04;
const JABRA_ON_USB: u8 = 0x08;
const JABRA_GET: u8 = 1;
const JABRA_RESPONSE: u8 = 3;
const JABRA_REFUSAL: u8 = 0xFE;
/// Command and sub-command: the headset's battery, and its name.
const JABRA_BATTERY: (u8, u8) = (0x12, 0x02);
const JABRA_NAME: (u8, u8) = (0x02, 0x00);

/// Requests are numbered and the reply repeats the number, so a reply to somebody else's request isn't taken.
static JABRA_SEQ: AtomicU8 = AtomicU8::new(0x40);

/// `05 <to> 00 <seq> 46 <command> <sub>`: a GET without a payload, 6 bytes from the address on.
fn jabra_request(to: u8, seq: u8, (command, sub): (u8, u8)) -> Vec<u8> {
    pad(64, &[0x05, to, JABRA_HOST, seq, JABRA_GET << 6 | 6, command, sub])
}

/// A response to our request `seq`: the answer of the device asked, or a refusal from whoever sent one.
fn is_jabra_reply(d: &[u8], from: u8, seq: u8, (command, sub): (u8, u8)) -> bool {
    d.len() >= 7
        && d[0] == 0x05
        && d[1] == JABRA_HOST
        && d[3] == seq
        && d[4] >> 6 == JABRA_RESPONSE
        && (d[5] == JABRA_REFUSAL || (d[2] == from && d[5] == command && d[6] == sub))
}

/// What follows the sub-command: the length counts from the address byte, 6 for an empty payload.
fn jabra_payload(d: &[u8]) -> &[u8] {
    let length = (d[4] & 0x3F) as usize;
    if length < 6 {
        return &[];
    }
    d.get(7..1 + length).unwrap_or(&[])
}

/// `<flags> <percent>`, bit 0 of the flags is charging. A refusal (the headset behind the dongle is off, nothing
/// at that address) is no state.
fn jabra_battery(d: &[u8]) -> Option<State> {
    let payload = jabra_payload(d);
    if d[5] == JABRA_REFUSAL || payload.len() < 2 {
        return None;
    }
    let charging = payload[0] & 0x01 != 0;
    Some(if payload[1] <= 100 { on(payload[1] as i32, charging) } else { level_unknown(charging) })
}

/// `<n> <n characters>`: the name the headset gives itself, "Jabra Evolve2 85".
fn jabra_name(d: &[u8]) -> Option<String> {
    let payload = jabra_payload(d);
    let n = *payload.first()? as usize;
    let name = std::str::from_utf8(payload.get(1..1 + n)?).ok()?.trim_end_matches('\0').trim();
    if d[5] == JABRA_REFUSAL || name.is_empty() || name.chars().any(char::is_control) {
        return None;
    }
    Some(if name.to_ascii_lowercase().starts_with("jabra") { name.to_string() } else { format!("Jabra {name}") })
}

fn jabra_get(io: &mut Io, address: u8, what: (u8, u8)) -> win::Result<Option<Vec<u8>>> {
    let seq = JABRA_SEQ.fetch_add(1, Ordering::Relaxed);
    io.ask(&jabra_request(address, seq, what), |d| is_jabra_reply(d, address, seq, what), 16, false)
}

fn jabra(io: &mut Io, address: u8) -> win::Result<State> {
    io.set_timeout(1000);
    let Some(mut state) = jabra_get(io, address, JABRA_BATTERY)?.and_then(|r| jabra_battery(&r)) else {
        return Ok(off());
    };
    if address == JABRA_BEHIND_DONGLE {
        io.set_timeout(400);
        state.name = jabra_get(io, address, JABRA_NAME)?.and_then(|r| jabra_name(&r));
    }
    Ok(state)
}

// ---- EPOS H3PRO Hybrid ----

/// Report ID and 106 bytes.
const EPOS_REPORT_LENGTH: usize = 107;
const EPOS_REQUEST: u16 = 0x0004;
/// The device that is plugged in: the dongle, or the headset on its cable.
const EPOS_PLUGGED: u16 = 0x0001;
/// The headset behind the dongle.
const EPOS_HEADSET: u16 = 0x0005;
/// Group and command: whether a headset is connected to the dongle, and the battery level.
const EPOS_CONNECTED: (u16, u16) = (0x01, 0x09);
const EPOS_BATTERY: (u16, u16) = (0x12, 0x03);

#[derive(Debug, PartialEq, Eq)]
struct EposFrame {
    kind: u16,
    target: u16,
    command: (u16, u16),
    values: Vec<u16>,
}

/// CRC-8 with polynomial 07, starting from FF.
fn epos_crc(payload: &[u8]) -> u8 {
    payload.iter().fold(0xFF, |crc, &byte| {
        (0..8).fold(crc ^ byte, |crc, _| if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 })
    })
}

fn epos_request(target: u16, (group, command): (u16, u16)) -> Vec<u8> {
    let payload: Vec<u8> = [group, command].iter().flat_map(|w| w.to_le_bytes()).collect();
    let header = [0x0001, EPOS_REQUEST, target, payload.len() as u16, epos_crc(&payload) as u16];
    let mut report = vec![0x01, 0xEB, 0xFE];
    report.extend(header.iter().flat_map(|w| w.to_le_bytes()));
    report.extend_from_slice(&payload);
    pad(EPOS_REPORT_LENGTH, &report)
}

/// The frame in an input report; None for other reports (media keys) and for a frame whose checksum is wrong.
fn epos_frame(d: &[u8]) -> Option<EposFrame> {
    if d.len() < 13 || d[..3] != [0x01, 0xEB, 0xFE] {
        return None;
    }
    let word = |at: usize| u16::from_le_bytes([d[at], d[at + 1]]);
    let length = word(9) as usize;
    let payload = d.get(13..13 + length)?;
    if length < 4 || !length.is_multiple_of(2) || word(11) != epos_crc(payload) as u16 {
        return None;
    }
    let words: Vec<u16> = payload.chunks(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect();
    Some(EposFrame { kind: word(5), target: word(7), command: (words[0], words[1]), values: words[2..].to_vec() })
}

/// The reply to `command` sent to `target`: it comes back from the target minus one. A request for the headset
/// that the dongle couldn't pass on comes back unanswered with the dongle's own target, which is taken too and
/// means the headset is off. Notifications have another type.
fn epos_reply(d: &[u8], target: u16, command: (u16, u16)) -> Option<EposFrame> {
    epos_frame(d).filter(|f| {
        f.kind == EPOS_REQUEST
            && f.command == command
            && (f.target == target - 1 || (target == EPOS_HEADSET && f.target == EPOS_PLUGGED))
    })
}

fn epos_ask(io: &mut Io, target: u16, command: (u16, u16)) -> win::Result<Option<EposFrame>> {
    let reply = io.ask(&epos_request(target, command), |d| epos_reply(d, target, command).is_some(), 8, false)?;
    Ok(reply.and_then(|r| epos_reply(&r, target, command)))
}

/// The level from the battery reply: one value, 0-100 (in steps of 20 on the unit the source had).
fn epos_level(reply: &EposFrame, target: u16) -> Option<i32> {
    (reply.target == target - 1 && reply.values.len() == 1 && reply.values[0] <= 100).then(|| reply.values[0] as i32)
}

fn epos_h3pro(io: &mut Io, dongle: bool) -> win::Result<State> {
    io.set_timeout(1000);
    if dongle {
        // The dongle says whether its headset is there, which saves waiting for an answer it can't pass on
        match epos_ask(io, EPOS_PLUGGED, EPOS_CONNECTED)? {
            Some(connected) if connected.values == [1] => {}
            _ => return Ok(off()),
        }
    }
    let target = if dongle { EPOS_HEADSET } else { EPOS_PLUGGED };
    let reply = epos_ask(io, target, EPOS_BATTERY)?;
    // No charging state is known, on the cable either
    Ok(reply.and_then(|r| epos_level(&r, target)).map_or_else(off, |level| on(level, false)))
}

// ---- JBL Quantum ----

/// The collection of the newer models: page FF12 with input report 86, output 87 and feature 88, 64 bytes each.
fn is_jbl_info_collection(i: &HidInfo) -> bool {
    i.usage_page == 0xFF12 && i.feature_length >= 11 && i.input_length >= 16
}

fn has_feature_reports(i: &HidInfo) -> bool {
    i.feature_length >= 2
}

/// "Get device info" for the battery: `88, 00 DD (the device itself), 01 00 (get), 01 (one packet), 00 (index),
/// 02 00 (payload length), 0D 00 (the battery)`.
const JBL_BATTERY_REQUEST: [u8; 11] = [0x88, 0x00, 0xDD, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x0D, 0x00];

/// The reply `86 00 DD 01 00 01 <index> <length, 2> <status, 2> <feature, 2> <size, 2> <value>`: an error (the top
/// bit of the status), or the battery with a value.
fn is_jbl_info_reply(d: &[u8]) -> bool {
    d.len() >= 16
        && d[..6] == [0x86, 0x00, 0xDD, 0x01, 0x00, 0x01]
        && (d[10] & 0x80 != 0 || (d[11..13] == [0x0D, 0x00] && d[13..15] != [0x00, 0x00]))
}

/// The value is the percentage with the top bit set while charging.
fn jbl_info_state(d: &[u8]) -> Option<State> {
    let level = d[15] & 0x7F;
    (d[10] & 0x80 == 0 && level <= 100).then(|| on(level as i32, d[15] & 0x80 != 0))
}

fn jbl_device_info(io: &mut Io) -> win::Result<State> {
    io.set_timeout(800);
    io.drain()?;
    if !io.set_feature(&JBL_BATTERY_REQUEST) {
        return Ok(off());
    }
    // A dongle whose headset is off doesn't answer at all
    Ok(io.receive(is_jbl_info_reply, 8)?.and_then(|r| jbl_info_state(&r)).unwrap_or_else(off))
}

/// The byte of a one-byte feature report. The firmware answers a report it doesn't have with the one it sent
/// last, so the reply has to start with the ID that was asked for.
fn jbl_feature_byte(report: Option<Vec<u8>>, id: u8) -> Option<u8> {
    report.filter(|r| r.len() >= 2 && r[0] == id).map(|r| r[1])
}

/// The level of a linked headset from feature report 49. It is a number the dongle keeps, not one the headset
/// is asked for, so a 0 is one the dongle hasn't been given yet: a headset that empty is off.
fn jbl_kept_level(level: Option<u8>) -> State {
    match level {
        Some(level @ 1..=100) => on(level as i32, false),
        _ => level_unknown(false),
    }
}

/// The older dongles are only read: feature report 68 is 1 while the headset is linked, and 49 is the level,
/// which stays at its last value while the headset is off - hence the link first.
fn jbl_feature_bytes(io: &mut Io) -> win::Result<State> {
    if jbl_feature_byte(io.get_feature(0x68), 0x68) != Some(1) {
        return Ok(off());
    }
    Ok(jbl_kept_level(jbl_feature_byte(io.get_feature(0x49), 0x49)))
}

// ---- ASUS ROG ----

/// G-Helper's rule: the collection on the vendor page FF00 that takes 64-byte output reports.
fn is_rog_collection(i: &HidInfo) -> bool {
    i.usage_page == 0xFF00 && i.output_length >= 64
}

/// The dongle echoes the request, or answers `CC FF AA` to one it can't serve.
fn is_rog_reply(d: &[u8], sub: u8) -> bool {
    d.len() >= 9 && d[0] == 0xCC && (d[1..3] == [0x12, sub] || d[1..3] == [0xFF, 0xAA])
}

/// A reply with data; FF AA in place of the echo or of the data is a headset that is asleep or off.
fn rog_data(d: &[u8]) -> Option<&[u8]> {
    (d[1] == 0x12 && d[5..7] != [0xFF, 0xAA]).then_some(d)
}

fn rog_ask(io: &mut Io, sub: u8) -> win::Result<Option<Vec<u8>>> {
    // The first answer after the headset reconnects can take about 750 ms
    io.set_timeout(1200);
    let reply = io.ask(&pad(64, &[0xCC, 0x12, sub]), |d| is_rog_reply(d, sub), 4, false)?;
    Ok(reply.filter(|r| rog_data(r).is_some()))
}

/// The lower of the two earbuds; one that reports 0 is in its case or wasn't heard from.
fn lower_bud(left: u8, right: u8) -> Option<i32> {
    [left, right].iter().filter(|&&level| (1..=100).contains(&level)).map(|&level| level as i32).min()
}

/// `CC 12 07`: byte 6 is the level, FE while it isn't known yet. `CC 12 08`: byte 5 is 1 while charging.
fn rog_headset(io: &mut Io) -> win::Result<State> {
    let Some(battery) = rog_ask(io, 0x07)? else { return Ok(off()) };
    let charging = rog_ask(io, 0x08)?.is_some_and(|c| c[5] == 1);
    Ok(if battery[6] <= 100 { on(battery[6] as i32, charging) } else { level_unknown(charging) })
}

/// The same two requests, with the left earbud, the right one and the case in bytes 6-8, and a charging bit
/// for each earbud.
fn rog_cetra_speednova(io: &mut Io) -> win::Result<State> {
    let Some(battery) = rog_ask(io, 0x07)? else { return Ok(off()) };
    let Some(level) = lower_bud(battery[6], battery[7]) else { return Ok(off()) };
    let charging = rog_ask(io, 0x08)?.is_some_and(|c| c[5] & 0x03 != 0);
    Ok(on(level, charging))
}

/// `CC 12 12`: the left earbud, the right one and the case in bytes 5-7. No charging state is known.
fn rog_cetra_open(io: &mut Io) -> win::Result<State> {
    let Some(battery) = rog_ask(io, 0x12)? else { return Ok(off()) };
    Ok(lower_bud(battery[5], battery[6]).map_or_else(off, |level| on(level, false)))
}

/// What the Strix Go 2.4 answers in feature report FF: byte 1 is 1B for a live headset (zeros or FF AA
/// otherwise), bytes 11-12 the battery voltage in mV, byte 9 is 0A while charging. The level is the quarter
/// Armoury Crate shows for that voltage, with G-Helper's thresholds; on the charger the voltage is lifted and
/// says little, so no level is given then.
fn strix_go_state(d: &[u8]) -> Option<State> {
    if d.len() < 13 || d[0] != 0xFF || d[1] != 0x1B {
        return None;
    }
    let mv = u16::from_le_bytes([d[11], d[12]]);
    if d[9] == 0x0A {
        return Some(State::new(true, None, true));
    }
    let level = match mv {
        4100.. => 100,
        3800.. => 75,
        3700.. => 50,
        3500.. => 25,
        _ => 10,
    };
    // A voltage no lithium cell has is no reading
    Some(if (3000..=4400).contains(&mv) {
        State { estimate: true, detail: Some("d.estimate".into()), ..on(level, false) }
    } else {
        level_unknown(false)
    })
}

fn rog_strix_go(io: &mut Io) -> win::Result<State> {
    for _ in 0..2 {
        if !io.set_feature(&[0xFF, 0x08, 0x00, 0xFD, 0x04, 0x12, 0xF1, 0x03, 0x52, 0x01]) {
            return Ok(off());
        }
        sleep_ms(35);
        if let Some(reply) = io.get_feature(0xFF) {
            return Ok(strix_go_state(&reply).unwrap_or_else(off));
        }
    }
    Ok(off())
}

// ---- Fractal Design Scape ----

/// Who a request is for: the dongle, or the headset (through the dongle, or itself on its cable).
const FRACTAL_DONGLE: u8 = 0x11;
const FRACTAL_HEADSET: u8 = 0xF1;
const FRACTAL_GET_STATE: u8 = 0x21;

/// `02 <who> 21` is answered with the same three bytes and the state behind them.
fn is_fractal_state(d: &[u8], who: u8) -> bool {
    d.len() >= 24 && d[..3] == [0x02, who, FRACTAL_GET_STATE]
}

/// Byte 15 of the headset's state is the level, byte 23 is 1 on its USB cable (taken for charging, as the source
/// does: the vendor's configurator shows no charging state).
fn fractal_battery(d: &[u8]) -> Option<State> {
    (d[15] <= 100).then(|| on(d[15] as i32, d[23] == 1))
}

fn fractal_state(io: &mut Io, who: u8) -> win::Result<Option<Vec<u8>>> {
    io.ask(&pad(64, &[0x02, who, FRACTAL_GET_STATE]), |d| is_fractal_state(d, who), 8, false)
}

fn fractal_scape(io: &mut Io, dongle: bool) -> win::Result<State> {
    io.set_timeout(1000);
    if dongle {
        // Byte 4 of the dongle's state: 1 while a headset is linked (on its charging stand it unlinks)
        match fractal_state(io, FRACTAL_DONGLE)? {
            Some(state) if state[4] == 1 => {}
            _ => return Ok(off()),
        }
    }
    Ok(fractal_state(io, FRACTAL_HEADSET)?.and_then(|r| fractal_battery(&r)).unwrap_or_else(off))
}

// ---- MOONDROP GM01 Pro ----

/// The read `4B 80 27 00` is answered `4B 80 27 06 00 00 00 00 <linked> <percent>`; the dongle passes it to the
/// headset and stays silent without one.
fn moondrop_state(d: &[u8]) -> Option<State> {
    (d.len() >= 10 && d[..3] == [0x4B, 0x80, 0x27] && d[3] >= 6 && d[8] != 0 && d[9] <= 100)
        .then(|| on(d[9] as i32, false))
}

fn moondrop_gm01(io: &mut Io) -> win::Result<State> {
    io.set_timeout(400);
    let reply =
        io.ask(&pad(64, &[0x4B, 0x80, 0x27, 0x00]), |d| d.len() >= 10 && d[..3] == [0x4B, 0x80, 0x27], 4, false)?;
    Ok(reply.and_then(|r| moondrop_state(&r)).unwrap_or_else(off))
}

// ---- Sony PULSE 3D ----

const SONY: u16 = 0x054C;
const PULSE_3D_DONGLE: u16 = 0x0D5E;
const PULSE_3D_NAME: &str = "Sony PULSE 3D";

/// What the last status report said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pulse3dStatus {
    connected: bool,
    charging: bool,
    /// In steps of 10; not sent while charging.
    level: Option<i32>,
}

/// The status report `B0 .. .. <level> <flags> ..`, 8 bytes: the level is 0-100, or 80 (hex) while charging;
/// bit 08 of the flags is set while the headset is connected. Reports of the other collections have other IDs.
pub fn pulse_3d_status(report: &[u8]) -> Option<Pulse3dStatus> {
    if report.len() < 8 || report[0] != 0xB0 {
        return None;
    }
    let connected = report[4] & 0x08 != 0;
    Some(match report[3] {
        0x80 => Pulse3dStatus { connected, charging: true, level: None },
        level @ 0..=100 => Pulse3dStatus { connected, charging: false, level: Some(level as i32) },
        _ => Pulse3dStatus { connected, charging: false, level: None },
    })
}

/// The PULSE 3D's dongle (054C:0D5E) answers no request: on its collection FF01:0020 it sends a status report
/// when something changes - the headset is switched on or off, a button is pressed, the charger is plugged in,
/// the battery moves a step. So the collection is kept open, what has queued up is read on every poll, and the
/// newest report is what is shown. Until the first one arrives nothing is known about the headset.
#[derive(Default)]
pub struct Pulse3dProvider {
    link: Option<HidDevice>,
    status: Option<Pulse3dStatus>,
    /// The collections went to the log.
    logged: bool,
}

impl Pulse3dProvider {
    fn forget(&mut self) {
        self.link = None;
        self.status = None;
    }

    /// Reads what the dongle sent since the last poll; the last status report counts.
    fn listen(&mut self) {
        let Some(dev) = &self.link else { return };
        for _ in 0..64 {
            match dev.read(20) {
                Ok(Some(report)) => {
                    let Some(status) = pulse_3d_status(&report) else { continue };
                    if self.status.is_none() {
                        log::write(&format!("{PULSE_3D_NAME}: first status report {}", hex_trimmed(&report)));
                    }
                    self.status = Some(status);
                }
                Ok(None) => return,
                Err(_) => {
                    // The dongle was pulled out
                    self.forget();
                    return;
                }
            }
        }
    }

    /// The reading for what is known about the headset: (online, level, charging, text key).
    pub fn state(status: Option<Pulse3dStatus>) -> (bool, Option<i32>, bool, Option<&'static str>) {
        match status {
            None => (false, None, false, Some("d.levelUnknown")),
            Some(s) if !s.connected => (false, None, false, Some("d.offOrAway")),
            // The level isn't sent while charging, and the one from before is not the level now
            Some(s) if s.charging => (true, None, true, None),
            Some(s) => (true, s.level, false, s.level.is_none().then_some("d.levelUnknown")),
        }
    }
}

impl Provider for Pulse3dProvider {
    fn name(&self) -> &'static str {
        "Sony PULSE 3D"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let found = hid::enumerate(|vid, pid| vid == SONY && pid == PULSE_3D_DONGLE);
        if found.is_empty() {
            self.forget();
            return Vec::new();
        }
        if !self.logged {
            self.logged = true;
            log::write(&format!("{PULSE_3D_NAME}: {} HID collections", found.len()));
            for info in &found {
                log::write(&format!("  mi={} {info}", info.interface_number()));
            }
        }
        let mut reading = DeviceReading::new("headset:sony-pulse-3d", PULSE_3D_NAME, DeviceKind::Headset, t("via.24"));
        let Some(info) = found.iter().find(|i| i.usage_page == 0xFF01 && i.usage == 0x0020) else {
            self.forget();
            reading.detail = Some(t(no_link_reason(&found)));
            return vec![reading];
        };
        if self.link.as_ref().is_none_or(|dev| dev.info.path != info.path) {
            self.forget();
            match HidDevice::open_read_only(info) {
                Ok(dev) => self.link = Some(dev),
                Err(e) => {
                    reading.detail = Some(tf("d.error", &[&e]));
                    return vec![reading];
                }
            }
        }
        self.listen();
        let (online, level, charging, detail) = Self::state(self.status);
        vec![DeviceReading { online, level, charging, detail: detail.map(t), ..reading }]
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        secs(30)
    }
}

#[cfg(test)]
mod tests {
    //! Frames are the ones the sources captured or quote, where they have any: HeadsetControl's tests for the
    //! Maxwell, the EPOS H3PRO and the PULSE 3D, Wireless-Device-Battery-Monitor's for the AW920H and its notes
    //! for the Maxwell 2, the AKG N9 and the Cetra Open.
    use super::*;

    fn unknown(charging: bool) -> State {
        State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, charging) }
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

    /// An input report 07 with these valid bytes and `tail` left over behind them.
    fn report_07(fragment: &[u8], tail: &[u8]) -> Vec<u8> {
        let mut report = vec![0x07, fragment.len() as u8, 0x80];
        report.extend_from_slice(fragment);
        report.extend_from_slice(tail);
        pad(62, &report)
    }

    const ACK: [u8; 7] = [0x05, 0x5B, 0x03, 0x00, 0xD6, 0x0C, 0x00];
    const RESULT_62: [u8; 9] = [0x05, 0x5D, 0x05, 0x00, 0xD6, 0x0C, 0x00, 0x00, 0x3E];

    #[test]
    fn keys_are_lowercase_and_no_two_rows_share_one() {
        let mut keys = std::collections::HashSet::new();
        for spec in SPECS {
            assert!(spec.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", spec.key);
            assert!(keys.insert(spec.key), "{} is the key of two rows", spec.key);
        }
    }

    #[test]
    fn the_battery_request_is_the_one_the_vendor_apps_send() {
        let request = race_request(RACE_BATTERY, &[0x00]);
        assert_eq!(request.len(), 62);
        assert_eq!(request[..10], [0x06, 0x07, 0x80, 0x05, 0x5A, 0x03, 0x00, 0xD6, 0x0C, 0x00]);
        assert!(request[10..].iter().all(|&b| b == 0));
    }

    #[test]
    fn acknowledgement_and_result_come_in_one_report() {
        // The Maxwell 2's report from the notes: 51%
        let raw = pad(
            62,
            &[
                0x07, 0x10, 0x80, 0x05, 0x5B, 0x03, 0x00, 0xD6, 0x0C, 0x00, 0x05, 0x5D, 0x05, 0x00, 0xD6, 0x0C, 0x00,
                0x00, 0x33,
            ],
        );
        assert_eq!(race_answer(race_fragment(&raw)), Some(RaceAnswer::Level(51)));
        // The first Maxwell, 62%
        let both = [ACK.as_slice(), &RESULT_62].concat();
        assert_eq!(race_answer(race_fragment(&report_07(&both, &[]))), Some(RaceAnswer::Level(62)));
    }

    #[test]
    fn the_result_may_come_in_the_report_after_the_acknowledgement() {
        let mut stream = race_fragment(&report_07(&ACK, &[])).to_vec();
        assert_eq!(race_answer(&stream), None);
        // AKG N9 Hybrid: 50%, split in the middle
        let result = [0x05, 0x5D, 0x05, 0x00, 0xD6, 0x0C, 0x00, 0x00, 0x32];
        stream.extend_from_slice(race_fragment(&report_07(&result[..4], &[])));
        assert_eq!(race_answer(&stream), None);
        stream.extend_from_slice(race_fragment(&report_07(&result[4..], &[])));
        assert_eq!(race_answer(&stream), Some(RaceAnswer::Level(50)));
    }

    #[test]
    fn other_messages_around_the_result_are_skipped() {
        // The reply to a setting the vendor's app read, then our result (61%), then another reply
        let mic = [0x05, 0x5B, 0x06, 0x00, 0x01, 0x09, 0x22, 0x00, 0x00, 0xFF];
        let result = [0x05, 0x5D, 0x05, 0x00, 0xD6, 0x0C, 0x00, 0x00, 0x3D];
        let stream = [mic.as_slice(), &result, &mic].concat();
        assert_eq!(race_answer(&stream), Some(RaceAnswer::Level(61)));
        assert_eq!(race_answer(&mic), None);
    }

    #[test]
    fn an_old_result_behind_the_valid_bytes_is_not_read() {
        // What a dongle with its headset off hands out: length 0, the last answer still in the buffer
        let stale = report_07(&[], &RESULT_62);
        assert!(race_fragment(&stale).is_empty());
        assert_eq!(race_answer(race_fragment(&stale)), None);
        // And with only the acknowledgement valid
        let stale = report_07(&ACK, &RESULT_62);
        assert_eq!(race_answer(race_fragment(&stale)), None);
    }

    #[test]
    fn a_report_that_claims_more_bytes_than_it_has_or_is_another_report_is_empty() {
        assert!(race_fragment(&[0x07, 0x3C, 0x80, 0x05, 0x5D]).is_empty());
        let mut other = report_07(&RESULT_62, &[]);
        other[0] = 0x08;
        assert!(race_fragment(&other).is_empty());
        assert!(race_fragment(&[0x07, 0x00]).is_empty());
        // A dongle that passes nothing on answers with the head of the request and no bytes (HaloBattery)
        assert!(race_fragment(&pad(62, &[0x07, 0x00, 0x80])).is_empty());
    }

    #[test]
    fn a_report_the_dongle_held_before_the_request_is_not_news_until_it_has_let_go_of_it() {
        let reply = report_07(&[ACK.as_slice(), &RESULT_62].concat(), &[]);
        let nothing = pad(62, &[0x07, 0x00, 0x80]);
        // Reading took the old report away: the first one with bytes is the answer
        let mut held = Vec::new();
        assert!(!is_news(&nothing, &mut held));
        assert!(is_news(&reply, &mut held));
        // The dongle kept handing out the old report: the same one after the request is still that one
        let mut held = reply.clone();
        assert!(!is_news(&reply, &mut held));
        assert!(is_news(&report_07(&RESULT_62, &[]), &mut held));
        // It showed an empty report in between: the answer of a level that hasn't moved reads the same
        let mut held = reply.clone();
        assert!(!is_news(&reply, &mut held));
        assert!(!is_news(&nothing, &mut held));
        assert!(is_news(&reply, &mut held));
    }

    #[test]
    fn a_refused_request_and_a_level_over_100_are_no_level() {
        assert_eq!(race_answer(&[0x05, 0x5B, 0x03, 0x00, 0xD6, 0x0C, 0x02]), Some(RaceAnswer::Refused));
        assert_eq!(race_answer(&[0x05, 0x5D, 0x05, 0x00, 0xD6, 0x0C, 0x00, 0x00, 0x65]), None);
        // A result with an error status
        assert_eq!(race_answer(&[0x05, 0x5D, 0x05, 0x00, 0xD6, 0x0C, 0x01, 0x00, 0x32]), None);
        // Padding and a message that isn't finished
        assert_eq!(race_answer(&[0x00, 0x00, 0x05, 0x5D, 0x05, 0x00, 0xD6, 0x0C]), None);
    }

    #[test]
    fn a_maxwell_on_its_cable_is_charging_and_zero_from_a_dongle_is_not_a_level() {
        assert_eq!(race_state(62, RaceLink::Dongle), State::new(true, Some(62), false));
        assert_eq!(race_state(62, RaceLink::Cable), State::new(true, Some(62), true));
        assert_eq!(race_state(0, RaceLink::Cable), State::new(true, Some(0), true));
        assert_eq!(race_state(0, RaceLink::AudezeDongle), unknown(false));
    }

    #[test]
    fn the_maxwell_dongle_names_itself_dongle_without_a_headset() {
        assert!(names_no_headset(Some("Audeze Maxwell Dongle")));
        assert!(names_no_headset(Some("Audeze Maxwell XBOX Dongle ")));
        assert!(!names_no_headset(Some("Audeze Maxwell HID")));
        assert!(!names_no_headset(None));
    }

    #[test]
    fn alienware_replies_as_captured() {
        let captured = [0x07, 0xC0, 0x0A, 0x09, 0x00, 0x01, 0x5F, 0x00, 0x00, 0x1C, 0x10, 0x5F, 0x02, 0x06];
        assert_eq!(alienware_state(&pad(62, &captured)), Some(State::new(true, Some(95), false)));
        let another = [0x07, 0xC0, 0x0A, 0x09, 0x00, 0x01, 0x5F, 0x00, 0x00, 0x0B, 0x10, 0x5F, 0x02, 0x11];
        assert_eq!(alienware_state(&pad(62, &another)), Some(State::new(true, Some(95), false)));
    }

    /// An AW920H reply with a right checksum.
    fn aw920h(linked: u8, level: u8, charging: u8, critical: u8, critical_level: u8) -> Vec<u8> {
        let mut r = vec![0x07, 0xC0, 0x0A, 0x09, 0x00, linked, level, charging, critical, 0x1C, 0x10, critical_level];
        r.push(0x02);
        r.push(r[3..=12].iter().fold(0, |x, b| x ^ b));
        pad(62, &r)
    }

    #[test]
    fn alienware_charging_critical_and_unlinked() {
        assert_eq!(alienware_state(&aw920h(1, 40, 1, 0, 40)), Some(State::new(true, Some(40), true)));
        // A critical battery has its level in byte 11
        assert_eq!(alienware_state(&aw920h(1, 5, 0, 1, 3)), Some(State::new(true, Some(3), false)));
        assert_eq!(alienware_state(&aw920h(0, 80, 0, 0, 80)), Some(off()));
    }

    #[test]
    fn alienware_rejects_what_is_not_a_valid_reply() {
        assert_eq!(alienware_state(&pad(62, &[0x07, 0x00, 0x80])), None); // nothing yet
        let mut broken = aw920h(1, 95, 0, 0, 95);
        broken[13] ^= 0x01;
        assert_eq!(alienware_state(&broken), None);
        assert_eq!(alienware_state(&aw920h(1, 0, 0, 0, 0)), None);
        assert_eq!(alienware_state(&aw920h(1, 120, 0, 0, 0)), None);
        assert_eq!(alienware_state(&[0x07, 0xC0, 0x0A, 0x09]), None);
    }

    #[test]
    fn poly_frames_as_documented() {
        assert_eq!(
            poly_frame(POLY_HELLO, POLY_SESSION, POLY_ADAPTER)[..11],
            [0x07, 0x01, 0x01, 0x10, 0x07, 0x00, 0x00, 0x00, 0x01, 0x01, 0x02]
        );
        assert_eq!(
            poly_frame(POLY_HELLO, POLY_SESSION, POLY_HEADSET)[..11],
            [0x07, 0x01, 0x01, 0x10, 0x07, 0x20, 0x00, 0x00, 0x01, 0x01, 0x02]
        );
        let get = poly_frame(POLY_GET, POLY_BATTERY, POLY_HEADSET);
        assert_eq!(get[..11], [0x07, 0x01, 0x01, 0x10, 0x06, 0x20, 0x00, 0x00, 0x02, 0x0A, 0x1A]);
        assert_eq!(get.len(), 64);
    }

    #[test]
    fn poly_level_seven_of_ten_is_seventy_percent() {
        // The value from HeadsetControl's notes: 07 0B 00 02 EE 01
        let reply = pad(
            64,
            &[0x07, 0x01, 0x01, 0x10, 0x0C, 0x02, 0x00, 0x00, 0x03, 0x0A, 0x1A, 0x07, 0x0B, 0x00, 0x02, 0xEE, 0x01],
        );
        assert!(is_poly_reply(&reply, POLY_GET_REPLY, POLY_BATTERY, POLY_FROM_HEADSET));
        assert_eq!(poly_battery(&reply), Some(State::new(true, Some(70), false)));
        let mut charging = reply.clone();
        charging[11] = 0x0A;
        charging[13] = 0x01;
        assert_eq!(poly_battery(&charging), Some(State::new(true, Some(100), true)));
    }

    #[test]
    fn poly_events_catalogues_and_odd_scales_are_not_the_battery() {
        let reply = pad(64, &[0x07, 0x01, 0x01, 0x10, 0x0C, 0x02, 0x00, 0x00, 0x03, 0x0A, 0x1A, 0x07, 0x0B]);
        let mut event = reply.clone();
        event[5] = 0x00; // from the adapter
        assert!(!is_poly_reply(&event, POLY_GET_REPLY, POLY_BATTERY, POLY_FROM_HEADSET));
        let mut catalogue = reply.clone();
        catalogue[3] = 0x11;
        assert!(!is_poly_reply(&catalogue, POLY_GET_REPLY, POLY_BATTERY, POLY_FROM_HEADSET));
        let ack = pad(64, &[0x07, 0x01, 0x01, 0x10, 0x06, 0x02, 0x00, 0x00, 0x08, 0x01, 0x02]);
        assert!(is_poly_reply(&ack, POLY_HELLO_ACK, POLY_SESSION, POLY_FROM_HEADSET));
        assert!(!is_poly_reply(&ack, POLY_GET_REPLY, POLY_BATTERY, POLY_FROM_HEADSET));
        let mut empty = reply.clone();
        empty[12] = 0x01; // one level: nothing to divide by
        assert_eq!(poly_battery(&empty), None);
        let mut over = reply;
        over[11] = 0x0B; // level 11 on a scale of 0-10
        assert_eq!(poly_battery(&over), None);
    }

    #[test]
    fn jabra_battery_request_and_reply() {
        let request = jabra_request(JABRA_BEHIND_DONGLE, 0x01, JABRA_BATTERY);
        assert_eq!(request[..8], [0x05, 0x04, 0x00, 0x01, 0x46, 0x12, 0x02, 0x00]);
        assert_eq!(request.len(), 64);
        // Flags, level and the two bytes of the measurement behind them
        let reply = pad(64, &[0x05, 0x00, 0x04, 0x01, 0xCA, 0x12, 0x02, 0x00, 0x3C, 0x0E, 0x10]);
        assert!(is_jabra_reply(&reply, JABRA_BEHIND_DONGLE, 0x01, JABRA_BATTERY));
        assert_eq!(jabra_battery(&reply), Some(State::new(true, Some(60), false)));
        let charging = pad(64, &[0x05, 0x00, 0x08, 0x01, 0xC8, 0x12, 0x02, 0x41, 0x5F]);
        assert!(is_jabra_reply(&charging, JABRA_ON_USB, 0x01, JABRA_BATTERY));
        assert_eq!(jabra_battery(&charging), Some(State::new(true, Some(95), true)));
    }

    #[test]
    fn jabra_headset_that_is_off_is_refused_by_the_dongle() {
        let refusal = pad(64, &[0x05, 0x00, 0x04, 0x01, 0xC6, 0xFE, 0xF3]);
        assert!(is_jabra_reply(&refusal, JABRA_BEHIND_DONGLE, 0x01, JABRA_BATTERY));
        assert_eq!(jabra_battery(&refusal), None);
        // The dongle may answer in its own name
        let from_dongle = pad(64, &[0x05, 0x00, 0x01, 0x01, 0xC6, 0xFE, 0xF5]);
        assert!(is_jabra_reply(&from_dongle, JABRA_BEHIND_DONGLE, 0x01, JABRA_BATTERY));
        assert_eq!(jabra_battery(&from_dongle), None);
    }

    #[test]
    fn jabra_replies_to_other_requests_are_not_ours() {
        let reply = pad(64, &[0x05, 0x00, 0x04, 0x01, 0xC8, 0x12, 0x02, 0x00, 0x3C]);
        assert!(!is_jabra_reply(&reply, JABRA_BEHIND_DONGLE, 0x02, JABRA_BATTERY)); // another number
        assert!(!is_jabra_reply(&reply, JABRA_ON_USB, 0x01, JABRA_BATTERY)); // another device
        assert!(!is_jabra_reply(&reply, JABRA_BEHIND_DONGLE, 0x01, JABRA_NAME)); // another command
        let event = pad(64, &[0x05, 0x00, 0x04, 0x01, 0x08, 0x12, 0x02, 0x00, 0x3C]);
        assert!(!is_jabra_reply(&event, JABRA_BEHIND_DONGLE, 0x01, JABRA_BATTERY));
        let ours = pad(64, &[0x05, 0x04, 0x00, 0x01, 0x46, 0x12, 0x02]);
        assert!(!is_jabra_reply(&ours, JABRA_BEHIND_DONGLE, 0x01, JABRA_BATTERY));
    }

    #[test]
    fn jabra_level_over_100_and_a_reply_without_a_level() {
        let over = pad(64, &[0x05, 0x00, 0x08, 0x01, 0xC8, 0x12, 0x02, 0x01, 0xC8]);
        assert_eq!(jabra_battery(&over), Some(unknown(true)));
        let short = pad(64, &[0x05, 0x00, 0x08, 0x01, 0xC7, 0x12, 0x02, 0x01]);
        assert_eq!(jabra_battery(&short), None);
    }

    #[test]
    fn jabra_headset_gives_its_name() {
        let mut reply = vec![0x05, 0x00, 0x04, 0x07, 0xC0 | (6 + 17), 0x02, 0x00, 16];
        reply.extend_from_slice(b"Jabra Evolve2 85");
        assert_eq!(jabra_name(&pad(64, &reply)), Some("Jabra Evolve2 85".to_string()));
        let mut bare = vec![0x05, 0x00, 0x04, 0x07, 0xC0 | (6 + 11), 0x02, 0x00, 10];
        bare.extend_from_slice(b"Evolve2 85");
        assert_eq!(jabra_name(&pad(64, &bare)), Some("Jabra Evolve2 85".to_string()));
        assert_eq!(jabra_name(&pad(64, &[0x05, 0x00, 0x04, 0x07, 0xC6, 0xFE, 0xFF])), None);
        assert_eq!(jabra_name(&pad(64, &[0x05, 0x00, 0x04, 0x07, 0xC7, 0x02, 0x00, 0x00])), None);
        // A length that runs past the payload
        assert_eq!(jabra_name(&pad(64, &[0x05, 0x00, 0x04, 0x07, 0xC8, 0x02, 0x00, 0x09, b'J'])), None);
    }

    const EPOS_CONNECTED_REQUEST: [u8; 17] =
        [0x01, 0xEB, 0xFE, 0x01, 0x00, 0x04, 0x00, 0x01, 0x00, 0x04, 0x00, 0x7A, 0x00, 0x01, 0x00, 0x09, 0x00];
    const EPOS_IS_CONNECTED: [u8; 19] = [
        0x01, 0xEB, 0xFE, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x06, 0x00, 0x35, 0x00, 0x01, 0x00, 0x09, 0x00, 0x01,
        0x00,
    ];
    const EPOS_NOT_CONNECTED: [u8; 19] = [
        0x01, 0xEB, 0xFE, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x06, 0x00, 0x20, 0x00, 0x01, 0x00, 0x09, 0x00, 0x00,
        0x00,
    ];
    const EPOS_CABLE_BATTERY_REQUEST: [u8; 17] =
        [0x01, 0xEB, 0xFE, 0x01, 0x00, 0x04, 0x00, 0x01, 0x00, 0x04, 0x00, 0xA5, 0x00, 0x12, 0x00, 0x03, 0x00];
    const EPOS_BATTERY_40: [u8; 19] = [
        0x01, 0xEB, 0xFE, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x06, 0x00, 0x5F, 0x00, 0x12, 0x00, 0x03, 0x00, 0x28,
        0x00,
    ];
    const EPOS_NOTIFICATION: [u8; 21] = [
        0x01, 0xEB, 0xFE, 0x01, 0x00, 0x10, 0x01, 0x00, 0x00, 0x08, 0x00, 0xF4, 0x00, 0x01, 0x00, 0xF0, 0x00, 0x01,
        0x00, 0x01, 0x00,
    ];

    #[test]
    fn epos_requests_match_the_captured_ones() {
        let connected = epos_request(EPOS_PLUGGED, EPOS_CONNECTED);
        assert_eq!(connected.len(), 107);
        assert_eq!(connected[..17], EPOS_CONNECTED_REQUEST);
        assert_eq!(epos_request(EPOS_PLUGGED, EPOS_BATTERY)[..17], EPOS_CABLE_BATTERY_REQUEST);
        let through_dongle = epos_request(EPOS_HEADSET, EPOS_BATTERY);
        assert_eq!(through_dongle[7..13], [0x05, 0x00, 0x04, 0x00, 0xA5, 0x00]);
    }

    #[test]
    fn epos_replies_are_parsed() {
        let battery = epos_reply(&pad(107, &EPOS_BATTERY_40), EPOS_PLUGGED, EPOS_BATTERY).unwrap();
        assert_eq!(battery.values, [0x28]);
        assert_eq!(epos_level(&battery, EPOS_PLUGGED), Some(40));
        let connected = epos_reply(&pad(107, &EPOS_IS_CONNECTED), EPOS_PLUGGED, EPOS_CONNECTED).unwrap();
        assert_eq!(connected.values, [1]);
        let away = epos_reply(&pad(107, &EPOS_NOT_CONNECTED), EPOS_PLUGGED, EPOS_CONNECTED).unwrap();
        assert_eq!(away.values, [0]);
    }

    #[test]
    fn epos_notifications_other_commands_and_bad_checksums_are_skipped() {
        let event = epos_frame(&pad(107, &EPOS_NOTIFICATION)).unwrap();
        assert_eq!((event.kind, event.values.as_slice()), (0x0110, [1, 1].as_slice()));
        assert_eq!(epos_reply(&pad(107, &EPOS_NOTIFICATION), EPOS_PLUGGED, EPOS_CONNECTED), None);
        assert_eq!(epos_reply(&pad(107, &EPOS_IS_CONNECTED), EPOS_PLUGGED, EPOS_BATTERY), None);
        let mut corrupted = pad(107, &EPOS_BATTERY_40);
        corrupted[17] ^= 0x01;
        assert_eq!(epos_frame(&corrupted), None);
        assert_eq!(epos_frame(&[0x05, 0x01]), None); // a media key
                                                     // The reply to the cable's request is not the one to a request sent through the dongle
        assert_eq!(epos_reply(&pad(107, &EPOS_BATTERY_40), EPOS_HEADSET, EPOS_BATTERY), None);
    }

    #[test]
    fn epos_request_that_comes_back_from_the_dongle_means_the_headset_is_off() {
        let returned = epos_request(EPOS_PLUGGED, EPOS_BATTERY);
        let frame = epos_reply(&returned, EPOS_HEADSET, EPOS_BATTERY).unwrap();
        assert_eq!(epos_level(&frame, EPOS_HEADSET), None);
        // Through the dongle the answer comes from target 4
        let mut answer = pad(107, &EPOS_BATTERY_40);
        answer[7] = 0x04;
        answer[17] = 0x3C;
        answer[11] = epos_crc(&answer[13..19]);
        let frame = epos_reply(&answer, EPOS_HEADSET, EPOS_BATTERY).unwrap();
        assert_eq!(epos_level(&frame, EPOS_HEADSET), Some(60));
    }

    #[test]
    fn epos_level_over_100_is_no_reading() {
        let mut answer = pad(107, &EPOS_BATTERY_40);
        answer[17] = 0x65;
        answer[11] = epos_crc(&answer[13..19]);
        let frame = epos_reply(&answer, EPOS_PLUGGED, EPOS_BATTERY).unwrap();
        assert_eq!(epos_level(&frame, EPOS_PLUGGED), None);
    }

    /// A reply of the newer JBL models to "get device info" for `feature`.
    fn jbl_info(status_high: u8, feature: u8, size: u8, value: u8) -> Vec<u8> {
        pad(
            64,
            &[
                0x86,
                0x00,
                0xDD,
                0x01,
                0x00,
                0x01,
                0x00,
                0x05,
                0x00,
                0x00,
                status_high,
                feature,
                0x00,
                size,
                0x00,
                value,
            ],
        )
    }

    #[test]
    fn jbl_value_is_the_level_with_the_charging_bit() {
        let reply = jbl_info(0x00, 0x0D, 0x01, 85);
        assert!(is_jbl_info_reply(&reply));
        assert_eq!(jbl_info_state(&reply), Some(State::new(true, Some(85), false)));
        let charging = jbl_info(0x00, 0x0D, 0x01, 0x80 | 40);
        assert_eq!(jbl_info_state(&charging), Some(State::new(true, Some(40), true)));
        assert_eq!(jbl_info_state(&jbl_info(0x00, 0x0D, 0x01, 0x7F)), None);
    }

    #[test]
    fn jbl_errors_other_features_and_empty_values() {
        let error = jbl_info(0x80, 0x00, 0x00, 0x00);
        assert!(is_jbl_info_reply(&error));
        assert_eq!(jbl_info_state(&error), None);
        assert!(!is_jbl_info_reply(&jbl_info(0x00, 0x0C, 0x01, 85))); // another feature
        assert!(!is_jbl_info_reply(&jbl_info(0x00, 0x0D, 0x00, 85))); // no value
        let mut other = jbl_info(0x00, 0x0D, 0x01, 85);
        other[2] = 0xDE; // not the device itself
        assert!(!is_jbl_info_reply(&other));
        assert!(!is_jbl_info_reply(&[0x86, 0x00, 0xDD, 0x01, 0x00, 0x01]));
    }

    #[test]
    fn jbl_collections() {
        assert!(is_jbl_info_collection(&collection(0xFF12, 0x0001, 64, 64, 64)));
        assert!(!is_jbl_info_collection(&collection(0xFF13, 0x0001, 62, 62, 0)));
        assert!(!is_jbl_info_collection(&collection(0xFF12, 0x0001, 64, 64, 0)));
        assert!(has_feature_reports(&collection(0xFF13, 0x0001, 2, 0, 2)));
        assert!(!has_feature_reports(&collection(0x000C, 0x0001, 3, 0, 0)));
    }

    #[test]
    fn jbl_feature_report_must_be_the_one_asked_for() {
        assert_eq!(jbl_feature_byte(Some(pad(64, &[0x68, 0x01])), 0x68), Some(1));
        assert_eq!(jbl_feature_byte(Some(pad(64, &[0x49, 0x55])), 0x49), Some(85));
        // The firmware repeats the last report for an ID it doesn't have
        assert_eq!(jbl_feature_byte(Some(pad(64, &[0x49, 0x55])), 0x68), None);
        assert_eq!(jbl_feature_byte(Some(vec![0x68]), 0x68), None);
        assert_eq!(jbl_feature_byte(None, 0x68), None);
    }

    #[test]
    fn jbl_level_the_dongle_keeps() {
        // Read on a Quantum 610 dongle (jbl-quantum-linux, docs/QUANTUM610_PACKETS.md): 68 01, 49 5a = 90%
        assert_eq!(jbl_feature_byte(Some(vec![0x68, 0x01]), 0x68), Some(1));
        let level = jbl_feature_byte(Some(vec![0x49, 0x5A]), 0x49);
        assert_eq!(jbl_kept_level(level), State::new(true, Some(90), false));
        // Nothing kept yet, and a byte that is no percentage
        assert_eq!(jbl_kept_level(Some(0)), unknown(false));
        assert_eq!(jbl_kept_level(Some(0x65)), unknown(false));
        assert_eq!(jbl_kept_level(None), unknown(false));
    }

    #[test]
    fn rows_on_ids_other_products_may_carry_are_listed_only_with_their_collection() {
        for key in ["fractal-scape", "moondrop-gm01-pro"] {
            let spec = SPECS.iter().find(|s| s.key == key).unwrap();
            assert!(spec.shared && spec.accept.is_some(), "{key}");
        }
        let scape = SPECS.iter().find(|s| s.key == "fractal-scape").unwrap().accept.unwrap();
        assert!(scape(&collection(0xFF00, 0x0001, 64, 64, 0)));
        assert!(!scape(&collection(0xFF00, 0x0001, 8, 64, 0)));
        assert!(!scape(&collection(0x000C, 0x0001, 64, 64, 0)));
    }

    #[test]
    fn rog_replies_echo_the_request() {
        // The Cetra Open against Armoury Crate: left 99%, right 99%, case 86%
        let cetra = pad(64, &[0xCC, 0x12, 0x12, 0x00, 0x00, 0x63, 0x63, 0x56]);
        assert!(is_rog_reply(&cetra, 0x12));
        assert!(!is_rog_reply(&cetra, 0x07));
        assert!(rog_data(&cetra).is_some());
        assert_eq!(lower_bud(cetra[5], cetra[6]), Some(99));
        let battery = pad(64, &[0xCC, 0x12, 0x07, 0x00, 0x00, 0x05, 0x4D, 0x14, 0x01]);
        assert!(is_rog_reply(&battery, 0x07));
        assert_eq!(rog_data(&battery).map(|d| d[6]), Some(77));
        assert!(!is_rog_reply(&pad(64, &[0xCD, 0x12, 0x07]), 0x07));
    }

    #[test]
    fn rog_headset_that_is_away_answers_ff_aa() {
        let refused = pad(64, &[0xCC, 0xFF, 0xAA]);
        assert!(is_rog_reply(&refused, 0x07));
        assert!(rog_data(&refused).is_none());
        let asleep = pad(64, &[0xCC, 0x12, 0x07, 0x00, 0x00, 0xFF, 0xAA]);
        assert!(is_rog_reply(&asleep, 0x07));
        assert!(rog_data(&asleep).is_none());
    }

    #[test]
    fn an_earbud_that_reports_zero_does_not_count() {
        assert_eq!(lower_bud(80, 64), Some(64));
        assert_eq!(lower_bud(0, 64), Some(64));
        assert_eq!(lower_bud(100, 0), Some(100));
        assert_eq!(lower_bud(0, 0), None);
        assert_eq!(lower_bud(0xFE, 0xFF), None);
    }

    /// A Strix Go 2.4 reply with this voltage.
    fn strix_go(header: u8, mv: u16, power: u8) -> Vec<u8> {
        let mut r = pad(64, &[0xFF, header]);
        r[9] = power;
        r[11..13].copy_from_slice(&mv.to_le_bytes());
        r[13] = 0x40;
        r
    }

    #[test]
    fn strix_go_level_is_a_quarter_from_the_voltage() {
        let estimate =
            |level| State { estimate: true, detail: Some("d.estimate".into()), ..State::new(true, Some(level), false) };
        // The pairs HaloBattery's reporter measured against Armoury Crate: 3831 mV at 75%, 3743 mV at 50%
        assert_eq!(strix_go_state(&strix_go(0x1B, 3831, 0x03)), Some(estimate(75)));
        assert_eq!(strix_go_state(&strix_go(0x1B, 3743, 0x03)), Some(estimate(50)));
        assert_eq!(strix_go_state(&strix_go(0x1B, 4120, 0x03)), Some(estimate(100)));
        assert_eq!(strix_go_state(&strix_go(0x1B, 3560, 0x03)), Some(estimate(25)));
        assert_eq!(strix_go_state(&strix_go(0x1B, 3400, 0x03)), Some(estimate(10)));
    }

    #[test]
    fn strix_go_gives_no_level_on_the_charger_or_from_a_headset_that_is_off() {
        assert_eq!(strix_go_state(&strix_go(0x1B, 3897, 0x0A)), Some(State::new(true, None, true)));
        assert_eq!(strix_go_state(&strix_go(0x00, 0, 0x00)), None);
        assert_eq!(strix_go_state(&pad(64, &[0xFF, 0xFF, 0xAA])), None);
        assert_eq!(strix_go_state(&strix_go(0x1B, 0x4000, 0x03)), Some(unknown(false)));
        assert_eq!(strix_go_state(&[0xFF, 0x1B]), None);
    }

    /// The Scape's state as `who` sends it.
    fn scape(who: u8, linked: u8, level: u8, cable: u8) -> Vec<u8> {
        let mut r = pad(64, &[0x02, who, 0x21]);
        r[4] = linked;
        r[15] = level;
        r[23] = cable;
        r
    }

    #[test]
    fn fractal_state_of_the_headset() {
        let over_the_air = scape(FRACTAL_HEADSET, 0, 73, 0);
        assert!(is_fractal_state(&over_the_air, FRACTAL_HEADSET));
        assert!(!is_fractal_state(&over_the_air, FRACTAL_DONGLE));
        assert_eq!(fractal_battery(&over_the_air), Some(State::new(true, Some(73), false)));
        assert_eq!(fractal_battery(&scape(FRACTAL_HEADSET, 0, 73, 1)), Some(State::new(true, Some(73), true)));
        assert_eq!(fractal_battery(&scape(FRACTAL_HEADSET, 0, 0xFF, 0)), None);
        assert!(!is_fractal_state(&[0x02, FRACTAL_HEADSET, 0x21, 0x00], FRACTAL_HEADSET));
        assert!(!is_fractal_state(&pad(64, &[0x02, FRACTAL_HEADSET, 0x22]), FRACTAL_HEADSET));
    }

    #[test]
    fn moondrop_reply_needs_a_linked_headset() {
        let linked = pad(64, &[0x4B, 0x80, 0x27, 0x06, 0x00, 0x00, 0x00, 0x00, 0x01, 0x32]);
        assert_eq!(moondrop_state(&linked), Some(State::new(true, Some(50), false)));
        let unlinked = pad(64, &[0x4B, 0x80, 0x27, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32]);
        assert_eq!(moondrop_state(&unlinked), None);
        let link_reply = pad(64, &[0x4B, 0x80, 0x31, 0x01, 0x01]);
        assert_eq!(moondrop_state(&link_reply), None);
        let short = pad(64, &[0x4B, 0x80, 0x27, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x32]);
        assert_eq!(moondrop_state(&short), None);
        let over = pad(64, &[0x4B, 0x80, 0x27, 0x06, 0x00, 0x00, 0x00, 0x00, 0x01, 0x80]);
        assert_eq!(moondrop_state(&over), None);
    }

    const PULSE_AT_80: [u8; 8] = [0xB0, 0x05, 0x0A, 0x50, 0xEF, 0x11, 0x11, 0x64];
    const PULSE_AT_30: [u8; 8] = [0xB0, 0x05, 0x00, 0x1E, 0xEF, 0x15, 0x11, 0x64];
    const PULSE_CHARGING: [u8; 8] = [0xB0, 0x05, 0x00, 0x80, 0xED, 0x58, 0x11, 0x64];
    const PULSE_OFF: [u8; 8] = [0xB0, 0x05, 0x14, 0x28, 0xE1, 0x5B, 0x11, 0x64];

    #[test]
    fn pulse_3d_status_reports_as_captured() {
        let status = |connected, charging, level| Some(Pulse3dStatus { connected, charging, level });
        assert_eq!(pulse_3d_status(&PULSE_AT_80), status(true, false, Some(80)));
        assert_eq!(pulse_3d_status(&PULSE_AT_30), status(true, false, Some(30)));
        assert_eq!(pulse_3d_status(&PULSE_CHARGING), status(true, true, None));
        assert_eq!(pulse_3d_status(&PULSE_OFF), status(false, false, Some(40)));
    }

    #[test]
    fn pulse_3d_reports_of_other_collections_are_not_the_status() {
        assert_eq!(pulse_3d_status(&[0x01, 0x00, 0x00, 0x00, 0x00]), None); // a media key
        assert_eq!(pulse_3d_status(&[0x01, 0x05, 0x00, 0x64, 0xEF, 0x15, 0x11, 0x64]), None);
        assert_eq!(pulse_3d_status(&PULSE_AT_80[..7]), None);
    }

    #[test]
    fn pulse_3d_is_unknown_until_it_reports_and_off_when_it_says_so() {
        assert_eq!(Pulse3dProvider::state(None), (false, None, false, Some("d.levelUnknown")));
        assert_eq!(Pulse3dProvider::state(pulse_3d_status(&PULSE_AT_80)), (true, Some(80), false, None));
        assert_eq!(Pulse3dProvider::state(pulse_3d_status(&PULSE_OFF)), (false, None, false, Some("d.offOrAway")));
        // The level from before the charger was plugged in is not shown as the level now
        assert_eq!(Pulse3dProvider::state(pulse_3d_status(&PULSE_CHARGING)), (true, None, true, None));
        let odd = [0xB0, 0x05, 0x00, 0x7F, 0xEF, 0x15, 0x11, 0x64];
        assert_eq!(Pulse3dProvider::state(pulse_3d_status(&odd)), (true, None, false, Some("d.levelUnknown")));
    }
}
