//! ASUS ROG / TUF wireless mice and keyboards (vendor ID 0B05): on their own 2.4 GHz receiver, on the cable, and
//! behind the two receivers that are not one device's own - the ROG Omni Receiver (0B05:1ACE), which carries a
//! mouse and a keyboard at once, and the ROG SpeedNova 8K receiver of the Harpe II Ace (0B05:1AD0).
//!
//! One request each, an output report of the "get" family (12 ..) on the device's vendor control collection,
//! answered by an input report that echoes it. Bytes counted from the echo:
//!   mouse     12 07 -> 12 07 00 00 LL ..   [4] level: percent, or quarters 0-4 on the older mice (the Strix Carry
//!                                          has them at [6]), shown as an estimate; [9] not 0 while charging
//!   keyboard  12 01 -> 12 01 00 00 00 LL   [5] percent ([10] on the Falchion family); [8] = 1 while charging
//! A device that is asleep or switched off has its receiver answer FF AA in place of the echo, or an empty report,
//! or a level of 0: none of them is an empty battery. Some mice (the Gladius III Wireless) answer only while they
//! are awake.
//!   Own receiver or cable: no report IDs, 64 data bytes. Mice have the control collection on interface 0 (usage
//!     page FF01; Strix Carry interface 1, Gladius II Wireless and MD200 interface 2, MD301 interface 3), keyboards
//!     on interface 1 (usage page FF00).
//!   Omni and SpeedNova receivers: interface 2 has one vendor collection per report ID, 64 bytes with the ID.
//!     FF02, report 01: `01 A0 00` reads the list of paired devices, `01 A0 00 NN 00` and then four bytes a device -
//!     its product ID (little-endian), the report ID of its channel, one more byte - up to a product ID of 0. The
//!     list holds what is paired, not what is switched on.
//!     FF00, report 02: the keyboard, asked `02 12 01`.  FF01, report 03: the mouse, asked `03 12 07`.
//!     Replies are the ones above with the report ID in front.
//! Only reads are sent. 50 .. and 51 .. are the commands that save and change settings, and nothing here sends them.
//!
//! Protocol facts from G-Helper (github.com/seerge/g-helper: app/Peripherals/Mouse/AsusMouse.cs and Models/*.cs,
//! Keyboard/AsusKeyboard.cs and Models/*.cs, PeripheralsProvider.cs), checked against rogctl
//! (github.com/Humblemonk/rogctl: .claude/rules/protocol.md, with replies captured from a Harpe II Ace and a Falchion
//! RX Low Profile), HaloBattery (github.com/HeyOkay/HaloBattery: providers/asus.py, docs/protocols.md and the frames in
//! its tests), OpenAzoth (github.com/albzO/OpenAzoth: docs/PROTOCOL.md), ChargeDock (github.com/SgZincanya/ChargeDock:
//! rog_proto.py), rogdrv before it moved to ratbag (github.com/kyokenn/rogdrv: rog/device/base.py, mixins.py),
//! rog-mouse-battery (github.com/BillyNeverDies/rog-mouse-battery: README) and OpenRGB's detector tables for the
//! interfaces and usage pages (Controllers/AsusAuraUSBController). Own implementation. Not yet verified on hardware by
//! this project, which is why the first exchange with each device goes to the log.

use super::{off, off_because, on, Io, Query, Spec, State};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::providers::{contains_ignore_case, group_by, no_link_reason, one_per_id, secs, Provider};
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const VID: u16 = 0x0B05;
const GET: u8 = 0x12;
const MOUSE_BATTERY: u8 = 0x07;
const KEYBOARD_BATTERY: u8 = 0x01;
/// A receiver answers within a few milliseconds, also for a device that is off; G-Helper waits 300 ms.
const TIMEOUT_MS: u64 = 500;
/// The shortest a control collection's reports are, as Windows counts them: 64 bytes with the report ID on the shared
/// receivers, 65 with the 00 it puts in front of reports without one.
const REPORT_LENGTH: usize = 64;
/// A reply is read up to the keyboards' second level byte.
const REPLY_LENGTH: usize = 12;

/// Where a mouse's reply has the level, counted from the echo, and what the level counts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Percent(usize),
    /// 0-4, a quarter of the battery each.
    Quarters(usize),
    /// The same on a mouse that runs on AA cells: it never charges, and in its layout no source says what the byte
    /// holds that means "charging" on the others.
    CellQuarters(usize),
}

const MOUSE_PERCENT: Level = Level::Percent(4);
const MOUSE_QUARTERS: Level = Level::Quarters(4);
const STRIX_CARRY_QUARTERS: Level = Level::CellQuarters(6);
const KEYBOARD_PERCENT: usize = 5;
/// The Falchion family: G-Helper found a 0-10 gauge in the usual place on the first Falchion, and the percentage here.
const FALCHION_PERCENT: usize = 10;

const fn mouse(
    key: &'static str,
    name: &'static str,
    pids: &'static [u16],
    cable_pids: &'static [u16],
    interface: i32,
    query: Query,
) -> Spec {
    Spec { key, name, vid: VID, pids, cable_pids, interface, accept: Some(is_mouse_control), query, ..Spec::MOUSE }
}

const fn keyboard(
    key: &'static str,
    name: &'static str,
    pids: &'static [u16],
    cable_pids: &'static [u16],
    interface: i32,
    query: Query,
) -> Spec {
    Spec {
        key,
        name,
        vid: VID,
        pids,
        cable_pids,
        interface,
        accept: Some(is_keyboard_control),
        query,
        ..Spec::KEYBOARD
    }
}

/// The devices on their own receiver or cable: key, name, every ID, the IDs that are the cable, the interface. The
/// models that come with an Omni receiver have only their cable here; the receiver is `AsusReceiverProvider`'s, which
/// gives a model the same key.
pub static SPECS: &[Spec] = &[
    // ---- Mice that report a percentage ----
    mouse("asus-rog-chakram-x", "ROG Chakram X", &[0x1A1A, 0x1A18], &[0x1A18], 0, mouse_percent),
    mouse("asus-rog-gladius-iii-wireless", "ROG Gladius III Wireless", &[0x197F, 0x197D], &[0x197D], 0, mouse_percent),
    mouse(
        "asus-rog-gladius-iii-aimpoint",
        "ROG Gladius III Wireless AimPoint",
        &[0x1A72, 0x1A70],
        &[0x1A70],
        0,
        mouse_percent,
    ),
    mouse(
        "asus-rog-gladius-iii-aimpoint-eva-02",
        "ROG Gladius III Wireless AimPoint EVA-02 Edition",
        &[0x1B0C, 0x1B0A],
        &[0x1B0A],
        0,
        mouse_percent,
    ),
    mouse(
        "asus-rog-harpe-ace-aim-lab",
        "ROG Harpe Ace Aim Lab Edition",
        &[0x1A94, 0x1A92],
        &[0x1A92],
        0,
        mouse_percent,
    ),
    mouse("asus-rog-harpe-ace-extreme", "ROG Harpe Ace Extreme", &[0x1B67], &[0x1B67], 0, mouse_percent),
    mouse("asus-rog-harpe-ace-mini", "ROG Harpe Ace Mini", &[0x1B63], &[0x1B63], 0, mouse_percent),
    mouse("asus-rog-harpe-ii-ace", "ROG Harpe II Ace", &[0x1C69], &[0x1C69], 0, mouse_percent),
    mouse(
        "asus-rog-keris-wireless-aimpoint",
        "ROG Keris Wireless AimPoint",
        &[0x1A68, 0x1A66],
        &[0x1A66],
        0,
        mouse_percent,
    ),
    mouse("asus-rog-keris-ii-ace", "ROG Keris II Ace", &[0x1B16], &[0x1B16], 0, mouse_percent),
    mouse("asus-rog-keris-ii-origin", "ROG Keris II Origin", &[0x1C0C], &[0x1C0C], 0, mouse_percent),
    mouse("asus-rog-keris-ii-origin-kjp", "ROG Keris II Origin KJP", &[0x1D4C], &[0x1D4C], 0, mouse_percent),
    mouse("asus-rog-spatha-x", "ROG Spatha X", &[0x1979, 0x1977], &[0x1977], 0, mouse_percent),
    mouse("asus-tuf-gaming-m4-wireless", "TUF Gaming M4 Wireless", &[0x19F4], &[], 0, mouse_percent),
    mouse("asus-tx-gaming-mouse", "ASUS TX Gaming Mouse", &[0x1A8D], &[], 0, mouse_percent),
    mouse("asus-tx-gaming-mini", "ASUS TX Gaming Mini", &[0x1AF5, 0x1AF3], &[0x1AF3], 0, mouse_percent),
    mouse("asus-tuf-gaming-mini-miku", "TUF Gaming Mini Miku Edition", &[0x1C57, 0x1C56], &[0x1C56], 0, mouse_percent),
    // Not in G-Helper: HaloBattery has a reply captured on this receiver (its pull request 205)
    mouse("asus-tx-gaming-mini-miku", "ASUS TX Gaming Mini Miku Edition", &[0x1C5A], &[], 0, mouse_percent),
    mouse("asus-md200", "ASUS MD200", &[0x1A24], &[], 2, mouse_percent),
    mouse("asus-proart-md301", "ASUS ProArt Mouse MD301", &[0x1CD3, 0x1CD5], &[0x1CD5], 3, mouse_percent),
    // ---- Older mice, which report quarters ----
    mouse("asus-rog-chakram", "ROG Chakram", &[0x18E5, 0x18E3], &[0x18E3], 0, mouse_quarters),
    mouse("asus-rog-keris-wireless", "ROG Keris Wireless", &[0x1960, 0x195E], &[0x195E], 0, mouse_quarters),
    mouse(
        "asus-rog-keris-wireless-eva",
        "ROG Keris Wireless EVA Edition",
        &[0x1A59, 0x1A57],
        &[0x1A57],
        0,
        mouse_quarters,
    ),
    mouse("asus-rog-pugio-ii", "ROG Pugio II", &[0x1908, 0x1906], &[0x1906], 0, mouse_quarters),
    mouse(
        "asus-rog-strix-impact-ii-wireless",
        "ROG Strix Impact II Wireless",
        &[0x1949, 0x1947],
        &[0x1947],
        0,
        mouse_quarters,
    ),
    mouse("asus-rog-gladius-ii-wireless", "ROG Gladius II Wireless", &[0x18A0], &[], 2, mouse_quarters),
    mouse("asus-rog-strix-carry", "ROG Strix Carry", &[0x18B4], &[], 1, strix_carry),
    // ---- Keyboards ----
    keyboard("asus-rog-azoth", "ROG Azoth", &[0x1A85, 0x1A83], &[0x1A83], 1, keyboard_percent),
    keyboard("asus-rog-azoth-extreme", "ROG Azoth Extreme", &[0x1B3F], &[0x1B3F], 1, keyboard_percent),
    keyboard("asus-rog-azoth-extreme-se", "ROG Azoth Extreme SE", &[0x1CEF], &[0x1CEF], 1, keyboard_percent),
    keyboard("asus-rog-azoth-x", "ROG Azoth X", &[0x1C25, 0x1C24], &[0x1C24], 1, keyboard_percent),
    keyboard("asus-rog-falchion", "ROG Falchion", &[0x193E, 0x193C], &[0x193C], 1, falchion),
    keyboard("asus-rog-falchion-rx-low-profile", "ROG Falchion RX Low Profile", &[0x1B04], &[0x1B04], 1, falchion),
    keyboard(
        "asus-rog-strix-scope-ii-96-wireless",
        "ROG Strix Scope II 96 Wireless",
        &[0x1AAE],
        &[0x1AAE],
        1,
        keyboard_percent,
    ),
    keyboard(
        "asus-rog-strix-scope-ii-96-rx-wireless",
        "ROG Strix Scope II 96 RX Wireless",
        &[0x1B78],
        &[0x1B78],
        1,
        keyboard_percent,
    ),
    // Left out: the ROG Strix Scope RX TKL Wireless Deluxe (0B05:1A07). G-Helper has it without an interface and with
    // the level in the usual place, and no owner has compared that with Armoury Crate's; the first Falchion, a
    // keyboard of the same years, has a 0-10 gauge there, which would be shown as a percentage
];

/// A mouse's control collection: the vendor collection of its interface, with reports of the full length. The mouse
/// and keyboard collections next to it are not asked.
fn is_mouse_control(i: &HidInfo) -> bool {
    i.usage_page >= 0xFF00 && has_full_reports(i)
}

fn is_keyboard_control(i: &HidInfo) -> bool {
    i.usage_page == 0xFF00 && has_full_reports(i)
}

fn has_full_reports(i: &HidInfo) -> bool {
    i.input_length >= REPORT_LENGTH && i.output_length >= REPORT_LENGTH
}

fn mouse_percent(io: &mut Io) -> win::Result<State> {
    mouse_battery(io, 0x00, MOUSE_PERCENT)
}

fn mouse_quarters(io: &mut Io) -> win::Result<State> {
    mouse_battery(io, 0x00, MOUSE_QUARTERS)
}

fn strix_carry(io: &mut Io) -> win::Result<State> {
    mouse_battery(io, 0x00, STRIX_CARRY_QUARTERS)
}

fn keyboard_percent(io: &mut Io) -> win::Result<State> {
    keyboard_battery(io, 0x00, KEYBOARD_PERCENT)
}

fn falchion(io: &mut Io) -> win::Result<State> {
    keyboard_battery(io, 0x00, FALCHION_PERCENT)
}

/// `id`: the report ID the collection takes requests on, 0 when it has none.
fn mouse_battery(io: &mut Io, id: u8, level: Level) -> win::Result<State> {
    Ok(match ask(io, id, MOUSE_BATTERY)? {
        Answer::Reply(b) => mouse_state(&b, level),
        Answer::Asleep => off_because("d.asleep"),
        Answer::Silent => off(),
    })
}

fn keyboard_battery(io: &mut Io, id: u8, level_at: usize) -> win::Result<State> {
    Ok(match ask(io, id, KEYBOARD_BATTERY)? {
        Answer::Reply(b) => keyboard_state(&b, level_at),
        Answer::Asleep => off_because("d.asleep"),
        Answer::Silent => off(),
    })
}

/// What came back for a request: the reply from its echo on, the receiver's "no answer from the device", or nothing.
enum Answer {
    Reply(Vec<u8>),
    Asleep,
    Silent,
}

fn ask(io: &mut Io, id: u8, sub: u8) -> win::Result<Answer> {
    io.set_timeout(TIMEOUT_MS);
    // Button and profile events come in on the same collection, and so do replies to what Armoury Crate or G-Helper
    // ask when they run too: only this request's echo counts
    let answer =
        io.ask(&request(id, sub), |d| body(d, id).is_some_and(|b| is_reply(b, sub) || is_no_answer(b)), 8, false)?;
    Ok(match answer.as_deref().and_then(|d| body(d, id)) {
        Some(b) if is_reply(b, sub) => Answer::Reply(b.to_vec()),
        Some(_) => Answer::Asleep,
        None => Answer::Silent,
    })
}

/// A read of the "get" family; `Io::send` pads it to the collection's report length.
pub fn request(id: u8, sub: u8) -> [u8; 3] {
    [id, GET, sub]
}

/// An input report from the echo on: all of it for a collection without report IDs, else what follows the ID the
/// request went out with. A report with another ID is not for this request.
pub fn body(d: &[u8], id: u8) -> Option<&[u8]> {
    if id == 0 {
        Some(d)
    } else {
        d.strip_prefix(&[id])
    }
}

pub fn is_reply(b: &[u8], sub: u8) -> bool {
    b.len() >= REPLY_LENGTH && b[0] == GET && b[1] == sub
}

/// FF AA, the firmware's "no such device or command", or an empty report: what a receiver sends back for a device
/// that is asleep, switched off or out of range.
pub fn is_no_answer(b: &[u8]) -> bool {
    b.len() >= 3 && (b[..2] == [0xFF, 0xAA] || b[..3] == [0x00, 0x00, 0x00])
}

/// What a mouse's reply says, from the echo on.
pub fn mouse_state(b: &[u8], level: Level) -> State {
    let charging = b[9] != 0 && !matches!(level, Level::CellQuarters(_));
    match level {
        Level::Percent(at) => state(Some(b[at] as i32).filter(|l| *l <= 100), charging),
        Level::Quarters(at) | Level::CellQuarters(at) => {
            let state = state(Some(b[at] as i32).filter(|q| *q <= 4).map(|q| q * 25), charging);
            // A quarter is all the mouse says: 25% is anything from there to just under 50%
            State { estimate: state.level.is_some(), ..state }
        }
    }
}

/// What a keyboard's reply says, from the echo on.
pub fn keyboard_state(b: &[u8], level_at: usize) -> State {
    state(Some(b[level_at] as i32).filter(|l| *l <= 100), b[8] == 0x01)
}

/// `level`: None when the byte can't be a level.
fn state(level: Option<i32>, charging: bool) -> State {
    match level {
        // A level of 0 is what comes back for a device that sleeps (G-Helper: "standby"), and G-Helper never takes it
        // for a level. On the charger the device is there all the same, with a level that isn't known
        Some(0) if !charging => off_because("d.asleep"),
        Some(level) if level > 0 => on(level, charging),
        _ => State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, charging) },
    }
}

// ---- The Omni and SpeedNova receivers ----

const OMNI: u16 = 0x1ACE;
const SPEEDNOVA: u16 = 0x1AD0;
const RECEIVER_INTERFACE: i32 = 2;
const PAIR_LIST: u8 = 0xA0;
/// The key of a mouse behind an Omni receiver whose model the pair list doesn't tell.
const UNNAMED_MOUSE: &str = "asus-omni-mouse";

/// One of a receiver's vendor collections and the report ID it takes requests on.
#[derive(Clone, Copy)]
struct Channel {
    page: u16,
    id: u8,
}

const PAIRS: Channel = Channel { page: 0xFF02, id: 0x01 };
const KEYBOARD: Channel = Channel { page: 0xFF00, id: 0x02 };
const MOUSE: Channel = Channel { page: 0xFF01, id: 0x03 };

/// Mice by the product ID an Omni receiver's pair list has for them (G-Helper's MouseFromOmniPid): ID, key, name.
/// All of them report a percentage. A key that is also in `SPECS` is the same mouse on its cable or own receiver.
static OMNI_MICE: &[(u16, &str, &str)] = &[
    (0x1B65, "asus-rog-harpe-ace-mini", "ROG Harpe Ace Mini"),
    (0x1C0E, "asus-rog-keris-ii-origin", "ROG Keris II Origin"),
    (0x1D4E, "asus-rog-keris-ii-origin-kjp", "ROG Keris II Origin KJP"),
    (0x1A94, "asus-rog-harpe-ace-aim-lab", "ROG Harpe Ace Aim Lab Edition"),
    (0x1AD7, "asus-rog-strix-impact-iii-wireless", "ROG Strix Impact III Wireless"),
    (0x1A72, "asus-rog-gladius-iii-aimpoint", "ROG Gladius III Wireless AimPoint"),
    (0x1A68, "asus-rog-keris-wireless-aimpoint", "ROG Keris Wireless AimPoint"),
    (0x1A6A, "asus-rog-keris-wireless-aimpoint", "ROG Keris Wireless AimPoint"),
    (0x1B1A, "asus-rog-keris-ii-ace", "ROG Keris II Ace"),
    (0x1B18, "asus-rog-keris-ii-ace", "ROG Keris II Ace"),
    (0x1B68, "asus-rog-harpe-ace-extreme", "ROG Harpe Ace Extreme"),
    (0x1B69, "asus-rog-harpe-ace-extreme", "ROG Harpe Ace Extreme"),
];

/// Keyboards by their ID in the pair list (G-Helper's KeyboardFromOmniPid): ID, key, name, where the level is. The
/// Strix Morph 96 is HaloBattery's, whose reporter compared the level with Gear Link's (its issue 340).
static OMNI_KEYBOARDS: &[(u16, &str, &str, usize)] = &[
    (0x1A85, "asus-rog-azoth", "ROG Azoth", KEYBOARD_PERCENT),
    (0x1B42, "asus-rog-azoth-extreme", "ROG Azoth Extreme", KEYBOARD_PERCENT),
    (0x1CF1, "asus-rog-azoth-extreme-se", "ROG Azoth Extreme SE", KEYBOARD_PERCENT),
    (0x1AB0, "asus-rog-strix-scope-ii-96-wireless", "ROG Strix Scope II 96 Wireless", KEYBOARD_PERCENT),
    (0x1B7A, "asus-rog-strix-scope-ii-96-rx-wireless", "ROG Strix Scope II 96 RX Wireless", KEYBOARD_PERCENT),
    (0x1B06, "asus-rog-falchion-rx-low-profile", "ROG Falchion RX Low Profile", FALCHION_PERCENT),
    (0x1CEA, "asus-rog-strix-morph-96-wireless", "ROG Strix Morph 96 Wireless", KEYBOARD_PERCENT),
];

/// One entry of an Omni receiver's pair list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub pid: u16,
    /// The report ID of the channel the device is on: 02 for the keyboards and 03 for the mouse in the lists
    /// captured so far.
    pub channel: u8,
}

/// What an Omni receiver says about its pair list. None: it has none to give - no such collection, or the request
/// refused - which is the receiver's firmware and the same at every poll.
pub type PairList = Option<Vec<Slot>>;

fn is_receiver(pid: u16) -> bool {
    pid == OMNI || pid == SPEEDNOVA
}

/// The answer to the pair list request, or the receiver's refusal of it.
fn is_pairs_answer(d: &[u8]) -> bool {
    d.len() >= 5 && d[0] == PAIRS.id && (d[1..3] == [PAIR_LIST, 0x00] || d[1..3] == [0xFF, 0xAA])
}

/// The paired devices; None when the report is not a pair list.
pub fn parse_pairs(d: &[u8]) -> Option<Vec<Slot>> {
    if d.len() < 5 || d[..3] != [PAIRS.id, PAIR_LIST, 0x00] {
        return None;
    }
    let slots = d[5..].as_chunks::<4>().0.iter().map(|s| Slot { pid: u16::from_le_bytes([s[0], s[1]]), channel: s[2] });
    Some(slots.take_while(|s| s.pid != 0).collect())
}

/// A device behind a receiver before it is asked. `name`: empty for a mouse whose model isn't known. `listed`: it
/// is shown also while it doesn't answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paired {
    pub key: &'static str,
    pub name: &'static str,
    pub listed: bool,
}

/// The mouse to ask an Omni receiver for. `pairs`: None when the receiver has no pair list to give.
/// The first mouse of the list that is known names it, as in G-Helper. Every mouse on this receiver answers the same
/// way, so one the table doesn't have is still read, without a model name; so is the mouse of a receiver without a
/// pair list, which is then shown only once it has answered. A list without a mouse leaves the channel alone.
pub fn omni_mouse(pairs: Option<&[Slot]>) -> Option<Paired> {
    let Some(slots) = pairs else { return Some(Paired { key: UNNAMED_MOUSE, name: "", listed: false }) };
    let known = slots.iter().find_map(|s| OMNI_MICE.iter().find(|m| m.0 == s.pid));
    match known {
        Some(&(_, key, name)) => Some(Paired { key, name, listed: true }),
        None => {
            slots.iter().any(|s| s.channel == MOUSE.id).then_some(Paired { key: UNNAMED_MOUSE, name: "", listed: true })
        }
    }
}

/// The keyboard to ask an Omni receiver for and where its level is. Where it is depends on the model, so a keyboard
/// the table doesn't have is not read.
pub fn omni_keyboard(pairs: &[Slot]) -> Option<(Paired, usize)> {
    let &(_, key, name, level_at) = pairs.iter().find_map(|s| OMNI_KEYBOARDS.iter().find(|k| k.0 == s.pid))?;
    Some((Paired { key, name, listed: true }, level_at))
}

/// The mouse of a SpeedNova 8K receiver, which is one of two by the receiver's product string (G-Helper). The
/// receiver answers "asleep" on its mouse channel also when it has no mouse (Wireless-Device-Battery-Monitor's issue
/// 140), so the mouse is shown only once it has answered.
pub fn speednova_mouse(product: Option<&str>) -> Paired {
    if product.is_some_and(|p| contains_ignore_case(p, "EXTREME")) {
        Paired { key: "asus-rog-harpe-ii-extreme", name: "ROG Harpe II Extreme Edition 20", listed: false }
    } else {
        Paired { key: "asus-rog-harpe-ii-ace", name: "ROG Harpe II Ace", listed: false }
    }
}

/// What the collections of one USB interface share in their paths: `8&2a1b3c&0` in
/// `\\?\hid#vid_0b05&pid_1ace&mi_02&col03#8&2a1b3c&0&0002#{..}`. It keeps two receivers apart.
pub fn instance(path: &str) -> &str {
    let part = path.split('#').nth(2).unwrap_or(path);
    part.rsplit_once('&').map_or(part, |(head, _)| head)
}

fn channel(collections: &[HidInfo], of: Channel) -> Option<&HidInfo> {
    collections.iter().find(|i| !i.access_denied && i.usage_page == of.page && has_full_reports(i))
}

/// Whether the model with this key is plugged in itself, on its cable or with its own receiver. `plugged`: the
/// product IDs present.
pub fn is_plugged_in(key: &str, plugged: &[u16]) -> bool {
    SPECS.iter().any(|s| s.key == key && s.pids.iter().any(|p| plugged.contains(p)))
}

/// A device behind a receiver and what it answered.
struct Behind {
    device: Paired,
    kind: DeviceKind,
    state: State,
}

impl Behind {
    fn reading(self) -> DeviceReading {
        let name = if self.device.name.is_empty() { tf("name.brandMouse", &[&"ROG"]) } else { t(self.device.name) };
        // The Id the table gives the same model on its cable
        let id = Spec { key: self.device.key, kind: self.kind, ..Spec::MOUSE }.id();
        DeviceReading {
            online: self.state.online,
            level: self.state.level,
            estimate: self.state.estimate && self.state.level.is_some(),
            charging: self.state.charging,
            full_on_charger: self.state.full,
            detail: self.state.detail.as_deref().map(t),
            ..DeviceReading::new(id, name, self.kind, t("via.24"))
        }
    }
}

/// The ROG Omni Receiver and the SpeedNova 8K receiver: up to two devices behind one USB ID, named by a pair list or
/// not at all, which is more than a row of the table can say.
#[derive(Default)]
pub struct AsusReceiverProvider {
    /// The receivers whose mouse gave a level in this session: see `Paired::listed`.
    answered: HashSet<String>,
    /// What each Omni receiver said last about its pair list: see `pair_list`.
    pairs: HashMap<String, PairList>,
    /// What this session's log already has: see `log_once`.
    logged: HashSet<String>,
}

impl Provider for AsusReceiverProvider {
    fn name(&self) -> &'static str {
        "ASUS receivers"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let found = hid::enumerate(|v, p| v == VID && (is_receiver(p) || SPECS.iter().any(|s| s.pids.contains(&p))));
        let plugged: Vec<u16> = found.iter().map(|i| i.product_id).filter(|p| !is_receiver(*p)).collect();
        let channels: Vec<HidInfo> = found
            .into_iter()
            .filter(|i| is_receiver(i.product_id) && i.interface_number() == RECEIVER_INTERFACE)
            .collect();

        let mut result = Vec::new();
        for ((pid, port), collections) in group_by(channels, |i| (i.product_id, instance(&i.path).to_string())) {
            let mut lines = Vec::new();
            let devices = self.ask_receiver(pid, &port, &collections, &mut lines);
            self.log_once(pid, &port, &collections, &lines, devices.iter().any(|d| d.state.online));
            for device in devices {
                // In wired mode a mouse or keyboard leaves its receiver and shows up under its cable's ID, which the
                // table reads: the receiver that finds it off says nothing then, so the cable's reading stands
                if device.state.online || !is_plugged_in(device.device.key, &plugged) {
                    result.push(device.reading());
                }
            }
        }
        // Two receivers with the same model behind them
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

impl AsusReceiverProvider {
    /// Asks one receiver for the devices behind it. `lines` is filled with what was sent and received.
    fn ask_receiver(
        &mut self,
        pid: u16,
        instance: &str,
        collections: &[HidInfo],
        lines: &mut Vec<String>,
    ) -> Vec<Behind> {
        let mut devices = Vec::new();
        let receiver = format!("{pid:04x} {instance}");
        let pairs = if pid == OMNI {
            let said = read_pairs(collections, lines);
            let Some(pairs) = self.pair_list(&receiver, said) else { return devices };
            pairs
        } else {
            None
        };
        let mouse = if pid == SPEEDNOVA {
            Some(speednova_mouse(collections.iter().find_map(|i| i.product.as_deref())))
        } else {
            omni_mouse(pairs.as_deref())
        };

        if let Some(device) = mouse {
            let state = on_channel(collections, MOUSE, lines, |io| mouse_battery(io, MOUSE.id, MOUSE_PERCENT))
                .unwrap_or_else(off_because);
            if state.online {
                self.answered.insert(receiver.clone());
            }
            if device.listed || self.answered.contains(&receiver) {
                devices.push(Behind { device, kind: DeviceKind::Mouse, state });
            }
        }
        if let Some((device, level_at)) = pairs.as_deref().and_then(omni_keyboard) {
            let state = on_channel(collections, KEYBOARD, lines, |io| keyboard_battery(io, KEYBOARD.id, level_at))
                .unwrap_or_else(off_because);
            devices.push(Behind { device, kind: DeviceKind::Keyboard, state });
        }
        devices
    }

    /// The pair list to go by. `said`: what the receiver says now, None when it has a list and gave nothing that reads
    /// as one this time. The list names the devices, and the app remembers every name it has shown a level under: a
    /// mouse read without its list for one poll would stay in the app's list as a second, nameless one. So a receiver
    /// that doesn't say is taken at its last word, and one that hasn't said yet (None) is asked again at the next poll.
    fn pair_list(&mut self, receiver: &str, said: Option<PairList>) -> Option<PairList> {
        match said {
            Some(pairs) => {
                self.pairs.insert(receiver.into(), pairs.clone());
                Some(pairs)
            }
            None => self.pairs.get(receiver).cloned(),
        }
    }

    /// Logs a receiver's collections and the exchange with it, the first time nothing behind it answers and the first
    /// time something does in a session.
    fn log_once(&mut self, pid: u16, instance: &str, collections: &[HidInfo], lines: &[String], online: bool) {
        if !self.logged.insert(format!("{pid:04x} {instance} {online}")) {
            return;
        }
        log::write(&format!(
            "ASUS receiver {VID:04x}:{pid:04x}: {}, {} HID collections on interface {RECEIVER_INTERFACE}",
            if online { "a device behind it is on" } else { "nothing behind it answers" },
            collections.len()
        ));
        for info in collections {
            log::write(&format!("  {info}"));
        }
        for line in lines {
            log::write(&format!("  {line}"));
        }
    }
}

/// What an Omni receiver says about its pair list; None when it has the collection and didn't answer the request
/// this time, or couldn't be asked.
fn read_pairs(collections: &[HidInfo], lines: &mut Vec<String>) -> Option<PairList> {
    let request = [PAIRS.id, PAIR_LIST, 0x00];
    let has_collection = channel(collections, PAIRS).is_some();
    match on_channel(collections, PAIRS, lines, |io| io.ask(&request, is_pairs_answer, 4, false)) {
        // The list, or the refusal of the request
        Ok(Some(d)) => Some(parse_pairs(&d)),
        Err(_) if !has_collection => Some(None),
        _ => None,
    }
}

/// Runs `query` on one channel of a receiver. The error is the text for a reading: why the channel couldn't be asked.
fn on_channel<T>(
    collections: &[HidInfo],
    to: Channel,
    lines: &mut Vec<String>,
    query: impl FnOnce(&mut Io) -> win::Result<T>,
) -> Result<T, String> {
    let Some(info) = channel(collections, to) else {
        lines.push(format!("report {:02x}: no collection on page {:04X}", to.id, to.page));
        return Err(no_link_reason(collections).into());
    };
    let mut trace = Vec::new();
    let result = HidDevice::open(info).and_then(|dev| {
        let mut io = Io::new(dev, &mut trace);
        io.set_timeout(TIMEOUT_MS);
        query(&mut io)
    });
    if let Err(e) = &result {
        trace.push(format!("error: {e}"));
    }
    lines.push(format!("report {:02x}: {}", to.id, trace.join("  ")));
    result.map_err(|e| tf("d.error", &[&e]))
}

#[cfg(test)]
mod tests {
    //! Replies as the sources captured them. Harpe II Ace on the SpeedNova receiver: HaloBattery
    //! (tests/test_harpe_ii.py, from its pull request 364) and rogctl (.claude/rules/protocol.md). Harpe Ace Mini and
    //! Falchion RX Low Profile on an Omni receiver, with its pair list: HaloBattery (tests/test_device_pr185_asus.py)
    //! and rogctl. TX Gaming Mini Miku on its own receiver: HaloBattery (tests/test_device_pr205_asus.py). Gladius II
    //! Wireless on its receiver: the log attached to G-Helper's issue 2762. The other frames are made up after the
    //! layouts in G-Helper and OpenAzoth, and say so.
    use super::*;

    fn h(text: &str) -> Vec<u8> {
        let mut report: Vec<u8> = text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect();
        report.resize(64, 0);
        report
    }

    fn asleep() -> State {
        off_because("d.asleep")
    }

    fn level_unknown(charging: bool) -> State {
        State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, charging) }
    }

    /// A level that is one of the mouse's quarters.
    fn about(level: i32, charging: bool) -> State {
        State { estimate: true, ..on(level, charging) }
    }

    /// What `ask` makes of an input report, as far as it can be told without a device.
    fn mouse_on(report: &[u8], id: u8, level: Level) -> Option<State> {
        let b = body(report, id)?;
        if is_reply(b, MOUSE_BATTERY) {
            Some(mouse_state(b, level))
        } else {
            is_no_answer(b).then(asleep)
        }
    }

    fn keyboard_on(report: &[u8], id: u8, level_at: usize) -> Option<State> {
        let b = body(report, id)?;
        if is_reply(b, KEYBOARD_BATTERY) {
            Some(keyboard_state(b, level_at))
        } else {
            is_no_answer(b).then(asleep)
        }
    }

    fn collection(pid: u16, mi: u8, page: u16, input: usize, output: usize) -> HidInfo {
        HidInfo {
            path: format!(r"\\?\hid#vid_0b05&pid_{pid:04x}&mi_{mi:02x}&col{page:04x}#8&2a1b3c&0&0000#{{4d1e55b2}}"),
            vendor_id: VID,
            product_id: pid,
            usage_page: page,
            usage: 0x0001,
            input_length: input,
            output_length: output,
            ..HidInfo::default()
        }
    }

    /// The collections the row of this product ID is asked on, by the row's own rule: (interface, usage page).
    fn asked(pid: u16, all: &[HidInfo]) -> Vec<(i32, u16)> {
        let spec = SPECS.iter().find(|s| s.pids.contains(&pid)).unwrap();
        let accept = spec.accept.unwrap();
        all.iter()
            .filter(|i| (spec.interface < 0 || i.interface_number() == spec.interface) && accept(i))
            .map(|i| (i.interface_number(), i.usage_page))
            .collect()
    }

    #[test]
    fn requests_are_reads_with_the_channels_report_id() {
        assert_eq!(request(0x00, MOUSE_BATTERY), [0x00, 0x12, 0x07]);
        assert_eq!(request(0x00, KEYBOARD_BATTERY), [0x00, 0x12, 0x01]);
        assert_eq!(request(MOUSE.id, MOUSE_BATTERY), [0x03, 0x12, 0x07]);
        assert_eq!(request(KEYBOARD.id, KEYBOARD_BATTERY), [0x02, 0x12, 0x01]);
    }

    #[test]
    fn harpe_ii_ace_on_the_speednova_receiver() {
        // 53% and 80%, not charging; the two bytes after the warning level look like millivolts
        assert_eq!(mouse_on(&h("03 12 07 00 00 35 02 14 fd 0e 00 00"), 0x03, MOUSE_PERCENT), Some(on(53, false)));
        assert_eq!(mouse_on(&h("03 12 07 00 00 50 02 14 d8 0f 00 00 01"), 0x03, MOUSE_PERCENT), Some(on(80, false)));
    }

    #[test]
    fn harpe_ace_mini_on_the_omni_receiver() {
        assert_eq!(mouse_on(&h("03 12 07 00 00 37 02 0a ed 0e 00 00 01"), 0x03, MOUSE_PERCENT), Some(on(55, false)));
    }

    #[test]
    fn tx_gaming_mini_miku_on_its_own_receiver() {
        // No report IDs: Io::data has dropped the 00 Windows puts in front
        assert_eq!(mouse_on(&h("12 07 00 00 2e 02 14 d7 0e 00 00 01"), 0x00, MOUSE_PERCENT), Some(on(46, false)));
    }

    #[test]
    fn a_mouse_on_the_charger_says_so() {
        // Made up: 75%, the charging byte set
        assert_eq!(mouse_on(&h("03 12 07 00 00 4b 04 14 00 00 01"), 0x03, MOUSE_PERCENT), Some(on(75, true)));
        assert_eq!(mouse_on(&h("12 07 00 00 4b 04 14 00 00 01"), 0x00, MOUSE_PERCENT), Some(on(75, true)));
    }

    #[test]
    fn falchion_rx_low_profile_on_the_omni_receiver() {
        // 52% and 66%, not charging: this keyboard has the level in both places
        let rogctl = h("02 12 01 00 00 00 34 02 01 00 14 34 ff 0e");
        let halo = h("02 12 01 00 00 00 42 02 01 00 14 42 27 0f");
        assert_eq!(keyboard_on(&rogctl, 0x02, FALCHION_PERCENT), Some(on(52, false)));
        assert_eq!(keyboard_on(&halo, 0x02, FALCHION_PERCENT), Some(on(66, false)));
    }

    #[test]
    fn falchion_rx_low_profile_on_its_cable_is_charging() {
        assert_eq!(
            keyboard_on(&h("12 01 00 00 00 34 02 01 01 14 34 84 0f"), 0x00, FALCHION_PERCENT),
            Some(on(52, true))
        );
    }

    #[test]
    fn the_first_falchion_has_a_gauge_where_the_others_have_the_level() {
        // Made up after G-Helper: 7 of 10 in the usual place, 70% further on
        let r = h("12 01 00 00 00 07 02 01 00 14 46");
        assert_eq!(keyboard_on(&r, 0x00, FALCHION_PERCENT), Some(on(70, false)));
    }

    #[test]
    fn azoth_has_the_level_right_after_the_echo() {
        // Made up after OpenAzoth: 77%, idle setting, power saving, charging, warning level
        assert_eq!(keyboard_on(&h("12 01 00 00 00 4d 02 00 01 14"), 0x00, KEYBOARD_PERCENT), Some(on(77, true)));
        assert_eq!(keyboard_on(&h("02 12 01 00 00 00 4d 02 00 00 14"), 0x02, KEYBOARD_PERCENT), Some(on(77, false)));
    }

    #[test]
    fn a_keyboard_charges_only_when_the_byte_is_one() {
        assert_eq!(keyboard_on(&h("12 01 00 00 00 4d 02 00 02 14"), 0x00, KEYBOARD_PERCENT), Some(on(77, false)));
    }

    #[test]
    fn a_device_that_is_off_has_its_receiver_answer_ff_aa() {
        // rogctl: the SpeedNova with the Harpe II Ace off, the Omni with the Falchion RX off
        assert_eq!(mouse_on(&h("03 ff aa"), 0x03, MOUSE_PERCENT), Some(asleep()));
        assert_eq!(keyboard_on(&h("02 ff aa"), 0x02, FALCHION_PERCENT), Some(asleep()));
        // rog-mouse-battery: the Gladius III Wireless while it is idle
        assert_eq!(mouse_on(&h("ff aa"), 0x00, MOUSE_PERCENT), Some(asleep()));
    }

    #[test]
    fn an_empty_report_is_a_device_asleep() {
        assert_eq!(mouse_on(&h("03 00 00 00"), 0x03, MOUSE_PERCENT), Some(asleep()));
        assert_eq!(mouse_on(&h("00"), 0x00, MOUSE_PERCENT), Some(asleep()));
    }

    #[test]
    fn level_zero_is_a_device_asleep_not_an_empty_one() {
        // Made up: the echo with nothing behind it, and with the power-off setting only
        assert_eq!(mouse_on(&h("03 12 07 00 00 00 ff"), 0x03, MOUSE_PERCENT), Some(asleep()));
        assert_eq!(mouse_on(&h("12 07 00 00 00 02 14"), 0x00, MOUSE_QUARTERS), Some(asleep()));
        assert_eq!(keyboard_on(&h("02 12 01 00 00 00 00 02 01"), 0x02, KEYBOARD_PERCENT), Some(asleep()));
    }

    #[test]
    fn level_zero_on_the_charger_is_a_device_whose_level_is_not_known() {
        assert_eq!(mouse_on(&h("12 07 00 00 00 02 14 00 00 01"), 0x00, MOUSE_PERCENT), Some(level_unknown(true)));
        assert_eq!(mouse_on(&h("12 07 00 00 00 02 01 00 00 01"), 0x00, MOUSE_QUARTERS), Some(level_unknown(true)));
        assert_eq!(keyboard_on(&h("12 01 00 00 00 00 02 00 01"), 0x00, KEYBOARD_PERCENT), Some(level_unknown(true)));
    }

    #[test]
    fn other_reports_are_not_the_answer() {
        // A button event, the reply to another read, a keyboard reply on the mouse channel
        assert_eq!(mouse_on(&h("03 05 01 00"), 0x03, MOUSE_PERCENT), None);
        assert_eq!(mouse_on(&h("03 12 00 00 00 35"), 0x03, MOUSE_PERCENT), None);
        assert_eq!(mouse_on(&h("03 12 01 00 00 00 35"), 0x03, MOUSE_PERCENT), None);
        // Another channel's report ID, also when it refuses
        assert_eq!(mouse_on(&h("02 12 07 00 00 35"), 0x03, MOUSE_PERCENT), None);
        assert_eq!(mouse_on(&h("02 ff aa"), 0x03, MOUSE_PERCENT), None);
        // A mouse reply where a keyboard's is waited for
        assert_eq!(keyboard_on(&h("12 07 00 00 35"), 0x00, KEYBOARD_PERCENT), None);
        // Cut short
        assert_eq!(mouse_on(&[0x03, 0x12, 0x07, 0x00, 0x00, 0x35], 0x03, MOUSE_PERCENT), None);
        assert_eq!(mouse_on(&[], 0x03, MOUSE_PERCENT), None);
    }

    #[test]
    fn older_mice_count_in_quarters_which_are_shown_as_estimates() {
        // Made up after G-Helper and rogdrv: the level, the power-off setting, the warning level
        assert_eq!(mouse_on(&h("12 07 00 00 04 02 01"), 0x00, MOUSE_QUARTERS), Some(about(100, false)));
        assert_eq!(mouse_on(&h("12 07 00 00 03 02 01"), 0x00, MOUSE_QUARTERS), Some(about(75, false)));
        assert_eq!(mouse_on(&h("12 07 00 00 01 02 01 00 00 01"), 0x00, MOUSE_QUARTERS), Some(about(25, true)));
        // A percentage is the mouse's own number
        assert!(!mouse_on(&h("12 07 00 00 4b"), 0x00, MOUSE_PERCENT).unwrap().estimate);
    }

    #[test]
    fn gladius_ii_wireless_full_on_its_receiver() {
        // The reply in the log of G-Helper's issue 2762, with Armoury Crate showing 100%: the level in the same place
        // as on the newer mice and nothing after it, not where the Strix Carry has it
        assert_eq!(mouse_on(&h("12 07 00 00 04"), 0x00, MOUSE_QUARTERS), Some(about(100, false)));
        assert_eq!(mouse_on(&h("12 07 00 00 04"), 0x00, STRIX_CARRY_QUARTERS), Some(asleep()));
    }

    #[test]
    fn strix_carry_has_the_power_off_setting_first_and_the_level_two_bytes_on() {
        assert_eq!(mouse_on(&h("12 07 00 00 02 01 03"), 0x00, STRIX_CARRY_QUARTERS), Some(about(75, false)));
    }

    #[test]
    fn strix_carry_runs_on_aa_cells_and_never_charges() {
        // Made up: whatever its reply has in the byte that says "charging" on the other mice
        assert_eq!(mouse_on(&h("12 07 00 00 02 01 03 00 00 01"), 0x00, STRIX_CARRY_QUARTERS), Some(about(75, false)));
        assert_eq!(mouse_on(&h("12 07 00 00 02 01 00 00 00 01"), 0x00, STRIX_CARRY_QUARTERS), Some(asleep()));
    }

    #[test]
    fn a_byte_that_cannot_be_a_level_is_no_level() {
        assert_eq!(mouse_on(&h("12 07 00 00 65"), 0x00, MOUSE_PERCENT), Some(level_unknown(false)));
        assert_eq!(mouse_on(&h("12 07 00 00 05"), 0x00, MOUSE_QUARTERS), Some(level_unknown(false)));
        assert_eq!(mouse_on(&h("12 07 00 00 4b"), 0x00, MOUSE_QUARTERS), Some(level_unknown(false)));
        assert_eq!(keyboard_on(&h("12 01 00 00 00 fe 00 00 01"), 0x00, KEYBOARD_PERCENT), Some(level_unknown(true)));
    }

    #[test]
    fn pair_list_with_a_keyboard_and_a_mouse() {
        // HaloBattery: a Falchion RX Low Profile and a Harpe Ace Mini
        let slots = parse_pairs(&h("01 a0 00 02 00 06 1b 02 04 65 1b 03 05")).unwrap();
        assert_eq!(slots, [Slot { pid: 0x1B06, channel: 0x02 }, Slot { pid: 0x1B65, channel: 0x03 }]);
        assert_eq!(
            omni_mouse(Some(&slots)),
            Some(Paired { key: "asus-rog-harpe-ace-mini", name: "ROG Harpe Ace Mini", listed: true })
        );
        let (keyboard, level_at) = omni_keyboard(&slots).unwrap();
        assert_eq!(keyboard.name, "ROG Falchion RX Low Profile");
        assert_eq!(level_at, FALCHION_PERCENT);
    }

    #[test]
    fn pair_list_with_a_keyboard_only_leaves_the_mouse_channel_alone() {
        // rogctl: the Falchion RX Low Profile alone; HaloBattery's issue 340: the Strix Morph 96
        let falchion = parse_pairs(&h("01 a0 00 01 00 06 1b 02 04")).unwrap();
        assert_eq!(falchion, [Slot { pid: 0x1B06, channel: 0x02 }]);
        assert_eq!(omni_mouse(Some(&falchion)), None);

        let morph = parse_pairs(&h("01 a0 00 01 00 ea 1c 02 04")).unwrap();
        assert_eq!(omni_mouse(Some(&morph)), None);
        let (keyboard, level_at) = omni_keyboard(&morph).unwrap();
        assert_eq!((keyboard.key, level_at), ("asus-rog-strix-morph-96-wireless", KEYBOARD_PERCENT));
    }

    #[test]
    fn an_empty_pair_list_has_nobody_to_ask() {
        let slots = parse_pairs(&h("01 a0 00 00 00")).unwrap();
        assert_eq!(slots, []);
        assert_eq!(omni_mouse(Some(&slots)), None);
        assert_eq!(omni_keyboard(&slots), None);
    }

    #[test]
    fn a_mouse_the_table_does_not_have_is_read_without_a_model_name() {
        let slots = parse_pairs(&h("01 a0 00 02 00 34 12 03 05 35 12 02 04")).unwrap();
        assert_eq!(omni_mouse(Some(&slots)), Some(Paired { key: UNNAMED_MOUSE, name: "", listed: true }));
        // A keyboard isn't: where its level is depends on the model
        assert_eq!(omni_keyboard(&slots), None);
    }

    #[test]
    fn without_a_pair_list_the_mouse_is_shown_only_once_it_answers() {
        assert_eq!(omni_mouse(None), Some(Paired { key: UNNAMED_MOUSE, name: "", listed: false }));
    }

    #[test]
    fn a_receiver_that_misses_one_answer_is_taken_at_its_last_word() {
        let mut p = AsusReceiverProvider::default();
        // Nothing said yet: nobody is asked under a name the next poll would change
        assert_eq!(p.pair_list("1ace a", None), None);
        let list = parse_pairs(&h("01 a0 00 02 00 06 1b 02 04 65 1b 03 05"));
        assert_eq!(p.pair_list("1ace a", Some(list.clone())), Some(list.clone()));
        assert_eq!(p.pair_list("1ace a", None), Some(list));
        // A refusal is an answer too, and another receiver has its own
        assert_eq!(p.pair_list("1ace b", Some(parse_pairs(&h("01 ff aa")))), Some(None));
        assert_eq!(p.pair_list("1ace b", None), Some(None));
        assert_eq!(p.pair_list("1ace c", None), None);
    }

    #[test]
    fn a_refusal_or_another_report_is_not_a_pair_list() {
        assert!(is_pairs_answer(&h("01 ff aa")));
        assert_eq!(parse_pairs(&h("01 ff aa")), None);
        assert!(!is_pairs_answer(&h("03 12 07 00 00 35")));
        assert_eq!(parse_pairs(&h("03 a0 00 01 00 06 1b 02 04")), None);
        assert_eq!(parse_pairs(&[0x01, 0xA0, 0x00]), None);
    }

    #[test]
    fn the_speednova_mouse_is_named_by_the_receivers_product_string() {
        assert_eq!(speednova_mouse(Some("ROG SPEEDNOVA 8K RECEIVER")).name, "ROG Harpe II Ace");
        assert_eq!(speednova_mouse(Some("ROG HARPE II EXTREME")).name, "ROG Harpe II Extreme Edition 20");
        assert_eq!(speednova_mouse(None).key, "asus-rog-harpe-ii-ace");
        // WDBM's issue 140: a receiver without a mouse answers "asleep" all the same
        assert!(!speednova_mouse(None).listed);
    }

    #[test]
    fn a_receivers_channels_are_told_by_their_usage_pages() {
        let all = [
            collection(OMNI, 2, 0xFF02, 64, 64),
            collection(OMNI, 2, 0xFF00, 64, 64),
            collection(OMNI, 2, 0xFF01, 64, 64),
            collection(OMNI, 2, 0xFFC0, 64, 64),
        ];
        assert_eq!(channel(&all, PAIRS).map(|i| i.usage_page), Some(0xFF02));
        assert_eq!(channel(&all, KEYBOARD).map(|i| i.usage_page), Some(0xFF00));
        assert_eq!(channel(&all, MOUSE).map(|i| i.usage_page), Some(0xFF01));
    }

    #[test]
    fn a_locked_or_short_collection_is_no_channel() {
        let locked = [HidInfo { access_denied: true, ..collection(OMNI, 2, 0xFF01, 64, 64) }];
        assert!(channel(&locked, MOUSE).is_none());
        assert!(channel(&[collection(OMNI, 2, 0xFF01, 8, 0)], MOUSE).is_none());
        assert_eq!(no_link_reason(&locked), "d.needsAdmin");
    }

    #[test]
    fn collections_of_one_receiver_share_their_instance() {
        let col1 = r"\\?\hid#vid_0b05&pid_1ace&mi_02&col01#8&2a1b3c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let col3 = r"\\?\hid#vid_0b05&pid_1ace&mi_02&col03#8&2a1b3c&0&0002#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let other = r"\\?\hid#vid_0b05&pid_1ace&mi_02&col03#8&77aa01&0&0002#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(instance(col1), "8&2a1b3c&0");
        assert_eq!(instance(col3), instance(col1));
        assert_ne!(instance(other), instance(col1));
        assert_eq!(instance("no such path"), "no such path");
    }

    #[test]
    fn a_receiver_gives_way_to_the_cable_of_the_same_model() {
        assert!(is_plugged_in("asus-rog-harpe-ii-ace", &[0x1C69]));
        assert!(is_plugged_in("asus-rog-azoth", &[0x1234, 0x1A83]));
        assert!(!is_plugged_in("asus-rog-harpe-ii-ace", &[0x1B63]));
        assert!(!is_plugged_in("asus-rog-strix-impact-iii-wireless", &[0x1C69]));
        assert!(!is_plugged_in(UNNAMED_MOUSE, &[0x1C69]));
    }

    #[test]
    fn a_device_behind_a_receiver_has_the_id_of_its_row() {
        let device = Paired { key: "asus-rog-azoth", name: "ROG Azoth", listed: true };
        let reading = Behind { device, kind: DeviceKind::Keyboard, state: on(77, true) }.reading();
        let row = SPECS.iter().find(|s| s.key == "asus-rog-azoth").unwrap();
        assert_eq!((reading.id, reading.kind), (row.id(), row.kind));
        assert_eq!((reading.level, reading.charging, reading.estimate), (Some(77), true, false));
    }

    #[test]
    fn a_model_keeps_its_key_name_and_kind_behind_the_omni_receiver() {
        let behind = OMNI_MICE
            .iter()
            .map(|m| (m.1, m.2, DeviceKind::Mouse))
            .chain(OMNI_KEYBOARDS.iter().map(|k| (k.1, k.2, DeviceKind::Keyboard)));
        for (key, name, kind) in behind {
            for spec in SPECS.iter().filter(|s| s.key == key) {
                assert_eq!((spec.name, spec.kind), (name, kind), "{key}");
            }
        }
        for key in ["asus-rog-harpe-ii-ace", "asus-rog-harpe-ii-extreme", UNNAMED_MOUSE] {
            assert!(SPECS.iter().all(|s| s.key != key || s.kind == DeviceKind::Mouse), "{key}");
        }
    }

    #[test]
    fn keys_are_lowercase_with_hyphens_and_ids_are_asus_ones() {
        let behind = OMNI_MICE.iter().map(|m| m.1).chain(OMNI_KEYBOARDS.iter().map(|k| k.1));
        for key in SPECS.iter().map(|s| s.key).chain(behind) {
            assert!(key.starts_with("asus-"), "{key}");
            assert!(key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'), "{key}");
        }
        for spec in SPECS {
            assert_eq!(spec.vid, VID, "{}", spec.key);
            assert!(spec.pids.iter().all(|p| !is_receiver(*p)), "{}: a shared receiver is not a row", spec.key);
        }
    }

    #[test]
    fn a_mouse_is_asked_on_the_vendor_collection_of_its_interface() {
        // The collections of the Gladius III Wireless AimPoint in HaloBattery's issue 81, and a plain one beside the
        // control collection
        let all = [
            collection(0x1A72, 0, 0xFF01, 65, 65),
            collection(0x1A72, 0, 0x0001, 9, 0),
            collection(0x1A72, 2, 0xFFC1, 65, 65),
            collection(0x1A72, 2, 0x0001, 9, 2),
        ];
        assert_eq!(asked(0x1A72, &all), [(0, 0xFF01)]);
    }

    #[test]
    fn a_keyboard_is_asked_on_its_ff00_collection() {
        let all = [
            collection(0x1A83, 0, 0x0001, 9, 2),
            collection(0x1A83, 1, 0xFF00, 65, 65),
            collection(0x1A83, 2, 0x000C, 5, 0),
            collection(0x1A83, 2, 0xFF00, 65, 65),
        ];
        assert_eq!(asked(0x1A83, &all), [(1, 0xFF00)]);
    }

    #[test]
    fn every_row_names_its_interface() {
        // A row that takes its collection wherever it is would ask every vendor collection of the device
        for spec in SPECS {
            assert!(spec.interface >= 0, "{}", spec.key);
        }
    }
}
