//! Mice whose configuration tools speak one OEM command frame, CompX's: VXE and ATK mice (ATK HUB), Pulsar, Lamzu,
//! Scyrox, Kysona, G-Wolves, Hitscan, Corsair's Sabre v2 Pro and others, and mice on Areson receivers (the OemDrv.exe
//! that Redragon, Fantech and others ship). Both directions are 17-byte frames:
//!   [0] report ID  [1] command  [2] status, 0 = ok  [3..4] address  [5] data length  [6..15] data  [16] checksum,
//! the checksum making all 17 bytes add up to 0x55. The battery is command 04 (GetPower):
//!   request  08 04 00 .. 00 49
//!   reply    RR 04 00 00 00 LL PP CC ..   PP level, CC charging (most also send the voltage after it)
//! The level comes in percent, or on some Areson receivers in tens of percent. Command 03 asks a receiver whether
//! its mouse is there: the reply's first data byte is 1 when it is.
//! A mouse that is asleep or off doesn't answer through most receivers. Some receivers go quiet themselves until
//! they are asked 03, and a few keep answering 04 with the last level of a mouse that is gone, so the newer rows ask
//! 03 as well (see `Link`). The frame travels two ways:
//!   Output: output report 08 on the vendor collection FF02:0002, the reply as input report 08 on the same
//!   collection.
//!   Feature: feature report 08 on the FF02 collection, the reply as input report 09 on the FF01 collection of the
//!   same interface (the configuration tool writes and reads on two handles).
//! Many receivers are shared by several models, and by brands: those are named by the receiver's product string.
//! Protocol facts from OpenMouse (mouse-protocol: src/drivers/atk, scyrox, lamzu-atlantis, vgn, gwolves, dareu and
//! their testing notes), the Linux driver hid-pulsar (commands 03 and 04, the Pulsar and Kysona IDs),
//! vxe-battery-tray (PROTOCOL.md), redragon-mouse-linux (PROTOCOL.md), xm2we-linux (PROTOCOL.md), HaloBattery
//! (providers/pulsar.py, compx.py, endgame_we.py, dareu.py), OpenLinkHub (the Corsair Sabre v2 Pro) and
//! Wireless-Device-Battery-Monitor (docs/areson.md, src/drivers/kysona.cpp, dareu.cpp, and the readings owners posted
//! in its issues). The Ardor Renegade on 25A7:FA4A answers in tens: its owner captured the exchange while Ardor's
//! software showed 70% for a level byte of 07 (issue #18). Own implementation. Not yet verified on hardware by this
//! project, which is why the first exchange goes to the log.

use super::{
    contains_ignore_case, group_by, hex_trimmed as hex, no_link_reason, one_per_id, receiver_name, remaining_ms, secs,
    Provider,
};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const REQUEST_ID: u8 = 0x08;
const LINK_COMMAND: u8 = 0x03;
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

/// Whether the receiver is asked if its mouse is there (command 03).
/// `Ask`: before the level, as the Linux driver does; a receiver that says "not there" has no level to give, and one
/// that doesn't answer the question is asked for the level anyway. It also wakes the receivers that stay quiet until
/// then. `Must`: the receiver answers with the last level of a mouse that is off, so the level counts only when it
/// says the mouse is there, or the mouse is charging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    No,
    Ask,
    Must,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level: i32,
    pub charging: bool,
}

/// `name`: a text key or a ready name. `step`: what one unit of the level byte is worth, in percent: 10 when the
/// level comes in tens. `by_product`: several models carry these IDs, so the name comes from the product string and
/// `name` is only what to call it when that string names nobody: a model, a brand (one word) or "" for just a mouse.
/// With a brand, the product string counts only when it carries that brand: the others name the receiver.
struct Model {
    key: &'static str,
    name: &'static str,
    transport: Transport,
    wireless: bool,
    step: i32,
    link: Link,
    by_product: bool,
}

impl Model {
    const fn by_product(self) -> Model {
        Model { by_product: true, ..self }
    }

    const fn link(self, link: Link) -> Model {
        Model { link, ..self }
    }
}

/// The rows this provider started with: only the level is asked.
const fn vxe(key: &'static str, name: &'static str, wireless: bool) -> Model {
    Model { key, name, transport: Transport::Output, wireless, step: 1, link: Link::No, by_product: false }
}

/// A mouse on the output transport; its receiver is asked for the link first.
const fn compx(key: &'static str, name: &'static str, wireless: bool) -> Model {
    vxe(key, name, wireless).link(if wireless { Link::Ask } else { Link::No })
}

const fn areson(key: &'static str, name: &'static str, wireless: bool, step: i32) -> Model {
    Model { transport: Transport::Feature, step, ..vxe(key, name, wireless) }
}

const R: bool = true; // on its receiver
const C: bool = false; // on its cable

static MODELS: &[((u16, u16), Model)] = &[
    // 3554 is CompX's vendor ID, which Pulsar, VGN, Lamzu and others use too: only these products are claimed.
    // F58A is the receiver of the R1 PRO as well, F58C the cable of both
    ((0x3554, 0xF58A), vxe("vxe:r1promax", "VXE R1 PRO / PRO MAX", R)),
    ((0x3554, 0xF58C), vxe("vxe:r1promax", "VXE R1 PRO / PRO MAX", C)),
    ((0x373B, 0x1272), vxe("vxe:r1promax", "VXE R1 PRO / PRO MAX", C)),
    ((0x373B, 0x1085), vxe("vxe:r1se", "VXE R1 SE / SE+", R)),
    ((0x3554, 0xF58E), vxe("vxe:r1se", "VXE R1 SE / SE+", R)),
    ((0x3554, 0xF58F), vxe("vxe:r1se", "VXE R1 SE / SE+", C)),
    // The R1's receivers call themselves after their radio ("VXE NordicMouse 1K Dongle"), so the string isn't asked
    ((0x3554, 0xF505), compx("vxe:r1-4k", "VGN / VXE Dragonfly R1", R)),
    ((0x3554, 0xF506), compx("vxe:r1-4k", "VGN / VXE Dragonfly R1", C)),
    ((0x3554, 0xFB56), compx("vgn:f2", "VGN Dragonfly F2", R)),
    ((0x3554, 0xFB57), compx("vgn:f2", "VGN Dragonfly F2", C)),
    ((0x3554, 0xF503), compx("compx:f503", "VGN", R).by_product()),
    // ATK's own vendor ID. Its receivers serve the whole range, so the product string is all there is to go by
    ((0x373B, 0x101A), compx("atk:101a", "ATK", R).by_product()),
    ((0x373B, 0x101B), compx("atk:101b", "ATK", R).by_product()),
    ((0x373B, 0x10C9), compx("atk:10c9", "ATK", R).by_product()),
    ((0x373B, 0x1278), compx("atk:1278", "ATK", R).by_product()),
    ((0x373B, 0x1040), compx("atk:1040", "ATK", R).by_product()),
    // "Wireless mouse 8k dongle-L": read with an A9 Ultimate behind it and with an F1 Ultimate 2.0
    ((0x373B, 0x11D9), compx("atk:11d9", "ATK", R).by_product()),
    ((0x373B, 0x11B6), compx("atk:a9-ultimate", "ATK A9 Ultimate", C)),
    // "Wireless mouse 8k NANO dongle-L": the product string names nobody, two owners name this mouse (issue #30).
    // Asked for the level alone, as in the release that added it
    ((0x373B, 0x11FE), vxe("atk:a9-ultra-max-2", "ATK A9 Ultra Max 2.0", R)),
    ((0x373B, 0x1017), compx("atk:x1-pro-max", "ATK X1 Pro Max", C)),
    ((0x373B, 0x1115), compx("atk:a9-plus", "ATK A9 Plus", C)),
    ((0x373B, 0x11E4), compx("atk:f1-ultimate-2", "ATK F1 Ultimate 2.0", C)),
    ((0x373B, 0x128E), compx("atk:a9-mini-plus", "ATK A9 Mini+", C)),
    // Six Lamzu models of the Atlantis generation share these IDs and one product string
    ((0x3554, 0xF50D), compx("lamzu:atlantis", "Lamzu Atlantis / Thorn", R)),
    ((0x3554, 0xF510), compx("lamzu:atlantis", "Lamzu Atlantis / Thorn", R)),
    ((0x3554, 0xF50F), compx("lamzu:atlantis", "Lamzu Atlantis / Thorn", C)),
    ((0x3554, 0xF517), compx("scyrox:v8s", "Scyrox V8S", R)),
    ((0x3554, 0xF515), compx("scyrox:v8s", "Scyrox V8S", C)),
    ((0x3554, 0xF5F7), compx("scyrox:v8", "Scyrox V8 / V6", R)),
    ((0x3554, 0xF5F6), compx("scyrox:v8", "Scyrox V8 / V6", C)),
    ((0x3554, 0xF508), compx("pulsar:x2v2", "Pulsar X2 V2", R).by_product()),
    ((0x3554, 0xF509), compx("pulsar:x2v2", "Pulsar X2 V2", R).by_product()),
    ((0x3554, 0xF507), compx("pulsar:x2v2", "Pulsar X2 V2", C).by_product()),
    // Pulsar's own vendor ID has three unrelated protocols; this receiver of the CrazyLight mice speaks this one
    ((0x3710, 0x5406), compx("pulsar:5406", "Pulsar", R).by_product()),
    ((0x3554, 0xF57C), compx("kysona:m600", "Kysona M600", R)),
    ((0x3554, 0xF57D), compx("kysona:m600", "Kysona M600", C)),
    // Redragon, Taidu and Kysona mice share the first receiver; the second one and the cable ("3-mode mouse") are
    // those of several Redragon models
    ((0x3554, 0xF5D5), compx("compx:f5d5", "", R).by_product()),
    ((0x3554, 0xF55D), compx("compx:f55d", "Redragon", R).by_product()),
    ((0x3554, 0xF55E), compx("compx:f55e", "Redragon", C).by_product()),
    ((0x3554, 0xF512), compx("compx:f512", "Chimera V2", R).by_product()),
    // "Compx 2.4G Wireless 8K", read on a Zaopin Z2 Mini: silent until it is asked for the link
    ((0x3554, 0xF527), compx("compx:f527", "", R).by_product()),
    ((0x3554, 0xF524), compx("compx:f524", "", R).by_product()),
    ((0x3554, 0xF5DF), compx("cougar:revenger-pro", "Cougar Revenger Pro", R)),
    ((0x3554, 0xF5DC), compx("cougar:revenger-pro", "Cougar Revenger Pro", R)),
    ((0x3554, 0xF5DE), compx("cougar:revenger-pro", "Cougar Revenger Pro", C)),
    ((0x3554, 0xF54B), compx("gravastar:f54b", "GravaStar", R).by_product()),
    ((0x3554, 0xF575), compx("gravastar:f575", "GravaStar", R).by_product()),
    ((0x3554, 0xF548), compx("gravastar:f548", "GravaStar", R).by_product()),
    ((0x3554, 0xF577), compx("gravastar:f577", "GravaStar", R).by_product()),
    ((0x33E4, 0x3854), compx("gwolves:3854", "G-Wolves", R).by_product()),
    ((0x33E4, 0x5618), compx("gwolves:htx-ultra", "G-Wolves HTX Ultra", C)),
    ((0x260D, 0x1114), compx("dareu:1114", "Dareu", R).by_product()),
    ((0x260D, 0x1154), compx("dareu:a950-wing", "Dareu A950 Wing", R)),
    ((0x260D, 0x1196), compx("dareu:a950-wing", "Dareu A950 Wing", C)),
    ((0x35BB, 0xD3FB), compx("hecate:g5m-pro", "HECATE G5M PRO", R)),
    ((0x046A, 0x0330), compx("cherry:xtrfy-1k", "CHERRY XTRFY", R).by_product()),
    ((0x3770, 0x0200), compx("hitscan:hyperlight", "Hitscan Hyperlight", R)),
    ((0x3770, 0x0300), compx("hitscan:hyperlight", "Hitscan Hyperlight", R)),
    ((0x3770, 0x0100), compx("hitscan:hyperlight", "Hitscan Hyperlight", C)),
    ((0x1B1C, 0x2B2A), compx("corsair:sabre-v2-pro", "Corsair Sabre v2 Pro Ultralight Wireless", R)),
    ((0x1B1C, 0x2B28), compx("corsair:sabre-v2-pro", "Corsair Sabre v2 Pro Ultralight Wireless", C)),
    ((0x1B1C, 0x2B34), compx("corsair:sabre-v2-pro-mg", "Corsair Sabre v2 Pro Wireless MG", R)),
    ((0x1B1C, 0x2B29), compx("corsair:sabre-v2-pro-mg", "Corsair Sabre v2 Pro Wireless MG", C)),
    ((0x1B1C, 0x2B35), compx("corsair:sabre-v2-pro-cf", "Corsair Sabre v2 Pro Wireless CF", R)),
    ((0x1B1C, 0x2B25), compx("corsair:sabre-v2-pro-cf", "Corsair Sabre v2 Pro Wireless CF", C)),
    // Sold under many names (Redragon M693, Fantech Helios Go XD5, ...), so none is shown
    ((0x25A7, 0xFA7C), areson("areson:fa7c", "name.aresonMouse", R, 1)),
    ((0x25A7, 0xFA7B), areson("areson:fa7c", "name.aresonMouse", C, 1)),
    // The Ardor Renegade's receiver, which sends the level in tens. So do the older FA07 / FA08 receivers of the
    // Redragon M915RGB-WL and M913
    ((0x25A7, 0xFA4A), areson("areson:fa4a", "name.aresonMouse", R, 10)),
    ((0x25A7, 0xFA07), areson("areson:fa07", "name.aresonMouse", R, 10)),
    ((0x25A7, 0xFA08), areson("areson:fa07", "name.aresonMouse", R, 10)),
    // This receiver may answer with what it last heard from a mouse that is off. It was read with an EM901X behind
    // it and with an A950
    ((0x260D, 0x1074), areson("dareu:em901x", "Dareu EM901X / A950", R, 1).link(Link::Ask)),
    ((0x260D, 0x1090), areson("dareu:em901x", "Dareu EM901X / A950", C, 1)),
    // And this one always does
    ((0x3367, 0x1961), areson("endgame:xm2we", "Endgame Gear XM2we / OP1we", R, 1).link(Link::Must)),
    ((0x3367, 0x1960), areson("endgame:xm2we", "Endgame Gear XM2we / OP1we", C, 1)),
];

fn model(vid: u16, pid: u16) -> Option<&'static Model> {
    MODELS.iter().find(|(k, _)| *k == (vid, pid)).map(|(_, m)| m)
}

/// What the mouse is called: its table name, or for IDs several models carry, what the product string says.
fn name(model: &Model, product: Option<&str>) -> String {
    if !model.by_product {
        return t(model.name);
    }
    match model.name {
        "" => receiver_name::mouse(product, None),
        // A name of one word is a brand: "ATK mouse". A string without it ("8K dongle-L", "MAD 8K DONGLE", "3-mode
        // mouse") says what the receiver is, not whose mouse it has
        brand if !brand.contains(' ') => {
            let fallback = tf("name.brandMouse", &[&brand]);
            if contains_ignore_case(&receiver_name::brand(product), brand) {
                receiver_name::mouse(product, Some(&fallback))
            } else {
                fallback
            }
        }
        name => receiver_name::mouse(product, Some(name)),
    }
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
            let product = product(send, &collections);
            self.log_once((vid, pid), &collections, send, listen, product, &trace, battery.is_some());

            result.push(DeviceReading {
                online: battery.is_some(),
                level: battery.map(|b| b.level),
                charging: battery.is_some_and(|b| b.charging),
                detail: if battery.is_none() { Some(error.unwrap_or_else(|| t("d.asleep"))) } else { None },
                ..DeviceReading::new(
                    model.key,
                    name(model, product),
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

/// The device's product string, from the collection that is asked when it has one.
fn product<'a>(send: Option<&'a HidInfo>, collections: &'a [HidInfo]) -> Option<&'a str> {
    send.and_then(|s| s.product.as_deref()).or_else(|| collections.iter().find_map(|i| i.product.as_deref()))
}

impl OemMouseProvider {
    /// Logs the mouse's HID collections and what was sent and received, the first time it is found off and the
    /// first time it is found on in a session, as the headsets and SteelSeries mice do.
    #[allow(clippy::too_many_arguments)]
    fn log_once(
        &mut self,
        id: (u16, u16),
        collections: &[HidInfo],
        send: Option<&HidInfo>,
        listen: Option<&HidInfo>,
        product: Option<&str>,
        trace: &[String],
        online: bool,
    ) {
        let key = format!("{:04x}:{:04x}", id.0, id.1);
        if !self.logged.insert(format!("{key}{online}")) {
            return;
        }
        log::write(&format!(
            "VXE / Areson {key}: found {}, {} HID collections, product \"{}\"",
            if online { "on" } else { "off" },
            collections.len(),
            product.unwrap_or_default()
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
        // Some mice have a second FF02 collection with longer reports of another ID, which doesn't take the frame
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

/// The handles a mouse is asked through: one for both directions, or one to write the feature report to and one to
/// read the reply from.
struct Channel {
    transport: Transport,
    writer: HidDevice,
    reader: Option<HidDevice>,
}

impl Channel {
    fn open(transport: Transport, send: &HidInfo, listen: &HidInfo) -> win::Result<Self> {
        if transport == Transport::Output {
            return Ok(Self { transport, writer: HidDevice::open(send)?, reader: None });
        }
        let reader = HidDevice::open_read_only(listen)?;
        let writer = HidDevice::open_sync(send).or_else(|_| HidDevice::open_features_only(send))?;
        Ok(Self { transport, writer, reader: Some(reader) })
    }

    /// Sends a command with no data and returns its reply; None when it wasn't taken or nothing answered.
    fn ask(&self, command: u8, trace: &mut Vec<String>) -> win::Result<Option<Vec<u8>>> {
        let request = frame(REQUEST_ID, command);
        let reader = self.reader.as_ref().unwrap_or(&self.writer);
        reader.flush();
        if self.transport == Transport::Output {
            trace.push(format!("> {}", hex(&request)));
            if !self.writer.write(&request, TIMEOUT_MS as u32)? {
                trace.push("(not sent)".into());
                return Ok(None);
            }
        } else {
            let mut feature = vec![0u8; self.writer.info.feature_length.max(FRAME_LENGTH)];
            feature[..FRAME_LENGTH].copy_from_slice(&request);
            trace.push(format!("feature > {}", hex(&feature)));
            if !self.writer.set_feature(&feature) {
                trace.push("(not taken)".into());
                return Ok(None);
            }
        }
        receive(reader, command, trace)
    }
}

/// Asks for the battery; None when the mouse didn't answer, isn't there or the reply says nothing about it.
fn query(model: &Model, send: &HidInfo, listen: &HidInfo, trace: &mut Vec<String>) -> win::Result<Option<Battery>> {
    let channel = Channel::open(model.transport, send, listen)?;
    if model.link == Link::Ask && channel.ask(LINK_COMMAND, trace)?.and_then(|r| linked(&r)) == Some(false) {
        return Ok(None);
    }
    let Some(battery) = channel.ask(POWER_COMMAND, trace)?.and_then(|r| parse(&r, model.step)) else {
        return Ok(None);
    };
    // Asked after the level: the link drops for a moment when the charger goes in, while the level stays good
    if model.link == Link::Must
        && !battery.charging
        && channel.ask(LINK_COMMAND, trace)?.and_then(|r| linked(&r)) != Some(true)
    {
        return Ok(None);
    }
    Ok(Some(battery))
}

/// Waits for the reply to a command; other reports (DPI and profile button events, command 0A) are skipped.
fn receive(dev: &HidDevice, command: u8, trace: &mut Vec<String>) -> win::Result<Option<Vec<u8>>> {
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

/// A whole frame answering `command`, with a checksum that adds up.
fn is_reply(r: &[u8], command: u8) -> bool {
    r.len() >= FRAME_LENGTH && r[1] == command && r[FRAME_LENGTH - 1] == checksum(r)
}

/// A whole frame answering GetPower, with a checksum that adds up.
pub fn is_power_reply(r: &[u8]) -> bool {
    is_reply(r, POWER_COMMAND)
}

/// What the reply to command 03 says: whether the receiver has its mouse. None when it refused or says nothing.
pub fn linked(r: &[u8]) -> Option<bool> {
    (is_reply(r, LINK_COMMAND) && r[2] == 0x00 && r[5] >= 1).then(|| r[6] != 0)
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

    #[test]
    fn link_request_is_the_captured_one() {
        assert_eq!(frame(0x08, 0x03), h("08 03 00 00 00 00 00 00 00 00 00 00 00 00 00 00 4A"));
    }

    #[test]
    fn receiver_says_whether_its_mouse_is_there() {
        assert_eq!(linked(&reply(&[0x08, 0x03, 0x00, 0x00, 0x00, 0x01, 0x01])), Some(true));
        assert_eq!(linked(&reply(&[0x08, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00])), Some(false));
        assert_eq!(linked(&reply(&[0x09, 0x03, 0x00, 0x00, 0x00, 0x01, 0x01])), Some(true));
    }

    #[test]
    fn a_refused_or_other_reply_says_nothing_about_the_link() {
        assert_eq!(linked(&reply(&[0x08, 0x03, 0x01])), None);
        assert_eq!(linked(&reply(&[0x08, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01])), None);
        assert_eq!(linked(&reply(&[0x08, 0x04, 0x00, 0x00, 0x00, 0x02, 0x5F])), None);
        assert_eq!(linked(&h("08 03 00 00 00 01 01 00 00 00 00 00 00 00 00 00 00")), None);
    }

    #[test]
    fn atk_x1_pro_max_on_its_dongle_and_on_its_cable() {
        // Captured by OpenMouse: 25% on battery through the 8K dongle, 45% charging on the cable
        assert_eq!(parse(&h("08 04 00 00 00 02 19 00 0E 46 00 00 00 00 00 00 DA"), 1), b(25, false));
        assert_eq!(parse(&h("08 04 00 00 00 02 2D 01 0F 06 00 00 00 00 00 00 04"), 1), b(45, true));
        assert_eq!(parse(&h("08 04 00 00 00 02 64 01 10 8E 00 00 00 00 00 00 44"), 1), b(100, true));
    }

    #[test]
    fn atk_a9_ultra_max_2_on_its_nano_dongle() {
        // Posted by an owner (vigil-watt issue 71): 50% on battery, 50% charging, full on the cable
        assert_eq!(parse(&h("08 04 00 00 00 02 32 00 0F 9B 00 00 00 00 00 00 6B"), 1), b(50, false));
        assert_eq!(parse(&h("08 04 00 00 00 02 32 01 0F 96 00 00 00 00 00 00 6F"), 1), b(50, true));
        assert_eq!(parse(&h("08 04 00 00 00 02 64 01 11 2A 00 00 00 00 00 00 A7"), 1), b(100, true));
        let dongle = model(0x373B, 0x11FE).unwrap();
        assert_eq!(name(dongle, Some("Wireless mouse 8k NANO dongle-L")), "ATK A9 Ultra Max 2.0");
        assert!(dongle.wireless && dongle.transport == Transport::Output && dongle.step == 1);
    }

    #[test]
    fn cherry_xtrfy_and_g_wolves_receivers() {
        // Posted by owners: 95% at 4084 mV, 80% at 4013 mV
        assert_eq!(parse(&h("08 04 00 00 00 02 5F 00 0F F4 00 00 00 00 00 00 E5"), 1), b(95, false));
        assert_eq!(parse(&h("08 04 00 00 00 02 50 00 0F AD 00 00 00 00 00 00 3B"), 1), b(80, false));
    }

    #[test]
    fn redragon_m915_on_fa07_answers_in_tens() {
        // Captured while the vendor's program showed 80%
        let r = h("09 04 00 00 00 02 08 00 00 00 00 00 00 00 00 00 3E");
        assert_eq!(parse(&r, model(0x25A7, 0xFA07).unwrap().step), b(80, false));
    }

    #[test]
    fn dareu_answers_in_percent() {
        let r = h("09 04 00 00 00 02 5A 00 00 00 00 00 00 00 00 00 EC");
        assert_eq!(parse(&r, model(0x260D, 0x1074).unwrap().step), b(90, false));
    }

    #[test]
    fn no_id_is_listed_twice_and_one_key_is_one_name() {
        for (i, (id, m)) in MODELS.iter().enumerate() {
            assert!(MODELS[..i].iter().all(|(other, _)| other != id), "{:04X}:{:04X} is listed twice", id.0, id.1);
            assert!(MODELS.iter().all(|(_, o)| o.key != m.key || o.name == m.name), "{}: two names", m.key);
        }
    }

    #[test]
    fn receivers_ask_for_the_link_and_cables_do_not() {
        assert_eq!(model(0x3554, 0xF527).unwrap().link, Link::Ask);
        assert_eq!(model(0x3554, 0xF515).unwrap().link, Link::No);
        assert_eq!(model(0x3367, 0x1961).unwrap().link, Link::Must);
        // The rows that were here first are asked as before
        assert_eq!(model(0x3554, 0xF58A).unwrap().link, Link::No);
        assert_eq!(model(0x373B, 0x11FE).unwrap().link, Link::No);
        assert_eq!(model(0x25A7, 0xFA7C).unwrap().link, Link::No);
    }

    #[test]
    fn a_shared_receiver_is_named_by_its_product_string() {
        let atk = model(0x373B, 0x101B).unwrap();
        assert_eq!(name(atk, Some("ATK Mouse 8K Dongle")), "ATK mouse");
        assert_eq!(name(atk, Some("2.4G Wireless Receiver")), "ATK mouse");
        assert_eq!(name(model(0x3554, 0xF512).unwrap(), Some("2.4G Wireless Receiver")), "Chimera V2");
        assert_eq!(name(model(0x3554, 0xF5D5).unwrap(), None), "Mouse");
        assert_eq!(name(model(0x3554, 0xF517).unwrap(), Some("Scyrox Turbo 8k dongle")), "Scyrox V8S");
        assert_eq!(name(model(0x3554, 0xF503).unwrap(), Some("VGN Mouse")), "VGN mouse");
        assert_eq!(name(model(0x3554, 0xF524).unwrap(), Some("2.4G Wireless")), "Mouse");
    }

    #[test]
    fn a_product_string_without_the_brand_names_the_receiver_not_the_mouse() {
        // The strings these receivers and this cable were seen with
        assert_eq!(name(model(0x373B, 0x1278).unwrap(), Some("8K dongle-L")), "ATK mouse");
        assert_eq!(name(model(0x373B, 0x11D9).unwrap(), Some("Wireless mouse 8k dongle-L")), "ATK mouse");
        assert_eq!(name(model(0x373B, 0x1040).unwrap(), Some("MAD 8K DONGLE")), "ATK mouse");
        assert_eq!(name(model(0x373B, 0x10C9).unwrap(), Some("NK mouse NANO dongle")), "ATK mouse");
        assert_eq!(name(model(0x3554, 0xF55E).unwrap(), Some("3-mode mouse")), "Redragon mouse");
        assert_eq!(name(model(0x3554, 0xF55D).unwrap(), Some("2.4G Wireless Receiver")), "Redragon mouse");
        // The Dragonfly R1's receivers are named after their radio
        assert_eq!(name(model(0x3554, 0xF505).unwrap(), Some("VXE NordicMouse 1K Dongle")), "VGN / VXE Dragonfly R1");
    }

    #[test]
    fn a_receiver_read_with_two_models_is_named_after_neither() {
        // 373B:11D9 with an A9 Ultimate and with an F1 Ultimate 2.0, 260D:1074 with an EM901X and with an A950
        let dongle = model(0x373B, 0x11D9).unwrap();
        assert!(dongle.by_product && dongle.key != model(0x373B, 0x11B6).unwrap().key);
        assert_eq!(name(model(0x260D, 0x1074).unwrap(), None), "Dareu EM901X / A950");
    }

    #[test]
    fn the_collection_with_17_byte_reports_is_preferred() {
        // An ATK A9 Plus on its cable has a second FF02 collection with 19-byte reports of another ID
        let all = [collection(1, 0xFF02, 19, 19, 0, false), collection(2, 0xFF02, 17, 17, 0, false)];
        let vendor = Some(all[1].path.as_str());
        assert_eq!(paths(channels(Transport::Output, &all)), (vendor, vendor));
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
