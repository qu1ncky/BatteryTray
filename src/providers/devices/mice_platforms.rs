//! Mice on OEM platforms and vendor web-driver protocols, each a small request and reply on one vendor collection.
//! Byte positions are with the report ID first for numbered reports and for feature reports, and as `Io::data`
//! returns them (no leading 00) for input reports of devices without report IDs.
//!
//! Keychron (the Launcher's protocols):
//!   "8k", FFC1:0001: output B3 06 -> input B4 06 .., [20] bit 7 charging, the rest percent. The Ultra-Link 8K and
//!     Link-KM receivers first say who is paired: output B5 03 -> [1]=03, then from [3] five bytes a slot: vendor,
//!     product (little-endian), 1 = connected.
//!   "1k", 008C:0001, Telink receivers: feature report 51, 20 bytes after the ID. 51 03 -> the same slot list from
//!     [3]; 51 06 -> 51 06 01 xx VV VV PP PP .. [10] bits 0-2 the link (0 cable, 1 receiver), bit 3 mouse linked,
//!     [11] percent, [12] 1 charging, 2 full. VV PP are the mouse's own IDs. Machenike's L7 and Darmoshark's M3
//!     speak the same report (their tools are built on the same platform).
//!   "4k", FF0A:0001, Nordic receivers: 64 bytes without a report ID, CC 00 81 SS .. KK, KK making the sum A1; bit 6
//!     of CC sends the request on to the mouse. 01 .. 01 is answered by the receiver itself, 41 .. 01 by the mouse:
//!     [5] 1 or 3 charging, 2 full, [6] percent. 40 .. 00 -> [4..5] the model.
//!   Nape Pro, FF60:0061 (32 bytes, no ID): B2 -> slots from [2] (big-endian, 4004 = Nape Pro); A7 31 -> A7 31 PP SS.
//!   The Link and Link-KM receivers also come with keyboards, so they are not table rows: `KeychronLinkProvider`
//!   lists a mouse behind one only when the receiver says one is paired.
//! Pulsar X2 V3 generation (Sonix), FFFF:0001: feature report 00 + 64 bytes: 00 08 81 01 .. and the 16-bit sum of
//!   the first 62 in the last two. Read back: 01 08 81 01 xx xx PP once answered (02 / 05 while pending).
//! MCHOSE, FF01:0001. M HUB's older generation: feature report 11 (20 bytes) or 12 (64), every byte inverted:
//!   11 F9 FF.. reads back as 11 F9 and, inverted, vendor, model, firmware (4), flags, percent, charging. Zeros mean
//!   "not yet" or a sleeping mouse. The newer one: output 4D 01 01 00 00 09 00 00 08 (read 0900, xor checksum) ->
//!   input 4D 01 FF LL 00 09 00 00 data.. xor; data [10] link (0 = mouse away), [11] 1 = charging, [12] percent;
//!   a single FF as data = the mouse can't be reached.
//! ATK / VXE "BitMouse", FF05:0001: output 08 KK 72 02 00 TT CC 01 .. (KK the sum of what follows, TT 1 = the mouse
//!   behind a receiver, CC 07 level, 0F charging) -> input 08 72 SS xx xx CC 01 VV.
//! MSI, interface 1: output 0F 20 -> input 0F 20 .. [14] bit 7 charging, the rest percent.
//! AJAZZ AJ179, AULA SC580: 32 bytes without an ID, CC SS NN LL data(27) sum-of-data. CC 10 / 30 = device info: SS
//!   0 ok, 2 mouse away; [4..7] model, [12] 1 charging, 2 full, [13] percent, [14] mouse linked.
//! Delux M800 Pro, FF00:0001: feature 0C 01 20 00 NN -> 0C xx 20 .. [6] linked, [18] percent, [19] 1 charging, 2 full.
//! Niceboy MW400: output 05 24 FA 48 -> input 05 xx FA 48 LO HI CC, the battery voltage in 990/240 mV.
//! Redragon M602-KS (Sinowealth receiver): feature 05 80 -> 05 80 01 linked; 05 90 -> 05 90 11 PP.
//! DAREU A980 Pro: feature 0A SL LL CL CM on the consumer collection, read back once [1] & 0F is 2: slot 0, 00 A0 ->
//!   [7] count, then present flag and product per mouse; slot n, 07 80 -> [7] 1 charging, [8] level, [9] its maximum.
//! Royal Kludge MG6 Pro: output 01 01 02 01 -> input 01 01 02 FF mV(2) percent(2) .. [10] on USB power; bit 4 of FF
//!   says the mouse itself answered.
//!
//! Only reads are sent; nothing here changes a setting or a mode. Protocol facts from Wireless-Device-Battery-Monitor
//! (src/drivers: keychron, machenike, darmoshark, pulsar, mchose, mchose_modern, atk_bitmouse, msi, misc_mice, aula,
//! niceboy, sinowealth_receiver, dareu, royal_kludge .cpp, docs/machenike.md and its issues 12, 54, 153, 154),
//! OpenMouse (mouse-protocol: src/keychron, src/drivers/keychron, src/mchose, src/bitmouse, docs/mchose-protocol.md,
//! docs/redragon-m690-pro-testing.md), keychron-battery-dkms (keychron_battery.c), kolbasky/keychron-battery,
//! darmoshark-m3-configurator (PROTOCOL.md), PulsarBattery (Sonix64Protocol.cs), mchose-linux (PROTOCOL.md),
//! HaloBattery (providers/mchose.py, docs/protocols.md) and aj179-linux (docs/PROTOCOL.md). Own implementation. Not
//! yet verified on hardware by this project, which is why the first exchange with each device goes to the log.

use super::{off, off_because, on, Io, Spec, State};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::providers::{group_by, one_per_id, receiver_name, secs, sleep_ms, Provider};
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

const KEYCHRON: u16 = 0x3434;
const ULTRA_LINK_8K: u16 = 0xD028;
const LINK_KM_TYPE_C: u16 = 0xD029;
const LINK_KM: u16 = 0xD026;
const LINK_1K: [u16; 2] = [0xD030, 0xD031];
const NAPE_CABLE: u16 = 0x0440;
/// What a Link-KM receiver calls the Nape Pro in its slot list.
const NAPE_IN_SLOT: u16 = 0x4004;
const TELINK: u16 = 0x248A;
const ATK_ZERO_CABLE: u16 = 0x1154;

/// Product IDs of Keychron mice as the Launcher lists them: the mouse on its cable, and what a receiver reports for
/// the mouse paired to it.
const KEYCHRON_MICE: &[(u16, &str)] = &[
    (0xD032, "Keychron M3"),
    (0xD033, "Keychron M3"),
    (0xD035, "Keychron M1"),
    (0xD036, "Keychron M3 Mini"),
    (0xD03E, "Keychron M3 Mini"),
    (0xD03B, "Keychron M2"),
    (0xD03D, "Keychron M2 Mini"),
    (0xD03F, "Keychron M6"),
    (0xD044, "Keychron M7"),
    (0xD048, "Keychron M5 8K"),
    (0xD049, "Keychron M6 8K"),
    (0xD050, "Keychron M3 8K"),
    (0xD051, "Keychron M3 Mini 8K"),
    (0xD052, "Keychron M2 8K"),
    (0xD053, "Keychron M4 8K"),
    (0xD054, "Keychron M1 8K"),
    (0xD055, "Keychron M2 Mini 8K"),
    (0xD056, "Keychron M7 8K"),
    (0xD060, "Keychron M6"),
];

/// The model a 4K mouse reports through the 4K Link.
const KEYCHRON_4K_MODELS: &[(u16, &str)] = &[
    (0x0620, "Keychron M3 Mini 4K"),
    (0x0621, "Keychron M4 4K"),
    (0x0622, "Keychron M3 Mini 4K"),
    (0x0623, "Keychron M2 4K"),
    (0x0624, "Keychron M6 4K"),
    (0x07A0, "Keychron M3 4K"),
];

/// The model an M HUB mouse of the older generation reports in its status, which is also its ID on the cable.
const MCHOSE_MODELS: &[(u16, &str)] = &[
    (0x0031, "MCHOSE M7 Ultra"),
    (0x00B0, "MCHOSE L7 Pro"),
    (0x4015, "MCHOSE L7 Pro+"),
    (0x4018, "MCHOSE A7 V2 Pro"),
    (0x4019, "MCHOSE A7 V2 Ultra"),
    (0x4021, "MCHOSE A7 V2 Ultra+"),
    (0x4023, "MCHOSE A7 V2 Pro+"),
];

pub static SPECS: &[Spec] = &[
    // ---- Keychron ----
    // The M5 and M6 8K on the cable and the Ultra-Link 8K are read by keychron-battery-dkms, the M6 (D060) by
    // OpenMouse; the other cable IDs are the rest of the 8K line in the Launcher's list, which pairs to the same
    // receiver. The Link receivers that keyboards share, and the Nape Pro, are read by KeychronLinkProvider below.
    Spec {
        key: "keychron-8k-mouse",
        name: "Keychron 8K mouse",
        vid: KEYCHRON,
        pids: &[ULTRA_LINK_8K, 0xD048, 0xD049, 0xD050, 0xD051, 0xD052, 0xD053, 0xD054, 0xD055, 0xD056, 0xD060],
        cable_pids: &[0xD048, 0xD049, 0xD050, 0xD051, 0xD052, 0xD053, 0xD054, 0xD055, 0xD056, 0xD060],
        page: 0xFFC1,
        usage: 0x0001,
        query: keychron_8k,
        ..Spec::MOUSE
    },
    Spec {
        key: "keychron-4k-mouse",
        name: "Keychron 4K mouse",
        vid: KEYCHRON,
        pids: &[0xD038],
        page: 0xFF0A,
        usage: 0x0001,
        query: keychron_4k,
        ..Spec::MOUSE
    },
    // ---- Telink receivers with the same status report as Keychron's 1K ones ----
    Spec {
        key: "machenike-l7",
        name: "Machenike L7 Arctic / L7 Mix",
        vid: TELINK,
        pids: &[0xFF4C, 0xFF4E],
        cable_pids: &[0xFF4E],
        accept: Some(is_dms_collection),
        shared: true,
        query: machenike_l7,
        ..Spec::MOUSE
    },
    Spec {
        key: "darmoshark-m3",
        name: "Darmoshark M3",
        vid: TELINK,
        pids: &[0xFF30, 0xFF12],
        cable_pids: &[0xFF12],
        accept: Some(is_dms_collection),
        shared: true,
        query: |io| darmoshark(io, 0xFF12),
        ..Spec::MOUSE
    },
    Spec {
        key: "darmoshark-m3s",
        name: "Darmoshark M3S",
        vid: TELINK,
        pids: &[0xFF31],
        accept: Some(is_dms_collection),
        shared: true,
        query: |io| darmoshark(io, 0xFF18),
        ..Spec::MOUSE
    },
    // Nordic's own vendor ID, on a receiver that any OEM may ship
    Spec {
        key: "darmoshark-m3s-pro",
        name: "Darmoshark M3S Pro",
        vid: 0x1915,
        pids: &[0x0723],
        interface: 3,
        accept: Some(|i| i.usage_page == 0xFF0A && i.usage == 0x0001 && i.input_length == 65 && i.output_length == 65),
        shared: true,
        query: darmoshark_m3s_pro,
        ..Spec::MOUSE
    },
    // ---- Pulsar, the Sonix generation. The two dongles also come with the Xlite V4 and the X3. ----
    Spec {
        key: "pulsar-x2-v3",
        name: "Pulsar X2 V3",
        vid: 0x3710,
        pids: &[0x5402, 0x3402],
        cable_pids: &[0x3402],
        accept: Some(is_pulsar_collection),
        query: pulsar,
        ..Spec::MOUSE
    },
    Spec {
        key: "pulsar-x2-v3-es",
        name: "Pulsar X2 V3 eS",
        vid: 0x3710,
        pids: &[0x5403, 0x3406],
        cable_pids: &[0x3406],
        accept: Some(is_pulsar_collection),
        query: pulsar,
        ..Spec::MOUSE
    },
    // ---- MCHOSE (M HUB) ----
    // 5253 is the vendor ID of the L7 / M7 / A7 / K7 line: 1020 its receiver, 0031 the M7 Ultra on its cable
    Spec {
        key: "mchose-mouse",
        name: "MCHOSE L7 / M7 / A7 / K7",
        vid: 0x5253,
        pids: &[0x1020, 0x0031],
        cable_pids: &[0x0031],
        page: 0xFF01,
        usage: 0x0001,
        query: mchose_v2,
        ..Spec::MOUSE
    },
    // Receivers of the A7 V2 line (100B), the A5 V2 Ultra (100C) and the K7 Ultra (1001), then the mice on a cable
    Spec {
        key: "mchose-mouse",
        name: "MCHOSE A7 V2 / A5 V2 / K7",
        vid: 0x3837,
        pids: &[0x100B, 0x100C, 0x1001, 0x4015, 0x4018, 0x4019, 0x4021, 0x4023],
        cable_pids: &[0x4015, 0x4018, 0x4019, 0x4021, 0x4023],
        page: 0xFF01,
        usage: 0x0001,
        query: mchose_v2,
        ..Spec::MOUSE
    },
    // The receivers M HUB lists for its newer mice, then those mice on a cable (A5 V3, K7 V2, G3 V3, V7, A7 V3, R7,
    // K5)
    Spec {
        key: "mchose-mouse-v3",
        name: "MCHOSE A7 V3 / A5 V3 / K7 V2",
        vid: 0x3837,
        pids: &[
            0x1014, 0x1018, 0x1016, 0x102C, 0x102D, 0x4026, 0x4027, 0x4028, 0x4029, 0x402A, 0x4030, 0x4031, 0x4032,
            0x4033, 0x4034, 0x4035, 0x4036, 0x4037, 0x4038,
        ],
        cable_pids: &[
            0x4026, 0x4027, 0x4028, 0x4029, 0x402A, 0x4030, 0x4031, 0x4032, 0x4033, 0x4034, 0x4035, 0x4036, 0x4037,
            0x4038,
        ],
        page: 0xFF01,
        usage: 0x0001,
        query: mchose_v3,
        ..Spec::MOUSE
    },
    // ---- ATK / VXE BitMouse ----
    // The "NK mouse NANO dongle" of the VXE R1 NK and the other NearLink mice
    Spec {
        key: "atk-nearlink-mouse",
        name: "ATK / VXE NearLink",
        vid: 0x373B,
        pids: &[0x1216],
        page: 0xFF05,
        usage: 0x0001,
        query: |io| bitmouse(io, false),
        ..Spec::MOUSE
    },
    Spec {
        key: "atk-zero",
        name: "ATK ZERO",
        vid: 0x373B,
        pids: &[0x1155, 0x124F, ATK_ZERO_CABLE],
        cable_pids: &[ATK_ZERO_CABLE],
        page: 0xFF05,
        usage: 0x0001,
        query: |io| bitmouse(io, true),
        ..Spec::MOUSE
    },
    // ---- MSI ----
    Spec {
        key: "msi-versa-300-elite-wireless",
        name: "MSI VERSA 300 ELITE WIRELESS",
        vid: 0x0DB0,
        pids: &[0x9E22],
        page: 0xFF0D,
        usage: 0x00B0,
        interface: 1,
        query: msi,
        ..Spec::MOUSE
    },
    Spec {
        key: "msi-clutch-gm31-lightweight-wireless",
        name: "MSI CLUTCH GM31 LIGHTWEIGHT WIRELESS",
        vid: 0x0DB0,
        pids: &[0x0D3C, 0x0D3B],
        cable_pids: &[0x0D3B],
        page: 0xFF00,
        usage: 0x0001,
        interface: 1,
        query: msi,
        ..Spec::MOUSE
    },
    // ---- AJAZZ AJ179 and AULA SC580: one device-info frame, the receiver under one vendor ID, the cable under
    // another ----
    Spec {
        key: "ajazz-aj179",
        name: "AJAZZ AJ179",
        vid: 0x249A,
        pids: &[0x5C2F],
        interface: 2,
        accept: Some(is_aj179_collection),
        shared: true,
        query: aj179,
        ..Spec::MOUSE
    },
    Spec {
        key: "ajazz-aj179",
        name: "AJAZZ AJ179",
        vid: TELINK,
        pids: &[0x5C2E, 0x5D2E, 0x5E2E],
        cable_pids: &[0x5C2E, 0x5D2E, 0x5E2E],
        interface: 2,
        accept: Some(is_aj179_collection),
        shared: true,
        query: aj179,
        ..Spec::MOUSE
    },
    Spec {
        key: "aula-sc580",
        name: "AULA SC580",
        vid: 0x089D,
        pids: &[0x062F],
        page: 0xFF02,
        usage: 0x0000,
        query: sc580,
        ..Spec::MOUSE
    },
    Spec {
        key: "aula-sc580",
        name: "AULA SC580",
        vid: 0x088D,
        pids: &[0x062E],
        cable_pids: &[0x062E],
        page: 0xFF02,
        usage: 0x0000,
        query: sc580,
        ..Spec::MOUSE
    },
    // ---- One protocol each ----
    // The first Ninjutso Sora is an M800 Pro inside and comes with the same receiver, which then names it
    Spec {
        key: "delux-m800-pro",
        name: "Delux M800 Pro",
        vid: TELINK,
        pids: &[0x5B2F, 0x5B2E],
        cable_pids: &[0x5B2E],
        accept: Some(|i| i.usage_page == 0xFF00 && i.usage == 0x0001 && i.feature_length >= 33),
        shared: true,
        query: delux_m800,
        ..Spec::MOUSE
    },
    Spec {
        key: "niceboy-mw400",
        name: "Niceboy MW400",
        vid: TELINK,
        pids: &[0xFA02],
        interface: 0,
        accept: Some(|i| i.usage_page == 0x0001 && i.usage == 0x0000 && i.input_length == 33 && i.output_length == 33),
        shared: true,
        query: niceboy_mw400,
        ..Spec::MOUSE
    },
    // Sinowealth's stock receiver ID. The M602-KS has a collection with nothing but the 8-byte feature report;
    // receivers of other mice (the M690 Pro) are shaped differently and aren't read.
    Spec {
        key: "redragon-m602-ks",
        name: "Redragon M602-KS",
        vid: 0x258A,
        pids: &[0x002F],
        interface: 1,
        accept: Some(|i| {
            i.usage_page == 0xFF00
                && i.usage == 0x0001
                && i.feature_length == 8
                && i.input_length == 0
                && i.output_length == 0
        }),
        shared: true,
        query: sinowealth_receiver,
        ..Spec::MOUSE
    },
    Spec {
        key: "dareu-a980-pro",
        name: "DAREU A980 Pro",
        vid: 0x260D,
        pids: &[0x1131, 0x1148],
        cable_pids: &[0x1148],
        accept: Some(|i| i.usage_page == 0x000C && i.usage == 0x0001 && i.feature_length == 65),
        query: dareu_togran,
        ..Spec::MOUSE
    },
    Spec {
        key: "rk-mg6-pro",
        name: "Royal Kludge MG6 Pro",
        vid: 0x2E3C,
        pids: &[0x00A6, 0x00E6],
        page: 0xFF00,
        usage: 0x0001,
        query: rk_mg6_pro,
        ..Spec::MOUSE
    },
    Spec {
        key: "rk-mg6-pro",
        name: "Royal Kludge MG6 Pro",
        vid: 0x2E3C,
        pids: &[0x00A5, 0x00E5],
        cable_pids: &[0x00A5, 0x00E5],
        page: 0xFFF2,
        usage: 0x0001,
        query: rk_mg6_pro,
        ..Spec::MOUSE
    },
];

fn le(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

fn sum(bytes: &[u8]) -> u32 {
    bytes.iter().map(|&b| b as u32).sum()
}

/// A level a device that answered can have. Empty and not charging is a mouse that has switched itself off, so it
/// is its receiver answering for it.
fn battery(level: u8, charging: bool) -> State {
    if level > 100 || (level == 0 && !charging) {
        off()
    } else {
        on(level as i32, charging)
    }
}

/// Bit 7 says charging, the rest is the level.
fn power_byte(b: u8) -> State {
    battery(b & 0x7F, b & 0x80 != 0)
}

fn asleep() -> State {
    off_because("d.asleep")
}

fn brand_mouse(brand: &str) -> String {
    tf("name.brandMouse", &[&brand])
}

/// The slots of a receiver's answer: vendor, product and status of each paired device, five bytes a slot from `at`.
pub fn slots(r: &[u8], at: usize, big_endian: bool) -> Vec<(u16, u16, u8)> {
    let word = |a: u8, b: u8| if big_endian { u16::from_be_bytes([a, b]) } else { u16::from_le_bytes([a, b]) };
    let (paired, _) = r.get(at..).unwrap_or_default().as_chunks::<5>();
    paired.iter().take(3).map(|s| (word(s[0], s[1]), word(s[2], s[3]), s[4])).filter(|s| s.0 != 0 || s.1 != 0).collect()
}

// ---- Keychron ----

const KEYCHRON_TIMEOUT_MS: u64 = 600;
const SLOT_CONNECTED: u8 = 1;

fn keychron_name(pid: u16) -> Option<&'static str> {
    KEYCHRON_MICE.iter().find(|m| m.0 == pid).map(|m| m.1)
}

fn keychron_8k(io: &mut Io) -> win::Result<State> {
    io.set_timeout(KEYCHRON_TIMEOUT_MS);
    let mut pid = io.product_id();
    if pid == ULTRA_LINK_8K {
        // The receiver answers this itself; without a connected mouse there is nobody to ask for the level
        let paired = keychron_8k_slots(io)?.unwrap_or_default();
        match paired.iter().find(|s| s.2 == SLOT_CONNECTED) {
            Some(mouse) => pid = mouse.1,
            None => return Ok(State { name: Some(brand_mouse("Keychron")), ..off() }),
        }
    }
    let name = keychron_name(pid).map_or_else(|| brand_mouse("Keychron"), str::to_string);
    Ok(State { name: Some(name), ..keychron_8k_power(io)? })
}

/// Who is paired to the receiver, or None when it didn't say.
fn keychron_8k_slots(io: &mut Io) -> win::Result<Option<Vec<(u16, u16, u8)>>> {
    Ok(io.ask(&[0xB5, 0x03], |d| d.len() >= 18 && d[1] == 0x03, 8, false)?.map(|r| slots(&r, 3, false)))
}

/// The status of the mouse; one that is asleep behind its receiver doesn't answer.
fn keychron_8k_power(io: &mut Io) -> win::Result<State> {
    Ok(io.ask(&[0xB3, 0x06], is_keychron_8k_status, 8, false)?.map_or_else(off, |r| power_byte(r[20])))
}

pub fn is_keychron_8k_status(d: &[u8]) -> bool {
    d.len() >= 21 && d[0] == 0xB4 && d[1] == 0x06
}

/// A request in the Nordic receivers' frame, report ID first: the command, 81 (one byte asked for), what to read and
/// the byte that brings the sum of the 64 to A1.
pub fn nordic_request(command: u8, what: u8) -> Vec<u8> {
    let mut r = vec![0u8; 65];
    r[1] = command;
    r[3] = 0x81;
    r[4] = what;
    r[64] = 0xA1u8.wrapping_sub(sum(&r[1..64]) as u8);
    r
}

fn is_nordic_reply(d: &[u8], command: u8, what: u8) -> bool {
    d.len() >= 13 && d[0] == command && d[3] == what
}

fn nordic_ask(io: &mut Io, command: u8, what: u8) -> win::Result<Option<Vec<u8>>> {
    io.ask(&nordic_request(command, what), |d| is_nordic_reply(d, command, what), 8, false)
}

fn keychron_4k(io: &mut Io) -> win::Result<State> {
    io.set_timeout(KEYCHRON_TIMEOUT_MS);
    // The receiver's own answer tells the Launcher's two protocols on this collection apart, and they keep the
    // level in different places
    match nordic_ask(io, 0x01, 0x01)? {
        Some(own) if !is_8k_nordic(&own) => {}
        _ => return Ok(off()),
    }
    let Some(power) = nordic_ask(io, 0x41, 0x01)? else { return Ok(off()) };
    let state = keychron_4k_state(&power);
    if !state.online {
        return Ok(state);
    }
    let model = nordic_ask(io, 0x40, 0x00)?.map(|v| le(&v, 4));
    let name = model.and_then(|id| KEYCHRON_4K_MODELS.iter().find(|m| m.0 == id)).map(|m| m.1.to_string());
    Ok(State { name, ..state })
}

/// The Launcher's "8k_nordic" devices put a vendor ID (Keychron's or Lemokey's) where a 4K one has its level.
pub fn is_8k_nordic(own: &[u8]) -> bool {
    matches!(le(own, 6), KEYCHRON | 0x362D)
}

pub fn keychron_4k_state(d: &[u8]) -> State {
    State { full: d[5] == 2, ..battery(d[6], matches!(d[5], 1 | 3)) }
}

/// The Nape Pro itself, or the one a Link-KM passes the request to.
fn nape_battery(io: &mut Io) -> win::Result<State> {
    let r = io.ask(&[0x00, 0xA7, 0x31], |d| d.len() >= 4 && d[0] == 0xA7 && d[1] == 0x31, 8, false)?;
    Ok(r.map_or_else(off, |r| State { full: r[3] == 2, ..battery(r[2], r[3] == 1) }))
}

// ---- The Telink status report ("dms" in the vendors' tools) ----

const DMS_REPORT: u8 = 0x51;
const DMS_BOND: u8 = 0x03;
const DMS_STATUS: u8 = 0x06;
/// Report 51 with its ID. The collection's longest feature report is 1000 bytes, which is what Windows reports.
const DMS_LENGTH: usize = 21;

fn is_dms_collection(i: &HidInfo) -> bool {
    i.usage_page == 0x008C && i.usage == 0x0001 && i.feature_length >= DMS_LENGTH
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmsStatus {
    pub vid: u16,
    pub pid: u16,
    /// 0 on the cable, 1 on the receiver.
    pub mode: u8,
    pub linked: bool,
    pub level: u8,
    pub power: u8,
}

pub fn dms_status(r: &[u8]) -> Option<DmsStatus> {
    if r.len() < 13 || r[0] != DMS_REPORT || r[1] != DMS_STATUS || r[2] != 0x01 {
        return None;
    }
    Some(DmsStatus {
        vid: le(r, 4),
        pid: le(r, 6),
        mode: r[10] & 0x07,
        linked: r[10] & 0x08 != 0,
        level: r[11],
        power: r[12],
    })
}

/// Sends a command and reads the report back until it echoes the command: until then it still holds the answer
/// to the one before. A mouse that is asleep never answers, so the echo never comes.
fn dms_ask(io: &mut Io, command: u8) -> Option<Vec<u8>> {
    if !io.set_feature(&[DMS_REPORT, command]) {
        return None;
    }
    for _ in 0..15 {
        sleep_ms(30);
        let r = io.get_feature(DMS_REPORT)?;
        if r.len() >= 13 && r[0] == DMS_REPORT && r[1] == command {
            return Some(r);
        }
    }
    None
}

fn machenike_l7(io: &mut Io) -> win::Result<State> {
    // Machenike's own tool writes and reads report 51 at its own length, which is how this was checked on an L7;
    // the collection's longest report is far longer
    io.set_feature_length(DMS_LENGTH);
    let cable = io.product_id() == 0xFF4E;
    let status = dms_ask(io, DMS_STATUS).as_deref().and_then(dms_status);
    Ok(status.map_or_else(off, |s| machenike_state(&s, cable)))
}

/// The receiver goes on answering with the last level it heard while the mouse is off or on its cable, and clears
/// the link bit: no reading then. The IDs in the reply are the L7's own on its cable.
pub fn machenike_state(s: &DmsStatus, cable: bool) -> State {
    let ours = (s.vid, s.pid) == (TELINK, 0xFF4E) && s.mode == u8::from(!cable);
    if !ours || s.level > 100 || s.power > 1 || !(cable || s.linked) {
        return off();
    }
    on(s.level as i32, s.power == 1)
}

fn darmoshark(io: &mut Io, mouse: u16) -> win::Result<State> {
    if io.product_id() != mouse {
        // The receiver answers this one itself, at once; the status that follows comes from the mouse, and the
        // echo of this one in between is what proves that status new
        let linked = dms_ask(io, DMS_BOND).is_some_and(|r| slots(&r, 3, false).contains(&(TELINK, mouse, 1)));
        if !linked {
            return Ok(off());
        }
    }
    Ok(match dms_ask(io, DMS_STATUS).as_deref().and_then(dms_status) {
        Some(s) if (s.vid, s.pid) == (TELINK, mouse) => battery(s.level, false),
        _ => off(),
    })
}

fn darmoshark_m3s_pro(io: &mut Io) -> win::Result<State> {
    io.set_timeout(KEYCHRON_TIMEOUT_MS);
    let r = io.ask(&nordic_request(0x41, 0x01), is_m3s_pro_reply, 8, false)?;
    Ok(r.map_or_else(off, |r| battery(r[6], false)))
}

/// 41 00 8C 01 with all 64 bytes adding up to A1.
pub fn is_m3s_pro_reply(d: &[u8]) -> bool {
    d.len() >= 64 && d[0] == 0x41 && d[2] == 0x8C && d[3] == 0x01 && sum(&d[..64]) as u8 == 0xA1
}

/// A Keychron mouse on a 1K receiver: 1 = charging, 2 = full with the cable in.
pub fn keychron_1k_state(s: &DmsStatus) -> State {
    State { full: s.power & 3 == 2, ..battery(s.level, s.power & 3 == 1) }
}

// ---- Keychron's Link receivers, which keyboards use too ----

/// What a Link receiver has paired that is read here, or the Nape Pro on its cable.
pub struct Paired {
    pub key: String,
    pub name: String,
    pub state: State,
}

/// The Keychron Link (3434:D030 Type-A, D031 Type-C) and Link-KM (D026, D029 Type-C) receivers. The same receiver
/// comes with keyboards, which don't answer a battery request, so a table row would list a mouse for every keyboard
/// owner. This asks the receiver what is paired and lists a mouse only when one is: the M-series mice of the
/// Launcher's "1k" protocol on the Link, those of its "8k" protocol on the Link-KM Type-C, the Nape Pro on the
/// Link-KM. The Nape Pro on its cable (3434:0440) is read here as well, so that it stays one entry with the one on
/// the receiver.
#[derive(Default)]
pub struct KeychronLinkProvider {
    /// What this session's log already has.
    logged: HashSet<String>,
}

impl Provider for KeychronLinkProvider {
    fn name(&self) -> &'static str {
        "Keychron Link"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        let found = hid::enumerate(|v, p| v == KEYCHRON && is_link(p));
        for ((_, pid), collections) in group_by(found, |i| (i.vendor_id, i.product_id)) {
            let Some(info) = collections.iter().find(|i| !i.access_denied && is_link_collection(pid, i)) else {
                continue;
            };
            let mut trace = Vec::new();
            // The 1K receivers are only ever sent feature reports, which a handle without access rights carries too
            let opened = HidDevice::open(info).or_else(|e| {
                if LINK_1K.contains(&pid) {
                    HidDevice::open_features_only(info)
                } else {
                    Err(e)
                }
            });
            let paired = opened.and_then(|dev| {
                let mut io = Io::new(dev, &mut trace);
                io.set_timeout(KEYCHRON_TIMEOUT_MS);
                link_query(&mut io, pid)
            });
            let paired = paired.unwrap_or_else(|e| {
                trace.push(format!("error: {e}"));
                None
            });
            self.log_once(info, &trace, paired.as_ref());
            let Some(p) = paired else { continue };
            result.push(DeviceReading {
                online: p.state.online,
                level: p.state.level,
                charging: p.state.charging,
                full_on_charger: p.state.full,
                detail: p.state.detail.as_deref().map(t),
                ..DeviceReading::new(
                    format!("dev:{}", p.key),
                    p.name,
                    DeviceKind::Mouse,
                    t(if pid == NAPE_CABLE { "via.cable" } else { "via.24" }),
                )
            });
        }
        // A Nape Pro on its cable is still paired to its receiver, which finds it off: keep the reading that
        // reached it
        one_per_id(result, |r| r.online)
    }

    fn delay(&self, last: &[DeviceReading]) -> Duration {
        if last.iter().any(|r| r.online) {
            secs(60)
        } else {
            secs(30)
        }
    }
}

impl KeychronLinkProvider {
    /// Logs the exchange with a receiver once for each outcome: nothing of ours paired, a mouse off, a mouse on.
    fn log_once(&mut self, info: &HidInfo, trace: &[String], paired: Option<&Paired>) {
        let outcome = match paired {
            None => "nothing read here is paired",
            Some(p) if p.state.online => "found on",
            Some(_) => "found off",
        };
        if !self.logged.insert(format!("{:04x}{outcome}", info.product_id)) {
            return;
        }
        log::write(&format!("Keychron Link {:04x}:{:04x}: {outcome}", info.vendor_id, info.product_id));
        log::write(&format!("  mi={} {info}: {}", info.interface_number(), trace.join("  ")));
    }
}

fn is_link(pid: u16) -> bool {
    LINK_1K.contains(&pid) || matches!(pid, LINK_KM | LINK_KM_TYPE_C | NAPE_CABLE)
}

/// The collection of a Link receiver that the request for its protocol goes to.
pub fn is_link_collection(pid: u16, i: &HidInfo) -> bool {
    match pid {
        LINK_KM | NAPE_CABLE => i.usage_page == 0xFF60 && i.usage == 0x0061 && i.output_length == 33,
        LINK_KM_TYPE_C => i.usage_page == 0xFFC1 && i.usage == 0x0001 && i.output_length >= 21,
        _ => is_dms_collection(i),
    }
}

/// The first of the paired devices that `ours` accepts, a connected one before the others.
pub fn pick(paired: &[(u16, u16, u8)], ours: impl Fn(u16) -> bool) -> Option<(u16, bool)> {
    let mut mice = paired.iter().filter(|s| s.0 == KEYCHRON && ours(s.1));
    let first = mice.clone().find(|s| s.2 == SLOT_CONNECTED).or_else(|| mice.next())?;
    Some((first.1, first.2 == SLOT_CONNECTED))
}

fn keychron_mouse(pid: u16, state: State) -> Paired {
    Paired { key: format!("keychron-{pid:04x}"), name: keychron_name(pid).unwrap_or("Keychron").to_string(), state }
}

fn link_query(io: &mut Io, pid: u16) -> win::Result<Option<Paired>> {
    let is_mouse = |pid| keychron_name(pid).is_some();
    let nape = |state| Paired { key: "keychron-nape-pro".into(), name: "Keychron Nape Pro".into(), state };
    Ok(match pid {
        NAPE_CABLE => Some(nape(nape_battery(io)?)),
        LINK_KM => {
            let paired = io.ask(&[0x00, 0xB2], |d| d.len() >= 17 && d[0] == 0xB2, 8, false)?;
            let Some((_, connected)) = paired.and_then(|r| pick(&slots(&r, 2, true), |p| p == NAPE_IN_SLOT)) else {
                return Ok(None);
            };
            Some(nape(if connected { nape_battery(io)? } else { off() }))
        }
        LINK_KM_TYPE_C => {
            let Some((mouse, connected)) = keychron_8k_slots(io)?.and_then(|s| pick(&s, is_mouse)) else {
                return Ok(None);
            };
            Some(keychron_mouse(mouse, if connected { keychron_8k_power(io)? } else { off() }))
        }
        _ => {
            let paired = dms_ask(io, DMS_BOND).map(|r| slots(&r, 3, false));
            let Some((mouse, connected)) = paired.and_then(|s| pick(&s, is_mouse)) else { return Ok(None) };
            if !connected {
                return Ok(Some(keychron_mouse(mouse, off())));
            }
            // The status carries the mouse's own IDs; another device's is not this mouse's level
            Some(match dms_ask(io, DMS_STATUS).as_deref().and_then(dms_status) {
                Some(s) if s.vid == KEYCHRON && is_mouse(s.pid) => keychron_mouse(s.pid, keychron_1k_state(&s)),
                _ => keychron_mouse(mouse, off()),
            })
        }
    })
}

// ---- Pulsar (Sonix) ----

const PULSAR_BATTERY: [u8; 3] = [0x08, 0x81, 0x01];

fn is_pulsar_collection(i: &HidInfo) -> bool {
    i.usage_page == 0xFFFF && i.usage == 0x0001 && i.feature_length >= 65
}

/// The battery read, report ID first: the register and the 16-bit sum of the packet's first 62 bytes.
pub fn pulsar_request() -> Vec<u8> {
    let mut r = vec![0u8; 65];
    r[2..5].copy_from_slice(&PULSAR_BATTERY);
    let total = sum(&r[1..63]) as u16;
    r[63..65].copy_from_slice(&total.to_le_bytes());
    r
}

/// The level once the report holds the answer to the battery read; None while it is pending (02, 05) or holds
/// something else.
pub fn pulsar_level(r: &[u8]) -> Option<u8> {
    (r.len() >= 65 && r[1] == 0x01 && r[2..5] == PULSAR_BATTERY).then(|| r[7])
}

fn pulsar(io: &mut Io) -> win::Result<State> {
    let wired = matches!(io.product_id(), 0x3402 | 0x3406);
    // A dongle doesn't say which of the models that ship with it is paired
    let name = (!wired).then(|| brand_mouse("Pulsar"));
    let request = pulsar_request();
    // The vendor collection is there twice and one of the two refuses the request: the engine then tries the other
    for _ in 0..3 {
        if !io.set_feature(&request) {
            break;
        }
        for _ in 0..6 {
            sleep_ms(50);
            let Some(r) = io.get_feature(0x00) else { break };
            if let Some(level) = pulsar_level(&r) {
                // There is no charging flag that works; a mouse on its cable is charging
                return Ok(State { name, ..battery(level, wired) });
            }
        }
    }
    Ok(State { name, ..off() })
}

// ---- MCHOSE ----

const MCHOSE_SHORT: u8 = 0x11;
const MCHOSE_LONG: u8 = 0x12;
const MCHOSE_STATUS: u8 = 0x06;
/// The vendor field of a status: 5253 from the older line, 3837 or 41E4 from the receivers under 3837.
const MCHOSE_VENDORS: [u16; 3] = [0x5253, 0x3837, 0x41E4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MchoseStatus {
    Level {
        model: u16,
        level: u8,
        charging: bool,
    },
    /// The command echoed with nothing behind it: not answered yet, or the mouse is asleep.
    Empty,
    Other,
}

/// The status read on the short or the long report, every byte after the ID inverted, the zeros that pad it too.
pub fn mchose_v2_request(report: u8) -> Vec<u8> {
    let mut r = vec![0xFF; if report == MCHOSE_SHORT { 21 } else { 65 }];
    r[0] = report;
    r[1] = !MCHOSE_STATUS;
    r
}

pub fn mchose_v2_status(r: &[u8]) -> MchoseStatus {
    if r.len() < 13 || !matches!(r[0], MCHOSE_SHORT | MCHOSE_LONG) || r[1] != !MCHOSE_STATUS {
        return MchoseStatus::Other;
    }
    let p: Vec<u8> = r[2..13].iter().map(|b| !b).collect();
    if p.iter().all(|&b| b == 0) {
        return MchoseStatus::Empty;
    }
    if !MCHOSE_VENDORS.contains(&le(&p, 0)) || p[9] > 100 {
        return MchoseStatus::Other;
    }
    MchoseStatus::Level { model: le(&p, 2), level: p[9], charging: p[10] != 0 }
}

/// A name out of a product string: "MCHOSE A7 V3 Ultra+" as it is, "L7 Ultra" with the brand in front.
fn mchose_name(product: Option<&str>) -> String {
    let said = receiver_name::brand(product);
    if said.is_empty() || said.eq_ignore_ascii_case("MCHOSE") {
        brand_mouse("MCHOSE")
    } else if said.to_ascii_uppercase().contains("MCHOSE") {
        said
    } else {
        format!("MCHOSE {said}")
    }
}

fn mchose_v2(io: &mut Io) -> win::Result<State> {
    let name = mchose_name(io.info().product.as_deref());
    let mut echoed = false;
    // M HUB reads the status on the short report; the long one is what answered on some receivers
    for report in [MCHOSE_SHORT, MCHOSE_LONG] {
        let request = mchose_v2_request(report);
        let mut seen = None;
        // The request is sent again for every read: a second read only returns the request. About every other
        // exchange carries the answer. The buffer can be caught half written, so an answer is taken at once only
        // when the one before said the same; failing that, the last one stands.
        for _ in 0..6 {
            if !io.set_feature(&request) {
                break;
            }
            sleep_ms(100);
            match io.get_feature(report).map(|r| mchose_v2_status(&r)) {
                Some(MchoseStatus::Level { model, level, charging }) => {
                    let agrees = seen == Some((model, level, charging));
                    seen = Some((model, level, charging));
                    if agrees {
                        break;
                    }
                }
                Some(MchoseStatus::Empty) => echoed = true,
                _ => {}
            }
        }
        if let Some((model, level, charging)) = seen {
            // The mouse names its model itself, also through a receiver that several models ship with
            let known = MCHOSE_MODELS.iter().find(|m| m.0 == model).map(|m| m.1.to_string());
            return Ok(State { name: Some(known.unwrap_or(name)), ..battery(level, charging) });
        }
        if echoed {
            break;
        }
    }
    Ok(State { name: Some(name), ..if echoed { asleep() } else { off() } })
}

const MCHOSE_V3_REPORT: u8 = 0x4D;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MchoseInfo {
    Info {
        linked: bool,
        charging: bool,
        level: u8,
    },
    /// "Ask again": the receiver can't reach its mouse.
    Busy,
    Other,
}

fn xor(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |x, b| x ^ b)
}

/// Read 0900, the device info: version 1, flags 1 (a checksum follows), no data, the command, and the xor of all
/// from the flags on.
pub fn mchose_v3_request() -> Vec<u8> {
    let mut r = vec![MCHOSE_V3_REPORT, 0x01, 0x01, 0x00, 0x00, 0x09, 0x00, 0x00];
    r.push(xor(&r[2..]));
    r
}

pub fn mchose_v3_info(r: &[u8]) -> MchoseInfo {
    if r.len() < 10 || r[0] != MCHOSE_V3_REPORT || r[1] != 0x01 || r[4] != 0x00 || r[5] != 0x09 {
        return MchoseInfo::Other;
    }
    let end = 8 + r[3] as usize;
    if end >= r.len() {
        return MchoseInfo::Other;
    }
    let data = &r[8..end];
    if data == [0xFF] {
        return MchoseInfo::Busy;
    }
    if data.len() < 14 || r[end] != xor(&r[2..end]) || le(data, 0) != 0x3837 || data[12] > 100 {
        return MchoseInfo::Other;
    }
    MchoseInfo::Info { linked: data[10] != 0, charging: data[11] == 1, level: data[12] }
}

fn mchose_v3(io: &mut Io) -> win::Result<State> {
    // The product string names the model; the ID inside the reply doesn't (an A7 V3 Ultra+ reports the A5's)
    let name = Some(mchose_name(io.info().product.as_deref()));
    // A receiver answers for an idle mouse a second after the request
    io.set_timeout(1500);
    for _ in 0..2 {
        let r = io.ask(&mchose_v3_request(), |d| mchose_v3_info(d) != MchoseInfo::Other, 8, false)?;
        match r.as_deref().map(mchose_v3_info) {
            Some(MchoseInfo::Info { linked: true, charging, level }) => {
                return Ok(State { name, ..battery(level, charging) })
            }
            Some(MchoseInfo::Busy) => {}
            Some(_) => break,
            None => return Ok(State { name, ..off() }),
        }
    }
    Ok(State { name, ..asleep() })
}

// ---- ATK / VXE BitMouse ----

const BITMOUSE_LEVEL: u8 = 0x07;
const BITMOUSE_CHARGING: u8 = 0x0F;

/// Asks `target` (0 the device itself, 1 the mouse behind it) for one byte.
pub fn bitmouse_request(target: u8, command: u8) -> Vec<u8> {
    let mut r = vec![0u8; 64];
    r[0] = 0x08;
    r[2] = 0x72;
    r[3] = 0x02;
    r[5] = target;
    r[6] = command;
    r[7] = 0x01;
    r[1] = sum(&r[2..]) as u8;
    r
}

/// The byte a reply to `command` carries. The ATK ZERO's replies count only with status 00 and one byte declared,
/// the NearLink dongle's unless the status is FF.
pub fn bitmouse_value(d: &[u8], command: u8, zero: bool) -> Option<u8> {
    let reply = d.len() >= 8 && d[0] == 0x08 && d[1] == 0x72 && d[5] == command;
    let good = if zero { d.get(2) == Some(&0x00) && d.get(6) == Some(&0x01) } else { d.get(2) != Some(&0xFF) };
    (reply && good).then(|| d[7])
}

fn bitmouse(io: &mut Io, zero: bool) -> win::Result<State> {
    io.set_timeout(700);
    let target = u8::from(io.product_id() != ATK_ZERO_CABLE);
    let mut ask = |command: u8| {
        io.ask(
            &bitmouse_request(target, command),
            |d| d.len() >= 8 && d[0] == 0x08 && d[1] == 0x72 && d[5] == command,
            8,
            false,
        )
        .map(|r| r.and_then(|r| bitmouse_value(&r, command, zero)))
    };
    // A mouse that is asleep or off doesn't answer through its dongle
    let Some(level) = ask(BITMOUSE_LEVEL)?.filter(|l| (1..=100).contains(l)) else { return Ok(off()) };
    let charging = ask(BITMOUSE_CHARGING)? == Some(1);
    Ok(on(level as i32, charging))
}

// ---- MSI ----

fn msi(io: &mut Io) -> win::Result<State> {
    io.set_timeout(1000);
    // Only this status request is sent. MSI Center first sends 41 80, which puts the mouse under software control:
    // that one never is.
    let r = io.ask(&[0x00, 0x0F, 0x20], is_msi_status, 8, false)?;
    Ok(r.map_or_else(off, |r| power_byte(r[14])))
}

pub fn is_msi_status(d: &[u8]) -> bool {
    d.len() >= 15 && d[0] == 0x0F && d[1] == 0x20
}

// ---- AJAZZ AJ179, AULA SC580 ----

fn is_aj179_collection(i: &HidInfo) -> bool {
    i.input_length == 33 && i.output_length == 33
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceInfo {
    /// `power`: 0 on battery, 1 charging, 2 full.
    Info { model: [u8; 4], power: u8, level: u8, linked: bool },
    /// The receiver says its mouse is asleep or out of range.
    Away,
    /// Busy or a damaged frame: ask again.
    Again,
}

/// The reply to a device-info command, or None when the report is something else (the receivers also send C0 and
/// D1 reports when the mouse dozes off or wakes).
pub fn device_info(d: &[u8], command: u8) -> Option<DeviceInfo> {
    if d.len() < 32 || d[0] != command {
        return None;
    }
    Some(match d[1] {
        0 if d[31] == sum(&d[4..31]) as u8 => {
            DeviceInfo::Info { model: [d[4], d[5], d[6], d[7]], power: d[12], level: d[13], linked: d[14] != 0 }
        }
        2 => DeviceInfo::Away,
        _ => DeviceInfo::Again,
    })
}

fn ask_device_info(io: &mut Io, command: u8, packets: u8) -> win::Result<Option<DeviceInfo>> {
    io.set_timeout(400);
    for _ in 0..3 {
        // No data, so the checksum in the last byte is 0
        let r = io.ask(&[0x00, command, 0x00, packets], |d| device_info(d, command).is_some(), 8, false)?;
        match r.and_then(|r| device_info(&r, command)) {
            Some(DeviceInfo::Again) => sleep_ms(150),
            other => return Ok(other),
        }
    }
    Ok(None)
}

fn aj179(io: &mut Io) -> win::Result<State> {
    let receiver = io.info().vendor_id != TELINK;
    Ok(ask_device_info(io, 0x10, 0x00)?.map_or_else(off, |info| aj179_state(info, receiver)))
}

/// The receiver answers this for itself and goes on reporting a level while the mouse sleeps: only its "linked"
/// byte says whether the mouse is there.
pub fn aj179_state(info: DeviceInfo, receiver: bool) -> State {
    match info {
        DeviceInfo::Info { linked: false, .. } if receiver => asleep(),
        DeviceInfo::Info { model, power, level, .. } => {
            // The same receiver ships with siblings that carry another model code
            let name = (&model != b"M179").then(|| brand_mouse("AJAZZ"));
            State { name, ..battery(level, power != 0) }
        }
        _ => asleep(),
    }
}

fn sc580(io: &mut Io) -> win::Result<State> {
    Ok(ask_device_info(io, 0x30, 0x01)?.map_or_else(off, sc580_state))
}

/// While it charges the mouse reports 100 whatever the charge is, so there is no level then.
pub fn sc580_state(info: DeviceInfo) -> State {
    match info {
        DeviceInfo::Info { linked: true, power: 1, .. } => {
            State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, true) }
        }
        DeviceInfo::Info { linked: true, power, level, .. } => State { full: power == 2, ..battery(level, false) },
        _ => asleep(),
    }
}

// ---- Delux M800 Pro ----

/// The receiver ignores a request that carries the same number as the one before it.
static DELUX_SEQUENCE: AtomicU8 = AtomicU8::new(0);

fn delux_m800(io: &mut Io) -> win::Result<State> {
    let receiver = io.product_id() == 0x5B2F;
    let sora = io.info().product.as_deref().is_some_and(|p| p.to_ascii_lowercase().contains("sora"));
    let name = sora.then(|| "Ninjutso Sora".to_string());
    let mut state = off();
    // A receiver that says "not linked" is asked once more, as it may answer with what it had before
    for _ in 0..2 {
        let number = DELUX_SEQUENCE.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        if !io.set_feature(&[0x0C, 0x01, 0x20, 0x00, number]) {
            break;
        }
        sleep_ms(20);
        let Some(r) = io.get_feature(0x0C) else { break };
        state = delux_state(&r, receiver);
        if state.online {
            break;
        }
    }
    Ok(State { name, ..state })
}

pub fn delux_state(r: &[u8], receiver: bool) -> State {
    if r.len() < 20 || r[0] != 0x0C || r[2] != 0x20 || (receiver && r[6] == 0) {
        return off();
    }
    match r[19] {
        2 => State { full: true, ..on(100, false) },
        state => battery(r[18], state == 1),
    }
}

// ---- Niceboy MW400 ----

fn niceboy_mw400(io: &mut Io) -> win::Result<State> {
    io.set_timeout(500);
    let r = io.ask(&[0x05, 0x24, 0xFA, 0x48], is_niceboy_reply, 8, false)?;
    Ok(r.map_or_else(off, |r| niceboy_state(&r)))
}

pub fn is_niceboy_reply(d: &[u8]) -> bool {
    d.len() >= 7 && d[0] == 0x05 && d[2] == 0xFA && d[3] == 0x48
}

/// The mouse reports its battery voltage, which Niceboy's own tool turns into tens of percent with this table. All
/// zeros is a receiver without its mouse.
pub fn niceboy_state(d: &[u8]) -> State {
    const TABLE: [(i32, i32); 9] =
        [(4200, 100), (4100, 90), (4000, 80), (3900, 70), (3800, 60), (3700, 50), (3500, 40), (3400, 30), (3200, 20)];
    let mv = le(d, 4) as i32 * 990 / 240;
    if !(2500..=4500).contains(&mv) {
        return asleep();
    }
    on(TABLE.iter().find(|row| mv >= row.0).map_or(10, |row| row.1), d[6] != 0)
}

// ---- Redragon M602-KS on Sinowealth's receiver ----

fn sinowealth_receiver(io: &mut Io) -> win::Result<State> {
    let name = Some(receiver_name::mouse(io.info().product.as_deref(), Some("Redragon M602-KS")));
    let mut ask = |command: u8| {
        if !io.set_feature(&[0x05, command]) {
            return None;
        }
        sleep_ms(100);
        io.get_feature(0x05).filter(|r| r.len() >= 4 && r[0] == 0x05 && r[1] == command)
    };
    // The receiver answers the level from its own copy even with the mouse off, so the link is asked first
    if !ask(0x80).is_some_and(|r| r[2] == 0x01) {
        return Ok(State { name, ..off() });
    }
    let level = ask(0x90).filter(|r| r[2] == 0x11).map(|r| r[3]);
    Ok(State { name, ..level.map_or_else(off, |level| battery(level, false)) })
}

// ---- DAREU A980 Pro ----

const TOGRAN_REPORT: u8 = 0x0A;

/// The report holds the finished answer to this class and command.
pub fn is_togran_answer(r: &[u8], class: u8, command: u8) -> bool {
    r.len() >= 11 && r[0] == TOGRAN_REPORT && r[1] & 0x0F == 2 && r[3] == class && r[4] == command
}

fn togran_ask(io: &mut Io, slot: u8, length: u8, class: u8, command: u8) -> Option<Vec<u8>> {
    if !io.set_feature(&[TOGRAN_REPORT, slot << 4, length, class, command]) {
        return None;
    }
    for _ in 0..15 {
        sleep_ms(30);
        let r = io.get_feature(TOGRAN_REPORT)?;
        if is_togran_answer(&r, class, command) {
            return Some(r);
        }
    }
    None
}

/// The first mouse the receiver has on the air: its slot (from 1) and its product ID. A mouse on its cable stays in
/// the list with the flag cleared.
pub fn togran_present(r: &[u8]) -> Option<(u8, u16)> {
    let count = (r[7] as usize).min(5);
    let (mice, _) = r.get(8..8 + 3 * count)?.as_chunks::<3>();
    mice.iter()
        .zip(1u8..)
        .find(|(entry, _)| entry[0] != 0)
        .map(|(entry, slot)| (slot, u16::from_be_bytes([entry[1], entry[2]])))
}

/// The level counts up to a maximum the mouse gives with it.
pub fn togran_battery(r: &[u8]) -> State {
    let max = if r[9] == 0 { 100 } else { r[9] as u32 };
    if r[8] as u32 > max {
        return off();
    }
    battery((r[8] as u32 * 100 / max) as u8, r[7] == 1)
}

fn dareu_togran(io: &mut Io) -> win::Result<State> {
    let mut name = None;
    let slot = if io.product_id() == 0x1148 {
        0
    } else {
        // The receiver takes several mice: the first one present is read
        let Some((slot, mouse)) = togran_ask(io, 0, 6, 0x00, 0xA0).and_then(|r| togran_present(&r)) else {
            return Ok(off());
        };
        if mouse != 0x1148 {
            name = Some(brand_mouse("DAREU"));
        }
        slot
    };
    Ok(State { name, ..togran_ask(io, slot, 4, 0x07, 0x80).map_or_else(off, |r| togran_battery(&r)) })
}

// ---- Royal Kludge MG6 Pro ----

fn rk_mg6_pro(io: &mut Io) -> win::Result<State> {
    io.set_timeout(600);
    let cable = matches!(io.product_id(), 0x00A5 | 0x00E5);
    // Other notifications arrive on the same collection
    let r = io.ask(&[0x00, 0x01, 0x01, 0x02, 0x01], is_mg6_battery, 8, false)?;
    Ok(r.map_or_else(off, |r| mg6_state(&r, cable)))
}

pub fn is_mg6_battery(d: &[u8]) -> bool {
    d.len() >= 11 && d[0] & 0x1F == 0x01 && d[1] & 0x0F == 0x01 && d[2] == 0x02
}

/// Bit 4 of [3] says the mouse itself answered; without it the receiver answered for a mouse that is asleep.
pub fn mg6_state(d: &[u8], cable: bool) -> State {
    if d[3] & 0x10 == 0 {
        return asleep();
    }
    match le(d, 6) {
        level @ 0..=100 => battery(level as u8, cable || d[10] != 0),
        _ => off(),
    }
}

#[cfg(test)]
mod tests {
    //! Requests and replies of the protocols above. Frames marked "captured" are the ones the sources publish:
    //! Wireless-Device-Battery-Monitor (docs/machenike.md, the headers of msi.cpp and niceboy.cpp, its Keychron
    //! and BeiYing tests), aj179-linux (docs/PROTOCOL.md), OpenMouse (docs/mchose-protocol.md), HaloBattery
    //! (docs/protocols.md) and PulsarBattery (Sonix64ProtocolTests.cs). The rest are built to the documented layout.
    use super::*;
    use crate::providers::pad;

    fn h(text: &str) -> Vec<u8> {
        text.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    fn info(page: u16, usage: u16, input: usize, output: usize, feature: usize) -> HidInfo {
        HidInfo {
            usage_page: page,
            usage,
            input_length: input,
            output_length: output,
            feature_length: feature,
            ..HidInfo::default()
        }
    }

    fn charged(state: State) -> State {
        State { full: true, ..state }
    }

    #[test]
    fn an_empty_battery_that_is_not_charging_is_no_reading() {
        assert_eq!(battery(0, false), off());
        assert_eq!(battery(0, true), on(0, true));
        assert_eq!(battery(100, false), on(100, false));
        assert_eq!(battery(101, false), off());
        assert_eq!(battery(0xFF, true), off());
    }

    #[test]
    fn the_top_bit_of_a_power_byte_is_charging() {
        assert_eq!(power_byte(0xE4), on(100, true));
        assert_eq!(power_byte(0x37), on(55, false));
        assert_eq!(power_byte(0x00), off());
        assert_eq!(power_byte(0x80), on(0, true));
        assert_eq!(power_byte(0xFF), off());
    }

    #[test]
    fn a_receiver_lists_what_is_paired_in_five_byte_slots() {
        // An Ultra-Link 8K with an M5 (D048) connected in its second slot
        let r = pad(64, &h("B6 03 02 34 34 49 D0 00 34 34 48 D0 01"));
        assert_eq!(slots(&r, 3, false), [(0x3434, 0xD049, 0), (0x3434, 0xD048, 1)]);
        // A Link-KM names the Nape Pro with the bytes the other way round
        let km = pad(32, &h("B2 03 34 34 40 04 01"));
        assert_eq!(slots(&km, 2, true), [(0x3434, 0x4004, 1)]);
        assert!(slots(&[0xB6, 0x03], 3, false).is_empty());
    }

    #[test]
    fn a_connected_mouse_is_picked_before_a_paired_one() {
        let paired = [(0x3434, 0x0A50, 1), (0x3434, 0xD033, 0), (0x3434, 0xD03F, 1)];
        let is_mouse = |pid| keychron_name(pid).is_some();
        assert_eq!(pick(&paired, is_mouse), Some((0xD03F, true)));
        assert_eq!(pick(&paired[..2], is_mouse), Some((0xD033, false)));
    }

    #[test]
    fn a_link_with_only_a_keyboard_lists_no_mouse() {
        let is_mouse = |pid| keychron_name(pid).is_some();
        assert_eq!(pick(&[(0x3434, 0x0A50, 1)], is_mouse), None);
        assert_eq!(pick(&[], is_mouse), None);
        // Another vendor's device with a mouse's product ID is not it either
        assert_eq!(pick(&[(0x362D, 0xD033, 1)], is_mouse), None);
    }

    #[test]
    fn each_link_is_asked_on_its_own_collection() {
        let raw = info(0xFF60, 0x0061, 33, 33, 0);
        let status = info(0x008C, 0x0001, 33, 33, 1001);
        let mouse = info(0xFFC1, 0x0001, 64, 64, 0);
        assert!(is_link_collection(0xD030, &status) && !is_link_collection(0xD030, &raw));
        assert!(is_link_collection(0xD026, &raw) && !is_link_collection(0xD026, &status));
        assert!(is_link_collection(0x0440, &raw) && !is_link_collection(0x0440, &mouse));
        assert!(is_link_collection(0xD029, &mouse) && !is_link_collection(0xD029, &raw));
        // The Ultra-Link 8K has the 008C collection too, without the feature report
        assert!(!is_dms_collection(&info(0x008C, 0x0001, 33, 33, 0)));
    }

    #[test]
    fn keychron_8k_status_is_report_b4_echoing_the_command() {
        let mut r = pad(64, &[0xB4, 0x06]);
        r[20] = 0xE4;
        assert!(is_keychron_8k_status(&r));
        assert_eq!(power_byte(r[20]), on(100, true));
        assert!(!is_keychron_8k_status(&pad(64, &[0xB4, 0x04])));
        assert!(!is_keychron_8k_status(&pad(64, &[0xB6, 0x06])));
        assert!(!is_keychron_8k_status(&[0xB4, 0x06, 0x00]));
    }

    #[test]
    fn nordic_requests_add_up_to_a1() {
        // The three checksums Wireless-Device-Battery-Monitor's Keychron test expects
        assert_eq!(nordic_request(0x01, 0x01), [h("00 01 00 81 01"), vec![0; 59], vec![0x1E]].concat());
        assert_eq!(nordic_request(0x41, 0x01)[64], 0xDE);
        assert_eq!(nordic_request(0x40, 0x00)[64], 0xE0);
        assert_eq!(sum(&nordic_request(0x41, 0x01)) as u8, 0xA1);
    }

    #[test]
    fn keychron_4k_keeps_the_level_behind_the_charge_state() {
        assert_eq!(keychron_4k_state(&pad(64, &h("41 00 81 01 00 01 49 02"))), on(73, true));
        assert_eq!(keychron_4k_state(&pad(64, &h("41 00 81 01 00 00 61"))), on(97, false));
        assert_eq!(keychron_4k_state(&pad(64, &h("41 00 81 01 00 02 64"))), charged(on(100, false)));
        assert_eq!(keychron_4k_state(&pad(64, &h("41 00 81 01 00 00 00"))), off());
        assert_eq!(keychron_4k_state(&pad(64, &h("41 00 81 01 00 01 FF"))), off());
    }

    #[test]
    fn an_8k_nordic_receiver_is_not_read_as_a_4k_one() {
        assert!(is_8k_nordic(&pad(64, &h("01 00 81 01 00 00 34 34 00 00 01 40"))));
        assert!(is_8k_nordic(&pad(64, &h("01 00 81 01 00 00 2D 36"))));
        assert!(!is_8k_nordic(&pad(64, &h("01 00 81 01 00 00 12 05"))));
    }

    #[test]
    fn darmoshark_m3s_pro_reply_has_its_own_mark_and_a_checksum() {
        let mut r = pad(64, &h("41 00 8C 01 00 00 5A"));
        r[63] = 0xA1u8.wrapping_sub(sum(&r[..63]) as u8);
        assert!(is_m3s_pro_reply(&r));
        assert_eq!(battery(r[6], false), on(90, false));
        r[6] = 0x5B;
        assert!(!is_m3s_pro_reply(&r));
        assert!(!is_m3s_pro_reply(&pad(64, &h("41 00 81 01 00 01 49"))));
    }

    #[test]
    fn machenike_l7_on_its_receiver() {
        // Captured: 64%, the level Machenike's tool showed
        let s = dms_status(&h("51 06 01 00 8A 24 4E FF 05 01 09 40 00 00 00 00 00 00 00 00 00")).unwrap();
        assert_eq!(s, DmsStatus { vid: 0x248A, pid: 0xFF4E, mode: 1, linked: true, level: 64, power: 0 });
        assert_eq!(machenike_state(&s, false), on(64, false));
    }

    #[test]
    fn machenike_l7_charging_on_its_cable() {
        // Captured: 74%
        let s = dms_status(&h("51 06 01 00 8A 24 4E FF 05 01 00 4A 01 00 00 00 00 00 00 00 00")).unwrap();
        assert_eq!(machenike_state(&s, true), on(74, true));
        // The cable's reply is not the receiver's
        assert_eq!(machenike_state(&s, false), off());
    }

    #[test]
    fn machenike_receiver_without_its_mouse_keeps_the_old_level_to_itself() {
        // Captured while the mouse was on its cable: the receiver still says 64%, with the link bit clear
        let s = dms_status(&h("51 06 01 00 8A 24 4E FF 05 01 01 40 01 00 00 00 00 00 00 00 00")).unwrap();
        assert!(!s.linked);
        assert_eq!(machenike_state(&s, false), off());
    }

    #[test]
    fn machenike_reply_of_another_device_is_not_read() {
        let s = dms_status(&h("51 06 01 00 8A 24 12 FF 05 01 09 40 00")).unwrap();
        assert_eq!(machenike_state(&s, false), off());
        let s = dms_status(&h("51 06 01 00 8A 24 4E FF 05 01 09 65 00")).unwrap();
        assert_eq!(machenike_state(&s, false), off());
    }

    #[test]
    fn a_status_must_echo_the_command_and_say_ok() {
        assert_eq!(dms_status(&h("51 03 01 8A 24 12 FF 01 00 00 00 00 00")), None);
        assert_eq!(dms_status(&h("51 06 00 00 34 34 33 D0 00 00 01 57 00")), None);
        assert_eq!(dms_status(&h("51 06 01 00 34 34")), None);
    }

    #[test]
    fn keychron_m3_through_the_1k_receiver() {
        // The layout of Wireless-Device-Battery-Monitor's test: an M3 (D033) at 87%
        let s = dms_status(&h("51 06 01 00 34 34 33 D0 00 00 01 57 00")).unwrap();
        assert_eq!((s.vid, s.pid), (0x3434, 0xD033));
        assert_eq!(keychron_1k_state(&s), on(87, false));
        assert_eq!(keychron_1k_state(&DmsStatus { power: 1, ..s }), on(87, true));
        assert_eq!(keychron_1k_state(&DmsStatus { power: 2, level: 100, ..s }), charged(on(100, false)));
        assert_eq!(keychron_1k_state(&DmsStatus { level: 101, ..s }), off());
    }

    #[test]
    fn pulsar_request_is_the_captured_one() {
        assert_eq!(pulsar_request(), [h("00 00 08 81 01"), vec![0; 58], h("8A 00")].concat());
    }

    #[test]
    fn pulsar_level_counts_once_the_read_is_answered() {
        let answer = |status: u8, level: u8| pad(65, &[0x00, status, 0x08, 0x81, 0x01, 0x00, 0x00, level]);
        assert_eq!(pulsar_level(&answer(0x01, 92)), Some(92));
        // Pending, or the dongle has no mouse
        assert_eq!(pulsar_level(&answer(0x02, 92)), None);
        assert_eq!(pulsar_level(&answer(0x05, 0)), None);
        // The request as it was sent
        assert_eq!(pulsar_level(&pulsar_request()), None);
        // The answer to another register
        assert_eq!(pulsar_level(&pad(65, &[0x00, 0x01, 0x08, 0x82, 0x01, 0x00, 0x00, 0x01])), None);
        assert_eq!(pulsar_level(&answer(0x01, 92)[..40]), None);
    }

    #[test]
    fn pulsar_on_a_dongle_at_zero_is_a_mouse_that_is_off() {
        assert_eq!(battery(0, false), off());
        assert_eq!(battery(0xFF, false), off());
        // On the cable the mouse is charging, whatever its level
        assert_eq!(battery(0, true), on(0, true));
    }

    #[test]
    fn mchose_requests_are_inverted_to_the_last_byte() {
        assert_eq!(mchose_v2_request(0x11), [vec![0x11, 0xF9], vec![0xFF; 19]].concat());
        assert_eq!(mchose_v2_request(0x12), [vec![0x12, 0xF9], vec![0xFF; 63]].concat());
    }

    /// A status with this payload, inverted as the mouse sends it.
    fn mchose_reply(payload: &str) -> Vec<u8> {
        let inverted: Vec<u8> = h(payload).iter().map(|b| !b).collect();
        pad(65, &[vec![0x11, 0xF9], inverted].concat())
    }

    #[test]
    fn mchose_m7_ultra_through_its_receiver() {
        // Captured by HaloBattery: vendor 5253, model 0031, 100%, not charging
        assert_eq!(
            mchose_v2_status(&mchose_reply("53 52 31 00 02 05 07 00 09 64 00 64")),
            MchoseStatus::Level { model: 0x0031, level: 100, charging: false }
        );
        assert_eq!(
            mchose_v2_status(&mchose_reply("E4 41 00 00 01 02 03 04 09 2D 01")),
            MchoseStatus::Level { model: 0, level: 45, charging: true }
        );
    }

    #[test]
    fn mchose_echo_without_an_answer_is_empty() {
        // The request read back, and what the receiver answers for a sleeping mouse
        assert_eq!(mchose_v2_status(&pad(65, &mchose_v2_request(0x11))), MchoseStatus::Empty);
        assert_eq!(mchose_v2_status(&mchose_reply("00 00 00 00 00 00 00 00 00 00 00")), MchoseStatus::Empty);
    }

    #[test]
    fn mchose_status_of_another_command_or_vendor_is_not_read() {
        let mut other = mchose_reply("53 52 31 00 02 05 07 00 09 64 00");
        other[1] = 0xFC; // the bond command, 03
        assert_eq!(mchose_v2_status(&other), MchoseStatus::Other);
        assert_eq!(mchose_v2_status(&mchose_reply("34 12 31 00 02 05 07 00 09 64 00")), MchoseStatus::Other);
        assert_eq!(mchose_v2_status(&mchose_reply("53 52 31 00 02 05 07 00 09 65 00")), MchoseStatus::Other);
        assert_eq!(mchose_v2_status(&[0x11, 0xF9, 0xAC]), MchoseStatus::Other);
        // Not inverted at all: the request as some firmware hands it back
        assert_eq!(mchose_v2_status(&pad(65, &[0x11, 0x06])), MchoseStatus::Other);
    }

    #[test]
    fn mchose_names_come_from_the_product_string() {
        assert_eq!(mchose_name(Some("MCHOSE A7 V3 Ultra+")), "MCHOSE A7 V3 Ultra+");
        assert_eq!(mchose_name(Some("L7 Ultra 8K Dongle")), "MCHOSE L7 Ultra");
    }

    #[test]
    fn mchose_v3_request_is_the_documented_one() {
        assert_eq!(mchose_v3_request(), h("4D 01 01 00 00 09 00 00 08"));
    }

    /// A device-info reply with this data and a checksum that fits.
    fn mchose_v3_reply(data: &str) -> Vec<u8> {
        let data = h(data);
        let mut r = [vec![0x4D, 0x01, 0x01, data.len() as u8, 0x00, 0x09, 0x00, 0x00], data].concat();
        r.push(xor(&r[2..]));
        pad(64, &r)
    }

    #[test]
    fn mchose_a7_v3_ultra_plus_through_its_receiver() {
        // The data block OpenMouse captured behind receiver 3837:1018: linked, charging, 85%
        let r = mchose_v3_reply("37 38 26 40 04 00 00 00 00 10 02 01 55 00 08 E4");
        assert_eq!(mchose_v3_info(&r), MchoseInfo::Info { linked: true, charging: true, level: 85 });
    }

    #[test]
    fn mchose_v3_receiver_without_its_mouse() {
        // Captured by Wireless-Device-Battery-Monitor with the mouse off
        assert_eq!(mchose_v3_info(&pad(64, &h("4D 01 01 01 00 09 00 00 FF 00"))), MchoseInfo::Busy);
        let unlinked = mchose_v3_reply("37 38 26 40 04 00 00 00 00 10 00 00 55 00");
        assert_eq!(mchose_v3_info(&unlinked), MchoseInfo::Info { linked: false, charging: false, level: 85 });
    }

    #[test]
    fn mchose_v3_reply_must_fit_in_every_part() {
        let good = mchose_v3_reply("37 38 26 40 04 00 00 00 00 10 02 00 55 00");
        assert_eq!(mchose_v3_info(&good), MchoseInfo::Info { linked: true, charging: false, level: 85 });
        let mut damaged = good.clone();
        damaged[20] = 0x54;
        assert_eq!(mchose_v3_info(&damaged), MchoseInfo::Other);
        // Another command, another vendor, a level that can't be, a refusal (command 0000)
        let mut settings = good.clone();
        settings[4] = 0x02;
        assert_eq!(mchose_v3_info(&settings), MchoseInfo::Other);
        assert_eq!(mchose_v3_info(&mchose_v3_reply("34 12 26 40 04 00 00 00 00 10 02 00 55 00")), MchoseInfo::Other);
        assert_eq!(mchose_v3_info(&mchose_v3_reply("37 38 26 40 04 00 00 00 00 10 02 00 FE 00")), MchoseInfo::Other);
        assert_eq!(mchose_v3_info(&pad(64, &h("4D 01 00 00 00 00 00 FF"))), MchoseInfo::Other);
        assert_eq!(mchose_v3_info(&h("4D 01 01 0E 00 09 00 00 37 38")), MchoseInfo::Other);
    }

    #[test]
    fn bitmouse_request_is_the_captured_one() {
        // The level asked of the mouse behind the NearLink dongle
        assert_eq!(bitmouse_request(1, 0x07), pad(64, &h("08 7D 72 02 00 01 07 01")));
        assert_eq!(bitmouse_request(0, 0x0F)[..8], h("08 84 72 02 00 00 0F 01"));
    }

    #[test]
    fn bitmouse_reply_is_matched_by_its_command() {
        let level = pad(64, &h("08 72 00 3A 01 07 01 3C"));
        assert_eq!(bitmouse_value(&level, 0x07, false), Some(60));
        assert_eq!(bitmouse_value(&level, 0x07, true), Some(60));
        assert_eq!(bitmouse_value(&level, 0x0F, false), None);
        assert_eq!(bitmouse_value(&h("08 72 00 3A 01 07 01"), 0x07, false), None);
    }

    #[test]
    fn bitmouse_error_is_no_value() {
        assert_eq!(bitmouse_value(&pad(64, &h("08 72 FF 3A 01 07 01 3C")), 0x07, false), None);
        // The ZERO's replies count only when they are fresh and one byte long
        assert_eq!(bitmouse_value(&pad(64, &h("08 72 01 3A 01 07 01 3C")), 0x07, true), None);
        assert_eq!(bitmouse_value(&pad(64, &h("08 72 00 3A 01 07 02 3C")), 0x07, true), None);
        assert_eq!(bitmouse_value(&pad(64, &h("08 72 01 3A 01 07 01 3C")), 0x07, false), Some(60));
    }

    #[test]
    fn msi_clutch_gm31_at_86_percent() {
        // Captured while MSI Center showed 86%
        let r = pad(64, &h("0F 20 00 00 23 05 7A 91 00 00 00 00 05 80 56"));
        assert!(is_msi_status(&r));
        assert_eq!(power_byte(r[14]), on(86, false));
    }

    #[test]
    fn msi_refusal_and_sleep_are_no_reading() {
        // What the dongle answers to a request it doesn't know
        assert!(!is_msi_status(&pad(64, &h("FF AA 00 00 0F 20"))));
        assert!(!is_msi_status(&h("0F 20 00 00")));
        // A level of 0 on battery is what it reports between readings while the mouse sleeps
        assert_eq!(power_byte(0x00), off());
    }

    #[test]
    fn aj179_awake_behind_its_receiver() {
        // Captured: 81%
        let r = h("10 00 01 0B 4D 31 37 39 00 00 95 01 00 51 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 D6");
        let info = device_info(&r, 0x10).unwrap();
        assert_eq!(info, DeviceInfo::Info { model: *b"M179", power: 0, level: 81, linked: true });
        assert_eq!(aj179_state(info, true), on(81, false));
    }

    #[test]
    fn aj179_asleep_is_off_though_the_receiver_still_has_a_level() {
        // Captured: 82%, mouse asleep
        let r = h("10 00 01 0B 4D 31 37 39 00 00 95 01 00 52 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 D6");
        let info = device_info(&r, 0x10).unwrap();
        assert_eq!(aj179_state(info, true), asleep());
        // On the cable there is no such byte to go by
        assert_eq!(aj179_state(info, false), on(82, false));
    }

    #[test]
    fn aj179_sibling_gets_the_brand_for_a_name() {
        let info = DeviceInfo::Info { model: *b"M603", power: 1, level: 40, linked: true };
        assert_eq!(aj179_state(info, true), State { name: Some(brand_mouse("AJAZZ")), ..on(40, true) });
    }

    /// A device-info frame with this head and the checksum over its data.
    fn info_frame(head: &str) -> Vec<u8> {
        let mut r = pad(32, &h(head));
        r[31] = sum(&r[4..31]) as u8;
        r
    }

    #[test]
    fn device_info_needs_the_command_and_a_checksum_that_fits() {
        let good = info_frame("30 00 01 0B 4D 31 33 37 01 02 95 00 00 5E 01");
        assert_eq!(
            device_info(&good, 0x30),
            Some(DeviceInfo::Info { model: *b"M137", power: 0, level: 94, linked: true })
        );
        let mut damaged = good.clone();
        damaged[13] = 0x5F;
        assert_eq!(device_info(&damaged, 0x30), Some(DeviceInfo::Again));
        // A reply to another command, a notification, a short report
        assert_eq!(device_info(&good, 0x10), None);
        assert_eq!(device_info(&pad(32, &h("D1 01 5E 00")), 0x30), None);
        assert_eq!(device_info(&good[..20], 0x30), None);
        assert_eq!(device_info(&pad(32, &h("30 02")), 0x30), Some(DeviceInfo::Away));
        assert_eq!(device_info(&pad(32, &h("30 01")), 0x30), Some(DeviceInfo::Again));
    }

    #[test]
    fn sc580_has_no_level_while_it_charges() {
        let on_battery = DeviceInfo::Info { model: *b"M137", power: 0, level: 94, linked: true };
        assert_eq!(sc580_state(on_battery), on(94, false));
        let charging = DeviceInfo::Info { model: *b"M137", power: 1, level: 100, linked: true };
        assert_eq!(
            sc580_state(charging),
            State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, true) }
        );
        let full = DeviceInfo::Info { model: *b"M137", power: 2, level: 100, linked: true };
        assert_eq!(sc580_state(full), charged(on(100, false)));
    }

    #[test]
    fn sc580_away_from_its_receiver_is_asleep() {
        assert_eq!(sc580_state(DeviceInfo::Away), asleep());
        let unlinked = DeviceInfo::Info { model: *b"M137", power: 0, level: 94, linked: false };
        assert_eq!(sc580_state(unlinked), asleep());
    }

    /// A Delux status: linked flag, level and charge state at their places.
    fn delux_reply(linked: u8, level: u8, state: u8) -> Vec<u8> {
        let mut r = pad(33, &h("0C 01 20 00 05 00 00 00 4D 38 30 32"));
        r[6] = linked;
        r[18] = level;
        r[19] = state;
        r
    }

    #[test]
    fn delux_m800_states() {
        assert_eq!(delux_state(&delux_reply(1, 81, 0), true), on(81, false));
        assert_eq!(delux_state(&delux_reply(1, 60, 1), true), on(60, true));
        assert_eq!(delux_state(&delux_reply(1, 97, 2), true), charged(on(100, false)));
        // On the cable the mouse itself answers
        assert_eq!(delux_state(&delux_reply(0, 81, 1), false), on(81, true));
    }

    #[test]
    fn delux_receiver_without_its_mouse_is_off() {
        assert_eq!(delux_state(&delux_reply(0, 81, 0), true), off());
        assert_eq!(delux_state(&delux_reply(1, 0, 0), true), off());
        assert_eq!(delux_state(&delux_reply(1, 101, 0), true), off());
        // Not the status command, and a report cut short
        let mut other = delux_reply(1, 81, 0);
        other[2] = 0x21;
        assert_eq!(delux_state(&other, true), off());
        assert_eq!(delux_state(&delux_reply(1, 81, 0)[..12], true), off());
    }

    #[test]
    fn niceboy_mw400_at_4133_millivolts() {
        // Captured: raw 1002, 90% in Niceboy's tool
        let r = h("05 49 FA 48 EA 03 00");
        assert!(is_niceboy_reply(&r));
        assert_eq!(niceboy_state(&r), on(90, false));
        assert_eq!(niceboy_state(&h("05 49 FA 48 FB 03 01")), on(100, true));
        assert_eq!(niceboy_state(&h("05 49 FA 48 00 03 00")), on(10, false));
    }

    #[test]
    fn niceboy_receiver_without_its_mouse_answers_zeros() {
        assert_eq!(niceboy_state(&h("05 49 FA 48 00 00 00")), asleep());
        assert!(!is_niceboy_reply(&h("05 49 FA 47 EA 03 00")));
        assert!(!is_niceboy_reply(&h("05 49 FA 48 EA 03")));
    }

    #[test]
    fn dareu_receiver_lists_its_mice_by_slot() {
        // Two entries, the second one on the air: slot 2, product 1148
        let list = pad(65, &h("0A 02 06 00 A0 00 00 02 00 11 48 01 11 48"));
        assert!(is_togran_answer(&list, 0x00, 0xA0));
        assert_eq!(togran_present(&list), Some((2, 0x1148)));
        assert_eq!(togran_present(&pad(65, &h("0A 02 06 00 A0 00 00 01 00 11 48"))), None);
        assert_eq!(togran_present(&pad(65, &h("0A 02 06 00 A0 00 00 00"))), None);
    }

    #[test]
    fn dareu_answer_counts_once_it_is_finished_and_echoes_the_request() {
        let battery = pad(65, &h("0A 22 04 07 80 00 00 01 03 04"));
        assert!(is_togran_answer(&battery, 0x07, 0x80));
        assert!(!is_togran_answer(&battery, 0x00, 0xA0));
        assert!(!is_togran_answer(&pad(65, &h("0A 21 04 07 80")), 0x07, 0x80));
    }

    #[test]
    fn dareu_level_counts_up_to_its_maximum() {
        assert_eq!(togran_battery(&pad(65, &h("0A 22 04 07 80 00 00 01 03 04"))), on(75, true));
        assert_eq!(togran_battery(&pad(65, &h("0A 22 04 07 80 00 00 00 50 64"))), on(80, false));
        assert_eq!(togran_battery(&pad(65, &h("0A 22 04 07 80 00 00 00 50 00"))), on(80, false));
        assert_eq!(togran_battery(&pad(65, &h("0A 22 04 07 80 00 00 00 05 04"))), off());
    }

    #[test]
    fn rk_mg6_pro_at_95_percent() {
        // 4102 mV and 95%, as its owner's receiver reported them
        let r = pad(64, &h("01 01 02 11 06 10 5F 00 00 00 00"));
        assert!(is_mg6_battery(&r));
        assert_eq!(mg6_state(&r, false), on(95, false));
        assert_eq!(mg6_state(&pad(64, &h("01 01 02 11 06 10 5F 00 00 00 01")), false), on(95, true));
        assert_eq!(mg6_state(&r, true), on(95, true));
    }

    #[test]
    fn rk_receiver_answering_for_a_sleeping_mouse() {
        assert_eq!(mg6_state(&pad(64, &h("01 01 02 01 06 10 5F 00")), false), asleep());
        assert_eq!(mg6_state(&pad(64, &h("01 01 02 11 06 10 2C 01")), false), off());
        assert!(!is_mg6_battery(&pad(64, &h("01 01 03 11 06 10 5F 00"))));
        assert!(!is_mg6_battery(&h("01 01 02 11")));
    }

    #[test]
    fn keys_and_names_are_there_and_ids_are_not_repeated() {
        let mut seen = HashSet::new();
        for spec in SPECS {
            assert!(spec.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", spec.key);
            for pid in spec.pids {
                assert!(seen.insert((spec.vid, *pid)), "{:04X}:{pid:04X} is listed twice", spec.vid);
                assert!(!is_link(*pid) || spec.vid != KEYCHRON, "{pid:04X} belongs to the Link provider");
            }
        }
    }
}
