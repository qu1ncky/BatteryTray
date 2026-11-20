//! The battery overlay: a click-through panel
//! over the active app, shown by the global shortcut for 5 seconds or until pressed again.

use crate::app::level_color;
use crate::gui::{self, Kind, OverlayEntry, OverlayRow, OverlayWindow};
use crate::hotkey::{Binding, Gesture, WindowHotkeys};
use crate::reading::DeviceKind;
use crate::rows::DeviceRow;
use crate::settings::AppSettings;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, RECT};

/// Visibility on a monotonic clock: device updates and wall-clock changes can't extend a peek.
#[derive(Default)]
pub struct Session {
    visible: bool,
    timed: bool,
    started: Option<Instant>,
}

impl Session {
    pub const DURATION: Duration = Duration::from_secs(5);

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn is_timed(&self) -> bool {
        self.visible && self.timed
    }

    /// Mode 0 shows for 5 seconds (again from the start on each press); mode 1 toggles.
    pub fn press(&mut self, mode: i32, now: Instant) {
        if mode == 1 && self.visible {
            self.hide();
        } else {
            self.show(mode != 1, now);
        }
    }

    pub fn preview(&mut self, now: Instant) {
        self.show(true, now);
    }

    fn show(&mut self, expire: bool, now: Instant) {
        self.visible = true;
        self.timed = expire;
        self.started = Some(now);
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn expire(&mut self, now: Instant) {
        if self.visible
            && self.timed
            && self.started.is_some_and(|s| now.saturating_duration_since(s) >= Self::DURATION)
        {
            self.hide();
        }
    }
}

/// The devices the overlay shows: on the air and not hidden, in the order of the list (never by level). Names are
/// shown for kinds that occur more than once, or for all when the settings say so.
pub fn select<'a>(rows: &'a [DeviceRow], settings: &AppSettings) -> Vec<(&'a DeviceRow, bool)> {
    let shown: Vec<&DeviceRow> = crate::rows::list_order(rows, settings)
        .into_iter()
        .filter(|r| r.online() && !settings.device(&r.id).hidden)
        .collect();
    let kinds: Vec<DeviceKind> = shown.iter().map(|r| r.kind()).collect();
    let duplicate = |k: DeviceKind| kinds.iter().filter(|&&x| x == k).count() > 1;
    shown.into_iter().map(|r| (r, settings.overlay_show_names || duplicate(r.kind()))).collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// As WPF's IntersectsWith: touching edges count.
    pub fn intersects(&self, o: &Rect) -> bool {
        o.x <= self.right() && o.right() >= self.x && o.y <= self.bottom() && o.bottom() >= self.y
    }

    pub fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }
}

/// Where the overlay goes: its corner of the work area inside a margin, moved off the reminder when they would
/// overlap (to the nearest place that fits). All in the same units (physical pixels).
pub fn place(area: Rect, size: (f64, f64), corner: i32, margin: f64, obstruction: Option<Rect>) -> (f64, f64) {
    let inset = margin.min(area.w.min(area.h) / 2.0);
    let inner = Rect {
        x: area.x + inset,
        y: area.y + inset,
        w: (area.w - inset * 2.0).max(0.0),
        h: (area.h - inset * 2.0).max(0.0),
    };
    let (w, h) = (size.0.min(inner.w), size.1.min(inner.h));
    let right = corner == 1 || corner == 3;
    let bottom = corner == 2 || corner == 3;
    let point = (if right { inner.right() - w } else { inner.x }, if bottom { inner.bottom() - h } else { inner.y });
    let Some(blocked) = obstruction else { return point };
    let at = |p: (f64, f64)| Rect { x: p.0, y: p.1, w, h };
    if !at(point).intersects(&blocked) {
        return point;
    }
    let alternatives = [
        (point.0, blocked.y - margin - h),
        (point.0, blocked.bottom() + margin),
        (blocked.x - margin - w, point.1),
        (blocked.right() + margin, point.1),
    ];
    alternatives
        .into_iter()
        .filter(|&p| inner.contains(&at(p)) && !at(p).intersects(&blocked))
        .min_by(|a, b| {
            let d = |p: (f64, f64)| (p.0 - point.0).powi(2) + (p.1 - point.1).powi(2);
            d(*a).total_cmp(&d(*b))
        })
        .unwrap_or(point)
}

/// Owns the overlay window and its shortcut.
pub struct Overlay {
    binding: Binding<WindowHotkeys>,
    session: Session,
    window: Option<OverlayWindow>,
    rows: Rc<VecModel<OverlayRow>>,
    /// What `rows` was made of: the entries and the width they were wrapped to
    shown: (Vec<OverlayEntry>, f32),
    expiry: slint::Timer,
    placement: slint::Timer,
    mode: i32,
    enabled: bool,
    /// Shared with the window's resize handler
    corner: Rc<std::cell::Cell<i32>>,
    /// Shown but not yet seen: waiting for its real size (as the reminder, see alert_view.rs)
    revealing: Rc<std::cell::Cell<bool>>,
    reveal_timer: Rc<slint::Timer>,
    /// While the shortcut is being recorded in the main window, pressing it does nothing.
    pub suspended: bool,
}

/// How opaque the overlay is: the C# overlay's background was #ED202020, 93%.
const ALPHA: u8 = 237;

/// In `ms`, unless its size changes again first: into its corner, and seen. The first time, Windows creates the
/// window only once the event loop gets to it after show(); until then this waits for it.
fn reveal_after(
    timer: &Rc<slint::Timer>,
    w: &OverlayWindow,
    corner: &Rc<std::cell::Cell<i32>>,
    revealing: &Rc<std::cell::Cell<bool>>,
    ms: u64,
) {
    let weak = w.as_weak();
    let (again, corner, revealing) = (timer.clone(), corner.clone(), revealing.clone());
    timer.start(slint::TimerMode::SingleShot, Duration::from_millis(ms), move || {
        let Some(w) = weak.upgrade() else { return };
        if gui::hwnd(w.window()).is_none() {
            reveal_after(&again, &w, &corner, &revealing, 20);
            return;
        }
        place_window(w.window(), corner.get(), None);
        // Off the reminder, which only the app knows about
        crate::app::with_app(|app| app.overlay_place());
        if revealing.replace(false) {
            gui::set_alpha(w.window(), ALPHA);
        }
    });
}

impl Overlay {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            binding: Binding::new(WindowHotkeys(hwnd)),
            session: Session::default(),
            window: None,
            rows: Rc::new(VecModel::default()),
            shown: (Vec::new(), 0.0),
            expiry: slint::Timer::default(),
            placement: slint::Timer::default(),
            mode: 0,
            enabled: false,
            corner: Rc::new(std::cell::Cell::new(1)),
            revealing: Rc::new(std::cell::Cell::new(false)),
            reveal_timer: Rc::new(slint::Timer::default()),
            suspended: false,
        }
    }

    /// The panel's window, once it has been shown.
    pub fn window(&self) -> Option<&slint::Window> {
        self.window.as_ref().map(|w| w.window())
    }

    pub fn is_timed_visible(&self) -> bool {
        self.session.is_timed()
    }

    pub fn shortcut_available(&self) -> bool {
        self.binding.active.is_some()
    }

    /// Registers or releases the shortcut as the settings say; a changed mode or turning it off hides the panel.
    pub fn apply_settings(&mut self, s: &AppSettings) {
        if self.mode != s.overlay_mode || (self.enabled && !s.overlay_enabled) {
            self.hide();
        }
        self.mode = s.overlay_mode;
        self.enabled = s.overlay_enabled;
        self.corner.set(s.overlay_corner);
        if !self.enabled {
            self.binding.clear();
        } else {
            self.binding.try_change(Gesture { key: s.overlay_hotkey_key, modifiers: s.overlay_hotkey_modifiers });
        }
    }

    /// Takes a new shortcut if it is valid and free (checked even while the overlay is off, then released again).
    pub fn try_set_shortcut(&mut self, gesture: Gesture, s: &mut AppSettings) -> bool {
        if !gesture.is_valid() || !self.binding.try_change(gesture) {
            return false;
        }
        s.overlay_hotkey_key = gesture.key;
        s.overlay_hotkey_modifiers = gesture.modifiers;
        if !s.overlay_enabled {
            self.binding.clear();
        }
        true
    }

    /// WM_HOTKEY; true when the overlay's visibility changed.
    pub fn on_hotkey(&mut self, id: i32, rows: &[DeviceRow], s: &AppSettings, obstruction: Option<Rect>) {
        if self.suspended || !self.binding.is_current(id) {
            return;
        }
        self.session.press(self.mode, Instant::now());
        self.present(rows, s, obstruction);
    }

    pub fn preview(&mut self, rows: &[DeviceRow], s: &AppSettings, obstruction: Option<Rect>) {
        self.session.preview(Instant::now());
        self.present(rows, s, obstruction);
    }

    fn present(&mut self, rows: &[DeviceRow], s: &AppSettings, obstruction: Option<Rect>) {
        if !self.session.is_visible() {
            self.hide();
            return;
        }
        if self.window.is_none() {
            match gui::create(Kind::Overlay, OverlayWindow::new) {
                Ok(w) => {
                    gui::setup_texts(&w.global::<gui::T>());
                    w.set_rows(ModelRc::from(self.rows.clone()));
                    // Whenever its size changes (it gets its real one, a device comes or goes, another scale): back
                    // into its corner; while still unseen, shown once its size has stayed the same for a moment. The
                    // reminder it stays clear of is looked up afterwards, outside this event.
                    let weak = w.as_weak();
                    let (corner, revealing, timer) =
                        (Rc::clone(&self.corner), Rc::clone(&self.revealing), Rc::clone(&self.reveal_timer));
                    w.window().on_winit_window_event(move |_, event| {
                        use slint::winit_030::winit::event::WindowEvent;
                        if matches!(event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
                            if let Some(w) = weak.upgrade() {
                                place_window(w.window(), corner.get(), None);
                                if revealing.get() {
                                    reveal_after(&timer, &w, &corner, &revealing, 40);
                                } else {
                                    slint::Timer::single_shot(Duration::ZERO, || {
                                        crate::app::with_app(|app| app.overlay_place());
                                    });
                                }
                            }
                        }
                        slint::winit_030::EventResult::Propagate
                    });
                    self.window = Some(w);
                }
                Err(e) => {
                    crate::log::write(&format!("overlay: {e}"));
                    self.session.hide();
                    return;
                }
            }
        }
        self.refresh(rows, s, obstruction);
        if let Some(w) = &self.window {
            if !w.window().is_visible() {
                // On the monitor of the app in front, where the user is looking, and already in its corner when it
                // appears: its size worked out from its contents at that monitor's scale
                let (area, scale) = gui::active_monitor();
                let scale = scale as f64;
                let size = (w.get_content_width() as f64 * scale, w.get_content_height() as f64 * scale);
                let bounds = Rect {
                    x: area.left as f64,
                    y: area.top as f64,
                    w: (area.right - area.left) as f64,
                    h: (area.bottom - area.top) as f64,
                };
                let (x, y) = place(bounds, size, self.corner.get(), 12.0 * scale, obstruction);
                w.window().set_position(slint::PhysicalPosition::new(x.round() as i32, y.round() as i32));
                // Unseen until it has its real size in its corner
                self.revealing.set(true);
                gui::set_alpha(w.window(), 0); // shown before: hidden again until it stands in its corner
                let _ = w.show();
                reveal_after(&self.reveal_timer, w, &self.corner, &self.revealing, 80);
            }
        }
        let weak = self.window.as_ref().map(|w| w.as_weak());
        if self.session.is_timed() {
            self.expiry.start(slint::TimerMode::Repeated, Duration::from_millis(100), || {
                crate::app::with_app(|app| app.overlay_tick());
            });
        } else {
            self.expiry.stop();
        }
        // Device lists change size and displays come and go: placed again every few seconds while shown
        self.placement.start(slint::TimerMode::Repeated, Duration::from_secs(3), move || {
            if let Some(w) = weak.as_ref().and_then(|w| w.upgrade()) {
                gui::keep_on_top(w.window());
            }
            crate::app::with_app(|app| app.overlay_place());
        });
    }

    /// Every 100 ms while a timed overlay is shown; true when it just went away.
    pub fn tick(&mut self) -> bool {
        self.session.expire(Instant::now());
        if !self.session.is_visible() && self.window.as_ref().is_some_and(|w| w.window().is_visible()) {
            self.hide();
            return true;
        }
        false
    }

    /// New readings or settings: the contents change, the visibility clock doesn't.
    pub fn refresh(&mut self, rows: &[DeviceRow], s: &AppSettings, obstruction: Option<Rect>) {
        if self.window.is_none() || !self.session.is_visible() {
            return;
        }
        let entries: Vec<OverlayEntry> = select(rows, s)
            .into_iter()
            .map(|(row, show_name)| {
                let c = row.current.as_ref().expect("shown rows are on the air");
                let power = c.charging || c.full_on_charger;
                let (r, g, b) = level_color(c.level, power, true, &row.id, s);
                OverlayEntry {
                    glyph: crate::main_view::glyph(row.kind()).into(),
                    name: row.name.clone().into(),
                    show_name,
                    level: match c.level {
                        Some(l) => format!("{}{l}%", if c.estimate { "≈" } else { "" }).into(),
                        None if power => "".into(),
                        None => "—".into(),
                    },
                    color: slint::Color::from_rgb_u8(r, g, b),
                    power,
                }
            })
            .collect();
        let limit = self.row_limit();
        if self.shown != (entries.clone(), limit) {
            self.rows.set_vec(into_rows(entries.clone(), limit));
            self.shown = (entries, limit);
        }
        self.place(obstruction);
    }

    /// How wide a row may get on the monitor the panel is on, or is about to open on.
    fn row_limit(&self) -> f32 {
        let shown = self.window.as_ref().filter(|w| w.window().is_visible()).and_then(|w| gui::window_rect(w.window()));
        let (area, scale) = match shown {
            Some((r, scale)) => (monitor_area_at(r.left, r.top), scale),
            None => gui::active_monitor(),
        };
        row_limit((area.right - area.left) as f32 / scale.max(0.1))
    }

    /// The language changed: the panel's own text ("no devices") and its font follow.
    pub fn set_language(&self) {
        if let Some(w) = &self.window {
            w.global::<gui::T>().set_lang(crate::loc::current().into());
        }
    }

    /// Puts the panel in its corner of the work area of the monitor it is on, off the reminder: by the window's real
    /// size, as Windows has it.
    pub fn place(&self, obstruction: Option<Rect>) {
        let Some(w) = &self.window else { return };
        if !w.window().is_visible() {
            return;
        }
        place_window(w.window(), self.corner.get(), obstruction);
    }

    pub fn hide(&mut self) {
        self.session.hide();
        self.expiry.stop();
        self.placement.stop();
        if let Some(w) = &self.window {
            let _ = w.hide();
        }
    }
}

/// The widest a row of entries gets on a work area `work_width` wide: half of it, as the C# panel (never under 240,
/// never past the screen), less the panel's padding. All in the panel's own pixels.
pub fn row_limit(work_width: f32) -> f32 {
    (work_width - 24.0).min((work_width * 0.5).max(240.0)) - 12.0
}

/// About how wide the panel draws an entry (overlay.slint): good enough to decide where the next row starts, and a
/// little on the wide side, so a row comes out under the limit rather than over it.
fn entry_width(e: &OverlayEntry) -> f32 {
    let name: f32 = e.name.chars().map(|c| if (c as u32) < 0x2E80 { 6.8 } else { 13.5 }).sum();
    let name = if e.show_name { 7.0 + name.min(140.0) } else { 0.0 };
    let level = e.level.chars().count() as f32 * 11.0 + if e.level.is_empty() { 0.0 } else { 4.0 };
    let power = if e.power { 21.0 } else { 0.0 };
    16.0 + 19.0 + name + 7.0 + level + power
}

/// How many entries go in each row: as many as fit in `limit`, at least one.
fn wrap(widths: &[f32], limit: f32) -> Vec<usize> {
    let mut rows = Vec::new();
    let (mut count, mut width) = (0usize, 0f32);
    for w in widths {
        if count > 0 && width + w > limit {
            rows.push(count);
            (count, width) = (0, 0.0);
        }
        count += 1;
        width += w;
    }
    if count > 0 {
        rows.push(count);
    }
    rows
}

/// The entries in the rows the panel shows them in.
pub fn into_rows(entries: Vec<OverlayEntry>, limit: f32) -> Vec<OverlayRow> {
    let widths: Vec<f32> = entries.iter().map(entry_width).collect();
    let mut entries = entries.into_iter();
    wrap(&widths, limit)
        .into_iter()
        .map(|n| {
            let row: Vec<OverlayEntry> = entries.by_ref().take(n).collect();
            OverlayRow { entries: ModelRc::from(Rc::new(VecModel::from(row))) }
        })
        .collect()
}

fn place_window(window: &slint::Window, corner: i32, obstruction: Option<Rect>) {
    let Some((r, scale)) = gui::window_rect(window) else { return };
    // Stays on the monitor it was opened on
    let area = monitor_area_at(r.left, r.top);
    let (x, y) = place(
        Rect {
            x: area.left as f64,
            y: area.top as f64,
            w: (area.right - area.left) as f64,
            h: (area.bottom - area.top) as f64,
        },
        ((r.right - r.left) as f64, (r.bottom - r.top) as f64),
        corner,
        12.0 * scale as f64,
        obstruction,
    );
    let (x, y) = (x.round() as i32, y.round() as i32);
    if (r.left, r.top) != (x, y) {
        gui::move_window(window, x, y);
    }
}

fn monitor_area_at(x: i32, y: i32) -> RECT {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: x + 1, y: y + 1 }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            gui::primary_work_area()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::DeviceReading;

    #[test]
    fn a_long_list_goes_on_in_the_next_row() {
        assert_eq!(wrap(&[100.0, 100.0, 100.0], 250.0), [2, 1]);
        assert_eq!(wrap(&[100.0, 100.0], 250.0), [2]);
        assert_eq!(wrap(&[300.0, 100.0, 100.0, 100.0], 250.0), [1, 2, 1], "one too wide still gets a row");
        assert!(wrap(&[], 250.0).is_empty());
        // Half the work area, as the C# panel: 1920 wide gives 960 less the padding; a narrow one never under 240
        assert_eq!((row_limit(1920.0), row_limit(400.0), row_limit(200.0)), (948.0, 228.0, 164.0));
    }

    #[test]
    fn timed_press_restarts_the_five_second_deadline() {
        let start = Instant::now();
        let mut s = Session::default();
        assert!(!s.is_visible());
        s.press(0, start);
        assert!(s.is_timed());
        s.expire(start + Duration::from_secs(4));
        assert!(s.is_visible());
        let again = start + Duration::from_secs(4);
        s.press(0, again);
        s.expire(again + Duration::from_millis(4900));
        assert!(s.is_visible());
        s.expire(again + Duration::from_secs(5));
        assert!(!s.is_visible());
    }

    #[test]
    fn toggle_stays_visible_until_pressed_again() {
        let start = Instant::now();
        let mut s = Session::default();
        s.press(1, start);
        assert!(!s.is_timed());
        s.expire(start + Duration::from_secs(3600));
        assert!(s.is_visible());
        s.press(1, start);
        assert!(!s.is_visible());
    }

    #[test]
    fn preview_times_out_even_after_a_toggle_session_and_hide_cancels_it() {
        let start = Instant::now();
        let mut s = Session::default();
        s.press(1, start);
        s.preview(start);
        s.expire(start + Duration::from_secs(5));
        assert!(!s.is_visible());
        s.preview(start);
        s.hide();
        s.expire(start + Duration::from_secs(1));
        assert!(!s.is_visible());
    }

    fn row(id: &str, kind: DeviceKind, online: bool) -> DeviceRow {
        let r = DeviceReading { online, level: Some(50), ..DeviceReading::new(id, id, kind, "test") };
        DeviceRow { id: id.into(), current: Some(r.clone()), last: Some(r), name: id.into() }
    }

    #[test]
    fn filters_offline_and_hidden_devices_without_using_remembered_levels() {
        let mut settings = AppSettings::default();
        settings.device_mut("hidden").hidden = true;
        let offline = row("offline", DeviceKind::Mouse, false);
        let remembered = DeviceRow { id: "remembered".into(), current: None, ..offline.clone() };
        let rows =
            [row("shown", DeviceKind::Keyboard, true), offline, remembered, row("hidden", DeviceKind::Mouse, true)];
        let result = select(&rows, &settings);
        assert_eq!(result.iter().map(|r| r.0.id.as_str()).collect::<Vec<_>>(), ["shown"]);
    }

    #[test]
    fn names_are_shown_only_for_duplicate_kinds_unless_enabled_for_all() {
        let rows = [
            row("m1", DeviceKind::Mouse, true),
            row("m2", DeviceKind::Mouse, true),
            row("k", DeviceKind::Keyboard, true),
        ];
        let mut settings = AppSettings::default();
        for (r, show) in select(&rows, &settings) {
            assert_eq!(show, r.kind() == DeviceKind::Mouse);
        }
        settings.overlay_show_names = true;
        assert!(select(&rows, &settings).iter().all(|r| r.1));
    }

    #[test]
    fn order_does_not_follow_battery_levels() {
        let rows: Vec<DeviceRow> = (0..20).rev().map(|i| row(&format!("{i:02}"), DeviceKind::Headset, true)).collect();
        let settings = AppSettings::default();
        let before: Vec<String> = select(&rows, &settings).iter().map(|r| r.0.id.clone()).collect();
        let changed: Vec<DeviceRow> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut r = r.clone();
                r.current.as_mut().unwrap().level = Some(i as i32);
                r
            })
            .collect();
        let after: Vec<String> = select(&changed, &settings).iter().map(|r| r.0.id.clone()).collect();
        assert_eq!(before, after);
        assert_eq!(before.len(), 20);
    }

    #[test]
    fn the_order_is_the_lists() {
        let rows = [
            row("mouse", DeviceKind::Mouse, true),
            row("off", DeviceKind::Headset, false),
            row("headset", DeviceKind::Headset, true),
            row("keys", DeviceKind::Keyboard, true),
            row("pad", DeviceKind::Gamepad, true),
        ];
        let shown = |settings: &AppSettings| -> Vec<String> {
            select(&rows, settings).iter().map(|r| r.0.id.clone()).collect()
        };
        let mut settings = AppSettings::default();
        assert_eq!(shown(&settings), ["headset", "mouse", "keys", "pad"], "by kind and name until the user orders it");
        // The pinned one first, then the others as the user put them; what is off or hidden is left out as before
        settings.device_order = ["pad", "off", "keys", "headset", "mouse"].map(String::from).to_vec();
        settings.device_mut("mouse").pinned = true;
        assert_eq!(shown(&settings), ["mouse", "pad", "keys", "headset"]);
        settings.device_mut("keys").hidden = true;
        assert_eq!(shown(&settings), ["mouse", "pad", "headset"]);
    }

    #[test]
    fn anchors_all_corners_on_a_negative_coordinate_monitor() {
        let area = Rect { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 };
        for (corner, expected) in
            [(0, (-1908.0, 12.0)), (1, (-212.0, 12.0)), (2, (-1908.0, 1028.0)), (3, (-212.0, 1028.0))]
        {
            assert_eq!(place(area, (200.0, 40.0), corner, 12.0, None), expected);
        }
    }

    #[test]
    fn applies_the_margin_and_avoids_the_reminder() {
        let area = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
        let reminder = Rect { x: 1490.0, y: 760.0, w: 420.0, h: 310.0 };
        let point = place(area, (400.0, 80.0), 3, 18.0, Some(reminder));
        assert_eq!(point, (1502.0, 662.0));
        let at = Rect { x: point.0, y: point.1, w: 400.0, h: 80.0 };
        assert!(!at.intersects(&reminder) && area.contains(&at));
    }

    #[test]
    fn oversized_content_and_tiny_work_areas_never_go_negative() {
        assert_eq!(place(Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, (100.0, 100.0), 3, 12.0, None), (5.0, 5.0));
    }
}
