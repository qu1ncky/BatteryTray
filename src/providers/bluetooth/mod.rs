//! Bluetooth devices whose battery level the system itself knows: on Windows the level in Settings > Bluetooth &
//! devices, on macOS the one the Bluetooth menu shows. Their IDs are "bt:" and the device address on both. On Windows
//! a connected device's own Battery Services and AirPods' advertisements tell more, where the device has them.

#[cfg(windows)]
mod advertisements;
#[cfg(target_os = "macos")]
mod macos;
// Pure, so the Mac runs its tests too
#[cfg(any(windows, test))]
mod parts;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(windows)]
use windows as imp;

#[cfg(target_os = "macos")]
pub use macos::level_shown;
#[cfg(windows)]
pub use windows::parse_address;

use super::{secs, Provider};
use crate::reading::{DeviceKind, DeviceReading};
use std::time::Duration;

pub struct BluetoothProvider;

impl Provider for BluetoothProvider {
    fn name(&self) -> &'static str {
        "Bluetooth"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        imp::poll()
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        secs(60)
    }
}

/// The names that say a device is a keyboard: the word itself, or a line of keyboards. A maker's name isn't among
/// them: Keychron's makes a device a keyboard only when nothing else is known about it.
const KEYBOARD_NAMES: [&str; 4] = ["keyboard", "клав", "rainy", "mx keys"];

pub fn kind_by_name(name: &str) -> DeviceKind {
    let n = name.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if any(&["iphone"]) {
        DeviceKind::Phone
    } else if any(&["ipad"]) {
        DeviceKind::Tablet
    } else if any(&["mouse", "мыш", "trackball", "mx ergo", "mx master", "mx anywhere", "trackpad"]) {
        DeviceKind::Mouse
    } else if any(&["controller", "gamepad", "xbox", "dualsense", "dualshock", "joy"]) {
        DeviceKind::Gamepad
    } else if any(&KEYBOARD_NAMES) || any(&["keychron"]) {
        DeviceKind::Keyboard
    } else if any(&["headset", "headphone", "buds", "barracuda", "kraken", "arctis", "airpods", "наушн"]) {
        DeviceKind::Headset
    } else {
        DeviceKind::Other
    }
}

/// A device's kind by the minor type macOS gives it (`system_profiler`'s "Minor Type"), or by its name when that
/// says nothing. macOS files some keyboards on Bluetooth LE as a mouse - an MX Keys comes with "Minor Type: Mouse"
/// (issue #52) - so a pointing device whose name says keyboard is a keyboard. A name that only says who made the
/// device doesn't count for that: Keychron makes mice too.
#[cfg(any(target_os = "macos", test))]
fn kind_by_minor_type(minor_type: Option<&str>, name: &str) -> DeviceKind {
    match minor_type.unwrap_or("").to_ascii_lowercase().as_str() {
        "headphones" | "headset" | "speaker" | "loudspeaker" | "portable audio" | "hifi audio" | "car audio" => {
            DeviceKind::Headset
        }
        "mouse" | "trackpad" | "pointing device" if named_keyboard(name) => DeviceKind::Keyboard,
        "mouse" | "trackpad" | "pointing device" => DeviceKind::Mouse,
        "keyboard" | "keypad" => DeviceKind::Keyboard,
        "gamepad" | "game controller" | "joystick" => DeviceKind::Gamepad,
        _ => kind_by_name(name),
    }
}

/// Whether the name itself says the device is a keyboard ("MX Keys", "Magic Keyboard").
#[cfg(any(target_os = "macos", test))]
fn named_keyboard(name: &str) -> bool {
    let n = name.to_lowercase();
    kind_by_name(name) == DeviceKind::Keyboard && KEYBOARD_NAMES.iter().any(|w| n.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_the_name() {
        assert_eq!(kind_by_name("Razer Barracuda X"), DeviceKind::Headset);
        assert_eq!(kind_by_name("Беспроводная мышь"), DeviceKind::Mouse);
        assert_eq!(kind_by_name("Xbox Wireless Controller"), DeviceKind::Gamepad);
        assert_eq!(kind_by_name("Keychron K2"), DeviceKind::Keyboard);
        assert_eq!(kind_by_name("MX Keys"), DeviceKind::Keyboard);
        assert_eq!(kind_by_name("MX Ergo"), DeviceKind::Mouse);
        assert_eq!(kind_by_name("iPhone (Никита)"), DeviceKind::Phone);
    }

    #[test]
    fn kind_from_the_minor_type_macos_gives() {
        let kind = |minor_type, name| kind_by_minor_type(Some(minor_type), name);
        // The MX Keys of issue #52 on Bluetooth LE, as `system_profiler` lists it, and its siblings
        assert_eq!(kind("Mouse", "MX Keys"), DeviceKind::Keyboard);
        assert_eq!(kind("Mouse", "MX Keys Mini"), DeviceKind::Keyboard);
        assert_eq!(kind("Mouse", "Bluetooth Keyboard"), DeviceKind::Keyboard);
        // A mouse stays one: of the maker of keyboards, by its own line's name, with both words in its name
        assert_eq!(kind("Mouse", "Keychron M6"), DeviceKind::Mouse);
        assert_eq!(kind("Mouse", "MX Master 3S"), DeviceKind::Mouse);
        assert_eq!(kind("Mouse", "Keyboard Mouse Combo"), DeviceKind::Mouse);
        assert_eq!(kind("Trackpad", "Magic Trackpad"), DeviceKind::Mouse);
        // The other types as macOS gives them, whatever the name
        assert_eq!(kind("Keyboard", "Keychron K6"), DeviceKind::Keyboard);
        assert_eq!(kind("Headphones", "AirPods Pro"), DeviceKind::Headset);
        assert_eq!(kind("Headset", "Xbox Wireless Headset"), DeviceKind::Headset);
        assert_eq!(kind("Gamepad", "Xbox Wireless Controller"), DeviceKind::Gamepad);
        // A type that says nothing, or none: the name
        assert_eq!(kind("Phone", "iPhone"), DeviceKind::Phone);
        assert_eq!(kind_by_minor_type(None, "Keychron K6"), DeviceKind::Keyboard);
        assert_eq!(kind_by_minor_type(None, "Speaker"), DeviceKind::Other);
    }
}
