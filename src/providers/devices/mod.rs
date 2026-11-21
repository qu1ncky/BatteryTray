//! Devices that answer a battery request on one HID collection, each a row in a table: which collection to talk
//! to and a function that asks and reads the reply. The tables are `headset.rs` and the modules below, one per
//! vendor or platform; each says where its protocol facts come from. Two providers poll them, the headsets and
//! everything else, so a slow headset doesn't hold up a mouse.
//! Byte positions in the query functions are as hidapi returns them: for devices without report IDs Windows puts a
//! 0x00 in front of every input report, which `Io::data` strips the same way hidapi does.
//! None of these is verified on hardware by this project unless its module says so, which is why the first exchange
//! with each device goes to the log.

pub mod asus;
pub mod centurion;
pub mod compx_page;
pub mod controllers;
pub mod corsair;
pub mod hyperx;
pub mod keyboards;
pub mod mice_brands;
pub mod mice_platforms;
pub mod passive_mice;
pub mod razer_audio;
pub mod usb_headsets;

use super::{group_by, headset, hex_trimmed as hex, remaining_ms, secs, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::reading::{DeviceKind, DeviceReading};
use crate::state::StateStore;
use crate::win::{self, Error};
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a reply is waited for, unless the query sets its own with `Io::set_timeout`.
const TIMEOUT_MS: u64 = 2000;

static HEADSETS: &[&[Spec]] =
    &[headset::SPECS, hyperx::SPECS, centurion::SPECS, razer_audio::SPECS, usb_headsets::SPECS];

static PERIPHERALS: &[&[Spec]] = &[
    hyperx::MICE,
    asus::SPECS,
    corsair::SPECS,
    compx_page::SPECS,
    keyboards::SPECS,
    mice_brands::SPECS,
    mice_platforms::SPECS,
    passive_mice::SPECS,
    controllers::SPECS,
];

/// `detail`: a text key or a ready text. `full`: the device itself says it is full, with the charger connected.
/// `name`: what to call the device when the table's name doesn't say (several models behind one product ID).
/// `estimate`: the level is worked out here (from a voltage, say), not the device's own number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub online: bool,
    pub level: Option<i32>,
    pub charging: bool,
    pub detail: Option<String>,
    pub full: bool,
    pub name: Option<String>,
    pub estimate: bool,
}

impl State {
    pub fn new(online: bool, level: Option<i32>, charging: bool) -> Self {
        Self { online, level, charging, detail: None, full: false, name: None, estimate: false }
    }
}

/// The device didn't answer or said it is off.
pub fn off() -> State {
    State { detail: Some("d.offOrAway".into()), ..State::new(false, None, false) }
}

pub fn off_because(detail: impl Into<String>) -> State {
    State { detail: Some(detail.into()), ..off() }
}

/// The device is on with this level, which is brought into 0-100.
pub fn on(level: i32, charging: bool) -> State {
    State::new(true, Some(level.clamp(0, 100)), charging)
}

/// Clamps v to a..b and scales it to c..d (integer arithmetic, as HeadsetControl's map()).
pub fn map(v: i32, a: i32, b: i32, c: i32, d: i32) -> i32 {
    let v = v.clamp(a, b);
    c + (v - a) * (d - c) / (b - a)
}

pub type Query = fn(&mut Io) -> win::Result<State>;

/// One device, or several that are asked the same way.
/// `name`: a text key or a ready name. `pids`: every product ID, `cable_pids`: those of them that are the device on
/// its cable (shown as "cable", the rest as "2.4 GHz"). `page`, `usage`: the collection to talk to, page 0 = any
/// vendor collection (or any collection at all when `interface` is given). `interface`: the USB interface, -1 = any.
/// `accept`: when the collection can't be told by page and usage alone (by its report lengths, say), the rule that
/// replaces them; `interface` still counts. `shared`: unrelated devices carry the same IDs, so the device is listed
/// only while a collection that fits is there, and only once it has answered (now, or ever before on this PC): what
/// never answers may be somebody else's device, which is then asked less and less often. `via`: the text key for how
/// the device is connected, "" = "2.4 GHz", or "cable" for `cable_pids`.
pub struct Spec {
    pub key: &'static str,
    pub name: &'static str,
    pub kind: DeviceKind,
    pub vid: u16,
    pub pids: &'static [u16],
    pub cable_pids: &'static [u16],
    pub page: u16,
    pub usage: u16,
    pub interface: i32,
    pub accept: Option<fn(&HidInfo) -> bool>,
    pub shared: bool,
    pub via: &'static str,
    pub query: Query,
}

impl Spec {
    /// What a row leaves out: `Spec { key: .., ..Spec::HEADSET }`.
    pub const HEADSET: Spec = Spec::base(DeviceKind::Headset);
    pub const MOUSE: Spec = Spec::base(DeviceKind::Mouse);
    pub const KEYBOARD: Spec = Spec::base(DeviceKind::Keyboard);
    pub const GAMEPAD: Spec = Spec::base(DeviceKind::Gamepad);

    const fn base(kind: DeviceKind) -> Spec {
        Spec {
            key: "",
            name: "",
            kind,
            vid: 0,
            pids: &[],
            cable_pids: &[],
            page: 0,
            usage: 0,
            interface: -1,
            accept: None,
            shared: false,
            via: "",
            query: |_| Ok(off()),
        }
    }

    fn has(&self, vid: u16, pid: u16) -> bool {
        self.vid == vid && self.pids.contains(&pid)
    }

    /// The stable key of the reading. Headsets keep the one they had before the other kinds joined.
    pub fn id(&self) -> String {
        if self.kind == DeviceKind::Headset {
            format!("headset:{}", self.key)
        } else {
            format!("dev:{}", self.key)
        }
    }
}

fn find(tables: &'static [&'static [Spec]], vid: u16, pid: u16) -> Option<&'static Spec> {
    tables.iter().flat_map(|t| t.iter()).find(|s| s.has(vid, pid))
}

/// Product IDs handled by these tables, so the general Logitech HID++ provider leaves them alone.
pub fn handled(vid: u16, pid: u16) -> bool {
    find(HEADSETS, vid, pid).or_else(|| find(PERIPHERALS, vid, pid)).is_some()
}

/// The USB IDs a table row has had an answer from in this session.
static READ: Mutex<Vec<(u16, u16)>> = Mutex::new(Vec::new());

/// Whether a table row has read this device in this session. The providers that take any device of a kind (the
/// standard HID battery, XInput) leave it alone from then on, so it isn't listed twice; until then they go on
/// showing what they can, in case the row doesn't work on this device.
pub fn read_here(vid: u16, pid: u16) -> bool {
    READ.lock().unwrap_or_else(|e| e.into_inner()).contains(&(vid, pid))
}

fn mark_read(vid: u16, pid: u16) {
    let mut read = READ.lock().unwrap_or_else(|e| e.into_inner());
    if !read.contains(&(vid, pid)) {
        read.push((vid, pid));
    }
}

/// How long a `shared` row's device that has never answered is left alone after `tries` unanswered requests in a
/// row: two minutes, then twice as long each time, up to an hour. It may be somebody else's device with the same
/// IDs, and a request it doesn't understand is known to wake some keyboards.
fn stranger_wait(tries: u32) -> Duration {
    secs((120u64 << tries.min(5)).min(3600))
}

/// Whether a `shared` row's device goes into the list: once it has answered in this session, or when this PC has
/// seen its level before.
fn proven(online: bool, answered: bool, known: bool) -> bool {
    online || answered || known
}

pub struct TableProvider {
    name: &'static str,
    tables: &'static [&'static [Spec]],
    /// How long to wait before the next poll: with a device online, and with none.
    delays: (Duration, Duration),
    /// The levels this PC has seen, for telling a `shared` row's device from somebody else's.
    state: Option<Arc<StateStore>>,
    /// The Ids that have answered in this session.
    answered: HashSet<String>,
    /// `shared` rows' devices that have never answered, by USB IDs: how many requests went unanswered and when
    /// the next one may go.
    strangers: HashMap<(u16, u16), (u32, Instant)>,
    /// The names devices gave for themselves, by Id: kept for when they are off.
    names: HashMap<String, String>,
    /// What this session's log already has: see `log_once`.
    logged: HashSet<String>,
}

impl TableProvider {
    pub fn headsets() -> Self {
        Self::new("Headsets", HEADSETS)
    }

    pub fn peripherals() -> Self {
        Self::new("Devices", PERIPHERALS).every(secs(60), secs(60))
    }

    /// A provider over tables of its own, polled every minute, or every half a minute while nothing is online.
    pub fn new(name: &'static str, tables: &'static [&'static [Spec]]) -> Self {
        Self {
            name,
            tables,
            delays: (secs(60), secs(30)),
            state: None,
            answered: HashSet::new(),
            strangers: HashMap::new(),
            names: HashMap::new(),
            logged: HashSet::new(),
        }
    }

    /// The same with other waits: `online` with a device online, `offline` with none. For devices that a request
    /// may keep awake.
    pub fn every(self, online: Duration, offline: Duration) -> Self {
        Self { delays: (online, offline), ..self }
    }

    /// With the levels this PC has seen: a `shared` row's device that has had one is listed while it is off too.
    pub fn remembering(self, state: Arc<StateStore>) -> Self {
        Self { state: Some(state), ..self }
    }

    /// Logs how a device was queried - its HID collections, the requests and the replies - the first time it is
    /// found off and the first time it is found on in a session.
    fn log_once(&mut self, spec: &Spec, collections: &[HidInfo], asked: &[(String, Vec<String>)], online: bool) {
        let id = format!("{:04x}:{:04x}", collections[0].vendor_id, collections[0].product_id);
        if !self.logged.insert(format!("{id}{online}")) {
            return;
        }
        log::write(&format!(
            "{} {id}: found {}, {} HID collections",
            t(spec.name),
            if online { "on" } else { "off" },
            collections.len()
        ));
        for info in collections {
            let trace =
                asked.iter().find(|a| a.0 == info.path).map(|a| a.1.join("  ")).unwrap_or_else(|| "not asked".into());
            log::write(&format!("  mi={} {info}: {trace}", info.interface_number()));
        }
    }

    /// One line, once, about a device with a `shared` row's IDs and no collection its query could be tried on:
    /// most likely somebody else's device, but if it is the row's, this is all the log would have of it.
    fn log_unfit(&mut self, spec: &Spec, collections: &[HidInfo]) {
        let id = format!("{:04x}:{:04x}", collections[0].vendor_id, collections[0].product_id);
        if !self.logged.insert(format!("{id}unfit")) {
            return;
        }
        let list = collections.iter().map(|i| format!("mi={} {i}", i.interface_number())).collect::<Vec<_>>();
        log::write(&format!("{} {id}: no collection fits, not listed: {}", t(spec.name), list.join("; ")));
    }

    /// Whether the device of a `shared` row has shown that it is the row's device.
    fn is_proven(&self, id: &str, online: bool) -> bool {
        let known = self.state.as_ref().is_some_and(|s| s.get(id).is_some());
        proven(online, self.answered.contains(id), known)
    }
}

impl Provider for TableProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result: Vec<(DeviceReading, bool)> = Vec::new();
        let tables = self.tables;
        // A device on Bluetooth has the same IDs in its path, and Windows shows its battery itself
        let found: Vec<HidInfo> =
            hid::enumerate(|v, p| find(tables, v, p).is_some()).into_iter().filter(|i| !i.is_bluetooth()).collect();
        let present: HashSet<(u16, u16)> = found.iter().map(|i| (i.vendor_id, i.product_id)).collect();
        self.strangers.retain(|ids, _| present.contains(ids));
        for ((vid, pid), collections) in group_by(found, |i| (i.vendor_id, i.product_id)) {
            let Some(spec) = find(tables, vid, pid) else { continue };
            let id = spec.id();
            if spec.shared {
                if !fits(spec, &collections) {
                    self.log_unfit(spec, &collections);
                    continue;
                }
                let waiting = self.strangers.get(&(vid, pid)).is_some_and(|s| Instant::now() < s.1);
                if waiting && !self.is_proven(&id, false) {
                    continue;
                }
            }
            let mut asked = Vec::new();
            let state = query(spec, &collections, &mut asked);
            self.log_once(spec, &collections, &asked, state.online);
            if state.online {
                self.answered.insert(id.clone());
                self.strangers.remove(&(vid, pid));
                mark_read(vid, pid);
            }
            if spec.shared && !self.is_proven(&id, state.online) {
                let tries = self.strangers.get(&(vid, pid)).map_or(0, |s| s.0);
                self.strangers.insert((vid, pid), (tries + 1, Instant::now() + stranger_wait(tries)));
                continue;
            }
            // A device that names itself keeps that name while it is off
            let name = match state.name {
                Some(name) => {
                    self.names.insert(id.clone(), name.clone());
                    name
                }
                None => self.names.get(&id).cloned().unwrap_or_else(|| t(spec.name)),
            };
            let cable = spec.cable_pids.contains(&pid);
            let via = match (cable, spec.via) {
                (true, _) => "via.cable",
                (false, "") => "via.24",
                (false, via) => via,
            };
            let reading = DeviceReading {
                online: state.online,
                level: state.level,
                estimate: state.estimate && state.level.is_some(),
                charging: state.charging,
                full_on_charger: state.full,
                detail: state.detail.as_deref().map(t),
                ..DeviceReading::new(id, name, spec.kind, t(via))
            };
            result.push((reading, cable));
        }
        one_per_device(result)
    }

    fn delay(&self, last: &[DeviceReading]) -> Duration {
        if last.iter().any(|r| r.online) {
            self.delays.0
        } else {
            self.delays.1
        }
    }
}

/// One reading per Id, in the order the Ids first appear. A device charging on its cable while its receiver stays
/// plugged in is listed twice under one Id: the reading that reached the device wins, and when both did (a headset
/// that keeps its radio link on the cable), the cable's.
fn one_per_device(readings: Vec<(DeviceReading, bool)>) -> Vec<DeviceReading> {
    let mut result: Vec<(DeviceReading, bool)> = Vec::new();
    for (reading, cable) in readings {
        match result.iter_mut().find(|other| other.0.id == reading.id) {
            Some(other) => {
                if (reading.online, cable) > (other.0.online, other.1) {
                    *other = (reading, cable);
                }
            }
            None => result.push((reading, cable)),
        }
    }
    result.into_iter().map(|r| r.0).collect()
}

/// The collections of a device that the row's query is tried on, the ones that take output reports first. A
/// collection Windows won't open has no usage to go by, so it passes for the "needs administrator rights" answer.
fn candidates<'a>(spec: &Spec, collections: &'a [HidInfo]) -> Vec<&'a HidInfo> {
    let mut candidates: Vec<&HidInfo> = collections
        .iter()
        .filter(|i| spec.interface < 0 || i.interface_number().max(0) == spec.interface)
        .filter(|i| {
            i.access_denied
                || match spec.accept {
                    Some(accept) => accept(i),
                    None if spec.page == 0 => i.usage_page >= 0xFF00 || spec.interface >= 0,
                    None => i.usage_page == spec.page && i.usage == spec.usage,
                }
        })
        .collect();
    candidates.sort_by_key(|i| if i.output_length > 0 { 0 } else { 1 });
    candidates
}

/// Whether the device has a collection the row's query can be tried on.
fn fits(spec: &Spec, collections: &[HidInfo]) -> bool {
    candidates(spec, collections).iter().any(|i| !i.access_denied)
}

/// `asked` is filled with the collections that were tried and what was sent to and received from each.
fn query(spec: &Spec, collections: &[HidInfo], asked: &mut Vec<(String, Vec<String>)>) -> State {
    let candidates = candidates(spec, collections);
    if candidates.is_empty() {
        return off_because("d.noLink");
    }
    if candidates.iter().all(|i| i.access_denied) {
        return off_because("d.needsAdmin");
    }

    // A clean "device off" from one collection beats an error from another that can't take the request
    let mut clean = None;
    let mut error = off();
    for info in candidates.into_iter().filter(|i| !i.access_denied) {
        let mut trace = Vec::new();
        let result = guarded(|| HidDevice::open(info).and_then(|dev| (spec.query)(&mut Io::new(dev, &mut trace))));
        match result {
            Ok(state) if state.online => {
                asked.push((info.path.clone(), trace));
                return state;
            }
            Ok(state) => {
                clean.get_or_insert(state);
            }
            Err(e) => {
                trace.push(format!("error: {e}"));
                error = off_because(tf("d.error", &[&e]));
            }
        }
        asked.push((info.path.clone(), trace));
    }
    clean.unwrap_or(error)
}

/// Runs the query of one device. A reply shorter or stranger than a query expects must cost that one device its
/// reading, not every device of the provider, so a panic ends here as an error (the panic hook has logged it).
fn guarded<T>(query: impl FnOnce() -> win::Result<T>) -> win::Result<T> {
    catch_unwind(AssertUnwindSafe(query))
        .unwrap_or_else(|_| Err(Error::with_message(0, "the query failed on a reply it didn't expect")))
}

/// Request/response helpers over one HID collection, with hidapi's view of input reports.
pub struct Io<'a> {
    dev: HidDevice,
    trace: &'a mut Vec<String>, // what was sent and received, for log_once
    timeout_ms: u64,
    feature_length: usize,
}

impl<'a> Io<'a> {
    const ERROR_INVALID_FUNCTION: u32 = 1;

    /// For a provider of its own that talks to a collection it has opened: what is sent and received goes to
    /// `trace`, for its log.
    pub fn new(dev: HidDevice, trace: &'a mut Vec<String>) -> Self {
        let feature_length = dev.info.feature_length;
        Self { dev, trace, timeout_ms: TIMEOUT_MS, feature_length }
    }

    /// The length feature reports are sent and read at from here on, the report ID included, instead of the
    /// collection's longest one: for a device that wants a report at its own length.
    pub fn set_feature_length(&mut self, length: usize) {
        self.feature_length = length;
    }

    pub fn product_id(&self) -> u16 {
        self.dev.info.product_id
    }

    /// The collection being talked to: its lengths, usage and the device's product string.
    pub fn info(&self) -> &HidInfo {
        &self.dev.info
    }

    /// How long `ask` and `receive` wait for a reply from here on, and `send` for the report to be taken.
    pub fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }

    /// An input report as hidapi returns it: without the 0x00 Windows adds when there are no report IDs.
    pub fn data(raw: &[u8]) -> &[u8] {
        if raw.first() == Some(&0x00) {
            &raw[1..]
        } else {
            raw
        }
    }

    /// Writes an output report, padded with zeros to the collection's length. As with hidapi, the first byte is
    /// the report ID: 0x00 for a device without report IDs.
    pub fn send(&mut self, report: &[u8]) -> win::Result<bool> {
        self.trace.push(format!("> {}", hex(report)));
        let sent = self.dev.write(report, self.timeout_ms as u32)?;
        if !sent {
            self.trace.push("(not sent)".into());
        }
        Ok(sent)
    }

    /// Sends a request and returns the first input report that `matches` accepts, out of `reads` at most.
    /// `or_feature`: send the request as a feature report when the collection doesn't take it as an output one -
    /// HyperHeadset found HyperX dongles that take theirs only that way on Windows.
    pub fn ask(
        &mut self,
        request: &[u8],
        matches: impl Fn(&[u8]) -> bool,
        reads: usize,
        or_feature: bool,
    ) -> win::Result<Option<Vec<u8>>> {
        self.dev.flush();
        let sent = match self.send(request) {
            Ok(sent) => sent,
            // What Windows answers to an output report the collection doesn't have
            Err(Error { code: Some(Self::ERROR_INVALID_FUNCTION), .. }) if or_feature => false,
            Err(e) => return Err(e),
        };
        if sent || (or_feature && self.set_feature(request)) {
            self.receive(matches, reads)
        } else {
            Ok(None)
        }
    }

    /// The first input report that `matches` accepts, out of `reads` at most; None when the device went silent.
    pub fn receive(&mut self, matches: impl Fn(&[u8]) -> bool, reads: usize) -> win::Result<Option<Vec<u8>>> {
        let deadline = Instant::now() + Duration::from_millis(self.timeout_ms);
        for _ in 0..reads {
            if Instant::now() >= deadline {
                break;
            }
            let Some(raw) = self.dev.read(remaining_ms(deadline))? else {
                self.trace.push("< nothing".into());
                return Ok(None);
            };
            let data = Self::data(&raw);
            self.trace.push(format!("< {}", hex(data)));
            if matches(data) {
                return Ok(Some(data.to_vec()));
            }
        }
        Ok(None)
    }

    /// Throws away whatever the device sent unasked.
    pub fn drain(&mut self) -> win::Result<()> {
        for _ in 0..16 {
            if self.dev.read(5)?.is_none() {
                break;
            }
        }
        Ok(())
    }

    /// Sends a feature report of exactly the length the device declares, as hidapi does: `report` is padded with
    /// zeros or cut to it. The first byte is the report ID.
    pub fn set_feature(&mut self, report: &[u8]) -> bool {
        let length = if self.feature_length > 0 { self.feature_length } else { report.len() };
        let mut buf = vec![0u8; length];
        let n = report.len().min(length);
        buf[..n].copy_from_slice(&report[..n]);
        self.trace.push(format!("feature > {}", hex(&buf)));
        let taken = self.dev.set_feature(&buf);
        if !taken {
            self.trace.push("(not taken)".into());
        }
        taken
    }

    /// Reads a feature report, report ID first, at the length the device declares.
    pub fn get_feature(&mut self, report_id: u8) -> Option<Vec<u8>> {
        let r = self.dev.get_feature_of(report_id, self.feature_length);
        self.traced("feature < ", r)
    }

    /// Fetches an input report over the control pipe (HidD_GetInputReport), report ID first.
    pub fn get_input_report(&mut self, report_id: u8) -> Option<Vec<u8>> {
        if self.dev.info.input_length == 0 {
            return None;
        }
        let r = self.dev.get_input_report(report_id);
        self.traced("input < ", r)
    }

    fn traced(&mut self, what: &str, report: Option<Vec<u8>>) -> Option<Vec<u8>> {
        self.trace.push(format!("{what}{}", report.as_deref().map(hex).unwrap_or_else(|| "nothing".into())));
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(mi: u8, page: u16, usage: u16, output: usize, denied: bool) -> HidInfo {
        HidInfo {
            path: format!(r"\\?\hid#vid_1234&pid_5678&mi_{mi:02x}&col{page:04x}{usage:04x}#8&1&0&0000#{{4d1e55b2}}"),
            vendor_id: 0x1234,
            product_id: 0x5678,
            usage_page: page,
            usage,
            input_length: 64,
            output_length: output,
            access_denied: denied,
            ..HidInfo::default()
        }
    }

    fn picked(spec: &Spec, all: &[HidInfo]) -> Vec<(i32, u16)> {
        candidates(spec, all).iter().map(|i| (i.interface_number(), i.usage_page)).collect()
    }

    #[test]
    fn no_product_id_is_in_two_rows() {
        let mut seen = HashSet::new();
        for spec in HEADSETS.iter().chain(PERIPHERALS).flat_map(|t| t.iter()) {
            for pid in spec.pids {
                assert!(seen.insert((spec.vid, *pid)), "{:04X}:{pid:04X} is listed twice ({})", spec.vid, spec.key);
            }
            for pid in spec.cable_pids {
                assert!(spec.pids.contains(pid), "{}: cable ID {pid:04X} is not among its IDs", spec.key);
            }
        }
    }

    #[test]
    fn rows_of_one_key_are_one_kind() {
        let all: Vec<&Spec> = HEADSETS.iter().chain(PERIPHERALS).flat_map(|t| t.iter()).collect();
        for spec in &all {
            assert!(!spec.key.is_empty() && !spec.name.is_empty(), "a row without a key or a name");
            assert!(all.iter().all(|o| o.key != spec.key || o.kind == spec.kind), "{}: two kinds", spec.key);
        }
    }

    #[test]
    fn headsets_keep_their_ids() {
        let spec = Spec { key: "arctis-9", ..Spec::HEADSET };
        assert_eq!(spec.id(), "headset:arctis-9");
        assert_eq!(Spec { key: "some-mouse", ..Spec::MOUSE }.id(), "dev:some-mouse");
    }

    #[test]
    fn page_and_usage_pick_the_collection() {
        let all = [collection(0, 0x0001, 0x0006, 0, false), collection(3, 0xFFC0, 0x0001, 64, false)];
        let spec = Spec { page: 0xFFC0, usage: 0x0001, ..Spec::HEADSET };
        assert_eq!(picked(&spec, &all), [(3, 0xFFC0)]);
        assert_eq!(picked(&Spec { interface: 2, ..spec }, &all), []);
    }

    #[test]
    fn page_zero_is_any_vendor_collection_or_any_at_all_on_a_given_interface() {
        let all = [collection(0, 0x000C, 0x0001, 0, false), collection(3, 0xFF00, 0x0001, 64, false)];
        assert_eq!(picked(&Spec::HEADSET, &all), [(3, 0xFF00)]);
        assert_eq!(picked(&Spec { interface: 0, ..Spec::HEADSET }, &all), [(0, 0x000C)]);
    }

    #[test]
    fn accept_replaces_page_and_usage() {
        let all = [collection(1, 0xFF01, 0x0001, 0, false), collection(1, 0xFF02, 0x0001, 64, false)];
        let spec = Spec { page: 0xFF01, accept: Some(|i| i.output_length == 64), ..Spec::MOUSE };
        assert_eq!(picked(&spec, &all), [(1, 0xFF02)]);
    }

    #[test]
    fn collections_that_take_output_reports_go_first() {
        let all = [collection(1, 0xFF01, 0x0001, 0, false), collection(1, 0xFF02, 0x0001, 64, false)];
        assert_eq!(picked(&Spec::HEADSET, &all), [(1, 0xFF02), (1, 0xFF01)]);
    }

    #[test]
    fn a_locked_collection_is_kept_for_the_needs_admin_answer() {
        let all = [collection(3, 0, 0, 0, true)];
        let spec = Spec { page: 0xFFC0, usage: 0x0001, interface: 3, ..Spec::HEADSET };
        assert_eq!(picked(&spec, &all), [(3, 0)]);
        assert_eq!(query(&spec, &all, &mut Vec::new()), off_because("d.needsAdmin"));
        assert_eq!(query(&spec, &[], &mut Vec::new()), off_because("d.noLink"));
    }

    #[test]
    fn shared_ids_need_a_collection_that_fits() {
        let spec = Spec { page: 0xFF02, usage: 0x0001, shared: true, ..Spec::KEYBOARD };
        assert!(fits(&spec, &[collection(1, 0xFF02, 0x0001, 64, false)]));
        assert!(!fits(&spec, &[collection(1, 0xFF60, 0x0061, 33, false)]));
        // A locked collection may be anyone's
        assert!(!fits(&spec, &[collection(1, 0, 0, 0, true)]));
    }

    #[test]
    fn a_shared_rows_device_is_listed_once_it_has_answered() {
        // Never answered and never seen: it may be somebody else's device
        assert!(!proven(false, false, false));
        assert!(proven(true, false, false));
        // Asleep now, but it answered earlier in this session, or this PC has had its level before
        assert!(proven(false, true, false));
        assert!(proven(false, false, true));
    }

    #[test]
    fn a_device_that_never_answers_is_asked_less_and_less_often() {
        let waits: Vec<u64> = (0..8).map(|tries| stranger_wait(tries).as_secs()).collect();
        assert_eq!(waits, [120, 240, 480, 960, 1920, 3600, 3600, 3600]);
    }

    #[test]
    fn the_reading_that_reached_the_device_wins_and_then_the_cables() {
        let reading = |online: bool, via: &str| DeviceReading {
            online,
            ..DeviceReading::new("dev:mouse", "Mouse", DeviceKind::Mouse, via)
        };
        let via = |readings: Vec<(DeviceReading, bool)>| {
            let one = one_per_device(readings);
            assert_eq!(one.len(), 1);
            (one[0].online, one[0].via.clone())
        };
        assert_eq!(via(vec![(reading(false, "2.4"), false), (reading(true, "cable"), true)]), (true, "cable".into()));
        assert_eq!(via(vec![(reading(true, "2.4"), false), (reading(false, "cable"), true)]), (true, "2.4".into()));
        assert_eq!(via(vec![(reading(true, "2.4"), false), (reading(true, "cable"), true)]), (true, "cable".into()));
        assert_eq!(via(vec![(reading(true, "cable"), true), (reading(true, "2.4"), false)]), (true, "cable".into()));
        let two = one_per_device(vec![
            (reading(true, "2.4"), false),
            (DeviceReading::new("dev:other", "Other", DeviceKind::Mouse, "2.4"), false),
        ]);
        assert_eq!(two.len(), 2);
    }

    #[test]
    fn a_query_that_panics_costs_one_device_its_reading() {
        let short: Vec<u8> = vec![0x01];
        let failed = guarded(|| Ok(short[5]));
        assert!(failed.is_err_and(|e| e.message.contains("didn't expect")));
        assert_eq!(guarded(|| Ok(short[0])).ok(), Some(0x01));
    }

    #[test]
    fn a_device_counts_as_read_here_once_a_row_has_had_its_answer() {
        assert!(!read_here(0x1234, 0xFFF1));
        mark_read(0x1234, 0xFFF1);
        mark_read(0x1234, 0xFFF1);
        assert!(read_here(0x1234, 0xFFF1) && !read_here(0x1234, 0xFFF2));
    }

    #[test]
    fn map_scales_and_clamps() {
        assert_eq!(map(2, 0, 4, 0, 100), 50);
        assert_eq!(map(0xA0, 0x64, 0x9A, 0, 100), 100);
    }
}
