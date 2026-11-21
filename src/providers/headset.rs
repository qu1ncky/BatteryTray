//! Wireless gaming headsets that answer a battery request on a vendor HID collection: HyperX, Logitech G,
//! SteelSeries Arctis, Corsair, Sony INZONE and a few more. The request/response bytes
//! are the protocol facts collected by HeadsetControl (github.com/Sapd/HeadsetControl, lib/devices/*), for the HyperX
//! headsets by HyperHeadset (github.com/LennardKittner/HyperHeadset), for the Corsair ones by OpenLinkHub
//! (github.com/jurkovic-nikola/OpenLinkHub), and for the SteelSeries Arctis Nova Elite, the Sony INZONE Buds and the
//! MCHOSE, FIFINE and VGN headsets by HaloBattery (github.com/HeyOkay/HaloBattery) and Wireless-Device-Battery-Monitor
//! (github.com/krisgfx9/Wireless-Device-Battery-Monitor, src/drivers); this is an independent implementation. Byte
//! positions below are as hidapi returns them: for devices without report IDs Windows puts a 0x00 in front of every
//! input report, which `Io::data` strips the same way hidapi does.
//! Read-only status queries; not yet verified on hardware by this project, which is why the first exchange with
//! each headset goes to the log.

use super::devices::{map, off, on, Io, Spec, State};
use super::{pad, sleep_ms};
use crate::win;

// Voltage (mV) -> percent tables HeadsetControl uses for the Logitech headsets
const G533: &[(i32, i32)] = &[(4200, 100), (3850, 50), (3790, 30), (3750, 20), (3680, 5), (3330, 0)];
const G535: &[(i32, i32)] = &[(4175, 100), (3817, 50), (3766, 30), (3730, 20), (3664, 5), (3310, 0)];
const G633: &[(i32, i32)] =
    &[(4100, 100), (3950, 80), (3850, 60), (3750, 40), (3650, 20), (3500, 10), (3300, 5), (3150, 0)];
const GPRO: &[(i32, i32)] = &[(4150, 100), (3830, 50), (3780, 30), (3740, 20), (3670, 5), (3320, 0)];

pub static SPECS: &[Spec] = &[
    // ---- HyperX ----
    Spec {
        key: "hyperx-cloud-alpha-wireless",
        name: "HyperX Cloud Alpha Wireless",
        vid: 0x03F0,
        pids: &[0x098D, 0x1743, 0x1765],
        page: 0,
        usage: 0,
        interface: -1,
        query: hyperx_cloud_alpha,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-ii-wireless",
        name: "HyperX Cloud II Wireless",
        vid: 0x03F0,
        pids: &[0x0696, 0x018B],
        page: 0xFF90,
        usage: 0x0303,
        interface: -1,
        query: hyperx_cloud_ii_hp,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-ii-wireless",
        name: "HyperX Cloud II Wireless",
        vid: 0x0951,
        pids: &[0x1718],
        page: 0,
        usage: 0,
        interface: -1,
        query: hyperx_cloud_ii_kingston,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-stinger-2-wireless",
        name: "HyperX Cloud Stinger 2 Wireless",
        vid: 0x03F0,
        pids: &[0x0D93],
        page: 0xFF90,
        usage: 0x0303,
        interface: -1,
        query: hyperx_cloud_ii_hp,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-flight-s",
        name: "HyperX Cloud Flight S",
        vid: 0x0951,
        pids: &[0x16EA, 0x16EB],
        page: 0,
        usage: 0,
        interface: -1,
        query: hyperx_cloud_ii_kingston,
        ..Spec::HEADSET
    },
    // The Core speaks as the Cloud III does, not as the Cloud II
    Spec {
        key: "hyperx-cloud-ii-core-wireless",
        name: "HyperX Cloud II Core Wireless",
        vid: 0x03F0,
        pids: &[0x069F, 0x0995],
        page: 0,
        usage: 0,
        interface: -1,
        query: hyperx_cloud_iii,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-iii-wireless",
        name: "HyperX Cloud III Wireless",
        vid: 0x03F0,
        pids: &[0x05B7, 0x0C9D],
        page: 0,
        usage: 0,
        interface: -1,
        query: hyperx_cloud_iii,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-flight",
        name: "HyperX Cloud Flight",
        vid: 0x0951,
        pids: &[0x16C4, 0x1723, 0x1749],
        page: 0,
        usage: 0,
        interface: -1,
        query: hyperx_cloud_flight,
        ..Spec::HEADSET
    },
    Spec {
        key: "hyperx-cloud-flight",
        name: "HyperX Cloud Flight",
        vid: 0x03F0,
        pids: &[0x0E90],
        page: 0,
        usage: 0,
        interface: -1,
        query: hyperx_cloud_flight,
        ..Spec::HEADSET
    },
    // ---- Logitech G (HID++ with a fixed feature index; the reply carries the battery voltage) ----
    Spec {
        key: "logitech-g533",
        name: "Logitech G533",
        vid: 0x046D,
        pids: &[0x0A66],
        page: 0xFF43,
        usage: 0x0202,
        interface: 3,
        query: |io| logitech_voltage(io, 0x07, 0x01, G533),
        ..Spec::HEADSET
    },
    Spec {
        key: "logitech-g535",
        name: "Logitech G535",
        vid: 0x046D,
        pids: &[0x0AC4],
        page: 0x000C,
        usage: 0x0001,
        interface: 3,
        query: |io| logitech_voltage(io, 0x05, 0x0D, G535),
        ..Spec::HEADSET
    },
    Spec {
        key: "logitech-g-headset",
        name: "Logitech G633 / G635 / G733 / G933 / G935",
        vid: 0x046D,
        pids: &[0x0A5C, 0x0A89, 0x0A5B, 0x0A87, 0x0AB5, 0x0AFE, 0x0B1F],
        page: 0xFF43,
        usage: 0x0202,
        interface: -1,
        query: |io| logitech_voltage(io, 0x08, 0x0A, G633),
        ..Spec::HEADSET
    },
    Spec {
        key: "logitech-g-pro",
        name: "Logitech G PRO / PRO X Wireless",
        vid: 0x046D,
        pids: &[0x0AA7, 0x0AAA, 0x0ABA, 0x0AFB, 0x0AFC],
        page: 0xFF43,
        usage: 0x0202,
        interface: -1,
        query: |io| logitech_voltage(io, 0x06, 0x0D, GPRO),
        ..Spec::HEADSET
    },
    Spec {
        key: "logitech-g930",
        name: "Logitech G930",
        vid: 0x046D,
        pids: &[0x0A1F],
        page: 0xFF00,
        usage: 0x0001,
        interface: -1,
        query: logitech_g930,
        ..Spec::HEADSET
    },
    Spec {
        key: "astro-a50",
        name: "ASTRO A50 (Gen 5) / A50 X",
        vid: 0x046D,
        pids: &[0x0B1C, 0x0B0B],
        page: 0xFF32,
        usage: 0x0074,
        interface: -1,
        query: astro_a50,
        ..Spec::HEADSET
    },
    // ---- SteelSeries Arctis ----
    Spec {
        key: "arctis-1",
        name: "SteelSeries Arctis 1 Wireless",
        vid: 0x1038,
        pids: &[0x12B3, 0x12B6, 0x12D7, 0x12D5],
        page: 0xFF43,
        usage: 0x0202,
        interface: 3,
        query: arctis_1,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-7",
        name: "SteelSeries Arctis 7 / Arctis Pro (2019)",
        vid: 0x1038,
        pids: &[0x1260, 0x12AD, 0x1252, 0x1280],
        page: 0,
        usage: 0,
        interface: 5,
        query: arctis_7,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-9",
        name: "SteelSeries Arctis 9",
        vid: 0x1038,
        pids: &[0x12C2, 0x12C4],
        page: 0,
        usage: 0,
        interface: -1,
        query: arctis_9,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-pro-wireless",
        name: "SteelSeries Arctis Pro Wireless",
        vid: 0x1038,
        pids: &[0x1290],
        page: 0,
        usage: 0,
        interface: -1,
        query: arctis_pro_wireless,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-nova-pro-wireless",
        name: "SteelSeries Arctis Nova Pro Wireless",
        vid: 0x1038,
        pids: &[0x12E0, 0x12E5, 0x225D],
        page: 0,
        usage: 0,
        interface: 4,
        query: arctis_nova_pro,
        ..Spec::HEADSET
    },
    // The station splits interface 3 into two vendor collections, and only one of them takes the request
    Spec {
        key: "arctis-nova-elite",
        name: "SteelSeries Arctis Nova Elite",
        vid: 0x1038,
        pids: &[0x2244, 0x2246, 0x2249, 0x2270],
        page: 0,
        usage: 0,
        interface: 3,
        accept: Some(|i| i.usage_page >= 0xFF00 && i.output_length > 0),
        query: arctis_nova_elite,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-nova-pro-omni",
        name: "SteelSeries Arctis Nova Pro Omni",
        vid: 0x1038,
        pids: &[0x2290],
        page: 0,
        usage: 0,
        interface: 3,
        accept: Some(|i| i.usage_page >= 0xFF00 && i.output_length > 0),
        query: arctis_nova_elite,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-7-plus",
        name: "SteelSeries Arctis 7+",
        vid: 0x1038,
        pids: &[0x220E, 0x2212, 0x2216, 0x2236],
        page: 0xFFC0,
        usage: 0x0001,
        interface: 3,
        query: arctis_7_plus,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-nova-7",
        name: "SteelSeries Arctis Nova 7",
        vid: 0x1038,
        pids: &[
            0x2202, 0x22A1, 0x227E, 0x2206, 0x2258, 0x229E, 0x22AD, 0x223A, 0x22A9, 0x227A, 0x22A4, 0x22A5, 0x220A,
            0x22A7, 0x2298,
        ],
        page: 0xFFC0,
        usage: 0x0001,
        interface: 3,
        query: arctis_nova_7,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-nova-5",
        name: "SteelSeries Arctis Nova 5 / Nova 3P",
        vid: 0x1038,
        pids: &[0x2232, 0x2253, 0x2269, 0x226D],
        page: 0xFFC0,
        usage: 0x0001,
        interface: 3,
        query: arctis_nova_5,
        ..Spec::HEADSET
    },
    Spec {
        key: "arctis-gamebuds",
        name: "SteelSeries Arctis GameBuds",
        vid: 0x1038,
        pids: &[0x230A],
        page: 0xFFC0,
        usage: 0x0001,
        interface: 3,
        query: arctis_gamebuds,
        ..Spec::HEADSET
    },
    // ---- Corsair ----
    Spec {
        key: "corsair-void",
        name: "Corsair Void / HS70",
        vid: 0x1B1C,
        pids: &[
            0x1B1C, 0x1B27, 0x0A14, 0x0A16, 0x0A17, 0x0A1D, 0x0A1A, 0x1B2A, 0x1B23, 0x1B29, 0x0A55, 0x0A51, 0x0A52,
            0x0A38, 0x0A4F, 0x0A2B, 0x0A75, 0x0A56,
        ],
        page: 0xFFC5,
        usage: 0x0001,
        interface: 3,
        query: corsair_void,
        ..Spec::HEADSET
    },
    Spec {
        key: "corsair-wireless-v2",
        name: "Corsair Void / Virtuoso / HS80 MAX Wireless",
        vid: 0x1B1C,
        pids: &[0x2A08, 0x2A02, 0x0A97, 0x2A0F, 0x2A03],
        page: 0,
        usage: 0,
        interface: 4,
        query: corsair_wireless_v2,
        ..Spec::HEADSET
    },
    // The Xbox receivers and the Void v2's second one have the same channel on interface 3
    Spec {
        key: "corsair-wireless-v2",
        name: "Corsair Void / Virtuoso / HS80 MAX Wireless",
        vid: 0x1B1C,
        pids: &[0x2A0C, 0x2A12, 0x2A05],
        page: 0,
        usage: 0,
        interface: 3,
        query: corsair_wireless_v2,
        ..Spec::HEADSET
    },
    // Receiver and cable IDs share a key: a Virtuoso XT on its cable leaves the receiver and shows up under the cable
    // ID, and the reading that reaches the headset wins (see poll)
    Spec {
        key: "corsair-virtuoso-xt",
        name: "Corsair Virtuoso / Virtuoso XT / SE",
        vid: 0x1B1C,
        pids: &[0x0A64, 0x0A62, 0x0A3E, 0x0A3D, 0x0A40, 0x0A3F, 0x0A42, 0x0A41, 0x0A44, 0x0A43],
        page: 0xFF42,
        usage: 0x0001,
        interface: 3,
        query: |io| {
            let wired = matches!(io.product_id(), 0x0A62 | 0x0A3D | 0x0A3F | 0x0A41 | 0x0A43);
            bragi(io, wired)
        },
        ..Spec::HEADSET
    },
    Spec {
        key: "corsair-hs80-rgb-wireless",
        name: "Corsair HS80 RGB Wireless",
        vid: 0x1B1C,
        pids: &[0x0A6B, 0x0A73, 0x0A69],
        page: 0xFF42,
        usage: 0x0001,
        interface: 3,
        query: |io| {
            let wired = io.product_id() == 0x0A69;
            bragi(io, wired)
        },
        ..Spec::HEADSET
    },
    // ---- Others ----
    Spec {
        key: "sony-inzone-h5",
        name: "Sony INZONE H5",
        vid: 0x054C,
        pids: &[0x0EBF],
        page: 0xFF04,
        usage: 0x0002,
        interface: -1,
        query: sony_inzone_h5,
        ..Spec::HEADSET
    },
    Spec {
        key: "sony-inzone-h9-ii",
        name: "Sony INZONE H9 II",
        vid: 0x054C,
        pids: &[0x0FA8],
        page: 0,
        usage: 0,
        interface: -1,
        accept: Some(|i| i.usage_page == 0xFF04 && i.output_length >= 64),
        query: sony_inzone_h5,
        ..Spec::HEADSET
    },
    Spec {
        key: "sony-inzone-buds",
        name: "Sony INZONE Buds",
        vid: 0x054C,
        pids: &[0x0EC2],
        page: 0,
        usage: 0,
        interface: -1,
        accept: Some(|i| i.usage_page == 0xFF04 && i.output_length >= 64),
        query: sony_inzone_buds,
        ..Spec::HEADSET
    },
    Spec {
        key: "mchose-x9",
        name: "MCHOSE X9",
        vid: 0x3837,
        pids: &[0x6045],
        page: 0xFF90,
        usage: 0x0001,
        interface: -1,
        query: |io| status_5565(io, mhub_status),
        ..Spec::HEADSET
    },
    Spec {
        key: "fifine-ht6",
        name: "FIFINE HT6",
        vid: 0x3142,
        pids: &[0x00C3],
        page: 0xFF90,
        usage: 0x0001,
        interface: -1,
        query: |io| status_5565(io, mhub_status),
        ..Spec::HEADSET
    },
    // One Windows PC showed this dongle with another vendor page, so any vendor collection is tried
    Spec {
        key: "mchose-v9-pro",
        name: "MCHOSE V9 Pro",
        vid: 0x291D,
        pids: &[0x385D],
        page: 0,
        usage: 0,
        interface: -1,
        query: |io| status_5565(io, v9_pro_status),
        ..Spec::HEADSET
    },
    Spec {
        key: "mchose-v9-pro-2",
        name: "MCHOSE V9 Pro 2",
        vid: 0x3837,
        pids: &[0x6011],
        page: 0xFF07,
        usage: 0x0222,
        interface: -1,
        query: mchose_v9_pro_2,
        ..Spec::HEADSET
    },
    // Its control channel is a consumer-control collection, not a vendor one
    Spec {
        key: "vgn-omnisound",
        name: "VGN OMNISOUND",
        vid: 0x372E,
        pids: &[0x10BB],
        page: 0,
        usage: 0,
        interface: -1,
        accept: Some(|i| i.usage_page == 0x000C && i.usage == 0x0001 && i.output_length >= 64),
        query: vgn_omnisound,
        ..Spec::HEADSET
    },
    Spec {
        key: "lenovo-wireless-voip",
        name: "Lenovo Wireless VoIP Headset",
        vid: 0x17EF,
        pids: &[0xA07D],
        page: 0xFF07,
        usage: 0x0222,
        interface: 3,
        query: lenovo_voip,
        ..Spec::HEADSET
    },
];

fn hyperx_cloud_alpha(io: &mut Io) -> win::Result<State> {
    // Mute events 21 BB 23 arrive on the same collection: match the command echo
    let ask = |io: &mut Io, cmd: u8| {
        io.ask(&[0x21, 0xBB, cmd], |d| d.len() > 3 && d[0] == 0x21 && d[1] == 0xBB && d[2] == cmd, 4, false)
    };
    let Some(link) = ask(io, 0x03)? else { return Ok(off()) };
    if link[3] == 0x01 {
        return Ok(off());
    }
    let charging = ask(io, 0x0C)?.is_some_and(|c| c[3] == 0x01);
    Ok(match ask(io, 0x0B)? {
        Some(b) => on(b[3] as i32, charging),
        None => off(),
    })
}

fn hyperx_cloud_ii_hp(io: &mut Io) -> win::Result<State> {
    let ask = |io: &mut Io, cmd: u8| {
        io.ask(
            &pad(52, &[0x06, 0xFF, 0xBB, cmd]),
            |d| d.len() >= 8 && d[0] == 0x06 && d[1] == 0xFF && d[2] == 0xBB && d[3] == cmd,
            4,
            false,
        )
    };
    let Some(b) = ask(io, 0x02)? else { return Ok(off()) };
    let charging = ask(io, 0x03)?.is_some_and(|c| c[4] == 0x01);
    Ok(on(b[7] as i32, charging))
}

fn hyperx_cloud_ii_kingston(io: &mut Io) -> win::Result<State> {
    let ask = |io: &mut Io, cmd: u8| {
        io.get_input_report(0x06); // HeadsetControl wakes the receiver this way; the reply is ignored
        io.ask(
            &pad(62, &[0x06, 0x00, 0x02, 0x00, 0x9A, 0x00, 0x00, 0x68, 0x4A, 0x8E, 0x0A, 0, 0, 0, 0xBB, cmd]),
            |d| d.len() >= 8 && d[0] == 0x0B && d[2] == 0xBB && d[3] == cmd,
            4,
            false,
        )
    };
    let Some(b) = ask(io, 0x02)? else { return Ok(off()) };
    let charging = ask(io, 0x03)?.is_some_and(|c| c[4] == 0x01);
    Ok(on(b[7] as i32, charging))
}

fn hyperx_cloud_iii(io: &mut Io) -> win::Result<State> {
    // The dongle also sends the same reports by itself when something changes, with 0B / 0C / 0D in place of the
    // command: either one is the answer
    let ask = |io: &mut Io, cmd: u8, unasked: u8| {
        io.ask(&pad(62, &[0x66, cmd]), |d| d.len() > 4 && d[0] == 0x66 && (d[1] == cmd || d[1] == unasked), 4, true)
    };
    match ask(io, 0x82, 0x0B)? {
        Some(link) if link[2] == 0x01 => {}
        _ => return Ok(off()),
    }
    // 1 = charging, 2 = full with the cable still in
    let power = ask(io, 0x8A, 0x0C)?.map_or(0, |c| c[2]);
    // The level counts only when one of the two bytes in front of it is set
    let level = ask(io, 0x89, 0x0D)?.filter(|b| (b[2] | b[3]) != 0).map(|b| (b[4] as i32).clamp(0, 100));
    Ok(State {
        detail: level.is_none().then(|| "d.levelUnknown".to_string()),
        full: power == 2,
        ..State::new(true, level, power == 1)
    })
}

fn hyperx_cloud_flight(io: &mut Io) -> win::Result<State> {
    // 64 xx (power) and 65 xx (mute) are events, not the reply
    let Some(r) = io.ask(&pad(20, &[0x21, 0xFF, 0x05]), |d| d.len() >= 5 && !matches!(d[0], 0x64 | 0x65), 4, false)?
    else {
        return Ok(off());
    };
    let mv = (r[3] as i32) << 8 | r[4] as i32;
    if mv > 4107 {
        return Ok(State::new(true, None, true)); // charging: the voltage says nothing then
    }
    if mv < 3000 {
        return Ok(off());
    }
    let v = mv as f64;
    let percent = if mv <= 3648 {
        0.00125 * v
    } else if mv > 3975 {
        100.0
    } else {
        2.547505e-8 * v.powi(4) - 3.900299e-4 * v.powi(3) + 2.238321 * v * v - 5706.256 * v + 5452299.0
    };
    Ok(on(percent.round_ties_even() as i32, false))
}

fn logitech_g930(io: &mut Io) -> win::Result<State> {
    if !io.set_feature(&pad(64, &[0xFF, 0x09, 0x00, 0xFD, 0xF4, 0x10, 0x05, 0xB1, 0xBF, 0xA0, 0x04])) {
        return Ok(off());
    }
    let mut r = None;
    for _ in 0..3 {
        sleep_ms(100);
        r = io.get_feature(0xFF).or(r);
    }
    Ok(match r {
        Some(r) if r.len() > 13 => on(map(r[13] as i32, 44, 91, 0, 100), false),
        _ => off(),
    })
}

fn astro_a50(io: &mut Io) -> win::Result<State> {
    let r = io.ask(
        &pad(64, &[0x02, 0x0C, 0x03, 0x00, 0x06, 0x0C]),
        |d| d.len() > 8 && d[0] == 0x02 && d[1] == 0x0C && d[4] == 0x06,
        8,
        false,
    )?;
    Ok(r.map_or_else(off, |r| on(r[6] as i32, r[8] != 0)))
}

fn arctis_1(io: &mut Io) -> win::Result<State> {
    Ok(match io.ask(&[0x06, 0x12], |d| d.len() > 3, 4, false)? {
        Some(r) if r[2] != 0x01 => on(r[3] as i32, false),
        _ => off(),
    })
}

fn arctis_7(io: &mut Io) -> win::Result<State> {
    Ok(io.ask(&[0x06, 0x18], |d| d.len() > 2, 4, false)?.map_or_else(off, |r| on(r[2] as i32, false)))
}

fn arctis_9(io: &mut Io) -> win::Result<State> {
    Ok(match io.ask(&[0x00, 0x20], |d| d.len() > 4, 4, false)? {
        Some(r) if r[4] != 0xFF => on(map(r[3] as i32, 0x64, 0x9A, 0, 100), r[4] == 0x01),
        _ => off(),
    })
}

fn arctis_pro_wireless(io: &mut Io) -> win::Result<State> {
    match io.ask(&pad(31, &[0x41, 0xAA]), |d| !d.is_empty(), 4, false)? {
        Some(link) if link[0] != 0x02 => {}
        _ => return Ok(off()),
    }
    Ok(io
        .ask(&pad(31, &[0x40, 0xAA]), |d| !d.is_empty(), 4, false)?
        .map_or_else(off, |r| on(map(r[0] as i32, 0, 4, 0, 100), false)))
}

fn arctis_nova_pro(io: &mut Io) -> win::Result<State> {
    // The base station also streams volume / chat-mix / EQ frames: take the B0 status reply only
    Ok(match io.ask(&pad(31, &[0x06, 0xB0]), |d| d.len() >= 16 && d[1] == 0xB0, 4, false)? {
        Some(r) if r[15] != 0x01 => on(map(r[6] as i32, 0, 8, 0, 100), r[15] == 0x02),
        _ => off(),
    })
}

fn arctis_nova_elite(io: &mut Io) -> win::Result<State> {
    // The station also sends 07 xx frames by itself (volume, the spare battery, power): only the reply counts
    let omni = io.product_id() == 0x2290;
    let r = io.ask(&pad(64, &[0x01, 0xB0]), |d| d.len() > 15 && d[0] == 0x01 && d[1] == 0xB0, 8, false)?;
    Ok(r.map_or_else(off, |r| nova_elite_state(&r, omni)))
}

/// The status reply of the Nova Elite's station: [6] the headset's level in percent, [14] its power (01 off, 02 on
/// the cable, 08 on), [15] 02 while it charges. The Nova Pro Omni's hub answers the same way; one of its owners
/// reads [15] as 01 off, 02 charging, 08 on, another takes [14] for the link as on the Elite, and nobody has looked
/// at the two with the headset off, so on the Omni both have to say it is there. A headset that is off doesn't
/// always come as "off", but then its level is 0.
pub fn nova_elite_state(r: &[u8], omni: bool) -> State {
    let (level, power, charge) = (r[6], r[14], r[15]);
    let there = power != 0x01 && (!omni || matches!(charge, 0x02 | 0x08));
    let charging = charge == 0x02 || (!omni && power == 0x02);
    if !there || level > 100 || (level == 0 && !charging) {
        return off();
    }
    on(level as i32, charging)
}

fn arctis_7_plus(io: &mut Io) -> win::Result<State> {
    Ok(match nova_status(io)? {
        Some(r) if r[1] != 0x01 => on(map(r[2] as i32, 0, 4, 0, 100), r[3] == 0x01),
        _ => off(),
    })
}

fn arctis_nova_7(io: &mut Io) -> win::Result<State> {
    let pid = io.product_id();
    Ok(match nova_status(io)? {
        Some(r) if r[3] != 0x00 => {
            // Older firmware reports five steps (0-4), newer ones a percentage
            let steps = matches!(pid, 0x2202 | 0x2206 | 0x220A | 0x223A | 0x227A | 0x22A4);
            on(if steps { map(r[2] as i32, 0, 4, 0, 100) } else { r[2] as i32 }, matches!(r[3], 0x01 | 0x02))
        }
        _ => off(),
    })
}

fn arctis_nova_5(io: &mut Io) -> win::Result<State> {
    Ok(match nova_status(io)? {
        Some(r) if r[1] != 0x02 => on(r[3] as i32, r[4] == 0x01),
        _ => off(),
    })
}

fn arctis_gamebuds(io: &mut Io) -> win::Result<State> {
    let Some(r) = nova_status(io)?.filter(|r| r.len() >= 7) else { return Ok(off()) };
    // 03 = out of the case; a bud in the case (02) reports 0% and no charging state
    let lowest = [(r[3], r[5]), (r[4], r[6])].iter().filter(|b| b.0 == 0x03).map(|b| b.1 as i32).min();
    Ok(lowest.map_or_else(off, |level| on(level, false)))
}

fn corsair_void(io: &mut Io) -> win::Result<State> {
    Ok(match io.ask(&[0xC9, 0x64], |d| d.len() > 4 && d[0] == 0x64, 4, false)? {
        Some(r) if r[4] != 0 => on((r[2] & 0x7F) as i32, matches!(r[4], 4 | 5)),
        _ => off(),
    })
}

fn corsair_wireless_v2(io: &mut Io) -> win::Result<State> {
    // Read-only "get" requests: 02 <endpoint: 08 receiver, 09 headset> 02 <property>. On the receivers that number
    // their reports the 02 is the report ID; on the others a 00 goes in front of it, as Windows wants for a
    // collection without report IDs. The reply reads the same either way: 01 <who> 02 <status> <value>
    let numbered = io.info().output_report_id != 0 || io.info().output_length == 64;
    let get = move |endpoint: u8, property: u8| {
        if numbered {
            pad(64, &[0x02, endpoint, 0x02, property])
        } else {
            pad(65, &[0x00, 0x02, endpoint, 0x02, property])
        }
    };
    io.send(&get(0x08, 0x13))?;
    io.send(&get(0x08, 0x12))?;
    io.drain()?;
    if io.ask(&get(0x09, 0x12), |d| !d.is_empty(), 4, false)?.is_none() {
        return Ok(off());
    }
    io.drain()?;
    for _ in 0..3 {
        // The receiver also sends the level by itself on report 03, with the bytes one place further (03 01 01 0F 00
        // LL HH), and its own late answers come as 01 00: only the headset's reply to a read counts
        let Some(r) = io.ask(&get(0x09, 0x0F), |d| is_bragi_reply(d, BRAGI_HEADSET), 4, false)? else {
            return Ok(off());
        };
        let value = r[4] as i32 | (r[5] as i32) << 8;
        if (1..=1000).contains(&value) {
            return Ok(on(value / 10, false));
        }
    }
    Ok(off())
}

fn sony_inzone_h5(io: &mut Io) -> win::Result<State> {
    // FF = the headset is off; anything else over 100 is no level either
    Ok(match sony_inzone_battery(io, 2)? {
        Some(r) if r[14] <= 100 => on(r[14] as i32, r[13] != 0),
        _ => off(),
    })
}

fn sony_inzone_buds(io: &mut Io) -> win::Result<State> {
    // With both buds in the case the dongle takes the request and stays silent
    let r = sony_inzone_battery(io, 6)?;
    Ok(r.and_then(|r| buds_level(&r[13..19])).map_or_else(off, |level| on(level, false)))
}

/// The lowest level of the INZONE Buds that are out of the case. The reply carries a state and a level for the
/// left bud, the right one and the case; a bud in the case has FF for a level.
pub fn buds_level(payload: &[u8]) -> Option<i32> {
    [payload[1], payload[3]].iter().filter(|&&level| level <= 100).map(|&level| level as i32).min()
}

/// Asks an INZONE dongle for the battery and returns the reply, whose payload of at least `payload` bytes starts at
/// [13]. The request is a "get" of event 04 to the headset (41), with our transaction ID (02 00) and a checksum.
fn sony_inzone_battery(io: &mut Io, payload: usize) -> win::Result<Option<Vec<u8>>> {
    let mut request = pad(64, &[0x02, 0x0C, 0x01, 0x00, 0xFC, 0x08, 0x96, 0xC3, 0x41, 0x04, 0x01, 0x02, 0x00]);
    request[13] = request[6..=12].iter().fold(0u8, |sum, &b| sum.wrapping_add(b));
    // Our transaction ID and a return / active-notification type, so unsolicited events don't count
    io.ask(
        &request,
        |d| {
            d.len() > 12 + payload
                && d[0] == 0x02
                && d[1] as usize >= 12 + payload
                && d[2] == 0x04
                && d[3] == 0xFF
                && d[6] == 0x96
                && d[7] == 0xC3
                && (d[8] >> 4) == 1
                && d[9] == 0x04
                && matches!(d[10], 0x10 | 0xA0)
                && d[11] == 0x02
                && d[12] == 0x00
        },
        10,
        false,
    )
}

/// What the status byte of a "55 65" reply says: (charging, full), or None when it isn't a state of a headset that
/// is there.
type Status5565 = fn(u8) -> Option<(bool, bool)>;

/// The MCHOSE X9 and the FIFINE HT6, as the vendor's M HUB reads it: 2 on battery, 3 charging, 4 full. The HT6
/// answers 1A when it has lost its headset.
fn mhub_status(status: u8) -> Option<(bool, bool)> {
    match status {
        0x02 => Some((false, false)),
        0x03 => Some((true, false)),
        0x04 => Some((false, true)),
        _ => None,
    }
}

/// The MCHOSE V9 Pro: 2 on battery, and 0 was measured on the cable.
fn v9_pro_status(status: u8) -> Option<(bool, bool)> {
    match status {
        0x00 => Some((true, false)),
        other => mhub_status(other),
    }
}

/// What the dongle puts in the status byte when it has lost its headset (the HT6), or the headset is asleep (M HUB).
const STATUS_5565_AWAY: u8 = 0x1A;

/// The status request several C-Media dongles share: 55 65 01, answered by 55 65 <level> <status>. Other 55 xx
/// reports come on the same collection (the HT6 says 55 0A when its headset is switched off).
fn status_5565(io: &mut Io, status: Status5565) -> win::Result<State> {
    let r = io.ask(&pad(64, &[0x55, 0x65, 0x01]), |d| is_5565_reply(d, status), 32, false)?;
    Ok(r.map_or_else(off, |r| parse_5565(&r, status)))
}

/// A "55 65" report that answers the request: one with a status this headset is known to send. HeadsetControl takes
/// the byte after the X9's value for the name of the field and waits through the reports that have another one
/// there; so a status nobody has named is waited through here too, not taken for "off" at once.
pub fn is_5565_reply(d: &[u8], status: Status5565) -> bool {
    d.len() > 3 && d[0] == 0x55 && d[1] == 0x65 && (d[3] == STATUS_5565_AWAY || status(d[3]).is_some())
}

/// A "55 65" reply: [2] the level in percent, [3] the status. A level of 0 with a status of 0 is an empty reply,
/// not a headset charging from nothing.
pub fn parse_5565(r: &[u8], status: Status5565) -> State {
    match status(r[3]) {
        Some((charging, full)) if r[2] <= 100 && (r[2] | r[3]) != 0 => State { full, ..on(r[2] as i32, charging) },
        _ => off(),
    }
}

fn vgn_omnisound(io: &mut Io) -> win::Result<State> {
    // The same request; the reply has no status byte, and the vendor's program shows no charging state either
    let r = io.ask(&pad(64, &[0x55, 0x65, 0x01]), |d| d.len() > 2 && d[0] == 0x55 && d[1] == 0x65, 32, false)?;
    Ok(match r {
        Some(r) if (1..=100).contains(&r[2]) => on(r[2] as i32, false),
        _ => off(),
    })
}

fn mchose_v9_pro_2(io: &mut Io) -> win::Result<State> {
    // Read battery: start AA, version 01, no checksum, length 1, a request (00 01), sub-command 0B
    let request = pad(64, &[0x55, 0xAA, 0x01, 0x00, 0x01, 0x00, 0x01, 0x0B]);
    let r = io.ask(&request, |d| mc_battery(d).is_some(), 16, false)?;
    Ok(r.and_then(|r| mc_battery(&r)).unwrap_or_else(off))
}

/// The battery in a frame of MCHOSE's "MC" protocol, report ID 55 first:
///   [1] AA  [2] version 01  [3] flags, 1 = a checksum follows  [4] length of the sub-command and the body
///   [5..6] type, 00 01 a reply, 00 04 a notification  [7] sub-command  [8..] body, then the checksum (XOR of
///   everything from the flags on) or 0.
/// The reply to "read battery" (0B) and the headset's own battery notification (22) have the same body: [0] is 0
/// when the headset is off or asleep, [1] holds charging in bit 7 and the level in the rest. None = another frame.
pub fn mc_battery(d: &[u8]) -> Option<State> {
    if d.len() < 10 || d[0] != 0x55 || d[1] != 0xAA || d[2] != 0x01 || d[5] != 0x00 {
        return None;
    }
    if !matches!((d[6], d[7]), (0x01, 0x0B) | (0x04, 0x22)) {
        return None;
    }
    let length = d[4] as usize;
    if length < 3 || 7 + length >= d.len() {
        return None;
    }
    let check = if d[3] == 0x01 { d[3..7 + length].iter().fold(0, |x, &b| x ^ b) } else { 0 };
    if d[7 + length] != check {
        return None;
    }
    if d[8] == 0 {
        return Some(off());
    }
    let level = d[9] & 0x7F;
    (level <= 100).then(|| on(level as i32, d[9] & 0x80 != 0))
}

fn lenovo_voip(io: &mut Io) -> win::Result<State> {
    if !io.set_feature(&pad(61, &[0x24, 0x01])) {
        return Ok(off());
    }
    Ok(match io.receive(|d| d.len() > 7 && d[0] == 0x27 && d[1] == 0x01, 4)? {
        Some(r) if r[2] == 0 => on(r[7] as i32, false),
        _ => off(),
    })
}

/// Status reports (B0) can also arrive unasked on this interface; other replies start with 20.
fn nova_status(io: &mut Io) -> win::Result<Option<Vec<u8>>> {
    io.ask(&[0x00, 0xB0], |d| d.len() > 4 && d[0] == 0xB0, 4, false)
}

// Corsair Bragi: who a request goes to, and the properties read here
const BRAGI_SELF: u8 = 0x08;
const BRAGI_HEADSET: u8 = 0x09;
const BRAGI_LEVEL: u8 = 0x0F;
const BRAGI_POWER: u8 = 0x10;

/// Corsair's Bragi property protocol on report 02 of the FF42 collection, as the Virtuoso XT / SE and the HS80 RGB
/// Wireless speak it. `02 TT 02 PP` reads property PP of target TT; the reply comes on report 01 as
/// `01 FF 02 SS VV VV VV VV`: FF who answered (01 the headset behind a receiver, 00 the device itself), 02 the echo,
/// SS the status (00 ok, 05 no such property), then the value, little-endian. A receiver relays to its headset on
/// target 09; a headset on its cable answers on 08, except the Virtuoso SE, which takes 09 there too and refuses 08.
/// Read-only: properties are only read, the headset stays in hardware mode.
fn bragi(io: &mut Io, wired: bool) -> win::Result<State> {
    let targets: &[u8] = if wired { &[BRAGI_SELF, BRAGI_HEADSET] } else { &[BRAGI_HEADSET] };
    for &target in targets {
        // A receiver without its headset is silent or answers "no such property" for itself
        let Some(level) = bragi_get(io, target, BRAGI_LEVEL)? else { continue };
        let power = bragi_get(io, target, BRAGI_POWER)?;
        return Ok(bragi_state(level, power));
    }
    Ok(off())
}

fn bragi_get(io: &mut Io, target: u8, property: u8) -> win::Result<Option<u32>> {
    Ok(io
        .ask(&pad(64, &[0x02, target, 0x02, property]), |d| is_bragi_reply(d, target), 8, false)?
        .and_then(|r| bragi_value(&r)))
}

/// A reply to a read sent to `target`; volume events on report 0E and what a receiver answers for itself don't count.
pub fn is_bragi_reply(d: &[u8], target: u8) -> bool {
    d.len() >= 8 && d[0] == 0x01 && d[1] == if target == BRAGI_HEADSET { 0x01 } else { 0x00 } && d[2] == 0x02
}

/// The value of a reply, or None when the status isn't ok.
pub fn bragi_value(d: &[u8]) -> Option<u32> {
    (d[3] == 0x00).then(|| u32::from_le_bytes([d[4], d[5], d[6], d[7]]))
}

/// `level`: tenths of a percent, 0-1000. `power`: charge state, if the firmware has it: 1 charging, 2 on battery,
/// 3 charged (ckb-next's BatteryStatus).
pub fn bragi_state(level: u32, power: Option<u32>) -> State {
    let (charging, full) = (power == Some(1), power == Some(3));
    if level <= 1000 {
        State { full, ..State::new(true, Some(level as i32 / 10), charging) }
    } else {
        State { detail: Some("d.levelUnknown".into()), full, ..State::new(true, None, charging) }
    }
}

fn logitech_voltage(io: &mut Io, feature_index: u8, function: u8, table: &[(i32, i32)]) -> win::Result<State> {
    let r = io.ask(
        &pad(20, &[0x11, 0xFF, feature_index, function]),
        |d| {
            d.len() >= 7
                && d[0] == 0x11
                && d[1] == 0xFF
                && ((d[2] == feature_index && d[3] == function) || d[2] == 0xFF)
        },
        4,
        false,
    )?;
    let Some(r) = r.filter(|r| r[2] != 0xFF) else { return Ok(off()) }; // HID++ error: headset off
    let mv = (r[4] as i32) << 8 | r[5] as i32;
    if mv < table[table.len() - 1].0 {
        return Ok(off());
    }
    Ok(on(interpolate(mv, table), r[6] == 0x03))
}

fn interpolate(mv: i32, table: &[(i32, i32)]) -> i32 {
    if mv >= table[0].0 {
        return 100;
    }
    for i in 1..table.len() {
        if mv < table[i].0 {
            continue;
        }
        let (hi_mv, hi_p) = table[i - 1];
        let (lo_mv, lo_p) = table[i];
        return lo_p + (mv - lo_mv) * (hi_p - lo_p) / (hi_mv - lo_mv);
    }
    0
}

#[cfg(test)]
mod tests {
    //! Corsair's Bragi property replies as the Virtuoso XT / SE and the HS80 RGB Wireless send them on report 01:
    //! `01 FF 02 SS VV VV VV VV`, with the layout from HeadsetControl (corsair_virtuoso_xt.hpp) and OpenLinkHub. The
    //! report ID comes first, so nothing is stripped in front of it.
    use super::*;

    const RECEIVER: u8 = 0x09;
    const SELF: u8 = 0x08;

    fn reply(data: &[u8]) -> Vec<u8> {
        pad(64, data)
    }

    fn state(online: bool, level: Option<i32>, charging: bool) -> State {
        State::new(online, level, charging)
    }

    #[test]
    fn headset_behind_the_receiver_answers() {
        // 87.3% = 873 = 0x0369
        let r = reply(&[0x01, 0x01, 0x02, 0x00, 0x69, 0x03]);
        assert!(is_bragi_reply(&r, RECEIVER));
        assert_eq!(bragi_value(&r), Some(873));
    }

    #[test]
    fn headset_on_its_cable_answers_for_itself() {
        let r = reply(&[0x01, 0x00, 0x02, 0x00, 0xE8, 0x03]);
        assert!(is_bragi_reply(&r, SELF));
        assert!(!is_bragi_reply(&r, RECEIVER));
        assert_eq!(bragi_value(&r), Some(1000));
    }

    #[test]
    fn receiver_answering_for_itself_is_not_the_headset() {
        assert!(!is_bragi_reply(&reply(&[0x01, 0x00, 0x02, 0x00, 0x1C, 0x1B]), RECEIVER));
    }

    #[test]
    fn no_such_property_is_no_value() {
        assert_eq!(bragi_value(&reply(&[0x01, 0x01, 0x02, 0x05])), None);
    }

    #[test]
    fn other_reports_are_not_the_reply() {
        assert!(!is_bragi_reply(&reply(&[0x0E, 0x01]), RECEIVER)); // volume event
        assert!(!is_bragi_reply(&reply(&[0x01, 0x01, 0x01, 0x00]), RECEIVER)); // reply to a write
        assert!(!is_bragi_reply(&[0x01, 0x01, 0x02, 0x00], RECEIVER)); // too short
    }

    #[test]
    fn level_is_in_tenths_of_a_percent() {
        assert_eq!(bragi_state(873, Some(2)), state(true, Some(87), false));
        assert_eq!(bragi_state(9, Some(2)), state(true, Some(0), false));
        assert_eq!(bragi_state(1000, None), state(true, Some(100), false));
    }

    #[test]
    fn charging_and_charged() {
        assert_eq!(bragi_state(540, Some(1)), state(true, Some(54), true));
        assert_eq!(bragi_state(1000, Some(3)), State { full: true, ..state(true, Some(100), false) });
    }

    #[test]
    fn level_out_of_range_is_not_known() {
        assert_eq!(
            bragi_state(0x0A69, Some(1)),
            State { detail: Some("d.levelUnknown".into()), ..state(true, None, true) }
        );
    }

    #[test]
    fn nova_pro_omni_reply_on_battery() {
        // Captured by an owner: 48%, on, on battery
        let r = [0x01, 0xB0, 0, 0, 0x04, 0x01, 0x30, 0x3C, 0x08, 0x01, 0, 0x0A, 0x05, 0, 0x08, 0x08, 0x03];
        assert_eq!(nova_elite_state(&r, true), state(true, Some(48), false));
        assert_eq!(nova_elite_state(&r, false), state(true, Some(48), false));
    }

    #[test]
    fn nova_elite_power_and_charging() {
        let reply = |level: u8, power: u8, charge: u8| {
            let mut r = [0u8; 64];
            (r[0], r[1], r[6], r[14], r[15]) = (0x01, 0xB0, level, power, charge);
            r
        };
        assert_eq!(nova_elite_state(&reply(77, 0x08, 0x08), false), state(true, Some(77), false));
        assert_eq!(nova_elite_state(&reply(77, 0x02, 0x02), false), state(true, Some(77), true));
        assert_eq!(nova_elite_state(&reply(77, 0x01, 0x00), false), off());
        // A headset that is off without saying so reads 0%
        assert_eq!(nova_elite_state(&reply(0, 0x08, 0x08), false), off());
        assert_eq!(nova_elite_state(&reply(101, 0x08, 0x08), false), off());
        // The Omni's hub says "off" in the other byte, or in this one: its owners don't agree which
        assert_eq!(nova_elite_state(&reply(77, 0x08, 0x01), true), off());
        assert_eq!(nova_elite_state(&reply(77, 0x01, 0x08), true), off());
        assert_eq!(nova_elite_state(&reply(77, 0x08, 0x02), true), state(true, Some(77), true));
        assert_eq!(nova_elite_state(&reply(77, 0x08, 0x04), true), off());
    }

    #[test]
    fn inzone_buds_show_the_lower_bud_that_is_out_of_the_case() {
        assert_eq!(buds_level(&[0x01, 80, 0x01, 65, 0x01, 100]), Some(65));
        assert_eq!(buds_level(&[0x01, 80, 0x00, 0xFF, 0x01, 100]), Some(80));
        assert_eq!(buds_level(&[0x00, 0xFF, 0x00, 0xFF, 0x01, 100]), None);
    }

    #[test]
    fn fifine_ht6_reply_on_battery() {
        // Captured by HeadsetControl: 80%, on battery
        let r = [0x55, 0x65, 0x50, 0x02, 0x02, 0x00, 0x00, 0xFF];
        assert_eq!(parse_5565(&r, mhub_status), state(true, Some(80), false));
    }

    #[test]
    fn status_of_a_55_65_reply() {
        assert_eq!(parse_5565(&[0x55, 0x65, 0x32, 0x03], mhub_status), state(true, Some(50), true));
        assert_eq!(
            parse_5565(&[0x55, 0x65, 0x64, 0x04], mhub_status),
            State { full: true, ..state(true, Some(100), false) }
        );
        // The HT6 without its headset, and a status nobody has seen
        assert_eq!(parse_5565(&[0x55, 0x65, 0x32, 0x1A], mhub_status), off());
        assert_eq!(parse_5565(&[0x55, 0x65, 0x32, 0x00], mhub_status), off());
        assert_eq!(parse_5565(&[0x55, 0x65, 0x65, 0x02], mhub_status), off());
    }

    #[test]
    fn a_55_65_report_with_an_unnamed_status_is_not_the_reply() {
        // The X9 as HeadsetControl captured it, then charging and full as M HUB reads them
        assert!(is_5565_reply(&[0x55, 0x65, 0x50, 0x02], mhub_status));
        assert!(is_5565_reply(&[0x55, 0x65, 0x50, 0x03], mhub_status));
        assert!(is_5565_reply(&[0x55, 0x65, 0x64, 0x04], mhub_status));
        // The dongle without its headset is an answer; another byte there, another report and a short one are not
        assert!(is_5565_reply(&[0x55, 0x65, 0x32, 0x1A], mhub_status));
        assert!(!is_5565_reply(&[0x55, 0x65, 0x32, 0x07], mhub_status));
        assert!(!is_5565_reply(&[0x55, 0x65, 0x32, 0x00], mhub_status));
        assert!(!is_5565_reply(&[0x55, 0x0A, 0x32, 0x02], mhub_status));
        assert!(!is_5565_reply(&[0x55, 0x65, 0x32], mhub_status));
        // Only the V9 Pro has 0 for a status
        assert!(is_5565_reply(&[0x55, 0x65, 0x32, 0x00], v9_pro_status));
    }

    #[test]
    fn nova_pro_omni_needs_both_bytes_to_say_the_headset_is_there() {
        // The captured reply with either byte turned to "off"
        let mut r = [0x01, 0xB0, 0, 0, 0x04, 0x01, 0x30, 0x3C, 0x08, 0x01, 0, 0x0A, 0x05, 0, 0x08, 0x08, 0x03];
        assert_eq!(nova_elite_state(&r, true), state(true, Some(48), false));
        r[14] = 0x01;
        assert_eq!(nova_elite_state(&r, true), off());
        (r[14], r[15]) = (0x08, 0x01);
        assert_eq!(nova_elite_state(&r, true), off());
    }

    #[test]
    fn corsair_wireless_v2_level_is_the_headsets_reply_to_a_read() {
        // An HS80 MAX on its 0A97 receiver, as Windows returned it: 42.0%
        assert!(is_bragi_reply(&reply(&[0x01, 0x01, 0x02, 0x00, 0xA4, 0x01]), RECEIVER));
        // The level the receiver sends by itself, and its own answer to a read of its product ID
        assert!(!is_bragi_reply(&reply(&[0x03, 0x01, 0x01, 0x0F, 0x00, 0xAC, 0x03]), RECEIVER));
        assert!(!is_bragi_reply(&reply(&[0x01, 0x00, 0x02, 0x00, 0x40, 0x0A]), RECEIVER));
    }

    #[test]
    fn mchose_v9_pro_on_battery_and_on_the_cable() {
        assert_eq!(parse_5565(&[0x55, 0x65, 0x32, 0x02], v9_pro_status), state(true, Some(50), false));
        assert_eq!(parse_5565(&[0x55, 0x65, 0x32, 0x00], v9_pro_status), state(true, Some(50), true));
        assert_eq!(parse_5565(&[0x55, 0x65, 0x00, 0x00], v9_pro_status), off());
    }

    #[test]
    fn mchose_v9_pro_2_battery_frames() {
        // A reply without a checksum: 67%, charging
        let reply = pad(64, &[0x55, 0xAA, 0x01, 0x00, 0x03, 0x00, 0x01, 0x0B, 0x01, 0xC3, 0x00]);
        assert_eq!(mc_battery(&reply), Some(state(true, Some(67), true)));
        // The headset's own notification, with a checksum: 01^03^00^04^22^01^40
        let notify = pad(64, &[0x55, 0xAA, 0x01, 0x01, 0x03, 0x00, 0x04, 0x22, 0x01, 0x40, 0x65]);
        assert_eq!(mc_battery(&notify), Some(state(true, Some(64), false)));
        let off_frame = pad(64, &[0x55, 0xAA, 0x01, 0x00, 0x03, 0x00, 0x01, 0x0B, 0x00, 0x00, 0x00]);
        assert_eq!(mc_battery(&off_frame), Some(off()));
    }

    #[test]
    fn other_mc_frames_are_not_the_battery() {
        // Another sub-command, a wrong checksum, a level over 100
        assert_eq!(mc_battery(&pad(64, &[0x55, 0xAA, 0x01, 0x00, 0x03, 0x00, 0x01, 0x0C, 0x01, 0x40])), None);
        assert_eq!(mc_battery(&pad(64, &[0x55, 0xAA, 0x01, 0x01, 0x03, 0x00, 0x04, 0x22, 0x01, 0x40, 0x66])), None);
        assert_eq!(mc_battery(&pad(64, &[0x55, 0xAA, 0x01, 0x00, 0x03, 0x00, 0x01, 0x0B, 0x01, 0x7F])), None);
        assert_eq!(mc_battery(&[0x55, 0xAA, 0x01]), None);
    }

    #[test]
    fn logitech_voltage_tables() {
        assert_eq!(interpolate(4200, G533), 100);
        assert_eq!(interpolate(3850, G533), 50);
        assert_eq!(interpolate(3820, G533), 40);
        assert_eq!(interpolate(3000, G533), 0);
    }
}
