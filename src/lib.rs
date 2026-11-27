//! BatteryTray: the HID layer, the battery protocols, what they need (readings, state.json, texts, log, the polling
//! loop), and the app around them. On Windows the app is here too (tray, windows, settings, autostart, updater);
//! on macOS this crate is the core of an app written in Swift (macos/), which reaches it through macos/core.
//! Ported from the C# app that BatteryTray was up to 1.8.8 (the `csharp` branch); installed copies moved from one to
//! the other in place, which is why so much here is said to be "as the C# app" did it.

#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("BatteryTray runs on Windows and macOS");

#[cfg(windows)]
pub mod alert_view;
#[cfg(windows)]
pub mod app;
pub mod autostart;
pub mod diag;
pub mod feedback;
#[cfg(windows)]
pub mod gui;
pub mod hid;
#[cfg(windows)]
pub mod hotkey;
pub mod http;
pub mod list_diff;
pub mod loc;
pub mod log;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
pub mod main_view;
pub mod monitor;
#[cfg(windows)]
pub mod overlay;
pub mod providers;
pub mod reading;
pub mod rows;
pub mod settings;
#[cfg(windows)]
pub mod sound_player;
pub mod sounds;
pub mod state;
#[cfg(windows)]
pub mod strip;
pub mod sys;
#[cfg(windows)]
pub mod theme;
#[cfg(windows)]
pub mod tray;
pub mod update_service;
pub mod updater;
pub mod view;
#[cfg(windows)]
pub mod win;
