//! Mice on one ODM firmware platform whose configuration tools all speak the same "page command" protocol (the
//! vendors' tools call it XVI or DriverCore): the Glorious wireless line on Sinowealth's vendor ID, LAMZU's Aurora
//! generation, WLmouse, Arbiter Studio Akitsu, Attack Shark R5 Ultra / R6 / R8, CRDRAKO KO-ONE, LunaFury, the first
//! MCHOSE A5 and G-Wolves. Everything travels as feature report 0, 64 bytes, on the one collection that declares it
//! (FFFF:0000 on interface 2 where it was captured). A request is written with SET_FEATURE and the answer read back
//! with GET_FEATURE some tens of milliseconds later; until it is there, the request itself reads back.
//!   request  00 00 TT LL PP CC args..   TT who is asked: 02 the mouse, 00 the device on the cable itself, 01 the
//!                                       receiver about its pairing; LL length of the data wanted; PP page; CC command
//!   reply    SS 00 TT LL PP CC data..   SS A1 = done, A0 = busy or waking up, A2 = refused, A4 = the receiver cannot
//!                                       find the mouse; LL length of the data that follows
//! The battery is page 00, command 83: two bytes of data, 01 while charging and the level in percent. A receiver
//! whose mouse is off answers A4, A0, a level of 0 or nothing at all, depending on the brand: all of them are "off"
//! here. A receiver that several models share says which mouse it holds on page 00, command 8B (the mouse's vendor
//! and product ID); it is asked only where a tool is known to ask, and the other shared receivers go by the brand's
//! name. G-Wolves' older models keep the transport and ask in an older layout: `00 02 8F WW` (WW 01
//! through a receiver, 00 on the cable), answered `A1 02 8F .. CC PP`, CC charging and PP the level.
//! Only these reads are sent; nothing is written to the mouse and no mode is changed. A receiver passes the battery
//! request on to its mouse over the radio, and the tools this follows allow for a dozing mouse answering late, so
//! the request may wake one.
//! Protocol facts from OpenMouse (mouse-protocol: src/compx/codec.ts, src/drivers/lamzu/hid.ts,
//! src/drivers/wlmouse/hid.ts, src/glorious-core2/index.ts, src/drivers/gwolves/xvi-hid.ts,
//! docs/lamzu-inca-testing.md, docs/vancer-akitsu.md and the captures under captures/vancer-akitsu-medium and
//! captures/glorious-o2-pro-4k8k-wired), mow (src/report/battery.rs, src/util/devices.rs), GloriousBatteryMonitor
//! (Mice/*.cs), Wireless-Device-Battery-Monitor (src/drivers/glorious.cpp, lamzu.cpp, wlmouse.cpp, arbiter.cpp,
//! mchose_a5.cpp), HaloBattery (docs/protocols.md, providers/gwolves.py, providers/wlmouse.py), lamzu-battery
//! (src/protocol/aurora.rs) and mchose-macos (README). Product IDs are the ones the vendors' own web configurators
//! list (Config/env-models.json of lamzu.net, gm.wlmouse.gg, webapp.arbiterstudio.com and mouse.xyz), which also say
//! which of the two battery requests a model takes. Own implementation. Not yet verified on hardware by this
//! project, which is why the first exchange with each mouse goes to the log.

use super::{off, off_because, on, Io, Spec, State};
use crate::hid::HidInfo;
use crate::loc::tf;
use crate::providers::sleep_ms;
use crate::win;

/// A feature report as Windows counts it: the report ID and the 64 bytes.
const REPORT_LENGTH: usize = 65;
const REPORT_ID: u8 = 0x00;

const TARGET_SELF: u8 = 0x00;
const TARGET_PAIRING: u8 = 0x01;
const TARGET_MOUSE: u8 = 0x02;
const PAGE_DEVICE: u8 = 0x00;
const BATTERY: u8 = 0x83;
const PAIRED_MOUSE: u8 = 0x8B;
const LEGACY_BATTERY: u8 = 0x8F;

const STATUS_DONE: u8 = 0xA1;
/// What a Glorious receiver answers when it cannot find its mouse.
const STATUS_NO_MOUSE: u8 = 0xA4;
/// What G-Wolves' configurator takes for "the mouse is asleep".
const STATUS_ASLEEP: u8 = 0x02;

/// How often a request is sent, how often the answer is looked for after each, and the time between two looks. The
/// tools this follows wait 30 to 100 ms for the trip to the mouse and back and ask again after about 300 ms.
const SENDS: usize = 2;
const READS: usize = 5;
const WAIT_MS: u64 = 60;

const GLORIOUS: u16 = 0x258A;
const COMPX: u16 = 0x373E;
const LAMZU: u16 = 0x37B0;
const WLMOUSE: u16 = 0x36A7;
const MCHOSE: u16 = 0x2023;
const GWOLVES: u16 = 0x33E4;

/// Mice that answer on their cable only when asked as the device itself (OpenMouse's `mouseTarget`): the others
/// take 02 on the cable as well as through the receiver.
const ASKED_AS_ITSELF: &[(u16, u16)] = &[(COMPX, 0x006A), (COMPX, 0x0032), (COMPX, 0x0054)];

/// Receivers that come with several models, or are sold on their own, with the brand to show while the receiver
/// doesn't say which mouse it holds.
const SHARED_RECEIVERS: &[((u16, u16), &str)] = &[
    ((LAMZU, 0x0001), "LAMZU"),
    ((LAMZU, 0x0003), "LAMZU"),
    ((LAMZU, 0x002E), "LAMZU"),
    ((LAMZU, 0x0036), "LAMZU"),
    ((LAMZU, 0x0048), "LAMZU"),
    ((WLMOUSE, 0xA882), "WLmouse"),
    ((GWOLVES, 0x3817), "G-Wolves"),
];

/// The shared receivers that are asked which mouse they hold: the ones OpenMouse sends that request to (LAMZU's two
/// Aurora receivers, the 8K one seen answering it, and WLmouse's). No tool is known to send it to the others, so
/// they keep the brand's name.
const NAMES_ITS_MOUSE: &[(u16, u16)] = &[(LAMZU, 0x0001), (LAMZU, 0x0003), (WLMOUSE, 0xA882)];

/// Receiver IDs first, then the mouse on its cable. Two rows share a key where one model has IDs under two vendors.
pub static SPECS: &[Spec] = &[
    // ---- Glorious, on Sinowealth's vendor ID. The first four are mow's, confirmed by their owners; the rest are
    // the pairs GloriousBatteryMonitor reads the same way, and the Model O 2 PRO 4K/8KHz was captured on its cable
    glorious("glorious-model-o-wireless", "Glorious Model O Wireless", &[0x2022, 0x2011], &[0x2011]),
    glorious("glorious-model-o-minus-wireless", "Glorious Model O- Wireless", &[0x2024, 0x2013], &[0x2013]),
    glorious("glorious-model-d-wireless", "Glorious Model D Wireless", &[0x2023, 0x2012], &[0x2012]),
    glorious("glorious-model-d-minus-wireless", "Glorious Model D- Wireless", &[0x2025, 0x2014], &[0x2014]),
    glorious("glorious-model-o-pro", "Glorious Model O PRO Wireless", &[0x2027, 0x2015], &[0x2015]),
    glorious("glorious-series-one-pro", "Glorious Series One PRO Wireless", &[0x2031, 0x2018], &[0x2018]),
    glorious("glorious-model-o-2-pro", "Glorious Model O 2 PRO", &[0x2033, 0x2019], &[0x2019]),
    glorious("glorious-model-d-2-pro", "Glorious Model D 2 PRO", &[0x2034, 0x201A], &[0x201A]),
    glorious("glorious-series-2-pro", "Glorious Series 2 PRO", &[0x2039, 0x201D], &[0x201D]),
    glorious("glorious-model-o-2-pro-8k", "Glorious Model O 2 PRO 4K/8KHz Edition", &[0x2035, 0x201B], &[0x201B]),
    glorious("glorious-model-d-2-pro-8k", "Glorious Model D 2 PRO 4K/8KHz Edition", &[0x2036, 0x201C], &[0x201C]),
    // ---- LAMZU, Aurora generation. The Atlantis and the older Thorn / Maya on 3554 speak another protocol
    page("lamzu-maya-x", "LAMZU Maya X", COMPX, &[0x001D, 0x001E, 0x001C], &[0x001C]),
    page("lamzu-maya-x", "LAMZU Maya X", LAMZU, &[0x003B, 0x003A, 0x0044], &[0x003A, 0x0044]),
    page("lamzu-maya-x-v2", "LAMZU Maya X V2", COMPX, &[0x0016], &[]),
    page("lamzu-maya-x-v2", "LAMZU Maya X V2", LAMZU, &[0x001F], &[0x001F]),
    page("lamzu-tachi", "LAMZU Tachi", LAMZU, &[0x000B, 0x000C, 0x0005], &[0x0005]),
    page("lamzu-tachi-lite", "LAMZU Tachi Lite", LAMZU, &[0x001E, 0x001C], &[0x001C]),
    page("lamzu-inca", "LAMZU Inca", LAMZU, &[0x000F, 0x0010, 0x0009], &[0x0009]),
    page("lamzu-inca-v2", "LAMZU Inca V2", LAMZU, &[0x005A], &[0x005A]),
    page("lamzu-maya", "LAMZU Maya", LAMZU, &[0x0013, 0x0015, 0x0011, 0x003C, 0x0050], &[0x0011, 0x003C, 0x0050]),
    page("lamzu-maya-m", "LAMZU Maya M", LAMZU, &[0x002C, 0x0052], &[0x002C, 0x0052]),
    page("lamzu-paro", "LAMZU Paro", LAMZU, &[0x000D, 0x000E, 0x0007], &[0x0007]),
    page("lamzu-thorn", "LAMZU Thorn", LAMZU, &[0x0019, 0x001B, 0x0017], &[0x0017]),
    page(
        "lamzu-thorn-v2",
        "LAMZU Thorn V2",
        LAMZU,
        &[0x0023, 0x0032, 0x0021, 0x0030, 0x0040],
        &[0x0021, 0x0030, 0x0040],
    ),
    page("lamzu-atlantis-og-champion", "LAMZU Atlantis OG Champion", LAMZU, &[0x0027, 0x0025], &[0x0025]),
    page("lamzu-atlantis-mini", "LAMZU Atlantis Mini", LAMZU, &[0x002B, 0x002A, 0x0028], &[0x0028]),
    page("lamzu-orcus", "LAMZU Orcus", LAMZU, &[0x0042], &[0x0042]),
    page("lamzu-orcus-v2", "LAMZU Orcus V2", LAMZU, &[0x0056], &[0x0056]),
    page("lamzu-mini", "LAMZU Mini", LAMZU, &[0x0054, 0x0046], &[0x0046]),
    // Shared receivers: the name comes from the mouse they say they hold (see SHARED_RECEIVERS)
    page("lamzu-aurora-8k-receiver", "LAMZU mouse", LAMZU, &[0x0001], &[]),
    page("lamzu-aurora-1k-receiver", "LAMZU mouse", LAMZU, &[0x0003], &[]),
    page("lamzu-receiver-002e", "LAMZU mouse", LAMZU, &[0x002E], &[]),
    page("lamzu-receiver-0036", "LAMZU mouse", LAMZU, &[0x0036], &[]),
    page("lamzu-receiver-0048", "LAMZU mouse", LAMZU, &[0x0048], &[]),
    // ---- Other brands on the ODM's vendor ID
    page("arbiter-akitsu", "Arbiter Studio Akitsu", COMPX, &[0x000A, 0x0009], &[0x0009]),
    page("arbiter-akitsu-3950", "Arbiter Studio Akitsu 3950", COMPX, &[0x0039, 0x0038], &[0x0038]),
    page("arbiter-akitsu-medium-3950", "Arbiter Studio Akitsu Medium 3950", COMPX, &[0x0067, 0x0066], &[0x0066]),
    page("attack-shark-r5-ultra", "Attack Shark R5 Ultra", COMPX, &[0x0047, 0x0046], &[0x0046]),
    page("attack-shark-r6", "Attack Shark R6", COMPX, &[0x0022, 0x0021], &[0x0021]),
    page("attack-shark-r8", "Attack Shark R8", COMPX, &[0x003B, 0x003A], &[0x003A]),
    page("crdrako-ko-one", "CRDRAKO KO-ONE", COMPX, &[0x006B, 0x006A], &[0x006A]),
    page("lunafury-luna33", "LunaFury LUNA33", COMPX, &[0x0033, 0x0032], &[0x0032]),
    page("lunafury-type33", "LunaFury TYPE33", COMPX, &[0x0084, 0x0054], &[0x0054]),
    // ---- WLmouse. A887 / A888 (Beast X 4K, the first Beast X) look the same and never answer this: not claimed
    page("wlmouse-beast-x", "WLmouse Beast X", WLMOUSE, &[0xA883, 0xA884], &[0xA884]),
    page("wlmouse-beast-x-pro", "WLmouse Beast X Pro", WLMOUSE, &[0xA870, 0xA871], &[0xA871]),
    page("wlmouse-beast-x-mini", "WLmouse Beast X Mini", WLMOUSE, &[0xA885, 0xA886], &[0xA886]),
    page("wlmouse-beast-x-mini-pro", "WLmouse Beast X Mini Pro", WLMOUSE, &[0xA868, 0xA869], &[0xA869]),
    page("wlmouse-beast-x-max", "WLmouse Beast X Max", WLMOUSE, &[0xA880, 0xA881], &[0xA881]),
    page("wlmouse-beast-x-v2", "WLmouse Beast X V2", WLMOUSE, &[0xA856, 0xA857], &[0xA857]),
    page("wlmouse-beast-g", "WLmouse Beast G", WLMOUSE, &[0xA860, 0xA861], &[0xA861]),
    page("wlmouse-beast-miao", "WLmouse Beast Miao", WLMOUSE, &[0xA866, 0xA867], &[0xA867]),
    // One 8K receiver for the Huan and the Huan M
    page("wlmouse-huan", "WLmouse Huan / Huan M", WLMOUSE, &[0xA863, 0xA864, 0xA859], &[0xA864, 0xA859]),
    page("wlmouse-strider", "WLmouse Strider", WLMOUSE, &[0xA872, 0xA873], &[0xA873]),
    page("wlmouse-ying", "WLmouse Ying", WLMOUSE, &[0xA874, 0xA875], &[0xA875]),
    page("wlmouse-sword-x", "WLmouse Sword X", WLMOUSE, &[0xA878, 0xA879], &[0xA879]),
    page("wlmouse-1k-receiver", "WLmouse mouse", WLMOUSE, &[0xA882], &[]),
    // ---- MCHOSE A5, first generation: the two receivers came with either model
    page("mchose-a5-pro", "MCHOSE A5 Pro / A5 Pro Max", MCHOSE, &[0xF013, 0xF015, 0xF019, 0xF017], &[0xF019, 0xF017]),
    // ---- G-Wolves models their configurator marks "new protocol". 3817 is the HTM Plus's receiver and, in the same
    // table, the receiver the other three can come with: it gets the brand's name, not one model's
    page("gwolves-htm-plus", "G-Wolves HTM Plus", GWOLVES, &[0x3808], &[0x3808]),
    page("gwolves-receiver-3817", "G-Wolves mouse", GWOLVES, &[0x3817], &[]),
    page("gwolves-hsk-pro-2", "G-Wolves HSK Pro 2.0", GWOLVES, &[0x6817, 0x6808], &[0x6808]),
    page("gwolves-htxu", "G-Wolves HTXU", GWOLVES, &[0x5617, 0x5608], &[0x5608]),
    page("gwolves-fenrir-pro", "G-Wolves Fenrir Pro", GWOLVES, &[0x3617, 0x3608], &[0x3608]),
    // ---- G-Wolves models on the older request. Receivers ending in 07 are the 4K ones. The mice on the shared
    // receiver 33E4:3854 are another protocol and not these
    legacy("gwolves-fenrir", "G-Wolves Fenrir", &[0x3517, 0x3508], &[0x3508]),
    legacy("gwolves-fenrir-max", "G-Wolves Fenrir Max", &[0x3717, 0x3708], &[0x3708]),
    legacy("gwolves-vuk", "G-Wolves VUK", &[0x3917, 0x3908], &[0x3908]),
    legacy("gwolves-htx-mini", "G-Wolves HTX Mini", &[0x2717, 0x2708], &[0x2708]),
    legacy("gwolves-htx", "G-Wolves HTX", &[0x5717, 0x5707, 0x5708], &[0x5708]),
    legacy("gwolves-htx-ace", "G-Wolves HTX ACE", &[0x5703, 0x5704], &[0x5704]),
    legacy("gwolves-hts-plus", "G-Wolves HTS Plus", &[0x5417, 0x5407, 0x5408], &[0x5408]),
    legacy("gwolves-hts-plus-ace", "G-Wolves HTS Plus ACE", &[0x5403, 0x5404], &[0x5404]),
    legacy("gwolves-hts-ultra", "G-Wolves HTS Ultra", &[0x5317, 0x5308], &[0x5308]),
    legacy("gwolves-hsk-pro", "G-Wolves HSK Pro", &[0x5817, 0x5807, 0x5808], &[0x5808]),
    legacy("gwolves-hsk-pro-ace", "G-Wolves HSK Pro ACE", &[0x5803, 0x5804], &[0x5804]),
    legacy("gwolves-hsk-plus", "G-Wolves HSK Plus", &[0x5917, 0x5907, 0x5908], &[0x5908]),
    legacy("gwolves-hsk-plus-ace", "G-Wolves HSK Plus ACE", &[0x5903, 0x5904], &[0x5904]),
    legacy("gwolves-hsk-lite", "G-Wolves HSK Lite", &[0x7203, 0x7204], &[0x7204]),
    legacy("gwolves-htr", "G-Wolves HTR", &[0x7713, 0x7704], &[0x7704]),
    legacy("gwolves-htr-pro", "G-Wolves HTR Pro", &[0x7717, 0x7708], &[0x7708]),
    legacy("gwolves-ht-s2", "G-Wolves HT-S2", &[0x7913, 0x7904], &[0x7904]),
    legacy("gwolves-ht-s2-pro", "G-Wolves HT-S2 Pro", &[0x7917, 0x7908], &[0x7908]),
];

/// A mouse asked with the page command. 373E is the ODM's own vendor ID, which every brand it builds for can carry
/// (some with another protocol): a device on it without the report's collection is not listed at all.
const fn page(
    key: &'static str,
    name: &'static str,
    vid: u16,
    pids: &'static [u16],
    cable_pids: &'static [u16],
) -> Spec {
    Spec {
        key,
        name,
        vid,
        pids,
        cable_pids,
        accept: Some(takes_the_report),
        shared: vid == COMPX,
        query: page_battery,
        ..Spec::MOUSE
    }
}

/// 258A is Sinowealth's vendor ID, which keyboards and mice of many brands carry: the collection has to be a vendor
/// one, and a device without it is not listed at all.
const fn glorious(key: &'static str, name: &'static str, pids: &'static [u16], cable_pids: &'static [u16]) -> Spec {
    Spec { accept: Some(vendor_and_takes_the_report), shared: true, ..page(key, name, GLORIOUS, pids, cable_pids) }
}

/// A G-Wolves mouse asked the older way.
const fn legacy(key: &'static str, name: &'static str, pids: &'static [u16], cable_pids: &'static [u16]) -> Spec {
    Spec { query: legacy_battery, ..page(key, name, GWOLVES, pids, cable_pids) }
}

/// The configuration collection is the one with a 64-byte feature report: the vendors' web configurators find it by
/// that and not by its usage. The vendor collections next to it (FFA0:0001, FFFF:0001) have no feature report.
fn takes_the_report(info: &HidInfo) -> bool {
    info.feature_length == REPORT_LENGTH
}

fn vendor_and_takes_the_report(info: &HidInfo) -> bool {
    info.usage_page >= 0xFF00 && takes_the_report(info)
}

/// What one GET_FEATURE says about the request before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply<T> {
    Data(T),
    /// The receiver says its mouse is asleep or gone.
    Asleep,
    /// No answer yet: the request still reads back, the device is busy, or what reads back answers something else.
    Pending,
}

impl<T> Reply<T> {
    /// The data read further; data that doesn't hold up is no answer.
    fn and_then<U>(self, read: impl FnOnce(T) -> Option<U>) -> Reply<U> {
        match self {
            Reply::Data(data) => read(data).map_or(Reply::Pending, Reply::Data),
            Reply::Asleep => Reply::Asleep,
            Reply::Pending => Reply::Pending,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level: i32,
    pub charging: bool,
    pub full: bool,
}

/// A page command as SET_FEATURE takes it, report ID first; the engine pads it to the report's length.
pub fn request(target: u8, length: u8, page: u8, command: u8, args: &[u8]) -> Vec<u8> {
    let mut report = vec![REPORT_ID, 0x00, 0x00, target, length, page, command];
    report.extend_from_slice(args);
    report
}

/// G-Wolves' older battery request, report ID first.
pub fn legacy_request(on_cable: bool) -> Vec<u8> {
    vec![REPORT_ID, 0x00, 0x02, LEGACY_BATTERY, if on_cable { 0x00 } else { 0x01 }]
}

/// What a status byte says, for a report that is already known to answer our command.
fn by_status<T>(status: u8, data: T) -> Reply<T> {
    match status {
        STATUS_DONE => Reply::Data(data),
        STATUS_NO_MOUSE | STATUS_ASLEEP => Reply::Asleep,
        _ => Reply::Pending,
    }
}

/// The data of a reply to a page command, as GET_FEATURE returns it (report ID first). The page and the command
/// have to be the ones asked for: the report keeps the last answer, whatever it was to.
pub fn page_data(r: &[u8], page: u8, command: u8) -> Reply<&[u8]> {
    if r.len() < 7 || r[0] != REPORT_ID || r[5] != page || r[6] != command {
        return Reply::Pending;
    }
    let end = (7 + r[4] as usize).min(r.len());
    by_status(r[1], &r[7..end])
}

/// The two data bytes of a battery answer. A level over 100 is no reading (the MCHOSE A5 sends FF for "not known").
/// Neither is 0 on battery: a receiver answers that for a mouse that is switched off, and some answer it before the
/// mouse's own reply has come in. 02 in the first byte is "full, on the cable" in two of the sources.
pub fn battery(charge: u8, level: u8) -> Option<Battery> {
    if level > 100 || (level == 0 && charge != 0x01) {
        return None;
    }
    Some(Battery { level: level as i32, charging: charge == 0x01, full: charge == 0x02 })
}

pub fn battery_reply(r: &[u8]) -> Reply<Battery> {
    page_data(r, PAGE_DEVICE, BATTERY).and_then(|d| if d.len() >= 2 { battery(d[0], d[1]) } else { None })
}

/// The mouse a shared receiver holds: its vendor and product ID (the mouse's ID on the cable), big-endian, after
/// two bytes that aren't used here.
pub fn paired_reply(r: &[u8]) -> Reply<(u16, u16)> {
    page_data(r, PAGE_DEVICE, PAIRED_MOUSE)
        .and_then(|d| (d.len() >= 6).then(|| (u16::from_be_bytes([d[2], d[3]]), u16::from_be_bytes([d[4], d[5]]))))
}

/// The answer to G-Wolves' older request: status, the length of its data (2), the command, a byte that isn't used,
/// then charging and the level.
pub fn legacy_reply(r: &[u8]) -> Reply<Battery> {
    if r.len() < 7 || r[0] != REPORT_ID || r[2] != 0x02 || r[3] != LEGACY_BATTERY {
        return Reply::Pending;
    }
    by_status(r[1], (r[5], r[6])).and_then(|(charge, level)| battery(charge, level))
}

pub fn state(reply: Reply<Battery>) -> State {
    match reply {
        Reply::Data(b) => State { full: b.full, ..on(b.level, b.charging) },
        Reply::Asleep => off_because("d.asleep"),
        Reply::Pending => off(),
    }
}

/// The brand of a receiver that doesn't say the model by its ID.
fn shared_receiver(id: (u16, u16)) -> Option<&'static str> {
    SHARED_RECEIVERS.iter().find(|(shared, _)| *shared == id).map(|(_, brand)| *brand)
}

/// The name of the model with this ID.
fn model(vid: u16, pid: u16) -> Option<&'static str> {
    if shared_receiver((vid, pid)).is_some() {
        return None;
    }
    SPECS.iter().find(|s| s.vid == vid && s.pids.contains(&pid)).map(|s| s.name)
}

fn is_cable(vid: u16, pid: u16) -> bool {
    SPECS.iter().any(|s| s.vid == vid && s.cable_pids.contains(&pid))
}

/// Sends a request and looks for its answer. A receiver relays the request to its mouse, so the first looks find
/// the request still there.
fn ask<T>(io: &mut Io, request: &[u8], sends: usize, read: impl Fn(&[u8]) -> Reply<T>) -> Reply<T> {
    for _ in 0..sends {
        if !io.set_feature(request) {
            break;
        }
        for _ in 0..READS {
            sleep_ms(WAIT_MS);
            let Some(r) = io.get_feature(REPORT_ID) else { return Reply::Pending };
            match read(&r) {
                Reply::Pending => {}
                answer => return answer,
            }
        }
    }
    Reply::Pending
}

fn page_battery(io: &mut Io) -> win::Result<State> {
    let id = (io.info().vendor_id, io.product_id());
    let target = if ASKED_AS_ITSELF.contains(&id) { TARGET_SELF } else { TARGET_MOUSE };
    let mut state = state(ask(io, &request(target, 2, PAGE_DEVICE, BATTERY, &[]), SENDS, battery_reply));
    if let Some(brand) = shared_receiver(id) {
        let name = if NAMES_ITS_MOUSE.contains(&id) {
            // The receiver answers this one itself, so the name stays while the mouse is off
            let paired = ask(io, &request(TARGET_PAIRING, 6, PAGE_DEVICE, PAIRED_MOUSE, &[0x02]), 1, paired_reply);
            match paired {
                Reply::Data((vid, pid)) => model(vid, pid),
                _ => None,
            }
        } else {
            None
        };
        state.name = Some(name.map_or_else(|| tf("name.brandMouse", &[&brand]), str::to_string));
    }
    Ok(state)
}

fn legacy_battery(io: &mut Io) -> win::Result<State> {
    let on_cable = is_cable(io.info().vendor_id, io.product_id());
    Ok(state(ask(io, &legacy_request(on_cable), SENDS, legacy_reply)))
}

#[cfg(test)]
mod tests {
    //! The frames are the ones OpenMouse publishes: the Vancer Akitsu Medium 3950 on its 8K receiver and its cable
    //! (captures/vancer-akitsu-medium), Glorious CORE reading a Model O 2 PRO 4K/8KHz on its cable
    //! (captures/glorious-o2-pro-4k8k-wired/core-session.hex), the LAMZU Inca's data bytes
    //! (docs/lamzu-inca-testing.md) and the Paro behind an Aurora receiver (src/drivers/lamzu/hid.test.ts). They
    //! are published without the report ID, which Windows puts in front.
    use super::*;
    use crate::providers::pad;

    /// A report as GET_FEATURE returns it on Windows: the report ID, the bytes, zeros up to 65.
    fn report(text: &str) -> Vec<u8> {
        let mut r = vec![REPORT_ID];
        r.extend(text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()));
        pad(REPORT_LENGTH, &r)
    }

    fn b(level: i32, charging: bool) -> Reply<Battery> {
        Reply::Data(Battery { level, charging, full: false })
    }

    fn collection(page: u16, usage: u16, feature: usize) -> HidInfo {
        HidInfo { usage_page: page, usage, feature_length: feature, ..HidInfo::default() }
    }

    fn spec(vid: u16, pid: u16) -> &'static Spec {
        SPECS.iter().find(|s| s.vid == vid && s.pids.contains(&pid)).unwrap()
    }

    #[test]
    fn battery_request_is_the_captured_one() {
        let sent = pad(REPORT_LENGTH, &request(TARGET_MOUSE, 2, PAGE_DEVICE, BATTERY, &[]));
        assert_eq!(sent, report("00 00 02 02 00 83"));
    }

    #[test]
    fn pairing_request_asks_the_receiver_with_its_one_argument() {
        let sent = pad(REPORT_LENGTH, &request(TARGET_PAIRING, 6, PAGE_DEVICE, PAIRED_MOUSE, &[0x02]));
        assert_eq!(sent, report("00 00 01 06 00 8B 02"));
    }

    #[test]
    fn akitsu_through_its_receiver_and_on_its_cable() {
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 00 64")), b(100, false));
    }

    #[test]
    fn glorious_model_o_2_pro_on_its_cable() {
        // Glorious CORE showed 100%, not charging
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 00 64")), b(100, false));
    }

    #[test]
    fn inca_charging_on_its_cable() {
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 01 64")), b(100, true));
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 01 2a")), b(42, true));
    }

    #[test]
    fn the_request_reading_back_is_no_answer_yet() {
        assert_eq!(battery_reply(&report("00 00 02 02 00 83")), Reply::Pending);
    }

    #[test]
    fn an_answer_to_another_command_is_not_the_battery() {
        // The Akitsu's firmware answer, which carries 07 02 where a level would be
        assert_eq!(battery_reply(&report("a1 00 02 06 00 81 00 00 07 02 00 66")), Reply::Pending);
        // The same command on another page
        assert_eq!(battery_reply(&report("a1 00 02 02 01 83 00 64")), Reply::Pending);
    }

    #[test]
    fn a_report_with_an_id_is_not_ours() {
        let mut r = report("a1 00 02 02 00 83 00 64");
        r[0] = 0x04;
        assert_eq!(battery_reply(&r), Reply::Pending);
    }

    #[test]
    fn a_receiver_without_its_mouse_says_asleep() {
        assert_eq!(battery_reply(&report("a4 00 02 02 00 83 00 00")), Reply::Asleep);
        assert_eq!(battery_reply(&report("02 00 02 02 00 83 00 00")), Reply::Asleep);
        assert_eq!(state(Reply::Asleep), off_because("d.asleep"));
    }

    #[test]
    fn busy_and_refused_are_no_answer() {
        assert_eq!(battery_reply(&report("a0 00 02 02 00 83 00 00")), Reply::Pending);
        assert_eq!(battery_reply(&report("a3 00 02 02 00 83 00 00")), Reply::Pending);
        // A2 comes with a level in some tools and means "refused" in others: not trusted
        assert_eq!(battery_reply(&report("a2 00 02 02 00 83 00 50")), Reply::Pending);
        assert_eq!(state(Reply::Pending), off());
    }

    #[test]
    fn empty_on_battery_is_the_receiver_answering_for_an_off_mouse() {
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 00 00")), Reply::Pending);
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 01 00")), b(0, true));
    }

    #[test]
    fn level_over_a_hundred_is_no_reading() {
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 00 65")), Reply::Pending);
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 00 ff")), Reply::Pending);
    }

    #[test]
    fn an_answer_without_its_two_bytes_is_no_reading() {
        assert_eq!(battery_reply(&report("a1 00 02 01 00 83 00 64")), Reply::Pending);
        assert_eq!(battery_reply(&report("a1 00 02 00 00 83 00 64")), Reply::Pending);
        assert_eq!(battery_reply(&[0x00, 0xA1, 0x00, 0x02, 0x02, 0x00]), Reply::Pending);
        assert_eq!(battery_reply(&[]), Reply::Pending);
    }

    #[test]
    fn full_on_the_cable_is_not_charging() {
        let full = Battery { level: 100, charging: false, full: true };
        assert_eq!(battery_reply(&report("a1 00 02 02 00 83 02 64")), Reply::Data(full));
        assert_eq!(state(Reply::Data(full)), State { full: true, ..on(100, false) });
    }

    #[test]
    fn a_reading_is_the_level_and_the_charging_flag() {
        assert_eq!(state(b(76, true)), on(76, true));
        assert_eq!(state(b(5, false)), on(5, false));
    }

    #[test]
    fn aurora_receiver_names_the_paro_it_holds() {
        let r = report("a1 00 01 06 00 8b 02 01 37 b0 00 07");
        assert_eq!(paired_reply(&r), Reply::Data((0x37B0, 0x0007)));
        assert_eq!(model(0x37B0, 0x0007), Some("LAMZU Paro"));
    }

    #[test]
    fn a_pairing_answer_cut_short_names_nobody() {
        assert_eq!(paired_reply(&report("a1 00 01 04 00 8b 02 01 37 b0 00 07")), Reply::Pending);
        assert_eq!(paired_reply(&report("a1 00 02 02 00 83 00 64")), Reply::Pending);
    }

    #[test]
    fn a_mouse_the_table_does_not_know_keeps_the_receivers_name() {
        // DM198, a model the LAMZU table lists under a code name only
        assert_eq!(model(0x37B0, 0x0034), None);
        // A receiver is not a model
        assert_eq!(model(0x37B0, 0x0001), None);
        assert_eq!(shared_receiver((0x37B0, 0x0001)), Some("LAMZU"));
        assert_eq!(shared_receiver((0x36A7, 0xA882)), Some("WLmouse"));
        assert_eq!(shared_receiver((0x37B0, 0x0010)), None);
    }

    #[test]
    fn only_receivers_a_tool_asks_are_asked_which_mouse_they_hold() {
        for id in NAMES_ITS_MOUSE {
            assert!(shared_receiver(*id).is_some(), "{:04X}:{:04X} is not a shared receiver", id.0, id.1);
        }
        // LAMZU's newer receivers and G-Wolves' are in no tool's list: they are named by the brand and not asked
        for id in [(LAMZU, 0x002E), (LAMZU, 0x0036), (LAMZU, 0x0048), (GWOLVES, 0x3817)] {
            assert!(shared_receiver(id).is_some() && !NAMES_ITS_MOUSE.contains(&id), "{:04X}:{:04X}", id.0, id.1);
        }
    }

    #[test]
    fn gwolves_receiver_of_four_models_is_not_named_after_one() {
        // mouse.xyz gives 3817 to the HTM Plus and as the shared receiver of the HSK Pro 2.0, HTXU and Fenrir Pro
        assert_eq!(shared_receiver((GWOLVES, 0x3817)), Some("G-Wolves"));
        assert_eq!(model(GWOLVES, 0x3817), None);
        assert_eq!(model(GWOLVES, 0x3808), Some("G-Wolves HTM Plus"));
        assert_ne!(spec(GWOLVES, 0x3817).key, spec(GWOLVES, 0x3808).key);
    }

    #[test]
    fn older_gwolves_request_says_how_the_mouse_is_connected() {
        assert_eq!(pad(REPORT_LENGTH, &legacy_request(false)), report("00 02 8F 01"));
        assert_eq!(pad(REPORT_LENGTH, &legacy_request(true)), report("00 02 8F 00"));
    }

    #[test]
    fn older_gwolves_answer() {
        assert_eq!(legacy_reply(&report("a1 02 8f 01 00 4c")), b(76, false));
        assert_eq!(legacy_reply(&report("a1 02 8f 00 01 64")), b(100, true));
    }

    #[test]
    fn older_gwolves_answer_is_matched_as_strictly() {
        // The request reading back, a sleeping mouse, another command, an empty level
        assert_eq!(legacy_reply(&report("00 02 8f 01")), Reply::Pending);
        assert_eq!(legacy_reply(&report("02 02 8f 01 00 4c")), Reply::Asleep);
        assert_eq!(legacy_reply(&report("a1 02 82 01 00 4c")), Reply::Pending);
        assert_eq!(legacy_reply(&report("a1 01 8f 01 00 4c")), Reply::Pending);
        assert_eq!(legacy_reply(&report("a1 02 8f 01 00 00")), Reply::Pending);
        assert_eq!(legacy_reply(&report("a1 02 8f 01 00 65")), Reply::Pending);
        // The newer answer is not the older one, and the other way round
        assert_eq!(legacy_reply(&report("a1 00 02 02 00 83 00 64")), Reply::Pending);
        assert_eq!(battery_reply(&report("a1 02 8f 01 00 4c")), Reply::Pending);
    }

    #[test]
    fn only_the_collection_with_the_feature_report_is_asked() {
        // The LAMZU Inca's collections: the configuration one and its two vendor neighbours
        assert!(takes_the_report(&collection(0xFFFF, 0x0000, 65)));
        assert!(!takes_the_report(&collection(0xFFFF, 0x0001, 0)));
        assert!(!takes_the_report(&collection(0xFFA0, 0x0001, 0)));
        assert!(!takes_the_report(&collection(0x0001, 0x0002, 0)));
        // The wired Glorious Model O V2 declares a 263-byte report, and these commands do nothing on it
        assert!(!takes_the_report(&collection(0xFF00, 0x0001, 263)));
    }

    #[test]
    fn sinowealth_ids_need_a_vendor_collection_and_are_marked_shared() {
        assert!(vendor_and_takes_the_report(&collection(0xFFFF, 0x0000, 65)));
        assert!(vendor_and_takes_the_report(&collection(0xFF01, 0x0001, 65)));
        assert!(!vendor_and_takes_the_report(&collection(0x0001, 0x0006, 65)));
        assert!(!vendor_and_takes_the_report(&collection(0xFF00, 0x0001, 6)));
        for s in SPECS.iter() {
            // The two vendor IDs that aren't one brand's own
            assert_eq!(s.shared, s.vid == GLORIOUS || s.vid == COMPX, "{}", s.key);
            assert!(s.accept.is_some(), "{}", s.key);
        }
    }

    #[test]
    fn the_exceptions_are_in_the_table() {
        for &(vid, pid) in ASKED_AS_ITSELF {
            assert!(is_cable(vid, pid), "{vid:04X}:{pid:04X} is not a cable ID");
        }
        for &((vid, pid), _) in SHARED_RECEIVERS {
            assert!(spec(vid, pid).cable_pids.is_empty() && spec(vid, pid).pids.len() == 1, "{vid:04X}:{pid:04X}");
        }
    }

    #[test]
    fn cable_and_receiver_of_one_mouse_share_a_key() {
        assert_eq!(spec(GLORIOUS, 0x2022).key, spec(GLORIOUS, 0x2011).key);
        assert!(is_cable(GLORIOUS, 0x2011) && !is_cable(GLORIOUS, 0x2022));
        // The Maya X has IDs under two vendors
        assert_eq!(spec(COMPX, 0x001E).key, spec(LAMZU, 0x003A).key);
        // 003A is a LAMZU Maya X under one vendor ID and an Attack Shark R8 under the other
        assert_eq!(spec(COMPX, 0x003A).name, "Attack Shark R8");
        assert_eq!(spec(LAMZU, 0x003A).name, "LAMZU Maya X");
    }

    #[test]
    fn keys_are_lowercase_with_hyphens_and_one_key_is_one_name() {
        for s in SPECS.iter() {
            assert!(s.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", s.key);
            assert!(SPECS.iter().all(|o| o.key != s.key || o.name == s.name), "{}", s.key);
        }
    }
}
