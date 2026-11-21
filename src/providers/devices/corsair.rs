//! Corsair wireless mice and keyboards: the SLIPSTREAM receivers with whatever is paired to them, and the same
//! devices on their USB cable. They speak Corsair's Bragi property protocol on the vendor collection FF42:0001 of
//! USB interface 1, which has no report IDs (65-byte reports on Windows: a 0x00 and 64 bytes):
//!   request  TT 02 PP        read property PP of target TT
//!   reply    SS 02 ST VV VV VV VV   SS who answers, 02 the echo of "read", ST the status (00 ok, 05 no such
//!                                   property), then the value, little-endian
//! Target 08 is the receiver itself, or a device on its cable; 08 + n is the device in the receiver's slot n (1-7).
//! A reply starts with the slot number (00 from the receiver, 01 from the device in slot 1); ckb-next takes the low
//! three bits, so the target's address in that place is accepted too. Properties read here:
//!   36 (receiver)  the slots that are linked now, bit n for slot n
//!   0F  battery level in tenths of a percent, 0-1000
//!   10  charge state: 1 charging, 2 on battery, 3 charged
//!   12  product ID of the device, the one it has on its cable
//! A device that is off or asleep loses its bit, and a read sent to its slot gets no reply at all; the receiver
//! answers "no such property" for its own battery.
//! Read-only: properties are only read, which works in hardware mode, so nothing is switched to software mode and
//! no heartbeat is sent. The headset receivers use numbered reports and are in headset.rs.
//! The reply doesn't say which property it answers, and with iCUE running the answers to its requests arrive on the
//! same collection. So the level counts once two reads in a row agree, and so do the charge state and the product
//! ID.
//! A receiver doesn't say what is paired to it while the device sleeps: a device is listed from the first time it
//! answers, and stays listed as asleep for the rest of the session. Only the receivers sold with one device list
//! it from the start.
//! Protocol facts from ckb-next (src/daemon/bragi_proto.h, bragi_common.c, usb_bragi.c, keymap.c, usb.h),
//! OpenLinkHub (src/devices/devices.go, slipstream/slipstream.go and the per-device drivers), OpenRGB
//! (Controllers/CorsairPeripheralV2Controller), Wireless-Device-Battery-Monitor (src/drivers/corsair.cpp; pull
//! requests 132 and 152 with frames captured on Windows from the receivers 1B1C:2B00 and 1B1C:1BDC, 4032 on which
//! slot a reply belongs to) and BatteryWatch (contents/bin/read_hid_devices and its tests: the K57 RGB Wireless
//! behind its receiver 1B1C:1B62).
//! Own implementation. Not yet verified on hardware by this project, which is why the first exchange with each
//! receiver and each device on a cable goes to the log.
//! Left out: the SLIPSTREAM v2 receiver 1B1C:2B1B (a longer header that one young project knows, with no captured
//! frame), the older Dark Core RGB / SE and K63 Wireless (a five-step level that ckb-next and OpenLinkHub turn into
//! different percentages, on a collection nobody has seen on Windows) and the SCUF Envision Pro (one young
//! implementation, its collection on Windows unknown).

use super::Spec;
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::providers::{group_by, hex_trimmed as hex, no_link_reason, one_per_id, remaining_ms, secs, Provider};
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Nothing here is a table row: one receiver carries several devices, and what was seen behind it is remembered
/// between polls. `CorsairProvider` below does the work.
pub static SPECS: &[Spec] = &[];

const VID: u16 = 0x1B1C;
const COMMAND_PAGE: u16 = 0xFF42;
const COMMAND_USAGE: u16 = 0x0001;
const COMMAND_INTERFACE: i32 = 1;
/// The report ID Windows adds and the 64 bytes of a packet. The K100 Air, the K70 Pro Mini and the MAKR 75 on
/// their cable have longer ones.
const REPORT_LENGTH: usize = 65;
/// A device that isn't there doesn't answer, so every poll of a sleeping one waits this long.
const TIMEOUT_MS: u64 = 500;

/// The receiver, or a device on its cable; a paired device is this plus its slot.
const SELF: u8 = 0x08;
const GET: u8 = 0x02;
const LEVEL: u8 = 0x0F;
const POWER: u8 = 0x10;
const PRODUCT: u8 = 0x12;
const LINKED: u8 = 0x36;
const SLOTS: u8 = 7;
/// The bits of a reply's first byte that hold the slot, as ckb-next takes them.
const SLOT_MASK: u8 = 0x07;
const CHARGING: u32 = 1;
const CHARGED: u32 = 3;

/// How many times a level or a product ID is asked for until two answers in a row agree.
const READS: usize = 3;
/// Two level reads agree when they are this close, in tenths of a percent.
const AGREE_TENTHS: u32 = 10;

/// What a paired device is called when its product ID is one this table doesn't know.
const UNKNOWN_NAME: &str = "Corsair wireless device";

const M: DeviceKind = DeviceKind::Mouse;
const K: DeviceKind = DeviceKind::Keyboard;

/// `pids`: the product IDs the device has on its cable, which are also what it answers to property 12 behind a
/// receiver. `cable`: it has a cable to be read on (the others run on AA cells).
struct Model {
    key: &'static str,
    name: &'static str,
    kind: DeviceKind,
    pids: &'static [u16],
    cable: bool,
}

const fn model(kind: DeviceKind, key: &'static str, name: &'static str, pids: &'static [u16], cable: bool) -> Model {
    Model { key, name, kind, pids, cable }
}

static MODELS: &[Model] = &[
    model(M, "dark-core-rgb-pro-se", "Corsair Dark Core RGB Pro SE", &[0x1B7E], true),
    model(M, "dark-core-rgb-pro", "Corsair Dark Core RGB Pro", &[0x1B80], true),
    model(M, "ironclaw-rgb-wireless", "Corsair Ironclaw RGB Wireless", &[0x1B4C], true),
    model(M, "ironclaw-wireless-se", "Corsair Ironclaw Wireless SE", &[0x2B32], true),
    model(M, "harpoon-rgb-wireless", "Corsair Harpoon RGB Wireless", &[0x1B5E], true),
    model(M, "harpoon-v2-wireless", "Corsair Harpoon V2 Wireless", &[0x2B2E], true),
    model(M, "sabre-rgb-pro-wireless", "Corsair Sabre RGB Pro Wireless", &[0x1B98], true),
    model(M, "m65-rgb-ultra-wireless", "Corsair M65 RGB Ultra Wireless", &[0x1BB5, 0x1BD6], true),
    model(M, "m75-wireless", "Corsair M75 Wireless", &[0x1BF1, 0x2B08], true),
    model(M, "m75-air", "Corsair M75 Air", &[0x1BF2], true),
    model(M, "scimitar-rgb-elite-wireless", "Corsair Scimitar RGB Elite Wireless", &[0x1BDB], true),
    model(M, "scimitar-elite-wireless-se", "Corsair Scimitar Elite Wireless SE", &[0x2B22], true),
    model(M, "nightsabre-wireless", "Corsair Nightsabre Wireless", &[0x1BB8], true),
    model(M, "darkstar-wireless", "Corsair Darkstar Wireless", &[0x1BB2], true),
    model(M, "m55-wireless", "Corsair M55 Wireless", &[0x1BFB], false),
    // No source says what it answers to property 12: it is only known as the device of its own receiver
    model(M, "katar-pro-wireless", "Corsair Katar Pro Wireless", &[], false),
    model(K, "k57-rgb-wireless", "Corsair K57 RGB Wireless", &[0x1B6E], true),
    model(K, "k100-air", "Corsair K100 Air Wireless", &[0x1BAB], true),
    model(K, "k70-pro-mini-wireless", "Corsair K70 Pro Mini Wireless", &[0x1BB6, 0x1BD3], true),
    model(K, "k70-core-tkl-wireless", "Corsair K70 Core TKL Wireless", &[0x2B02], true),
    model(K, "k65-plus-wireless", "Corsair K65 Plus Wireless", &[0x2B10, 0x2B11, 0x2B23], true),
    model(K, "makr-75", "Corsair MAKR 75", &[0x2B04], true),
];

/// `own`: the key of the device a receiver is sold with. It is listed as asleep while nothing has been seen behind
/// the receiver, and it is what a device with an unknown product ID behind it is taken for. The SLIPSTREAM
/// receivers take any device, so nothing is listed for them until one answers.
struct Receiver {
    pid: u16,
    own: Option<&'static str>,
}

static RECEIVERS: &[Receiver] = &[
    Receiver { pid: 0x1BA6, own: None },
    Receiver { pid: 0x1BDC, own: None },
    Receiver { pid: 0x2B00, own: None },
    Receiver { pid: 0x1B7F, own: Some("dark-core-rgb-pro-se") },
    Receiver { pid: 0x1B81, own: Some("dark-core-rgb-pro") },
    Receiver { pid: 0x1B65, own: Some("harpoon-rgb-wireless") },
    Receiver { pid: 0x1B66, own: Some("ironclaw-rgb-wireless") },
    Receiver { pid: 0x1B62, own: Some("k57-rgb-wireless") },
    Receiver { pid: 0x1B94, own: Some("katar-pro-wireless") },
    Receiver { pid: 0x2B07, own: Some("k65-plus-wireless") },
];

fn model_of(pid: u16) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.pids.contains(&pid))
}

fn model_by_key(key: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.key == key)
}

fn receiver(pid: u16) -> Option<&'static Receiver> {
    RECEIVERS.iter().find(|r| r.pid == pid)
}

/// The model that shows up on USB under this product ID when its cable is plugged in.
fn cable_model(pid: u16) -> Option<&'static Model> {
    model_of(pid).filter(|m| m.cable)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Value(u32),
    /// The status the read was refused with: 05 when the target has no such property.
    Refused(u8),
}

/// `level`: None when the device is there but says nothing usable about its level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level: Option<i32>,
    pub charging: bool,
    pub full: bool,
}

/// A device that answered from a receiver's slot. `pid`: None when it wouldn't say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Child {
    slot: u8,
    pid: Option<u16>,
    battery: Battery,
}

/// What asking a target for a property came to.
#[derive(Debug, PartialEq, Eq)]
enum Read {
    /// Nobody answered for the target, or it has no such property.
    Absent,
    /// It answers, but two reads in a row didn't agree on a value that makes sense.
    Unsettled,
    Settled(u32),
}

/// The output report that reads `property` of `target`, the report ID first; writing pads it to the collection's
/// length.
pub fn request(target: u8, property: u8) -> [u8; 4] {
    [0x00, target, GET, property]
}

/// An input report without the report ID Windows puts in front of it; None when it isn't report 0.
fn payload(raw: &[u8]) -> Option<&[u8]> {
    match raw.split_first() {
        Some((0x00, data)) => Some(data),
        _ => None,
    }
}

/// What an input report says in answer to a read sent to `target`; None when it is something else: a reply from
/// another slot, a reply to a write, a notification.
pub fn answer(data: &[u8], target: u8) -> Option<Answer> {
    if data.len() < 7 || data[1] != GET || (data[0] != (target & SLOT_MASK) && data[0] != target) {
        return None;
    }
    Some(match data[2] {
        0x00 => Answer::Value(u32::from_le_bytes([data[3], data[4], data[5], data[6]])),
        status => Answer::Refused(status),
    })
}

/// Tenths of a percent as a percentage. Zero is what a device says before it knows (while it wakes up, while iCUE
/// starts), so it is no reading, and neither is anything over 1000.
pub fn percent(tenths: u32) -> Option<i32> {
    (1..=1000).contains(&tenths).then(|| (tenths as i32 / 10).max(1))
}

/// `tenths`: a confirmed level, `power`: the charge state if the device has one. On the cable a level of exactly
/// 100% is shown only when the device also says it is charged: the Ironclaw RGB Wireless reads a flat 100% there
/// whatever its charge (OpenLinkHub issue 569, which doesn't say what charge state came with it).
pub fn battery(tenths: Option<u32>, power: Option<u32>, cable: bool) -> Battery {
    let full = power == Some(CHARGED);
    let level = tenths.filter(|&v| !(cable && !full && v >= 1000)).and_then(percent);
    Battery { level, charging: power == Some(CHARGING), full }
}

/// The slots to ask: the ones the receiver says are linked and the ones that answered at the last poll, so a
/// bitmap that is really the answer to another program's request doesn't put a device to sleep. A receiver that
/// doesn't say is asked for its first three, as Wireless-Device-Battery-Monitor does. Slot 1 is asked even when
/// nothing is said to be linked: that is the exchange captured on Windows, while the bitmap was only captured on
/// the K57's receiver.
fn slots_to_ask(linked: Option<u32>, answered_before: impl Fn(u8) -> bool) -> Vec<u8> {
    let said = |slot: u8| match linked {
        Some(bits) => (bits >> slot) & 1 == 1,
        None => slot <= 3,
    };
    let slots: Vec<u8> = (1..=SLOTS).filter(|&slot| said(slot) || answered_before(slot)).collect();
    if slots.is_empty() {
        vec![1]
    } else {
        slots
    }
}

/// One read and its answer, so the exchange can be tested without a receiver. None: nobody answered.
trait Link {
    fn get(&mut self, target: u8, property: u8) -> win::Result<Option<Answer>>;
}

struct HidLink<'a> {
    dev: HidDevice,
    trace: &'a mut Vec<String>, // what was sent and received, for log_once
}

impl Link for HidLink<'_> {
    fn get(&mut self, target: u8, property: u8) -> win::Result<Option<Answer>> {
        let request = request(target, property);
        // What arrived before the request can't be the answer to it
        self.dev.flush();
        self.trace.push(format!("> {}", hex(&request)));
        if !self.dev.write(&request, TIMEOUT_MS as u32)? {
            self.trace.push("(not sent)".into());
            return Ok(None);
        }
        let deadline = Instant::now() + Duration::from_millis(TIMEOUT_MS);
        while Instant::now() < deadline {
            let Some(raw) = self.dev.read(remaining_ms(deadline))? else { break };
            self.trace.push(format!("< {}", hex(&raw)));
            if let Some(answer) = payload(&raw).and_then(|data| answer(data, target)) {
                return Ok(Some(answer));
            }
        }
        self.trace.push("< nothing".into());
        Ok(None)
    }
}

/// A property of `target`, once two reads in a row give values that `valid` accepts and `same` takes for one. The
/// reply doesn't name the property, so a single read could be the answer to something iCUE asked.
fn read(
    link: &mut impl Link,
    target: u8,
    property: u8,
    valid: impl Fn(u32) -> bool,
    same: impl Fn(u32, u32) -> bool,
) -> win::Result<Read> {
    let mut before = None;
    for _ in 0..READS {
        let Some(Answer::Value(now)) = link.get(target, property)? else { return Ok(Read::Absent) };
        if valid(now) && before.is_some_and(|b| same(b, now)) {
            return Ok(Read::Settled(now));
        }
        before = valid(now).then_some(now);
    }
    Ok(Read::Unsettled)
}

fn settled(read: Read) -> Option<u32> {
    match read {
        Read::Settled(value) => Some(value),
        _ => None,
    }
}

/// The product ID of `target`, if it says one.
fn read_product(link: &mut impl Link, target: u8) -> win::Result<Option<u16>> {
    let pid = read(link, target, PRODUCT, |v| (1..0xFFFF).contains(&v), |a, b| a == b)?;
    Ok(settled(pid).map(|pid| pid as u16))
}

/// The battery of whoever answers for `target`; None when nobody does.
fn read_battery(link: &mut impl Link, target: u8, cable: bool) -> win::Result<Option<Battery>> {
    let close = |a: u32, b: u32| a.abs_diff(b) <= AGREE_TENTHS;
    let tenths = match read(link, target, LEVEL, |v| percent(v).is_some(), close)? {
        Read::Absent => return Ok(None),
        level => settled(level),
    };
    // Devices on AA cells have no charge state and refuse the read
    let power = read(link, target, POWER, |v| (CHARGING..=CHARGED).contains(&v), |a, b| a == b)?;
    Ok(Some(battery(tenths, settled(power), cable)))
}

/// The devices that answer from the receiver's slots right now. `known`: the product ID of a slot's device when it
/// answered at the last poll too, which saves asking who it is again.
fn read_receiver(link: &mut impl Link, known: impl Fn(u8) -> Option<u16>) -> win::Result<Vec<Child>> {
    let linked = match link.get(SELF, LINKED)? {
        Some(Answer::Value(bits)) => Some(bits),
        _ => None,
    };
    let mut children = Vec::new();
    for slot in slots_to_ask(linked, |slot| known(slot).is_some()) {
        let target = SELF | slot;
        let Some(battery) = read_battery(link, target, false)? else { continue };
        // A zero is what a device says while it wakes up. From a slot the receiver doesn't call linked it is nobody:
        // the device would be listed as on for as long as it is off
        if battery.level.is_none() && linked.is_some_and(|bits| (bits >> slot) & 1 == 0) {
            continue;
        }
        let pid = match known(slot) {
            Some(pid) => Some(pid),
            None => read_product(link, target)?,
        };
        children.push(Child { slot, pid, battery });
    }
    Ok(children)
}

/// The collection the requests go to: FF42:0001 on interface 1, without report IDs. FF42:0002 next to it only
/// sends notifications, and the headset receivers' FF42:0001 has numbered 64-byte reports.
fn is_command(info: &HidInfo) -> bool {
    !info.access_denied
        && info.interface_number() == COMMAND_INTERFACE
        && info.usage_page == COMMAND_PAGE
        && info.usage == COMMAND_USAGE
        && info.output_report_id == 0
        && info.input_length >= REPORT_LENGTH
        && info.output_length >= REPORT_LENGTH
}

/// Who a reading is about. A device has one key whichever way it is reached, so the reading from its cable and
/// the one from its receiver are one device.
struct Identity {
    id: String,
    name: String,
    kind: DeviceKind,
}

impl Identity {
    fn of(model: &Model) -> Self {
        Self { id: format!("corsair:{}", model.key), name: model.name.into(), kind: model.kind }
    }
}

/// `pid`: what the device answered to property 12, `own`: the model its receiver is sold with.
fn identity(pid: Option<u16>, own: Option<&Model>) -> Option<Identity> {
    match (pid.and_then(model_of).or(own), pid) {
        (Some(model), _) => Some(Identity::of(model)),
        (None, Some(pid)) => {
            Some(Identity { id: format!("corsair:pid-{pid:04x}"), name: UNKNOWN_NAME.into(), kind: DeviceKind::Other })
        }
        (None, None) => None,
    }
}

/// `battery`: None when the device wasn't reached, and `why` says the reason.
fn reading(who: &Identity, via: &str, battery: Option<Battery>, why: Option<String>) -> DeviceReading {
    let level = battery.and_then(|b| b.level);
    DeviceReading {
        online: battery.is_some(),
        level,
        charging: battery.is_some_and(|b| b.charging),
        full_on_charger: battery.is_some_and(|b| b.full),
        detail: match battery {
            Some(_) if level.is_none() => Some(t("d.levelUnknown")),
            Some(_) => None,
            None => why,
        },
        ..DeviceReading::new(who.id.clone(), who.name.clone(), who.kind, t(via))
    }
}

/// A device seen in a receiver's slot this session. `receiver`: the path of the receiver's command collection.
/// `linked`: it answered at the last poll.
struct Paired {
    receiver: String,
    slot: u8,
    pid: u16,
    linked: bool,
}

#[derive(Default)]
pub struct CorsairProvider {
    /// A receiver doesn't say what is paired to it while the device sleeps, so what answered once stays listed.
    paired: Vec<Paired>,
    /// What this session's log already has: see `log_once`.
    logged: HashSet<String>,
}

impl Provider for CorsairProvider {
    fn name(&self) -> &'static str {
        "Corsair"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let found = hid::enumerate(|v, p| v == VID && (receiver(p).is_some() || cable_model(p).is_some()));
        let mut result = Vec::new();
        let mut on_cable = Vec::new();
        for ((_, pid), collections) in group_by(found, |i| (i.vendor_id, i.product_id)) {
            if let Some(receiver) = receiver(pid) {
                result.extend(self.poll_receivers(receiver, &collections));
            } else if let Some(model) = cable_model(pid) {
                on_cable.push(self.poll_cable(model, &collections));
            }
        }
        // A device charging on its cable while its receiver stays plugged in is listed twice under one Id. The
        // receiver's reading comes first: if the device still answers there, its level is the one to trust
        result.extend(on_cable);
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

impl CorsairProvider {
    /// Every receiver with this product ID: two of a kind are told apart by their collections' paths.
    fn poll_receivers(&mut self, receiver: &Receiver, collections: &[HidInfo]) -> Vec<DeviceReading> {
        let own = receiver.own.and_then(model_by_key);
        let commands: Vec<&HidInfo> = collections.iter().filter(|i| is_command(i)).collect();
        if commands.is_empty() {
            self.log_once(format!("{:04x}", receiver.pid), collections, None, &[], false);
            // Only a receiver with a device of its own has someone to say this about
            let why = t(no_link_reason(collections));
            return own.map(|m| reading(&Identity::of(m), "via.24", None, Some(why))).into_iter().collect();
        }

        let mut result = Vec::new();
        for info in commands {
            let known: Vec<(u8, u16)> =
                self.paired.iter().filter(|p| p.receiver == info.path && p.linked).map(|p| (p.slot, p.pid)).collect();
            let mut trace = Vec::new();
            let children = HidDevice::open(info).and_then(|dev| {
                let mut link = HidLink { dev, trace: &mut trace };
                read_receiver(&mut link, |slot| known.iter().find(|k| k.0 == slot).map(|k| k.1))
            });
            let children = children.map_err(|e| {
                trace.push(format!("error: {e}"));
                tf("d.error", &[&e])
            });
            let online = children.as_ref().is_ok_and(|c| !c.is_empty());
            self.log_once(info.path.clone(), collections, Some(info), &trace, online);
            result.extend(self.receiver_readings(&info.path, own, children));
        }
        result
    }

    /// The readings of one receiver: the devices that answered, then the ones that answered earlier and don't
    /// now. `children`: who answered, or why the receiver couldn't be asked.
    fn receiver_readings(
        &mut self,
        receiver: &str,
        own: Option<&Model>,
        children: Result<Vec<Child>, String>,
    ) -> Vec<DeviceReading> {
        let (children, why) = match children {
            Ok(children) => (children, t("d.asleep")),
            Err(why) => (Vec::new(), why),
        };
        self.paired.iter_mut().filter(|p| p.receiver == receiver).for_each(|p| p.linked = false);

        let mut result = Vec::new();
        for child in &children {
            let at = self.paired.iter().position(|p| p.receiver == receiver && p.slot == child.slot);
            match (at, child.pid) {
                (Some(at), Some(pid)) => (self.paired[at].pid, self.paired[at].linked) = (pid, true),
                (Some(at), None) => self.paired[at].linked = true,
                (None, Some(pid)) => {
                    self.paired.push(Paired { receiver: receiver.into(), slot: child.slot, pid, linked: true })
                }
                (None, None) => {}
            }
            let pid = child.pid.or_else(|| at.map(|at| self.paired[at].pid));
            // Without a product ID and without a receiver that says whose it is, there is no telling what answered
            if let Some(who) = identity(pid, own) {
                result.push(reading(&who, "via.24", Some(child.battery), None));
            }
        }
        for p in self.paired.iter().filter(|p| p.receiver == receiver && !p.linked) {
            if let Some(who) = identity(Some(p.pid), own) {
                result.push(reading(&who, "via.24", None, Some(why.clone())));
            }
        }
        if result.is_empty() {
            result.extend(own.map(|m| reading(&Identity::of(m), "via.24", None, Some(why))));
        }
        result
    }

    /// A device on its cable answers for itself, on target 08.
    fn poll_cable(&mut self, model: &Model, collections: &[HidInfo]) -> DeviceReading {
        let command = collections.iter().find(|i| is_command(i));
        let mut trace = Vec::new();
        let (battery, why) = match command {
            None => (None, Some(t(no_link_reason(collections)))),
            Some(info) => {
                let battery = HidDevice::open(info).and_then(|dev| {
                    let mut link = HidLink { dev, trace: &mut trace };
                    read_battery(&mut link, SELF, true)
                });
                match battery {
                    Ok(Some(battery)) => (Some(battery), None),
                    Ok(None) => (None, Some(t("d.noLink"))),
                    Err(e) => {
                        trace.push(format!("error: {e}"));
                        (None, Some(tf("d.error", &[&e])))
                    }
                }
            }
        };
        let key = command.map_or_else(|| model.key.to_string(), |i| i.path.clone());
        self.log_once(key, collections, command, &trace, battery.is_some());
        reading(&Identity::of(model), "via.cable", battery, why)
    }

    /// Logs a device's HID collections and what was sent and received, the first time it is found off and the
    /// first time it is found on in a session. `key`: what tells it from the others.
    fn log_once(
        &mut self,
        key: String,
        collections: &[HidInfo],
        command: Option<&HidInfo>,
        trace: &[String],
        online: bool,
    ) {
        if !self.logged.insert(format!("{key}{online}")) {
            return;
        }
        let Some(first) = collections.first() else { return };
        log::write(&format!(
            "Corsair {:04x}:{:04x}: found {}, {} HID collections",
            first.vendor_id,
            first.product_id,
            if online { "on" } else { "off" },
            collections.len()
        ));
        for info in collections {
            let role = if command.is_some_and(|c| c.path == info.path) { "asked" } else { "not used" };
            log::write(&format!("  mi={} {info}: {role}", info.interface_number()));
        }
        log::write(&format!("  {}", if trace.is_empty() { "nothing sent".into() } else { trace.join("  ") }));
    }
}

#[cfg(test)]
mod tests {
    //! Bragi property reads as the SLIPSTREAM receivers answer them. The captured frames are the ones published
    //! with Wireless-Device-Battery-Monitor's pull requests 152 (Dark Core RGB Pro SE behind 1B1C:1BDC, Windows,
    //! iCUE closed) and 132 (Scimitar Elite Wireless SE behind 1B1C:2B00, Windows) and in BatteryWatch's tests
    //! (K57 RGB Wireless behind 1B1C:1B62). They are input reports without the report ID Windows adds.
    use super::*;
    use crate::providers::pad;

    fn h(text: &str) -> Vec<u8> {
        text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// A 64-byte reply that starts with these bytes.
    fn frame(text: &str) -> Vec<u8> {
        pad(64, &h(text))
    }

    fn b(level: Option<i32>, charging: bool, full: bool) -> Battery {
        Battery { level, charging, full }
    }

    /// A receiver that answers from a list of (target, property, reply). A reply is used once when another one
    /// for the same read follows it and every time otherwise; a read that isn't listed gets no answer.
    struct Script {
        replies: Vec<(u8, u8, Vec<u8>)>,
        asked: Vec<(u8, u8)>,
    }

    impl Script {
        fn new(replies: &[(u8, u8, &str)]) -> Self {
            Self { replies: replies.iter().map(|r| (r.0, r.1, frame(r.2))).collect(), asked: Vec::new() }
        }
    }

    impl Link for Script {
        fn get(&mut self, target: u8, property: u8) -> win::Result<Option<Answer>> {
            self.asked.push((target, property));
            let same = |r: &(u8, u8, Vec<u8>)| r.0 == target && r.1 == property;
            let Some(at) = self.replies.iter().position(same) else { return Ok(None) };
            let reply = if self.replies.iter().skip(at + 1).any(same) {
                self.replies.remove(at).2
            } else {
                self.replies[at].2.clone()
            };
            Ok(answer(&reply, target))
        }
    }

    fn nobody(_: u8) -> Option<u16> {
        None
    }

    #[test]
    fn request_is_the_captured_one() {
        assert_eq!(request(0x09, 0x0F).to_vec(), h("00 09 02 0f"));
        assert_eq!(request(0x08, 0x36).to_vec(), h("00 08 02 36"));
    }

    #[test]
    fn dark_core_rgb_pro_se_behind_the_slipstream_receiver() {
        assert_eq!(answer(&frame("01 02 00 ac 03"), 0x09), Some(Answer::Value(940)));
        assert_eq!(percent(940), Some(94));
        assert_eq!(answer(&frame("01 02 00 02"), 0x09), Some(Answer::Value(2)));
        assert_eq!(answer(&frame("01 02 00 7e 1b"), 0x09), Some(Answer::Value(0x1B7E)));
        assert_eq!(model_of(0x1B7E).map(|m| m.name), Some("Corsair Dark Core RGB Pro SE"));
    }

    #[test]
    fn scimitar_elite_wireless_se_behind_the_newer_receiver() {
        assert_eq!(answer(&h("01 02 00 bc 02 00 00"), 0x09), Some(Answer::Value(700)));
        assert_eq!(battery(Some(700), Some(2), false), b(Some(70), false, false));
    }

    #[test]
    fn k57_keyboard_charged_behind_its_receiver() {
        assert_eq!(answer(&frame("00 02 00 02 00"), 0x08), Some(Answer::Value(2)));
        assert_eq!(answer(&frame("01 02 00 e8 03"), 0x09), Some(Answer::Value(1000)));
        assert_eq!(answer(&frame("01 02 00 03 00"), 0x09), Some(Answer::Value(3)));
        assert_eq!(battery(Some(1000), Some(3), false), b(Some(100), false, true));
    }

    #[test]
    fn the_receiver_has_no_battery_of_its_own() {
        // The value bytes of a refused read are garbage
        assert_eq!(answer(&frame("00 02 05 a4 6a 00 20"), 0x08), Some(Answer::Refused(5)));
        assert_eq!(answer(&frame("00 02 05 01"), 0x08), Some(Answer::Refused(5)));
    }

    #[test]
    fn the_receiver_answering_is_not_the_device_behind_it() {
        assert_eq!(answer(&frame("00 02 00 ac 03"), 0x09), None);
        assert_eq!(answer(&frame("01 02 00 ac 03"), 0x08), None);
    }

    #[test]
    fn a_reply_from_another_slot_is_not_the_answer() {
        assert_eq!(answer(&frame("01 02 00 ac 03"), 0x0A), None);
        assert_eq!(answer(&frame("02 02 00 ac 03"), 0x0A), Some(Answer::Value(940)));
        assert_eq!(answer(&frame("03 02 00 ac 03"), 0x0B), Some(Answer::Value(940)));
    }

    #[test]
    fn the_address_in_place_of_the_slot_is_accepted() {
        assert_eq!(answer(&frame("09 02 00 ac 03"), 0x09), Some(Answer::Value(940)));
        assert_eq!(answer(&frame("08 02 00 02"), 0x08), Some(Answer::Value(2)));
        assert_eq!(answer(&frame("0a 02 00 ac 03"), 0x09), None);
    }

    #[test]
    fn other_reports_are_not_the_answer() {
        assert_eq!(answer(&frame("01 01 00 ac 03"), 0x09), None); // reply to a write
        assert_eq!(answer(&frame("00 01 36 00 02"), 0x08), None); // link notification
        assert_eq!(answer(&h("01 02 00 ac 03"), 0x09), None); // too short
        assert_eq!(answer(&[], 0x09), None);
    }

    #[test]
    fn report_id_is_taken_off_an_input_report() {
        let raw = pad(65, &h("00 01 02 00 ac 03"));
        assert_eq!(payload(&raw).and_then(|d| answer(d, 0x09)), Some(Answer::Value(940)));
        // The same bytes one place to the left would be the receiver saying 0x03AC00
        assert_eq!(payload(&pad(65, &h("01 02 00 ac 03"))), None);
        assert_eq!(payload(&[]), None);
    }

    #[test]
    fn level_is_in_tenths_of_a_percent() {
        assert_eq!(percent(873), Some(87));
        assert_eq!(percent(1000), Some(100));
        assert_eq!(percent(10), Some(1));
    }

    #[test]
    fn a_level_under_one_percent_is_not_shown_as_empty() {
        assert_eq!(percent(1), Some(1));
        assert_eq!(percent(9), Some(1));
    }

    #[test]
    fn zero_and_more_than_full_are_no_reading() {
        assert_eq!(percent(0), None);
        assert_eq!(percent(1001), None);
        assert_eq!(percent(0x1B7E), None);
        assert_eq!(battery(Some(0), Some(2), false), b(None, false, false));
    }

    #[test]
    fn charging_and_charged() {
        assert_eq!(battery(Some(540), Some(1), false), b(Some(54), true, false));
        assert_eq!(battery(Some(1000), Some(3), true), b(Some(100), false, true));
        assert_eq!(battery(Some(540), Some(1), true), b(Some(54), true, false));
    }

    #[test]
    fn no_charge_state_is_not_charging() {
        assert_eq!(battery(Some(620), None, false), b(Some(62), false, false));
        assert_eq!(battery(Some(620), Some(940), false), b(Some(62), false, false));
    }

    #[test]
    fn full_on_the_cable_is_a_level_only_when_the_device_says_it_is_charged() {
        assert_eq!(battery(Some(1000), Some(1), true), b(None, true, false));
        assert_eq!(battery(Some(1000), Some(2), true), b(None, false, false));
        assert_eq!(battery(Some(1000), None, true), b(None, false, false));
        assert_eq!(battery(Some(1000), Some(3), true), b(Some(100), false, true));
        assert_eq!(battery(Some(990), Some(2), true), b(Some(99), false, false));
        // Behind the receiver the level is the one read before the cable went in
        assert_eq!(battery(Some(1000), Some(1), false), b(Some(100), true, false));
        assert_eq!(battery(Some(1000), None, false), b(Some(100), false, false));
    }

    fn nobody_before(_: u8) -> bool {
        false
    }

    #[test]
    fn linked_slots_come_from_the_bitmap() {
        assert_eq!(slots_to_ask(Some(0x02), nobody_before), [1]);
        assert_eq!(slots_to_ask(Some(0x04), nobody_before), [2]);
        assert_eq!(slots_to_ask(Some(0x0C), nobody_before), [2, 3]);
        assert_eq!(slots_to_ask(Some(0xFE), nobody_before), [1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn slot_one_is_asked_even_when_nothing_is_said_to_be_linked() {
        assert_eq!(slots_to_ask(Some(0), nobody_before), [1]);
        assert_eq!(slots_to_ask(Some(0x01), nobody_before), [1]); // bit 0 is the receiver
    }

    #[test]
    fn a_receiver_that_does_not_say_is_asked_for_its_first_three_slots() {
        assert_eq!(slots_to_ask(None, nobody_before), [1, 2, 3]);
        assert_eq!(slots_to_ask(None, |slot| slot == 5), [1, 2, 3, 5]);
    }

    #[test]
    fn a_slot_that_answered_at_the_last_poll_is_asked_whatever_the_bitmap_says() {
        assert_eq!(slots_to_ask(Some(0), |slot| slot == 2), [2]);
        assert_eq!(slots_to_ask(Some(0x08), |slot| slot == 1), [1, 3]);
    }

    fn read_level(link: &mut Script, target: u8) -> Read {
        read(link, target, LEVEL, |v| percent(v).is_some(), |a, b| a.abs_diff(b) <= AGREE_TENTHS).unwrap()
    }

    #[test]
    fn level_counts_once_two_reads_agree() {
        let mut link = Script::new(&[(0x09, LEVEL, "01 02 00 ac 03")]);
        assert_eq!(read_level(&mut link, 0x09), Read::Settled(940));
        assert_eq!(link.asked, [(0x09, LEVEL), (0x09, LEVEL)]);
    }

    #[test]
    fn a_level_that_dropped_a_tenth_between_two_reads_still_agrees() {
        let mut link = Script::new(&[(0x09, LEVEL, "01 02 00 ac 03"), (0x09, LEVEL, "01 02 00 ab 03")]);
        assert_eq!(read_level(&mut link, 0x09), Read::Settled(939));
    }

    #[test]
    fn an_answer_to_another_programs_request_is_outvoted() {
        // 0x1B7E is what iCUE's read of the product ID would bring; 2 is a charge state
        let stray_first =
            [(0x09, LEVEL, "01 02 00 7e 1b"), (0x09, LEVEL, "01 02 00 ac 03"), (0x09, LEVEL, "01 02 00 ac 03")];
        assert_eq!(read_level(&mut Script::new(&stray_first), 0x09), Read::Settled(940));
        let stray_between =
            [(0x09, LEVEL, "01 02 00 ac 03"), (0x09, LEVEL, "01 02 00 02"), (0x09, LEVEL, "01 02 00 ac 03")];
        assert_eq!(read_level(&mut Script::new(&stray_between), 0x09), Read::Unsettled);
    }

    #[test]
    fn a_level_of_zero_is_not_known_yet() {
        let mut link = Script::new(&[(0x09, LEVEL, "01 02 00 00 00"), (0x09, POWER, "01 02 00 02")]);
        assert_eq!(read_level(&mut link, 0x09), Read::Unsettled);
        assert_eq!(link.asked.len(), READS);
        // The device is there all the same
        assert_eq!(read_battery(&mut link, 0x09, false).unwrap(), Some(b(None, false, false)));
    }

    #[test]
    fn silence_or_a_refusal_is_nobody_there() {
        assert_eq!(read_level(&mut Script::new(&[]), 0x09), Read::Absent);
        assert_eq!(read_level(&mut Script::new(&[(0x08, LEVEL, "00 02 05 01")]), 0x08), Read::Absent);
        assert_eq!(read_battery(&mut Script::new(&[]), 0x09, false).unwrap(), None);
    }

    #[test]
    fn charge_state_counts_once_two_reads_agree() {
        // A lone 1 could be the answer to iCUE asking for the mode or the polling rate
        let stray = [
            (0x09, LEVEL, "01 02 00 ac 03"),
            (0x09, POWER, "01 02 00 01"),
            (0x09, POWER, "01 02 00 02"),
            (0x09, POWER, "01 02 00 02"),
        ];
        assert_eq!(read_battery(&mut Script::new(&stray), 0x09, false).unwrap(), Some(b(Some(94), false, false)));
        let charging = [(0x09, LEVEL, "01 02 00 ac 03"), (0x09, POWER, "01 02 00 01")];
        assert_eq!(read_battery(&mut Script::new(&charging), 0x09, false).unwrap(), Some(b(Some(94), true, false)));
    }

    #[test]
    fn product_id_counts_once_two_reads_agree() {
        let mut link = Script::new(&[(0x09, PRODUCT, "01 02 00 7e 1b")]);
        assert_eq!(read_product(&mut link, 0x09).unwrap(), Some(0x1B7E));
        let stray = [(0x09, PRODUCT, "01 02 00 ac 03"), (0x09, PRODUCT, "01 02 00 7e 1b")];
        assert_eq!(read_product(&mut Script::new(&stray), 0x09).unwrap(), Some(0x1B7E));
    }

    #[test]
    fn product_id_that_is_none_is_not_one() {
        assert_eq!(read_product(&mut Script::new(&[(0x09, PRODUCT, "01 02 00 ff ff")]), 0x09).unwrap(), None);
        assert_eq!(read_product(&mut Script::new(&[(0x09, PRODUCT, "01 02 00 7e 1b 01")]), 0x09).unwrap(), None);
        assert_eq!(read_product(&mut Script::new(&[(0x09, PRODUCT, "01 02 05")]), 0x09).unwrap(), None);
        assert_eq!(read_product(&mut Script::new(&[]), 0x09).unwrap(), None);
    }

    #[test]
    fn k57_receiver_is_asked_for_its_link_then_the_keyboard() {
        let mut link = Script::new(&[
            (0x08, LINKED, "00 02 00 02 00"),
            (0x09, LEVEL, "01 02 00 e8 03"),
            (0x09, POWER, "01 02 00 03 00"),
            (0x09, PRODUCT, "01 02 00 6e 1b"),
        ]);
        let children = read_receiver(&mut link, nobody).unwrap();
        assert_eq!(children, [Child { slot: 1, pid: Some(0x1B6E), battery: b(Some(100), false, true) }]);
        assert_eq!(
            link.asked,
            [
                (0x08, LINKED),
                (0x09, LEVEL),
                (0x09, LEVEL),
                (0x09, POWER),
                (0x09, POWER),
                (0x09, PRODUCT),
                (0x09, PRODUCT)
            ]
        );
    }

    #[test]
    fn a_device_linked_at_the_last_poll_is_not_asked_who_it_is() {
        let mut link = Script::new(&[
            (0x08, LINKED, "00 02 00 02 00"),
            (0x09, LEVEL, "01 02 00 ac 03"),
            (0x09, POWER, "01 02 00 02"),
        ]);
        let children = read_receiver(&mut link, |slot| (slot == 1).then_some(0x1B7E)).unwrap();
        assert_eq!(children, [Child { slot: 1, pid: Some(0x1B7E), battery: b(Some(94), false, false) }]);
        assert!(!link.asked.contains(&(0x09, PRODUCT)));
    }

    #[test]
    fn a_sleeping_device_leaves_the_receiver_with_nobody() {
        let mut link = Script::new(&[(0x08, LINKED, "00 02 00 00 00")]);
        assert_eq!(read_receiver(&mut link, nobody).unwrap(), []);
        assert_eq!(link.asked, [(0x08, LINKED), (0x09, LEVEL)]);
    }

    #[test]
    fn a_zero_from_a_slot_that_is_not_linked_is_nobody() {
        // Made up: no source has a receiver answer for a device that sleeps, and this is what it would look like
        let asleep = [(0x08, LINKED, "00 02 00 00 00"), (0x09, LEVEL, "01 02 00 00 00"), (0x09, POWER, "01 02 00 02")];
        assert_eq!(read_receiver(&mut Script::new(&asleep), nobody).unwrap(), []);
        // Linked, it is a device that doesn't know its level yet
        let waking = [(0x08, LINKED, "00 02 00 02 00"), (0x09, LEVEL, "01 02 00 00 00"), (0x09, POWER, "01 02 00 02")];
        let children = read_receiver(&mut Script::new(&waking), |slot| (slot == 1).then_some(0x1B7E)).unwrap();
        assert_eq!(children, [Child { slot: 1, pid: Some(0x1B7E), battery: b(None, false, false) }]);
        // And so it is behind a receiver that doesn't say who is linked
        let silent = [(0x08, LINKED, "00 02 05"), (0x09, LEVEL, "01 02 00 00 00"), (0x09, POWER, "01 02 00 02")];
        let children = read_receiver(&mut Script::new(&silent), |slot| (slot == 1).then_some(0x1B7E)).unwrap();
        assert_eq!(children, [Child { slot: 1, pid: Some(0x1B7E), battery: b(None, false, false) }]);
        // A level that reads is a device that is there, whatever the receiver says about its link
        let unlisted =
            [(0x08, LINKED, "00 02 00 00 00"), (0x09, LEVEL, "01 02 00 ac 03"), (0x09, POWER, "01 02 00 02")];
        let children = read_receiver(&mut Script::new(&unlisted), |slot| (slot == 1).then_some(0x1B7E)).unwrap();
        assert_eq!(children, [Child { slot: 1, pid: Some(0x1B7E), battery: b(Some(94), false, false) }]);
    }

    #[test]
    fn a_receiver_that_does_not_say_who_is_linked_is_asked_slot_by_slot() {
        let mut link = Script::new(&[
            (0x08, LINKED, "00 02 05"),
            (0x09, LEVEL, "01 02 00 bc 02 00 00"),
            (0x09, POWER, "01 02 00 02 00 00 00"),
            (0x09, PRODUCT, "01 02 00 22 2b"),
        ]);
        let children = read_receiver(&mut link, nobody).unwrap();
        assert_eq!(children, [Child { slot: 1, pid: Some(0x2B22), battery: b(Some(70), false, false) }]);
        assert!(link.asked.contains(&(0x0A, LEVEL)) && link.asked.contains(&(0x0B, LEVEL)));
    }

    #[test]
    fn a_mouse_and_a_keyboard_on_one_receiver_are_two_devices() {
        let mut link = Script::new(&[
            (0x08, LINKED, "00 02 00 0c"),
            (0x0A, LEVEL, "02 02 00 ac 03"),
            (0x0A, POWER, "02 02 00 02"),
            (0x0A, PRODUCT, "02 02 00 f1 1b"),
            (0x0B, LEVEL, "03 02 00 2c 01"),
            (0x0B, POWER, "03 02 00 01"),
            (0x0B, PRODUCT, "03 02 00 02 2b"),
        ]);
        let children = read_receiver(&mut link, nobody).unwrap();
        assert_eq!(
            children,
            [
                Child { slot: 2, pid: Some(0x1BF1), battery: b(Some(94), false, false) },
                Child { slot: 3, pid: Some(0x2B02), battery: b(Some(30), true, false) },
            ]
        );
        assert!(!link.asked.contains(&(0x09, LEVEL)));
    }

    #[test]
    fn a_device_on_aa_cells_refuses_the_charge_state() {
        let mut link = Script::new(&[(0x09, LEVEL, "01 02 00 58 02"), (0x09, POWER, "01 02 05")]);
        assert_eq!(read_battery(&mut link, 0x09, false).unwrap(), Some(b(Some(60), false, false)));
    }

    #[test]
    fn a_device_on_its_cable_answers_for_itself() {
        let mut link = Script::new(&[(0x08, LEVEL, "00 02 00 f4 01"), (0x08, POWER, "00 02 00 01")]);
        assert_eq!(read_battery(&mut link, SELF, true).unwrap(), Some(b(Some(50), true, false)));
        assert_eq!(read_battery(&mut Script::new(&[]), SELF, true).unwrap(), None);
    }

    fn child(slot: u8, pid: Option<u16>, level: i32) -> Child {
        Child { slot, pid, battery: b(Some(level), false, false) }
    }

    /// (id, online, level) of each reading.
    fn brief(readings: &[DeviceReading]) -> Vec<(&str, bool, Option<i32>)> {
        readings.iter().map(|r| (r.id.as_str(), r.online, r.level)).collect()
    }

    #[test]
    fn a_device_behind_a_receiver_is_named_by_its_product_id() {
        let mut p = CorsairProvider::default();
        let readings = p.receiver_readings("a", None, Ok(vec![child(1, Some(0x1B7E), 94)]));
        assert_eq!(brief(&readings), [("corsair:dark-core-rgb-pro-se", true, Some(94))]);
        assert_eq!(readings[0].name, "Corsair Dark Core RGB Pro SE");
        assert_eq!(readings[0].kind, DeviceKind::Mouse);
        assert_eq!(readings[0].via, t("via.24"));
        assert_eq!(readings[0].detail, None);
    }

    #[test]
    fn a_device_seen_once_stays_listed_while_it_sleeps() {
        let mut p = CorsairProvider::default();
        p.receiver_readings("a", None, Ok(vec![child(1, Some(0x1BDB), 80)]));
        let asleep = p.receiver_readings("a", None, Ok(vec![]));
        assert_eq!(brief(&asleep), [("corsair:scimitar-rgb-elite-wireless", false, None)]);
        assert_eq!(asleep[0].detail, Some(t("d.asleep")));
        // And is asked who it is again when it comes back
        assert!(p.paired.iter().all(|x| !x.linked));
    }

    #[test]
    fn a_slipstream_receiver_with_nobody_seen_lists_nothing() {
        let mut p = CorsairProvider::default();
        assert_eq!(p.receiver_readings("a", None, Ok(vec![])), []);
        assert_eq!(p.receiver_readings("a", None, Err("error".into())), []);
    }

    #[test]
    fn a_receiver_sold_with_a_device_lists_it_before_it_was_seen() {
        let mut p = CorsairProvider::default();
        let own = receiver(0x1B62).and_then(|r| r.own).and_then(model_by_key);
        let readings = p.receiver_readings("a", own, Ok(vec![]));
        assert_eq!(brief(&readings), [("corsair:k57-rgb-wireless", false, None)]);
        assert_eq!(readings[0].kind, DeviceKind::Keyboard);
    }

    #[test]
    fn another_device_paired_to_such_a_receiver_takes_its_place() {
        let mut p = CorsairProvider::default();
        let own = model_by_key("harpoon-rgb-wireless");
        let on = p.receiver_readings("a", own, Ok(vec![child(1, Some(0x1BF1), 55)]));
        assert_eq!(brief(&on), [("corsair:m75-wireless", true, Some(55))]);
        let off = p.receiver_readings("a", own, Ok(vec![]));
        assert_eq!(brief(&off), [("corsair:m75-wireless", false, None)]);
    }

    #[test]
    fn an_unknown_product_id_is_listed_under_that_id() {
        let mut p = CorsairProvider::default();
        let readings = p.receiver_readings("a", None, Ok(vec![child(1, Some(0x2B40), 61)]));
        assert_eq!(brief(&readings), [("corsair:pid-2b40", true, Some(61))]);
        assert_eq!(readings[0].name, UNKNOWN_NAME);
        assert_eq!(readings[0].kind, DeviceKind::Other);
    }

    #[test]
    fn an_unknown_product_id_behind_a_receiver_with_a_device_of_its_own_is_that_device() {
        let mut p = CorsairProvider::default();
        let own = model_by_key("katar-pro-wireless");
        let readings = p.receiver_readings("a", own, Ok(vec![child(1, Some(0x1B94), 61), child(2, None, 40)]));
        assert_eq!(
            brief(&readings),
            [("corsair:katar-pro-wireless", true, Some(61)), ("corsair:katar-pro-wireless", true, Some(40))]
        );
    }

    #[test]
    fn a_device_that_will_not_say_who_it_is_is_left_out() {
        let mut p = CorsairProvider::default();
        assert_eq!(p.receiver_readings("a", None, Ok(vec![child(1, None, 61)])), []);
        // Unless the slot answered before
        p.receiver_readings("a", None, Ok(vec![child(1, Some(0x1BB8), 61)]));
        let readings = p.receiver_readings("a", None, Ok(vec![child(1, None, 60)]));
        assert_eq!(brief(&readings), [("corsair:nightsabre-wireless", true, Some(60))]);
    }

    #[test]
    fn two_receivers_keep_their_devices_apart() {
        let mut p = CorsairProvider::default();
        p.receiver_readings("a", None, Ok(vec![child(1, Some(0x1B7E), 94)]));
        p.receiver_readings("b", None, Ok(vec![child(1, Some(0x1BAB), 70)]));
        assert_eq!(brief(&p.receiver_readings("a", None, Ok(vec![]))), [("corsair:dark-core-rgb-pro-se", false, None)]);
        assert_eq!(
            brief(&p.receiver_readings("b", None, Ok(vec![child(1, Some(0x1BAB), 69)]))),
            [("corsair:k100-air", true, Some(69))]
        );
    }

    #[test]
    fn a_receiver_that_cannot_be_asked_says_why_for_the_devices_it_had() {
        let mut p = CorsairProvider::default();
        p.receiver_readings("a", None, Ok(vec![child(1, Some(0x1B7E), 94)]));
        let readings = p.receiver_readings("a", None, Err("error: Access is denied.".into()));
        assert_eq!(brief(&readings), [("corsair:dark-core-rgb-pro-se", false, None)]);
        assert_eq!(readings[0].detail.as_deref(), Some("error: Access is denied."));
    }

    #[test]
    fn a_device_without_a_level_is_on_with_the_level_not_known() {
        let who = Identity::of(model_of(0x1B4C).unwrap());
        let r = reading(&who, "via.cable", Some(b(None, true, false)), None);
        assert!(r.online && r.charging && r.level.is_none());
        assert_eq!(r.detail, Some(t("d.levelUnknown")));
        assert_eq!(r.via, t("via.cable"));
    }

    #[test]
    fn the_cable_and_the_receiver_give_one_device() {
        let mut p = CorsairProvider::default();
        p.receiver_readings("a", None, Ok(vec![child(1, Some(0x1B7E), 94)]));
        // The cable goes in: the mouse leaves the receiver and shows up under its own product ID
        let mut readings = p.receiver_readings("a", None, Ok(vec![]));
        let on_cable = Identity::of(cable_model(0x1B7E).unwrap());
        readings.push(reading(&on_cable, "via.cable", Some(b(Some(95), true, false)), None));
        let merged = one_per_id(readings, |r| r.online);
        assert_eq!(brief(&merged), [("corsair:dark-core-rgb-pro-se", true, Some(95))]);
        assert_eq!(merged[0].via, t("via.cable"));
    }

    #[test]
    fn devices_on_aa_cells_are_not_looked_for_on_a_cable() {
        assert!(model_of(0x1BFB).is_some() && cable_model(0x1BFB).is_none());
        assert!(cable_model(0x2B11).is_some());
        assert!(cable_model(0x1BDC).is_none());
    }

    #[test]
    fn no_product_id_is_claimed_twice() {
        let mut seen = HashSet::new();
        for pid in MODELS.iter().flat_map(|m| m.pids).chain(RECEIVERS.iter().map(|r| &r.pid)) {
            assert!(seen.insert(*pid), "{pid:04X} is listed twice");
        }
        let mut keys = HashSet::new();
        assert!(MODELS.iter().all(|m| keys.insert(m.key)), "a key is used twice");
        for r in RECEIVERS {
            if let Some(key) = r.own {
                assert!(model_by_key(key).is_some(), "{:04X}: no model with the key {key}", r.pid);
            }
        }
    }

    #[test]
    fn the_headsets_and_their_receivers_are_left_to_the_headset_table() {
        // They answer the same reads on numbered reports of another interface, and have their own rows
        let headsets = crate::providers::headset::SPECS.iter().filter(|s| s.vid == VID).flat_map(|s| s.pids);
        for pid in headsets {
            assert!(receiver(*pid).is_none() && model_of(*pid).is_none(), "{pid:04X} is a headset's");
        }
    }

    fn collection(mi: u8, usage: u16, input: usize, output: usize, id: u8) -> HidInfo {
        HidInfo {
            path: format!(r"\\?\hid#vid_1b1c&pid_1bdc&mi_{mi:02x}#8&1&0&0000#{{4d1e55b2}}"),
            vendor_id: 0x1B1C,
            product_id: 0x1BDC,
            usage_page: 0xFF42,
            usage,
            input_length: input,
            output_length: output,
            output_report_id: id,
            ..HidInfo::default()
        }
    }

    #[test]
    fn requests_go_to_the_unnumbered_collection_of_interface_one() {
        assert!(is_command(&collection(1, 0x0001, 65, 65, 0)));
        assert!(!is_command(&collection(2, 0x0002, 65, 0, 0))); // notifications
        assert!(!is_command(&collection(2, 0x0001, 65, 65, 0)));
        assert!(!is_command(&HidInfo { usage_page: 0x0001, ..collection(1, 0x0006, 65, 65, 0) }));
    }

    #[test]
    fn a_headset_receivers_numbered_collection_is_not_one() {
        assert!(!is_command(&collection(1, 0x0001, 64, 64, 2)));
        assert!(!is_command(&collection(1, 0x0001, 64, 64, 0)));
    }

    #[test]
    fn keyboards_with_long_reports_are_asked_at_their_own_length() {
        assert!(is_command(&collection(1, 0x0001, 1025, 1025, 0)));
    }

    #[test]
    fn a_locked_collection_is_never_asked() {
        let locked = HidInfo { access_denied: true, ..collection(1, 0x0001, 65, 65, 0) };
        assert!(!is_command(&locked));
        assert_eq!(no_link_reason(&[locked]), "d.needsAdmin");
    }
}
