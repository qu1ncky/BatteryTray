//! Wireless keyboards on SONiX 2.4 GHz dongles. On 0C45:FEFE ("2.4G Dongle"): the Attack Shark X85 (issue #21), the
//! EWEADN X87, AJAZZ AK-series boards and others; on 0C45:FDFD the AJAZZ AK820 Pro, on 0C45:FEFC the AULA F87 Pro
//! V2C, and on 05AC:024F the AJAZZ AK820 MAX, the EPOMAKER EA75, AULA boards and others. The dongle has two vendor
//! collections with the usage FF60:0061 (FF80:0001 on FEFC); the one with 33-byte reports (report ID 0 and 32 bytes)
//! takes the battery request, which comes in two forms:
//!   request  20 01 00 .. 00 21      the last byte is the sum of the others
//!   reply    20 01 00 LL ..         LL level in percent
//! is what the vendor's Windows tool for the X85 and the X87 asks every 15 s. It reads nothing but the level, so
//! there is no charging sign. A keyboard that is asleep or off doesn't answer, or the dongle answers for it with a
//! level of 0. Boards with other firmware (the AJAZZ AK820 Max Plus, the AULA F87 Pro V2 and F99 MAX) answer that
//! with 0 or not at all, and give their level in the device information their own driver reads:
//!   request  AA 10 18 00 ..         the first 24 bytes of the information
//!   reply    55 10 .. [25] the mode, 2 = 2.4 GHz, [26] the level in percent (counting the report ID)
//! So the second form is tried when the first gives no level, until one of them has: from then on the keyboard is
//! asked in that form alone, so that a board of the first kind doesn't get a request it doesn't know every time it
//! is asleep.
//! The byte after the level is a charging state in the vendors' drivers, but it didn't change with the cable on the
//! boards it was watched on, so it isn't shown. 05AC:024F is an ID that keyboards with unrelated firmware carry too
//! (see `aula_mchose`), so that dongle is listed only when it has the 33-byte collection on interface 3.
//! Protocol facts from vigil-watt (docs/hardware/eweadn.md: the EWEADN X87's vendor tool read statically and the
//! exchange measured on its 0C45:FEFE dongle; the X87 has the X85's USB IDs and the same vendor tool),
//! sonix-keyboard-studio, Wireless-Device-Battery-Monitor (src/drivers/sonix.cpp, and an AK820 Max Plus owner's
//! capture in its issue 2854), HaloBattery (providers/aula.py) and myfinder-device-battery
//! (engine/upstream-protocols.md: the AULA F87 Pro V2 and F99 MAX). Own implementation. Not yet verified on hardware
//! by this project, which is why the first exchange goes to the log.

use super::{group_by, no_link_reason, receiver_name, remaining_ms, secs, Proven, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::reading::{DeviceKind, DeviceReading};
use crate::state::StateStore;
use crate::win;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BATTERY_COMMAND: u8 = 0x20;
const BATTERY_ARGUMENT: u8 = 0x01;
const INFO_COMMAND: [u8; 2] = [0xAA, 0x10];
const INFO_REPLY: [u8; 2] = [0x55, 0x10];
/// How many bytes of the device information one request asks for.
const INFO_PAGE: u8 = 0x18;
/// Where the level is in the reply to it, report ID first.
const INFO_LEVEL: usize = 26;
/// The report ID, always 0, and 32 bytes.
const REPORT_LENGTH: usize = 33;
const TIMEOUT_MS: u64 = 500;
/// The forms of the request: 0 = "20 01", 1 = the device information.
const REQUEST_FORMS: usize = 2;

/// `page`, `usage`: the collection that takes the request. `shared`: keyboards with unrelated firmware carry these
/// IDs too, so the dongle is listed only when that collection is there, on the interface the SONiX dongles have it
/// on.
struct Dongle {
    vid: u16,
    pid: u16,
    page: u16,
    usage: u16,
    shared: bool,
}

/// Where a SONiX dongle has the collection. FF60:0061 with 32-byte reports is also what every QMK / VIA keyboard
/// has for its configuration tool, on another interface, and such keyboards are given Apple's 05AC:024F too.
const SONIX_INTERFACE: i32 = 3;

static DONGLES: &[Dongle] = &[
    Dongle { vid: 0x0C45, pid: 0xFEFE, page: 0xFF60, usage: 0x0061, shared: false },
    Dongle { vid: 0x0C45, pid: 0xFDFD, page: 0xFF60, usage: 0x0061, shared: false },
    Dongle { vid: 0x0C45, pid: 0xFEFC, page: 0xFF80, usage: 0x0001, shared: false },
    Dongle { vid: 0x05AC, pid: 0x024F, page: 0xFF60, usage: 0x0061, shared: true },
];

fn dongle(vid: u16, pid: u16) -> Option<&'static Dongle> {
    DONGLES.iter().find(|d| d.vid == vid && d.pid == pid)
}

/// The collection that takes the battery request. The other one with the same usage has 65-byte reports.
fn vendor_collection<'a>(dongle: &Dongle, collections: &'a [HidInfo]) -> Option<&'a HidInfo> {
    collections.iter().find(|i| {
        i.usage_page == dongle.page
            && i.usage == dongle.usage
            && i.output_length == REPORT_LENGTH
            && i.input_length == REPORT_LENGTH
            && (!dongle.shared || i.interface_number() == SONIX_INTERFACE)
    })
}

#[derive(Default)]
pub struct SonixKeyboardProvider {
    /// The form of the request each keyboard answered last, by device path: it is the one tried first.
    answered: HashMap<String, usize>,
    logged: HashSet<String>,
    proven: Proven,
}

impl SonixKeyboardProvider {
    /// `state`: the levels this PC has seen, by which a keyboard on a shared ID that is asleep is known.
    pub fn new(state: Arc<StateStore>) -> Self {
        Self { proven: Proven::new(state), ..Self::default() }
    }
}

impl Provider for SonixKeyboardProvider {
    fn name(&self) -> &'static str {
        "SONiX 2.4G"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        let found = hid::enumerate(|v, p| dongle(v, p).is_some());
        for ((vid, pid), collections) in group_by(found, |i| (i.vendor_id, i.product_id)) {
            let Some(dongle) = dongle(vid, pid) else { continue };
            let vendor = vendor_collection(dongle, &collections);
            if vendor.is_none() && dongle.shared {
                continue;
            }
            let mut trace = Vec::new();
            let mut level = None;
            let mut error = None;
            match vendor {
                None => error = Some(t(no_link_reason(&collections))),
                Some(v) => match self.query(v, &mut trace) {
                    Ok(l) => level = l,
                    Err(e) => {
                        trace.push(format!("error: {e}"));
                        error = Some(tf("d.error", &[&e]));
                    }
                },
            }
            let product = vendor
                .and_then(|v| v.product.as_deref())
                .or_else(|| collections.iter().find_map(|i| i.product.as_deref()));
            self.log_once((vid, pid), &collections, vendor, product, &trace, level.is_some());
            let id = format!("sonix:{vid:04x}:{pid:04x}");
            // The collection of a shared ID is one any QMK keyboard has too: until it answers, it may be one
            if dongle.shared && !self.proven.check(&id, level.is_some()) {
                continue;
            }

            result.push(DeviceReading {
                online: level.is_some(),
                level,
                detail: if level.is_none() { Some(error.unwrap_or_else(|| t("d.asleep"))) } else { None },
                ..DeviceReading::new(
                    id,
                    receiver_name::keyboard(product, Some(&t("name.fa09Keyboard"))),
                    DeviceKind::Keyboard,
                    t("via.24"),
                )
            });
        }
        result
    }

    /// The level changes slowly; a keyboard that didn't answer is asked again sooner, to catch it waking up.
    fn delay(&self, last: &[DeviceReading]) -> Duration {
        if last.iter().any(|r| r.online) {
            secs(300)
        } else {
            secs(60)
        }
    }
}

impl SonixKeyboardProvider {
    /// Logs the dongle's HID collections and what was sent and received, the first time the keyboard is found off
    /// and the first time it is found on in a session, as the other keyboards and mice do.
    fn log_once(
        &mut self,
        id: (u16, u16),
        collections: &[HidInfo],
        vendor: Option<&HidInfo>,
        product: Option<&str>,
        trace: &[String],
        online: bool,
    ) {
        let key = format!("{:04x}:{:04x}", id.0, id.1);
        if !self.logged.insert(format!("{key}{online}")) {
            return;
        }
        log::write(&format!(
            "SONiX {key}: found {}, {} HID collections, product \"{}\"",
            if online { "on" } else { "off" },
            collections.len(),
            product.unwrap_or_default()
        ));
        for info in collections {
            let role = if vendor.is_some_and(|v| v.path == info.path) { "asked" } else { "not used" };
            log::write(&format!("  mi={} {info}: {role}", info.interface_number()));
        }
        log::write(&format!("  {}", if trace.is_empty() { "nothing sent".into() } else { trace.join("  ") }));
    }

    /// Asks for the level; None when the keyboard didn't answer or no reply has one.
    fn query(&mut self, vendor: &HidInfo, trace: &mut Vec<String>) -> win::Result<Option<i32>> {
        let dev = HidDevice::open(vendor)?;
        for (i, form) in forms_to_try(self.answered.get(&vendor.path).copied()).into_iter().enumerate() {
            let mut reply = ask(&dev, form, trace)?;
            // The first request can go unanswered while the link wakes up
            if reply.is_none() && i == 0 {
                reply = ask(&dev, form, trace)?;
            }
            if let Some(level) = reply.and_then(|r| parse(form, &r)) {
                self.answered.insert(vendor.path.clone(), form);
                return Ok(Some(level));
            }
        }
        Ok(None)
    }
}

/// The forms of the request to send, in order. A keyboard that has given its level in one form this session is asked
/// in that form alone from then on: the other one is a request its firmware doesn't know, and a keyboard that is
/// asleep would get it once a minute.
fn forms_to_try(answered: Option<usize>) -> Vec<usize> {
    match answered {
        Some(form) => vec![form],
        None => (0..REQUEST_FORMS).collect(),
    }
}

/// Sends the battery request in one of its forms and waits for the reply to it; None when it didn't come.
fn ask(dev: &HidDevice, form: usize, trace: &mut Vec<String>) -> win::Result<Option<Vec<u8>>> {
    dev.flush();
    let request = if form == 0 { request() } else { info_request() };
    trace.push(format!("> {}", hex(&request)));
    if !dev.write(&request, TIMEOUT_MS as u32)? {
        trace.push("(not sent)".into());
        return Ok(None);
    }
    let deadline = Instant::now() + Duration::from_millis(TIMEOUT_MS);
    while Instant::now() < deadline {
        let Some(r) = dev.read(remaining_ms(deadline))? else {
            trace.push("< nothing".into());
            return Ok(None);
        };
        trace.push(format!("< {}", hex(&r)));
        if if form == 0 { is_reply(&r) } else { is_info_reply(&r) } {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

/// The level in a reply to the request of this form.
fn parse(form: usize, r: &[u8]) -> Option<i32> {
    if form == 0 {
        parse_level(r)
    } else {
        parse_info_level(r)
    }
}

/// 00 AA 10 18 00 ..: the first page of the device information, as the vendors' drivers ask for it.
pub fn info_request() -> Vec<u8> {
    let mut r = vec![0u8; REPORT_LENGTH];
    r[1..3].copy_from_slice(&INFO_COMMAND);
    r[3] = INFO_PAGE;
    r
}

/// A reply with device information, report ID first. The byte after the command is the page's length on some
/// firmware and 0 on other, so it isn't looked at.
pub fn is_info_reply(r: &[u8]) -> bool {
    r.len() > INFO_LEVEL && r[1..3] == INFO_REPLY
}

/// The level the device information carries; None for 0 and for anything over 100.
pub fn parse_info_level(r: &[u8]) -> Option<i32> {
    if !is_info_reply(r) {
        return None;
    }
    Some(r[INFO_LEVEL] as i32).filter(|l| (1..=100).contains(l))
}

/// 00 20 01 00 .. and the sum of the bytes before it, as the vendor tool sends it.
pub fn request() -> Vec<u8> {
    let mut r = vec![0u8; REPORT_LENGTH];
    r[1] = BATTERY_COMMAND;
    r[2] = BATTERY_ARGUMENT;
    r[REPORT_LENGTH - 1] = r[..REPORT_LENGTH - 1].iter().fold(0u8, |sum, &b| sum.wrapping_add(b));
    r
}

/// The answer to the battery request, report ID first: the command comes back with a 0 after it.
pub fn is_reply(r: &[u8]) -> bool {
    r.len() >= 5 && r[1] == BATTERY_COMMAND && r[2] == BATTERY_ARGUMENT && r[3] == 0x00
}

/// The level a reply carries; None for 0 (the dongle answering for a keyboard that's off, or the request coming
/// back) and for anything over 100.
pub fn parse_level(r: &[u8]) -> Option<i32> {
    if !is_reply(r) {
        return None;
    }
    Some(r[4] as i32).filter(|l| (1..=100).contains(l))
}

/// A report for the log: without the zeros that pad it. The request shows whole, its checksum being the last byte.
fn hex(report: &[u8]) -> String {
    let used = report.iter().rposition(|&b| b != 0).map_or(1, |i| i + 1);
    super::hex(&report[..used.min(report.len())])
}

#[cfg(test)]
mod tests {
    //! The battery read of keyboards on the SONiX dongle 0C45:FEFE (Attack Shark X85, EWEADN X87 and others): 33-byte
    //! reports, report ID 0 first. The exchange is the one vigil-watt (docs/hardware/eweadn.md) measured on an EWEADN
    //! X87: 00 20 01 00 .. 21 out, 00 20 01 00 64 .. back.
    use super::*;

    /// A 33-byte report starting with the given bytes, zero padded.
    fn report(head: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; 33];
        r[..head.len()].copy_from_slice(head);
        r
    }

    #[test]
    fn request_is_the_captured_one() {
        let mut expected = report(&[0x00, 0x20, 0x01]);
        expected[32] = 0x21;
        assert_eq!(request(), expected);
    }

    #[test]
    fn eweadn_x87_at_full() {
        let mut r = report(&[0x00, 0x20, 0x01, 0x00, 0x64]);
        r[32] = 0x85;
        assert_eq!(parse_level(&r), Some(100));
    }

    #[test]
    fn level_in_percent() {
        assert_eq!(parse_level(&report(&[0x00, 0x20, 0x01, 0x00, 0x49])), Some(73));
        assert_eq!(parse_level(&report(&[0x00, 0x20, 0x01, 0x00, 0x01])), Some(1));
    }

    #[test]
    fn zero_is_a_keyboard_that_is_off() {
        assert_eq!(parse_level(&report(&[0x00, 0x20, 0x01, 0x00, 0x00])), None);
    }

    #[test]
    fn the_request_coming_back_is_no_reading() {
        assert_eq!(parse_level(&request()), None);
    }

    #[test]
    fn level_over_100_is_no_reading() {
        assert_eq!(parse_level(&report(&[0x00, 0x20, 0x01, 0x00, 0x65])), None);
        assert_eq!(parse_level(&report(&[0x00, 0x20, 0x01, 0x00, 0xFF])), None);
    }

    #[test]
    fn other_reports_are_not_the_reply() {
        assert!(!is_reply(&report(&[0x00, 0x55, 0x10, 0x14, 0x00, 0x00, 0x01, 0x00, 0x00, 0x40])));
        assert!(!is_reply(&report(&[0x00, 0x20, 0x02, 0x00, 0x64])));
        assert!(!is_reply(&report(&[0x00, 0x20, 0x01, 0x01, 0x64])));
        assert_eq!(parse_level(&report(&[0x00, 0x0F, 0x01, 0x01, 0x11])), None);
    }

    /// A reply as the owners' captures print it, without the report ID that Windows puts in front.
    fn captured(text: &str) -> Vec<u8> {
        let mut r = vec![0u8];
        r.extend(text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()));
        r
    }

    #[test]
    fn info_request_is_the_captured_one() {
        assert_eq!(info_request(), report(&[0x00, 0xAA, 0x10, 0x18]));
    }

    #[test]
    fn ajazz_ak820_max_plus_at_99() {
        let r =
            captured("55 10 18 00 00 00 00 00 00 00 00 92 45 0C 44 80 49 01 00 00 66 01 04 10 02 63 00 00 05 00 00 00");
        assert_eq!(r.len(), 33);
        assert_eq!(parse_info_level(&r), Some(99));
        assert_eq!(parse_level(&r), None);
    }

    #[test]
    fn aula_f87_pro_v2_at_48() {
        let r =
            captured("55 10 18 00 00 00 00 00 00 00 00 12 A6 38 11 27 66 01 00 00 66 01 00 00 02 30 00 00 00 00 00 00");
        assert_eq!(parse_info_level(&r), Some(48));
    }

    #[test]
    fn device_information_without_a_level_is_no_reading() {
        let mut r = report(&[0x00, 0x55, 0x10, 0x18]);
        assert_eq!(parse_info_level(&r), None);
        r[26] = 101;
        assert_eq!(parse_info_level(&r), None);
        // The second page, the "20 01" reply and the request coming back are not the first page's reply
        assert_eq!(parse_info_level(&report(&[0x00, 0x20, 0x01, 0x00, 0x64])), None);
        assert_eq!(parse_info_level(&info_request()), None);
        assert!(!is_info_reply(&[0x00, 0x55, 0x10, 0x18]));
    }

    #[test]
    fn the_collection_with_33_byte_reports_takes_the_request() {
        let on = |mi: u8, page: u16, usage: u16, length: usize| HidInfo {
            path: format!(r"\\?\hid#vid_05ac&pid_024f&mi_{mi:02x}#{page:04x}:{usage:04x}:{length}"),
            usage_page: page,
            usage,
            input_length: length,
            output_length: length,
            ..HidInfo::default()
        };
        let collection = |page: u16, usage: u16, length: usize| on(3, page, usage, length);
        let all = [collection(0xFF60, 0x0061, 65), collection(0xFF60, 0x0061, 33), collection(0xFF02, 0x0002, 20)];
        let sonix = dongle(0x05AC, 0x024F).unwrap();
        assert_eq!(vendor_collection(sonix, &all).map(|i| i.path.as_str()), Some(all[1].path.as_str()));
        // A keyboard of another kind with the same IDs has no such collection, and isn't listed
        assert!(sonix.shared && vendor_collection(sonix, &all[2..]).is_none());
        assert!(vendor_collection(dongle(0x0C45, 0xFEFC).unwrap(), &all).is_none());
        assert!(!dongle(0x0C45, 0xFEFE).unwrap().shared);
    }

    #[test]
    fn a_qmk_keyboard_with_apples_ids_is_not_a_sonix_dongle() {
        // The configuration collection of QMK / VIA firmware: the same usage and lengths, on interface 1
        let raw_hid = HidInfo {
            path: r"\\?\hid#vid_05ac&pid_024f&mi_01#8&1&0&0000#{4d1e55b2}".into(),
            usage_page: 0xFF60,
            usage: 0x0061,
            input_length: 33,
            output_length: 33,
            ..HidInfo::default()
        };
        assert!(vendor_collection(dongle(0x05AC, 0x024F).unwrap(), std::slice::from_ref(&raw_hid)).is_none());
        // The IDs that are SONiX's own are taken wherever the collection is, as before
        assert!(vendor_collection(dongle(0x0C45, 0xFEFE).unwrap(), std::slice::from_ref(&raw_hid)).is_some());
    }

    #[test]
    fn a_keyboard_that_answered_one_form_is_asked_in_it_alone() {
        assert_eq!(forms_to_try(None), [0, 1]);
        assert_eq!(forms_to_try(Some(0)), [0]);
        assert_eq!(forms_to_try(Some(1)), [1]);
    }

    #[test]
    fn a_short_report_is_not_the_reply() {
        assert!(!is_reply(&[0x00, 0x20, 0x01, 0x00]));
        assert!(!is_reply(&[]));
    }

    #[test]
    fn the_whole_request_goes_to_the_log() {
        assert!(hex(&request()).ends_with(" 21"));
        assert_eq!(hex(&report(&[0x00, 0x20, 0x01, 0x00, 0x64])), "00 20 01 00 64");
        assert_eq!(hex(&report(&[])), "00");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn keyboard_name_from_the_dongle() {
        let fallback = t("name.fa09Keyboard");
        assert_eq!(receiver_name::keyboard(Some("2.4G Dongle"), Some(&fallback)), "Keyboard on a 2.4G receiver");
        assert_eq!(receiver_name::keyboard(Some("Neo Ergo 2.4G"), None), "Neo Ergo");
        assert_eq!(receiver_name::keyboard(Some("AK820MAX 2.4G"), None), "AK820MAX keyboard");
        assert_eq!(receiver_name::keyboard(Some("AJAZZ AK820MAX"), None), "AJAZZ AK820MAX");
        assert_eq!(receiver_name::keyboard(None, None), "Keyboard");
    }
}
