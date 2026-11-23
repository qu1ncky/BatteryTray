//! The main window's contents: what each card, row and setting shows, built from the app's state. The window's
//! callbacks go to the app (app.rs).
//! The device lists are changed row by row and never replaced: a row that stays keeps its element, and with it an
//! open menu's anchor, a name being typed, a card being dragged and a running animation.

use crate::app::{level_color, with_app, App};
use crate::autostart;
use crate::feedback;
use crate::gui::{self, DeviceItem, Kind, MainWindow};
use crate::hid;
use crate::hotkey::{self, Gesture};
use crate::list_diff::{self, Step};
use crate::loc::{self, t, tf};
use crate::reading::DeviceKind;
use crate::rows::{self, Block, DeviceRow};
use crate::settings::AppSettings;
use crate::update_service::{Stage, Status};
use crate::updater::Version;
use crate::win;
use chrono::{DateTime, Local};
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::time::Duration;

/// Segoe Fluent Icons / MDL2 glyph for a device kind.
pub fn glyph(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Headset => "\u{E7F6}",
        DeviceKind::Mouse => "\u{E962}",
        DeviceKind::Keyboard => "\u{E765}",
        DeviceKind::Gamepad => "\u{E7FC}",
        DeviceKind::Other => "\u{E772}",
    }
}

fn color((r, g, b): (u8, u8, u8)) -> slint::Color {
    slint::Color::from_rgb_u8(r, g, b)
}

pub struct MainView {
    pub ui: MainWindow,
    pinned: Rc<VecModel<DeviceItem>>,
    connected: Rc<VecModel<DeviceItem>>,
    offline: Rc<VecModel<DeviceItem>>,
    hidden: Rc<VecModel<DeviceItem>>,
    /// Keeps "N min ago" fresh
    _clock: slint::Timer,
}

/// The device list as the window shows it, block by block.
pub struct DeviceLists {
    pub pinned: Vec<DeviceItem>,
    pub connected: Vec<DeviceItem>,
    pub offline: Vec<DeviceItem>,
    pub hidden: Vec<DeviceItem>,
    /// No device of the first two blocks is connected: the window says so instead
    pub none_online: bool,
}

impl MainView {
    pub fn open(app: &App) -> Result<Self, slint::PlatformError> {
        let ui = gui::create(Kind::Normal, MainWindow::new)?;
        gui::setup_texts(&ui.global::<gui::T>());
        let view = Self {
            pinned: Rc::new(VecModel::default()),
            connected: Rc::new(VecModel::default()),
            offline: Rc::new(VecModel::default()),
            hidden: Rc::new(VecModel::default()),
            _clock: slint::Timer::default(),
            ui,
        };
        // Handed over once: a model given again would build every row anew
        view.ui.set_pinned_devices(ModelRc::from(view.pinned.clone()));
        view.ui.set_connected_devices(ModelRc::from(view.connected.clone()));
        view.ui.set_offline_devices(ModelRc::from(view.offline.clone()));
        view.ui.set_hidden_devices(ModelRc::from(view.hidden.clone()));
        view.ui.set_version_text(format!("BatteryTray {}", Version::current()).into());
        connect(&view.ui);
        view._clock.start(slint::TimerMode::Repeated, Duration::from_secs(15), || {
            with_app(|app| app.refresh_main());
        });

        view.load_settings(app);
        // Asked once here (and after a change): it means a call to the Task Scheduler
        view.ui.set_autostart(autostart::is_registered());
        view.refresh(&app.rows(), &app.settings);
        view.refresh_update(&app.update_status());

        // First shown in the bottom right corner, near the tray icon
        let area = gui::primary_work_area();
        let scale = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() } as f32 / 96.0;
        let (w, h) = ((440.0 + 16.0) * scale, (640.0 + 40.0) * scale);
        view.ui.window().set_position(slint::PhysicalPosition::new(
            area.right - w as i32 - (12.0 * scale) as i32,
            area.bottom - h as i32 - (12.0 * scale) as i32,
        ));
        Ok(view)
    }

    /// Everything from the settings, as the window opens and after any change: the controls only ever show what the
    /// settings hold (a change the app refuses or adjusts is shown as it ends up).
    pub fn load_settings(&self, app: &App) {
        let s = &app.settings;
        let ui = &self.ui;
        ui.set_low(s.low_percent);
        ui.set_critical(s.critical_percent);
        ui.set_repeat(s.repeat_minutes);
        ui.set_sound(s.sound);
        ui.set_notify_charged(s.notify_charged);
        ui.set_auto_update(s.auto_update);
        ui.set_overlay_enabled(s.overlay_enabled);
        ui.set_overlay_mode(s.overlay_mode);
        ui.set_overlay_corner(s.overlay_corner);
        ui.set_overlay_names(s.overlay_show_names);
        ui.set_language(s.language.clone().into());
        ui.set_autostart_banner_dismissed(s.autostart_banner_dismissed);
        self.refresh_overlay_status(s, app.overlay.shortcut_available());
    }

    /// The shortcut as key caps, and why it doesn't work when it can't be registered.
    pub fn refresh_overlay_status(&self, s: &AppSettings, available: bool) {
        let gesture = Gesture { key: s.overlay_hotkey_key, modifiers: s.overlay_hotkey_modifiers };
        let keys: Vec<SharedString> = hotkey::parts(gesture).into_iter().map(SharedString::from).collect();
        // A new model draws the caps again: only when they changed
        if !self.ui.get_shortcut_keys().iter().eq(keys.iter().cloned()) {
            self.ui.set_shortcut_keys(ModelRc::new(VecModel::from(keys)));
        }
        self.ui.set_shortcut_text(hotkey::label(gesture).into());
        let error = if s.overlay_enabled && !available { t("overlay.unavailable") } else { String::new() };
        self.ui.set_shortcut_error(error.into());
    }

    /// A shortcut that can't be taken: says why, in red, until the next change.
    pub fn shortcut_refused(&self, valid: bool) {
        self.ui.set_shortcut_error(t(if valid { "overlay.conflict" } else { "overlay.invalid" }).into());
    }

    pub fn refresh(&self, rows: &[DeviceRow], s: &AppSettings) {
        let ui = &self.ui;
        let mut lists = device_lists(rows, s, Local::now());
        ui.set_none_online(lists.none_online);

        // A menu or a name field belongs to its device: when the device leaves the list, it goes too
        let shown = |id: &SharedString| {
            [&lists.pinned, &lists.connected, &lists.offline].iter().any(|l| l.iter().any(|d| d.id == *id))
        };
        let menu = ui.get_menu_id();
        if !menu.is_empty() && !shown(&menu) {
            ui.invoke_close_menu();
        }
        let renaming = ui.get_renaming_id();
        if !renaming.is_empty() && !shown(&renaming) {
            ui.invoke_cancel_rename();
        }
        // A name being typed keeps its field: the device stays in the block it is shown in until the rename ends, and
        // the app refreshes the list then
        let renaming = ui.get_renaming_id();
        if !renaming.is_empty() {
            let models = [&self.pinned, &self.connected, &self.offline];
            let home =
                models.iter().enumerate().find_map(|(i, m)| m.iter().position(|d| d.id == renaming).map(|at| (i, at)));
            if let Some((home, index)) = home {
                hold_renaming(&mut lists, home, index, &renaming);
            }
        }

        let drag = ui.get_drag_list();
        let blocks = [
            (&self.pinned, lists.pinned),
            (&self.connected, lists.connected),
            (&self.offline, lists.offline),
            (&self.hidden, lists.hidden),
        ];
        for (i, (model, fresh)) in blocks.into_iter().enumerate() {
            // A drag is laid out over the rows it started with: when they change it ends, and nothing moves
            if drag == i as i32 && !same_ids(model, &fresh) {
                ui.invoke_cancel_drag();
            }
            sync(model, fresh, &renaming);
        }

        // Windows keeps a device from the app and admin rights would get it in: whatever the device's row says, if the
        // device is listed at all
        ui.set_admin_banner(!s.admin_banner_dismissed && !win::is_elevated() && hid::admin_rights_would_help());

        let newest = rows.iter().filter_map(|r| r.current.as_ref().map(|c| c.time)).max();
        ui.set_updated_text(
            match newest {
                Some(t) => tf("updated.at", &[&t.format("%H:%M:%S")]),
                None => t("polling"),
            }
            .into(),
        );
    }

    pub fn refresh_update(&self, u: &Status) {
        let version = Version::current().to_string();
        let latest = u.latest.as_ref().map(|l| l.version.to_string()).unwrap_or_default();
        let status = match u.stage {
            Stage::Checking => t("upd.checking"),
            Stage::UpToDate => tf("upd.uptodate", &[&version]),
            Stage::Available => tf("upd.available", &[&latest, &version]),
            Stage::Installing if u.progress > 0 && u.progress < 100 => tf("upd.downloading", &[&latest, &u.progress]),
            Stage::Installing => tf("upd.installing", &[&latest]),
            Stage::Failed => tf("upd.failed", &[&u.error.clone().unwrap_or_default()]),
            Stage::NotChecked => tf("upd.idle", &[&version]),
        };
        // When it was last asked, as a second line while there is nothing newer
        let checked = match u.last_check {
            Some(c) if u.stage == Stage::UpToDate => tf("upd.checked", &[&c.format("%H:%M")]),
            _ => String::new(),
        };
        self.ui.set_update_checked_text(checked.into());
        let newer = u.newer_available();
        self.ui.set_install_visible(newer && matches!(u.stage, Stage::Available | Stage::Failed));
        self.ui.set_install_text(tf("upd.install", &[&latest]).into());
        self.ui.set_check_enabled(!matches!(u.stage, Stage::Checking | Stage::Installing));

        // Release notes are Markdown; shown as plain text
        let notes = if newer {
            u.latest.as_ref().map(|l| l.notes.replace("**", "").replace('`', "").trim().to_string())
        } else {
            None
        }
        .unwrap_or_default();
        let notes = if notes.chars().count() > 600 {
            notes.chars().take(600).collect::<String>().trim_end().to_string() + "…"
        } else {
            notes
        };
        self.ui.set_update_notes(notes.into());

        self.ui.set_banner_visible(newer && matches!(u.stage, Stage::Available | Stage::Installing));
        self.ui.set_banner_text(
            if u.stage == Stage::Installing { status.clone() } else { tf("upd.banner", &[&latest]) }.into(),
        );
        self.ui.set_banner_button_visible(u.stage == Stage::Available);
        self.ui.set_update_status(status.into());
    }
}

/// Brings a block of the list up to date row by row: a row that stays keeps its element (an open menu's anchor, a
/// name being typed, a card being dragged, a running animation), only what changed is replaced.
fn sync(model: &VecModel<DeviceItem>, fresh: Vec<DeviceItem>, renaming: &SharedString) {
    let shown: Vec<DeviceItem> = model.iter().collect();
    let keep = (!renaming.is_empty()).then(|| renaming.clone());
    for step in list_diff::steps(&shown, fresh, |d| d.id.clone(), keep) {
        match step {
            Step::Remove(i) => {
                model.remove(i);
            }
            Step::Insert(i, d) => model.insert(i, d),
            Step::Replace(i, d) => model.set_row_data(i, d),
        }
    }
}

fn same_ids(model: &VecModel<DeviceItem>, fresh: &[DeviceItem]) -> bool {
    model.row_count() == fresh.len() && model.iter().zip(fresh).all(|(shown, f)| shown.id == f.id)
}

/// Keeps the device whose name is being typed in list `home` (0 pinned, 1 connected, 2 offline), where its field is:
/// with its fresh data, where the fresh list has it when its block did not change, else at `index`, its place in the
/// block it is shown in. Every other row stays where the fresh lists put it.
fn hold_renaming(lists: &mut DeviceLists, home: usize, index: usize, id: &str) {
    let blocks = [&mut lists.pinned, &mut lists.connected, &mut lists.offline];
    let Some((from, at)) =
        blocks.iter().enumerate().find_map(|(b, l)| l.iter().position(|d| d.id == id).map(|at| (b, at)))
    else {
        return;
    };
    if from != home {
        let item = blocks[from].remove(at);
        let len = blocks[home].len();
        blocks[home].insert(index.min(len), item);
    }
}

/// The rows in the list's order, each in the block it is shown in.
pub fn device_lists(rows: &[DeviceRow], s: &AppSettings, now: DateTime<Local>) -> DeviceLists {
    let mut lists = DeviceLists {
        pinned: Vec::new(),
        connected: Vec::new(),
        offline: Vec::new(),
        hidden: Vec::new(),
        none_online: true,
    };
    for row in rows::list_order(rows, s) {
        let block = rows::block(row, s);
        // A pinned device is shown with or without a connection; a hidden one doesn't count
        if row.online() && matches!(block, Block::Pinned | Block::Connected) {
            lists.none_online = false;
        }
        let item = device_item(row, s, now);
        match block {
            Block::Pinned => lists.pinned.push(item),
            Block::Connected => lists.connected.push(item),
            Block::Offline => lists.offline.push(item),
            Block::Hidden => lists.hidden.push(item),
        }
    }
    lists
}

/// One device as any part of the list shows it (a card, a "Not connected" row, a hidden row), and what its menu and
/// its name field start from.
pub fn device_item(row: &DeviceRow, s: &AppSettings, now: DateTime<Local>) -> DeviceItem {
    let mut parts = Vec::new();
    let (mut level, mut estimate, mut unknown, mut charging) = (None, false, false, false);
    let mut level_rgb = (0x7A, 0x7A, 0x7A);
    if row.online() {
        let cur = row.current.as_ref().expect("online rows have a reading");
        level = cur.level;
        estimate = cur.estimate;
        unknown = cur.charging && level.is_none();
        if cur.charging {
            parts.push(t("state.charging"));
        } else if cur.full_on_charger {
            parts.push(t("state.full"));
        }
        parts.push(cur.via.clone());
        if let Some(detail) = cur.detail.as_ref().filter(|d| !d.is_empty()) {
            parts.push(detail.clone());
        }
        charging = cur.charging;
        level_rgb = level_color(level, cur.charging, true, &row.id, s);
    } else {
        // The reason, when the provider knows it ("asleep or off"), reads better than a bare "Not connected"
        let why = row.current.as_ref().and_then(|c| c.detail.clone()).filter(|d| !d.is_empty());
        parts.push(match why {
            Some(why) => {
                let mut chars = why.chars();
                chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
            }
            None => t("state.offline"),
        });
        if let Some(last) = &row.last {
            if let Some(was) = last.level {
                parts.push(tf("last.seen", &[&was, &ago(last.time, now)]));
            }
        }
    }
    let ds = s.device(&row.id);
    let any = row.any();
    DeviceItem {
        id: row.id.clone().into(),
        name: row.name.clone().into(),
        glyph: glyph(row.kind()).into(),
        subtitle: parts.join(" · ").into(),
        connected: row.online(),
        pinned: s.is_pinned(&row.id),
        // An estimate is marked as one; while charging with no readable level the lightning alone says it all
        level_text: match level {
            Some(l) => format!("{}{l}%", if estimate { "≈" } else { "" }),
            None if unknown => String::new(),
            None => "—".into(),
        }
        .into(),
        color: color(level_rgb),
        charging,
        indeterminate: unknown,
        fill: (level.unwrap_or(0) as f32 / 100.0).clamp(0.0, 1.0),
        custom_name: ds.name.clone().unwrap_or_default().into(),
        own_name: any.name.clone().into(),
        source: format!("{} · {}", any.via, any.name).into(),
        alerts: ds.alerts,
        low: s.low_for(&row.id),
        critical: s.critical_for(&row.id),
        own_levels: ds.low_percent.is_some() || ds.critical_percent.is_some(),
    }
}

fn ago(time: DateTime<Local>, now: DateTime<Local>) -> String {
    let d = now - time;
    if d.num_minutes() < 1 {
        t("ago.now")
    } else if d.num_hours() < 1 {
        tf("ago.min", &[&d.num_minutes()])
    } else if d.num_days() < 1 {
        tf("ago.hours", &[&d.num_hours()])
    } else {
        loc::date_time(&time)
    }
}

/// The window's callbacks: each one changes the app and lets it refresh what depends on it.
fn connect(ui: &MainWindow) {
    ui.on_refresh(|| {
        with_app(|a| a.refresh_now());
    });
    ui.on_install_update(|| {
        with_app(|a| a.updates_install());
    });
    ui.on_check_updates(|| {
        with_app(|a| a.updates_check());
    });
    ui.on_language_chosen(|l| {
        with_app(|a| a.set_language(&l));
    });
    ui.on_test_alert(|| {
        with_app(|a| a.show_test_alert());
    });
    ui.on_open_folder(|| {
        let _ = std::process::Command::new("explorer.exe").arg(crate::log::folder()).spawn();
    });
    ui.on_suggest_improvement(|| {
        let url = feedback::issue_url(&Version::current().to_string(), win::windows_build(), loc::current());
        // The shell is another process: the window doesn't wait for it
        std::thread::spawn(move || feedback::open(&url));
    });
    ui.on_autostart_changed(|on| {
        with_app(|a| a.set_autostart(on));
    });
    ui.on_dismiss_autostart_banner(|| {
        with_app(|a| a.dismiss_autostart_banner());
    });
    ui.on_dismiss_admin_banner(|| {
        with_app(|a| a.dismiss_admin_banner());
    });
    ui.on_grant_admin(|| {
        with_app(|a| a.grant_admin());
    });
    ui.on_overlay_reset(|| {
        with_app(|a| {
            a.set_overlay_shortcut(Gesture {
                key: crate::settings::DEFAULT_HOTKEY.0,
                modifiers: crate::settings::DEFAULT_HOTKEY.1,
            })
        });
    });
    ui.on_overlay_preview(|| {
        with_app(|a| a.overlay_preview());
    });

    // Typed values come in as they were typed; the settings keep them in range and 5 apart
    ui.on_low_changed(|v| {
        with_app(|a| a.edit(|s| s.set_low(v)));
    });
    ui.on_critical_changed(|v| {
        with_app(|a| a.edit(|s| s.set_critical(v)));
    });
    ui.on_repeat_changed(|v| {
        with_app(|a| a.edit(|s| s.set_repeat(v)));
    });
    ui.on_sound_changed(|on| {
        with_app(|a| a.edit(|s| s.sound = on));
    });
    ui.on_notify_charged_changed(|on| {
        with_app(|a| a.edit(|s| s.notify_charged = on));
    });
    ui.on_auto_update_changed(|on| {
        with_app(|a| a.edit(|s| s.auto_update = on));
    });
    ui.on_overlay_enabled_changed(|on| {
        with_app(|a| a.edit(|s| s.overlay_enabled = on));
    });
    ui.on_overlay_mode_chosen(|m| {
        with_app(|a| a.edit(|s| s.overlay_mode = m));
    });
    ui.on_overlay_corner_chosen(|c| {
        with_app(|a| a.edit(|s| s.overlay_corner = c));
    });
    ui.on_overlay_names_changed(|on| {
        with_app(|a| a.edit(|s| s.overlay_show_names = on));
    });

    ui.on_device_renamed(|id, text| {
        with_app(|a| a.rename_device(&id, &text));
    });
    ui.on_device_pinned_changed(|id, pinned| {
        with_app(|a| a.pin_device(&id, pinned));
    });
    ui.on_device_hidden_changed(|id, hidden| {
        with_app(|a| a.edit(|s| s.device_mut(&id).hidden = hidden));
    });
    ui.on_device_moved(|id, onto| {
        with_app(|a| a.move_device(&id, &onto));
    });
    ui.on_device_alerts_changed(|id, on| {
        with_app(|a| a.edit(|s| s.device_mut(&id).alerts = on));
    });
    ui.on_device_low_changed(|id, v| {
        with_app(|a| a.edit(|s| s.set_device_low(&id, v)));
    });
    ui.on_device_critical_changed(|id, v| {
        with_app(|a| a.edit(|s| s.set_device_critical(&id, v)));
    });
    ui.on_device_levels_reset(|id| {
        with_app(|a| a.edit(|s| s.reset_device_levels(&id)));
    });

    // Closing drops the window and everything it holds; it is built again the next time it opens. What is open in
    // it is settled first, as when it loses the focus: a name being typed is saved
    let weak = ui.as_weak();
    ui.window().on_close_requested(move || {
        let weak = weak.clone();
        slint::Timer::single_shot(Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_deactivated();
            }
            with_app(|a| a.close_main());
        });
        slint::CloseRequestResponse::HideWindow
    });

    // What Slint has no window-wide hook for, taken from winit before Slint sees the event. Whatever it starts in
    // the window runs later, from the event loop: Windows sends these also while the app is in the middle of
    // something, and a window callback reached then would find the app busy and be dropped
    let weak = ui.as_weak();
    ui.window().on_winit_window_event(move |_, event| {
        use winit::event::{ElementState, WindowEvent};
        use winit::keyboard::{Key, NamedKey};
        use winit::platform::scancode::PhysicalKeyExtScancode;
        let Some(ui) = weak.upgrade() else { return EventResult::Propagate };
        match event {
            // A menu, a list or a tooltip doesn't stay open in a window that lost the focus, and a name being typed
            // is saved
            WindowEvent::Focused(false) => {
                let weak = ui.as_weak();
                slint::Timer::single_shot(Duration::ZERO, move || {
                    if let Some(ui) = weak.upgrade() {
                        // The release of a button that is down goes to another window: whatever holds the pointer
                        // (a grip being dragged) has to be told it is gone, or it eats the next click
                        ui.window().dispatch_event(slint::platform::WindowEvent::PointerExited);
                        ui.invoke_deactivated();
                    }
                });
                EventResult::Propagate
            }
            // Escape is the window's whatever has the focus (also nothing, also the shortcut field: it ends the
            // recording). The logical key: an input method that takes Escape for itself reports another one
            WindowEvent::KeyboardInput { event, .. } if event.logical_key == Key::Named(NamedKey::Escape) => {
                if event.state == ElementState::Pressed && !event.repeat {
                    let weak = ui.as_weak();
                    slint::Timer::single_shot(Duration::ZERO, move || {
                        if let Some(ui) = weak.upgrade() {
                            let renaming = !ui.get_renaming_id().is_empty();
                            ui.invoke_escape();
                            // A cancelled rename raises nothing, but the list may have held its device back
                            if renaming && ui.get_renaming_id().is_empty() {
                                with_app(|a| a.refresh_main());
                            }
                        }
                    });
                }
                EventResult::PreventDefault
            }
            // The shortcut field records the next key combination from the raw key event: the physical key, so the
            // same shortcut on every keyboard layout
            WindowEvent::KeyboardInput { event, .. } if ui.get_recording_shortcut() => {
                let Some(scancode) = event.physical_key.to_scancode() else { return EventResult::Propagate };
                let vk = hotkey::virtual_key(scancode);
                if vk == 0x09 {
                    return EventResult::Propagate; // keyboard users can leave the field
                }
                if event.state == ElementState::Pressed && !event.repeat && !hotkey::is_modifier(vk) {
                    let gesture = Gesture { key: vk as i32, modifiers: hotkey::modifiers_down() };
                    slint::Timer::single_shot(Duration::ZERO, move || {
                        with_app(|a| a.set_overlay_shortcut(gesture));
                    });
                }
                EventResult::PreventDefault
            }
            _ => EventResult::Propagate,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::DeviceReading;

    fn row(id: &str, kind: DeviceKind, online: bool) -> DeviceRow {
        let reading = DeviceReading { online, level: Some(50), ..DeviceReading::new(id, id, kind, "2.4 GHz") };
        DeviceRow { id: id.into(), current: Some(reading), last: None, name: id.into() }
    }

    fn ids(list: &[DeviceItem]) -> Vec<&str> {
        list.iter().map(|d| d.id.as_str()).collect()
    }

    #[test]
    fn a_device_is_listed_in_one_block_in_list_order() {
        let rows = [
            row("mouse", DeviceKind::Mouse, false),
            row("hidden", DeviceKind::Headset, true),
            row("keyboard", DeviceKind::Keyboard, true),
            row("pad", DeviceKind::Gamepad, false),
            row("headset", DeviceKind::Headset, true),
            row("pinned-mouse", DeviceKind::Mouse, true),
        ];
        let mut s = AppSettings::default();
        s.device_mut("pinned-mouse").pinned = true;
        s.device_mut("pad").pinned = true;
        // Hiding a pinned device keeps its pin, and it is shown with the hidden ones only
        s.device_mut("hidden").pinned = true;
        s.device_mut("hidden").hidden = true;
        s.device_order = vec!["keyboard".into(), "headset".into()];
        let lists = device_lists(&rows, &s, Local::now());
        assert_eq!(ids(&lists.pinned), ["pinned-mouse", "pad"]);
        assert_eq!(ids(&lists.connected), ["keyboard", "headset"]);
        assert_eq!(ids(&lists.offline), ["mouse"]);
        assert_eq!(ids(&lists.hidden), ["hidden"]);
        let pad = &lists.pinned[1];
        assert_eq!((pad.connected, pad.pinned, pad.level_text.as_str()), (false, true, "—"));
    }

    #[test]
    fn no_device_online_counts_pinned_ones() {
        let mut s = AppSettings::default();
        assert!(device_lists(&[], &s, Local::now()).none_online, "no devices at all");

        let rows = [row("pinned", DeviceKind::Mouse, true), row("away", DeviceKind::Keyboard, false)];
        s.device_mut("pinned").pinned = true;
        assert!(!device_lists(&rows, &s, Local::now()).none_online, "a pinned device is connected");

        s.device_mut("pinned").hidden = true;
        assert!(device_lists(&rows, &s, Local::now()).none_online, "a hidden one doesn't count");
    }

    fn item(id: &str, subtitle: &str) -> DeviceItem {
        DeviceItem { id: id.into(), subtitle: subtitle.into(), ..Default::default() }
    }

    fn lists(pinned: &[&str], connected: &[&str], offline: &[&str]) -> DeviceLists {
        let block = |ids: &[&str]| ids.iter().map(|id| item(id, "")).collect();
        DeviceLists {
            pinned: block(pinned),
            connected: block(connected),
            offline: block(offline),
            hidden: Vec::new(),
            none_online: false,
        }
    }

    fn blocks(lists: &DeviceLists) -> [Vec<&str>; 3] {
        [ids(&lists.pinned), ids(&lists.connected), ids(&lists.offline)]
    }

    #[test]
    fn a_device_being_renamed_stays_where_it_is_shown() {
        // Its block did not change: where the fresh list has it
        let mut fresh = lists(&["a"], &["b", "d", "c"], &["e"]);
        hold_renaming(&mut fresh, 1, 1, "c");
        assert_eq!(blocks(&fresh), [vec!["a"], vec!["b", "d", "c"], vec!["e"]]);

        // It went offline while its name is typed on a card: it stays a card, at its place, with what it shows now
        let mut fresh = lists(&["a"], &["b", "d"], &["c", "e"]);
        fresh.offline[0].subtitle = "not connected".into();
        hold_renaming(&mut fresh, 1, 1, "c");
        assert_eq!(blocks(&fresh), [vec!["a"], vec!["b", "c", "d"], vec!["e"]]);
        assert_eq!(fresh.connected[1].subtitle, "not connected");

        // Its old place is past the end of a block that got shorter
        let mut fresh = lists(&["c"], &["b"], &[]);
        hold_renaming(&mut fresh, 2, 4, "c");
        assert_eq!(blocks(&fresh), [vec![], vec!["b"], vec!["c"]]);
    }
}
