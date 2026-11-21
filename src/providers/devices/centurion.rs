//! Logitech headsets that speak "Centurion" instead of HID++ - G PRO X 2 LIGHTSPEED, G522 LIGHTSPEED, G PRO X 3
//! LIGHTSPEED, ASTRO A20 X - and the ASTRO A50 Gen 4 base station, which still carries Astro's own vendor ID.
//!
//! Centurion, on the vendor collection FFA0:0001, 64-byte reports both ways:
//! - A frame is `prefix, length, flags, payload`: the prefix is report 51 on the PRO X 2 and report 50 with the
//!   address 23 on the later models, the length counts the flags byte and the payload, the flags are 00 for a
//!   message that fits one frame.
//! - A request to the receiver is `feature index, function | software ID, parameters`, and the reply echoes the
//!   first two bytes. An error is `FF, feature index, function | software ID, code`.
//! - The headset is reached through the receiver's bridge feature (0x0003): `bridge, 10 | software ID, size (2
//!   bytes), 00, feature index, function | software ID, parameters`. The receiver acknowledges with `bridge,
//!   10 | software ID`, and the headset's answer comes as `bridge, 10, size, 00, feature index, function | software
//!   ID, parameters` - or with FF and then the feature index when the headset refuses.
//! - Root (index 0) function 0 tells where a feature is: `index, type, version`, index 0 = no such feature. The
//!   battery feature 0x0104, function 0, answers `percent, a second level that is not used here, charge state`
//!   (0 on battery, 1 and 2 charging, 3 charged).
//! - A headset that is off: the receiver refuses the bridge request (`FF, bridge, ...`), or acknowledges it and
//!   nothing follows, or reports `bridge, 00, 00, 00` (the headset left).
//!
//! ASTRO A50 Gen 4 base station, on the vendor collection FF32:0074, report 02, 64 bytes: the request is `02,
//! command`, the reply `02, status, length, data` with status 02 = data follows, 00 = taken, 01 = error. One reply
//! per request and nothing unasked; the reply doesn't say which command it answers. Command 54 answers the
//! headset's state (bit 0 on the dock, bit 1 radio link), 7C the battery (bit 7 charging, the rest percent). The
//! base station keeps answering with the last level after the headset went off, hence the look at 54 first.
//!
//! The A50 Gen 3 base station (9886:0015) is left out on purpose. Its level is known (it refuses 7C and has the
//! battery in the answer to `72 01`), but not how it tells that the headset is off: the one study of Astro's own
//! software for it found no link state, and 54 was only seen answering with the headset on the dock. Without that,
//! a level the base station remembers would be shown for a headset that is off.
//!
//! Facts from HeadsetControl (github.com/Sapd/HeadsetControl: lib/devices/protocols/logitech_centurion_protocol.hpp,
//! logitech_gpro_x2_lightspeed.hpp, logitech_g522_lightspeed.hpp, logitech_gpro_x3_lightspeed.hpp,
//! logitech_astro_a50_gen4.hpp, the captured frames in tests/test_protocols.cpp, pull request 641 for the A20 X and
//! issue 539 for a PRO X 2 owner's captures), Solaar (github.com/pwr-Solaar/Solaar: lib/logitech_receiver/base.py,
//! device.py, centurion.py), eh-fifty (github.com/tdryer/eh-fifty) and AstroBar (github.com/nichtlegacy/AstroBar)
//! for the A50 Gen 4; what is said of the A50 Gen 3 is from OpenAstroCC (github.com/BenStein1/OpenAstroCC,
//! docs/GEN3-IMPLEMENTATION-GUIDE.md) and Wireless-Device-Battery-Monitor
//! (github.com/krisgfx9/Wireless-Device-Battery-Monitor, src/drivers/astro.cpp and pull request 2329). Own
//! implementation. Read-only: only functions that read are sent. Not yet verified on hardware by this project.

use super::{off, on, Io, Spec, State};
use crate::win;

pub static SPECS: &[Spec] = &[
    Spec {
        key: "logitech-g-pro-x-2",
        name: "Logitech G PRO X 2 LIGHTSPEED",
        vid: 0x046D,
        pids: &[0x0AF7],
        page: 0xFFA0,
        usage: 0x0001,
        query: |io| behind_receiver(io, PRO_X_2),
        ..Spec::HEADSET
    },
    Spec {
        key: "logitech-g522",
        name: "Logitech G522 LIGHTSPEED",
        vid: 0x046D,
        pids: &[0x0B18],
        page: 0xFFA0,
        usage: 0x0001,
        query: |io| behind_receiver(io, ADDRESSED),
        ..Spec::HEADSET
    },
    Spec {
        key: "logitech-g-pro-x-3",
        name: "Logitech G PRO X 3 LIGHTSPEED",
        vid: 0x046D,
        pids: &[0x0B45],
        page: 0xFFA0,
        usage: 0x0001,
        query: |io| behind_receiver(io, ADDRESSED),
        ..Spec::HEADSET
    },
    // The transmitter and the headset on its cable share a key: the reading that reaches the headset wins (see poll)
    Spec {
        key: "astro-a20-x",
        name: "ASTRO A20 X",
        vid: 0x046D,
        pids: &[0x0B35, 0x0B2E],
        cable_pids: &[0x0B2E],
        page: 0xFFA0,
        usage: 0x0001,
        query: |io| {
            if io.product_id() == 0x0B2E {
                on_its_cable(io, ADDRESSED)
            } else {
                behind_receiver(io, ADDRESSED)
            }
        },
        ..Spec::HEADSET
    },
    Spec {
        key: "astro-a50-gen-4",
        name: "ASTRO A50 Gen 4",
        vid: 0x9886,
        pids: &[0x002C],
        page: 0xFF32,
        usage: 0x0074,
        query: astro_a50_gen4,
        ..Spec::HEADSET
    },
];

// ---- Centurion ----

/// What starts every frame: report 51 on the PRO X 2; report 50 and the address 23 on the models after it. A
/// receiver ignores a frame with another address.
const PRO_X_2: &[u8] = &[0x51];
const ADDRESSED: &[u8] = &[0x50, 0x23];

/// Echoed in every reply, which tells ours from those to other programs on the same collection: HeadsetControl
/// uses 1, Solaar B, and G HUB was captured with A and C.
const SOFTWARE_ID: u8 = 0x06;

/// Feature IDs, and the index every device has Root at.
const ROOT: u8 = 0x00;
const BRIDGE: u16 = 0x0003;
const BATTERY: u16 = 0x0104;

/// Function 0 of Root: where is the feature with this ID. Function 0 of the battery feature: the level.
const GET_FEATURE: u8 = 0x00;
const GET_LEVEL: u8 = 0x00;
/// Function 1 of the bridge: pass this message to the headset. The headset's answer comes back as event 1.
const SEND: u8 = 0x10;

/// G HUB polls the same collection, so other frames may come before ours.
const READS: usize = 16;

/// What a frame is to the request that was sent.
#[derive(Debug, PartialEq, Eq)]
enum Reply {
    /// The answer: the parameters after the echoed feature index and function.
    Data(Vec<u8>),
    /// Whoever was asked answered with an error.
    Refused,
    /// The receiver won't pass the request on or says the headset left; also what silence counts as.
    Gone,
    /// An acknowledgement, an event, a reply to another program: not the end of this exchange.
    Other,
}

fn frame(prefix: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut frame = prefix.to_vec();
    frame.push(payload.len() as u8 + 1);
    frame.push(0x00);
    frame.extend_from_slice(payload);
    frame
}

/// The payload of a frame that carries a whole message; a piece of a longer one (flags not 00) is nothing we
/// asked for.
fn payload<'a>(prefix: &[u8], report: &'a [u8]) -> Option<&'a [u8]> {
    match report.strip_prefix(prefix)? {
        [length, 0x00, rest @ ..] if *length >= 2 => rest.get(..*length as usize - 1),
        _ => None,
    }
}

fn direct_request(prefix: &[u8], feature: u8, function: u8, params: &[u8]) -> Vec<u8> {
    let mut message = vec![feature, function | SOFTWARE_ID];
    message.extend_from_slice(params);
    frame(prefix, &message)
}

fn bridged_request(prefix: &[u8], bridge: u8, feature: u8, function: u8, params: &[u8]) -> Vec<u8> {
    let size = params.len() as u8 + 3;
    let mut message = vec![bridge, SEND | SOFTWARE_ID, 0x00, size, 0x00, feature, function | SOFTWARE_ID];
    message.extend_from_slice(params);
    frame(prefix, &message)
}

fn direct_reply(payload: &[u8], feature: u8, function: u8) -> Reply {
    let echo = function | SOFTWARE_ID;
    match payload {
        [0xFF, f, e, ..] if *f == feature && *e == echo => Reply::Refused,
        [f, e, params @ ..] if *f == feature && *e == echo => Reply::Data(params.to_vec()),
        _ => Reply::Other,
    }
}

fn bridged_reply(payload: &[u8], bridge: u8, feature: u8, function: u8) -> Reply {
    let echo = function | SOFTWARE_ID;
    match payload {
        // The receiver refuses to pass it on: a PRO X 2 receiver without its headset does
        [0xFF, b, e, ..] if *b == bridge && *e == (SEND | SOFTWARE_ID) => Reply::Gone,
        // The bridge's own event: bridge, 00, 00, connected
        [b, 0x00, 0x00, 0x00, ..] if *b == bridge => Reply::Gone,
        // A message from the headset (device 0 behind the bridge), of the size the two bytes say
        [b, SEND, 0x00, size, rest @ ..] if *b == bridge => match rest.get(..*size as usize) {
            Some([0x00, 0xFF, f, e, ..]) if *f == feature && *e == echo => Reply::Refused,
            Some([0x00, f, e, params @ ..]) if *f == feature && *e == echo => Reply::Data(params.to_vec()),
            _ => Reply::Other,
        },
        _ => Reply::Other,
    }
}

/// Root.getFeature answers index, type, version; index 0 = the device has no such feature.
fn feature_index(found: &[u8]) -> Option<u8> {
    match found {
        [index, _, _, ..] if *index != 0 => Some(*index),
        _ => None,
    }
}

/// The battery feature's answer: percent, a second level that is not used here, charge state.
fn battery_state(params: &[u8]) -> State {
    match params {
        [level, _, power, ..] if *level <= 100 => {
            State { full: *power == 3, ..on(*level as i32, matches!(power, 1 | 2)) }
        }
        _ => level_unknown(),
    }
}

/// The headset answers, but not with a level that can be shown.
fn level_unknown() -> State {
    State { detail: Some("d.levelUnknown".into()), ..State::new(true, None, false) }
}

/// Sends a request and reads until a frame ends the exchange, as `sort` sees it.
fn exchange(io: &mut Io, prefix: &[u8], request: &[u8], sort: impl Fn(&[u8]) -> Reply) -> win::Result<Reply> {
    let ends = |report: &[u8]| payload(prefix, report).is_some_and(|p| sort(p) != Reply::Other);
    let report = io.ask(request, ends, READS, false)?;
    Ok(report.as_deref().and_then(|r| payload(prefix, r)).map_or(Reply::Gone, sort))
}

fn ask_directly(io: &mut Io, prefix: &[u8], feature: u8, function: u8, params: &[u8]) -> win::Result<Reply> {
    let request = direct_request(prefix, feature, function, params);
    exchange(io, prefix, &request, |p| direct_reply(p, feature, function))
}

fn ask_headset(io: &mut Io, prefix: &[u8], bridge: u8, feature: u8, function: u8, params: &[u8]) -> win::Result<Reply> {
    let request = bridged_request(prefix, bridge, feature, function, params);
    exchange(io, prefix, &request, |p| bridged_reply(p, bridge, feature, function))
}

/// A headset behind its receiver: find the bridge on the receiver, the battery feature on the headset, then ask
/// it. Three exchanges a poll, as nothing is kept between polls; the indexes differ by model (the battery is
/// feature 4 on a PRO X 2, 5 on a PRO X 3, 6 on an A20 X).
fn behind_receiver(io: &mut Io, prefix: &[u8]) -> win::Result<State> {
    let Reply::Data(found) = ask_directly(io, prefix, ROOT, GET_FEATURE, &BRIDGE.to_be_bytes())? else {
        return Ok(off());
    };
    let Some(bridge) = feature_index(&found) else { return Ok(off()) };
    let battery = match ask_headset(io, prefix, bridge, ROOT, GET_FEATURE, &BATTERY.to_be_bytes())? {
        Reply::Data(found) => feature_index(&found),
        Reply::Refused => None,
        _ => return Ok(off()),
    };
    // Without the battery feature the headset is on, and that is all there is to say. HeadsetControl then sends a
    // PRO X 2 a request with the index fixed at 4; whatever feature sits there would be read as the battery
    let Some(battery) = battery else { return Ok(level_unknown()) };
    Ok(match ask_headset(io, prefix, bridge, battery, GET_LEVEL, &[])? {
        Reply::Data(params) => battery_state(&params),
        Reply::Refused => level_unknown(),
        _ => off(),
    })
}

/// The ASTRO A20 X on its cable has no bridge: the headset itself answers.
fn on_its_cable(io: &mut Io, prefix: &[u8]) -> win::Result<State> {
    let battery = match ask_directly(io, prefix, ROOT, GET_FEATURE, &BATTERY.to_be_bytes())? {
        Reply::Data(found) => feature_index(&found),
        Reply::Refused => None,
        _ => return Ok(off()),
    };
    let Some(battery) = battery else { return Ok(level_unknown()) };
    Ok(match ask_directly(io, prefix, battery, GET_LEVEL, &[])? {
        Reply::Data(params) => battery_state(&params),
        Reply::Refused => level_unknown(),
        _ => off(),
    })
}

// ---- ASTRO A50 Gen 4 base station ----

const A50_REPORT: u8 = 0x02;
const A50_HEADSET_STATE: u8 = 0x54;
const A50_BATTERY: u8 = 0x7C;

/// Bits of the headset state: on the dock, and linked by radio (which is also "switched on").
const A50_DOCKED: u8 = 0x01;
const A50_LINKED: u8 = 0x02;

/// The base station answers within milliseconds.
const A50_TIMEOUT_MS: u64 = 1000;

/// Neither command sent here takes data: the report ID and the command are the whole request.
fn a50_request(command: u8) -> [u8; 2] {
    [A50_REPORT, command]
}

/// A reply that carries data: 02, status 02, length, data. The length is not looked at here: Gen 4 firmware is
/// known to get it wrong.
fn a50_has_data(reply: &[u8]) -> bool {
    matches!(reply, [A50_REPORT, 0x02, _, _, ..])
}

/// The first data byte of the reply to a command, when the reply carries data.
fn a50_ask(io: &mut Io, command: u8) -> win::Result<Option<u8>> {
    // One reply per request and nothing unasked: the first report is the answer, whatever its status
    let reply = io.ask(&a50_request(command), |d| d.first() == Some(&A50_REPORT), 4, false)?;
    Ok(reply.filter(|r| a50_has_data(r)).map(|r| r[3]))
}

/// Whether the level the base station has is the headset's current one: it is while the headset is linked or sits
/// on the dock, and the last one heard otherwise. That is AstroBar's rule, which works out how fast the headset
/// charges on the dock from these levels; HeadsetControl reads the level only with the link bit set.
fn a50_is_there(state: u8) -> bool {
    state & (A50_DOCKED | A50_LINKED) != 0
}

/// The battery byte: bit 7 charging, the rest percent.
fn a50_state(battery: u8) -> State {
    match (battery & 0x7F) as i32 {
        level if level <= 100 => on(level, battery & 0x80 != 0),
        _ => level_unknown(),
    }
}

fn astro_a50_gen4(io: &mut Io) -> win::Result<State> {
    io.set_timeout(A50_TIMEOUT_MS);
    if !a50_ask(io, A50_HEADSET_STATE)?.is_some_and(a50_is_there) {
        return Ok(off());
    }
    Ok(a50_ask(io, A50_BATTERY)?.map_or_else(off, a50_state))
}

#[cfg(test)]
mod tests {
    //! Centurion frames as HeadsetControl's tests have them from G PRO X 2, G522, G PRO X 3 and ASTRO A20 X
    //! captures, with our software ID where theirs (1) was: the ID is only echoed. A50 Gen 4 replies as in
    //! HeadsetControl's tests.
    use super::*;

    const BRIDGE_AT: u8 = 0x03;

    fn state(online: bool, level: Option<i32>, charging: bool) -> State {
        State::new(online, level, charging)
    }

    /// A frame as it is read: padded with zeros to 64 bytes.
    fn report(prefix: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut report = frame(prefix, payload);
        report.resize(64, 0);
        report
    }

    #[test]
    fn requests_to_the_receiver_carry_the_prefix_and_their_exact_length() {
        // Where is the bridge
        assert_eq!(
            direct_request(PRO_X_2, ROOT, GET_FEATURE, &BRIDGE.to_be_bytes()),
            [0x51, 0x05, 0x00, 0x00, 0x06, 0x00, 0x03]
        );
        assert_eq!(
            direct_request(ADDRESSED, ROOT, GET_FEATURE, &BRIDGE.to_be_bytes()),
            [0x50, 0x23, 0x05, 0x00, 0x00, 0x06, 0x00, 0x03]
        );
        // The A20 X on its cable: where is the battery, then the level from index 6
        assert_eq!(
            direct_request(ADDRESSED, ROOT, GET_FEATURE, &BATTERY.to_be_bytes()),
            [0x50, 0x23, 0x05, 0x00, 0x00, 0x06, 0x01, 0x04]
        );
        assert_eq!(direct_request(ADDRESSED, 0x06, GET_LEVEL, &[]), [0x50, 0x23, 0x03, 0x00, 0x06, 0x06]);
    }

    #[test]
    fn requests_to_the_headset_go_through_the_bridge() {
        // Where is the battery
        assert_eq!(
            bridged_request(PRO_X_2, BRIDGE_AT, ROOT, GET_FEATURE, &BATTERY.to_be_bytes()),
            [0x51, 0x0A, 0x00, 0x03, 0x16, 0x00, 0x05, 0x00, 0x00, 0x06, 0x01, 0x04]
        );
        // The level, from index 2 on a G522 and 5 on a G PRO X 3
        assert_eq!(
            bridged_request(ADDRESSED, BRIDGE_AT, 0x02, GET_LEVEL, &[]),
            [0x50, 0x23, 0x08, 0x00, 0x03, 0x16, 0x00, 0x03, 0x00, 0x02, 0x06]
        );
        assert_eq!(
            bridged_request(ADDRESSED, BRIDGE_AT, 0x05, GET_LEVEL, &[]),
            [0x50, 0x23, 0x08, 0x00, 0x03, 0x16, 0x00, 0x03, 0x00, 0x05, 0x06]
        );
    }

    #[test]
    fn the_payload_is_what_the_length_byte_covers() {
        // G HUB switching a G522's lights on, as captured
        let lights = [
            0x50, 0x23, 0x12, 0x00, 0x03, 0x1C, 0x00, 0x0D, 0x00, 0x12, 0x3C, 0x00, 0x00, 0x04, 0x00, 0xB8, 0xFC, 0xFF,
            0x00, 0xAB, 0x64, 0x00, 0x00,
        ];
        assert_eq!(payload(ADDRESSED, &lights), Some(&lights[4..21]));
        // A PRO X 2 owner's capture of a battery reply
        let battery = report(PRO_X_2, &[0x03, 0x10, 0x00, 0x06, 0x00, 0x04, 0x0A, 0x64, 0x64, 0x00]);
        assert_eq!(battery[..3], [0x51, 0x0B, 0x00]);
        assert_eq!(payload(PRO_X_2, &battery), Some(&battery[3..13]));
    }

    #[test]
    fn frames_of_another_model_or_cut_short_have_no_payload() {
        let reply = report(ADDRESSED, &[0x00, 0x06, 0x03, 0x00, 0x01]);
        assert_eq!(payload(PRO_X_2, &reply), None);
        assert_eq!(payload(&[0x50, 0x24], &reply), None);
        // The length says more than there is
        assert_eq!(payload(PRO_X_2, &[0x51, 0x0B, 0x00, 0x03, 0x10]), None);
        // Nothing but the flags byte
        assert_eq!(payload(PRO_X_2, &report(PRO_X_2, &[])), None);
        assert_eq!(payload(PRO_X_2, &[]), None);
    }

    #[test]
    fn a_piece_of_a_longer_message_has_no_payload() {
        let mut piece = report(PRO_X_2, &[0x03, 0x10, 0x00, 0x06, 0x00, 0x02, 0x06, 0x2F, 0x32, 0x00]);
        piece[2] = 0x01;
        assert_eq!(payload(PRO_X_2, &piece), None);
    }

    #[test]
    fn the_receiver_tells_where_its_bridge_is() {
        let reply = direct_reply(&[0x00, 0x06, 0x03, 0x00, 0x01], ROOT, GET_FEATURE);
        assert_eq!(reply, Reply::Data(vec![0x03, 0x00, 0x01]));
        assert_eq!(feature_index(&[0x03, 0x00, 0x01]), Some(0x03));
    }

    #[test]
    fn index_zero_means_no_such_feature() {
        assert_eq!(feature_index(&[0x00, 0x00, 0x00]), None);
        assert_eq!(feature_index(&[0x06]), None);
        assert_eq!(feature_index(&[]), None);
    }

    #[test]
    fn an_error_from_the_receiver_is_a_refusal() {
        // An A20 X on its cable asked for the bridge it doesn't have
        assert_eq!(direct_reply(&[0xFF, 0x00, 0x06, 0x05], ROOT, GET_FEATURE), Reply::Refused);
    }

    #[test]
    fn replies_to_other_requests_are_not_ours() {
        // Another program's software ID
        assert_eq!(direct_reply(&[0x00, 0x01, 0x03, 0x00, 0x01], ROOT, GET_FEATURE), Reply::Other);
        // Another function of the feature we asked: a late reply an A20 X sent on its cable
        assert_eq!(direct_reply(&[0x06, 0x16, 0x0F, 0x39], 0x06, GET_LEVEL), Reply::Other);
        // Another feature
        assert_eq!(direct_reply(&[0x05, 0x06, 0x2D, 0x2D, 0x00], 0x06, GET_LEVEL), Reply::Other);
        assert_eq!(direct_reply(&[0xFF, 0x05, 0x06, 0x05], 0x06, GET_LEVEL), Reply::Other);
        assert_eq!(direct_reply(&[0x06], 0x06, GET_LEVEL), Reply::Other);
    }

    #[test]
    fn the_headset_on_its_cable_answers_with_the_level() {
        // An A20 X: 70%, charging; later 45%, on battery
        let reply = direct_reply(&[0x06, 0x06, 0x46, 0x46, 0x02], 0x06, GET_LEVEL);
        assert_eq!(reply, Reply::Data(vec![0x46, 0x46, 0x02]));
        assert_eq!(battery_state(&[0x46, 0x46, 0x02]), state(true, Some(70), true));
        assert_eq!(battery_state(&[0x2D, 0x2D, 0x00]), state(true, Some(45), false));
    }

    #[test]
    fn the_headset_answers_through_the_bridge() {
        // An A20 X behind its transmitter: the battery is feature 6, then 78%
        let found = [0x03, 0x10, 0x00, 0x06, 0x00, 0x00, 0x06, 0x06, 0x00, 0x03];
        assert_eq!(bridged_reply(&found, BRIDGE_AT, ROOT, GET_FEATURE), Reply::Data(vec![0x06, 0x00, 0x03]));
        let level = [0x03, 0x10, 0x00, 0x06, 0x00, 0x06, 0x06, 0x4E, 0x50, 0x00];
        assert_eq!(bridged_reply(&level, BRIDGE_AT, 0x06, GET_LEVEL), Reply::Data(vec![0x4E, 0x50, 0x00]));
        assert_eq!(battery_state(&[0x4E, 0x50, 0x00]), state(true, Some(78), false));
    }

    #[test]
    fn what_follows_the_message_in_the_frame_is_not_part_of_it() {
        let level = [0x03, 0x10, 0x00, 0x06, 0x00, 0x02, 0x06, 0x2F, 0x32, 0x00, 0xAA, 0xBB];
        assert_eq!(bridged_reply(&level, BRIDGE_AT, 0x02, GET_LEVEL), Reply::Data(vec![0x2F, 0x32, 0x00]));
    }

    #[test]
    fn a_message_shorter_than_its_size_is_not_an_answer() {
        let cut = [0x03, 0x10, 0x00, 0x06, 0x00, 0x02, 0x06, 0x2F];
        assert_eq!(bridged_reply(&cut, BRIDGE_AT, 0x02, GET_LEVEL), Reply::Other);
        // A size that doesn't fit one frame, or a device other than the headset
        let long = [0x03, 0x10, 0x01, 0x06, 0x00, 0x02, 0x06, 0x2F, 0x32, 0x00];
        assert_eq!(bridged_reply(&long, BRIDGE_AT, 0x02, GET_LEVEL), Reply::Other);
    }

    #[test]
    fn the_acknowledgement_is_not_the_answer() {
        assert_eq!(bridged_reply(&[0x03, 0x16], BRIDGE_AT, 0x02, GET_LEVEL), Reply::Other);
        // As a PRO X 2 sent it to G HUB
        assert_eq!(bridged_reply(&[0x03, 0x1A], BRIDGE_AT, 0x04, GET_LEVEL), Reply::Other);
    }

    #[test]
    fn answers_to_other_programs_and_other_requests_are_not_ours() {
        // A PRO X 2 answering G HUB (software ID A) with 100%
        let theirs = [0x03, 0x10, 0x00, 0x06, 0x00, 0x04, 0x0A, 0x64, 0x64, 0x00];
        assert_eq!(bridged_reply(&theirs, BRIDGE_AT, 0x04, GET_LEVEL), Reply::Other);
        let ours = [0x03, 0x10, 0x00, 0x06, 0x00, 0x04, 0x06, 0x64, 0x64, 0x00];
        assert_eq!(bridged_reply(&ours, BRIDGE_AT, 0x04, GET_LEVEL), Reply::Data(vec![0x64, 0x64, 0x00]));
        // Another feature, another function, another bridge index
        assert_eq!(bridged_reply(&ours, BRIDGE_AT, 0x02, GET_LEVEL), Reply::Other);
        assert_eq!(bridged_reply(&ours, BRIDGE_AT, 0x04, 0x10), Reply::Other);
        assert_eq!(bridged_reply(&ours, 0x04, 0x04, GET_LEVEL), Reply::Other);
        // The headset's own news (FF where a reply has 00)
        let news = [0x03, 0x10, 0x00, 0x06, 0xFF, 0x04, 0x06, 0x64, 0x64, 0x00];
        assert_eq!(bridged_reply(&news, BRIDGE_AT, 0x04, GET_LEVEL), Reply::Other);
    }

    #[test]
    fn an_error_from_the_headset_is_a_refusal() {
        let error = [0x03, 0x10, 0x00, 0x05, 0x00, 0xFF, 0x04, 0x06, 0x01];
        assert_eq!(bridged_reply(&error, BRIDGE_AT, 0x04, GET_LEVEL), Reply::Refused);
        // For a request that wasn't ours
        assert_eq!(bridged_reply(&error, BRIDGE_AT, 0x02, GET_LEVEL), Reply::Other);
        let theirs = [0x07, 0x10, 0x00, 0x05, 0x00, 0xFF, 0x04, 0x20, 0x01];
        assert_eq!(bridged_reply(&theirs, 0x07, 0x04, 0x20), Reply::Other);
    }

    #[test]
    fn a_receiver_without_its_headset_says_so() {
        // A PRO X 2 receiver refusing to pass the request on
        assert_eq!(bridged_reply(&[0xFF, 0x03, 0x16, 0x0B], BRIDGE_AT, ROOT, GET_FEATURE), Reply::Gone);
        // The headset went off while its answer was waited for
        assert_eq!(bridged_reply(&[0x03, 0x00, 0x00, 0x00], BRIDGE_AT, 0x02, GET_LEVEL), Reply::Gone);
    }

    #[test]
    fn other_refusals_and_a_headset_coming_on_end_nothing() {
        // The receiver refusing another function of the bridge, or another program's request
        assert_eq!(bridged_reply(&[0xFF, 0x03, 0x21, 0x0B], BRIDGE_AT, ROOT, GET_FEATURE), Reply::Other);
        assert_eq!(bridged_reply(&[0xFF, 0x03, 0x11, 0x0B], BRIDGE_AT, ROOT, GET_FEATURE), Reply::Other);
        assert_eq!(bridged_reply(&[0x03, 0x00, 0x00, 0x01], BRIDGE_AT, 0x02, GET_LEVEL), Reply::Other);
        assert_eq!(bridged_reply(&[0x04, 0x00, 0x00, 0x00], BRIDGE_AT, 0x02, GET_LEVEL), Reply::Other);
        // The bridge telling how many headsets it has, which is not asked here
        assert_eq!(bridged_reply(&[0x03, 0x01, 0x03, 0xE8, 0x00, 0x00, 0x01], BRIDGE_AT, 0x02, 0x00), Reply::Other);
    }

    #[test]
    fn the_charge_state_tells_charging_from_charged() {
        assert_eq!(battery_state(&[0x2F, 0x32, 0x00]), state(true, Some(47), false));
        assert_eq!(battery_state(&[0x30, 0x32, 0x02]), state(true, Some(48), true));
        assert_eq!(battery_state(&[42, 42, 0x01]), state(true, Some(42), true));
        assert_eq!(battery_state(&[100, 100, 0x03]), State { full: true, ..state(true, Some(100), false) });
    }

    #[test]
    fn a_level_over_100_or_a_short_answer_is_no_level() {
        assert_eq!(battery_state(&[101, 100, 0x00]), level_unknown());
        assert_eq!(battery_state(&[0xFF, 0xFF, 0xFF]), level_unknown());
        assert_eq!(battery_state(&[50]), level_unknown());
        assert_eq!(battery_state(&[]), level_unknown());
        assert_eq!(level_unknown().level, None);
        assert!(level_unknown().online);
    }

    #[test]
    fn a50_requests_are_the_report_id_and_the_command() {
        assert_eq!(a50_request(A50_HEADSET_STATE), [0x02, 0x54]);
        assert_eq!(a50_request(A50_BATTERY), [0x02, 0x7C]);
    }

    #[test]
    fn a50_headset_is_there_while_linked_or_on_the_dock() {
        // The answers to 54 that HeadsetControl's tests have: linked off the dock, linked on it, on it without a
        // link, and off - when what 7C answers is the last level heard
        assert!(a50_is_there(0x02));
        assert!(a50_is_there(0x03));
        assert!(a50_is_there(0x01));
        assert!(!a50_is_there(0x00));
        // A bit nobody has a meaning for is not the headset
        assert!(!a50_is_there(0x04));
    }

    #[test]
    fn a50_replies_carry_data_only_with_status_02() {
        assert!(a50_has_data(&[0x02, 0x02, 0x01, 0x48]));
        // Taken, without data
        assert!(!a50_has_data(&[0x02, 0x00, 0x00, 0x00]));
        // An error: its code and its name
        assert!(!a50_has_data(&[0x02, 0x01, 0x1F, 0x01, 0x00, 0x00, 0x00, b'H', b'I', b'D', b'_', b'E']));
        assert!(!a50_has_data(&[0x02, 0x02, 0x01]));
        assert!(!a50_has_data(&[0x03, 0x02, 0x01, 0x48]));
    }

    #[test]
    fn a50_battery_byte_is_percent_and_a_charging_bit() {
        assert_eq!(a50_state(0x48), state(true, Some(72), false));
        assert_eq!(a50_state(0xE1), state(true, Some(97), true));
        assert_eq!(a50_state(0x80 | 90), state(true, Some(90), true));
        assert_eq!(a50_state(0x00), state(true, Some(0), false));
        assert_eq!(a50_state(100), state(true, Some(100), false));
    }

    #[test]
    fn a50_battery_byte_over_100_is_no_level() {
        assert_eq!(a50_state(101), level_unknown());
        assert_eq!(a50_state(0xFF), level_unknown());
    }

    #[test]
    fn the_a50_gen_3_is_not_claimed() {
        // See the module's header: nothing tells when its headset is off
        assert!(!SPECS.iter().any(|s| s.vid == 0x9886 && s.pids.contains(&0x0015)));
    }
}
