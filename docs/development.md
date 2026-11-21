# Working on BatteryTray

The app is written in Rust, with the windows in [Slint](https://slint.dev). Up to 1.8.8 it was a C# (WPF) app; that
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
```

The C runtime is linked into the exe (`.cargo/config.toml`), so it starts on a Windows without the Visual C++
redistributable. CI runs the release exe on such a Windows, Server Core in a container
(`.github/scripts/clean-windows.ps1`), and `build-release.ps1` refuses an exe that names the runtime's DLLs.

The version in `Cargo.toml` is the release version: the updater only installs an exe whose FileVersion equals the
release's, and `build.rs` writes it into the exe. `build-release.ps1` builds the release exe and zip; it checks that
version and writes the crates' licenses with [cargo-about](https://github.com/EmbarkStudios/cargo-about)
(`about.toml`, `about.hbs`).

## Layout

| | |
|---|---|
| `src/hid.rs`, `src/win.rs` | HID enumeration and I/O over hand-declared Win32 calls: overlapped reads with a timeout that cancel what they started, so a late reply is never read as the answer to the next request |
| `src/providers/` | one module per device family; each says where its protocol facts come from |
| `src/providers/devices/` | devices that answer one request on one HID collection, as rows of tables: `mod.rs` is the engine (`Spec`, `State`, `Io`, `TableProvider`), the other modules are tables by vendor or platform, some with a provider of their own for what doesn't fit a row (several devices behind one receiver, devices that are only listened to) |
| `src/monitor.rs` | polls every provider on its own thread and schedule; providers whose devices push changes cut their wait short |
| `src/state.rs`, `src/settings.rs` | `state.json` and `settings.json` in `%LOCALAPPDATA%\BatteryTray` |
| `src/app.rs` | startup, single instance, the tray, reminders, the overlay and updates; everything UI runs in Slint's event loop |
| `src/gui.rs`, `ui/*.slint` | the windows (Slint, software renderer), and the Win32 parts Slint has no API for |
| `src/updater.rs`, `src/update_service.rs`, `src/http.rs` | self-update from GitHub Releases over WinHTTP |
| `src/autostart.rs` | the logon task (elevated when installed in Program Files) |

## Tools

- `batterytray-core dump | watch | list` - a console tool for looking at real devices without running the app: `dump`
  prints what `BatteryTray.exe --dump` writes, `watch` polls as the app does and prints every change, `list` prints
  every HID collection. It reads the app's `state.json` but never writes it, and logs to
  `%LOCALAPPDATA%\BatteryTray\rust-core`.
- `cargo run --example snapshot -- <folder> [ru|en|zh] [scale]` - renders the windows with sample data to PNG files,
  off screen, to check a layout without running the app: the device list (with the update banner, and without it for
  the README; a device's menu, a smaller one, a name being edited, the hidden devices shown, and the empty list with
  the autostart banner), each settings tab (and the update card with a newer version), the reminder and the overlay. A
  drag, a tooltip and an open dropdown need a real pointer and are not drawn.
- `cargo run --example popup_probe` - opens the reminder and the overlay as the app does and prints where their windows
  are every 5 ms, with their extended styles: for catching a popup that jumps, can be clicked, takes focus or doesn't
  follow its contents' size.

## Checked on a real desktop

The `Desktop` workflow (`.github/scripts/desktop-test.ps1`) takes the release exe through what happens on a user's PC,
on runners with a desktop - Windows Server 2022, which has Windows 10's shell and fonts, and Server 2025, which has
Windows 11's: the last C# release installs from its zip and updates itself to the build, the build updates itself to a
newer one, then uninstall, a fresh install, the window, a portable copy, `--update`, and screenshots of the windows.
The binaries are not changed for it: `api.github.com` is pointed at a server on the runner that answers with a release
made of the exe under test. Each run keeps that exe as an artifact (`BatteryTray-windows-2022`, `-2025`), for trying a
build before it is released.

## What installed copies rely on

An installed copy updates itself by taking the "BatteryTray.exe" asset of the latest (non-prerelease) release when its
size and SHA-256 match what GitHub publishes and its FileVersion equals the release version. That is also how copies
of the C# app move to this one, so a release has to keep what they and their settings depend on: the asset's name and
version resource, the `BatteryTray.SingleInstance` mutex, the `BatteryTray.Activator` window a second launch signals,
the `BatteryTray` logon task, the `--background`, `--update`, `--register-task`, `--unregister-task` and `--dump`
switches, and the format of `settings.json` and `state.json` (values the app doesn't know are kept). On its first
start after the C# app, this one removes what that left behind (`BatteryTray.old.exe` and the .NET single-file
extraction folder in `%TEMP%`).

## Licenses

MIT, as the rest of the project. The licenses of the crates in the exe go into `THIRD_PARTY_LICENSES.md` in the release
zip (see above). The UI is built with [Slint](https://slint.dev) under its Royalty-free license 2.0,
which asks for an attribution: the Slint badge on a public page the app is downloaded from - the main README, which the
releases link to. Without it the app may not be distributed.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)
