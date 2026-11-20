//! "Suggest an improvement": a new GitHub issue, already holding the questions to answer and what the app is and runs
//! on. Nothing that names the user's devices or anything else of theirs goes into the link.

use crate::loc;

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
