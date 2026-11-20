//! Small Win32 helpers shared by the rest: UTF-16 strings, Win32 errors, elevation, the app's data folder.

use std::ffi::c_void;
use std::fmt;
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

/// What failed, as C#'s exceptions said it: a Win32 error code when there is one, and the text shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: Option<u32>,
    pub message: String,
}

impl Error {
    /// Windows' own text for the code ("Access is denied."), as C#'s Win32Exception gives it.
    pub fn win32(code: u32) -> Self {
        Self { code: Some(code), message: system_message(code) }
    }

    pub fn last() -> Self {
        Self::win32(last_error())
    }

    pub fn with_message(code: u32, message: impl Into<String>) -> Self {
        Self { code: Some(code), message: message.into() }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self { code: None, message: message.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

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
