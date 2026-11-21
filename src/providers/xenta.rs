//! Attack Shark mice on the Xenta 2.4 GHz receiver 1D57:FA60 ("2.4G Wireless Device"), and the Delux and Inphic
//! mice that share it; and the 8K receiver 1D57:FA65 of the Attack Shark X8 Pro, the Inphic IN10 MAX and others.
//! Nothing is ever sent: the mouse pushes a 5-byte input report by itself
//! about every 2 s on the receiver's collection with usage page 0x000A, and this only reads it, on its own
//! GENERIC_READ handle:
//!   03 MM 40 ST LL  -  report ID 3, model code, 0x40 = battery frame (some models send 0x41), state, level.
//! What ST and LL mean depends on the model code, so only codes seen on real mice are read:
//!   0x55 (X11), 0x4E (X3 Max), 0xE5 (V3 Pro), 0x07 (X11 SE), 0x08 (X8 Pro), 0x24 (V6), 0x20 (Delux M600 Pro),
//!   0x30 and 0x50 (Delux M900 Pro), 0x73 and 0x74 (Inphic IN10R MAX, IN10 MAX): ST 01 = on battery, LL = percent;
//!     02 and 03 = on external power (cable or dock), when the level they send is frozen, so none is shown.
//!   0x10 (X6 / R1): ST is the DPI stage and LL the level in tens; there is no charging state.
//!   The V6 was only ever seen at 100%, which doesn't tell percent from tens, so its levels up to 10 aren't shown.
//! A mouse with another code is listed without a level, and its frames go to the log so that it can be added.
//! Each time the mouse links up it sends one "on battery, 100%" frame before the real one, so a reading only
//! counts once two battery frames in a row agree.
//! On a USB cable the mouse leaves the receiver, which goes quiet, and shows up as its own USB device with the
//! same collections (1D57:FA55 for the X11): it takes over the receiver's slot, as charging.
//! One firmware of the FA65 receiver has its frames on the vendor collection FF00:0001 instead of page 0x000A.
//! Protocol facts from daeoxide/attackshark-battery-windows (docs/PROTOCOL.md), Attack-Shark-Battery-KDE
//! (gitlab.com/gabriel17166), the HaloBattery community fork (providers/inphic.py), OpenMouse (mouse-protocol:
//! src/drivers/attackshark/hid.ts, docs/delux-m600-pro-testing.md, docs/attackshark-x11-testing.md and an X8 Pro
//! owner's notes in its issue 171) and Wireless-Device-Battery-Monitor (docs/inphic.md, src/drivers/misc_mice.cpp);
//! own implementation.

use super::{group_by, secs, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::monitor::{no_push, Pusher};
use crate::reading::{DeviceKind, DeviceReading};
use crate::state::StateStore;
use crate::win;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const VID: u16 = 0x1D57;
const RECEIVER_PID: u16 = 0xFA60;
/// The 8K receiver ("2.4G 8K HS Receiver", "Inphic KP 8K").
const RECEIVER_8K_PID: u16 = 0xFA65;
const RECEIVER_PIDS: [u16; 2] = [RECEIVER_PID, RECEIVER_8K_PID];
/// The mouse on its USB cable: FA55 is the X11, 2155 the mouse of Attack-Shark-Battery-KDE, FA71 the Delux M600 Pro,
/// 2126 the X8 Pro.
const WIRED_PIDS: [u16; 4] = [0xFA55, 0x2155, 0xFA71, 0x2126];
const STATUS_PAGE: u16 = 0x000A;
/// Where one firmware of the 8K receiver sends its frames.
const VENDOR_STATUS: (u16, u16) = (0xFF00, 0x0001);
const REPORT_ID: u8 = 0x03;
/// The battery frame; some models (the V6, the IN10 MAX) send 0x41 with the same contents.
const BATTERY_FRAMES: [u8; 2] = [0x40, 0x41];
const ON_BATTERY: u8 = 0x01;
const RECEIVER_ID: &str = "xenta:1d57:fa60";
const DEVICE_NAME: &str = "Attack Shark";
/// How long a poll waits for a new listener's first reading, so the mouse doesn't show up as asleep for a moment:
/// a reading takes two frames, 2 s apart.
const FIRST_READING_WAIT: Duration = secs(5);

/// (code, the mouse it was seen on - for the log only, other models may send the same code, the 0x10 layout)
const MODELS: [(u8, &str, bool); 12] = [
    (0x55, "Attack Shark X11", false),
    (0x4E, "Attack Shark X3 Max", false),
    (0xE5, "Attack Shark V3 Pro", false),
    (0x10, "Attack Shark X6 / R1", true),
    (0x07, "Attack Shark X11 SE", false),
    (0x08, "Attack Shark X8 Pro", false),
    (0x24, "Attack Shark V6", false),
    (0x20, "Delux M600 Pro", false),
    (0x30, "Delux M900 Pro", false),
    (0x50, "Delux M900 Pro", false),
    (0x73, "Inphic IN10R MAX", false),
    (0x74, "Inphic IN10 MAX", false),
];

fn model(code: u8) -> Option<(&'static str, bool)> {
    MODELS.iter().find(|m| m.0 == code).map(|m| (m.1, m.2))
}

/// Codes whose only captured frame is "on battery, 100%" - which is also the false frame every mouse sends when it
/// links up, the ones that count in tens too. A level of 10 or less from such a mouse may be tens, so it isn't
/// shown until one of them has been seen at another level.
const SCALE_UNPROVEN: [u8; 1] = [0x24];

/// Whose mouse sends this code. The model isn't shown: several send the same code.
fn brand(code: u8) -> Option<&'static str> {
    let (seen_on, _) = model(code)?;
    [DEVICE_NAME, "Delux", "Inphic"].into_iter().find(|b| seen_on.starts_with(b))
}

/// What a mouse is called before it has sent a code: these IDs are Attack Shark's own, the rest are shared.
fn default_name(pid: u16) -> String {
    match pid {
        RECEIVER_PID | 0xFA55 | 0x2155 | 0x2126 => DEVICE_NAME.to_string(),
        0xFA71 => "Delux".to_string(),
        _ => t("name.mouse"),
    }
}

/// The receiver a mouse with this cable ID comes with: the X8 Pro's is the 8K one.
fn receiver_of(wired_pid: u16) -> u16 {
    if wired_pid == 0x2126 {
        RECEIVER_8K_PID
    } else {
        RECEIVER_PID
    }
}

fn known(code: Option<u8>) -> bool {
    code.is_some_and(|c| model(c).is_some())
}

/// What one battery frame says. `level` is None on external power, or when the frame has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub on_power: bool,
    pub level: Option<i32>,
}

/// What the frames of one status collection said so far. `running`: its listener still reads. `battery`: the last
/// reading two frames in a row agreed on, since the mouse linked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    pub online: bool,
    pub code: Option<u8>,
    pub battery: Option<Battery>,
    pub error: Option<String>,
}

/// The model code and what the frame says, or None when the report isn't a battery frame. The battery is None when
/// the model code is unknown or the frame doesn't fit its layout.
pub fn parse(report: &[u8]) -> Option<(u8, Option<Battery>)> {
    if report.len() < 5 || report[0] != REPORT_ID || !BATTERY_FRAMES.contains(&report[2]) {
        return None;
    }
    let (code, st, level) = (report[1], report[3], report[4] as i32);
    let Some((_, tens)) = model(code) else { return Some((code, None)) };
    if tens {
        return Some((
            code,
            (level <= 10).then(|| Battery { on_power: false, level: (level != 0).then_some(level * 10) }),
        ));
    }
    // 0 is what devices of this kind send before they have measured: unknown, not empty
    let readable = level != 0 && !(SCALE_UNPROVEN.contains(&code) && level <= 10);
    Some((
        code,
        match st {
            ON_BATTERY if level <= 100 => Some(Battery { on_power: false, level: readable.then_some(level) }),
            0x02 | 0x03 => Some(Battery { on_power: true, level: None }),
            _ => None,
        },
    ))
}

pub struct XentaReceiverProvider {
    state: Arc<StateStore>,
    push: Pusher,
    listeners: HashMap<String, FrameListener>, // by collection path
    logged_errors: HashMap<String, String>,
    described: HashSet<u16>,
}

impl XentaReceiverProvider {
    pub fn new(state: Arc<StateStore>) -> Self {
        Self {
            state,
            push: no_push(),
            listeners: HashMap::new(),
            logged_errors: HashMap::new(),
            described: HashSet::new(),
        }
    }

    /// The readings for the receivers' and cables' status collections, in the order Windows lists them.
    pub fn build(
        &self,
        receivers: &[(u16, Status)],
        wired: &[(u16, Status)],
        receiver_locked: bool,
    ) -> Vec<DeviceReading> {
        let mut result: Vec<DeviceReading> = Vec::new();
        for (pid, s) in receivers {
            let id = next_id(&result, &format!("xenta:1d57:{pid:04x}"));
            result.push(self.reading(id, *pid, s, false));
        }
        if receivers.is_empty() && receiver_locked {
            result.push(DeviceReading {
                detail: Some(t("d.needsAdmin")),
                ..DeviceReading::new(RECEIVER_ID, DEVICE_NAME, DeviceKind::Mouse, t("via.24"))
            });
        }

        // A mouse on the cable is the one that went quiet on its receiver when the cable went in. A quiet receiver
        // of the other kind is another mouse asleep
        let mut slots: Vec<usize> = (0..receivers.len()).collect(); // the receivers' indexes in result
        for (pid, s) in wired {
            if !s.online {
                continue; // a mouse on the cable that sends no battery frames: nothing to show
            }
            match slots.iter().position(|&i| !result[i].online && receivers[i].0 == receiver_of(*pid)) {
                Some(slot) => {
                    let at = slots.remove(slot);
                    let id = result[at].id.clone();
                    result[at] = self.reading(id, *pid, s, true);
                }
                None => {
                    let id = next_id(&result, &format!("xenta:1d57:{pid:04x}"));
                    result.push(self.reading(id, *pid, s, true));
                }
            }
        }
        result
    }

    pub fn reading(&self, id: String, pid: u16, s: &Status, cable: bool) -> DeviceReading {
        // The brand comes with the model code; a mouse that hasn't sent one this session keeps the name it had
        let name = s
            .code
            .and_then(brand)
            .map(str::to_string)
            .or_else(|| self.state.get(&id).map(|was| was.name).filter(|name| !name.is_empty()))
            .unwrap_or_else(|| default_name(pid));
        let mut r = DeviceReading::new(id, name, DeviceKind::Mouse, t(if cable { "via.cable" } else { "via.24" }));
        // Silence is the mouse asleep, switched off or on Bluetooth - never an empty battery
        if !s.online {
            r.detail = Some(match &s.error {
                Some(e) => tf("d.error", &[e]),
                None => t("d.asleep"),
            });
            return r;
        }
        r.online = true;
        if !known(s.code) {
            r.detail = Some(t("d.unknownModel"));
            return r;
        }
        let Some(b) = s.battery else {
            r.detail = Some(t("d.levelUnknown"));
            return r;
        };
        if b.on_power {
            let before = self.state.get(&r.id).and_then(|x| x.level);
            let why = t(if cable { "d.cableHidden" } else { "d.chargerHidden" });
            r.charging = true;
            r.detail = Some(match before {
                Some(was) => format!("{why} · {}", tf("d.wasBefore", &[&was])),
                None => why,
            });
            return r;
        }
        let Some(level) = b.level else {
            r.detail = Some(t("d.levelUnknown"));
            return r;
        };
        let tens = s.code.and_then(model).is_some_and(|m| m.1);
        r.level = Some(level);
        r.estimate = tens;
        r.detail = tens.then(|| t("d.tenSteps"));
        r
    }

    /// Starts a listener for every new status collection and drops the ones that are gone.
    fn track(&mut self, status: &[HidInfo]) {
        let paths: Vec<String> = self.listeners.keys().cloned().collect();
        for path in paths {
            let present = status.iter().any(|i| i.path == path);
            let l = &self.listeners[&path];
            if l.running() && present {
                continue;
            }
            // A listener stops by itself when its device is unplugged; a collection still there that can't be read
            // is worth one line in the log (it gets a new listener right below and is tried again)
            if present {
                if let Some(error) = l.current().error {
                    if self.logged_errors.get(&path) != Some(&error) {
                        log::write(&format!(
                            "Xenta 1d57:{:04x}: cannot read the status collection: {error}",
                            l.info.product_id
                        ));
                        self.logged_errors.insert(path.clone(), error);
                    }
                }
            }
            self.listeners.remove(&path); // dropping it stops it
        }

        let mut fresh = Vec::new();
        for info in status {
            if self.listeners.contains_key(&info.path) {
                continue;
            }
            self.listeners.insert(info.path.clone(), FrameListener::start(info.clone(), self.push.clone()));
            fresh.push(info.path.clone());
        }
        let until = Instant::now() + FIRST_READING_WAIT;
        while fresh.iter().any(|p| !settled(&self.listeners[p].current())) && Instant::now() < until {
            super::sleep_ms(100);
        }
    }

    /// One line per USB ID and session with its HID collections: enough to see from a user's log whether a mouse
    /// that isn't read has the status collection at all.
    fn describe(&mut self, all: &[HidInfo], status: &[HidInfo]) {
        for (pid, collections) in group_by(all.to_vec(), |i| i.product_id) {
            if !self.described.insert(pid) {
                continue;
            }
            let list = collections
                .iter()
                .map(|i| {
                    if i.access_denied {
                        format!("mi{} access denied", i.interface_number())
                    } else {
                        format!(
                            "mi{} {:04X}:{:04X} in={} feat={}",
                            i.interface_number(),
                            i.usage_page,
                            i.usage,
                            i.input_length,
                            i.feature_length
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let listening = status.iter().any(|i| i.product_id == pid);
            log::write(&format!(
                "Xenta 1d57:{pid:04x}: {}; collections: {list}",
                if listening { "listening for battery frames" } else { "no readable collection on page 000A" }
            ));
        }
    }
}

/// The collections the frames come on: the one with usage page 0x000A, or on an 8K receiver that has none, its
/// vendor collection.
fn status_collections(all: &[HidInfo]) -> Vec<HidInfo> {
    let readable = |i: &&HidInfo| !i.access_denied && i.input_length >= 5;
    let on_status_page = |i: &&HidInfo| readable(i) && i.usage_page == STATUS_PAGE;
    let vendor_only = !all.iter().any(|i| i.product_id == RECEIVER_8K_PID && on_status_page(&i));
    all.iter()
        .filter(|i| {
            on_status_page(i)
                || (vendor_only
                    && readable(i)
                    && i.product_id == RECEIVER_8K_PID
                    && (i.usage_page, i.usage) == VENDOR_STATUS)
        })
        .cloned()
        .collect()
}

/// The listener has something to show: a confirmed reading, a model it can't read, or an error.
fn settled(s: &Status) -> bool {
    !s.running || s.battery.is_some() || (s.online && !known(s.code))
}

fn next_id(result: &[DeviceReading], id: &str) -> String {
    let same = result.iter().filter(|r| r.id == id || r.id.starts_with(&format!("{id}#"))).count();
    if same == 0 {
        id.to_string()
    } else {
        format!("{id}#{}", same + 1)
    }
}

impl Provider for XentaReceiverProvider {
    fn name(&self) -> &'static str {
        "Xenta 2.4G"
    }

    fn set_pusher(&mut self, push: Pusher) {
        self.push = push;
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let all = hid::enumerate(|v, p| v == VID && (RECEIVER_PIDS.contains(&p) || WIRED_PIDS.contains(&p)));
        let status = status_collections(&all);
        self.describe(&all, &status);
        self.track(&status);

        let current = |receiver: bool| -> Vec<(u16, Status)> {
            status
                .iter()
                .filter(|i| RECEIVER_PIDS.contains(&i.product_id) == receiver)
                .map(|i| (i.product_id, self.listeners[&i.path].current()))
                .collect()
        };
        let (receivers, wired) = (current(true), current(false));
        // A third-party driver can lock the receiver's collections to administrators (see the README)
        let locked = all.iter().any(|i| i.product_id == RECEIVER_PID && i.access_denied && i.interface_number() == 2);
        self.build(&receivers, &wired, locked)
    }

    fn delay(&self, last: &[DeviceReading]) -> Duration {
        // The listeners push every change; polling is for receivers and cables that come and go
        if last.iter().any(|r| r.online) {
            secs(30)
        } else {
            secs(15)
        }
    }
}

/// What the frames of one collection said so far. No I/O and no clock of its own, so it can be fed by hand. Frames
/// it can't read go to the log, the first few of each, so that a new model can be added from a user's log.
#[derive(Default)]
pub struct FrameTracker {
    logged_frames: HashSet<String>,
    candidate: Option<Battery>,
    last_frame: Option<Instant>,
    pub online: bool,
    pub code: Option<u8>,
    /// The last reading two battery frames in a row agreed on, since the mouse linked up.
    pub confirmed: Option<Battery>,
}

impl FrameTracker {
    /// The mouse sends every 2 s while it's linked; this much silence means it's gone.
    pub const SILENT_AFTER: Duration = secs(15);
    const LOGGED_FRAMES_MAX: usize = 16;

    /// Takes one input report; true when what a poll would show has changed. Lines worth logging go to `log`.
    pub fn feed(&mut self, report: &[u8], now: Instant, log: &mut Vec<String>) -> bool {
        let Some((code, battery)) = parse(report) else {
            // Unremarkable once the model is known (link blinks, acknowledgements)
            if !known(self.code) {
                self.log_frame(log, report, "not a battery frame");
            }
            return false;
        };
        let mut news = false;
        self.last_frame = Some(now);
        self.online = true;
        if self.code != Some(code) {
            self.code = Some(code);
            news = true;
            log.push(match model(code) {
                Some((seen_on, _)) => format!("model code 0x{code:02X} (seen on the {seen_on})"),
                None => format!("model code 0x{code:02X} is not known yet; its frames follow"),
            });
        }
        match battery {
            Some(b) => {
                if self.candidate == Some(b) && self.confirmed != Some(b) {
                    self.confirmed = Some(b);
                    news = true;
                }
                self.candidate = Some(b);
            }
            None => self.log_frame(log, report, if known(Some(code)) { "unknown state" } else { "unknown model" }),
        }
        news
    }

    /// True when the mouse has just gone quiet.
    pub fn tick(&mut self, now: Instant) -> bool {
        if !self.online || self.last_frame.is_some_and(|t| now.saturating_duration_since(t) < Self::SILENT_AFTER) {
            return false;
        }
        self.lost();
        true
    }

    /// The link is gone. The next link-up starts with the false 100% again, so it's confirmed afresh.
    pub fn lost(&mut self) {
        self.online = false;
        self.candidate = None;
        self.confirmed = None;
    }

    fn log_frame(&mut self, log: &mut Vec<String>, report: &[u8], why: &str) {
        let hex = super::hex(report);
        if self.logged_frames.len() < Self::LOGGED_FRAMES_MAX && self.logged_frames.insert(hex.clone()) {
            log.push(format!("frame {hex} ({why})"));
        }
    }
}

struct ListenerShared {
    inner: Mutex<(FrameTracker, Option<String>)>, // the tracker and the error that stopped the listener
    stopped: AtomicBool,
    running: AtomicBool,
}

/// Reads one status collection on its own thread into a FrameTracker.
struct FrameListener {
    info: HidInfo,
    shared: Arc<ListenerShared>,
}

impl FrameListener {
    /// A listener that stops after running this long (receiver unplugged) asks for a poll at once. One that fails
    /// right away waits for the next poll, or polls would follow each other without a pause.
    const PUSH_ON_STOP_AFTER: Duration = secs(10);

    fn start(info: HidInfo, changed: Pusher) -> Self {
        let shared = Arc::new(ListenerShared {
            inner: Mutex::new((FrameTracker::default(), None)),
            stopped: AtomicBool::new(false),
            running: AtomicBool::new(true),
        });
        let (s, i) = (shared.clone(), info.clone());
        if std::thread::Builder::new().name("Xenta frames".into()).spawn(move || Self::listen(i, s, changed)).is_err() {
            shared.running.store(false, Ordering::Relaxed);
        }
        Self { info, shared }
    }

    fn running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    fn current(&self) -> Status {
        let inner = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        Status {
            running: self.running(),
            online: inner.0.online,
            code: inner.0.code,
            battery: inner.0.confirmed,
            error: inner.1.clone(),
        }
    }

    /// The listening thread owns the handle and closes it itself, so no read is ever left pending on a closed handle.
    fn listen(info: HidInfo, shared: Arc<ListenerShared>, changed: Pusher) {
        // However it ends, a panic included: a listener that isn't running is started again by the next poll
        let _running = super::ClearOnDrop(&shared.running);
        let started = Instant::now();
        let result = (|| -> win::Result<()> {
            let dev = HidDevice::open_read_only(&info)?;
            let mut lines = Vec::new();
            while !shared.stopped.load(Ordering::Relaxed) {
                let report = dev.read(1000)?;
                let news = {
                    let mut inner = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
                    let fed = report.is_some_and(|r| inner.0.feed(&r, Instant::now(), &mut lines));
                    let quiet = inner.0.tick(Instant::now());
                    fed || quiet
                };
                for line in lines.drain(..) {
                    log::write(&format!("Xenta 1d57:{:04x}: {line}", info.product_id));
                }
                if news {
                    changed();
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let mut inner = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            // Windows' own text ("Access is denied.") rather than "Cannot open <a long device path>"
            inner.1 = Some(e.code.map(win::system_message).unwrap_or(e.message));
            inner.0.lost();
        }
        shared.running.store(false, Ordering::Relaxed);
        if !shared.stopped.load(Ordering::Relaxed) && started.elapsed() > Self::PUSH_ON_STOP_AFTER {
            changed();
        }
    }
}

impl Drop for FrameListener {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_frames_by_model() {
        assert_eq!(parse(&[3, 0x55, 0x40, 1, 77]), Some((0x55, Some(Battery { on_power: false, level: Some(77) }))));
        assert_eq!(parse(&[3, 0x55, 0x40, 2, 85]), Some((0x55, Some(Battery { on_power: true, level: None }))));
        assert_eq!(parse(&[3, 0x4E, 0x40, 1, 0]), Some((0x4E, Some(Battery { on_power: false, level: None }))));
        assert_eq!(parse(&[3, 0x10, 0x40, 2, 7]), Some((0x10, Some(Battery { on_power: false, level: Some(70) }))));
        assert_eq!(parse(&[3, 0x10, 0x40, 2, 11]), Some((0x10, None)));
        assert_eq!(parse(&[3, 0x77, 0x40, 1, 50]), Some((0x77, None)), "unknown model");
        assert_eq!(parse(&[3, 0x55, 0x01, 0, 0]), None, "link blink");
        assert_eq!(parse(&[3, 0x55, 0x40, 1]), None, "too short");
    }

    #[test]
    fn frames_of_the_mice_added_later() {
        let on_battery = |level| Some(Battery { on_power: false, level: Some(level) });
        // Captured by their owners: an X11 SE at 85%, a Delux M600 Pro at 77%, an X8 Pro at 78%
        assert_eq!(parse(&[3, 0x07, 0x40, 1, 0x55]), Some((0x07, on_battery(85))));
        assert_eq!(parse(&[3, 0x20, 0x40, 1, 0x4D]), Some((0x20, on_battery(77))));
        assert_eq!(parse(&[3, 0x08, 0x40, 1, 0x4E]), Some((0x08, on_battery(78))));
        assert_eq!(parse(&[3, 0x08, 0x40, 3, 0x4E]), Some((0x08, Some(Battery { on_power: true, level: None }))));
        // The X8 Pro's DPI button
        assert_eq!(parse(&[3, 0x08, 0x10, 3, 0]), None);
    }

    #[test]
    fn some_models_send_41_for_the_battery_frame() {
        // An Attack Shark V6 and an Inphic IN10R MAX, both at 100%
        assert_eq!(parse(&[3, 0x24, 0x41, 1, 0x64]), Some((0x24, Some(Battery { on_power: false, level: Some(100) }))));
        assert_eq!(parse(&[3, 0x73, 0x41, 1, 0x64]), Some((0x73, Some(Battery { on_power: false, level: Some(100) }))));
        assert_eq!(parse(&[3, 0x55, 0x42, 1, 0x64]), None);
    }

    #[test]
    fn a_level_that_may_be_tens_is_not_shown_for_a_mouse_whose_scale_nobody_has_seen() {
        // All that is known of the V6 is 03 24 41 01 64, which a mouse counting in tens sends too when it links up
        let on_battery = |level| Some(Battery { on_power: false, level });
        assert_eq!(parse(&[3, 0x24, 0x41, 1, 6]), Some((0x24, on_battery(None))));
        assert_eq!(parse(&[3, 0x24, 0x41, 1, 10]), Some((0x24, on_battery(None))));
        assert_eq!(parse(&[3, 0x24, 0x41, 1, 11]), Some((0x24, on_battery(Some(11)))));
        assert_eq!(parse(&[3, 0x24, 0x40, 1, 60]), Some((0x24, on_battery(Some(60)))));
        // The mice whose levels owners have read keep their low ones
        assert_eq!(parse(&[3, 0x55, 0x40, 1, 6]), Some((0x55, on_battery(Some(6)))));
        assert_eq!(parse(&[3, 0x08, 0x40, 1, 10]), Some((0x08, on_battery(Some(10)))));
    }

    #[test]
    fn the_name_is_the_brand_of_the_model_code() {
        let p = XentaReceiverProvider::new(Arc::new(StateStore::empty()));
        let on = |code| Status {
            running: true,
            online: true,
            code: Some(code),
            battery: Some(Battery { on_power: false, level: Some(50) }),
            error: None,
        };
        let name = |pid, s: &Status| p.reading("xenta:test".into(), pid, s, false).name;
        assert_eq!(name(RECEIVER_PID, &on(0xE5)), "Attack Shark");
        assert_eq!(name(RECEIVER_PID, &on(0x20)), "Delux");
        assert_eq!(name(RECEIVER_8K_PID, &on(0x73)), "Inphic");
        assert_eq!(name(RECEIVER_8K_PID, &on(0x08)), "Attack Shark");
        // Before a code has come, and for a code nobody knows
        let quiet = Status { running: true, online: false, code: None, battery: None, error: None };
        assert_eq!(name(RECEIVER_PID, &quiet), "Attack Shark");
        assert_eq!(name(RECEIVER_8K_PID, &quiet), "Mouse");
        assert_eq!(name(RECEIVER_PID, &on(0x99)), "Attack Shark");
    }

    #[test]
    fn the_8k_receiver_is_listened_to_on_its_vendor_collection_when_it_has_no_status_page() {
        let collection = |pid: u16, page: u16, usage: u16| HidInfo {
            path: format!("{pid:04x}:{page:04x}:{usage:04x}"),
            vendor_id: VID,
            product_id: pid,
            usage_page: page,
            usage,
            input_length: 5,
            ..HidInfo::default()
        };
        let paths = |all: &[HidInfo]| status_collections(all).iter().map(|i| i.path.clone()).collect::<Vec<_>>();
        let vendor = collection(RECEIVER_8K_PID, 0xFF00, 0x0001);
        let page = collection(RECEIVER_8K_PID, 0x000A, 0x0000);
        assert_eq!(paths(&[vendor.clone(), collection(RECEIVER_8K_PID, 0xFF01, 0x0002)]), ["fa65:ff00:0001"]);
        assert_eq!(paths(&[vendor, page]), ["fa65:000a:0000"]);
        // The older receiver's vendor collections are for its configuration tool
        assert_eq!(
            paths(&[collection(RECEIVER_PID, 0xFF00, 0x0001), collection(RECEIVER_PID, 0x000A, 0)]),
            ["fa60:000a:0000"]
        );
    }

    #[test]
    fn a_reading_counts_once_two_frames_agree() {
        let mut tracker = FrameTracker::default();
        let mut log = Vec::new();
        let now = Instant::now();
        // Link-up: the false 100% first, then the real level twice
        assert!(tracker.feed(&[3, 0x55, 0x40, 1, 100], now, &mut log), "model code is news");
        assert_eq!(tracker.confirmed, None);
        assert!(!tracker.feed(&[3, 0x55, 0x40, 1, 64], now, &mut log));
        assert!(tracker.feed(&[3, 0x55, 0x40, 1, 64], now, &mut log));
        assert_eq!(tracker.confirmed, Some(Battery { on_power: false, level: Some(64) }));
        assert_eq!(log, vec!["model code 0x55 (seen on the Attack Shark X11)".to_string()]);
    }

    #[test]
    fn silence_means_the_mouse_is_gone() {
        let mut tracker = FrameTracker::default();
        let start = Instant::now();
        tracker.feed(&[3, 0x55, 0x40, 1, 64], start, &mut Vec::new());
        tracker.feed(&[3, 0x55, 0x40, 1, 64], start, &mut Vec::new());
        assert!(!tracker.tick(start + secs(14)));
        assert!(tracker.tick(start + secs(15)));
        assert!(!tracker.online && tracker.confirmed.is_none());
    }

    #[test]
    fn mouse_on_the_cable_takes_over_the_quiet_receiver_slot() {
        let p = XentaReceiverProvider::new(Arc::new(StateStore::empty()));
        let quiet = Status { running: true, online: false, code: None, battery: None, error: None };
        let charging = Status {
            running: true,
            online: true,
            code: Some(0x55),
            battery: Some(Battery { on_power: true, level: None }),
            error: None,
        };
        let readings = p.build(&[(RECEIVER_PID, quiet)], &[(0xFA55, charging)], false);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].id, RECEIVER_ID);
        assert!(readings[0].online && readings[0].charging && readings[0].level.is_none());
    }

    #[test]
    fn mouse_on_the_cable_leaves_a_quiet_receiver_of_the_other_kind_alone() {
        let p = XentaReceiverProvider::new(Arc::new(StateStore::empty()));
        let quiet = Status { running: true, online: false, code: None, battery: None, error: None };
        let charging = |code| Status {
            running: true,
            online: true,
            code: Some(code),
            battery: Some(Battery { on_power: true, level: None }),
            error: None,
        };
        let receivers = [(RECEIVER_8K_PID, quiet.clone()), (RECEIVER_PID, quiet)];
        // An X11 on its cable: the 8K receiver listed first belongs to another mouse, which is asleep
        let readings = p.build(&receivers, &[(0xFA55, charging(0x55))], false);
        assert_eq!(readings.len(), 2);
        assert_eq!((readings[0].id.as_str(), readings[0].online), ("xenta:1d57:fa65", false));
        assert_eq!((readings[1].id.as_str(), readings[1].charging), (RECEIVER_ID, true));
        // An X8 Pro on its cable takes the 8K receiver's slot, and without that receiver gets one of its own
        let readings = p.build(&receivers, &[(0x2126, charging(0x08))], false);
        assert_eq!((readings[0].id.as_str(), readings[0].charging), ("xenta:1d57:fa65", true));
        assert!(!readings[1].online);
        let readings = p.build(&receivers[1..], &[(0x2126, charging(0x08))], false);
        assert_eq!(readings.len(), 2);
        assert_eq!((readings[1].id.as_str(), readings[1].charging), ("xenta:1d57:2126", true));
    }
}
