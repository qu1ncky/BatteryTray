//! Wireless keyboards on OEM platforms that `rongyuan.rs`, `aula_mchose.rs` and `sonix_keyboard.rs` don't cover, and
//! the mice that speak the first of these protocols.
//!
//! Evision ("weisheng") firmware: CHERRY MX 3.0S Wireless, PIXIU 75, MX 2.0S Wireless and MX 2.0S WL Pro; ATK V75X
//! and the VGN / VXE keyboards on the shared receiver 320F:5088 (cable 320F:5055); ATK Z87; AJAZZ AK992; the Akko V9
//! Dash Pro and Dash V9 Ultra mice; AULA mice on the "AULA-8K Dongle". Output report 04 on the vendor collection
//! FF1C:0092, answered by input report 04 on the same collection:
//!   request  04 TT TT 1A AA ..            TT a word the reply repeats, AA arguments that differ by vendor
//!   reply    04 TT TT 1A AA 00 00 xx LL PP   LL level in percent, PP 0 on battery, 1 charging, 2 charging or full
//!   asleep   04 TT TT FF ..               what the VGN and ATK Z87 receivers answer for a sleeping keyboard
//! CHERRY and Akko put the sum of the bytes from the command on into TT, ATK a request counter, VGN zeros.
//!
//! Semico tri-mode keyboards (Attack Shark X98, AJAZZ AK820 Max Plus) on the dongle 1A2C:8FFF or their cable:
//! 64-byte reports without an ID on FF02:0002, request AA 55 CC 33 0E, reply AA 55 CC 33 0E LL SS, SS 0 = nothing
//! behind the dongle.
//!
//! URX Core75 on the dongle 36B0:3002: the keyboard's VIA channel FF60:0061, VIA's "custom get" of value 7 on channel
//! 0: request 08 00 07, reply 08 00 07 LL.
//!
//! MCHOSE K87 V3 on the receiver 3837:1015: M HUB's "device info" read on FF80:0001, report 4D both ways:
//!   request  4D 01 01 00 00 09 00 00 XX       XX the xor of the bytes from the third on
//!   reply    4D 01 xx NN 00 09 xx xx DATA XX   NN bytes of data: vendor ID, model, .. link at 10, charging at 11,
//!                                              level at 12; a keyboard that is off gives one byte, FF
//!
//! Alienware Pro Wireless Gaming Keyboard on its receiver 413C:211D: a feature report without an ID, 0E A1, on
//! FF00:0001, answered by an input report 0E A1 LL SS, SS 04 with the cable in.
//!
//! Every request here only reads, but a request can count as activity. The owner of an ATK V75X saw HaloBattery's
//! periodic requests keep the keyboard from falling asleep (HaloBattery, pull request 308), so that app asks only
//! while the keyboard is typed on, and Wireless-Device-Battery-Monitor asks the 320F:5088 and ATK Z87 receivers every
//! 10 minutes at most and a MCHOSE K87 V3, which sleeps after half an hour, every 3. A Feker Alice80's backlight, on
//! the other hand, still timed out when asked every minute, and a CHERRY MX 2.0S falls asleep while asked twice a
//! minute. The receiver of a sleeping keyboard answers "asleep" or nothing by itself. An AULA mouse is woken by the
//! request. So the rows in `ROWS` are asked by a provider of their own, `KeyboardProvider`: every 5 minutes as
//! `aula_mchose.rs` does, the ones in `KEPT_AWAKE` every 35 minutes once they have answered, a device on shared IDs
//! that has never answered every 10, and the AULA mouse only while its dongle says the mouse is in use. The Akko
//! mice are in `SPECS` and asked with the other mice.
//!
//! Protocol facts from Wireless-Device-Battery-Monitor (src/drivers/atk_keyboard.cpp, cherry.cpp, akko.cpp,
//! ajazz_ak992.cpp, aula_mouse.cpp, semico.cpp, via_dongle.cpp, mchose_modern.cpp; issues 324 and 886, pull request
//! 2934), HaloBattery (providers/atk_keyboard.py, lofree.py, urx.py, alienware.py, docs/protocols.md, pull request
//! 308), BatteryWatch (src/Providers.cs: the CHERRY frame), cherry-battery-state (cherry_battery.py: the MX 2.0S),
//! myfinder-device-battery (engine/upstream-protocols.md: the VGN receiver, the MX 2.0S WL Pro) and openajazz
//! (PROTOCOL.md: the Semico status query). Own implementation. Not yet verified on hardware by this project, which is
//! why the first exchange with each device goes to the log.

use super::{off, off_because, on, Io, Spec, State, TableProvider};
use crate::hid::HidInfo;
use crate::loc::t;
use crate::providers::{secs, Provider};
use crate::reading::DeviceReading;
use crate::win;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

/// The mice, asked on every poll with the other mice. Nothing is published about what their receivers answer while
/// the mouse is off.
pub static SPECS: &[Spec] = &[
    Spec {
        key: "akko-v9-dash-pro",
        name: "Akko V9 Dash Pro",
        vid: 0x38EE,
        pids: &[0x000C],
        accept: Some(evision_64),
        query: |io| evision(io, &CHERRY),
        ..Spec::MOUSE
    },
    Spec {
        key: "akko-dash-v9-ultra",
        name: "Akko Dash V9 Ultra",
        vid: 0x38EE,
        pids: &[0x000F],
        accept: Some(evision_64),
        query: |io| evision(io, &CHERRY),
        ..Spec::MOUSE
    },
];

/// The devices `KeyboardProvider` asks.
static ROWS: &[Spec] = &[
    // ---- Evision ----
    // A second FF1C:0092 collection with 9-byte reports is not the one
    Spec {
        key: "cherry-mx-3-0s-wireless",
        name: "CHERRY MX 3.0S Wireless",
        vid: 0x046A,
        pids: &[0x00EA, 0x00E3],
        cable_pids: &[0x00E3],
        accept: Some(evision_64),
        query: |io| evision(io, &CHERRY),
        ..Spec::KEYBOARD
    },
    Spec {
        key: "cherry-pixiu-75",
        name: "CHERRY PIXIU 75",
        vid: 0x046A,
        pids: &[0x01E4],
        accept: Some(evision_64),
        query: |io| evision(io, &CHERRY),
        ..Spec::KEYBOARD
    },
    Spec {
        key: "cherry-mx-2-0s-wireless",
        name: "CHERRY MX 2.0S Wireless",
        vid: 0x046A,
        pids: &[0x01AC],
        accept: Some(evision_64),
        query: |io| evision(io, &CHERRY_MX2),
        ..Spec::KEYBOARD
    },
    Spec {
        key: "cherry-mx-2-0s-wl-pro",
        name: "CHERRY MX 2.0S WL Pro",
        vid: 0x046A,
        pids: &[0x01E0, 0x01DE],
        cable_pids: &[0x01DE],
        interface: 1,
        accept: Some(evision_64),
        query: |io| evision(io, &CHERRY_MX2),
        ..Spec::KEYBOARD
    },
    // Many Evision keyboards share these two IDs, and not all of them answer: only the product strings that ATK
    // HUB and VGN HUB drive this way are taken, and the keyboard is named after its string
    Spec {
        key: "evision-320f-5088",
        name: "name.keyboard",
        vid: 0x320F,
        pids: &[0x5088, 0x5055],
        cable_pids: &[0x5055],
        accept: Some(|i| evision_channel(i, 32) && board_5088(i.product.as_deref()).is_some()),
        shared: true,
        query: evision_5088,
        ..Spec::KEYBOARD
    },
    Spec {
        key: "atk-z87",
        name: "ATK Z87",
        vid: 0x373B,
        pids: &[0x1012],
        accept: Some(|i| evision_64(i) && product_is(i, "ATK Z87 Dongle")),
        shared: true,
        query: |io| evision(io, &ATK),
        ..Spec::KEYBOARD
    },
    // Evision's vendor ID, which its keyboards of any make carry: listed only with the collection this one has
    Spec {
        key: "ajazz-ak992",
        name: "AJAZZ AK992",
        vid: 0x320F,
        pids: &[0x50C3],
        interface: 1,
        accept: Some(|i| evision_64(i) && i.feature_length == 0),
        shared: true,
        query: |io| evision(io, &ATK),
        ..Spec::KEYBOARD
    },
    // A mouse, but one that a request wakes, so it is only asked while its dongle says it is in use. The X9 and V9
    // models share the dongle and nothing says which one is behind it
    Spec {
        key: "aula-8k-dongle",
        name: "AULA X9 Ultra / V9 Max+",
        vid: 0x320F,
        pids: &[0x22A0],
        interface: 1,
        accept: Some(|i| evision_64(i) && product_is(i, "AULA-8K Dongle")),
        shared: true,
        query: aula_8k,
        ..Spec::MOUSE
    },
    // ---- Semico ----
    // The dongle is Semico's generic one, sold with mice and with keyboards of other firmware families too
    Spec {
        key: "semico-1a2c",
        name: "name.keyboard",
        vid: 0x1A2C,
        pids: &[0x8FFF, 0xA034, 0xA036],
        cable_pids: &[0xA034, 0xA036],
        interface: 0,
        accept: Some(|i| i.usage_page == 0xFF02 && i.usage == 0x0002 && i.input_length == 65 && i.output_length == 65),
        shared: true,
        query: semico,
        ..Spec::KEYBOARD
    },
    // ---- VIA ----
    // The dongle of many unrelated keyboards (EPOMAKER, CIDOO, WOBKEY ...): only the one that names the Core75
    Spec {
        key: "urx-core75",
        name: "URX Core75",
        vid: 0x36B0,
        pids: &[0x3002],
        interface: 1,
        accept: Some(|i| {
            i.usage_page == 0xFF60
                && i.usage == 0x0061
                && i.input_length == 33
                && i.output_length == 33
                && i.product.as_deref().is_some_and(|p| p.trim().to_ascii_lowercase().starts_with("urx core75"))
        }),
        shared: true,
        query: via_battery,
        ..Spec::KEYBOARD
    },
    // ---- MCHOSE ----
    // The G98 V3 has the same receiver ID and is read another way; the model in the reply tells the K87 V3
    Spec {
        key: "mchose-k87-v3",
        name: "MCHOSE K87 V3",
        vid: 0x3837,
        pids: &[0x1015],
        accept: Some(|i| i.usage_page == 0xFF80 && i.usage == 0x0001 && i.input_length == 64 && i.output_length == 64),
        shared: true,
        query: mchose_k87_v3,
        ..Spec::KEYBOARD
    },
    // ---- Alienware ----
    Spec {
        key: "alienware-pro-wireless-keyboard",
        name: "Alienware Pro Wireless Gaming Keyboard",
        vid: 0x413C,
        pids: &[0x211D],
        page: 0xFF00,
        usage: 0x0001,
        query: alienware_pro,
        ..Spec::KEYBOARD
    },
];

static TABLES: &[&[Spec]] = &[ROWS];

/// Rows whose IDs unrelated devices share, with nothing but the answer to tell them apart: such a device is listed
/// once it has answered, so a keyboard of another make doesn't sit in the list as one that is always off.
const UNPROVEN: &[&str] = &["evision-320f-5088", "semico-1a2c", "mchose-k87-v3"];

/// Rows whose keyboard may take a request for activity and stay awake for it: an ATK V75X did, and the others here
/// have the same firmware maker or are asked rarely by their source for this reason.
const KEPT_AWAKE: &[&str] = &["evision-320f-5088", "atk-z87", "ajazz-ak992", "mchose-k87-v3"];

/// How long a `KEPT_AWAKE` keyboard that has answered is left alone: longer than the half hour after which such a
/// keyboard falls asleep, so that one request can put its sleep off once and no more.
const KEPT_AWAKE_WAIT: Duration = secs(35 * 60);

/// How long a device of an `UNPROVEN` row that has never answered is left alone, while nothing else is waited for:
/// it may be somebody else's keyboard (a WOBKEY Rainy 75 has the 320F:5088 receiver's IDs and name), and
/// Wireless-Device-Battery-Monitor asks that receiver every 10 minutes at most.
const STRANGER_WAIT: Duration = secs(10 * 60);

/// Asks the rows in `ROWS` the way the table engine asks the others, only less often.
pub struct KeyboardProvider {
    table: TableProvider,
    /// The readings of `UNPROVEN` rows that have had an answer in this session, by Id.
    answered: HashSet<String>,
    /// The last poll found a device of an `UNPROVEN` row that has never answered.
    strangers: bool,
}

impl Default for KeyboardProvider {
    fn default() -> Self {
        Self {
            table: TableProvider::new("Keyboards", TABLES).every(secs(300), secs(60)),
            answered: HashSet::new(),
            strangers: false,
        }
    }
}

impl Provider for KeyboardProvider {
    fn name(&self) -> &'static str {
        self.table.name()
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut readings = self.table.poll();
        let found = readings.len();
        readings.retain(|r| listed(&mut self.answered, r));
        self.strangers = readings.len() < found;
        readings
    }

    /// The level changes slowly, and a request may keep a keyboard from falling asleep. One that didn't answer is
    /// asked again sooner, to catch it waking up, as `sonix_keyboard.rs` does: its receiver answers for it.
    fn delay(&self, last: &[DeviceReading]) -> Duration {
        if last.iter().any(kept_awake) {
            KEPT_AWAKE_WAIT
        } else if last.is_empty() && self.strangers {
            STRANGER_WAIT
        } else {
            self.table.delay(last)
        }
    }
}

/// Whether a reading is of a `KEPT_AWAKE` keyboard that answered over its receiver: on its cable it is powered and
/// has no sleep to lose.
fn kept_awake(reading: &DeviceReading) -> bool {
    reading.online
        && reading.via != t("via.cable")
        && ROWS.iter().any(|s| KEPT_AWAKE.contains(&s.key) && s.id() == reading.id)
}

/// Whether a reading is shown: all of them but the `UNPROVEN` ones that have never answered.
fn listed(answered: &mut HashSet<String>, reading: &DeviceReading) -> bool {
    let unproven = ROWS.iter().any(|s| UNPROVEN.contains(&s.key) && s.id() == reading.id);
    if !unproven {
        return true;
    }
    if reading.online {
        answered.insert(reading.id.clone());
    }
    answered.contains(&reading.id)
}

/// The receiver is there and the keyboard behind it says nothing.
fn asleep() -> State {
    off_because("d.asleep")
}

fn product_is(info: &HidInfo, name: &str) -> bool {
    info.product.as_deref().is_some_and(|p| p.trim().eq_ignore_ascii_case(name))
}

// ---- Evision ----

const EVISION_REPORT: u8 = 0x04;
const EVISION_BATTERY: u8 = 0x1A;
const EVISION_ASLEEP: u8 = 0xFF;

/// The collection that takes the request, with reports of at least `length` bytes.
fn evision_channel(info: &HidInfo, length: usize) -> bool {
    info.usage_page == 0xFF1C && info.usage == 0x0092 && info.input_length >= length && info.output_length >= length
}

/// The same with reports of exactly 64 bytes, which is what all but the 320F:5088 receivers have.
fn evision_64(info: &HidInfo) -> bool {
    evision_channel(info, 64) && info.input_length == 64 && info.output_length == 64
}

/// What goes into bytes 1-2 of a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    /// The sum of the bytes from the command on, as Evision's own framing has it.
    Sum,
    Zero,
    /// A counter, so that the answer to an earlier request isn't taken for this one's.
    Counter,
}

/// How one vendor's software asks. `echo`: the reply repeats the tag and the first argument and keeps bytes 5-6 at
/// zero, and nothing else is the reply; without it only the report ID and the command are known to come back.
#[derive(Clone, Copy)]
pub struct Evision {
    tag: Tag,
    args: &'static [u8],
    echo: bool,
    wait_ms: u64,
    tries: usize,
}

/// CHERRY Utility's request, which the Akko mice answer too. The dongle is silent while the keyboard sleeps and can
/// miss the first request after it.
pub const CHERRY: Evision =
    Evision { tag: Tag::Sum, args: &[0x03, 0x00, 0x00, 0x55], echo: false, wait_ms: 400, tries: 2 };
pub const CHERRY_MX2: Evision = Evision { tag: Tag::Sum, args: &[0x06], echo: true, wait_ms: 1500, tries: 1 };
pub const ATK: Evision = Evision { tag: Tag::Counter, args: &[], echo: true, wait_ms: 1000, tries: 1 };
/// VGN HUB's request. A VGN receiver was seen to answer a second after the request.
pub const VGN: Evision = Evision { tag: Tag::Zero, args: &[0x06], echo: true, wait_ms: 2500, tries: 1 };

/// What a report says in answer to a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Battery { level: u8, power: u8 },
    Asleep,
}

static COUNTER: AtomicU16 = AtomicU16::new(0);

impl Evision {
    fn next_tag(&self) -> u16 {
        match self.tag {
            Tag::Sum => self.args.iter().fold(EVISION_BATTERY as u16, |sum, &b| sum.wrapping_add(b as u16)),
            Tag::Zero => 0,
            // 1 to FFFF: zeros are what an empty report has there
            Tag::Counter => COUNTER.fetch_add(1, Ordering::Relaxed) % 0xFFFF + 1,
        }
    }

    /// Output report 04: the tag, the command and its arguments. `Io::send` pads it with zeros, which the sum
    /// doesn't count.
    pub fn request(&self, tag: u16) -> Vec<u8> {
        let mut request = vec![EVISION_REPORT, tag as u8, (tag >> 8) as u8, EVISION_BATTERY];
        request.extend_from_slice(self.args);
        request
    }

    /// What `report` says in answer to the request tagged `tag`, or None when it isn't the answer.
    pub fn answer(&self, report: &[u8], tag: u16) -> Option<Answer> {
        if report.len() < 10 || report[0] != EVISION_REPORT {
            return None;
        }
        if !self.echo {
            return (report[3] == EVISION_BATTERY).then_some(Answer::Battery { level: report[8], power: report[9] });
        }
        if report[1..3] != tag.to_le_bytes() {
            return None;
        }
        if report[3] == EVISION_ASLEEP {
            return Some(Answer::Asleep);
        }
        let argument = self.args.first().is_none_or(|&a| report[4] == a);
        (report[3] == EVISION_BATTERY && argument && report[5] == 0 && report[6] == 0)
            .then_some(Answer::Battery { level: report[8], power: report[9] })
    }

    /// The state an answer stands for. A level of 0 is not a level: a WOBKEY Rainy 75, which shares the 320F:5088
    /// receiver's IDs and name, answers with zeros, and so does a request that comes back as it went out. With
    /// `echo` the power byte runs from 0 to 2 and anything else is another report; CHERRY's is "powered over USB".
    pub fn state(&self, answer: Answer) -> State {
        match answer {
            Answer::Asleep => asleep(),
            Answer::Battery { level, power } if (1..=100).contains(&level) && (!self.echo || power <= 2) => {
                on(level as i32, power != 0)
            }
            Answer::Battery { .. } => off(),
        }
    }
}

fn evision(io: &mut Io, how: &Evision) -> win::Result<State> {
    io.set_timeout(how.wait_ms);
    for _ in 0..how.tries {
        let tag = how.next_tag();
        let reply = io.ask(&how.request(tag), |d| how.answer(d, tag).is_some(), 32, false)?;
        if let Some(answer) = reply.and_then(|r| how.answer(&r, tag)) {
            return Ok(how.state(answer));
        }
    }
    Ok(asleep())
}

/// The product strings on 320F:5088 (receiver) and 320F:5055 (cable) that are asked, the name each stands for and
/// whether it is asked VGN HUB's way; the list is Wireless-Device-Battery-Monitor's, from VGN HUB's device table and
/// ATK HUB. "Wireless Gaming Keyboard" is what the receiver of a Feker Alice80 and of other keyboards calls itself,
/// so it names nobody.
const BOARDS_5088: &[(&str, Option<&str>, bool)] = &[
    ("ATK V75X 2.4G Dongle", Some("ATK V75X"), false),
    ("ATK V75X", Some("ATK V75X"), false),
    ("VGN S99 2.4G Dongle", Some("VGN S99"), true),
    ("VGN S99", Some("VGN S99"), true),
    ("VGN V98pro V2 2.4G Dongle", Some("VGN V98 Pro V2"), true),
    ("VGN V98pro V2", Some("VGN V98 Pro V2"), true),
    ("VGN V98 PRO 2.4G Dongle", Some("VGN V98 Pro"), true),
    ("VGN V98pro", Some("VGN V98 Pro"), true),
    ("VGN N75 PRO 2.4G Dongle", Some("VGN N75 Pro"), true),
    ("VGN N75 PRO", Some("VGN N75 Pro"), true),
    ("VGN VXE75 V2 2.4G Dongle", Some("VGN VXE75 V2"), true),
    ("VGN VXE75 V2", Some("VGN VXE75 V2"), true),
    ("VGN VXE75 2.4G Dongle", Some("VGN VXE75"), true),
    ("VGN VXE75", Some("VGN VXE75"), true),
    ("VXE V87 2.4G Dongle", Some("VXE V87"), true),
    ("VXE V87", Some("VXE V87"), true),
    ("VXE V87 RGB", Some("VXE V87"), true),
    ("Wireless Gaming Keyboard", None, false),
];

/// The name and the way of asking for a product string on 320F:5088 / 5055, or None for a keyboard that isn't asked.
pub fn board_5088(product: Option<&str>) -> Option<(Option<&'static str>, &'static Evision)> {
    let product = product?.trim();
    let (_, name, vgn) = BOARDS_5088.iter().find(|b| b.0.eq_ignore_ascii_case(product))?;
    Some((*name, if *vgn { &VGN } else { &ATK }))
}

fn evision_5088(io: &mut Io) -> win::Result<State> {
    let Some((name, how)) = board_5088(io.info().product.as_deref()) else { return Ok(off()) };
    let state = evision(io, how)?;
    Ok(State { name: name.map(str::to_string), ..state })
}

/// How long an AULA dongle is listened to for a word about the link, which comes about twice a second.
const AULA_LINK_MS: u64 = 800;
/// How long its answer is waited for after that.
const AULA_ANSWER_MS: u64 = 500;

/// The AULA dongle answers from what it remembers, whether the mouse is on or not, so the level alone proves
/// nothing, and the request wakes a mouse that sleeps. While the mouse is in use the dongle also sends, about twice
/// a second, 04 AA 00 AA 00 00 00 00 SS with SS FF for a link that is up and 00 for one that is down. So the dongle
/// is listened to first, and asked only once it has said "up": a mouse lying idle sends nothing, is sent nothing
/// and comes out as off until it moves. The vendor's software wraps the request in a "start" and an "end" (commands
/// 01 and 02); the mouse answers without them, and they aren't sent.
fn aula_8k(io: &mut Io) -> win::Result<State> {
    io.set_timeout(AULA_LINK_MS);
    let link = io.receive(|d| aula_link(d).is_some(), 64)?.and_then(|r| aula_link(&r));
    if link != Some(true) {
        return Ok(off());
    }
    io.set_timeout(AULA_ANSWER_MS);
    let reply = io.ask(&[EVISION_REPORT, 0x00, 0x00, EVISION_BATTERY], |d| aula_battery(d).is_some(), 32, false)?;
    Ok(aula_state(reply.and_then(|r| aula_battery(&r)), link))
}

/// The dongle's word about the link: Some(true) for up, Some(false) for down, None for any other report.
pub fn aula_link(report: &[u8]) -> Option<bool> {
    let link = report.len() >= 9 && report[..4] == [EVISION_REPORT, 0xAA, 0x00, 0xAA];
    match report.get(8) {
        Some(0xFF) if link => Some(true),
        Some(0x00) if link => Some(false),
        _ => None,
    }
}

/// The level and the power byte of the dongle's answer: 04 00 00 1A 00 00 00 00 LL PP.
pub fn aula_battery(report: &[u8]) -> Option<(u8, u8)> {
    (report.len() >= 10 && report[..8] == [EVISION_REPORT, 0x00, 0x00, EVISION_BATTERY, 0x00, 0x00, 0x00, 0x00])
        .then(|| (report[8], report[9]))
}

/// A level of 0 is a dongle that hasn't heard from its mouse yet. Power: 1 charging, 2 charged.
pub fn aula_state(battery: Option<(u8, u8)>, link: Option<bool>) -> State {
    match (battery, link) {
        (Some((level @ 1..=100, power @ 0..=2)), Some(true)) => {
            State { full: power == 2, ..on(level as i32, power == 1) }
        }
        _ => off(),
    }
}

// ---- Semico ----

const SEMICO_STATUS: [u8; 5] = [0xAA, 0x55, 0xCC, 0x33, 0x0E];

/// The vendor's tool asks the same way every 5 seconds. The neighbours of command 0E write (07 lighting, 08 factory
/// reset, 0C sleep timer), so nothing but this one frame is ever sent.
fn semico(io: &mut Io) -> win::Result<State> {
    io.set_timeout(600);
    let mut request = vec![0x00];
    request.extend_from_slice(&SEMICO_STATUS);
    let reply = io.ask(&request, |d| d.len() >= 7 && d[..5] == SEMICO_STATUS, 16, false)?;
    let name = match io.product_id() {
        0xA034 => Some("Attack Shark X98"),
        0xA036 => Some("AJAZZ AK820 Max Plus"),
        _ => None, // the dongle is the same for both and calls itself "2.4 Wireless Device"
    };
    Ok(State { name: name.map(str::to_string), ..reply.map_or_else(asleep, |r| semico_state(&r)) })
}

/// AA 55 CC 33 0E LL SS: SS 0 with nothing behind the dongle, 1 to 3 with a keyboard there. openajazz reads 1 as
/// charging and 2 as on battery, but an Attack Shark X98 said 1 both on its charger and off it, so there is no
/// charging sign.
pub fn semico_state(reply: &[u8]) -> State {
    if reply.len() < 7 || reply[..5] != SEMICO_STATUS {
        return off();
    }
    match (reply[5], reply[6]) {
        (_, 0) => asleep(),
        (level @ 1..=100, 1..=3) => on(level as i32, false),
        _ => off(),
    }
}

// ---- VIA ----

/// VIA's "custom get value" (08) of value 7 on channel 0, which is where URX HUB reads the level. No charging
/// state: the reply was the same with the cable in. The first request can be lost while the link wakes up.
fn via_battery(io: &mut Io) -> win::Result<State> {
    io.set_timeout(600);
    for _ in 0..2 {
        if let Some(reply) = io.ask(&[0x00, 0x08, 0x00, 0x07], |d| via_level(d).is_some(), 8, false)? {
            return Ok(via_level(&reply).flatten().map_or_else(off, |level| on(level, false)));
        }
    }
    Ok(asleep())
}

/// The level in 08 00 07 LL: Some(None) when the keyboard answered without one, None for any other report (FF .. is
/// a firmware that doesn't know the value).
pub fn via_level(report: &[u8]) -> Option<Option<i32>> {
    (report.len() >= 4 && report[..3] == [0x08, 0x00, 0x07])
        .then(|| (1..=100).contains(&report[3]).then_some(report[3] as i32))
}

// ---- MCHOSE ----

const MCHOSE_REPORT: u8 = 0x4D;
/// 0x3837 and the K87 V3's model, 0x2023, as the reply has them.
const MCHOSE_K87_V3: [u8; 4] = [0x37, 0x38, 0x23, 0x20];

/// What the receiver says about its keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MchoseInfo {
    Battery { level: u8, charging: bool },
    NotLinked,
}

/// Wireless-Device-Battery-Monitor asks at most every 3 minutes, "to avoid repeatedly waking an idle keyboard".
fn mchose_k87_v3(io: &mut Io) -> win::Result<State> {
    io.set_timeout(300);
    for _ in 0..3 {
        let reply = io.ask(&mchose_request(), |d| mchose_info(d).is_some(), 16, false)?;
        match reply.and_then(|r| mchose_info(&r)) {
            Some(MchoseInfo::Battery { level, charging }) if level >= 1 => return Ok(on(level as i32, charging)),
            Some(_) => return Ok(asleep()),
            None => {}
        }
    }
    Ok(asleep())
}

/// "Read device info": version 01, flags 01 (read), no data, command 0900, and the xor of bytes 2-7.
pub fn mchose_request() -> Vec<u8> {
    let mut request = vec![MCHOSE_REPORT, 0x01, 0x01, 0x00, 0x00, 0x09, 0x00, 0x00];
    request.push(request[2..].iter().fold(0, |x, &b| x ^ b));
    request
}

/// What a "device info" reply says, or None when the report isn't one from a K87 V3: a checksum that doesn't add
/// up, another vendor ID or another model (the G98 V3 has the same receiver ID).
pub fn mchose_info(report: &[u8]) -> Option<MchoseInfo> {
    if report.len() < 10 || report[0] != MCHOSE_REPORT || report[4..6] != [0x00, 0x09] {
        return None;
    }
    let length = report[3] as usize;
    let data = report.get(8..8 + length)?;
    let checksum = *report.get(8 + length)?;
    // With the keyboard off the receiver sends one byte, FF, and no checksum worth the name
    if data == [0xFF] {
        return Some(MchoseInfo::NotLinked);
    }
    if length < 20 || data[..4] != MCHOSE_K87_V3 || checksum != report[2..8 + length].iter().fold(0, |x, &b| x ^ b) {
        return None;
    }
    // 0 not linked, 1 on its cable, 2 on the receiver
    if data[10] == 0 {
        return Some(MchoseInfo::NotLinked);
    }
    (data[12] <= 100).then_some(MchoseInfo::Battery { level: data[12], charging: data[11] == 1 })
}

// ---- Alienware ----

/// Command Center sends 0E A1 as a feature report without an ID (as report 0E the receiver ignores it) and gets the
/// answer as an input report. The receiver sends other 0E reports by itself (0E 05, 0E 17, 0E 83 ...).
fn alienware_pro(io: &mut Io) -> win::Result<State> {
    let mut request = vec![0u8; 65];
    request[1..3].copy_from_slice(&[0x0E, 0xA1]);
    if !io.set_feature(&request) {
        return Ok(off());
    }
    io.set_timeout(1500);
    let reply = io.receive(|d| d.len() >= 4 && d[..2] == [0x0E, 0xA1], 16)?;
    Ok(reply.map_or_else(asleep, |r| alienware_state(&r)))
}

/// 0E A1 LL SS: SS 04 with the cable in and 00 without, the only two values its owner's captures have.
pub fn alienware_state(reply: &[u8]) -> State {
    match reply {
        [0x0E, 0xA1, level @ 1..=100, power, ..] => on(*level as i32, *power == 0x04),
        _ => off(),
    }
}

#[cfg(test)]
mod tests {
    //! Captured frames, as the sources publish them: the ATK V75X's from HaloBattery (docs/protocols.md), the VGN
    //! receiver's from myfinder-device-battery (engine/upstream-protocols.md), the WOBKEY Rainy 75's from
    //! Wireless-Device-Battery-Monitor (issue 324), the AULA V9 Max+'s from HaloBattery (providers/lofree.py), the
    //! Semico ones from openajazz (PROTOCOL.md), the URX Core75's and the Alienware keyboard's from HaloBattery. The
    //! CHERRY replies are made up from the layout the sources give, the MX 2.0S ones with the levels its owner read.
    //! Input reports are as `Io::data` hands them over: with their report ID, or without the 00 Windows puts in front
    //! when there is none.
    use super::*;
    use crate::providers::pad;

    fn h(text: &str) -> Vec<u8> {
        pad(64, &text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect::<Vec<_>>())
    }

    fn state(online: bool, level: Option<i32>, charging: bool) -> State {
        State::new(online, level, charging)
    }

    fn read(how: &Evision, reply: &str, tag: u16) -> Option<State> {
        how.answer(&h(reply), tag).map(|a| how.state(a))
    }

    fn row(key: &str) -> &'static Spec {
        ROWS.iter().chain(SPECS).find(|s| s.key == key).unwrap()
    }

    fn collection(page: u16, usage: u16, length: usize, product: &str) -> HidInfo {
        HidInfo {
            usage_page: page,
            usage,
            input_length: length,
            output_length: length,
            product: Some(product.to_string()),
            ..HidInfo::default()
        }
    }

    fn accepts(key: &str, info: &HidInfo) -> bool {
        (row(key).accept.unwrap())(info)
    }

    #[test]
    fn no_product_id_is_in_two_tables() {
        let mut seen = HashSet::new();
        for spec in
            ROWS.iter().chain(super::super::HEADSETS.iter().chain(super::super::PERIPHERALS).flat_map(|t| t.iter()))
        {
            for pid in spec.pids {
                assert!(seen.insert((spec.vid, *pid)), "{:04X}:{pid:04X} is listed twice ({})", spec.vid, spec.key);
            }
        }
    }

    #[test]
    fn rows_have_a_key_and_a_name_and_their_cable_ids() {
        for spec in ROWS {
            assert!(!spec.key.is_empty() && !spec.name.is_empty());
            assert_eq!(ROWS.iter().filter(|o| o.key == spec.key).count(), 1, "{}", spec.key);
            assert!(spec.cable_pids.iter().all(|p| spec.pids.contains(p)), "{}", spec.key);
        }
        assert!(UNPROVEN.iter().all(|k| ROWS.iter().any(|s| s.key == *k && s.shared)));
        assert!(KEPT_AWAKE.iter().all(|k| ROWS.iter().any(|s| s.key == *k)));
    }

    #[test]
    fn evisions_own_vendor_id_is_never_enough_to_list_a_device() {
        assert!(ROWS.iter().filter(|s| s.vid == 0x320F).all(|s| s.shared && s.accept.is_some()));
    }

    #[test]
    fn cherry_request_carries_the_sum_of_its_bytes() {
        assert_eq!(CHERRY.request(CHERRY.next_tag()), [0x04, 0x72, 0x00, 0x1A, 0x03, 0x00, 0x00, 0x55]);
        // cherry-battery-state's request for the MX 2.0S
        assert_eq!(CHERRY_MX2.request(CHERRY_MX2.next_tag()), [0x04, 0x20, 0x00, 0x1A, 0x06]);
    }

    #[test]
    fn vgn_request_is_vgn_hubs() {
        assert_eq!(VGN.request(VGN.next_tag()), [0x04, 0x00, 0x00, 0x1A, 0x06]);
    }

    #[test]
    fn atk_request_carries_a_counter_that_is_never_zero() {
        let (first, second) = (ATK.next_tag(), ATK.next_tag());
        assert!(first != 0 && second != 0 && first != second);
        assert_eq!(ATK.request(0x7B01), [0x04, 0x01, 0x7B, 0x1A]);
    }

    #[test]
    fn atk_v75x_on_its_receiver() {
        assert_eq!(read(&ATK, "04 01 6a 1a 00 00 00 00 3b 00", 0x6A01), Some(state(true, Some(59), false)));
        assert_eq!(read(&ATK, "04 01 7b 1a 00 00 00 00 3c 01", 0x7B01), Some(state(true, Some(60), true)));
        assert_eq!(read(&ATK, "04 01 7c 1a 00 00 00 00 3e 00", 0x7C01), Some(state(true, Some(62), false)));
    }

    #[test]
    fn atk_v75x_on_its_cable() {
        assert_eq!(read(&ATK, "04 03 01 1a 00 00 00 00 5b 01", 0x0103), Some(state(true, Some(91), true)));
    }

    #[test]
    fn the_answer_to_another_request_is_not_the_reply() {
        assert_eq!(read(&ATK, "04 01 7b 1a 00 00 00 00 3c 01", 0x7B02), None);
    }

    #[test]
    fn other_reports_are_not_the_reply() {
        assert_eq!(read(&ATK, "04 01 7b 05 00 00 00 00 3c 01", 0x7B01), None); // another command
        assert_eq!(read(&ATK, "04 01 7b 1a 00 08 00 00 3c 01", 0x7B01), None); // an offset
        assert_eq!(read(&ATK, "05 01 7b 1a 00 00 00 00 3c 01", 0x7B01), None); // another report
        assert_eq!(ATK.answer(&[0x04, 0x01, 0x7B, 0x1A, 0x00, 0x00, 0x00, 0x00, 0x3C], 0x7B01), None);
        // too short
    }

    #[test]
    fn vgn_receiver_with_its_keyboard_awake() {
        assert_eq!(read(&VGN, "04 00 00 1a 06 00 00 00 5d", 0), Some(state(true, Some(93), false)));
    }

    #[test]
    fn vgn_receiver_with_its_keyboard_asleep() {
        assert_eq!(read(&VGN, "04 00 00 ff 06", 0), Some(asleep()));
    }

    #[test]
    fn vgn_request_coming_back_is_no_reading() {
        assert_eq!(read(&VGN, "04 00 00 1a 06", 0), Some(off()));
        assert_eq!(read(&VGN, "04 00 00 1a 00 00 00 00 5d", 0), None); // without the argument it isn't VGN's reply
    }

    #[test]
    fn rainy_75_answers_zeros_which_is_no_reading() {
        assert_eq!(read(&ATK, "04 01 6a 1a 00 00 00 00 00 00", 0x6A01), Some(off()));
    }

    #[test]
    fn level_over_a_hundred_or_an_unknown_power_byte_is_no_reading() {
        assert_eq!(read(&ATK, "04 01 7b 1a 00 00 00 00 65 00", 0x7B01), Some(off()));
        assert_eq!(read(&ATK, "04 01 7b 1a 00 00 00 00 3c 03", 0x7B01), Some(off()));
        assert_eq!(read(&ATK, "04 01 7b 1a 00 00 00 00 3c 02", 0x7B01), Some(state(true, Some(60), true)));
    }

    #[test]
    fn cherry_reply_is_known_by_its_command_alone() {
        // The reply's bytes 1-2 are not known to repeat the request's
        assert_eq!(read(&CHERRY, "04 9b 00 1a 03 00 00 00 57 00", 0x0072), Some(state(true, Some(87), false)));
        assert_eq!(read(&CHERRY, "04 9b 00 1a 03 00 00 00 57 01", 0x0072), Some(state(true, Some(87), true)));
        assert_eq!(read(&CHERRY, "04 9b 00 05 03 00 00 00 57 01", 0x0072), None);
        assert_eq!(read(&CHERRY, "04 9b 00 1a 03 00 00 00 00 00", 0x0072), Some(off()));
        assert_eq!(read(&CHERRY, "04 9b 00 1a 03 00 00 00 c8 00", 0x0072), Some(off()));
    }

    #[test]
    fn cherry_mx_2_0s_reply_repeats_the_request() {
        assert_eq!(read(&CHERRY_MX2, "04 20 00 1a 06 00 00 00 37 00", 0x0020), Some(state(true, Some(55), false)));
        assert_eq!(read(&CHERRY_MX2, "04 20 00 1a 06 00 00 00 3a 01", 0x0020), Some(state(true, Some(58), true)));
        assert_eq!(read(&CHERRY_MX2, "04 20 00 1a 06", 0x0020), Some(off())); // the request itself
        assert_eq!(read(&CHERRY_MX2, "04 21 00 1a 06 00 00 00 37 00", 0x0020), None);
    }

    #[test]
    fn keyboards_on_320f_5088_are_known_by_their_product_string() {
        assert_eq!(
            board_5088(Some("ATK V75X 2.4G Dongle")).map(|b| (b.0, b.1.tag)),
            Some((Some("ATK V75X"), Tag::Counter))
        );
        assert_eq!(
            board_5088(Some("VGN V98pro V2 ")).map(|b| (b.0, b.1.tag)),
            Some((Some("VGN V98 Pro V2"), Tag::Zero))
        );
        assert_eq!(board_5088(Some("vxe v87 rgb")).map(|b| b.0), Some(Some("VXE V87")));
        assert_eq!(board_5088(Some("Wireless Gaming Keyboard")).map(|b| (b.0, b.1.tag)), Some((None, Tag::Counter)));
        assert!(board_5088(Some("Rainy 75")).is_none());
        assert!(board_5088(Some("VGN S99 Pro 2.4G Dongle")).is_none());
        assert!(board_5088(None).is_none());
    }

    #[test]
    fn receiver_320f_5088_is_taken_by_name_and_report_length() {
        let key = "evision-320f-5088";
        assert!(accepts(key, &collection(0xFF1C, 0x0092, 32, "ATK V75X 2.4G Dongle")));
        assert!(accepts(key, &collection(0xFF1C, 0x0092, 64, "VGN S99 2.4G Dongle")));
        assert!(!accepts(key, &collection(0xFF1C, 0x0092, 9, "VGN S99 2.4G Dongle")));
        assert!(!accepts(key, &collection(0xFF1C, 0x0092, 32, "Some Other Keyboard")));
        assert!(!accepts(key, &collection(0x0001, 0x0006, 64, "ATK V75X")));
    }

    #[test]
    fn cherry_has_a_second_collection_with_short_reports_that_is_not_taken() {
        assert!(accepts("cherry-mx-3-0s-wireless", &collection(0xFF1C, 0x0092, 64, "MX 3.0S Dongle")));
        assert!(!accepts("cherry-mx-3-0s-wireless", &collection(0xFF1C, 0x0092, 9, "MX 3.0S Dongle")));
    }

    #[test]
    fn dongles_that_share_their_ids_are_taken_by_name() {
        assert!(accepts("atk-z87", &collection(0xFF1C, 0x0092, 64, "ATK Z87 Dongle")));
        assert!(!accepts("atk-z87", &collection(0xFF1C, 0x0092, 64, "ATK RS7 Dongle")));
        assert!(accepts("aula-8k-dongle", &collection(0xFF1C, 0x0092, 64, "AULA-8K Dongle")));
        assert!(!accepts("aula-8k-dongle", &collection(0xFF1C, 0x0092, 64, "2.4G Wireless Receiver")));
        assert!(accepts("urx-core75", &collection(0xFF60, 0x0061, 33, "URX Core75 Wireless")));
        assert!(!accepts("urx-core75", &collection(0xFF60, 0x0061, 33, "Wireless 2.4G Dongle")));
        assert!(!accepts("urx-core75", &collection(0xFF60, 0x0061, 65, "URX Core75 Wireless")));
    }

    #[test]
    fn ak992_collection_has_no_feature_report() {
        let mut info = collection(0xFF1C, 0x0092, 64, "AJAZZ AK992");
        assert!(accepts("ajazz-ak992", &info));
        info.feature_length = 64;
        assert!(!accepts("ajazz-ak992", &info));
    }

    #[test]
    fn aula_dongle_says_whether_the_link_is_up() {
        assert_eq!(aula_link(&h("04 aa 00 aa 00 00 00 00 ff")), Some(true));
        assert_eq!(aula_link(&h("04 aa 00 aa 00 00 00 00 00")), Some(false));
        assert_eq!(aula_link(&h("04 aa 00 aa 00 00 00 00 01")), None);
        assert_eq!(aula_link(&h("04 00 00 1a 00 00 00 00 59 00")), None);
    }

    #[test]
    fn aula_v9_max_plus_with_the_link_up() {
        let linked = |reply: &str| aula_state(aula_battery(&h(reply)), Some(true));
        assert_eq!(linked("04 00 00 1a 00 00 00 00 59 00"), state(true, Some(89), false));
        assert_eq!(linked("04 00 00 1a 00 00 00 00 63 01"), state(true, Some(99), true));
        assert_eq!(linked("04 00 00 1a 00 00 00 00 64 02"), State { full: true, ..state(true, Some(100), false) });
    }

    #[test]
    fn aula_level_without_a_link_that_is_up_is_the_dongles_memory() {
        let battery = aula_battery(&h("04 00 00 1a 00 00 00 00 59 00"));
        assert_eq!(battery, Some((0x59, 0)));
        assert_eq!(aula_state(battery, None), off());
        assert_eq!(aula_state(battery, Some(false)), off());
        assert_eq!(aula_state(None, Some(true)), off());
    }

    #[test]
    fn aula_dongle_that_has_not_heard_from_its_mouse_answers_zero() {
        assert_eq!(aula_state(aula_battery(&h("04 00 00 1a 00 00 00 00 00 00")), Some(true)), off());
        assert_eq!(aula_state(aula_battery(&h("04 00 00 1a 00 00 00 00 65 00")), Some(true)), off());
        assert_eq!(aula_state(aula_battery(&h("04 00 00 1a 00 00 00 00 59 03")), Some(true)), off());
    }

    #[test]
    fn aula_acknowledgements_and_link_reports_are_not_the_level() {
        assert_eq!(aula_battery(&h("04 00 00 01 00 00 00 00 59 00")), None);
        assert_eq!(aula_battery(&h("04 aa 00 aa 00 00 00 00 ff")), None);
        assert_eq!(aula_battery(&h("04 00 00 1a 00 08 00 00 59 00")), None);
    }

    #[test]
    fn semico_keyboard_on_its_cable_and_behind_its_dongle() {
        // openajazz: "64 03" on the cable, "61 01" charging over 2.4G; the X98's owner: 96% on battery
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 64 03")), state(true, Some(100), false));
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 61 01")), state(true, Some(97), false));
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 60 01")), state(true, Some(96), false));
    }

    #[test]
    fn semico_dongle_with_nothing_behind_it() {
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 50 00")), asleep());
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 00 00")), asleep());
    }

    #[test]
    fn semico_reply_out_of_range_is_no_reading() {
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 65 01")), off());
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 50 04")), off());
        assert_eq!(semico_state(&h("aa 55 cc 33 0e 00 01")), off());
        assert_eq!(semico_state(&h("aa 55 cc 33 07 50 01")), off()); // the answer to another command
        assert_eq!(semico_state(&[0xAA, 0x55, 0xCC, 0x33, 0x0E, 0x50]), off());
    }

    #[test]
    fn semico_collection_is_the_one_on_interface_0_with_64_byte_reports() {
        let spec = row("semico-1a2c");
        assert_eq!(spec.interface, 0);
        assert!(accepts(spec.key, &collection(0xFF02, 0x0002, 65, "2.4 Wireless Device")));
        assert!(!accepts(spec.key, &collection(0xFF02, 0x0002, 17, "2.4 Wireless Device")));
        assert!(!accepts(spec.key, &collection(0xFF60, 0x0061, 65, "2.4 Wireless Device")));
    }

    #[test]
    fn urx_core75_level() {
        assert_eq!(via_level(&h("08 00 07 4c")), Some(Some(76)));
    }

    #[test]
    fn via_firmware_without_the_value_is_not_the_reply() {
        assert_eq!(via_level(&h("ff 07")), None);
        assert_eq!(via_level(&h("ff 00 07 4c")), None);
        assert_eq!(via_level(&h("08 00 06 4c")), None);
        assert_eq!(via_level(&[0x08, 0x00, 0x07]), None);
    }

    #[test]
    fn via_level_out_of_range_is_no_level() {
        assert_eq!(via_level(&h("08 00 07 00")), Some(None));
        assert_eq!(via_level(&h("08 00 07 65")), Some(None));
        assert_eq!(via_level(&h("08 00 07 ff")), Some(None));
    }

    /// A "device info" reply with this data, its length and its checksum. The flags byte is FF in the captured
    /// reply, not the request's 01.
    fn mchose_reply(data: &[u8]) -> Vec<u8> {
        let mut r = vec![0x4D, 0x01, 0xFF, data.len() as u8, 0x00, 0x09, 0x00, 0x00];
        r.extend_from_slice(data);
        r.push(r[2..].iter().fold(0, |x, &b| x ^ b));
        pad(64, &r)
    }

    /// The K87 V3's 20 bytes as its owner captured them (Wireless-Device-Battery-Monitor, pull request 2934), with
    /// the link, the charging flag and the level set: vendor ID, model, six bytes that aren't read, then the three.
    fn k87_data(link: u8, charging: u8, level: u8) -> Vec<u8> {
        let mut data = vec![0x37, 0x38, 0x23, 0x20, 0x03, 0x02, 0x00, 0x00, 0x00, 0x10, link, charging, level];
        data.resize(20, 0);
        data
    }

    #[test]
    fn mchose_request_is_m_hubs() {
        assert_eq!(mchose_request(), [0x4D, 0x01, 0x01, 0x00, 0x00, 0x09, 0x00, 0x00, 0x08]);
    }

    #[test]
    fn mchose_k87_v3_behind_its_receiver() {
        let info = |link, charging, level| mchose_info(&mchose_reply(&k87_data(link, charging, level)));
        // The captured reply: linked to the receiver, not charging, 99%
        assert_eq!(info(2, 0, 0x63), Some(MchoseInfo::Battery { level: 99, charging: false }));
        assert_eq!(info(2, 1, 25), Some(MchoseInfo::Battery { level: 25, charging: true }));
        assert_eq!(info(0, 0, 73), Some(MchoseInfo::NotLinked));
        assert_eq!(info(2, 0, 101), None);
    }

    #[test]
    fn mchose_receiver_with_its_keyboard_off() {
        // Captured by Wireless-Device-Battery-Monitor on a receiver of this family
        assert_eq!(mchose_info(&h("4d 01 01 01 00 09 00 00 ff 00")), Some(MchoseInfo::NotLinked));
    }

    #[test]
    fn mchose_reply_of_another_model_or_with_a_wrong_checksum_is_not_read() {
        let mut other = k87_data(2, 0, 73);
        other[2] = 0x28;
        assert_eq!(mchose_info(&mchose_reply(&other)), None);
        let mut vendor = k87_data(2, 0, 73);
        vendor[0] = 0x53;
        assert_eq!(mchose_info(&mchose_reply(&vendor)), None);
        let mut broken = mchose_reply(&k87_data(2, 0, 73));
        broken[20] ^= 0x01;
        assert_eq!(mchose_info(&broken), None);
        // A mouse's 14 bytes
        assert_eq!(mchose_info(&mchose_reply(&k87_data(2, 0, 73)[..14])), None);
    }

    #[test]
    fn mchose_other_reports_are_not_the_reply() {
        assert_eq!(mchose_info(&h("4d 01 01 00 04 09 00 00 0c")), None); // another command
        assert_eq!(mchose_info(&mchose_request()), None); // the request, unpadded
        assert_eq!(mchose_info(&h("4d 01 01 00 00 09 00 00 08")), None); // the request coming back
        let mut cut = mchose_reply(&k87_data(2, 0, 73));
        cut.truncate(20);
        assert_eq!(mchose_info(&cut), None);
    }

    #[test]
    fn alienware_keyboard_on_battery_and_on_its_cable() {
        assert_eq!(alienware_state(&h("0e a1 25 00")), state(true, Some(37), false));
        assert_eq!(alienware_state(&h("0e a1 5c 00")), state(true, Some(92), false));
        assert_eq!(alienware_state(&h("0e a1 5c 04")), state(true, Some(92), true));
    }

    #[test]
    fn alienware_other_reports_and_levels_out_of_range_are_no_reading() {
        assert_eq!(alienware_state(&h("0e 05 25 00")), off());
        assert_eq!(alienware_state(&h("0e a1 00 00")), off());
        assert_eq!(alienware_state(&h("0e a1 65 00")), off());
        assert_eq!(alienware_state(&[0x0E, 0xA1, 0x25]), off());
    }

    fn reading(key: &str, online: bool) -> DeviceReading {
        DeviceReading { online, ..DeviceReading::new(row(key).id(), "", row(key).kind, "") }
    }

    #[test]
    fn a_keyboard_on_shared_ids_is_listed_once_it_has_answered() {
        let mut answered = HashSet::new();
        assert!(!listed(&mut answered, &reading("semico-1a2c", false)));
        assert!(listed(&mut answered, &reading("semico-1a2c", true)));
        assert!(listed(&mut answered, &reading("semico-1a2c", false)));
        assert!(!listed(&mut answered, &reading("mchose-k87-v3", false)));
    }

    #[test]
    fn a_keyboard_that_a_request_may_keep_awake_is_left_alone_once_it_has_answered() {
        let provider = KeyboardProvider::default();
        // HaloBattery's requests kept an ATK V75X from falling asleep
        assert_eq!(provider.delay(&[reading("evision-320f-5088", true)]), KEPT_AWAKE_WAIT);
        assert_eq!(provider.delay(&[reading("cherry-pixiu-75", true), reading("atk-z87", true)]), KEPT_AWAKE_WAIT);
        // Asleep, its receiver answers for it, and it is asked again soon to catch it waking up
        assert_eq!(provider.delay(&[reading("evision-320f-5088", false)]), secs(60));
        assert_eq!(provider.delay(&[reading("mchose-k87-v3", false), reading("cherry-pixiu-75", true)]), secs(300));
        // A CHERRY keyboard falls asleep while it is asked
        assert_eq!(provider.delay(&[reading("cherry-pixiu-75", true)]), secs(300));
        // On its cable the keyboard is powered
        let cabled = DeviceReading { via: t("via.cable"), ..reading("evision-320f-5088", true) };
        assert_eq!(provider.delay(&[cabled]), secs(300));
        assert_eq!(provider.delay(&[]), secs(60));
        assert!(KEPT_AWAKE_WAIT > secs(30 * 60), "longer than the keyboards' own half hour");
    }

    #[test]
    fn a_device_on_shared_ids_that_has_never_answered_is_asked_rarely() {
        let provider = KeyboardProvider { strangers: true, ..KeyboardProvider::default() };
        assert_eq!(provider.delay(&[]), STRANGER_WAIT);
        // Next to a keyboard that is waited for it is asked as often as that one
        assert_eq!(provider.delay(&[reading("cherry-pixiu-75", false)]), secs(60));
        assert_eq!(provider.delay(&[reading("cherry-pixiu-75", true)]), secs(300));
    }

    #[test]
    fn a_keyboard_with_ids_of_its_own_is_listed_asleep_too() {
        let mut answered = HashSet::new();
        assert!(listed(&mut answered, &reading("cherry-pixiu-75", false)));
        assert!(listed(&mut answered, &reading("alienware-pro-wireless-keyboard", false)));
    }
}
