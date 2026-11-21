//! Logitech mice, keyboards and other HID++ devices behind a Unifying, Bolt, Lightspeed or Nano receiver (device slots
//! 1-6) or on a cable (device index 0xFF). Facts from Solaar
//! (github.com/pwr-Solaar/Solaar, lib/logitech_receiver), the Linux driver hid-logitech-hidpp and LGSTrayBattery:
//! - Windows splits the HID++ interface into a short (report 0x10, 7 bytes) and a long (report 0x11, 20 bytes)
//!   collection. Short requests can only be written to the short one, yet HID++ 2.0 replies arrive on the long one -
//!   so both are read.
//! - Ping: [10 dev 00 1s 00 00 mark] -> [11 dev 00 1s major minor mark] (HID++ 2.0), or the HID++ 1.0 error
//!   [10 dev 8F 00 1s code]: 01 = HID++ 1.0 device, 04/09 = offline, 08 = empty slot.
//! - Root.GetFeature(id): [11 dev 00 0s idHi idLo] -> [index type version]; index 0 = not supported.
//! - Battery, first feature found: 0x1004 UnifiedBattery GetStatus [soc levelBits chargeStatus extPower]; 0x1000
//!   BatteryStatus [level next status]; 0x1001 BatteryVoltage and 0x1F20 ADC [mV hi, mV lo, flags].
//! - Name and kind: 0x0005 DeviceName (length, 16-character chunks, type). HID++ 1.0: registers 0x0D/0x07.
//!
//! Only reads; never writes registers. Not yet verified on hardware by this project.

use super::{contains_ignore_case, group_by, one_per_id, secs, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::t;
use crate::log;
use crate::reading::{DeviceKind, DeviceReading};
use crate::win;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const LOGITECH_VID: u16 = 0x046D;
const SOFTWARE_ID: u8 = 0x0C; // Solaar uses 0B, G HUB 0D, OpenRGB 07: replies are told apart by it
const TIMEOUT_MS: u64 = 1500;
const FEATURE_NAME: u16 = 0x0005;
const BATTERY_FEATURES: [u16; 4] = [0x1004, 0x1000, 0x1001, 0x1F20];

/// What was learned about a device in a slot; kept so the slow discovery runs once.
#[derive(Clone)]
struct Slot {
    name: String,
    kind: DeviceKind,
    hidpp20: bool,
    feature_index: u8,
    feature_id: u16,
}

impl Slot {
    fn has_battery(&self) -> bool {
        !self.hidpp20 || self.feature_index != 0
    }
}

/// `full`: charging is complete and the charger still connected, as the device says.
#[derive(Clone, Copy)]
struct Battery {
    online: bool,
    level: Option<i32>,
    charging: bool,
    full: bool,
}

#[derive(Default)]
pub struct LogitechProvider {
    slots: HashMap<String, Slot>,
}

impl Provider for LogitechProvider {
    fn name(&self) -> &'static str {
        "Logitech"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        let collections: Vec<HidInfo> = hid::enumerate(|v, p| {
            v == LOGITECH_VID && !(0x0800..=0x09FF).contains(&p) // webcams lock up
                && !super::devices::handled(v, p)
        })
        .into_iter()
        // Bluetooth LE devices: Windows reports their battery itself (the Bluetooth provider)
        .filter(|i| !contains_ignore_case(&i.path, "{00001812-"))
        .collect();

        // One USB interface = one HID++ channel, split by Windows into collections
        for (channel, list) in group_by(collections, |i| channel_key(&i.path)) {
            let short = list.iter().find(|i| !i.access_denied && i.usage_page >= 0xFF00 && i.output_length == 7);
            let long = list.iter().find(|i| !i.access_denied && i.usage_page >= 0xFF00 && i.output_length == 20);
            if short.is_none() && long.is_none() {
                continue;
            }
            let pid = list[0].product_id;
            let receiver = (0xC500..=0xC5FF).contains(&pid);
            let polled = (|| -> win::Result<()> {
                let mut link = Link::open(short, long)?;
                let indices: &[u8] = if receiver { &[1, 2, 3, 4, 5, 6] } else { &[0xFF] };
                for &dev in indices {
                    if let Some(reading) = self.query_slot(&mut link, &channel, dev, receiver, pid)? {
                        result.push(reading);
                    }
                }
                Ok(())
            })();
            if let Err(e) = polled {
                log::write(&format!("logitech {pid:04X}: {e}"));
            }
        }
        one_per_id(result, |r| r.online)
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        secs(60)
    }
}

/// The interface a collection belongs to: Windows numbers collections twice in the path,
/// ...&mi_02&col01#7&123&0&0000#{guid} and ...&col02#7&123&0&0001#{guid}; both numbers go.
pub fn channel_key(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    // Without every "&colNN"
    let mut no_col = String::with_capacity(lower.len());
    let mut rest = lower.as_str();
    while let Some(at) = rest.find("&col") {
        let after = &rest[at + 4..];
        let hex2 = after.len() >= 2 && after.as_bytes()[..2].iter().all(|b| b.is_ascii_hexdigit());
        if hex2 {
            no_col.push_str(&rest[..at]);
            rest = &after[2..];
        } else {
            no_col.push_str(&rest[..at + 4]);
            rest = after;
        }
    }
    no_col.push_str(rest);
    // Without the "&NNNN" in front of the closing "#{guid}"
    if no_col.ends_with('}') {
        if let Some(brace) = no_col.rfind("#{") {
            let guid = &no_col[brace + 2..no_col.len() - 1];
            let guid_ok = !guid.is_empty() && guid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
            if guid_ok && brace >= 5 {
                let number = &no_col[brace - 5..brace];
                if number.starts_with('&') && number.as_bytes()[1..].iter().all(|b| b.is_ascii_hexdigit()) {
                    return format!("{}{}", &no_col[..brace - 5], &no_col[brace..]);
                }
            }
        }
    }
    no_col
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

impl LogitechProvider {
    fn query_slot(
        &mut self,
        link: &mut Link,
        channel: &str,
        dev: u8,
        receiver: bool,
        pid: u16,
    ) -> win::Result<Option<DeviceReading>> {
        let key = format!("{channel}#{dev}");
        let mut dev = dev;
        let mut ping = link.ping(dev)?;
        if ping == Ping::NoDevice && !receiver {
            dev = 0x00; // a directly connected HID++ 1.0 device answers on index 0
            ping = link.ping(dev)?;
        }
        if ping == Ping::NoDevice {
            self.slots.remove(&key);
            return Ok(None);
        }

        if ping == Ping::Offline {
            return Ok(self
                .slots
                .get(&key)
                .filter(|s| s.has_battery())
                .map(|s| reading(s, receiver, None, false, Some(t("d.asleep")), false)));
        }

        let slot = match self.slots.get(&key) {
            Some(s) => s.clone(),
            None => {
                // Discovery that didn't get every answer (device fell asleep) is retried on the next poll
                let Some(s) = describe(link, dev, ping == Ping::Hidpp20, pid)? else { return Ok(None) };
                self.slots.insert(key.clone(), s.clone());
                s
            }
        };
        if !slot.has_battery() {
            return Ok(None);
        }

        let battery = if slot.hidpp20 { read_feature(link, dev, &slot)? } else { read_registers(link, dev)? };
        let Some(b) = battery else {
            if slot.hidpp20 {
                self.slots.remove(&key); // look the feature up again next time
            }
            return Ok(Some(reading(&slot, receiver, None, false, Some(t("d.readFailed")), false)));
        };
        if !b.online {
            return Ok(Some(reading(&slot, receiver, None, false, Some(t("d.asleep")), false)));
        }
        let level = if b.full { Some(b.level.unwrap_or(100)) } else { b.level };
        Ok(Some(reading(&slot, receiver, level, b.charging && !b.full, None, b.full)))
    }
}

fn reading(
    slot: &Slot,
    receiver: bool,
    level: Option<i32>,
    charging: bool,
    detail: Option<String>,
    full: bool,
) -> DeviceReading {
    DeviceReading {
        online: detail.is_none(),
        level,
        charging,
        full_on_charger: full,
        detail,
        ..DeviceReading::new(
            format!("logitech:{}", slug(&slot.name)),
            slot.name.clone(),
            slot.kind,
            t(if receiver { "via.24" } else { "via.cable" }),
        )
    }
}

fn describe(link: &mut Link, dev: u8, hidpp20: bool, pid: u16) -> win::Result<Option<Slot>> {
    let mut slot = Slot {
        name: format!("Logitech {pid:04X}{}", if (1..=6).contains(&dev) { format!(" #{dev}") } else { String::new() }),
        kind: DeviceKind::Other,
        hidpp20,
        feature_index: 0,
        feature_id: 0,
    };
    if !hidpp20 {
        return Ok(Some(slot));
    }

    let Some(name_index) = link.get_feature(dev, FEATURE_NAME)? else { return Ok(None) };
    if name_index != 0 {
        let Some(len) = link.call(dev, name_index, 0, &[])? else { return Ok(None) };
        let total = len[0] as usize;
        if total > 0 {
            let mut name = Vec::new();
            while name.len() < total {
                let Some(chunk) = link.call(dev, name_index, 1, &[name.len() as u8])? else { break };
                let part = &chunk[..16.min(total - name.len())];
                if part.is_empty() {
                    break;
                }
                name.extend_from_slice(part);
            }
            let text = String::from_utf8_lossy(&name).trim_end_matches('\0').trim().to_string();
            if !text.is_empty() {
                slot.name = if text.to_lowercase().starts_with("logitech") { text } else { format!("Logitech {text}") };
            }
        }
        if let Some(kind) = link.call(dev, name_index, 2, &[])? {
            slot.kind = match kind[0] {
                0 | 2 => DeviceKind::Keyboard,
                3..=5 => DeviceKind::Mouse,
                8 => DeviceKind::Headset,
                _ => DeviceKind::Other,
            };
        }
    }

    for feature in BATTERY_FEATURES {
        let Some(index) = link.get_feature(dev, feature)? else { return Ok(None) };
        if index == 0 {
            continue;
        }
        slot.feature_index = index;
        slot.feature_id = feature;
        break;
    }
    Ok(Some(slot))
}

fn read_feature(link: &mut Link, dev: u8, slot: &Slot) -> win::Result<Option<Battery>> {
    let b = |online, level, charging, full| Some(Battery { online, level, charging, full });
    Ok(match slot.feature_id {
        0x1004 => {
            // UnifiedBattery.GetStatus; status as in 0x1000 (Solaar): 1 recharging, 2 almost full, 3 full, 4 slow recharge
            let Some(r) = link.call(dev, slot.feature_index, 1, &[])? else { return Ok(None) };
            let level = if r[0] != 0 {
                Some(r[0] as i32)
            } else {
                match r[1] {
                    8 => Some(90),
                    4 => Some(50),
                    2 => Some(20),
                    1 => Some(5),
                    _ => None,
                }
            };
            b(true, level, matches!(r[2], 1 | 2 | 4), r[2] == 3)
        }
        0x1000 => {
            // BatteryStatus.GetBatteryLevelStatus
            let Some(r) = link.call(dev, slot.feature_index, 0, &[])? else { return Ok(None) };
            let level = if r[0] != 0 {
                Some(r[0] as i32)
            } else if r[2] == 3 {
                Some(100)
            } else {
                None
            };
            b(true, level, matches!(r[2], 1 | 2 | 4), r[2] == 3)
        }
        0x1001 => {
            // BatteryVoltage.GetBatteryInfo. Flags: bit 7 = on external power, then bits 0-1 = charge status, 1 = full
            let Some(r) = link.call(dev, slot.feature_index, 0, &[])? else { return Ok(None) };
            let powered = r[2] & 0x80 != 0;
            b(true, Some(voltage_to_percent((r[0] as i32) << 8 | r[1] as i32)), powered, powered && r[2] & 0x01 != 0)
        }
        0x1F20 => {
            // ADC measurement (headsets); bit 0 of the flags = headset active
            match link.call(dev, slot.feature_index, 0, &[])? {
                Some(r) if r[2] & 0x01 != 0 => {
                    b(true, Some(voltage_to_percent((r[0] as i32) << 8 | r[1] as i32)), r[2] & 0x02 != 0, false)
                }
                _ => b(false, None, false, false),
            }
        }
        _ => None,
    })
}

/// HID++ 1.0 battery registers: 0x0D (percent), then 0x07 (four levels).
fn read_registers(link: &mut Link, dev: u8) -> win::Result<Option<Battery>> {
    if let Some(charge) = link.read_register(dev, 0x0D)? {
        if (1..=100).contains(&charge[0]) {
            return Ok(Some(Battery {
                online: true,
                level: Some(charge[0] as i32),
                charging: charge[2] & 0xF0 == 0x50,
                full: charge[2] & 0xF0 == 0x90,
            }));
        }
    }
    if let Some(status) = link.read_register(dev, 0x07)? {
        let level = match status[0] {
            7 => Some(90),
            5 => Some(50),
            3 => Some(20),
            1 => Some(5),
            _ => None,
        };
        let charging = status[1] & 0x21 == 0x21;
        return Ok(Some(Battery { online: true, level, charging, full: !charging && status[1] & 0x22 == 0x22 }));
    }
    Ok(None)
}

/// Solaar's discharge curve for HID++ voltage readings (mV -> %), interpolated linearly.
const CURVE: [(i32, i32); 13] = [
    (4186, 100),
    (4067, 90),
    (3989, 80),
    (3922, 70),
    (3859, 60),
    (3811, 50),
    (3778, 40),
    (3751, 30),
    (3717, 20),
    (3671, 10),
    (3646, 5),
    (3579, 2),
    (3500, 0),
];

fn voltage_to_percent(mv: i32) -> i32 {
    if mv >= CURVE[0].0 {
        return 100;
    }
    for i in 1..CURVE.len() {
        if mv < CURVE[i].0 {
            continue;
        }
        let (hi_mv, hi_p) = CURVE[i - 1];
        let (lo_mv, lo_p) = CURVE[i];
        return lo_p + (mv - lo_mv) * (hi_p - lo_p) / (hi_mv - lo_mv);
    }
    0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ping {
    Hidpp20,
    Hidpp10,
    Offline,
    NoDevice,
}

/// One HID++ channel: the short and the long collection of the same interface.
struct Link {
    short: Option<HidDevice>,
    long: Option<HidDevice>,
    mark: u8,
}

fn same_device(r: &[u8], dev: u8) -> bool {
    r[1] == dev || r[1] == dev ^ 0xFF
}

impl Link {
    fn open(short: Option<&HidInfo>, long: Option<&HidInfo>) -> win::Result<Self> {
        let short = short.map(HidDevice::open).transpose()?;
        let long = long.map(HidDevice::open).transpose()?;
        Ok(Self { short, long, mark: 0x5A })
    }

    fn ping(&mut self, dev: u8) -> win::Result<Ping> {
        self.mark = self.mark.wrapping_add(1);
        let (mark, f) = (self.mark, 0x10 | SOFTWARE_ID);
        let r = self.request(&[0x10, dev, 0x00, f, 0, 0, mark], &|x| {
            x.len() >= 7
                && same_device(x, dev)
                && ((x[2] == 0x00 && x[3] == f && x[6] == mark) || (x[2] == 0x8F && x[3] == 0x00 && x[4] == f))
        })?;
        Ok(match r {
            None => Ping::Offline,
            Some(r) if r[2] == 0x00 => Ping::Hidpp20,
            Some(r) => match r[5] {
                0x01 => Ping::Hidpp10,
                0x08 => Ping::NoDevice,
                _ => Ping::Offline,
            },
        })
    }

    /// The feature's index, 0 when the device doesn't have it, None when it didn't answer.
    fn get_feature(&mut self, dev: u8, feature_id: u16) -> win::Result<Option<u8>> {
        Ok(self.call(dev, 0x00, 0, &[(feature_id >> 8) as u8, feature_id as u8])?.map(|r| r[0]))
    }

    /// A HID++ 2.0 function call; returns the 16 reply parameter bytes, or None on error/timeout.
    fn call(&mut self, dev: u8, feature_index: u8, function: u8, parameters: &[u8]) -> win::Result<Option<Vec<u8>>> {
        let f = (function << 4) | SOFTWARE_ID;
        let mut request = vec![0u8; 20];
        request[0] = 0x11;
        request[1] = dev;
        request[2] = feature_index;
        request[3] = f;
        let n = parameters.len().min(16);
        request[4..4 + n].copy_from_slice(&parameters[..n]);
        for _ in 0..3 {
            let r = self.request(&request, &|x| {
                x.len() >= 7
                    && same_device(x, dev)
                    && ((x[2] == feature_index && x[3] == f)
                        || (matches!(x[2], 0xFF | 0x8F) && x[3] == feature_index && x[4] == f))
            })?;
            let Some(r) = r else { return Ok(None) };
            if r[2] == feature_index {
                let mut params = r[4..r.len().min(20)].to_vec();
                params.resize(16, 0);
                return Ok(Some(params));
            }
            if !(r[2] == 0xFF && r[5] == 0x08) {
                return Ok(None); // an error other than "busy"
            }
        }
        Ok(None)
    }

    /// HID++ 1.0 short register read; returns the three value bytes.
    fn read_register(&mut self, dev: u8, register: u8) -> win::Result<Option<Vec<u8>>> {
        let r = self.request(&[0x10, dev, 0x81, register, 0, 0, 0], &|x| {
            x.len() >= 7
                && same_device(x, dev)
                && ((x[2] == 0x81 && x[3] == register) || (x[2] == 0x8F && x[3] == 0x81 && x[4] == register))
        })?;
        Ok(r.filter(|r| r[2] == 0x81).map(|r| r[4..7].to_vec()))
    }

    fn request(&mut self, request: &[u8], is_reply: &dyn Fn(&[u8]) -> bool) -> win::Result<Option<Vec<u8>>> {
        let mut request = request.to_vec();
        let target = if request[0] == 0x10 {
            match (&self.short, &self.long) {
                (Some(s), _) => s,
                (None, Some(l)) => {
                    // No short collection (some Bluetooth/direct devices): send the same request long
                    request.resize(20, 0);
                    request[0] = 0x11;
                    l
                }
                (None, None) => return Ok(None),
            }
        } else {
            match &self.long {
                Some(l) => l,
                None => return Ok(None),
            }
        };

        if let Some(s) = &self.short {
            s.flush();
        }
        if let Some(l) = &self.long {
            l.flush();
        }
        if !target.write(&request, 500)? {
            return Ok(None);
        }

        // Replies can come on either collection; each keeps its own queue, so polling them in turn loses nothing
        let deadline = Instant::now() + Duration::from_millis(TIMEOUT_MS);
        while Instant::now() < deadline {
            for d in [&self.long, &self.short].into_iter().flatten() {
                if let Some(r) = d.read(15)? {
                    if is_reply(&r) {
                        return Ok(Some(r));
                    }
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collections_of_one_interface_share_a_channel() {
        let a = r"\\?\HID#VID_046D&PID_C539&MI_02&Col01#8&2B1A6F3&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let b = r"\\?\HID#VID_046D&PID_C539&MI_02&Col02#8&2B1A6F3&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let c = r"\\?\HID#VID_046D&PID_C539&MI_01#8&2B1A6F3&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(channel_key(a), channel_key(b));
        assert_ne!(channel_key(a), channel_key(c));
        assert_eq!(
            channel_key(a),
            r"\\?\hid#vid_046d&pid_c539&mi_02#8&2b1a6f3&0#{4d1e55b2-f16f-11cf-88cb-001111000030}"
        );
    }

    #[test]
    fn id_from_the_name() {
        assert_eq!(slug("Logitech G502 X PLUS"), "logitech-g502-x-plus");
        assert_eq!(slug("Logitech MX Master 3S (Bolt)"), "logitech-mx-master-3s-bolt");
    }

    #[test]
    fn solaar_curve() {
        assert_eq!(voltage_to_percent(4200), 100);
        assert_eq!(voltage_to_percent(3811), 50);
        assert_eq!(voltage_to_percent(3400), 0);
    }
}
