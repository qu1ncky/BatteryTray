//! BatteryTray: the HID layer, the battery protocols, what they need (readings, state.json, texts, log, the polling
//! loop), and the app around them (tray, windows, settings, autostart, updater). Ported from the C# app that
//! BatteryTray was up to 1.8.8 (the `csharp` branch); installed copies moved from one to the other in place, which
//! is why so much here is said to be "as the C# app" did it.

pub mod alert_view;
pub mod app;
pub mod autostart;
pub mod diag;
pub mod feedback;
pub mod gui;
pub mod hid;
pub mod hotkey;
pub mod http;
pub mod list_diff;
pub mod loc;
pub mod log;
pub mod main_view;
pub mod monitor;
pub mod overlay;
pub mod providers;
pub mod reading;
pub mod rows;
pub mod settings;
pub mod state;
pub mod tray;
pub mod update_service;
pub mod updater;
pub mod win;
