//! Updates from GitHub Releases: the latest release's asset for this system replaces the running app - the exe on
//! Windows, the app bundle (from a zip) on macOS. Which release is the latest, the update server says (`get_latest`),
//! or GitHub's API when the server can't; the download always comes from GitHub. The new copy is downloaded straight next to the running one, so an
//! install that only administrators can write to never passes through a folder other programs can write to. It must
//! match the SHA-256 digest GitHub publishes for the asset and carry the release's version (a mislabeled asset would
//! otherwise update forever). The old copy is swapped out under another name and deleted on the next start.
//! The C# app does exactly the same on Windows, which is how its installs move to this one: the Rust exe is just a
//! release asset with the right name, digest and FileVersion.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::{can_install_in_place, clean_up, install, install_elevated, sha256, ASSET_NAME};
#[cfg(windows)]
pub use windows::{can_install_in_place, clean_up, file_version, install, install_elevated, sha256, ASSET_NAME};

use crate::http;
use crate::loc::tf;
use crate::log;
use crate::sys::{Error, Result};
use std::fmt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const REPOSITORY: &str = "qu1ncky/BatteryTray";

/// The update server (server/; docs/development.md, "The update server"): the latest release as GitHub's API gives
/// it, passed on by a Cloudflare Worker, which counts the copies in use by the random number each one sends along
/// (`install_id`). Whatever it can't answer well, GitHub answers, as before there was a server.
const SERVER: &str = "https://updates.batterytray.workers.dev/v1/latest";
/// The server is given up for GitHub when a step of the request (connecting, a wait for data) takes longer.
const SERVER_STEP: Duration = Duration::from_secs(8);
/// While the server answers, GitHub is still asked at the first check of a run and then once a day, so that a server
/// giving an old release can't hold an update back for longer than that.
const GITHUB_EVERY: Duration = Duration::from_secs(24 * 3600);
/// Major.minor.build, as the C# app compares versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// "1.8.6", "1.8" or "1.8.6.0" (as .NET's Version.TryParse; the fourth part is dropped).
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.trim().split('.').collect();
        if !(2..=4).contains(&parts.len()) {
            return None;
        }
        let mut numbers = Vec::new();
        for p in &parts {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            numbers.push(p.parse::<u32>().ok()?);
        }
        Some(Self(numbers[0], numbers[1], numbers.get(2).copied().unwrap_or(0)))
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("the package version is a version")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The newest published release and its asset for this system (`exe_url` and `exe_size` are the zip's on macOS).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: Version,
    pub tag: String,
    pub notes: String,
    pub page_url: String,
    pub exe_url: String,
    pub exe_size: u64,
    pub sha256: String,
}

/// "BatteryTray/2.5.0 (Windows 10.0.26100; x64)", "BatteryTray/2.5.0 (macOS 26.6; arm64)": GitHub's API requires a
/// User-Agent, and the update server counts the copies by version and system from it.
pub(crate) fn user_agent() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("BatteryTray/{} ({}; {arch})", Version::current(), system())
}

#[cfg(windows)]
fn system() -> String {
    format!("Windows 10.0.{}", crate::win::windows_build())
}

#[cfg(target_os = "macos")]
fn system() -> String {
    let version = crate::mac::macos_version();
    format!("macOS {}", if version.is_empty() { "0" } else { &version })
}

/// The random number this copy sends to the update server, so that it is counted once however often it asks: made up
/// on the first check from the system's random source and kept in the app's folder, in `install-id`. It says nothing
/// about the computer or its user. Empty when it can't be kept: the check is then answered but not counted.
pub fn install_id() -> String {
    install_id_in(&crate::sys::app_data_folder())
}

fn install_id_in(folder: &Path) -> String {
    let file = folder.join("install-id");
    if let Ok(text) = std::fs::read_to_string(&file) {
        let id = text.trim();
        if id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return id.to_string();
        }
    }
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return String::new();
    }
    let id = hex(&bytes);
    let kept = std::fs::create_dir_all(folder).and_then(|_| std::fs::write(&file, &id));
    if kept.is_err() {
        return String::new();
    }
    id
}

/// The latest non-prerelease release, or None when it has no usable asset for this system: the update server's, or
/// GitHub's when the server gives nothing usable. GitHub is asked as well at the first check and once a day, and the
/// newer of the two answers wins.
pub fn get_latest() -> Result<Option<ReleaseInfo>> {
    static GITHUB_ASKED: Mutex<Option<Instant>> = Mutex::new(None);
    let asked = |at: Option<Instant>| *GITHUB_ASKED.lock().unwrap_or_else(|e| e.into_inner()) = at;
    let due = GITHUB_ASKED.lock().unwrap_or_else(|e| e.into_inner()).is_none_or(|at| at.elapsed() >= GITHUB_EVERY);
    let id = install_id();
    choose(
        || {
            let mut headers = vec![("Accept", "application/json")];
            if !id.is_empty() {
                headers.push(("BatteryTray-Install", id.as_str()));
            }
            from_server(&http::get_text_with(SERVER, &user_agent(), &headers, SERVER_STEP)?)
        },
        || {
            let latest = from_github()?;
            asked(Some(Instant::now()));
            Ok(latest)
        },
        due,
    )
}

/// The server's release when it gives one for this system; GitHub's otherwise, and GitHub's too when `github_due`,
/// the newer of the two winning. When neither answers, the check fails with GitHub's error.
fn choose(
    server: impl FnOnce() -> Result<Option<ReleaseInfo>>,
    github: impl FnOnce() -> Result<Option<ReleaseInfo>>,
    github_due: bool,
) -> Result<Option<ReleaseInfo>> {
    let release = match server() {
        Ok(Some(release)) => release,
        Ok(None) => {
            log::write("update server: no release for this system; asking GitHub");
            return github();
        }
        Err(e) => {
            log::write(&format!("update server: {e}; asking GitHub"));
            return github();
        }
    };
    if !github_due {
        return Ok(Some(release));
    }
    match github() {
        Ok(Some(newer)) if newer.version > release.version => Ok(Some(newer)),
        Ok(_) => Ok(Some(release)),
        Err(e) => {
            log::write(&format!("update check on GitHub failed: {e}; the update server's answer stands"));
            Ok(Some(release))
        }
    }
}

/// The update server's answer, used only when the release's page and download are this repository's on GitHub: the
/// download is checked against the digest that comes with it, so a server pointing elsewhere could hand out anything.
/// They are the addresses GitHub gives a release with this tag, compared whole: one that only begins as they do can
/// climb out of the repository with "/../", which the HTTP client resolves before it asks.
fn from_server(text: &str) -> Result<Option<ReleaseInfo>> {
    let root: serde_json::Value = serde_json::from_str(text).map_err(|e| Error::other(e.to_string()))?;
    let Some(release) = parse_release(&root) else {
        return Ok(None);
    };
    let releases = format!("https://github.com/{REPOSITORY}/releases");
    let download = format!("{releases}/download/{}/{ASSET_NAME}", release.tag);
    // The server gives the second page when GitHub's own isn't this repository's (server/src/release.js)
    let pages = [format!("{releases}/tag/{}", release.tag), format!("{releases}/latest")];
    if release.exe_url != download || !pages.contains(&release.page_url) {
        return Err(Error::other("its release isn't this repository's on GitHub"));
    }
    Ok(Some(release))
}

/// The latest release straight from GitHub's API.
fn from_github() -> Result<Option<ReleaseInfo>> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let text = http::get_text(&url, &user_agent(), Some("application/vnd.github+json"))?;
    let root: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error::other(e.to_string()))?;
    Ok(parse_release(&root))
}

pub fn parse_release(root: &serde_json::Value) -> Option<ReleaseInfo> {
    parse_release_for(root, ASSET_NAME)
}

/// The release with its asset of this name.
pub fn parse_release_for(root: &serde_json::Value, asset_name: &str) -> Option<ReleaseInfo> {
    let tag = root.get("tag_name")?.as_str()?.to_string();
    let version = Version::parse(tag.trim_start_matches(['v', 'V']))?;
    let asset =
        root.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset_name))?;
    let digest = asset.get("digest")?.as_str()?;
    if digest.len() < 7 || !digest[..7].eq_ignore_ascii_case("sha256:") {
        return None;
    }
    Some(ReleaseInfo {
        version,
        notes: root.get("body").and_then(|b| b.as_str()).unwrap_or_default().to_string(),
        page_url: root.get("html_url").and_then(|u| u.as_str()).unwrap_or_default().to_string(),
        exe_url: asset.get("browser_download_url").and_then(|u| u.as_str()).unwrap_or_default().to_string(),
        exe_size: asset.get("size").and_then(|s| s.as_u64())?,
        sha256: digest[7..].to_ascii_lowercase(),
        tag,
    })
}

/// The downloaded file has the size and the SHA-256 digest GitHub publishes for the asset.
pub(crate) fn check_digest(path: &Path, release: &ReleaseInfo) -> Result<()> {
    let bytes = std::fs::read(path).map_err(|e| Error::other(e.to_string()))?;
    if bytes.len() as u64 != release.exe_size {
        return Err(Error::other(tf("upd.err.size", &[&bytes.len(), &release.exe_size])));
    }
    if hex(&sha256(&bytes)?) != release.sha256 {
        return Err(Error::other(tf("upd.err.hash", &[])));
    }
    Ok(())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_as_dotnet_parses_them() {
        assert_eq!(Version::parse("1.8.6"), Some(Version(1, 8, 6)));
        assert_eq!(Version::parse("1.8.6.0"), Some(Version(1, 8, 6)));
        assert_eq!(Version::parse("2.0"), Some(Version(2, 0, 0)));
        assert_eq!(Version::parse("2"), None);
        assert_eq!(Version::parse("1.8.x"), None);
        assert!(Version(2, 0, 0) > Version(1, 99, 0) && Version(1, 99, 0) > Version(1, 8, 6));
    }

    #[test]
    fn release_from_githubs_json() {
        let json = serde_json::json!({
            "tag_name": "v1.8.6", "body": "notes", "html_url": "https://github.com/qu1ncky/BatteryTray/releases/tag/v1.8.6",
            "assets": [
                { "name": "BatteryTray-1.8.6.zip", "digest": "sha256:00", "size": 1, "browser_download_url": "z" },
                { "name": "BatteryTray.exe", "digest": "sha256:ABCDEF", "size": 78123456,
                  "browser_download_url": "https://github.com/qu1ncky/BatteryTray/releases/download/v1.8.6/BatteryTray.exe" },
                { "name": "BatteryTray-macOS.zip", "digest": "sha256:0123", "size": 9123456,
                  "browser_download_url": "https://github.com/qu1ncky/BatteryTray/releases/download/v1.8.6/BatteryTray-macOS.zip" }
            ]
        });
        let r = parse_release_for(&json, "BatteryTray.exe").unwrap();
        assert_eq!((r.version, r.exe_size, r.sha256.as_str()), (Version(1, 8, 6), 78123456, "abcdef"));
        let mac = parse_release_for(&json, "BatteryTray-macOS.zip").unwrap();
        assert_eq!((mac.exe_size, mac.sha256.as_str()), (9123456, "0123"));
        assert_eq!(parse_release(&json).map(|r| r.exe_url.ends_with(ASSET_NAME)), Some(true));
        let mut no_digest = json.clone();
        no_digest["assets"][1]["digest"] = serde_json::Value::Null;
        assert_eq!(parse_release_for(&no_digest, "BatteryTray.exe"), None, "no digest, no update");
    }

    fn release(version: u32) -> ReleaseInfo {
        let tag = format!("v2.{version}.0");
        ReleaseInfo {
            version: Version(2, version, 0),
            notes: String::new(),
            page_url: format!("https://github.com/{REPOSITORY}/releases/tag/{tag}"),
            exe_url: format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{ASSET_NAME}"),
            exe_size: 1,
            sha256: "00".into(),
            tag,
        }
    }

    fn failed() -> Result<Option<ReleaseInfo>> {
        Err(Error::other("no answer"))
    }

    #[test]
    fn the_servers_release_unless_it_gives_none_then_githubs() {
        let github_unasked = || -> Result<Option<ReleaseInfo>> { panic!("GitHub asked") };
        assert_eq!(choose(|| Ok(Some(release(5))), github_unasked, false).unwrap(), Some(release(5)));
        assert_eq!(choose(failed, || Ok(Some(release(5))), false).unwrap(), Some(release(5)), "the server fails");
        assert_eq!(
            choose(|| Ok(None), || Ok(Some(release(5))), false).unwrap(),
            Some(release(5)),
            "none for this system"
        );
        assert_eq!(choose(failed, failed, false).unwrap_err().message, "no answer", "GitHub's error");
        assert_eq!(choose(failed, || Ok(None), true).unwrap(), None);
    }

    #[test]
    fn github_asked_as_well_once_a_day_the_newer_winning() {
        assert_eq!(choose(|| Ok(Some(release(5))), || Ok(Some(release(6))), true).unwrap(), Some(release(6)));
        assert_eq!(choose(|| Ok(Some(release(6))), || Ok(Some(release(5))), true).unwrap(), Some(release(6)));
        assert_eq!(choose(|| Ok(Some(release(5))), failed, true).unwrap(), Some(release(5)), "GitHub fails");
        assert_eq!(choose(|| Ok(Some(release(5))), || Ok(None), true).unwrap(), Some(release(5)));
    }

    #[test]
    fn the_servers_release_only_with_this_repositorys_page_and_download() {
        let releases = format!("https://github.com/{REPOSITORY}/releases");
        let answer = |page: &str, download: &str| {
            serde_json::json!({
                "tag_name": "v2.5.0", "body": "notes", "html_url": page,
                "assets": [{ "name": ASSET_NAME, "size": 10, "digest": "sha256:ab", "browser_download_url": download }]
            })
            .to_string()
        };
        let page = format!("{releases}/tag/v2.5.0");
        let download = format!("{releases}/download/v2.5.0/{ASSET_NAME}");
        let r = from_server(&answer(&page, &download)).unwrap().unwrap();
        assert_eq!((r.version, r.exe_size, r.sha256.as_str()), (Version(2, 5, 0), 10, "ab"));
        assert!(from_server(&answer(&format!("{releases}/latest"), &download)).is_ok(), "the server's own page");
        let other_downloads = [
            "https://example.com/BatteryTray.exe".to_string(),
            format!("https://github.com/someone/else/releases/download/v2.5.0/{ASSET_NAME}"),
            // They begin as this repository's and climb out of it
            format!("{releases}/download/../../../../someone/else/releases/download/v2.5.0/{ASSET_NAME}"),
            format!("{download}#/../../../../../../someone/else/releases/download/v2.5.0/{ASSET_NAME}"),
            // Another release's
            format!("{releases}/download/v2.4.0/{ASSET_NAME}"),
        ];
        for other in &other_downloads {
            assert!(from_server(&answer(&page, other)).is_err(), "{other}");
        }
        for other in ["https://example.com/", &format!("{releases}/tag/../../../../someone/else")] {
            assert!(from_server(&answer(other, &download)).is_err(), "{other}");
        }
        assert!(from_server("<html>").is_err());
        assert_eq!(from_server(r#"{"tag_name": "v2.5.0", "assets": []}"#).unwrap(), None);
    }

    #[test]
    fn install_id_made_up_once_and_kept() {
        let folder = std::env::temp_dir().join(format!("BatteryTray-install-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let id = install_id_in(&folder);
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_eq!(install_id_in(&folder), id, "kept");
        std::fs::write(folder.join("install-id"), "broken").unwrap();
        let other = install_id_in(&folder);
        assert!(other.len() == 32 && other != id, "made up again");
        let _ = std::fs::remove_dir_all(&folder);
        let file = std::env::temp_dir().join(format!("BatteryTray-install-id-file-{}", std::process::id()));
        std::fs::write(&file, "").unwrap();
        assert_eq!(install_id_in(&file.join("folder")), "", "can't be kept, so not sent");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn user_agent_says_version_system_and_processor() {
        let agent = user_agent();
        let rest = agent.strip_prefix(&format!("BatteryTray/{} (", Version::current())).unwrap();
        let (system, arch) = rest.strip_suffix(')').unwrap().split_once("; ").unwrap();
        let (os, version) = system.split_once(' ').unwrap();
        assert!(["Windows", "macOS"].contains(&os), "{agent}");
        assert!(!version.is_empty() && version.split('.').all(|p| p.parse::<u32>().is_ok()), "{agent}");
        assert!(["x64", "arm64"].contains(&arch), "{agent}");
    }

    /// The real thing: the update server answers this app's HTTP client with a release it takes, and GitHub gives
    /// the same. `cargo test -- --ignored`; the test number it sends is counted as a copy, to be deleted after.
    #[test]
    #[ignore]
    fn the_update_server_answers_as_github() {
        let headers = [("Accept", "application/json"), ("BatteryTray-Install", "fffffffffffffffffffffffffffffff1")];
        let text = http::get_text_with(SERVER, &user_agent(), &headers, SERVER_STEP).unwrap();
        let served = from_server(&text).unwrap().unwrap();
        assert_eq!(Some(served), from_github().unwrap());
    }

    #[test]
    fn sha256_of_abc() {
        assert_eq!(hex(&sha256(b"abc").unwrap()), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
