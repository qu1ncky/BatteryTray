//! The low battery reminder window: in the bottom right corner, on top, never taking focus,
//! until the user snoozes or mutes it, or, when the settings say so, until it closes on its own as if snoozed. Its
//! time runs only while someone is at the computer, so it is still there for a user who was away, and while the
//! pointer is off the window, so it doesn't close under a hand on its way to a button.
//! It opens unseen and becomes visible only once it stands in its corner at its real size. Right after show() the
//! window still has the height Slint guessed before it existed (48 px short at 125%: text is measured exactly only
//! then), and gets its real one a moment later.

use crate::gui::{self, AlertRow, AlertWindow, Kind};
use crate::loc::tf;
use crate::overlay::Rect;
use crate::rows::AlertItem;
use crate::theme;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};

/// How often the line of the time left moves
const TICK: Duration = Duration::from_millis(33);
/// No keyboard or mouse input for this long: nobody is at the computer, unless a full-screen app runs
const AWAY_AFTER: Duration = Duration::from_secs(30);
/// How often Windows is asked whether someone is at the computer
const PRESENCE_EVERY: Duration = Duration::from_secs(1);

/// The time a reminder that closes on its own has left.
#[derive(Default)]
struct Countdown {
    /// None: it waits for an answer (or its time is up)
    total: Cell<Option<Duration>>,
    left: Cell<Duration>,
    /// When the time last ran
    since: Cell<Option<Instant>>,
    /// Its time ran out: the snooze that follows came from it, not from the user
    expired: Cell<bool>,
    /// Whether someone is at the computer, and when Windows was last asked
    presence: Cell<Option<(Instant, bool)>>,
}

impl Countdown {
    fn someone_there(&self, now: Instant) -> bool {
        match self.presence.get() {
            Some((asked, there)) if now - asked < PRESENCE_EVERY => there,
            _ => {
                let there = someone_there();
                self.presence.set(Some((now, there)));
                there
            }
        }
    }
}

pub struct AlertView {
    pub ui: AlertWindow,
    items: Vec<AlertItem>,
    rows: Rc<VecModel<AlertRow>>,
    /// Shown but not yet seen: waiting for its real size
    revealing: Rc<Cell<bool>>,
    reveal_timer: Rc<slint::Timer>,
    countdown: Rc<Countdown>,
    tick: slint::Timer,
    _keep_on_top: slint::Timer,
}

impl AlertView {
    /// A reminder window; its two answers call `snooze` and `mute`.
    pub fn new(snooze: impl Fn() + 'static, mute: impl Fn() + 'static) -> Result<Self, slint::PlatformError> {
        let ui = gui::create(Kind::Reminder, AlertWindow::new)?;
        gui::setup_texts(&ui.global::<gui::T>());
        gui::set_theme(&ui, theme::current().apps_light);
        let rows = Rc::new(VecModel::default());
        ui.set_items(ModelRc::from(rows.clone()));
        ui.on_snooze(snooze);
        ui.on_mute(mute);
        let revealing = Rc::new(Cell::new(false));
        let reveal_timer = Rc::new(slint::Timer::default());

        // Whenever its size changes (it gets its real one, a longer list, another scale): back into its corner. While
        // it is still unseen, it is shown once its size has stayed the same for a moment.
        let weak = ui.as_weak();
        let (r, timer) = (revealing.clone(), reveal_timer.clone());
        ui.window().on_winit_window_event(move |_, event| {
            use slint::winit_030::winit::event::WindowEvent;
            if matches!(event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
                if let Some(ui) = weak.upgrade() {
                    place(&ui);
                    if r.get() {
                        reveal_after(&timer, &ui, &r, 40);
                    }
                }
            }
            slint::winit_030::EventResult::Propagate
        });
        let keep_on_top = slint::Timer::default();
        let weak = ui.as_weak();
        // Full-screen apps and other topmost windows can push it back; reasserted every few seconds
        keep_on_top.start(slint::TimerMode::Repeated, Duration::from_secs(3), move || {
            if let Some(ui) = weak.upgrade() {
                gui::keep_on_top(ui.window());
                place(&ui);
            }
        });
        Ok(Self {
            ui,
            items: Vec::new(),
            rows,
            revealing,
            reveal_timer,
            countdown: Rc::default(),
            tick: slint::Timer::default(),
            _keep_on_top: keep_on_top,
        })
    }

    pub fn ids(&self) -> Vec<String> {
        self.items.iter().map(|i| i.id.clone()).collect()
    }

    /// True when the window already shows this device at this severity (no new sound needed).
    pub fn shows(&self, item: &AlertItem) -> bool {
        self.items.iter().any(|i| i.id == item.id && i.critical == item.critical)
    }

    /// The devices due and the snooze's minutes; `auto_close`: closes on its own after that long, its time starting
    /// again whenever a device or a severity it didn't show comes up.
    pub fn set_items(&mut self, due: Vec<AlertItem>, repeat_minutes: i32, auto_close: Option<Duration>) {
        if due.iter().any(|d| !self.shows(d)) || auto_close != self.countdown.total.get() {
            self.countdown.total.set(auto_close);
            self.countdown.left.set(auto_close.unwrap_or_default());
            self.countdown.since.set(None);
            self.ui.set_remaining(1.0);
        }
        self.ui.set_auto_close(auto_close.is_some());
        if auto_close.is_none() {
            self.tick.stop();
        } else if !self.tick.running() {
            self.run_countdown();
        }
        let critical = due.iter().any(|d| d.critical);
        self.ui.set_critical(critical);
        self.ui.set_snooze_text(tf("alert.snooze", &[&repeat_minutes]).into());
        self.rows.set_vec(
            due.iter()
                .map(|d| AlertRow {
                    glyph: crate::main_view::glyph(d.kind).into(),
                    name: d.name.clone().into(),
                    level: format!("{}%", d.level).into(),
                    percent: d.level,
                    critical: d.critical,
                })
                .collect::<Vec<_>>(),
        );
        self.items = due;
        if self.ui.window().is_visible() {
            self.style(); // a new height is placed by the resize handler
        }
    }

    /// It closed on its own: its time ran out, the user didn't answer.
    pub fn closed_on_its_own(&self) -> bool {
        self.countdown.expired.get()
    }

    /// The time runs while the window is seen, someone is at the computer and the pointer is off the window; at its
    /// end the window snoozes, as by the button (from the event loop: snoozing closes this window, timer and all).
    fn run_countdown(&self) {
        let (weak, countdown, revealing) = (self.ui.as_weak(), self.countdown.clone(), self.revealing.clone());
        self.tick.start(slint::TimerMode::Repeated, TICK, move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(total) = countdown.total.get() else { return };
            let now = Instant::now();
            let last = countdown.since.replace(Some(now));
            if !ui.window().is_visible() || revealing.get() || pointer_over(&ui) || !countdown.someone_there(now) {
                return;
            }
            let Some(last) = last else { return };
            let left = countdown.left.get().saturating_sub(now - last);
            countdown.left.set(left);
            ui.set_remaining(left.as_secs_f32() / total.as_secs_f32());
            if left.is_zero() {
                countdown.total.set(None);
                countdown.expired.set(true);
                let weak = ui.as_weak();
                slint::Timer::single_shot(Duration::ZERO, move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.invoke_snooze();
                    }
                });
            }
        });
    }

    pub fn show(&self) {
        if self.ui.window().is_visible() {
            return;
        }
        // Near its corner already (by the guessed size), unseen until it has its real size there
        let (area, scale) = gui::primary_monitor();
        let (w, h) = ((376.0 * scale).round() as i32, (self.ui.get_content_height() * scale).round() as i32);
        let margin = (16.0 * scale).round() as i32;
        self.ui.window().set_position(slint::PhysicalPosition::new(area.right - w - margin, area.bottom - h - margin));
        self.revealing.set(true);
        let _ = self.ui.show();
        // Seen once its size has settled (each resize starts this again; none may come if it was right already)
        reveal_after(&self.reveal_timer, &self.ui, &self.revealing, 80);
    }

    fn style(&self) {
        gui::set_border(self.ui.window(), Some(border()));
    }

    /// The app mode changed (Windows', or the theme picked in the settings): the window follows it, its border too once
    /// it is seen.
    pub fn follow_theme(&self) {
        gui::fade_theme(&self.ui, theme::current().apps_light);
        if self.ui.window().is_visible() && !self.revealing.get() {
            self.style();
        }
    }

    /// Where it is on the screen, for the overlay to stay clear of it.
    pub fn bounds(&self) -> Option<Rect> {
        if !self.ui.window().is_visible() {
            return None;
        }
        let r = screen_rect(&self.ui)?;
        Some(Rect { x: r.left as f64, y: r.top as f64, w: (r.right - r.left) as f64, h: (r.bottom - r.top) as f64 })
    }

    pub fn close(&self) {
        self.tick.stop();
        let _ = self.ui.hide();
    }
}

/// The window's border: neutral, the severity is in the levels' color. Light: WinUI's stroke around a surface (#757575
/// at 40%) over the window's colour.
fn border() -> (u8, u8, u8) {
    if theme::current().apps_light {
        (0xC5, 0xC5, 0xC5)
    } else {
        (0x3F, 0x3F, 0x3F)
    }
}

fn screen_rect(ui: &AlertWindow) -> Option<RECT> {
    let hwnd = gui::hwnd(ui.window())?;
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r).ok()? };
    Some(r)
}

/// The pointer is over the window (it never gets the focus, so its hover is asked of Windows).
fn pointer_over(ui: &AlertWindow) -> bool {
    let Some(r) = screen_rect(ui) else { return false };
    let mut p = POINT::default();
    let found = unsafe { GetCursorPos(&mut p).is_ok() };
    found && (r.left..r.right).contains(&p.x) && (r.top..r.bottom).contains(&p.y)
}

/// Someone is at the computer: not while it is locked, a screen saver runs or another user has the screen; while a
/// full-screen app runs (a game played with a controller, whose input Windows doesn't count as the user's, or a
/// film); otherwise when the keyboard or the mouse was used in the last half minute. A reminder that comes up while
/// the user is away waits for them.
fn someone_there() -> bool {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) if state == QUNS_NOT_PRESENT => return false,
        Ok(state) if [QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE].contains(&state) => return true,
        _ => {}
    }
    let mut input = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut input) }.as_bool() {
        return true; // unknown: as before, the time runs
    }
    // Both in ms since the system started, as a u32 that wraps after 49.7 days
    let idle = unsafe { GetTickCount() }.wrapping_sub(input.dwTime);
    Duration::from_millis(idle as u64) < AWAY_AFTER
}

#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    fn GetTickCount() -> u32;
}

/// In ms, unless its size changes again first: into its corner, and seen. Windows creates the window only once the
/// event loop gets to it after show(); until then this waits for it.
fn reveal_after(timer: &Rc<slint::Timer>, ui: &AlertWindow, revealing: &Rc<Cell<bool>>, ms: u64) {
    let weak = ui.as_weak();
    let (again, revealing) = (timer.clone(), revealing.clone());
    timer.start(slint::TimerMode::SingleShot, Duration::from_millis(ms), move || {
        let Some(ui) = weak.upgrade() else { return };
        if gui::hwnd(ui.window()).is_none() {
            reveal_after(&again, &ui, &revealing, 20);
            return;
        }
        place(&ui);
        if revealing.replace(false) {
            gui::set_border(ui.window(), Some(border()));
            gui::set_alpha(ui.window(), 255);
        }
    });
}

/// The bottom right corner of the primary screen's work area, 16 px in from its edges: by the window's real size.
fn place(ui: &AlertWindow) {
    let Some((r, scale)) = gui::window_rect(ui.window()) else { return };
    let area = gui::primary_work_area();
    let margin = (16.0 * scale).round() as i32;
    let (x, y) = (area.right - (r.right - r.left) - margin, area.bottom - (r.bottom - r.top) - margin);
    if (r.left, r.top) != (x, y) {
        gui::move_window(ui.window(), x, y);
    }
}
