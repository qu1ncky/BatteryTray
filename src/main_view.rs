//! The main window's contents: what each card, row and setting shows, built from the app's state. The window's
//! callbacks go to the app (app.rs).
//! The device lists are changed row by row and never replaced: a row that stays keeps its element, and with it an
//! open menu's anchor, a name being typed, a card being dragged and a running animation.

use crate::app::{with_app, App};
use crate::autostart;
use crate::feedback;
use crate::gui::{self, DeviceItem, Kind, MainWindow};
use crate::hid;
use crate::hotkey::{self, Gesture};
use crate::list_diff::{self, Step};
use crate::loc::{self, t};
use crate::reading::DeviceKind;
use crate::rows::DeviceRow;
use crate::settings::AppSettings;
use crate::sounds::{self, Choice, Event};
use crate::theme;
use crate::update_service::Status;
use crate::updater::Version;
use crate::view::{self, DeviceView};
use crate::win;
use chrono::{DateTime, Local};
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
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
        DeviceKind::Phone => "\u{E8EA}",
        DeviceKind::Tablet => "\u{E70A}",
    }
}

pub struct MainView {
    pub ui: MainWindow,
    pinned: Rc<VecModel<DeviceItem>>,
    connected: Rc<VecModel<DeviceItem>>,
    offline: Rc<VecModel<DeviceItem>>,
    hidden: Rc<VecModel<DeviceItem>>,
    /// The sounds as the lists show them now: a place in a list is a place here, whatever has happened to the Sounds
    /// folder since
    sounds: RefCell<Vec<Choice>>,
    /// Keeps "N min ago" fresh
    _clock: slint::Timer,
}

/// The device list as the window shows it, block by block.
pub type DeviceLists = view::DeviceLists<DeviceItem>;

impl MainView {
    pub fn open(app: &App, owner: windows::Win32::Foundation::HWND) -> Result<Self, slint::PlatformError> {
        let ui = gui::create(Kind::Panel(owner), MainWindow::new)?;
        ui.set_animate_banners(false);
        gui::setup_texts(&ui.global::<gui::T>());
        gui::set_theme(&ui, theme::current().apps_light);
        let view = Self {
            pinned: Rc::new(VecModel::default()),
            connected: Rc::new(VecModel::default()),
            offline: Rc::new(VecModel::default()),
            hidden: Rc::new(VecModel::default()),
            sounds: RefCell::new(Vec::new()),
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

        Ok(view)
    }

    /// The app mode changed (Windows', or the theme picked in the settings): the window and its title bar follow it.
    pub fn follow_theme(&self) {
        let light = theme::current().apps_light;
        gui::set_theme(&self.ui, light);
        if gui::hwnd(self.ui.window()).is_some() {
            gui::set_title_bar(self.ui.window(), light);
        } else {
            // Just opened: winit makes the window only once the event loop gets to it, with the mode the window was
            // opened in
            let ui = self.ui.as_weak();
            slint::Timer::single_shot(Duration::from_millis(100), move || {
                if let Some(ui) = ui.upgrade() {
                    gui::set_title_bar(ui.window(), theme::current().apps_light);
                }
            });
        }
    }

    pub(crate) fn show(&self) {
        let window = self.ui.window();
        let first_show = !window.is_visible();
        if first_show {
            let (area, scale) = gui::primary_monitor();
            let (w, h) = ((440.0 + 16.0) * scale, (self.ui.get_panel_height() + 40.0) * scale);
            window.set_position(slint::PhysicalPosition::new(
                area.right - w as i32 - (12.0 * scale) as i32,
                (area.bottom - h as i32 - (12.0 * scale) as i32).max(area.top),
            ));
        }
        gui::show_and_activate(&self.ui);
        gui::fit_panel(window, self.ui.get_panel_height(), first_show);
        self.ui.set_animate_banners(true);
    }

    /// Applies pending edits before hiding and releasing the panel. Call outside an app state borrow.
    pub(crate) fn dismiss(ui: &MainWindow) {
        ui.invoke_deactivated();
        let _ = ui.hide();
        with_app(|app| app.close_main());
    }

    /// Everything from the settings, as the window opens and after any change: the controls only ever show what the
    /// settings hold (a change the app refuses or adjusts is shown as it ends up).
    pub fn load_settings(&self, app: &App) {
        let s = &app.settings;
        let ui = &self.ui;
        ui.set_low(s.low_percent);
        ui.set_critical(s.critical_percent);
        ui.set_repeat(s.repeat_minutes);
        self.load_sounds(app);
        ui.set_notify_charged(s.notify_charged);
        ui.set_auto_close(s.auto_close);
        ui.set_auto_close_seconds(s.auto_close_seconds);
        ui.set_auto_update(s.auto_update);
        ui.set_overlay_enabled(s.overlay_enabled);
        ui.set_overlay_mode(s.overlay_mode);
        ui.set_overlay_corner(s.overlay_corner);
        ui.set_overlay_names(s.overlay_show_names);
        ui.set_language(s.language.clone().into());
        ui.set_theme(s.theme.clone().into());
        ui.set_tray_battery_icon(s.tray_battery_icon);
        ui.set_tray_priority(s.tray_priority.clone().into());
        ui.set_hide_offline(s.hide_offline);
        ui.set_autostart_banner_dismissed(s.autostart_banner_dismissed);
        self.refresh_overlay_status(s, app.overlay.shortcut_available());
    }

    /// The "Sounds" card: the list every row chooses from (sounds::choices, with "Custom file…" after them), what
    /// each row has chosen, which one is playing, and what is said under a row about its file.
    pub fn load_sounds(&self, app: &App) {
        let ui = &self.ui;
        let choices = sounds::choices(&sounds::files());
        let labels: Vec<SharedString> =
            choices.iter().map(|c| c.label()).chain([t("sounds.file")]).map(SharedString::from).collect();
        // A new model draws an open list again: only when it changed
        if !ui.get_sound_options().iter().eq(labels.iter().cloned()) {
            ui.set_sound_options(ModelRc::new(VecModel::from(labels)));
        }
        *self.sounds.borrow_mut() = choices.clone();
        ui.set_sound_break_a(sounds::BREAKS[0] as i32);
        ui.set_sound_break_b(sounds::BREAKS[1] as i32);
        ui.set_sound_none(choices.iter().position(|c| *c == Choice::None).unwrap_or(0) as i32);
        // A file that is gone from the folder isn't in the list: its row shows the system's sound, which it plays
        let chosen = Event::ALL.map(|e| app.settings.sound_for(e));
        let place = |choice: &Choice| choices.iter().position(|c| c == choice).unwrap_or(0) as i32;
        ui.set_sound_low(place(&chosen[0]));
        ui.set_sound_critical(place(&chosen[1]));
        ui.set_sound_charged(place(&chosen[2]));
        ui.set_sound_playing(app.sound_playing.map_or(-1, |e| e.index() as i32));
        ui.set_sound_refused(app.sound_refused.map_or(-1, |e| e.index() as i32));
        let notes: Vec<SharedString> = Event::ALL
            .iter()
            .zip(&chosen)
            .map(|(event, choice)| match choice {
                _ if app.sound_refused == Some(*event) => t("sounds.error"),
                Choice::File(name) if sounds::is_cut(&sounds::path_of(name)) => t("sounds.long"),
                _ => String::new(),
            })
            .map(SharedString::from)
            .collect();
        if !ui.get_sound_notes().iter().eq(notes.iter().cloned()) {
            ui.set_sound_notes(ModelRc::new(VecModel::from(notes)));
        }
    }

    /// The sound at a place in the lists as they are shown; None for their last line, "Custom file…", which is no
    /// sound.
    pub fn sound_at(&self, index: i32) -> Option<Choice> {
        usize::try_from(index).ok().and_then(|i| self.sounds.borrow().get(i).cloned())
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

        ui.set_updated_text(view::updated_text(rows).into());
    }

    pub fn refresh_update(&self, u: &Status) {
        let v = view::update_view(u);
        self.ui.set_update_checked_text(v.checked.into());
        self.ui.set_install_visible(v.install_visible);
        self.ui.set_install_text(v.install_text.into());
        self.ui.set_check_enabled(v.check_enabled);
        self.ui.set_update_notes(v.notes.into());
        self.ui.set_banner_visible(v.banner_visible);
        self.ui.set_banner_text(v.banner_text.into());
        self.ui.set_banner_button_visible(v.banner_button_visible);
        self.ui.set_update_status(v.status.into());
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
    view::device_lists(rows, s, now).map(device_item)
}

/// One device as the window's rows take it.
pub fn device_item(d: DeviceView) -> DeviceItem {
    DeviceItem {
        glyph: glyph(d.kind).into(),
        id: d.id.into(),
        name: d.name.into(),
        subtitle: d.subtitle.into(),
        connected: d.connected,
        pinned: d.pinned,
        level_text: d.level_text.into(),
        tone: d.tone.index(),
        charging: d.charging,
        indeterminate: d.indeterminate,
        fill: d.fill,
        custom_name: d.custom_name.into(),
        own_name: d.own_name.into(),
        source: d.source.into(),
        alerts: d.alerts,
        low: d.low,
        critical: d.critical,
        own_levels: d.own_levels,
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
    ui.on_theme_chosen(|setting| {
        with_app(|a| a.set_theme(&setting));
    });
    ui.on_tray_battery_icon_chosen(|enabled| {
        with_app(|a| a.edit(|s| s.tray_battery_icon = enabled));
    });
    ui.on_tray_priority_chosen(|p| {
        with_app(|a| {
            a.edit(|s| {
                s.tray_priority = p.to_string();
                s.normalize();
            });
        });
    });
    ui.on_hide_offline_changed(|on| {
        with_app(|a| a.edit(|s| s.hide_offline = on));
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
    ui.on_sound_chosen(|event, index| {
        let Some(event) = Event::from_index(event) else { return };
        match with_app(|a| a.sound_at(index)).flatten() {
            Some(choice) => {
                with_app(|a| a.choose_sound(event, choice));
            }
            // The last line, "Custom file…": Windows' dialog picks the file. From the event loop, with the app's
            // state free: the dialog runs a loop of its own until it closes
            None => slint::Timer::single_shot(Duration::ZERO, move || {
                let owner = with_app(|a| a.main_hwnd()).flatten();
                if let Some(file) = gui::pick_sound_file(owner) {
                    with_app(|a| a.add_sound(event, &file));
                }
            }),
        }
    });
    ui.on_sound_preview(|event| {
        if let Some(event) = Event::from_index(event) {
            with_app(|a| a.preview_sound(event));
        }
    });
    // The ✕ of a file of the user's: the file goes to the Recycle Bin (just deleted, if Windows can't put it there),
    // and out of every list. From the event loop, with the app's state free, as the dialog above: the shell may
    // handle messages while it works
    ui.on_sound_removed(|index| {
        let Some(Choice::File(name)) = with_app(|a| a.sound_at(index)).flatten() else { return };
        slint::Timer::single_shot(Duration::ZERO, move || {
            let path = sounds::path_of(&name);
            if !gui::recycle(&path) {
                if let Err(e) = std::fs::remove_file(&path) {
                    crate::log::write(&format!("sound {name} not removed: {e}"));
                }
            }
            with_app(|a| a.sound_removed(&name));
        });
    });
    ui.on_notify_charged_changed(|on| {
        with_app(|a| a.edit(|s| s.notify_charged = on));
    });
    ui.on_auto_close_changed(|on| {
        with_app(|a| a.edit(|s| s.auto_close = on));
    });
    ui.on_auto_close_seconds_changed(|v| {
        with_app(|a| a.edit(|s| s.set_auto_close_seconds(v)));
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
                MainView::dismiss(&ui);
            }
        });
        slint::CloseRequestResponse::KeepWindowShown
    });

    let weak = ui.as_weak();
    ui.on_panel_height_changed(move || {
        let weak = weak.clone();
        slint::Timer::single_shot(Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade().filter(|ui| ui.window().is_visible()) {
                gui::fit_panel(ui.window(), ui.get_panel_height(), false);
            }
        });
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
            // Back in the window: the Sounds folder may have changed meanwhile (a file put there or deleted by hand)
            WindowEvent::Focused(true) => {
                slint::Timer::single_shot(Duration::ZERO, || {
                    with_app(|a| a.show_sounds());
                });
                EventResult::Propagate
            }
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

    fn ids(list: &[DeviceItem]) -> Vec<&str> {
        list.iter().map(|d| d.id.as_str()).collect()
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
