//! "Suggest an improvement": a new GitHub issue, already holding the questions to answer and what the app is and runs
//! on. Nothing that names the user's devices or anything else of theirs goes into the link.
//! The link is opened by the shell (Explorer, asked through its automation object), so the browser gets the user's
//! own rights also when this app runs elevated, and gets the whole link. explorer.exe, which hands a link over as
//! well, passes on the first 259 characters of its command line: in 2.1.0 that cut the Russian and the Chinese link
//! in the middle of a letter, and GitHub answered HTTP 400.

use crate::{loc, log};
use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{
    CoAllowSetForegroundWindow, CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, IServiceProvider,
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows, SID_STopLevelBrowser, ShellWindows,
    CSIDL_DESKTOP, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// What explorer.exe passes on of a link on its command line.
const EXPLORER_TAKES: usize = 259;

/// Opens a link in the default browser. It waits for the shell, another process: not for the UI thread.
pub fn open(url: &str) {
    if let Err(e) = shell_open(url) {
        // No Explorer to ask (another shell, or it is restarting)
        log::write(&format!("the shell didn't open the link ({}): left to explorer.exe", e.message()));
        explorer_open(&for_explorer(url));
    }
}

/// Has Explorer open the link as it opens one typed into Win+R: the desktop's window gives the shell's automation
/// object, which runs in Explorer, with the user's own rights.
fn shell_open(url: &str) -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        // In a scope of its own: every interface is released before the apartment goes
        let result = (|| {
            let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
            let (desktop, empty, mut hwnd) = (VARIANT::from(CSIDL_DESKTOP as i32), VARIANT::default(), 0i32);
            let window = windows.FindWindowSW(&desktop, &empty, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
            let browser: IShellBrowser = window.cast::<IServiceProvider>()?.QueryService(&SID_STopLevelBrowser)?;
            let background: IDispatch = browser.QueryActiveShellView()?.GetItemObject(SVGIO_BACKGROUND)?;
            let shell: IShellDispatch2 = background.cast::<IShellFolderViewDual>()?.Application()?.cast()?;
            // The browser may come to the front, as when this app starts it itself
            let _ = CoAllowSetForegroundWindow(&shell, None);
            shell.ShellExecute(&BSTR::from(url), &empty, &empty, &empty, &VARIANT::from(SW_SHOWNORMAL.0))
        })();
        CoUninitialize();
        result
    }
}

/// The link as explorer.exe can pass it on: itself when it fits, else the empty form. A link cut in the middle is one
/// GitHub refuses.
fn for_explorer(url: &str) -> String {
    if url.len() <= EXPLORER_TAKES {
        url.to_string()
    } else {
        format!("{}/issues/new", env!("CARGO_PKG_REPOSITORY"))
    }
}

fn explorer_open(url: &str) {
    use std::os::windows::process::CommandExt;
    // Quoted as one field: Explorer splits its command line at commas and equals signs
    let _ = std::process::Command::new("explorer.exe").raw_arg(format!("\"{url}\"")).spawn();
}

/// The address of a new GitHub issue with the "suggest an improvement" template, in the given UI language
/// ("ru", "en", "zh"; anything else English): version, Windows 10/11 and build, language. Nothing else.
pub fn issue_url(version: &str, windows_build: u32, language: &str) -> String {
    // Windows 11 reports itself as 10.0, only the build tells them apart
    let windows = if windows_build >= 22000 { "11" } else { "10" };
    let about = loc::t_in(language, "feedback.about")
        .replace("{0}", version)
        .replace("{1}", windows)
        .replace("{2}", &windows_build.to_string());
    let (what, why) = (loc::t_in(language, "feedback.what"), loc::t_in(language, "feedback.why"));
    let body = format!("### {what}\n\n\n### {why}\n\n\n---\n{about}\n");
    format!("{}/issues/new?body={}", env!("CARGO_PKG_REPOSITORY"), percent_encode(&body))
}

/// Every byte but the few that are the same in every part of a URL as %XX: the text is a query value, and a browser
/// or the shell that hands the link over must find nothing in it to read as anything else.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_is_the_one_of_the_mockup() {
        // The link of the mockup's button (PreSetGeneral), byte for byte
        let expected = "https://github.com/qu1ncky/BatteryTray/issues/new?body=\
            %23%23%23%20%D0%A7%D1%82%D0%BE%20%D0%BF%D1%80%D0%B5%D0%B4%D0%BB%D0%B0%D0%B3%D0%B0%D0%B5%D1%82%D0%B5\
            %3F%0A%0A%0A%23%23%23%20%D0%97%D0%B0%D1%87%D0%B5%D0%BC%20%D1%8D%D1%82%D0%BE%20%D0%BD%D1%83%D0%B6%D0\
            %BD%D0%BE%3F%0A%0A%0A---%0ABatteryTray%202.0.0%20%C2%B7%20Windows%2011%20%28%D1%81%D0%B1%D0%BE%D1%80\
            %D0%BA%D0%B0%2026100%29%20%C2%B7%20%D1%8F%D0%B7%D1%8B%D0%BA%3A%20%D1%80%D1%83%D1%81%D1%81%D0%BA%D0\
            %B8%D0%B9%0A";
        assert_eq!(expected.len(), 463);
        assert_eq!(issue_url("2.0.0", 26100, "ru"), expected);
    }

    #[test]
    fn explorer_exe_gets_no_link_it_would_cut() {
        // 2.1.0 on Windows 11: the Russian link is 463 characters, and the 259th is inside a letter of "нужно"
        let ru = issue_url("2.1.0", 26200, "ru");
        assert_eq!(ru.len(), 463);
        assert!(ru[..EXPLORER_TAKES].ends_with("%D0%B6%D0%BD%D0"));
        let empty_form = "https://github.com/qu1ncky/BatteryTray/issues/new";
        assert_eq!(for_explorer(&ru), empty_form);
        assert_eq!(for_explorer(&issue_url("2.1.0", 26200, "zh")), empty_form);
        // The English one fits, by two characters
        let en = issue_url("2.1.0", 26200, "en");
        assert_eq!(en.len(), 257);
        assert_eq!(for_explorer(&en), en);
        // To the character
        let fits = format!("{empty_form}?body={}", "a".repeat(EXPLORER_TAKES - empty_form.len() - 6));
        assert_eq!((fits.len(), for_explorer(&fits)), (EXPLORER_TAKES, fits.clone()));
        assert_eq!(for_explorer(&(fits + "a")), empty_form);
    }

    #[test]
    fn windows_10_goes_up_to_build_22000() {
        assert!(issue_url("2.0.0", 19045, "ru").contains("Windows%2010%20"));
        assert!(issue_url("2.0.0", 21999, "en").contains("Windows%2010%20"));
        assert!(issue_url("2.0.0", 22000, "en").contains("Windows%2011%20"));
    }

    #[test]
    fn the_template_is_in_the_language_of_the_window() {
        assert!(issue_url("2.0.0", 26100, "en").contains("language%3A%20English"));
        assert_eq!(issue_url("2.0.0", 26100, "xx"), issue_url("2.0.0", 26100, "en"), "English when in doubt");
        assert_ne!(issue_url("2.0.0", 26100, "zh"), issue_url("2.0.0", 26100, "en"));
    }

    #[test]
    fn the_body_holds_nothing_a_url_could_read_as_its_own() {
        for language in ["ru", "en", "zh"] {
            let url = issue_url("2.0.1-beta+3", 26100, language);
            let (_, body) = url.split_once("?body=").unwrap();
            let bytes = body.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' {
                    let hex = &body[i + 1..i + 3];
                    assert!(hex.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)), "{body}");
                    i += 3;
                } else {
                    assert!(bytes[i].is_ascii_alphanumeric() || b"-._~".contains(&bytes[i]), "{body}");
                    i += 1;
                }
            }
        }
    }
}
