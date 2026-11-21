//! Mice whose configuration tools speak one OEM command frame: VXE R1 mice (ATK HUB) and mice on Areson receivers
//! (the OemDrv.exe that Redragon, Fantech and others ship). Both directions are
//! 17-byte frames:
//!   [0] report ID  [1] command  [2] status, 0 = ok  [3..4] address  [5] data length  [6..15] data  [16] checksum,
//! the checksum making all 17 bytes add up to 0x55. The battery is command 04 (GetPower), and it is all this sends:
//!   request  08 04 00 .. 00 49
//!   reply    RR 04 00 00 00 LL PP CC ..   PP level, CC charging (VXE also sends the voltage after it)
//! The level comes in percent, or on some Areson receivers in tens of percent.
//! A mouse that is asleep or off doesn't answer through its receiver. The frame travels two ways:
//!   VXE: output report 08 on the vendor collection FF02:0002, the reply as input report 08 on the same collection.
//!   Areson: feature report 08 on the FF02 collection, the reply as input report 09 on the FF01 collection of the
//!   same interface (the configuration tool writes and reads on two handles).
//! Protocol facts from OpenMouse (mouse-protocol: src/drivers/atk/hid.ts), vxe-battery-tray (PROTOCOL.md),
//! redragon-mouse-linux (PROTOCOL.md) and Wireless-Device-Battery-Monitor (docs/areson.md). The Ardor Renegade on
//! 25A7:FA4A answers in tens: its owner captured the exchange while Ardor's software showed 70% for a level byte of
//! 07 (issue #18). Own implementation. Not yet verified on hardware by this project, which is why the first exchange
//! goes to the log.

use super::{group_by, hex_trimmed as hex, no_link_reason, one_per_id, remaining_ms, secs, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const REQUEST_ID: u8 = 0x08;
const POWER_COMMAND: u8 = 0x04;
const FRAME_LENGTH: usize = 17;
const COMMAND_PAGE: u16 = 0xFF02;
const ARESON_REPLY_PAGE: u16 = 0xFF01;
const TIMEOUT_MS: u64 = 800;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    Output,
    Feature,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level: i32,
    pub charging: bool,
}

/// `name`: a text key or a ready name. `step`: what one unit of the level byte is worth, in percent: 10 when the
/// level comes in tens.
struct Model {
    key: &'static str,
    name: &'static str,
    transport: Transport,
    wireless: bool,
    step: i32,
}

static MODELS: &[((u16, u16), Model)] = &[
    // 3554 is CompX's vendor ID, which Pulsar, VGN, Lamzu and others use too: only these products are claimed
    (
        (0x3554, 0xF58A),
        Model { key: "vxe:r1promax", name: "VXE R1 PRO MAX", transport: Transport::Output, wireless: true, step: 1 },
    ),
    (
        (0x3554, 0xF58C),
        Model { key: "vxe:r1promax", name: "VXE R1 PRO MAX", transport: Transport::Output, wireless: false, step: 1 },
    ),
    (
        (0x373B, 0x1085),
        Model { key: "vxe:r1se", name: "VXE R1 SE / SE+", transport: Transport::Output, wireless: true, step: 1 },
    ),
    (
        (0x3554, 0xF58E),
        Model { key: "vxe:r1se", name: "VXE R1 SE / SE+", transport: Transport::Output, wireless: true, step: 1 },
    ),
    (
        (0x3554, 0xF58F),
        Model { key: "vxe:r1se", name: "VXE R1 SE / SE+", transport: Transport::Output, wireless: false, step: 1 },
    ),
    // 373B is ATK's own vendor ID. This receiver calls itself "Wireless mouse 8k NANO dongle-L" (issue #30)
    (
        (0x373B, 0x11FE),
        Model {
            key: "atk:a9-ultra-max-2",
            name: "ATK A9 Ultra Max 2.0",
            transport: Transport::Output,
            wireless: true,
            step: 1,
        },
    ),
    // Sold under many names (Redragon M693, Fantech Helios Go XD5, ...), so none is shown
    (
        (0x25A7, 0xFA7C),
        Model { key: "areson:fa7c", name: "name.aresonMouse", transport: Transport::Feature, wireless: true, step: 1 },
    ),
    (
        (0x25A7, 0xFA7B),
        Model { key: "areson:fa7c", name: "name.aresonMouse", transport: Transport::Feature, wireless: false, step: 1 },
    ),
    // The Ardor Renegade's receiver, which sends the level in tens. So do the older FA07 / FA08 receivers, which
    // aren't claimed.
    (
        (0x25A7, 0xFA4A),
        Model { key: "areson:fa4a", name: "name.aresonMouse", transport: Transport::Feature, wireless: true, step: 10 },
    ),
];

fn model(vid: u16, pid: u16) -> Option<&'static Model> {
    MODELS.iter().find(|(k, _)| *k == (vid, pid)).map(|(_, m)| m)
}

#[derive(Default)]
pub struct OemMouseProvider {
    logged: HashSet<String>,
}

impl Provider for OemMouseProvider {
    fn name(&self) -> &'static str {
        "VXE / Areson"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        let found = hid::enumerate(|v, p| model(v, p).is_some());
        for ((vid, pid), collections) in group_by(found, |i| (i.vendor_id, i.product_id)) {
            let Some(model) = model(vid, pid) else { continue };
            let (send, listen) = channels(model.transport, &collections);
            let mut trace = Vec::new();
            let mut battery = None;
            let mut error = None;
            match (send, listen) {
                (Some(s), Some(l)) => match query(model, s, l, &mut trace) {
                    Ok(b) => battery = b,
                    Err(e) => {
                        trace.push(format!("error: {e}"));
                        error = Some(tf("d.error", &[&e]));
                    }
                },
                _ => error = Some(t(no_link_reason(&collections))),
            }
            self.log_once((vid, pid), &collections, send, listen, &trace, battery.is_some());

            result.push(DeviceReading {
                online: battery.is_some(),
                level: battery.map(|b| b.level),
                charging: battery.is_some_and(|b| b.charging),
                detail: if battery.is_none() { Some(error.unwrap_or_else(|| t("d.asleep"))) } else { None },
                ..DeviceReading::new(
                    model.key,
                    t(model.name),
                    DeviceKind::Mouse,
                    t(if model.wireless { "via.24" } else { "via.cable" }),
                )
            });
        }
        // A mouse charging on its cable while its receiver stays plugged in is listed twice under one Id, and the
        // receiver finds it off: keep the reading that reached the mouse
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

impl OemMouseProvider {
    /// Logs the mouse's HID collections and what was sent and received, the first time it is found off and the
    /// first time it is found on in a session, as the headsets and SteelSeries mice do.
    fn log_once(
        &mut self,
        id: (u16, u16),
        collections: &[HidInfo],
        send: Option<&HidInfo>,
        listen: Option<&HidInfo>,
        trace: &[String],
        online: bool,
    ) {
        let key = format!("{:04x}:{:04x}", id.0, id.1);
        if !self.logged.insert(format!("{key}{online}")) {
            return;
        }
        log::write(&format!(
            "VXE / Areson {key}: found {}, {} HID collections",
            if online { "on" } else { "off" },
            collections.len()
        ));
        for info in collections {
            let is_send = send.is_some_and(|s| s.path == info.path);
            let is_listen = listen.is_some_and(|l| l.path == info.path);
            let role = match (is_send, is_listen) {
                (true, true) => "request and reply",
                (true, false) => "request",
                (false, true) => "reply",
                _ => "not used",
            };
            log::write(&format!("  mi={} {info}: {role}", info.interface_number()));
        }
        log::write(&format!("  {}", if trace.is_empty() { "nothing sent".into() } else { trace.join("  ") }));
    }
}

/// The collection the request goes to and the one the reply comes back on. A collection that Windows won't open has
/// no usage page to go by, so it is never one of them.
fn channels(transport: Transport, collections: &[HidInfo]) -> (Option<&HidInfo>, Option<&HidInfo>) {
    if transport == Transport::Output {
        // Some ATK mice have a second FF02 collection with longer reports of another ID, which doesn't take the frame
        let takes = |i: &HidInfo, exact: bool| {
            let fits = |length: usize| if exact { length == FRAME_LENGTH } else { length >= FRAME_LENGTH };
            i.usage_page == COMMAND_PAGE && fits(i.output_length) && fits(i.input_length)
        };
        let vendor =
            collections.iter().find(|i| takes(i, true)).or_else(|| collections.iter().find(|i| takes(i, false)));
        return (vendor, vendor);
    }
    let send = collections.iter().find(|i| i.usage_page == COMMAND_PAGE && i.feature_length >= FRAME_LENGTH);
    let listen = send.and_then(|s| {
        collections.iter().find(|i| {
            i.usage_page == ARESON_REPLY_PAGE
                && i.interface_number() == s.interface_number()
                && i.input_length >= FRAME_LENGTH
        })
    });
    (send, listen)
}

/// Asks for the battery; None when the mouse didn't answer or the reply says nothing about it.
fn query(model: &Model, send: &HidInfo, listen: &HidInfo, trace: &mut Vec<String>) -> win::Result<Option<Battery>> {
    let request = frame(REQUEST_ID, POWER_COMMAND);
    if model.transport == Transport::Output {
        let dev = HidDevice::open(send)?;
        dev.flush();
        trace.push(format!("> {}", hex(&request)));
        if !dev.write(&request, TIMEOUT_MS as u32)? {
            trace.push("(not sent)".into());
            return Ok(None);
        }
        return receive(&dev, model.step, trace);
    }

    let reader = HidDevice::open_read_only(listen)?;
    reader.flush();
    let writer = HidDevice::open_sync(send).or_else(|_| HidDevice::open_features_only(send))?;
    let mut feature = vec![0u8; send.feature_length.max(FRAME_LENGTH)];
    feature[..FRAME_LENGTH].copy_from_slice(&request);
    trace.push(format!("feature > {}", hex(&feature)));
    if !writer.set_feature(&feature) {
        trace.push("(not taken)".into());
        return Ok(None);
    }
    receive(&reader, model.step, trace)
}

/// Waits for the reply to GetPower; other reports (DPI and profile button events) are skipped.
fn receive(dev: &HidDevice, step: i32, trace: &mut Vec<String>) -> win::Result<Option<Battery>> {
    let deadline = Instant::now() + Duration::from_millis(TIMEOUT_MS);
    while Instant::now() < deadline {
        let Some(r) = dev.read(remaining_ms(deadline))? else {
            trace.push("< nothing".into());
            return Ok(None);
        };
        trace.push(format!("< {}", hex(&r)));
        if is_power_reply(&r) {
            return Ok(parse(&r, step));
        }
    }
    Ok(None)
}

/// A request frame: report ID, command, zeros and the checksum.
pub fn frame(report_id: u8, command: u8) -> Vec<u8> {
    let mut f = vec![0u8; FRAME_LENGTH];
    f[0] = report_id;
    f[1] = command;
    f[FRAME_LENGTH - 1] = checksum(&f);
    f
}

fn checksum(frame: &[u8]) -> u8 {
    let sum: u32 = frame[..FRAME_LENGTH - 1].iter().map(|&b| b as u32).sum();
    0x55u8.wrapping_sub(sum as u8)
}

/// A whole frame answering GetPower, with a checksum that adds up.
pub fn is_power_reply(r: &[u8]) -> bool {
    r.len() >= FRAME_LENGTH && r[1] == POWER_COMMAND && r[FRAME_LENGTH - 1] == checksum(r)
}

/// What a GetPower reply says, report ID first; None when the receiver refused or has no level. `step` is what one
/// unit of the level byte is worth, in percent. 0% on battery is no reading either: a mouse that empty has switched
/// itself off, so it's the receiver answering for it.
pub fn parse(r: &[u8], step: i32) -> Option<Battery> {
    if !is_power_reply(r) || r[2] != 0x00 || r[5] < 1 {
        return None;
    }
    let level = r[6] as i32 * step;
    let charging = r[5] >= 2 && r[7] != 0;
    if level > 100 || (level == 0 && !charging) {
        return None;
    }
    Some(Battery { level, charging })
}

#[cfg(test)]
mod tests {
    //! GetPower in the OEM frame of VXE (ATK) and Areson mice: 17 bytes, report ID first, adding up to 0x55. The
    //! captured frames are the ones vxe-battery-tray (PROTOCOL.md) and OpenMouse (src/drivers/atk/hid.test.ts) publish.
    use super::*;

    fn h(text: &str) -> Vec<u8> {
        text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// A reply with the given head, zero padded, and the checksum that makes it add up to 0x55.
    fn reply(head: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; 17];
        r[..head.len()].copy_from_slice(head);
        let sum: u32 = r[..16].iter().map(|&b| b as u32).sum();
        r[16] = 0x55u8.wrapping_sub(sum as u8);
        r
    }

    fn b(level: i32, charging: bool) -> Option<Battery> {
        Some(Battery { level, charging })
    }

    #[test]
    fn request_is_the_captured_one() {
        assert_eq!(frame(0x08, 0x04), h("08 04 00 00 00 00 00 00 00 00 00 00 00 00 00 00 49"));
    }

    #[test]
    fn vxe_r1_se_plus_on_battery() {
        // 95%, not charging, 4096 mV
        assert_eq!(parse(&h("08 04 00 00 00 02 5F 00 10 00 00 00 00 00 00 00 D8"), 1), b(95, false));
    }

    #[test]
    fn atk_mouse_through_its_dongle() {
        assert_eq!(parse(&h("08 04 00 00 00 02 64 00 10 8E 00 00 00 00 00 00 45"), 1), b(100, false));
    }

    #[test]
    fn atk_a9_ultra_max_2_on_its_nano_dongle() {
        // Posted by an owner (vigil-watt issue 71): 50% on battery, 50% charging, full on the cable
        assert_eq!(parse(&h("08 04 00 00 00 02 32 00 0F 9B 00 00 00 00 00 00 6B"), 1), b(50, false));
        assert_eq!(parse(&h("08 04 00 00 00 02 32 01 0F 96 00 00 00 00 00 00 6F"), 1), b(50, true));
        assert_eq!(parse(&h("08 04 00 00 00 02 64 01 11 2A 00 00 00 00 00 00 A7"), 1), b(100, true));
        let dongle = model(0x373B, 0x11FE).unwrap();
        assert!(dongle.wireless && dongle.transport == Transport::Output && dongle.step == 1);
    }

    #[test]
    fn the_collection_with_17_byte_reports_is_preferred() {
        let all = [collection(1, 0xFF02, 19, 19, 0, false), collection(2, 0xFF02, 17, 17, 0, false)];
        let vendor = Some(all[1].path.as_str());
        assert_eq!(paths(channels(Transport::Output, &all)), (vendor, vendor));
    }

    #[test]
    fn areson_reply_comes_as_report_09() {
        assert_eq!(parse(&reply(&[0x09, 0x04, 0x00, 0x00, 0x00, 0x02, 0x4B, 0x01]), 1), b(75, true));
    }

    #[test]
    fn level_without_charging_byte() {
        assert_eq!(parse(&reply(&[0x08, 0x04, 0x00, 0x00, 0x00, 0x01, 0x2A, 0x01]), 1), b(42, false));
    }

    #[test]
    fn wrong_checksum_is_not_the_reply() {
        let r = h("08 04 00 00 00 02 5F 00 10 00 00 00 00 00 00 00 D9");
        assert!(!is_power_reply(&r));
        assert_eq!(parse(&r, 1), None);
    }

    #[test]
    fn button_event_is_not_the_reply() {
        assert!(!is_power_reply(&reply(&[0x09, 0x0A, 0x00, 0x00, 0x00, 0x01, 0x01])));
    }

    #[test]
    fn refused_or_empty_is_no_reading() {
        assert_eq!(parse(&reply(&[0x08, 0x04, 0x01]), 1), None);
        assert_eq!(parse(&reply(&[0x08, 0x04, 0x00, 0x00, 0x00, 0x00]), 1), None);
        assert_eq!(parse(&[0x08, 0x04, 0x00, 0x00, 0x00, 0x02, 0x5F], 1), None);
    }

    #[test]
    fn empty_on_battery_is_the_receiver_answering_for_an_off_mouse() {
        assert_eq!(parse(&reply(&[0x08, 0x04, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00]), 1), None);
        assert_eq!(parse(&reply(&[0x08, 0x04, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01]), 1), b(0, true));
    }

    #[test]
    fn level_over_a_hundred_is_no_reading() {
        assert_eq!(parse(&reply(&[0x08, 0x04, 0x00, 0x00, 0x00, 0x02, 0x65, 0x00]), 1), None);
    }

    #[test]
    fn ardor_renegade_answers_in_tens() {
        // Captured by the mouse's owner on the 25A7:FA4A receiver while Ardor's software showed 70% (issue #18)
        let r = h("09 04 00 00 00 02 07 00 00 00 00 00 00 00 00 00 3F");
        assert_eq!(parse(&r, 10), b(70, false));
        assert_eq!(parse(&r, 1), b(7, false));
    }

    #[test]
    fn tens_run_from_zero_to_ten() {
        assert_eq!(parse(&reply(&[0x09, 0x04, 0x00, 0x00, 0x00, 0x02, 0x0A, 0x00]), 10), b(100, false));
        assert_eq!(parse(&reply(&[0x09, 0x04, 0x00, 0x00, 0x00, 0x02, 0x01, 0x01]), 10), b(10, true));
        assert_eq!(parse(&reply(&[0x09, 0x04, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01]), 10), b(0, true));
        assert_eq!(parse(&reply(&[0x09, 0x04, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00]), 10), None);
    }

    #[test]
    fn level_in_percent_is_no_reading_in_tens() {
        assert_eq!(parse(&reply(&[0x09, 0x04, 0x00, 0x00, 0x00, 0x02, 0x0B, 0x00]), 10), None);
        assert_eq!(parse(&reply(&[0x09, 0x04, 0x00, 0x00, 0x00, 0x02, 0x46, 0x00]), 10), None);
    }

    /// A collection as the enumeration returns it; `page` 0 with `denied` is one that Windows won't open.
    fn collection(mi: u8, page: u16, input: usize, output: usize, feature: usize, denied: bool) -> HidInfo {
        HidInfo {
            path: format!(r"\\?\hid#vid_25a7&pid_fa7c&mi_{mi:02x}&col{page:04x}#8&1&0&0000#{{4d1e55b2}}"),
            vendor_id: 0x25A7,
            product_id: 0xFA7C,
            usage_page: page,
            input_length: input,
            output_length: output,
            feature_length: feature,
            access_denied: denied,
            ..HidInfo::default()
        }
    }

    fn paths<'a>(pair: (Option<&'a HidInfo>, Option<&'a HidInfo>)) -> (Option<&'a str>, Option<&'a str>) {
        (pair.0.map(|i| i.path.as_str()), pair.1.map(|i| i.path.as_str()))
    }

    #[test]
    fn vxe_asks_and_listens_on_one_collection() {
        let all = [collection(0, 0x0001, 8, 0, 0, false), collection(1, 0xFF02, 17, 17, 0, false)];
        let vendor = Some(all[1].path.as_str());
        assert_eq!(paths(channels(Transport::Output, &all)), (vendor, vendor));
    }

    #[test]
    fn areson_asks_on_one_collection_and_listens_on_another() {
        let all = [
            collection(0, 0x0001, 8, 0, 0, false),
            collection(1, 0xFF01, 17, 0, 0, false),
            collection(1, 0xFF02, 0, 0, 17, false),
        ];
        assert_eq!(paths(channels(Transport::Feature, &all)), (Some(all[2].path.as_str()), Some(all[1].path.as_str())));
    }

    #[test]
    fn a_locked_collection_is_never_picked() {
        let all = [collection(0, 0x0001, 8, 0, 0, false), collection(1, 0, 0, 0, 0, true)];
        assert_eq!(paths(channels(Transport::Output, &all)), (None, None));
        assert_eq!(paths(channels(Transport::Feature, &all)), (None, None));
        assert_eq!(no_link_reason(&all), "d.needsAdmin");
    }

    #[test]
    fn areson_with_its_reply_collection_locked() {
        let all = [collection(1, 0xFF02, 0, 0, 17, false), collection(1, 0, 0, 0, 0, true)];
        assert_eq!(paths(channels(Transport::Feature, &all)), (Some(all[0].path.as_str()), None));
        assert_eq!(no_link_reason(&all), "d.needsAdmin");
    }

    #[test]
    fn areson_reply_collection_of_another_interface_is_not_the_one() {
        let all = [collection(1, 0xFF02, 0, 0, 17, false), collection(2, 0xFF01, 17, 0, 0, false)];
        assert_eq!(paths(channels(Transport::Feature, &all)), (Some(all[0].path.as_str()), None));
        assert_eq!(no_link_reason(&all), "d.noLink");
    }

    #[test]
    fn nothing_locked_is_no_link() {
        assert_eq!(no_link_reason(&[collection(0, 0x0001, 8, 0, 0, false)]), "d.noLink");
        assert_eq!(no_link_reason(&[]), "d.noLink");
    }

    #[test]
    fn fa4a_reads_in_tens_fa7c_in_percent() {
        assert_eq!(model(0x25A7, 0xFA4A).map(|m| m.step), Some(10));
        assert_eq!(model(0x25A7, 0xFA7C).map(|m| m.step), Some(1));
    }
}
