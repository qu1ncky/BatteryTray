//! HTTPS GET over WinHTTP, which is part of Windows: no TLS library inside the exe, and the system's proxy settings
//! and certificate store apply. Redirects (GitHub's downloads go to another host) are followed by WinHTTP itself.

use crate::win::{self, wide, Error, Result};
use std::ffi::c_void;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::time::Duration;

const ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
const FLAG_SECURE: u32 = 0x0080_0000;
const QUERY_STATUS_CODE: u32 = 19;
const QUERY_CONTENT_LENGTH: u32 = 5;
const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const QUERY_FLAG_NUMBER64: u32 = 0x0800_0000;

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn check(h: *mut c_void) -> Result<Handle> {
    if h.is_null() {
        Err(last_error())
    } else {
        Ok(Handle(h))
    }
}

fn ok(result: i32) -> Result<()> {
    if result != 0 {
        Ok(())
    } else {
        Err(last_error())
    }
}

/// WinHTTP's own errors (12xxx) have their texts in winhttp.dll.
fn last_error() -> Error {
    let code = win::last_error();
    if !(12000..13000).contains(&code) {
        return Error::os(code);
    }
    let mut buf = [0u16; 512];
    let module = unsafe { GetModuleHandleW(wide("winhttp.dll").as_ptr()) };
    const FROM_HMODULE: u32 = 0x800;
    const IGNORE_INSERTS: u32 = 0x200;
    let n = unsafe {
        FormatMessageW(FROM_HMODULE | IGNORE_INSERTS, module, code, 0, buf.as_mut_ptr(), buf.len() as u32, null())
    };
    let text = if n == 0 {
        format!("WinHTTP error {code}")
    } else {
        String::from_utf16_lossy(&buf[..n as usize]).trim_end().to_string()
    };
    Error::with_message(code, text)
}

/// An open response: the status was 2xx and the body can be read.
pub struct Response {
    // Dropped in this order: the request before the connection before the session
    request: Handle,
    _connect: Handle,
    _session: Handle,
    pub content_length: Option<u64>,
}

impl Response {
    /// Up to `buf.len()` bytes of the body; 0 at its end.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut read: u32 = 0;
        ok(unsafe { WinHttpReadData(self.request.0, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut read) })?;
        Ok(read as usize)
    }

    pub fn text(mut self) -> Result<String> {
        let mut body = Vec::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = self.read(&mut buf)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
        }
        String::from_utf8(body).map_err(|e| Error::other(e.to_string()))
    }
}

/// Sends a GET and waits for the response headers. Anything but 2xx is an error, as .NET's EnsureSuccessStatusCode.
pub fn get(url: &str, user_agent: &str, accept: Option<&str>) -> Result<Response> {
    let headers: Vec<(&str, &str)> = accept.map(|a| ("Accept", a)).into_iter().collect();
    get_with(url, user_agent, &headers, 30_000)
}

/// A GET with headers of its own, each step (resolving, connecting, sending) given `step_ms` at most and each wait
/// for data twice that.
fn get_with(url: &str, user_agent: &str, headers: &[(&str, &str)], step_ms: i32) -> Result<Response> {
    let rest = url.strip_prefix("https://").ok_or_else(|| Error::other(format!("not an https URL: {url}")))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| Error::other(format!("bad port in {url}")))?),
        None => (authority, 443),
    };

    unsafe {
        let session = check(WinHttpOpen(wide(user_agent).as_ptr(), ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0))?;
        ok(WinHttpSetTimeouts(session.0, step_ms, step_ms, step_ms, step_ms.saturating_mul(2)))?;
        let connect = check(WinHttpConnect(session.0, wide(host).as_ptr(), port, 0))?;
        let request = check(WinHttpOpenRequest(
            connect.0,
            wide("GET").as_ptr(),
            wide(path).as_ptr(),
            null(),
            null(),
            null(),
            FLAG_SECURE,
        ))?;
        let lines = headers.iter().map(|(name, value)| format!("{name}: {value}")).collect::<Vec<_>>().join("\r\n");
        let headers = (!lines.is_empty()).then(|| wide(&lines));
        let (headers_ptr, headers_len) = match &headers {
            Some(h) => (h.as_ptr(), u32::MAX),
            None => (null(), 0),
        };
        ok(WinHttpSendRequest(request.0, headers_ptr, headers_len, null(), 0, 0, 0))?;
        ok(WinHttpReceiveResponse(request.0, null_mut()))?;

        let mut status: u32 = 0;
        let mut size = 4u32;
        ok(WinHttpQueryHeaders(
            request.0,
            QUERY_STATUS_CODE | QUERY_FLAG_NUMBER,
            null(),
            &mut status as *mut u32 as *mut c_void,
            &mut size,
            null_mut(),
        ))?;
        if !(200..300).contains(&status) {
            return Err(Error::other(format!("Response status code does not indicate success: {status}.")));
        }
        let mut length: u64 = 0;
        let mut size = 8u32;
        let content_length = (WinHttpQueryHeaders(
            request.0,
            QUERY_CONTENT_LENGTH | QUERY_FLAG_NUMBER64,
            null(),
            &mut length as *mut u64 as *mut c_void,
            &mut size,
            null_mut(),
        ) != 0)
            .then_some(length);
        Ok(Response { _session: session, _connect: connect, request, content_length })
    }
}

/// The body of a GET as text.
pub fn get_text(url: &str, user_agent: &str, accept: Option<&str>) -> Result<String> {
    get(url, user_agent, accept)?.text()
}

/// The body of a GET with headers of its own, given up when a step takes longer than `step` (a wait for data, twice
/// that).
pub fn get_text_with(url: &str, user_agent: &str, headers: &[(&str, &str)], step: Duration) -> Result<String> {
    get_with(url, user_agent, headers, step.as_millis().min(i32::MAX as u128 / 2) as i32)?.text()
}

/// Downloads a file of `size` bytes into `path`, telling `progress` each new percentage.
pub fn download(url: &str, user_agent: &str, path: &Path, size: u64, progress: &dyn Fn(i32)) -> Result<()> {
    let mut response = get(url, user_agent, None)?;
    let mut file = File::create(path).map_err(|e| Error::other(e.to_string()))?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut last_percent = -1;
    loop {
        let n = response.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| Error::other(e.to_string()))?;
        done += n as u64;
        let percent = (done * 100 / size.max(1)) as i32;
        if percent != last_percent {
            last_percent = percent;
            progress(percent);
        }
    }
    file.sync_all().map_err(|e| Error::other(e.to_string()))
}

#[link(name = "winhttp", kind = "raw-dylib")]
extern "system" {
    fn WinHttpOpen(
        agent: *const u16,
        access_type: u32,
        proxy: *const u16,
        bypass: *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpSetTimeouts(h: *mut c_void, resolve: i32, connect: i32, send: i32, receive: i32) -> i32;
    fn WinHttpConnect(session: *mut c_void, server: *const u16, port: u16, reserved: u32) -> *mut c_void;
    fn WinHttpOpenRequest(
        connect: *mut c_void,
        verb: *const u16,
        object: *const u16,
        version: *const u16,
        referrer: *const u16,
        accept_types: *const *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpSendRequest(
        request: *mut c_void,
        headers: *const u16,
        headers_length: u32,
        optional: *const c_void,
        optional_length: u32,
        total_length: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: *mut c_void, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: *mut c_void,
        info_level: u32,
        name: *const u16,
        buffer: *mut c_void,
        buffer_length: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpReadData(request: *mut c_void, buffer: *mut c_void, to_read: u32, read: *mut u32) -> i32;
    fn WinHttpCloseHandle(h: *mut c_void) -> i32;
}

#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn FormatMessageW(
        flags: u32,
        source: *const c_void,
        id: u32,
        lang: u32,
        buf: *mut u16,
        size: u32,
        args: *const c_void,
    ) -> u32;
}
