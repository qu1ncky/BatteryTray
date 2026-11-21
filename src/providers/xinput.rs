//! Xbox controllers through the Xbox Wireless Adapter (or any XInput gamepad on batteries).
//! XInput only knows four levels - empty, low, medium, full - so the percentage is a
//! rough stand-in. Controllers on Bluetooth are skipped when XInput reveals their product ID: Windows reports their
//! exact level, see the Bluetooth provider.
//! xinput1_4.dll is loaded on the first poll, from System32: a Windows without it (Server Core, some stripped
//! builds) just has no Xbox gamepads here, instead of an app that can't start.

use super::{secs, Provider};
use crate::loc::t;
use crate::reading::{DeviceKind, DeviceReading};
use std::sync::OnceLock;
use std::time::Duration;
use windows::core::{s, w, PCSTR};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

const ERROR_SUCCESS: u32 = 0;
const DEV_TYPE_GAMEPAD: u8 = 0;
const TYPE_DISCONNECTED: u8 = 0;
const TYPE_WIRED: u8 = 1;

/// Product IDs Xbox controllers use over Bluetooth (classic and LE), as listed by SDL.
const BLUETOOTH_PIDS: [u16; 8] = [0x02E0, 0x02FD, 0x0B05, 0x0B0C, 0x0B13, 0x0B20, 0x0B21, 0x0B22];

pub struct XInputProvider;

impl Provider for XInputProvider {
    fn name(&self) -> &'static str {
        "XInput"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        let Some(api) = xinput() else { return result };
        for slot in 0..4u32 {
            let mut b = XBattery::default();
            if unsafe { (api.battery)(slot, DEV_TYPE_GAMEPAD, &mut b) } != ERROR_SUCCESS {
                continue;
            }
            if b.kind == TYPE_DISCONNECTED || b.kind == TYPE_WIRED {
                continue;
            }
            // Bluetooth controllers have their exact level from Windows, and a controller that a row of the device
            // tables has read has it from its own receiver
            let mut caps = CapabilitiesEx::default();
            if api.capabilities.is_some_and(|get| unsafe { get(1, slot, 0, &mut caps) } == ERROR_SUCCESS)
                && ((caps.vendor_id == 0x045E && BLUETOOTH_PIDS.contains(&caps.product_id))
                    || super::devices::read_here(caps.vendor_id, caps.product_id))
            {
                continue;
            }

            // Empty, low, medium, full
            let level = match b.level {
                0 => 5,
                1 => 15,
                2 => 50,
                _ => 100,
            };
            let name = if slot == 0 { t("name.xbox") } else { format!("{} #{}", t("name.xbox"), slot + 1) };
            result.push(DeviceReading {
                online: true,
                level: Some(level),
                estimate: true,
                detail: Some(t("d.fourLevels")),
                ..DeviceReading::new(format!("xinput:{slot}"), name, DeviceKind::Gamepad, "XInput")
            });
        }
        result
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        secs(60)
    }
}

#[repr(C)]
#[derive(Default)]
struct XBattery {
    kind: u8,
    level: u8,
}

/// XINPUT_CAPABILITIES_EX, returned by the undocumented ordinal 108 of xinput1_4.dll.
#[repr(C)]
#[derive(Default)]
struct CapabilitiesEx {
    kind: u8,
    sub_type: u8,
    flags: u16,
    gamepad: [u8; 12],   // XINPUT_GAMEPAD
    vibration: [u16; 2], // XINPUT_VIBRATION
    vendor_id: u16,
    product_id: u16,
    product_version: u16,
    reserved1: u16,
    reserved2: u32,
}

type GetBatteryInformation = unsafe extern "system" fn(user_index: u32, dev_type: u8, battery: *mut XBattery) -> u32;
type GetCapabilitiesEx =
    unsafe extern "system" fn(reserved: u32, user_index: u32, flags: u32, caps: *mut CapabilitiesEx) -> u32;

/// What GetProcAddress hands back, before it is given its real signature
type Proc = unsafe extern "system" fn() -> isize;

struct XInput {
    battery: GetBatteryInformation,
    /// The undocumented ordinal 108: only used to tell Bluetooth controllers apart
    capabilities: Option<GetCapabilitiesEx>,
}

fn xinput() -> Option<&'static XInput> {
    static API: OnceLock<Option<XInput>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let module = LoadLibraryExW(w!("xinput1_4.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let battery = GetProcAddress(module, s!("XInputGetBatteryInformation"))?;
        let capabilities = GetProcAddress(module, PCSTR(108usize as *const u8));
        Some(XInput {
            battery: std::mem::transmute::<Proc, GetBatteryInformation>(battery),
            capabilities: capabilities.map(|f| std::mem::transmute::<Proc, GetCapabilitiesEx>(f)),
        })
    })
    .as_ref()
}
