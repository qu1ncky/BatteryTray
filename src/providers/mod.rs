//! The battery protocols, one provider per family of devices.

pub mod ajazz_mouse;
pub mod aula_mchose;
pub mod barracuda;
pub mod bluetooth;
pub mod devices;
pub mod headset;
pub mod logitech;
pub mod oem_mouse;
pub mod playstation;
pub mod razer_peripheral;
pub mod receiver_name;
pub mod rongyuan;
pub mod sonix_keyboard;
pub mod standard_hid;
pub mod steelseries;
pub mod xenta;
pub mod xinput;

use crate::hid::HidInfo;
use crate::monitor::Pusher;
use crate::reading::DeviceReading;
use crate::state::StateStore;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub trait Provider: Send {
    fn name(&self) -> &'static str;

    /// Queries the hardware (blocking; called on a worker thread) and returns every device this provider
    /// currently knows, online or not.
    fn poll(&mut self) -> Vec<DeviceReading>;

    /// How long to wait before the next poll, given what the last one returned.
    fn delay(&self, last: &[DeviceReading]) -> Duration;

    /// For providers whose devices also report changes by themselves: how to ask for a poll right away.
    fn set_pusher(&mut self, _push: Pusher) {}
}

/// Every provider, in the order the C# app polls them.
pub fn create(state: &Arc<StateStore>) -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(barracuda::RazerBarracudaProvider::new(state.clone())),
        Box::new(rongyuan::RongyuanDongleProvider::new(state.clone())),
        Box::new(xenta::XentaReceiverProvider::new(state.clone())),
        Box::new(steelseries::SteelSeriesProvider::default()),
        Box::new(oem_mouse::OemMouseProvider::default()),
        Box::new(ajazz_mouse::AjazzMouseProvider::default()),
        Box::new(aula_mchose::AulaMchoseProvider::default()),
        Box::new(sonix_keyboard::SonixKeyboardProvider::default()),
        Box::new(razer_peripheral::RazerPeripheralProvider),
        Box::new(devices::TableProvider::headsets().remembering(state.clone())),
        Box::new(devices::razer_audio::BlackSharkV3Provider::default()),
        Box::new(devices::usb_headsets::Pulse3dProvider::default()),
        Box::new(devices::TableProvider::peripherals().remembering(state.clone())),
        // After the tables: for an Id both give, the list takes the reading of the later provider, and a device
        // read behind a shared receiver is to win over its own receiver sitting idle
        Box::new(devices::asus::AsusReceiverProvider::default()),
        Box::new(devices::corsair::CorsairProvider::default()),
        Box::new(devices::keyboards::KeyboardProvider::new(state.clone())),
        Box::new(devices::mice_platforms::KeychronLinkProvider::default()),
        Box::new(devices::passive_mice::PassiveMiceProvider::default()),
        Box::new(devices::controllers::SteamControllerProvider::default()),
        Box::new(logitech::LogitechProvider::default()),
        Box::new(playstation::PlayStationProvider),
        Box::new(xinput::XInputProvider),
        Box::new(bluetooth::BluetoothProvider),
        Box::new(standard_hid::StandardHidBatteryProvider),
    ]
}

/// Clears a "running" flag when it goes out of scope, however the scope is left.
pub(crate) struct ClearOnDrop<'a>(pub &'a std::sync::atomic::AtomicBool);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

pub(crate) fn key(vid: u16, pid: u16) -> u32 {
    ((vid as u32) << 16) | pid as u32
}

/// The text key for a device whose collection wasn't found among `collections`. A collection that Windows won't open
/// comes with no usage and no report lengths, so it never matches what a provider looks for. When there is one, it is
/// most likely the one that's missing, and administrator rights open it.
pub(crate) fn no_link_reason(collections: &[HidInfo]) -> &'static str {
    if collections.iter().any(|i| i.access_denied) {
        "d.needsAdmin"
    } else {
        "d.noLink"
    }
}

/// Milliseconds left until `deadline`, at least 1.
pub(crate) fn remaining_ms(deadline: Instant) -> u32 {
    (deadline.saturating_duration_since(Instant::now()).as_millis() as u32).max(1)
}

/// Bytes as "0a 1b 2c".
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// A report for the log: without the zeros that pad it, and no more than its first 32 bytes.
pub(crate) fn hex_trimmed(report: &[u8]) -> String {
    let used = report.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    if used == 0 {
        return "00".into();
    }
    let text = hex(&report[..used.min(32)]);
    if used > 32 {
        text + " ..."
    } else {
        text
    }
}

/// Groups items by a key, keeping the order in which each key first appears (as LINQ's GroupBy).
pub(crate) fn group_by<K: PartialEq>(items: Vec<HidInfo>, key: impl Fn(&HidInfo) -> K) -> Vec<(K, Vec<HidInfo>)> {
    let mut groups: Vec<(K, Vec<HidInfo>)> = Vec::new();
    for item in items {
        let k = key(&item);
        match groups.iter_mut().find(|(g, _)| *g == k) {
            Some((_, list)) => list.push(item),
            None => groups.push((k, vec![item])),
        }
    }
    groups
}

/// One reading per Id: the first one `better` accepts, else the first one (order of first appearance kept).
pub(crate) fn one_per_id(readings: Vec<DeviceReading>, better: impl Fn(&DeviceReading) -> bool) -> Vec<DeviceReading> {
    let mut result: Vec<DeviceReading> = Vec::new();
    for r in readings {
        match result.iter_mut().find(|o| o.id == r.id) {
            Some(other) => {
                if better(&r) && !better(other) {
                    *other = r;
                }
            }
            None => result.push(r),
        }
    }
    result
}

/// The report padded with zeros to `length` (never cut).
pub(crate) fn pad(length: usize, bytes: &[u8]) -> Vec<u8> {
    let mut report = vec![0u8; length.max(bytes.len())];
    report[..bytes.len()].copy_from_slice(bytes);
    report
}

pub(crate) fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

pub(crate) fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}
