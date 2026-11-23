//! HID enumeration and I/O with timeouts. Adapted (by way of the C# app this one was ported from) from
//! github.com/mehdibouchami/razer-barracuda-x-battery (MIT License, Copyright (c) 2026 Mehdi Bouchami).
//!
//! Reads and writes are overlapped with a timeout; a read that times out is cancelled with CancelIoEx and waited
//! for, so no read is ever left pending to swallow the next reply (hidapi leaves it in flight - which is why this
//! is not built on hidapi).

use crate::log;
use crate::win::{self, CloseHandle, Error, Result, INVALID_HANDLE};
use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HidInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub input_length: usize,
    pub output_length: usize,
    pub feature_length: usize,
    pub output_report_id: u8,
    pub product: Option<String>,
    /// The collection exists but its security descriptor won't let this process open it.
    pub access_denied: bool,
}

impl HidInfo {
    /// USB interface number from the device path ("...&mi_02..."), or -1.
    pub fn interface_number(&self) -> i32 {
        interface_number(&self.path)
    }

    /// The collection belongs to a device connected over Bluetooth (classic HID or LE), not over USB.
    pub fn is_bluetooth(&self) -> bool {
        let path = self.path.to_ascii_lowercase();
        path.contains("{00001124-") || path.contains("{00001812-")
    }
}

impl fmt::Display for HidInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04X}:{:04X} page=0x{:04X} usage=0x{:04X} in={} out={} feat={} outId=0x{:02X}{}",
            self.vendor_id,
            self.product_id,
            self.usage_page,
            self.usage,
            self.input_length,
            self.output_length,
            self.feature_length,
            self.output_report_id,
            if self.access_denied { " ACCESS DENIED" } else { "" }
        )
    }
}

fn hex_value(s: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(s).ok()?;
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(text, 16).ok()
}

/// USB interface number from a device path ("...&mi_02..."), or -1.
pub fn interface_number(path: &str) -> i32 {
    let lower = path.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(at) = lower[from..].find("&mi_") {
        let start = from + at + 4;
        if let Some(v) = bytes.get(start..start + 2).and_then(hex_value) {
            return v as i32;
        }
        from = from + at + 1;
    }
    -1
}

/// Vendor and product ID from a device path. USB: vid_1532&pid_0550; Bluetooth HID: _vid&0002054c_pid&0ce6;
/// Bluetooth LE: _vid&02045e_pid&0b13.
pub fn parse_ids(path: &str) -> Option<(u16, u16)> {
    let lower = path.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut from = 0;
    while let Some(at) = lower[from..].find("vid") {
        let after = from + at + 3;
        for prefix in ["_", "&0001", "&0002", "&01", "&02"] {
            if !lower[after..].starts_with(prefix) {
                continue;
            }
            let v = after + prefix.len();
            let Some(vid) = b.get(v..v + 4).and_then(hex_value) else { continue };
            let sep = v + 4;
            if !matches!(b.get(sep), Some(b'&') | Some(b'_')) || b.get(sep + 1..sep + 4) != Some(b"pid") {
                continue;
            }
            if !matches!(b.get(sep + 4), Some(b'&') | Some(b'_')) {
                continue;
            }
            let p = sep + 5;
            let Some(pid) = b.get(p..p + 4).and_then(hex_value) else { continue };
            return Some((vid as u16, pid as u16));
        }
        from = from + at + 1;
    }
    None
}

/// Every HID collection of the devices whose IDs `matches` accepts.
pub fn enumerate(matches: impl Fn(u16, u16) -> bool) -> Vec<HidInfo> {
    enumerate_with(matches, true)
}

/// As enumerate(), for the catch-all provider: a collection Windows refuses it may belong to any device at all, not
/// one the app reads, so it doesn't count towards offering admin rights.
pub fn enumerate_unclaimed(matches: impl Fn(u16, u16) -> bool) -> Vec<HidInfo> {
    enumerate_with(matches, false)
}

fn enumerate_with(matches: impl Fn(u16, u16) -> bool, note_refusals: bool) -> Vec<HidInfo> {
    let mut result = Vec::new();
    unsafe {
        let mut guid = ffi::Guid::default();
        ffi::HidD_GetHidGuid(&mut guid);
        let set = ffi::SetupDiGetClassDevsW(&guid, null(), 0, ffi::DIGCF_PRESENT | ffi::DIGCF_DEVICEINTERFACE);
        if set == INVALID_HANDLE {
            return result;
        }
        let mut data = ffi::SpDeviceInterfaceData {
            cb_size: size_of::<ffi::SpDeviceInterfaceData>() as u32,
            ..Default::default()
        };
        let mut index = 0;
        while ffi::SetupDiEnumDeviceInterfaces(set, null(), &guid, index, &mut data) != 0 {
            index += 1;
            let Some(path) = interface_path(set, &data) else { continue };
            let Some((vid, pid)) = parse_ids(&path) else { continue };
            if !matches(vid, pid) {
                continue;
            }
            if let Some(info) = query(path, vid, pid) {
                if note_refusals && info.access_denied {
                    note_refusal(&info.path, vid, pid, false);
                }
                result.push(info);
            }
        }
        ffi::SetupDiDestroyDeviceInfoList(set);
    }
    result
}

unsafe fn interface_path(set: isize, data: &ffi::SpDeviceInterfaceData) -> Option<String> {
    let mut size: u32 = 0;
    ffi::SetupDiGetDeviceInterfaceDetailW(set, data, null_mut(), 0, &mut size, null_mut());
    if size < 6 {
        return None;
    }
    // SP_DEVICE_INTERFACE_DETAIL_DATA_W: cbSize (8 on x64, 6 on x86), then the path
    let mut buf = vec![0u32; (size as usize).div_ceil(4) + 1];
    buf[0] = if cfg!(target_pointer_width = "64") { 8 } else { 6 };
    if ffi::SetupDiGetDeviceInterfaceDetailW(set, data, buf.as_mut_ptr() as *mut c_void, size, &mut size, null_mut())
        == 0
    {
        return None;
    }
    let chars = std::slice::from_raw_parts((buf.as_ptr() as *const u16).add(2), (size as usize - 4) / 2);
    Some(win::from_wide(chars))
}

const ERROR_ACCESS_DENIED: u32 = 5;

/// Devices whose collection Windows refused this process while administrators may open it, with when that last
/// happened. Some third-party drivers lock a dongle's vendor collection this way (e.g. G-SPY's vkbms.inf gives every
/// FF00:0001 collection a descriptor with only SYSTEM and administrators in it). Whatever a provider then shows -
/// "needs administrator rights", another status, or no device at all - the window offers admin rights.
static REFUSED: Mutex<Vec<(u16, u16, Instant)>> = Mutex::new(Vec::new());

/// How long a refusal counts: longer than any provider waits between polls of a device it can't read.
const REFUSAL_KEPT: Duration = Duration::from_secs(10 * 60);

/// A collection of a device the app reads was refused (`read_write`: opening it for reading and writing, else even
/// opening it with no access rights): noted when admin rights would get the app in.
fn note_refusal(path: &str, vid: u16, pid: u16, read_write: bool) {
    let security = security_of(path);
    if !admins_only(security.as_deref(), read_write) {
        return;
    }
    let mut refused = REFUSED.lock().unwrap_or_else(|e| e.into_inner());
    match refused.iter_mut().find(|(v, p, _)| (*v, *p) == (vid, pid)) {
        Some(entry) => entry.2 = Instant::now(),
        None => {
            log::write(&format!(
                "{vid:04X}:{pid:04X}: Windows refuses a collection to this process, administrators may open it ({})",
                security.as_deref().unwrap_or("no security descriptor in the registry")
            ));
            refused.push((vid, pid, Instant::now()));
        }
    }
}

/// True when Windows recently kept a device from this process and admin rights would let the app read it.
pub fn admin_rights_would_help() -> bool {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(_, _, at)| at.elapsed() < REFUSAL_KEPT)
}

/// The security descriptor (SDDL) Windows checks when the collection is opened: the device's own when its driver
/// package sets one, else its device class's. None when neither is in the registry.
fn security_of(path: &str) -> Option<String> {
    const SPDRP_SECURITY_SDS: u32 = 0x18; // SPCRP_SECURITY_SDS has the same number
    unsafe {
        let set = ffi::SetupDiCreateDeviceInfoList(null(), 0);
        if set == INVALID_HANDLE {
            return None;
        }
        let name = win::wide(path);
        let mut interface = ffi::SpDeviceInterfaceData {
            cb_size: size_of::<ffi::SpDeviceInterfaceData>() as u32,
            ..Default::default()
        };
        let mut device = ffi::SpDevinfoData { cb_size: size_of::<ffi::SpDevinfoData>() as u32, ..Default::default() };
        let mut text = [0u16; 1024];
        let size = (text.len() * 2) as u32;
        let buffer = text.as_mut_ptr() as *mut u8;
        // Opening the interface puts its device into the list
        let found = ffi::SetupDiOpenDeviceInterfaceW(set, name.as_ptr(), 0, &mut interface) != 0
            && ffi::SetupDiEnumDeviceInfo(set, 0, &mut device) != 0
            && (ffi::SetupDiGetDeviceRegistryPropertyW(
                set,
                &device,
                SPDRP_SECURITY_SDS,
                null_mut(),
                buffer,
                size,
                null_mut(),
            ) != 0
                || ffi::SetupDiGetClassRegistryPropertyW(
                    &device.class_guid,
                    SPDRP_SECURITY_SDS,
                    null_mut(),
                    buffer,
                    size,
                    null_mut(),
                    null(),
                    null_mut(),
                ) != 0);
        ffi::SetupDiDestroyDeviceInfoList(set);
        Some(win::from_wide(&text)).filter(|s| found && !s.is_empty())
    }
}

/// Whether a security descriptor keeps ordinary users out but lets administrators in, so that a refusal is about
/// missing admin rights. `read_write`: what was refused was an open for reading and writing; an open with no access
/// rights is refused only to someone the descriptor doesn't name at all. A refusal the descriptor doesn't explain
/// comes from elsewhere (a filter driver such as HidHide hides devices from every program it isn't told to let
/// through, administrators included). Without a descriptor in the registry the driver set its own, which only an
/// elevated process can find out: admin rights are taken to help.
fn admins_only(sddl: Option<&str>, read_write: bool) -> bool {
    let Some(sddl) = sddl else { return true };
    // The DACL, before the SACL if there is one
    let dacl = sddl.split_once("D:").map_or("", |(_, d)| d.split("S:").next().unwrap_or(d));
    let (mut admins, mut users) = (false, false);
    for ace in dacl.split('(').skip(1) {
        let fields: Vec<&str> = ace.split(')').next().unwrap_or("").split(';').collect();
        let [kind, _, rights, _, _, sid] = fields[..] else { continue };
        if kind != "A" || (read_write && !reads_and_writes(rights)) {
            continue;
        }
        match sid {
            "BA" | "S-1-5-32-544" => admins = true,
            "WD" | "BU" | "AU" | "IU" | "S-1-1-0" | "S-1-5-32-545" | "S-1-5-11" | "S-1-5-4" => users = true,
            _ => {}
        }
    }
    admins && !users
}

/// Whether an ACE's rights ("GA", "GRGW", "FA", "0x12019f"...) cover reading and writing.
fn reads_and_writes(rights: &str) -> bool {
    const FILE_READ_DATA: u32 = 0x1;
    const FILE_WRITE_DATA: u32 = 0x2;
    const GENERIC_ALL: u32 = 0x1000_0000;
    if let Some(hex) = rights.strip_prefix("0x").or_else(|| rights.strip_prefix("0X")) {
        let mask = u32::from_str_radix(hex, 16).unwrap_or(0);
        let read = mask & (FILE_READ_DATA | ffi::GENERIC_READ | GENERIC_ALL) != 0;
        let write = mask & (FILE_WRITE_DATA | ffi::GENERIC_WRITE | GENERIC_ALL) != 0;
        return read && write;
    }
    // Two-letter codes
    let codes: Vec<&str> = (0..rights.len() / 2).filter_map(|i| rights.get(i * 2..i * 2 + 2)).collect();
    let has = |code: &str| codes.contains(&code);
    has("GA") || has("FA") || ((has("GR") || has("FR")) && (has("GW") || has("FW")))
}

fn query(path: String, vid: u16, pid: u16) -> Option<HidInfo> {
    // Zero access rights: enough to read attributes even for devices opened exclusively
    let h = win::open_path(&path, 0, 0);
    if h == INVALID_HANDLE {
        return (win::last_error() == ERROR_ACCESS_DENIED).then(|| HidInfo {
            path,
            vendor_id: vid,
            product_id: pid,
            access_denied: true,
            ..Default::default()
        });
    }
    let mut info = HidInfo { path, vendor_id: vid, product_id: pid, ..Default::default() };
    unsafe {
        let mut pre: isize = 0;
        if ffi::HidD_GetPreparsedData(h, &mut pre) != 0 {
            let mut caps = ffi::HidpCaps::default();
            if ffi::HidP_GetCaps(pre, &mut caps) == ffi::HIDP_STATUS_SUCCESS {
                info.usage_page = caps.usage_page;
                info.usage = caps.usage;
                info.input_length = caps.input_report_byte_length as usize;
                info.output_length = caps.output_report_byte_length as usize;
                info.feature_length = caps.feature_report_byte_length as usize;
                info.output_report_id = output_report_id(pre, &caps);
            }
            ffi::HidD_FreePreparsedData(pre);
        }
        let mut name = [0u16; 128];
        if ffi::HidD_GetProductString(h, name.as_mut_ptr() as *mut c_void, 256) != 0 {
            let s = win::from_wide(&name);
            if !s.is_empty() {
                info.product = Some(s);
            }
        }
        CloseHandle(h);
    }
    Some(info)
}

/// Output reports are described by value caps (a byte array) or, less often, button caps.
unsafe fn output_report_id(pre: isize, caps: &ffi::HidpCaps) -> u8 {
    let id = caps_report_id(ffi::HidP_GetValueCaps, caps.number_output_value_caps, pre);
    if id != 0 {
        id
    } else {
        caps_report_id(ffi::HidP_GetButtonCaps, caps.number_output_button_caps, pre)
    }
}

/// HIDP_VALUE_CAPS and HIDP_BUTTON_CAPS are both 72 bytes; ReportID is the byte at offset 2.
unsafe fn caps_report_id(get: ffi::GetCapsFn, count: u16, pre: isize) -> u8 {
    if count == 0 {
        return 0;
    }
    let mut buf = vec![0u64; 9 * count as usize];
    let mut n = count;
    if get(ffi::HIDP_OUTPUT, buf.as_mut_ptr() as *mut u8, &mut n, pre) != ffi::HIDP_STATUS_SUCCESS || n == 0 {
        return 0;
    }
    *(buf.as_ptr() as *const u8).add(2)
}

/// Read/write handle to one HID collection.
pub struct HidDevice {
    handle: isize,
    pub info: HidInfo,
}

// The handle is only a kernel object reference; every call on it is thread-safe.
unsafe impl Send for HidDevice {}

impl HidDevice {
    /// Read and write, overlapped (needed for `read`/`write` with a timeout).
    pub fn open(info: &HidInfo) -> Result<Self> {
        Self::open_with(info, ffi::GENERIC_READ | ffi::GENERIC_WRITE, true)
    }

    /// Only input reports (enough for devices that stream their state).
    pub fn open_read_only(info: &HidInfo) -> Result<Self> {
        Self::open_with(info, ffi::GENERIC_READ, true)
    }

    /// Read and write, synchronous: for devices that only take feature reports.
    pub fn open_sync(info: &HidInfo) -> Result<Self> {
        Self::open_with(info, ffi::GENERIC_READ | ffi::GENERIC_WRITE, false)
    }

    /// No access rights at all: Windows holds mouse and keyboard collections exclusively, but feature reports
    /// still work through such a handle.
    pub fn open_features_only(info: &HidInfo) -> Result<Self> {
        Self::open_with(info, 0, false)
    }

    fn open_with(info: &HidInfo, access: u32, overlapped: bool) -> Result<Self> {
        let h = win::open_path(&info.path, access, if overlapped { ffi::FILE_FLAG_OVERLAPPED } else { 0 });
        if h == INVALID_HANDLE {
            let error = win::last_error();
            if error == ERROR_ACCESS_DENIED && access != 0 {
                note_refusal(&info.path, info.vendor_id, info.product_id, true);
            }
            return Err(Error::with_message(error, format!("Cannot open {}", info.path)));
        }
        Ok(Self { handle: h, info: info.clone() })
    }

    pub fn flush(&self) {
        unsafe { ffi::HidD_FlushQueue(self.handle) };
    }

    /// Synchronously fetches an input report over the control pipe (HidD_GetInputReport).
    pub fn get_input_report(&self, report_id: u8) -> Option<Vec<u8>> {
        if self.info.input_length == 0 {
            return None;
        }
        let mut buf = vec![0u8; self.info.input_length];
        buf[0] = report_id;
        let ok = unsafe { ffi::HidD_GetInputReport(self.handle, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        (ok != 0).then_some(buf)
    }

    pub fn set_feature(&self, report: &[u8]) -> bool {
        let mut buf = report.to_vec();
        unsafe { ffi::HidD_SetFeature(self.handle, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) != 0 }
    }

    pub fn get_feature(&self, report_id: u8) -> Option<Vec<u8>> {
        self.get_feature_of(report_id, self.info.feature_length)
    }

    /// Reads a feature report into a buffer of `length` bytes: for a report shorter than the collection's longest
    /// one, on a device that refuses to be asked for more than the report has.
    pub fn get_feature_of(&self, report_id: u8, length: usize) -> Option<Vec<u8>> {
        if self.info.feature_length == 0 || length == 0 {
            return None;
        }
        let mut buf = vec![0u8; length];
        buf[0] = report_id;
        let ok = unsafe { ffi::HidD_GetFeature(self.handle, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        (ok != 0).then_some(buf)
    }

    /// Writes an output report padded to the collection's length; false when it wasn't taken in time.
    pub fn write(&self, report: &[u8], timeout_ms: u32) -> Result<bool> {
        if self.info.output_length == 0 {
            return Ok(false); // the collection takes no output reports
        }
        let mut buf = vec![0u8; self.info.output_length];
        let n = report.len().min(buf.len());
        buf[..n].copy_from_slice(&report[..n]);
        Ok(self.io(&mut buf, true, timeout_ms)? == buf.len())
    }

    /// The next input report, or None on timeout.
    pub fn read(&self, timeout_ms: u32) -> Result<Option<Vec<u8>>> {
        if self.info.input_length == 0 {
            return Ok(None);
        }
        let mut buf = vec![0u8; self.info.input_length];
        let n = self.io(&mut buf, false, timeout_ms)?;
        if n == 0 {
            return Ok(None);
        }
        buf.truncate(n);
        Ok(Some(buf))
    }

    fn io(&self, data: &mut [u8], write: bool, timeout_ms: u32) -> Result<usize> {
        unsafe {
            let event = ffi::CreateEventW(null(), 1, 0, null());
            if event == 0 {
                return Err(Error::last());
            }
            let result = self.io_with(event, data, write, timeout_ms);
            CloseHandle(event);
            result
        }
    }

    unsafe fn io_with(&self, event: isize, data: &mut [u8], write: bool, timeout_ms: u32) -> Result<usize> {
        let mut ov = ffi::Overlapped { h_event: event, ..Default::default() };
        let ok = if write {
            ffi::WriteFile(self.handle, data.as_ptr(), data.len() as u32, null_mut(), &mut ov)
        } else {
            ffi::ReadFile(self.handle, data.as_mut_ptr(), data.len() as u32, null_mut(), &mut ov)
        };
        if ok == 0 {
            let err = win::last_error();
            if err != ffi::ERROR_IO_PENDING {
                return Err(Error::win32(err));
            }
            if ffi::WaitForSingleObject(event, timeout_ms) != ffi::WAIT_OBJECT_0 {
                ffi::CancelIoEx(self.handle, &mut ov);
            }
        }
        // Waiting: after a cancel this returns once the cancellation has completed, so `ov` and `data` may go
        let mut transferred: u32 = 0;
        if ffi::GetOverlappedResult(self.handle, &mut ov, &mut transferred, 1) == 0 {
            let err = win::last_error();
            if err == ffi::ERROR_OPERATION_ABORTED {
                return Ok(0);
            }
            return Err(Error::win32(err));
        }
        Ok(transferred as usize)
    }
}

impl Drop for HidDevice {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

/// A device-node property key (what Device Manager's "Details" tab shows).
#[derive(Clone, Copy)]
pub struct PropertyKey {
    pub format_id: ffi::Guid,
    pub id: u32,
}

/// Every present device node under the given enumerators ("BTHENUM", "BTHLE"...) that has a one-byte value for
/// `key`: (instance ID, value).
pub fn byte_property(enumerators: &[&str], key: PropertyKey) -> Vec<(String, u8)> {
    const DEVPROP_TYPE_BYTE: u32 = 0x3;
    let mut result = Vec::new();
    for enumerator in enumerators {
        unsafe {
            let name = win::wide(enumerator);
            let set = ffi::SetupDiGetClassDevsW(null(), name.as_ptr(), 0, ffi::DIGCF_PRESENT | ffi::DIGCF_ALLCLASSES);
            if set == INVALID_HANDLE {
                continue;
            }
            let mut data = ffi::SpDevinfoData { cb_size: size_of::<ffi::SpDevinfoData>() as u32, ..Default::default() };
            let dev_key = ffi::DevPropKey { fmtid: key.format_id, pid: key.id };
            let mut index = 0;
            while ffi::SetupDiEnumDeviceInfo(set, index, &mut data) != 0 {
                index += 1;
                let mut value = [0u8; 4];
                let mut kind: u32 = 0;
                if ffi::SetupDiGetDevicePropertyW(set, &data, &dev_key, &mut kind, value.as_mut_ptr(), 4, null_mut(), 0)
                    == 0
                    || kind != DEVPROP_TYPE_BYTE
                {
                    continue;
                }
                let mut id = [0u16; 512];
                if ffi::SetupDiGetDeviceInstanceIdW(set, &data, id.as_mut_ptr(), id.len() as u32, null_mut()) == 0 {
                    continue;
                }
                result.push((win::from_wide(&id), value[0]));
            }
            ffi::SetupDiDestroyDeviceInfoList(set);
        }
    }
    result
}

#[allow(non_snake_case)]
pub(crate) mod ffi {
    use std::ffi::c_void;

    pub const DIGCF_PRESENT: u32 = 0x2;
    pub const DIGCF_ALLCLASSES: u32 = 0x4;
    pub const DIGCF_DEVICEINTERFACE: u32 = 0x10;
    pub const GENERIC_READ: u32 = 0x8000_0000;
    pub const GENERIC_WRITE: u32 = 0x4000_0000;
    pub const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    pub const ERROR_IO_PENDING: u32 = 997;
    pub const ERROR_OPERATION_ABORTED: u32 = 995;
    pub const WAIT_OBJECT_0: u32 = 0;
    pub const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
    pub const HIDP_OUTPUT: i32 = 1;
    pub const HIDP_FEATURE: i32 = 2;

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct Guid {
        pub data1: u32,
        pub data2: u16,
        pub data3: u16,
        pub data4: [u8; 8],
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct SpDeviceInterfaceData {
        pub cb_size: u32,
        pub interface_class_guid: Guid,
        pub flags: u32,
        pub reserved: usize,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct SpDevinfoData {
        pub cb_size: u32,
        pub class_guid: Guid,
        pub dev_inst: u32,
        pub reserved: usize,
    }

    #[repr(C)]
    pub struct DevPropKey {
        pub fmtid: Guid,
        pub pid: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct HidpCaps {
        pub usage: u16,
        pub usage_page: u16,
        pub input_report_byte_length: u16,
        pub output_report_byte_length: u16,
        pub feature_report_byte_length: u16,
        pub reserved: [u16; 17],
        pub number_link_collection_nodes: u16,
        pub number_input_button_caps: u16,
        pub number_input_value_caps: u16,
        pub number_input_data_indices: u16,
        pub number_output_button_caps: u16,
        pub number_output_value_caps: u16,
        pub number_output_data_indices: u16,
        pub number_feature_button_caps: u16,
        pub number_feature_value_caps: u16,
        pub number_feature_data_indices: u16,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct Overlapped {
        pub internal: usize,
        pub internal_high: usize,
        pub offset: u32,
        pub offset_high: u32,
        pub h_event: isize,
    }

    pub type GetCapsFn =
        unsafe extern "system" fn(report_type: i32, caps: *mut u8, length: *mut u16, data: isize) -> i32;

    #[link(name = "hid", kind = "raw-dylib")]
    extern "system" {
        pub fn HidD_GetHidGuid(guid: *mut Guid);
        pub fn HidD_GetPreparsedData(h: isize, data: *mut isize) -> u8;
        pub fn HidD_FreePreparsedData(data: isize) -> u8;
        pub fn HidP_GetCaps(data: isize, caps: *mut HidpCaps) -> i32;
        pub fn HidP_GetValueCaps(report_type: i32, caps: *mut u8, length: *mut u16, data: isize) -> i32;
        pub fn HidP_GetButtonCaps(report_type: i32, caps: *mut u8, length: *mut u16, data: isize) -> i32;
        pub fn HidP_GetSpecificValueCaps(
            report_type: i32,
            usage_page: u16,
            link_collection: u16,
            usage: u16,
            caps: *mut u8,
            length: *mut u16,
            data: isize,
        ) -> i32;
        pub fn HidP_GetUsageValue(
            report_type: i32,
            usage_page: u16,
            link_collection: u16,
            usage: u16,
            value: *mut u32,
            data: isize,
            report: *mut u8,
            report_length: u32,
        ) -> i32;
        pub fn HidD_FlushQueue(h: isize) -> u8;
        pub fn HidD_GetInputReport(h: isize, buffer: *mut c_void, length: u32) -> u8;
        pub fn HidD_SetFeature(h: isize, buffer: *mut c_void, length: u32) -> u8;
        pub fn HidD_GetFeature(h: isize, buffer: *mut c_void, length: u32) -> u8;
        pub fn HidD_GetProductString(h: isize, buffer: *mut c_void, length: u32) -> u8;
    }

    #[link(name = "setupapi", kind = "raw-dylib")]
    extern "system" {
        pub fn SetupDiGetClassDevsW(
            class_guid: *const Guid,
            enumerator: *const u16,
            parent: isize,
            flags: u32,
        ) -> isize;
        pub fn SetupDiEnumDeviceInterfaces(
            set: isize,
            dev_info: *const c_void,
            guid: *const Guid,
            index: u32,
            data: *mut SpDeviceInterfaceData,
        ) -> i32;
        pub fn SetupDiGetDeviceInterfaceDetailW(
            set: isize,
            data: *const SpDeviceInterfaceData,
            detail: *mut c_void,
            detail_size: u32,
            required: *mut u32,
            dev_info: *mut c_void,
        ) -> i32;
        pub fn SetupDiDestroyDeviceInfoList(set: isize) -> i32;
        pub fn SetupDiCreateDeviceInfoList(class_guid: *const Guid, parent: isize) -> isize;
        pub fn SetupDiOpenDeviceInterfaceW(
            set: isize,
            path: *const u16,
            flags: u32,
            data: *mut SpDeviceInterfaceData,
        ) -> i32;
        pub fn SetupDiGetDeviceRegistryPropertyW(
            set: isize,
            data: *const SpDevinfoData,
            property: u32,
            kind: *mut u32,
            buffer: *mut u8,
            size: u32,
            required: *mut u32,
        ) -> i32;
        pub fn SetupDiGetClassRegistryPropertyW(
            class_guid: *const Guid,
            property: u32,
            kind: *mut u32,
            buffer: *mut u8,
            size: u32,
            required: *mut u32,
            machine: *const u16,
            reserved: *mut c_void,
        ) -> i32;
        pub fn SetupDiEnumDeviceInfo(set: isize, index: u32, data: *mut SpDevinfoData) -> i32;
        pub fn SetupDiGetDeviceInstanceIdW(
            set: isize,
            data: *const SpDevinfoData,
            id: *mut u16,
            size: u32,
            required: *mut u32,
        ) -> i32;
        pub fn SetupDiGetDevicePropertyW(
            set: isize,
            data: *const SpDevinfoData,
            key: *const DevPropKey,
            kind: *mut u32,
            buffer: *mut u8,
            size: u32,
            required: *mut u32,
            flags: u32,
        ) -> i32;
    }

    #[link(name = "kernel32", kind = "raw-dylib")]
    extern "system" {
        pub fn CreateEventW(security: *const c_void, manual_reset: i32, initial: i32, name: *const u16) -> isize;
        pub fn WaitForSingleObject(h: isize, ms: u32) -> u32;
        pub fn ReadFile(h: isize, buf: *mut u8, len: u32, read: *mut u32, ov: *mut Overlapped) -> i32;
        pub fn WriteFile(h: isize, buf: *const u8, len: u32, written: *mut u32, ov: *mut Overlapped) -> i32;
        pub fn GetOverlappedResult(h: isize, ov: *mut Overlapped, transferred: *mut u32, wait: i32) -> i32;
        pub fn CancelIoEx(h: isize, ov: *mut Overlapped) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_counts_only_when_the_descriptor_lets_administrators_in_and_users_out() {
        // G-SPY's vkbms.inf: SYSTEM and administrators only
        let gspy = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";
        assert!(admins_only(Some(gspy), false) && admins_only(Some(gspy), true));
        // Windows' own HID class: everyone reads and writes, so a refusal there is a filter driver's (HidHide)
        let hid_class = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GXGWGR;;;WD)(A;;GXGR;;;RC)";
        assert!(!admins_only(Some(hid_class), false) && !admins_only(Some(hid_class), true));
        // Users may only read: a zero-access open goes through, opening for writing too needs admin rights
        let read_only = "D:(A;;GA;;;BA)(A;;GR;;;BU)";
        assert!(!admins_only(Some(read_only), false) && admins_only(Some(read_only), true));
        assert!(!admins_only(Some("D:P(A;;GA;;;SY)"), false), "SYSTEM only: admin rights don't help");
        assert!(admins_only(Some("D:(A;;0x1f01ff;;;BA)S:(ML;;NW;;;LW)"), true), "a mask, and a SACL after the DACL");
        assert!(admins_only(None, false), "a descriptor the driver set itself: admin rights may help");
    }

    #[test]
    fn rights_that_cover_reading_and_writing() {
        for rights in ["GA", "FA", "GRGW", "GWGXGR", "FRFW", "0x12019f", "0xC0000000", "0x10000000"] {
            assert!(reads_and_writes(rights), "{rights}");
        }
        for rights in ["GR", "GXGR", "FR", "0x120089", "", "0xzz"] {
            assert!(!reads_and_writes(rights), "{rights}");
        }
    }

    #[test]
    fn ids_from_usb_and_bluetooth_paths() {
        assert_eq!(
            parse_ids(r"\\?\hid#vid_1532&pid_0550&mi_03#7&1a2b&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"),
            Some((0x1532, 0x0550))
        );
        assert_eq!(
            parse_ids(r"\\?\hid#{00001124-0000-1000-8000-00805f9b34fb}_vid&0002054c_pid&0ce6#9&2&0&0000#{4d1e55b2}"),
            Some((0x054C, 0x0CE6))
        );
        assert_eq!(
            parse_ids(
                r"\\?\hid#{00001812-0000-1000-8000-00805f9b34fb}_dev_vid&02045e_pid&0b13_rev&0509#a&1&0&0000#{x}"
            ),
            Some((0x045E, 0x0B13))
        );
        assert_eq!(parse_ids(r"\\?\HID#VID_046D&PID_C539&MI_02&Col01#8&abc#{x}"), Some((0x046D, 0xC539)));
        assert_eq!(parse_ids(r"\\?\hid#acpi0c50#3&1&0#{x}"), None);
    }

    #[test]
    fn bluetooth_paths_are_told_from_usb_ones() {
        let at = |path: &str| HidInfo { path: path.to_string(), ..HidInfo::default() }.is_bluetooth();
        assert!(at(r"\\?\hid#{00001124-0000-1000-8000-00805f9b34fb}_vid&0002054c_pid&0ce6#9&2&0&0000#{4d1e55b2}"));
        assert!(at(r"\\?\HID#{00001812-0000-1000-8000-00805F9B34FB}_dev_vid&02045e_pid&0b13_rev&0509#a&1&0&0000#{x}"));
        assert!(!at(r"\\?\hid#vid_1532&pid_0550&mi_03#7&1a2b&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"));
    }

    #[test]
    fn interface_number_from_path() {
        assert_eq!(interface_number(r"\\?\hid#vid_3151&pid_402d&mi_02#8&1&0&0000#{x}"), 2);
        assert_eq!(interface_number(r"\\?\HID#VID_046D&PID_C539&MI_0A&Col01#x"), 10);
        assert_eq!(interface_number(r"\\?\hid#vid_1532&pid_0550#8&1&0&0000#{x}"), -1);
    }
}
