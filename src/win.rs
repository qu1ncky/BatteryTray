//! Small Win32 helpers shared by the Windows parts: UTF-16 strings, Win32 errors, elevation, the app's data folder
//! (the rest of the core reaches the last three through `sys`).

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A UTF-16 buffer up to its first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub use crate::sys::{Error, Result};

pub fn last_error() -> u32 {
    unsafe { GetLastError() }
}

pub fn system_message(code: u32) -> String {
    const FROM_SYSTEM: u32 = 0x1000;
    const IGNORE_INSERTS: u32 = 0x200;
    let mut buf = [0u16; 512];
    let n = unsafe {
        FormatMessageW(FROM_SYSTEM | IGNORE_INSERTS, null(), code, 0, buf.as_mut_ptr(), buf.len() as u32, null())
    };
    if n == 0 {
        return format!("Unknown error (0x{code:x})");
    }
    String::from_utf16_lossy(&buf[..n as usize]).trim_end().to_string()
}

/// True when this process runs with administrator rights (an elevated token).
pub fn is_elevated() -> bool {
    const TOKEN_QUERY: u32 = 8;
    const TOKEN_ELEVATION: i32 = 20;
    unsafe {
        let mut token: isize = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevated: u32 = 0;
        let mut len: u32 = 0;
        let ok = GetTokenInformation(token, TOKEN_ELEVATION, &mut elevated as *mut u32 as *mut c_void, 4, &mut len);
        CloseHandle(token);
        ok != 0 && elevated != 0
    }
}

/// The exe name of the process that started this one: "svchost.exe" for the logon task, "explorer.exe" for a launch
/// by hand or from the Startup folder. None when it has ended since. For the log only: Windows gives the number of a
/// process that has ended to a new one, so now and then the name is another program's.
pub fn parent_process() -> Option<String> {
    const SNAP_PROCESS: u32 = 2;
    #[repr(C)]
    struct Entry {
        size: u32,
        usage: u32,
        id: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        exe: [u16; 260],
    }
    let mut entry = Entry {
        size: std::mem::size_of::<Entry>() as u32,
        usage: 0,
        id: 0,
        heap: 0,
        module: 0,
        threads: 0,
        parent: 0,
        priority: 0,
        flags: 0,
        exe: [0; 260],
    };
    let mut processes = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(SNAP_PROCESS, 0);
        if snapshot == INVALID_HANDLE {
            return None;
        }
        let mut more = Process32FirstW(snapshot, &mut entry as *mut Entry as *mut c_void);
        while more != 0 {
            processes.push((entry.id, entry.parent, from_wide(&entry.exe)));
            more = Process32NextW(snapshot, &mut entry as *mut Entry as *mut c_void);
        }
        CloseHandle(snapshot);
    }
    let me = std::process::id();
    let parent = processes.iter().find(|p| p.0 == me)?.1;
    processes.into_iter().find(|p| p.0 == parent).map(|p| p.2)
}

/// %LOCALAPPDATA%\BatteryTray: settings.json, state.json and the log.
pub fn app_data_folder() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("BatteryTray")
}

/// The primary language of the Windows display language: "ru", "zh" or "en".
pub fn ui_language() -> &'static str {
    const LANG_CHINESE: u16 = 0x04;
    const LANG_RUSSIAN: u16 = 0x19;
    match unsafe { GetUserDefaultUILanguage() } & 0x3FF {
        LANG_RUSSIAN => "ru",
        LANG_CHINESE => "zh",
        _ => "en",
    }
}

/// The Windows build number (22000 and up is Windows 11), as the kernel reports it whatever the manifest says.
pub fn windows_build() -> u32 {
    #[repr(C)]
    struct OsVersion {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    let mut info = OsVersion {
        size: std::mem::size_of::<OsVersion>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    unsafe { RtlGetVersion(&mut info as *mut OsVersion as *mut c_void) };
    info.build
}

pub const INVALID_HANDLE: isize = -1;

#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    pub fn GetLastError() -> u32;
    pub fn CloseHandle(handle: isize) -> i32;
    fn FormatMessageW(
        flags: u32,
        source: *const c_void,
        id: u32,
        lang: u32,
        buf: *mut u16,
        size: u32,
        args: *const c_void,
    ) -> u32;
    fn GetCurrentProcess() -> isize;
    fn GetUserDefaultUILanguage() -> u16;
    fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> isize;
    fn Process32FirstW(snapshot: isize, entry: *mut c_void) -> i32;
    fn Process32NextW(snapshot: isize, entry: *mut c_void) -> i32;
    pub fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: isize,
    ) -> isize;
}

#[link(name = "advapi32", kind = "raw-dylib")]
extern "system" {
    fn OpenProcessToken(process: isize, access: u32, token: *mut isize) -> i32;
    fn GetTokenInformation(token: isize, class: i32, info: *mut c_void, len: u32, returned: *mut u32) -> i32;
}

#[link(name = "ntdll", kind = "raw-dylib")]
extern "system" {
    fn RtlGetVersion(info: *mut c_void) -> i32;
}

/// Opens a device path for sharing: GENERIC_* access as given, OPEN_EXISTING.
pub fn open_path(path: &str, access: u32, flags: u32) -> isize {
    const SHARE_READ_WRITE: u32 = 3;
    const OPEN_EXISTING: u32 = 3;
    let name = wide(path);
    unsafe { CreateFileW(name.as_ptr(), access, SHARE_READ_WRITE, null_mut(), OPEN_EXISTING, flags, 0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests are started by cargo or by a test runner: an exe that is still there.
    #[test]
    fn the_process_that_started_this_one_has_a_name() {
        let parent = parent_process();
        println!("started by {parent:?}");
        assert!(parent.is_some_and(|p| p.to_lowercase().ends_with(".exe")));
    }
}
