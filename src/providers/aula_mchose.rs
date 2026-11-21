//! Wireless keyboards on CompX's "BY" 2.4 GHz receivers: 3554:FA09 ("2.4G Wireless Receiver"), which MCHOSE (M HUB)
//! and AULA keyboards, among others, share, and the receivers of the AULA F75 Max (3554:FB40), of MCHOSE's newer
//! boards (41E4:2001, 05AC:024F) and of VGN's (258A:024F, 258A:027F, 258A:027B, 391D:2A01, 391D:2A02). The
//! receiver's vendor collection
//! FF02:0002 takes 20-byte output reports 13 and answers with 20-byte input reports 13, the command echoed in byte 1:
//!   13 4A 01 00 .. 00 5E   battery: reply 13 4A 01 00 xx LL SS ..   LL level in percent, SS high nibble 1 = charging
//!   13 05 01 00 .. 00 19   model code: reply 13 05 01 00 xx C C C C C C ..   six bytes that name an MCHOSE model
//! The last request byte is the sum of the others; MCHOSE firmware leaves the reply's last byte at 0, so it isn't
//! checked. Some firmware sets the top bit of the echoed command. A keyboard that is asleep or off doesn't answer.
//! Only these two reads are sent, the model code once per receiver and only to the receivers M HUB asks it of. The
//! keyboard is asked every 5 minutes, no more: a request may wake a dozing keyboard (Wireless-Device-Battery-Monitor
//! issue 191). Except for FA09, the IDs are ones that unrelated devices carry too (a mouse receiver has FB40, SONiX
//! keyboards 05AC:024F: see `sonix_keyboard`), so those receivers are listed only when they have the 20-byte
//! collection.
//! Protocol facts from myfinder-device-battery (engine/upstream-protocols.md, "MCHOSE BY keyboard receiver
//! 3554:FA09", "VGN beiying keyboard receivers"), HaloBattery (providers/mchose_kb.py) and
//! Wireless-Device-Battery-Monitor (src/drivers/aula.cpp); own implementation. Not yet verified on
//! hardware by this project, which is why the first exchange goes to the log.

use super::{group_by, hex_trimmed as hex, no_link_reason, receiver_name, remaining_ms, secs, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// `name`: what to call the keyboard when neither its model code nor the product string says: a model, a brand (one
/// word) or "" for just a keyboard on a receiver. `model_code`: M HUB asks this receiver which MCHOSE model it has.
/// `shared`: unrelated devices carry these IDs too, so the receiver is listed only when it has the collection.
struct Receiver {
    vid: u16,
    pid: u16,
    name: &'static str,
    model_code: bool,
    shared: bool,
}

const fn shared(vid: u16, pid: u16, name: &'static str, model_code: bool) -> Receiver {
    Receiver { vid, pid, name, model_code, shared: true }
}

static RECEIVERS: &[Receiver] = &[
    Receiver { vid: 0x3554, pid: 0xFA09, name: "", model_code: true, shared: false },
    shared(0x3554, 0xFB40, "", false),
    shared(0x41E4, 0x2001, "MCHOSE", true),
    shared(0x05AC, 0x024F, "", true),
    shared(0x258A, 0x024F, "VGN V98 Pro V3", false),
    shared(0x258A, 0x027F, "VGN V98 Pro V3", false),
    shared(0x258A, 0x027B, "VGN V87 V2", false),
    shared(0x391D, 0x2A01, "VGN N75 V2", false),
    shared(0x391D, 0x2A02, "VGN V108", false),
];

fn receiver(vid: u16, pid: u16) -> Option<&'static Receiver> {
    RECEIVERS.iter().find(|r| r.vid == vid && r.pid == pid)
}

/// What the keyboard is called: by its product string, or by the receiver's row when that names nobody.
fn name(receiver: &Receiver, product: Option<&str>) -> String {
    let fallback = match receiver.name {
        "" => t("name.fa09Keyboard"),
        brand if !brand.contains(' ') => tf("name.brandKeyboard", &[&brand]),
        name => name.to_string(),
    };
    receiver_name::keyboard(product, Some(&fallback))
}

const VENDOR_PAGE: u16 = 0xFF02;
const VENDOR_USAGE: u16 = 0x0002;
const REPORT_ID: u8 = 0x13;
const BATTERY_COMMAND: u8 = 0x4A;
const MODEL_COMMAND: u8 = 0x05;
const FRAME_LENGTH: usize = 20;
const TIMEOUT_MS: u64 = 2500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level: i32,
    pub charging: bool,
}

/// M HUB's keyboardPwds table: the six bytes the model code request returns.
const MODELS: [([u8; 6], &str); 20] = [
    ([0x03, 0x00, 0x00, 0x00, 0x02, 0x1C], "MCHOSE G87"),
    ([0x03, 0x00, 0x00, 0x00, 0x00, 0x77], "MCHOSE K99"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0xDF], "MCHOSE K99 V2"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0x32], "MCHOSE G98"),
    ([0x03, 0x00, 0x00, 0x00, 0x00, 0xD6], "MCHOSE X75"),
    ([0x0A, 0x00, 0x00, 0x00, 0x00, 0x03], "MCHOSE G98 Pro"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0xC6], "MCHOSE X75 V2"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0x15], "MCHOSE G75 Pro"),
    ([0x0A, 0x00, 0x00, 0x00, 0x00, 0x18], "MCHOSE G98 V2"),
    ([0x0A, 0x00, 0x00, 0x00, 0x00, 0x2B], "MCHOSE UT98"),
    ([0x0A, 0x00, 0x00, 0x00, 0x00, 0x01], "MCHOSE Z75"),
    ([0x0A, 0x00, 0x00, 0x00, 0x00, 0x09], "MCHOSE Z75S"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0xC7], "MCHOSE KX75"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0x74], "MCHOSE GX87"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0xED], "MCHOSE GX87 V2"),
    ([0x03, 0x00, 0x00, 0x00, 0x01, 0x35], "MCHOSE K87"),
    ([0x03, 0x00, 0x00, 0x00, 0x03, 0x35], "MCHOSE G75 V2"),
    ([0x0A, 0x00, 0x00, 0x00, 0x00, 0x34], "MCHOSE G87 V2"),
    ([0x03, 0x00, 0x00, 0x00, 0x00, 0x6A], "Redragon Horus Pro"),
    ([0x03, 0x00, 0x00, 0x00, 0x03, 0xDA], "Redragon Horus Pro V2"),
];

#[derive(Default)]
pub struct AulaMchoseProvider {
    /// The model each receiver answered with, by device path; None = it answered with a code not in the table.
    models: HashMap<String, Option<&'static str>>,
    logged: HashSet<String>,
}

impl Provider for AulaMchoseProvider {
    fn name(&self) -> &'static str {
        "AULA / MCHOSE"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        let found = hid::enumerate(|v, p| receiver(v, p).is_some());
        for ((vid, pid), collections) in group_by(found, |i| (i.vendor_id, i.product_id)) {
            let Some(receiver) = receiver(vid, pid) else { continue };
            let vendor = vendor_collection(receiver, &collections);
            if vendor.is_none() && receiver.shared {
                continue;
            }
            let mut trace = Vec::new();
            let mut battery = None;
            let mut error = None;
            match vendor {
                None => error = Some(t(no_link_reason(&collections))),
                Some(v) => {
                    let asked = (|| -> win::Result<()> {
                        let dev = HidDevice::open(v)?;
                        if receiver.model_code && !self.models.contains_key(&v.path) {
                            if let Some(code) = ask(&dev, MODEL_COMMAND, &mut trace)? {
                                self.models.insert(v.path.clone(), model_name(&code));
                            }
                        }
                        if let Some(reply) = ask(&dev, BATTERY_COMMAND, &mut trace)? {
                            battery = parse_battery(&reply);
                        }
                        Ok(())
                    })();
                    if let Err(e) = asked {
                        trace.push(format!("error: {e}"));
                        error = Some(tf("d.error", &[&e]));
                    }
                }
            }
            let model = vendor.and_then(|v| self.models.get(&v.path).copied().flatten());
            let product = vendor
                .and_then(|v| v.product.as_deref())
                .or_else(|| collections.iter().find_map(|i| i.product.as_deref()));
            self.log_once((vid, pid), &collections, vendor, product, &trace, battery.is_some());

            result.push(DeviceReading {
                online: battery.is_some(),
                level: battery.map(|b| b.level),
                charging: battery.is_some_and(|b| b.charging),
                detail: if battery.is_none() { Some(error.unwrap_or_else(|| t("d.asleep"))) } else { None },
                ..DeviceReading::new(
                    format!("compx:{vid:04x}:{pid:04x}"),
                    model.map(str::to_string).unwrap_or_else(|| name(receiver, product)),
                    DeviceKind::Keyboard,
                    t("via.24"),
                )
            });
        }
        result
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        secs(5 * 60)
    }
}

impl AulaMchoseProvider {
    /// Logs the receiver's HID collections and what was sent and received, the first time the keyboard is found
    /// off and the first time it is found on in a session, as the headsets and mice do.
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
            "AULA / MCHOSE {key}: found {}, {} HID collections, product \"{}\"",
            if online { "on" } else { "off" },
            collections.len(),
            product.unwrap_or_default()
        ));
        for info in collections {
            let asked = vendor.is_some_and(|v| v.path == info.path);
            log::write(&format!(
                "  mi={} {info}: {}",
                info.interface_number(),
                if asked { "asked" } else { "not used" }
            ));
        }
        log::write(&format!("  {}", if trace.is_empty() { "nothing sent".into() } else { trace.join("  ") }));
    }
}

/// The collection that takes the requests. A collection that Windows won't open has no usage page to go by, so it is
/// never the one. Under IDs that unrelated devices carry, its reports have to be the frame's 20 bytes exactly: FF02
/// is a page other firmware has too, with reports of 17 or 64 bytes, and a request must not go there.
fn vendor_collection<'a>(receiver: &Receiver, collections: &'a [HidInfo]) -> Option<&'a HidInfo> {
    let fits = |length: usize| if receiver.shared { length == FRAME_LENGTH } else { length >= FRAME_LENGTH };
    collections.iter().find(|i| {
        i.usage_page == VENDOR_PAGE && i.usage == VENDOR_USAGE && fits(i.output_length) && fits(i.input_length)
    })
}

/// Sends one request and waits for its reply; None when the keyboard didn't answer.
fn ask(dev: &HidDevice, command: u8, trace: &mut Vec<String>) -> win::Result<Option<Vec<u8>>> {
    dev.flush();
    let req = request(command);
    trace.push(format!("> {}", hex(&req)));
    if !dev.write(&req, TIMEOUT_MS as u32)? {
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
        if is_reply(&r, command) {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

/// 13 <command> 01 00 .. and the sum of the bytes before it, as M HUB sends it.
pub fn request(command: u8) -> Vec<u8> {
    let mut r = vec![0u8; FRAME_LENGTH];
    r[0] = REPORT_ID;
    r[1] = command;
    r[2] = 0x01;
    let sum: u32 = r[..FRAME_LENGTH - 1].iter().map(|&b| b as u32).sum();
    r[FRAME_LENGTH - 1] = sum as u8;
    r
}

/// The answer to `command`: other reports 13 (status pushes) and a request that comes back as it went out, with
/// nothing after the header, don't count.
pub fn is_reply(r: &[u8], command: u8) -> bool {
    if r.len() < FRAME_LENGTH || r[0] != REPORT_ID || (r[1] & 0x7F) != command {
        return false;
    }
    r[4..FRAME_LENGTH - 1].iter().any(|&b| b != 0)
}

/// What a battery reply says; None for a level of 0 (the receiver answering for a keyboard that's off) or over 100.
pub fn parse_battery(r: &[u8]) -> Option<Battery> {
    if !is_reply(r, BATTERY_COMMAND) {
        return None;
    }
    let level = r[5] as i32;
    (1..=100).contains(&level).then_some(Battery { level, charging: r[6] >> 4 == 1 })
}

/// The MCHOSE model a model code reply names, or None. M HUB reads the code at byte 5; one owner's capture had it a
/// byte earlier, so byte 4 is tried too.
pub fn model_name(r: &[u8]) -> Option<&'static str> {
    if !is_reply(r, MODEL_COMMAND) {
        return None;
    }
    for offset in [5, 4] {
        for (code, name) in &MODELS {
            if r[offset..offset + code.len()] == code[..] {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! Keyboards on the CompX receiver 3554:FA09: 20-byte reports 13. The requests are M HUB's, and the two real
    //! replies are the ones a G75 Pro owner captured (myfinder-device-battery issue 476).
    use super::*;

    fn collection(usage_page: u16, usage: u16, length: usize, access_denied: bool) -> HidInfo {
        HidInfo {
            path: format!(r"\\?\hid#vid_3554&pid_fa09&mi_01&col{usage_page:04x}{length:02}"),
            vendor_id: 0x3554,
            product_id: 0xFA09,
            usage_page,
            usage,
            input_length: length,
            output_length: length,
            access_denied,
            ..HidInfo::default()
        }
    }

    #[test]
    fn vendor_collection_is_the_one_long_enough() {
        let fa09 = receiver(0x3554, 0xFA09).unwrap();
        let all =
            [collection(0x0001, 0x0006, 9, false), collection(0xFF02, 2, 8, false), collection(0xFF02, 2, 20, false)];
        assert_eq!(vendor_collection(fa09, &all).map(|i| i.path.as_str()), Some(all[2].path.as_str()));
        assert!(vendor_collection(fa09, &all[..2]).is_none());
        assert_eq!(no_link_reason(&all[..2]), "d.noLink");
    }

    #[test]
    fn a_shared_id_needs_the_20_byte_collection() {
        // A SONiX keyboard and a CompX mouse receiver with IDs this table has too
        let sonix = [collection(0xFF60, 0x0061, 33, false)];
        let mouse = [collection(0xFF02, 2, 17, false)];
        let apple = receiver(0x05AC, 0x024F).unwrap();
        let fb40 = receiver(0x3554, 0xFB40).unwrap();
        assert!(apple.shared && vendor_collection(apple, &sonix).is_none());
        assert!(fb40.shared && vendor_collection(fb40, &mouse).is_none());
        assert!(!receiver(0x3554, 0xFA09).unwrap().shared);
    }

    #[test]
    fn under_a_shared_id_longer_reports_on_the_same_page_are_someone_elses() {
        // 258A is a chip maker's vendor ID: another device of its with a 64-byte FF02:0002 collection gets nothing
        let other = [collection(0xFF02, 2, 64, false)];
        let by = [collection(0xFF02, 2, 20, false)];
        for r in RECEIVERS.iter().filter(|r| r.shared) {
            assert!(vendor_collection(r, &other).is_none(), "{:04X}:{:04X}", r.vid, r.pid);
            assert!(vendor_collection(r, &by).is_some(), "{:04X}:{:04X}", r.vid, r.pid);
        }
        // FA09 is this receiver's alone and keeps the rule it had
        assert!(vendor_collection(receiver(0x3554, 0xFA09).unwrap(), &other).is_some());
    }

    #[test]
    fn vgn_v98_pro_v3_on_battery_and_on_the_cable() {
        // Captured by an owner on 258A:024F: 99%, then the same with the cable in
        assert_eq!(parse_battery(&h("13 4A 01 00 02 63 00 00 00 00 00 00 00 00 00 00 00 00 00 C3")), b(99, false));
        assert_eq!(parse_battery(&h("13 4A 01 00 02 63 10 00 00 00 00 00 00 00 00 00 00 00 00 D3")), b(99, true));
    }

    #[test]
    fn names_from_the_product_string_or_the_row() {
        let vgn = receiver(0x258A, 0x027B).unwrap();
        assert_eq!(name(vgn, Some("2.4G Wireless Receiver")), "VGN V87 V2");
        assert_eq!(name(receiver(0x41E4, 0x2001).unwrap(), None), "MCHOSE keyboard");
        assert_eq!(name(receiver(0x3554, 0xFA09).unwrap(), Some("2.4G Wireless Receiver")), t("name.fa09Keyboard"));
        assert_eq!(name(receiver(0x3554, 0xFB40).unwrap(), Some("AULA F75 Max")), "AULA F75 Max");
    }

    #[test]
    fn a_locked_collection_is_never_the_vendor_one() {
        let all = [collection(0x0001, 0x0006, 9, false), collection(0, 0, 0, true)];
        assert!(vendor_collection(receiver(0x3554, 0xFA09).unwrap(), &all).is_none());
        assert_eq!(no_link_reason(&all), "d.needsAdmin");
    }

    fn h(text: &str) -> Vec<u8> {
        text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    fn frame(head: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; 20];
        r[..head.len()].copy_from_slice(head);
        r
    }

    fn b(level: i32, charging: bool) -> Option<Battery> {
        Some(Battery { level, charging })
    }

    #[test]
    fn requests_are_m_hubs() {
        assert_eq!(request(0x4A), h("13 4A 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 5E"));
        assert_eq!(request(0x05), h("13 05 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 19"));
    }

    #[test]
    fn captured_battery_reply() {
        // 100%; the low nibble of byte 6 is "full", not charging
        assert_eq!(parse_battery(&h("13 4A 01 00 02 64 01 00 00 00 00 00 00 00 00 00 00 00 00 00")), b(100, false));
    }

    #[test]
    fn captured_model_code_names_the_g75_pro() {
        assert_eq!(
            model_name(&h("13 05 01 00 03 00 00 00 01 15 00 00 00 00 00 00 00 00 00 00")),
            Some("MCHOSE G75 Pro")
        );
    }

    #[test]
    fn model_code_where_m_hub_reads_it() {
        assert_eq!(
            model_name(&frame(&[0x13, 0x05, 0x01, 0x00, 0x06, 0x03, 0x00, 0x00, 0x00, 0x01, 0xDF])),
            Some("MCHOSE K99 V2")
        );
    }

    #[test]
    fn unknown_model_code_is_no_name() {
        assert_eq!(model_name(&frame(&[0x13, 0x05, 0x01, 0x00, 0x06, 0x07, 0x00, 0x00, 0x00, 0x09, 0x99])), None);
    }

    #[test]
    fn charging() {
        // AULA sends 0x10 with the cable in, M HUB's charge flag is the same nibble
        assert_eq!(parse_battery(&frame(&[0x13, 0x4A, 0x01, 0x00, 0x02, 0x39, 0x10])), b(57, true));
    }

    #[test]
    fn echoed_command_with_the_top_bit_set() {
        assert_eq!(parse_battery(&frame(&[0x13, 0xCA, 0x00, 0x00, 0x02, 0x50, 0x01])), b(80, false));
    }

    #[test]
    fn other_reports_are_not_the_reply() {
        assert!(!is_reply(&frame(&[0x13, 0x0A, 0x01, 0x00, 0x04, 0x05, 0x5F, 0x01]), 0x4A)); // status push
        assert!(!is_reply(&request(0x4A), 0x4A)); // the request itself
        assert!(!is_reply(&h("13 4A 01 00 02 64"), 0x4A)); // too short
    }

    #[test]
    fn no_level_is_no_reading() {
        assert_eq!(parse_battery(&frame(&[0x13, 0x4A, 0x01, 0x00, 0x02, 0x00, 0x01])), None);
        assert_eq!(parse_battery(&frame(&[0x13, 0x4A, 0x01, 0x00, 0x02, 0x65, 0x00])), None);
    }
}
