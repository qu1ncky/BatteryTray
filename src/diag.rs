//! What `--dump` writes: every provider polled once, in the format of the C# app's dump.

use crate::{hid, monitor, providers, state::StateStore, win};
use std::sync::Arc;

pub fn dump_lines() -> Vec<String> {
    let state = Arc::new(StateStore::load());
    let mut lines = vec![format!(
        "{} elevated={}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        if win::is_elevated() { "True" } else { "False" }
    )];
    for mut provider in providers::create(&state) {
        let readings = monitor::poll_safely(provider.as_mut());
        if readings.is_empty() {
            lines.push(format!("[{}] nothing found", provider.name()));
        }
        for r in readings {
            lines.push(format!("[{}] {r}", provider.name()));
        }
    }
    // What the window offers then: Windows kept a device from this process, and admin rights would let it in
    if hid::admin_rights_would_help() {
        lines.push("admin rights would let the app read a device Windows keeps from it".into());
    }
    lines
}
