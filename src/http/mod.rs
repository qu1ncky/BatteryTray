//! HTTPS GET through the system's own client - WinHTTP on Windows, NSURLSession on macOS - so there is no TLS
//! library inside the app, and the system's proxy settings and certificates apply. Redirects (GitHub's downloads go
//! to another host) are followed by the client. Anything but a 2xx answer is an error, as .NET's
//! EnsureSuccessStatusCode.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::{download, get_text, get_text_with};
#[cfg(windows)]
pub use windows::{download, get, get_text, get_text_with, Response};
