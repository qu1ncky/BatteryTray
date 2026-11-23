//! Catch-all for any USB device that declares the standard HID "Battery Strength" field (Generic Device Controls page
//! 0x06, usage 0x20) in a feature report - common on generic 2.4 GHz receivers - which Windows never shows.
//! Only feature reports are read (a GET_REPORT has no side effects), so
//! devices with a dedicated provider and Bluetooth LE devices (Windows shows those itself) are skipped.

use super::{contains_ignore_case, one_per_id, secs, Provider};
use crate::hid::{self, ffi, HidInfo};
use crate::loc::t;
use crate::reading::{DeviceKind, DeviceReading};
use crate::win::{self, CloseHandle, INVALID_HANDLE};
use std::time::Duration;

const GENERIC_DEVICE_PAGE: u16 = 0x06;
const BATTERY_STRENGTH: u16 = 0x20;

/// Vendors read by their own providers. A device that a row of the tables in `devices` has read is left alone too.
const OWN_PROVIDERS: [u16; 7] = [0x1532, 0x1038, 0x046D, 0x054C, 0x03F0, 0x0951, 0x1B1C];

pub struct StandardHidBatteryProvider;

impl Provider for StandardHidBatteryProvider {
    fn name(&self) -> &'static str {
        "HID Battery Strength"
    }

    fn poll(&mut self) -> Vec<DeviceReading> {
        let mut result = Vec::new();
        for info in hid::enumerate_unclaimed(|v, p| !OWN_PROVIDERS.contains(&v) && !super::devices::read_here(v, p)) {
            if info.access_denied || info.feature_length == 0 || contains_ignore_case(&info.path, "{00001812-") {
                continue; // BLE HID: Windows shows those itself
            }
            // A device that refuses the request just isn't listed
            let Some(level) = read_level(&info) else { continue };
            let name =
                info.product.clone().unwrap_or_else(|| format!("HID {:04X}:{:04X}", info.vendor_id, info.product_id));
            result.push(DeviceReading {
                online: true,
                level: Some(level),
                ..DeviceReading::new(
                    format!("hid:{:04x}:{:04x}", info.vendor_id, info.product_id),
                    name,
                    kind_of(&info),
                    t("via.receiver"),
                )
            });
        }
        // One entry per device even if several of its collections carry the field
        one_per_id(result, |_| false)
    }

    fn delay(&self, _last: &[DeviceReading]) -> Duration {
        secs(2 * 60)
    }
}

fn kind_of(info: &HidInfo) -> DeviceKind {
    if info.usage_page != 0x01 {
        return DeviceKind::Other;
    }
    match info.usage {
        0x02 => DeviceKind::Mouse,
        0x06 => DeviceKind::Keyboard,
        0x04 | 0x05 => DeviceKind::Gamepad,
        _ => DeviceKind::Other,
    }
}

fn read_level(info: &HidInfo) -> Option<i32> {
    // No access rights needed for feature reports, even on collections Windows holds exclusively
    let h = win::open_path(&info.path, 0, 0);
    if h == INVALID_HANDLE {
        return None;
    }
    let level = unsafe { read_level_with(h, info) };
    unsafe { CloseHandle(h) };
    level
}

unsafe fn read_level_with(h: isize, info: &HidInfo) -> Option<i32> {
    let mut pre: isize = 0;
    if ffi::HidD_GetPreparsedData(h, &mut pre) == 0 {
        return None;
    }
    let level = (|| {
        const MAX_CAPS: u16 = 64; // HIDP_VALUE_CAPS is 72 bytes
        let mut caps = vec![0u64; 9 * MAX_CAPS as usize];
        let mut count = MAX_CAPS;
        if ffi::HidP_GetSpecificValueCaps(
            ffi::HIDP_FEATURE,
            GENERIC_DEVICE_PAGE,
            0,
            BATTERY_STRENGTH,
            caps.as_mut_ptr() as *mut u8,
            &mut count,
            pre,
        ) != ffi::HIDP_STATUS_SUCCESS
            || count == 0
        {
            return None;
        }
        // HIDP_VALUE_CAPS: ReportID at 2, LinkCollection at 6, LogicalMin at 40, LogicalMax at 44
        let bytes = std::slice::from_raw_parts(caps.as_ptr() as *const u8, 72);
        let report_id = bytes[2];
        let link = u16::from_le_bytes([bytes[6], bytes[7]]);
        let mut min = i32::from_le_bytes(bytes[40..44].try_into().unwrap());
        let mut max = i32::from_le_bytes(bytes[44..48].try_into().unwrap());

        let mut report = vec![0u8; info.feature_length];
        report[0] = report_id;
        if ffi::HidD_GetFeature(h, report.as_mut_ptr() as *mut _, report.len() as u32) == 0 {
            return None;
        }
        let mut raw: u32 = 0;
        if ffi::HidP_GetUsageValue(
            ffi::HIDP_FEATURE,
            GENERIC_DEVICE_PAGE,
            link,
            BATTERY_STRENGTH,
            &mut raw,
            pre,
            report.as_mut_ptr(),
            report.len() as u32,
        ) != ffi::HIDP_STATUS_SUCCESS
        {
            return None;
        }
        // Most devices declare 0-100 (or 0-255); a missing range is taken as a percentage
        if max <= min {
            (min, max) = (0, 100);
        }
        // Math.Round in C# rounds halves to even
        let percent = ((raw as i64 - min as i64) as f64 * 100.0 / (max - min) as f64).round_ties_even() as i32;
        (1..=100).contains(&percent).then_some(percent)
    })();
    ffi::HidD_FreePreparsedData(pre);
    level
}
