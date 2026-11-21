//! 2.4 GHz receivers of the Rongyuan (RY) platform, used by Attack Shark, Royuan and many other brands.
//! The receiver keeps the battery level of its paired mouse and keyboard, and
//! one status command returns both:
//!   SetFeature [00 F7 00 ...] on interface 2 (vendor page 0xFFFF, usage 2, 64-byte feature report), then
//!   GetFeature -> [00 canRead kbBattery mouseBattery kbOffline mouseOffline canSend ...] (offline: 0 = connected).
//! Taken from the official ATTACK SHARK web driver (qmk.top), which sends it every 5 s; verified on the Attack Shark
//! R3 receiver 3151:402D. Akko's web driver reads its receivers the same way. A read-only status query.
//! On a USB cable the mouse leaves the receiver (it reports the mouse offline) and shows up as its own USB device,
//! which charges it but has no battery query, so a mouse on the cable is shown as charging with no level.
//! A keyboard in 2.4 GHz mode stays on the receiver while its cable charges it; the X82 PRO HE then reports a level
//! 20-30% too high and nothing marks the cable (issue #5), so the level is shown as it comes.

use super::{key, receiver_name, secs, sleep_ms, Provider};
use crate::hid::{self, HidDevice, HidInfo};
use crate::loc::{t, tf};
use crate::log;
use crate::reading::{DeviceKind, DeviceReading};
use crate::state::StateStore;
use crate::win;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

const STATUS_COMMAND: u8 = 0xF7;

/// (vid << 16) | pid of every receiver the web driver drives with this protocol.
const RECEIVERS: &[u32] = &[
    0x0DB0197E, 0x0DB08E9A, 0x145F0376, 0x145F038D, 0x14A5C045, 0x25AA2008, 0x25AA4008, 0x25AA4018, 0x31514011,
    0x31514014, 0x31514017, 0x31514021, 0x31514027, 0x3151402D, 0x31514038, 0x31515006, 0x31515007, 0x31515008,
    0x31515020, 0x31515026, 0x3151502B, 0x31515031, 0x31515037, 0x31515038, 0x3151503D, 0x3151503F, 0x31515040,
    0x31515042, 0x31515045, 0x3151504B, 0x3151504E, 0x31515050, 0x31515052, 0x31515055, 0x3151505A, 0x342DE3E0,
    0x342DE3E1, 0x342DE3E2, 0x342DE3E6, 0x342DE3E7, 0x342DE3E8, 0x342DE3F5, 0x342DE3F6, 0x342DE3F7, 0x342DE3FB,
    0x342DE3FC, 0x342DE3FD, 0x347AA302, 0x347AA305, 0x374AA205, 0x374AA217, 0x374AA220, 0x374AA223, 0x374AA226,
    0x374AA229, 0x374AA234, 0x374AA237, 0x374AA251, 0x374AA263, 0x374AA268, 0x374AA311, 0x374AA314, 0x374AA319,
    0x374AA322, 0x374AA329, 0x374AA355, 0x379A1804, 0x379A1807, 0x38A90002, 0x38EE0000, 0x38EE0002, 0x38EE0039,
    0x391D1A04, 0x391D1A08, 0x391D1A0E, 0x391D1A13, 0x391D1A25, 0x391D1A2B, 0x391D1A34, 0x391D2017, 0x391D2A05,
    0x39AB7000, 0x39AB7003, 0x39AB7006, 0x39AB7007, 0x39AB7008, 0x39AB7009, 0x38EE0004,
];

/// (vid << 16) | pid of the mice the web driver drives on a USB cable.
const WIRED_MICE: &[u32] = &[
    0x3151400B, 0x31514026, 0x3151402A, 0x31515032, 0x31515035, 0x3151503A, 0x31515043, 0x31515058, 0x39AB6001,
    0x25AA4005, 0x25AA4006, 0x25AA4007, 0x25AA4019, 0x347AA301, 0x347AA304, 0x374AA310, 0x374AA313, 0x374AA318,
    0x374AA321, 0x374AA328, 0x374AA354, 0x391D1004, 0x391D100F, 0x391D1008, 0x391D103A, 0x391D1015, 0x391D1028,
    0x391D102F, 0x38A90019, 0x38A90021, 0x379A3357, 0x38EE0038, 0x39AB6003, 0x39AB6007, 0x39AB6009,
];

/// Receivers known to belong to a mouse: their mouse slot is listed even while it's offline. The name is used when
/// the receiver's product string names nobody: several brands share 3151:402D, and the status reply doesn't say which
/// mouse it is. The R3's receiver calls itself "2.4G Wireless Mouse", the AJAZZ AJ179 Pro's "AJAZZ 2.4G 8K" (issue
/// #22).
const KNOWN_MICE: &[(u32, &str)] = &[(0x3151402D, "Attack Shark R3")];

/// How many different status replies a receiver logs per session, in case some byte changes all the time.
const LOGGED_REPLIES_MAX: usize = 16;

pub struct RongyuanDongleProvider {
    state: Arc<StateStore>,
    seen_online: HashSet<String>,
    logged_replies: HashMap<String, HashSet<String>>, // by collection path
}

impl RongyuanDongleProvider {
    pub fn new(state: Arc<StateStore>) -> Self {
        Self { state, seen_online: HashSet::new(), logged_replies: HashMap::new() }
    }

    /// Adds the slot's reading; false when the slot isn't listed.
    #[allow(clippy::too_many_arguments)]
    fn add_slot(
        &mut self,
        result: &mut Vec<DeviceReading>,
        id: String,
        name: String,
        kind: DeviceKind,
        status: Option<&[u8]>,
        level_at: usize,
        offline_at: usize,
        error: Option<&str>,
        always_list: bool,
    ) -> bool {
        let online = status.is_some_and(|s| s[offline_at] == 0);
        if online {
            self.seen_online.insert(id.clone());
        } else if !always_list && !self.seen_online.contains(&id) && self.state.get(&id).is_none() {
            // A receiver has a mouse and a keyboard slot; only list the ones that were ever used
            return false;
        }

        // 0 right after wake-up means "not reported yet", not an empty battery
        let level = status.filter(|_| online).map(|s| s[level_at] as i32).filter(|l| (1..=100).contains(l));
        // The receiver answering with "offline" is the normal state of a mouse that went to sleep
        let detail = if online {
            level.is_none().then(|| t("d.levelUnknown"))
        } else {
            Some(error.map(str::to_string).unwrap_or_else(|| t("d.asleep")))
        };
        result.push(DeviceReading { online, level, detail, ..DeviceReading::new(id, name, kind, t("via.24")) });
        true
    }

    /// A mouse on the cable is the receiver's mouse that went offline when the cable went in, so it takes over that
    /// slot. Without such a slot it is left out: the same IDs cover wired-only mice, with no battery.
    fn add_wired_mice(&self, result: &mut [DeviceReading], receiver_mice: &mut Vec<usize>) {
        for info in hid::enumerate(|v, p| WIRED_MICE.contains(&key(v, p))) {
            if info.interface_number() != 2 {
                continue;
            }
            if !info.access_denied && (info.usage_page != 0xFFFF || info.usage != 2) {
                continue;
            }
            let Some(slot) = receiver_mice.iter().position(|&i| !result[i].online) else { continue };
            let at = receiver_mice.remove(slot);
            let r = &mut result[at];
            let before = self.state.get(&r.id).and_then(|s| s.level);
            r.via = t("via.cable");
            r.online = true;
            r.charging = true;
            r.level = None;
            r.detail = Some(match before {
                Some(was) => format!("{} · {}", t("d.cableHidden"), tf("d.wasBefore", &[&was])),
                None => t("d.cableHidden"),
            });
        }
    }

    /// Logs each different status reply of a receiver once per session, all of it. Only the first 7 bytes are
    /// known, and on an untested receiver the rest may hold what isn't read yet, such as a charging flag. The two
    /// levels don't make a reply different - they change by themselves and have log lines of their own.
    fn log_reply(&mut self, info: &HidInfo, reply: &[u8]) {
        let seen = self.logged_replies.entry(info.path.clone()).or_default();
        if seen.len() >= LOGGED_REPLIES_MAX {
            return;
        }
        let mut masked = reply.to_vec();
        masked[2] = 0;
        masked[3] = 0;
        if !seen.insert(super::hex(&masked)) {
            return;
        }
        let used = 7.max(reply.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1)).min(reply.len());
        let rest = if used < reply.len() { format!(", then {} zero bytes", reply.len() - used) } else { String::new() };
        let last = if seen.len() == LOGGED_REPLIES_MAX { " (the last one logged this session)" } else { "" };
        log::write(&format!(
            "Rongyuan {:04x}:{:04x}: status reply {}{rest}{last}",
            info.vendor_id,
            info.product_id,
            super::hex(&reply[..used])
        ));
    }
}

fn query_status(info: &HidInfo) -> win::Result<Option<Vec<u8>>> {
    let dev = HidDevice::open_sync(info)?;
    for attempt in 0..3 {
        if attempt > 0 {
            sleep_ms(100);
        }
        let mut command = vec![0u8; info.feature_length];
        command[1] = STATUS_COMMAND;
        if !dev.set_feature(&command) {
            return Ok(None);
        }
        sleep_ms(20);
        // All zeros would mean both devices online at 0% - the receiver just hasn't filled the reply yet
        if let Some(reply) = dev.get_feature(0) {
            if reply.len() >= 7 && (reply[2] | reply[3] | reply[4] | reply[5] | reply[6]) != 0 {
                return Ok(Some(reply));
            }
        }
    }
    Ok(None)
}

impl Provider for RongyuanDongleProvider {
    fn name(&self) -> &'static str {
        "Rongyuan 2.4G"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        let mut receiver_mice = Vec::new(); // indexes in result of the receivers' mouse slots
        for info in hid::enumerate(|v, p| RECEIVERS.contains(&key(v, p))) {
            if info.interface_number() != 2 {
                continue;
            }
            if !info.access_denied && (info.usage_page != 0xFFFF || info.usage != 2 || info.feature_length < 7) {
                continue;
            }

            let k = key(info.vendor_id, info.product_id);
            let base_id = format!("ry:{:04x}:{:04x}", info.vendor_id, info.product_id);
            let mut status = None;
            let mut error = None;
            if info.access_denied {
                error = Some(t("d.needsAdmin"));
            } else {
                match query_status(&info) {
                    Ok(Some(reply)) => {
                        self.log_reply(&info, &reply);
                        status = Some(reply);
                    }
                    Ok(None) => error = Some(t("d.receiverSilent")),
                    Err(e) => error = Some(tf("d.error", &[&e])),
                }
            }

            let mouse_name = KNOWN_MICE.iter().find(|m| m.0 == k).map(|m| m.1);
            let name = receiver_name::mouse(info.product.as_deref(), mouse_name);
            if self.add_slot(
                &mut result,
                format!("{base_id}:mouse"),
                name,
                DeviceKind::Mouse,
                status.as_deref(),
                3,
                5,
                error.as_deref(),
                mouse_name.is_some(),
            ) {
                receiver_mice.push(result.len() - 1);
            }
            self.add_slot(
                &mut result,
                format!("{base_id}:keyboard"),
                t("name.keyboard"),
                DeviceKind::Keyboard,
                status.as_deref(),
                2,
                4,
                error.as_deref(),
                false,
            );
        }
        self.add_wired_mice(&mut result, &mut receiver_mice);
        result
    }

    fn delay(&self, last: &[DeviceReading]) -> Duration {
        if last.iter().any(|r| r.online) {
            secs(30)
        } else {
            secs(15)
        }
    }
}
