//! The levels macOS knows: what `system_profiler SPBluetoothDataType` reports for the connected devices, which is
//! what the Bluetooth menu shows - a mouse's or keyboard's own level, a headset's, the two earbuds and the case of
//! AirPods. A headset that reports its level over the hands-free profile (as Windows reads it) has none there: macOS
//! keeps that one among its power sources, the accessories the battery widget and `pmset -g accps` show, under the
//! device's name. A paired device that isn't connected has no level; once this session has had one from it, it is
//! listed as offline, as Windows lists a device whose level it still holds.

use super::kind_by_minor_type;
use crate::loc::tf;
use crate::log;
use crate::mac::{items_of, number_of, string_of, value_of, CFTypeRef, Cf};
use crate::reading::DeviceReading;
use serde_json::Value;
use std::collections::HashSet;
use std::ffi::{c_char, c_void};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The devices this session has had a level from.
static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The connected devices macOS had a level for at the last look, and when that look began: for the providers that
/// could read one of them themselves and leave it to this one (`level_shown`).
static SHOWN: Mutex<Option<(Instant, Vec<Shown>)>> = Mutex::new(None);

pub fn poll() -> Vec<DeviceReading> {
    // Held while system_profiler runs, so a provider asking meanwhile waits for this look instead of making its own
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    let began = Instant::now();
    let Some(report) = system_profiler() else { return Vec::new() };
    let accessories = accessories();
    *shown = Some((began, levels_shown(&report, &accessories)));
    drop(shown);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let seen = seen.get_or_insert_with(HashSet::new);
    let readings = readings(&report, &accessories, seen);
    for r in &readings {
        if r.level.is_some() {
            seen.insert(r.id.clone());
        }
    }
    readings
}

/// Whether macOS shows a level for the connected device with these IDs or this name, so that this provider lists it.
/// The last look is taken if it began at `since` or later (this provider looks every minute), else one is made now.
pub fn level_shown(vendor_id: u16, product_id: u16, name: Option<&str>, since: Instant) -> bool {
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    if !shown.as_ref().is_some_and(|(began, _)| *began >= since) {
        let began = Instant::now();
        *shown = system_profiler().map(|report| (began, levels_shown(&report, &accessories())));
    }
    shown.as_ref().is_some_and(|(_, list)| list.iter().any(|s| s.is(vendor_id, product_id, name)))
}

/// A connected device macOS has a level for: the vendor and product IDs it reports for it, and its name.
#[derive(Debug, PartialEq)]
struct Shown {
    ids: Option<(u16, u16)>,
    name: String,
}

impl Shown {
    fn is(&self, vendor_id: u16, product_id: u16, name: Option<&str>) -> bool {
        self.ids == Some((vendor_id, product_id)) || name.is_some_and(|n| n.eq_ignore_ascii_case(&self.name))
    }
}

/// The connected devices of the report that have a level, as `readings` lists them.
fn levels_shown(report: &Value, accessories: &[Accessory]) -> Vec<Shown> {
    let controllers = report.get("SPBluetoothDataType").and_then(Value::as_array).into_iter().flatten();
    let devices = controllers.flat_map(|c| c.get("device_connected").and_then(Value::as_array).into_iter().flatten());
    let id = |device: &Value, key: &str| {
        let text = device.get(key).and_then(Value::as_str)?;
        u16::from_str_radix(text.trim().trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
    };
    devices
        .filter_map(Value::as_object)
        .flat_map(|o| o.iter())
        .filter(|(name, device)| {
            Battery::of(device).level().or_else(|| Battery::among(accessories, name).0.level()).is_some()
        })
        .map(|(name, device)| Shown {
            ids: id(device, "device_vendorID").zip(id(device, "device_productID")),
            name: name.clone(),
        })
        .collect()
}

/// What system_profiler reports, as JSON; None when it couldn't be run or said nothing in 20 seconds.
fn system_profiler() -> Option<Value> {
    let mut child = Command::new("/usr/sbin/system_profiler")
        .args(["SPBluetoothDataType", "-json", "-detailLevel", "basic"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| log::write(&format!("bluetooth: system_profiler: {e}")))
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).map(|_| text)
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().ok()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            log::write("bluetooth: system_profiler didn't answer in 20 s");
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let text = reader.join().ok()?.ok()?;
    serde_json::from_str(&text).ok()
}

/// A level among macOS's power sources: an accessory's, or one part's of it (an earbud, the case).
#[derive(Debug, Clone, PartialEq)]
pub struct Accessory {
    pub name: String,
    /// "Single" for the device itself, "Left", "Right", "Case" for the parts of earbuds
    pub part: String,
    pub level: i32,
    pub charging: bool,
}

/// The accessories macOS has a level for. The list comes from IOKit's IOPSCopyPowerSourcesByType, which isn't in the
/// SDK headers: it is looked up when the app runs, so a macOS without it only goes without these levels.
fn accessories() -> Vec<Accessory> {
    type ByType = unsafe extern "C" fn(kind: i32) -> CFTypeRef;
    // kIOPSSourceAll in IOPowerSourcesPrivate.h: the accessories, and also the HID devices with a battery that macOS
    // files as a UPS (ioupsd does so for those that aren't an Apple accessory, a peripheral or a controller)
    const ALL: i32 = 0;
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
    let function = unsafe { dlsym(RTLD_DEFAULT, c"IOPSCopyPowerSourcesByType".as_ptr()) };
    if function.is_null() {
        return Vec::new();
    }
    let by_type: ByType = unsafe { std::mem::transmute::<*mut c_void, ByType>(function) };
    let Some(blob) = Cf::new(unsafe { by_type(ALL) }) else { return Vec::new() };
    let Some(list) = Cf::new(unsafe { IOPSCopyPowerSourcesList(blob.0) }) else { return Vec::new() };
    let mut result = Vec::new();
    for source in items_of(list.0) {
        // Owned by the blob
        let description = unsafe { IOPSGetPowerSourceDescription(blob.0, source) };
        let text = |key| string_of(value_of(description, key));
        let number = |key| number_of(value_of(description, key));
        let (Some(name), Some(current), Some(max)) = (text("Name"), number("Current Capacity"), number("Max Capacity"))
        else {
            continue;
        };
        // The Mac's own battery isn't a device of the list
        if max <= 0 || number("Is Present") == Some(0) || text("Type").as_deref() == Some("InternalBattery") {
            continue;
        }
        let level = ((current * 100 + max / 2) / max).clamp(0, 100) as i32;
        let part = text("Part Identifier").unwrap_or_else(|| "Single".into());
        result.push(Accessory { name, part, level, charging: number("Is Charging").unwrap_or(0) != 0 });
    }
    result
}

/// The connected devices that report a level, and the devices `seen` before that are no longer connected.
pub fn readings(report: &Value, accessories: &[Accessory], seen: &HashSet<String>) -> Vec<DeviceReading> {
    let mut result = Vec::new();
    let controllers = report.get("SPBluetoothDataType").and_then(Value::as_array).cloned().unwrap_or_default();
    for controller in &controllers {
        for (list, connected) in [("device_connected", true), ("device_not_connected", false)] {
            let devices = controller.get(list).and_then(Value::as_array).cloned().unwrap_or_default();
            // Each entry is an object with one key: the device's name
            for (name, device) in devices.iter().filter_map(Value::as_object).flat_map(|o| o.iter()) {
                let Some(address) = device.get("device_address").and_then(Value::as_str).and_then(parse_address) else {
                    continue;
                };
                let id = format!("bt:{address:012X}");
                let mut battery = Battery::of(device);
                let mut charging = false;
                if connected && battery.level().is_none() {
                    (battery, charging) = Battery::among(accessories, name);
                }
                if connected && battery.level().is_none() {
                    continue; // nothing to show: a phone, a speaker without a level
                }
                if !connected && !seen.contains(&id) {
                    continue;
                }
                let kind = kind_by_minor_type(device.get("device_minorType").and_then(Value::as_str), name);
                result.push(DeviceReading {
                    online: connected,
                    level: battery.level().filter(|_| connected),
                    detail: battery.detail().filter(|_| connected),
                    charging,
                    ..DeviceReading::new(id, name.clone(), kind, "Bluetooth")
                });
            }
        }
    }
    result
}

/// "AA:BB:CC:DD:EE:FF" (or with dashes) as a number.
pub fn parse_address(text: &str) -> Option<u64> {
    let digits: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (digits.len() == 12).then(|| u64::from_str_radix(&digits, 16).ok()).flatten()
}

/// A device's levels: its own, or those of the two earbuds and their case.
#[derive(Default, Debug, PartialEq)]
struct Battery {
    main: Option<i32>,
    left: Option<i32>,
    right: Option<i32>,
    case: Option<i32>,
}

impl Battery {
    fn of(device: &Value) -> Self {
        let percent = |key: &str| {
            let text = device.get(key).and_then(Value::as_str)?;
            let n = text.trim().trim_end_matches('%').trim().parse::<i32>().ok()?;
            (0..=100).contains(&n).then_some(n)
        };
        Self {
            main: percent("device_batteryLevelMain"),
            left: percent("device_batteryLevelLeft"),
            right: percent("device_batteryLevelRight"),
            case: percent("device_batteryLevelCase"),
        }
    }

    /// The levels macOS keeps for the accessory of that name, and whether it is charging.
    fn among(accessories: &[Accessory], name: &str) -> (Self, bool) {
        let mut battery = Self::default();
        let mut charging = false;
        for a in accessories.iter().filter(|a| a.name == name) {
            let slot = match a.part.to_ascii_lowercase().as_str() {
                "left" => &mut battery.left,
                "right" => &mut battery.right,
                "case" => &mut battery.case,
                _ => {
                    charging = a.charging;
                    &mut battery.main
                }
            };
            *slot = Some(a.level);
        }
        (battery, charging)
    }

    /// The device's own level, or that of the earbud that runs out first.
    fn level(&self) -> Option<i32> {
        self.main.or(match (self.left, self.right) {
            (Some(l), Some(r)) => Some(l.min(r)),
            (one, other) => one.or(other),
        })
    }

    /// The earbuds and the case, each with its level.
    fn detail(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(l) = self.left {
            parts.push(tf("d.budLeft", &[&l]));
        }
        if let Some(r) = self.right {
            parts.push(tf("d.budRight", &[&r]));
        }
        if let Some(c) = self.case {
            parts.push(tf("d.case", &[&c]));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFTypeRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, source: CFTypeRef) -> CFTypeRef;
}

extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::DeviceKind;

    fn report() -> Value {
        serde_json::json!({ "SPBluetoothDataType": [{
            "controller_properties": { "controller_address": "BC:D0:74:00:00:01" },
            "device_connected": [
                { "MX Ergo": { "device_address": "D1:2C:26:38:38:90", "device_batteryLevelMain": "85%",
                    "device_minorType": "Mouse" } },
                { "AirPods Pro": { "device_address": "70:AE:D5:00:00:02", "device_batteryLevelLeft": "100%",
                    "device_batteryLevelRight": "90%", "device_batteryLevelCase": "45%",
                    "device_minorType": "Headphones" } },
                { "iPhone": { "device_address": "B4:55:75:00:00:03", "device_rssi": "-50" } },
                { "Razer Barracuda X (BT)": { "device_address": "44:5E:CD:52:31:8B", "device_minorType": "Headset",
                    "device_services": "0x800019 < HFP AVRCP A2DP ACL >" } },
                // The keyboard of issue #52 as its owner's Mac lists it: a mouse by its type, and no level of its own
                { "MX Keys": { "device_address": "E4:8C:73:00:00:06", "device_vendorID": "0x046D",
                    "device_productID": "0xB35B", "device_minorType": "Mouse",
                    "device_services": "0x400020 < HID BLE >" } }
            ],
            "device_not_connected": [
                { "Keychron K6": { "device_address": "DC:2C:26:00:00:04", "device_minorType": "Keyboard" } },
                { "Xbox Wireless Controller": { "device_address": "AC:8E:BD:00:00:05", "device_minorType": "Gamepad" } }
            ]
        }]})
    }

    fn accessory(name: &str, part: &str, level: i32, charging: bool) -> Accessory {
        Accessory { name: name.into(), part: part.into(), level, charging }
    }

    #[test]
    fn levels_of_the_connected_devices() {
        let seen = HashSet::from(["bt:DC2C26000004".to_string()]);
        let accessories = [
            accessory("Razer Barracuda X (BT)", "Single", 40, true),
            // system_profiler's own levels come first
            accessory("MX Ergo", "Single", 10, false),
            // A device that isn't connected shows no level
            accessory("Keychron K6", "Single", 50, false),
            accessory("MX Keys", "Single", 60, false),
        ];
        let all = readings(&report(), &accessories, &seen);
        let shown: Vec<_> =
            all.iter().map(|r| (r.id.as_str(), r.name.as_str(), r.kind, r.online, r.level, r.charging)).collect();
        assert_eq!(
            shown,
            [
                ("bt:D12C26383890", "MX Ergo", DeviceKind::Mouse, true, Some(85), false),
                ("bt:70AED5000002", "AirPods Pro", DeviceKind::Headset, true, Some(90), false),
                ("bt:445ECD52318B", "Razer Barracuda X (BT)", DeviceKind::Headset, true, Some(40), true),
                // A keyboard by its name, a mouse by the type macOS gives it; its level is among the power sources
                ("bt:E48C73000006", "MX Keys", DeviceKind::Keyboard, true, Some(60), false),
                // Not connected: only the one this session had a level from
                ("bt:DC2C26000004", "Keychron K6", DeviceKind::Keyboard, false, None, false),
            ]
        );
        assert!(readings(&report(), &[], &seen).iter().all(|r| !r.name.contains("Barracuda")));
        assert_eq!(all[1].detail.as_deref(), Some("left 100%, right 90%, case 45%"));
        assert_eq!(all[0].detail, None);
        assert!(all.iter().all(|r| r.via == "Bluetooth"));
    }

    /// The devices other providers leave to this one: connected, with a level of macOS's own or among its power
    /// sources, known by their IDs or their name.
    #[test]
    fn the_levels_macos_shows() {
        let mut report = report();
        report["SPBluetoothDataType"][0]["device_connected"][0]["MX Ergo"]["device_vendorID"] = "0x046D".into();
        report["SPBluetoothDataType"][0]["device_connected"][0]["MX Ergo"]["device_productID"] = "0xB01D".into();
        let accessories = [accessory("Razer Barracuda X (BT)", "Single", 40, true)];
        let shown = levels_shown(&report, &accessories);
        let names: Vec<_> = shown.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["MX Ergo", "AirPods Pro", "Razer Barracuda X (BT)"]);
        assert_eq!(shown[0].ids, Some((0x046D, 0xB01D)));
        assert!(shown[0].is(0x046D, 0xB01D, None), "by its IDs, also when its name is unknown");
        assert!(shown[0].is(0x046D, 0xB023, Some("mx ergo")), "by its name");
        assert!(!shown[0].is(0x046D, 0xB023, Some("MX Master 3S")));
        assert!(shown[2].is(0x1532, 0x0544, Some("Razer Barracuda X (BT)")));
        assert!(!shown[2].is(0x1532, 0x0544, None));
        // Not connected, or connected without a level: not shown
        assert!(!shown.iter().any(|s| s.name == "Keychron K6" || s.name == "iPhone"));
    }

    #[test]
    fn one_earbud_and_odd_values() {
        let one = Battery::of(&serde_json::json!({ "device_batteryLevelRight": "70 %" }));
        assert_eq!((one.level(), one.right), (Some(70), Some(70)));
        let odd =
            Battery::of(&serde_json::json!({ "device_batteryLevelMain": "120%", "device_batteryLevelCase": "x" }));
        assert_eq!(odd, Battery::default());
        assert_eq!(parse_address("AC-8E-BD-00-00-05"), Some(0xAC8EBD000005));
        assert_eq!(parse_address("AC:8E"), None);
        assert!(readings(&serde_json::json!({}), &[], &HashSet::new()).is_empty());
    }

    #[test]
    fn earbuds_among_the_power_sources() {
        let accessories = [
            accessory("Buds", "Left", 80, false),
            accessory("Buds", "Right", 60, false),
            accessory("Buds", "Case", 30, true),
            accessory("Other", "Single", 10, false),
        ];
        let (battery, charging) = Battery::among(&accessories, "Buds");
        assert_eq!(battery, Battery { main: None, left: Some(80), right: Some(60), case: Some(30) });
        assert_eq!((battery.level(), charging), (Some(60), false), "the case charging isn't the earbuds charging");
        assert_eq!(Battery::among(&accessories, "Nothing"), (Battery::default(), false));
    }

    /// The real thing, on this Mac.
    #[test]
    fn this_macs_devices() {
        for a in accessories() {
            println!("{a:?}");
        }
        for r in poll() {
            println!("{r}");
        }
    }
}
