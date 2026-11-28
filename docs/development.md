# Working on BatteryTray

BatteryTray runs on Windows and macOS. Both are one Rust core - the HID layer, every device protocol, the polling, the
settings, the reminders' and updates' decisions - and an app around it: on Windows in Rust too, with the windows in
[Slint](https://slint.dev); on macOS in Swift (SwiftUI and AppKit, `macos/`), which calls the core through a static
library (`macos/core`).

The Windows app is written in Rust, with the windows in [Slint](https://slint.dev). Up to 1.8.8 it was a C# (WPF) app; that
code is kept in the [`csharp`](https://github.com/qu1ncky/BatteryTray/tree/csharp) branch. Installed copies of it
update themselves to this app in place (see "What installed copies rely on" below).

## Building

Needs Rust (stable, see `rust-version` in `Cargo.toml`) and the MSVC build tools with a Windows SDK (`rc.exe` compiles
the icon, manifest and version resource).

```
cargo build --release        # target\release\BatteryTray.exe and batterytray-core.exe
cargo test                   # unit tests, no devices needed
cargo test -- --ignored      # also downloads the latest release from GitHub and checks it as an update would
cargo fmt --check            # formatting (rustfmt.toml)
cargo clippy --all-targets -- -D warnings
install.cmd                  # build and install, as a user's full install
powershell -File build-release.ps1   # the release exe and zip in dist\
```

The C runtime is linked into the exe (`.cargo/config.toml`), so it starts on a Windows without the Visual C++
redistributable. CI runs the release exe on such a Windows, Server Core in a container
(`.github/scripts/clean-windows.ps1`), and `build-release.ps1` refuses an exe that names the runtime's DLLs.

The version in `Cargo.toml` is the release version: the updater only installs an exe whose FileVersion equals the
release's, and `build.rs` writes it into the exe. `build-release.ps1` builds the release exe and zip; it checks that
version and writes the crates' licenses with [cargo-about](https://github.com/EmbarkStudios/cargo-about)
(`about.toml`, `about.hbs`).

### macOS

Needs Rust with the `aarch64-apple-darwin` and `x86_64-apple-darwin` targets and the Xcode command line tools. Xcode
itself isn't needed: the Swift sources avoid `@State`, which the macOS 26 SDK makes a macro whose plugin only Xcode
has. For the crates' licenses in the app, cargo-about (`cargo install cargo-about --locked --features cli`).

```
macos/build.sh               # target/macos/BatteryTray.app and BatteryTray-macOS.zip, Apple Silicon and Intel
macos/build.sh native        # this Mac's processor only
cargo test --workspace       # the core's tests, natively (HID, Bluetooth and update tests on this Mac included)
cargo clippy --workspace --all-targets -- -D warnings
```

`build.sh` builds the core for each processor, compiles the Swift sources against it (`swiftc`, no Xcode project),
joins the two into one app, draws the icon (`macos/make-icon.swift`), writes the version from `Cargo.toml` into
Info.plist, signs the app and zips it (a release's universal build also makes the disk image). The desktop widgets
(`macos/Widget`) are compiled the same way into an app extension in `Contents/PlugIns`, signed before the app with
the sandbox an extension must have (`macos/Widget/Widget.entitlements`). The signature is the
project's own certificate, "BatteryTray Code Signing" (a self-signed code signing certificate, valid until 2046): the
same one for every version, so macOS keeps the permissions it gave the app (Bluetooth, Input Monitoring) across updates, which it doesn't for an app signed ad hoc. `build.sh` takes it from the keychain or from
`BATTERYTRAY_SIGNING_IDENTITY`, and falls back to ad hoc with a warning when there is none. A build for a release
refuses that: with `BATTERYTRAY_REQUIRE_SIGNING=1` it fails without the certificate, and with `BATTERYTRAY_SIGNING_ROOT`
(the certificate's SHA-1, `d8322e30…0dba`) it fails when the app is signed by another one; CI sets both for main and
for tags. CI has the certificate in the repository secrets `MACOS_SIGNING_P12` (the .p12, base64) and
`MACOS_SIGNING_PASSWORD`, which only the step that imports it gets, once the crates' code has run: into a keychain of
the run, the key marked as not exportable, the keychain deleted at the end; the workflow's actions are pinned to a
commit. The app updates itself only to a version that meets its own designated requirement (`codesign --verify -R`),
that is, one signed by the same certificate; a copy signed ad hoc takes any release. So a new certificate means a
release that the copies out there refuse: changing it needs a plan (a version that trusts both). Losing the certificate
is fatal for updates in the same way. Keep a copy: Keychain Access → login → My Certificates → "BatteryTray Code
Signing" → Export. Windows' build dependencies (Slint's compiler, the resource compiler) are
built on a Mac too - Cargo picks a build dependency by the machine that builds, not by the target - but `build.rs`
does nothing for a target other than Windows.

## Layout

| | |
|---|---|
| `src/hid/` | HID enumeration and I/O. `windows.rs`: hand-declared Win32 calls, overlapped reads with a timeout that cancel what they started, so a late reply is never read as the answer to the next request. `macos.rs`: IOKit, declared by hand too; each device's report descriptor is split into the top-level collections Windows would list (`descriptor.rs`), named by paths of Windows' form, with input reports queued per collection as the Windows driver keeps them - so the providers, which group collections and tell USB from Bluetooth by the path, run unchanged |
| `src/sys.rs`, `src/win.rs`, `src/mac.rs` | What the core asks of the system (errors, the data folder, the language, admin rights) and the Win32 and CoreFoundation helpers behind it |
| `src/providers/` | one module per device family; each says where its protocol facts come from |
| `src/providers/devices/` | devices that answer one request on one HID collection, as rows of tables: `mod.rs` is the engine (`Spec`, `State`, `Io`, `TableProvider`), the other modules are tables by vendor or platform, some with a provider of their own for what doesn't fit a row (several devices behind one receiver, devices that are only listened to) |
| `src/monitor.rs` | polls every provider on its own thread and schedule; providers whose devices push changes cut their wait short. When the system says a device came or went (Windows: `WM_DEVICECHANGE` on the hidden window; macOS: IOKit's HID and CoreAudio's device notices, `macos/BatteryTray/DeviceWatch.swift`) the providers poll 1, 5 and 15 seconds later, a burst of changes being one set of rounds and rounds at least 5 seconds apart; a provider that keeps its own pace (keyboards a request keeps awake) says so in `polls_on_device_change` and isn't moved. Always on, on both systems |
| `src/providers/apple_mobile.rs` | macOS: iPhones and iPads on a cable through MobileDevice (private, looked up with `dlopen`): the framework reports the devices on a thread of the provider's own, a lockdown session reads `com.apple.mobile.battery`. USB only (a Wi-Fi device would need the Local Network permission), never pairs |
| `src/providers/portable_devices.rs` | Windows: Android phones and tablets on a cable through Windows Portable Devices (COM): the device object's `WPD_DEVICE_POWER_LEVEL`, which the MTP driver has from MTP's BatteryLevel. Seen only in the phone's "File transfer" or "PTP" USB mode; opened for reading only; iPhones and iPads left out |
| `src/providers/ups.rs` | Windows: a UPS's battery from the battery class, where hidbatt puts a HID Power Device: SetupAPI and `IOCTL_BATTERY_QUERY_*`, opened for reading only. Only short-term batteries (a UPS), never the computer's own |
| `src/state.rs`, `src/settings.rs` | `state.json` and `settings.json` in `%LOCALAPPDATA%\BatteryTray` (`~/Library/Application Support/BatteryTray` on macOS) |
| `src/sounds.rs` | The sounds of the reminder (at the reminder level and at the critical one) and of the "charged" notice: what a setting can name - the sound the system itself has for the event, none, one of the system's sounds by name (files of Windows' `Media` folder or of macOS' `/System/Library/Sounds`, played from there and not shipped), a file of the user's - the `Sounds` folder next to `settings.json` that a picked WAV or MP3 is copied into (10 MB at most), and how long a file plays (its first 10 seconds). Played by `src/sound_player.rs` on Windows (PlaySound; Windows' media player for an MP3) and by `Sounds` in `macos/BatteryTray/System.swift` (NSSound) |
| `src/view.rs` | What the windows show, worked out the same for both apps: a device's card or row, the update card, the tray number and tooltip, the overlay's entries |
| `src/app.rs` | Windows: startup, single instance, the tray, reminders, the overlay and updates; everything UI runs in Slint's event loop |
| `src/gui.rs`, `ui/*.slint` | Windows: the windows (Slint, software renderer), and the Win32 parts Slint has no API for |
| `src/updater/`, `src/update_service.rs`, `src/http/` | self-update from GitHub Releases: which release is the latest, the update server says (`server/`, below) or GitHub's API when it can't; the exe over WinHTTP on Windows, the app bundle from a zip over NSURLSession on macOS |
| `server/` | the update server, a Cloudflare Worker: the latest release for the update checks, and how many copies are in use ([The update server](#the-update-server)) |
| `src/autostart/` | the logon task on Windows (elevated when installed in Program Files), a LaunchAgent on macOS |
| `macos/core` | the core for the macOS app as a static library: one C call (`bt_call`) takes a command as JSON and answers with the state to show or the effects to carry out, and makes the decisions `src/app.rs` makes on Windows (reminders, "charged" notices, settings, the list's order, updates); `keys.rs` maps the overlay's shortcut between Win32 keys, which settings.json keeps, and the Mac's key codes; `demo.rs` the demo's devices |
| `macos/BatteryTray` | the macOS app: the menu bar item and its menu, the device window, Settings (toolbar tabs), the reminder and the overlay as non-activating panels, the shortcut through Carbon, notifications, game controllers through GameController |
| `macos/Widget` | the desktop widgets (WidgetKit, macOS 14 and later), an app extension in the sandbox: "Charge Next" (the device the menu bar's number is of), "Rings" and "List" (the list's devices as `src/view.rs` picks them for four or six places), each also colored. They read one folder of the user's: the app writes what they show into `widget/devices.json` in its folder, with the texts in its language, and asks WidgetKit to draw them again (`macos/core/src/widget.rs`): at once when something that shows changes, for a level alone at most every 15 minutes (WidgetKit allows a widget some 40-70 redraws a day). When the app quits they say it is closed. Their names and the texts they have without the app are in `macos/Widget/*.lproj` |

## Tools

- `batterytray-core dump | watch | list` - a console tool for looking at real devices without running the app: `dump`
  prints what `BatteryTray.exe --dump` writes, `watch` polls as the app does and prints every change, `list` prints
  every HID collection. It reads the app's `state.json` but never writes it, and logs to
  `%LOCALAPPDATA%\BatteryTray\rust-core`.
- `BatteryTray.app/Contents/MacOS/BatteryTray --snapshot <folder> [ru|en|zh] [light|dark] [frames]` - the macOS app draws each of its
  windows with the demo's devices to PNG files and quits: the device list, a device's menu, a name being edited, the
  menu bar item (drawn on a plain bar: its window belongs to the system) and its menu, each settings tab, the reminder
  and the overlay (an app may take pictures of its own windows, no Screen Recording permission needed). `--demo` runs the app with those devices, with settings of its own in the
  temporary folder. The site's Mac pictures (`docs/macos-*.png`) come from `--snapshot`. With `frames` it also draws the README's
  pictures: the menu bar item as a keyboard runs down and charges back, and the reminder with 70% of its time left.
- `cargo run --example snapshot -- <folder> [ru|en|zh] [scale] [light|dark]` (Windows) - renders the windows with sample data
  to PNG files, off screen, in the dark theme unless told otherwise (the overlay is dark in both), to check a layout without
  running the app: the device list (with the update banner, and without it for the README; a device's menu, a smaller
  one, a name being edited, the hidden devices shown, and the empty list with the autostart banner), each settings tab
  (the general one also with the taskbar strip switched on, and with the update card of a newer version), the reminder and the overlay. A drag, a tooltip and an open dropdown need
  a real pointer and are not drawn.
- `cargo run --example popup_probe` (Windows) - opens the reminder and the overlay as the app does and prints where their windows
  are every 5 ms, with their extended styles: for catching a popup that jumps, can be clicked, takes focus or doesn't
  follow its contents' size.
- `macos/readme-pictures.sh [Desktop run id]` (macOS) - draws the README's pictures (`docs/readme/`) in each language:
  the windows, the menu bar and the taskbar, the reminder, the overlay, the desktop widgets, where to click on the
  first start. It builds the app and runs `--snapshot … frames` (its windows come and go on the screen for a minute or
  two), draws the widgets with their own views and the demo's devices from the core it built
  (`macos/readme-widgets.swift`, off screen), takes the Windows windows and the taskbar strip's frames from the
  `desktop-windows-2025` artifact of the newest successful Desktop run of the branch (or main), and draws the scenes
  around them with `macos/readme-pictures.swift`. Needs
  ffmpeg and gh besides what `build.sh` needs.
- `cargo run --example strip_probe -- [folder] [seconds] [light|dark]` (Windows) - puts the taskbar strip (Settings > General >
  Taskbar, `src/strip.rs`) on the real taskbar with the demo's devices for 10 seconds, with no app around it, and prints
  every second where its window is, where the notification area starts and whether the mouse would reach the strip; first,
  where the apps' buttons end, as the accessibility tree says. With a folder it also draws the strip to PNG files, at rest
  and under the pointer, for a dark and a light taskbar, with a device that only says it charges (a lightning in place of
  its level), and the README's frames of it (`readme-NN.png`, 400%); 0 seconds
  only draws them, as the Desktop workflow does. The strip is a window of the app's own made a child of the taskbar's
  window (so it is under a full-screen app and in front with Start open, as the taskbar is), and this is how to see it
  on a taskbar the app has never run on: another scale, the taskbar at the top, Windows 10. A layered child window needs
  the app's manifest, which `build.rs` gives the examples too; a test binary has none.

## Checked on a real desktop

The `Desktop` workflow (`.github/scripts/desktop-test.ps1`) takes the release exe through what happens on a user's PC,
on runners with a desktop - Windows Server 2022, which has Windows 10's shell and fonts, and Server 2025, which has
Windows 11's: the last C# release installs from its zip and updates itself to the build, the build updates itself to a
newer one, then uninstall, a fresh install, the window, a portable copy, `--update`, and screenshots of the windows and the taskbar strip's pictures.
The binaries are not changed for it: `api.github.com` is pointed at a server on the runner that answers with a release
made of the exe under test. Each run keeps that exe as an artifact (`BatteryTray-windows-2022`, `-2025`), for trying a
build before it is released.

The `macOS` workflow lints and tests the core on a Mac, builds the app for Apple Silicon and Intel and draws its windows
in each language; it keeps `BatteryTray-macOS.zip` as an artifact (`BatteryTray-macOS`), which is what a release
carries for Macs.

## Releasing a version

1. Raise `version` in `Cargo.toml`.
2. `powershell -File build-release.ps1` on Windows, or from any computer: Actions → **Release files** → Run workflow on that commit, and take `BatteryTray.exe` and the zip from its `BatteryTray-release-windows` artifact.
3. Take `BatteryTray-macOS.zip` and `BatteryTray-macOS.dmg` from the `BatteryTray-macOS` artifact of the macOS workflow on that commit (or build them with `macos/build.sh` on a Mac).
4. `gh release create vX.Y.Z dist\BatteryTray-X.Y.Z-win-x64.zip dist\BatteryTray.exe BatteryTray-macOS.zip BatteryTray-macOS.dmg --title "BatteryTray X.Y.Z" --notes "what's new"`.

The README's download buttons take `BatteryTray.exe` and `BatteryTray-macOS.dmg` from the latest release, so both must be in every release.

Package managers: set the new version and the exe's SHA-256 in `bucket/batterytray.json` (Scoop), and send the version to winget with `komac update qu1ncky.BatteryTray --version X.Y.Z --urls https://github.com/qu1ncky/BatteryTray/releases/download/vX.Y.Z/BatteryTray.exe --submit`.

The updaters take the asset named exactly `BatteryTray.exe` (Windows) or `BatteryTray-macOS.zip` (macOS) from the latest release, and the tag must match the version inside the exe or the app. Drafts and pre-releases are ignored by the updater, handy for testing a build.

## The update server

`server/` is a Cloudflare Worker with a D1 database, on the free plan, at `https://updates.batterytray.workers.dev`.
An app's update check asks it first (`src/updater/mod.rs`): `GET /v1/latest` answers with the latest release as
GitHub's API gives it, with only what the apps read (`server/src/release.js`). GitHub is asked at most every 5 minutes,
and while it doesn't answer the release kept is served for an hour more.

The check sends the app's version, system and processor in its User-Agent ("BatteryTray/2.5.0 (Windows 10.0.26100;
x64)") and, in `BatteryTray-Install`, the random number the copy made up on its first check (`install-id` in its
folder). On a CI runner (`GITHUB_ACTIONS` or `CI` set to true) it sends none: a fresh machine on every run would be
counted as a new copy each time, and the Windows workflow's update test even as a version that doesn't exist (the
build with the patch number raised by 100). For each number the server keeps the days it was first and last seen,
that version, system and processor, and Cloudflare's guess of the country, never the address (`server/src/count.js`,
`server/migrations/`). Just after midnight UTC a cron trigger sums the finished day up: its users, those of the 7
and 30 days up to it, the copies new that day, and the day's users by system, country and version.

Nothing depends on it. An app that gets no usable answer within a few seconds asks GitHub, as it did before the server:
the server down or blocked, over the free plan's 100,000 requests a day, or switched off (`"SERVE": "off"` in
`server/wrangler.jsonc`, then `npm run deploy`). It asks GitHub as well at its first check and once a day, the newer
release winning, so a server giving an old one can't hold an update back for longer. A release from the server is used
only when its page and download are this repository's on GitHub.

The numbers are on `https://updates.batterytray.workers.dev/stats#key=…`, with the `STATS_KEY` secret as the key (the
page keeps it in the browser and leaves it out of the address).

Working on it, with Node 22: `cd server && npm install`, then `npm test` (an in-memory SQLite in D1's place and a
stand-in for GitHub), or `npx wrangler d1 migrations apply batterytray --local && npm run dev` for the real runtime
with a local database (`STATS_KEY=…` in `server/.dev.vars`). Deploying needs `npx wrangler login` with the project's
Cloudflare account; `npm run deploy` applies new migrations and deploys. The secrets: `npx wrangler secret put
STATS_KEY`, and optionally `GITHUB_TOKEN`, a fine-grained token that can only read public repositories, which spares
the server GitHub's limit of 60 requests an hour per address (and Workers share theirs).

## What installed copies rely on

An installed copy updates itself by taking the "BatteryTray.exe" asset of the latest (non-prerelease) release when its
size and SHA-256 match what GitHub publishes and its FileVersion equals the release version. That is also how copies
of the C# app move to this one, so a release has to keep what they and their settings depend on: the asset's name and
version resource, the `BatteryTray.SingleInstance` mutex, the `BatteryTray.Activator` window a second launch signals,
the `BatteryTray` logon task, the `--background`, `--update`, `--register-task`, `--unregister-task` and `--dump`
switches, and the format of `settings.json` and `state.json` (values the app doesn't know are kept). Copies that ask
the update server rely on its address and `/v1/latest` answering with a release of GitHub's shape, or not at all: they
then ask GitHub.

Copies on a Mac update themselves from the "BatteryTray-macOS.zip" asset in the same way (size, SHA-256, the
CFBundleShortVersionString of the BatteryTray.app inside it, which must equal the release's version, and its
signature, which must meet the running copy's designated requirement). They rely on the
bundle identifier `io.github.qu1ncky.BatteryTray` (one copy at a time, and the name of the login item
`~/Library/LaunchAgents/io.github.qu1ncky.BatteryTray.plist`), the widgets placed on the desktop on the extension's
`io.github.qu1ncky.BatteryTray.Widgets` and their kinds (`next`, `rings`, `list`, each also with `.color`), the `--background`, `--update` and `--dump` switches,
and the same `settings.json` and `state.json`. The overlay's shortcut is kept as on Windows (a Win32 virtual key and
MOD_* flags, ⌘ being MOD_WIN). On its first
start after the C# app, this one removes what that left behind (`BatteryTray.old.exe` and the .NET single-file
extraction folder in `%TEMP%`).

## Licenses

MIT, as the rest of the project. The licenses of the crates in the exe go into `THIRD_PARTY_LICENSES.md` in the release
zip (see above); those of the crates in the Mac app into the bundle's `Contents/Resources` (`macos/build.sh`, with
`macos/about.toml` and `macos/about.hbs`), next to `LICENSE` and `THIRD_PARTY_NOTICES.md`. The Windows UI is built with
[Slint](https://slint.dev) under its Royalty-free license 2.0, which asks for an attribution: the Slint badge on a
public page, easy to find there (the license prefers the page the app is downloaded from, but doesn't require it). It is
at the top of `THIRD_PARTY_NOTICES.md`, which the README's License section links to. Without it the app may not be
distributed.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)
