//! The taskbar strip (Settings > General > Taskbar): the first devices of the list as their glyphs and levels, on the
//! taskbar to the left of the notification area, in sight without a click.
//!
//! Windows gives an app no place of its own on the taskbar but its notification icon, and Windows 11 keeps a new icon
//! under the arrow until the user drags it out. So the strip is a window of the app's own, made a child of the
//! taskbar's window and drawn to look like a part of it: glyphs a little larger than the tray's, levels as high as
//! the taskbar's own text, a hairline between two devices, the taskbar's text color, one hover target for the whole
//! of it. The window is layered with an alpha of its own for every pixel (the taskbar shows through), never takes the
//! focus, and is off unless the user turns it on.
//!
//! A child of the taskbar, not a window kept on top of it, because only a child is wherever the taskbar is: under a
//! full-screen app (a game, a video: Windows drops the taskbar below it, however the app got that big), out of sight
//! with a taskbar that hides by itself, and in front while Start or the search is open. For those Explorer lifts the
//! taskbar into a band of windows no app's window can enter, and a window merely kept on top is left under it; and
//! guessing from outside which app is full-screen put the strip over a video or a game whenever the guess was wrong.
//!
//! What the strip still does by itself: it follows the notification area (an icon came or went) and the taskbar's
//! scale, moves to the new taskbar when Explorer starts again (the old one takes the strip's window with it), and
//! gives way when the apps' buttons reach it: first down to the one device that needs charging most, then away.
//!
//! A child window ties its thread's input to its parent's: while one of the two threads is busy, the other gets no
//! mouse. So the strip lives on a thread of its own that does nothing else, and neither the app's windows nor the
//! taskbar ever wait for the other. Where the apps' buttons end only the accessibility tree says (Windows 11 draws
//! them in XAML), and that is asked on one more thread, so a busy Explorer holds up nothing here either.

use crate::view::Tone;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush, GetSysColor,
    GetTextExtentPoint32W, MapWindowPoints, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, COLOR_HIGHLIGHT,
    COLOR_HIGHLIGHTTEXT, COLOR_WINDOWTEXT, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_LEFT, DT_NOCLIP, DT_NOPREFIX,
    DT_SINGLELINE, DT_TOP, FF_SWISS, FW_NORMAL, HBITMAP, HDC, HFONT, HGDIOBJ, OUT_TT_PRECIS, SYS_COLOR_INDEX,
    TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, SetWinEventHook, TreeScope_Descendants, UIA_BoundingRectanglePropertyId,
    UIA_ButtonControlTypeId, UIA_ControlTypePropertyId, UIA_MenuItemControlTypeId, UIA_SplitButtonControlTypeId,
    UnhookWinEvent, HCF_HIGHCONTRASTON, HIGHCONTRASTW, HWINEVENTHOOK,
};
use windows::Win32::UI::Controls::{
    SetWindowTheme, TOOLTIPS_CLASSW, TTF_CENTERTIP, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH,
    TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyWindow, DispatchMessageW, FindWindowExW,
    FindWindowW, GetMessageW, GetTopWindow, GetWindowRect, GetWindowThreadProcessId, IsWindow, KillTimer, LoadCursorW,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterShellHookWindow, RegisterWindowMessageW, SendMessageW,
    SetTimer, SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, UpdateLayeredWindow, CW_USEDEFAULT,
    EVENT_OBJECT_LOCATIONCHANGE, HWND_TOP, IDC_ARROW, MA_NOACTIVATE, MSG, SPI_GETHIGHCONTRAST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA,
    WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCDESTROY, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_THEMECHANGED,
    WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOPARENTNOTIFY, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// Sizes in Windows' own pixels at 100%. A taskbar item's: on a 48 px taskbar its hover target is 40 px high with
/// corners of 4. The strip's own: a glyph of 18 px, a little more than a tray icon's 16, and levels of 13 px, whose
/// digits GDI draws about as high as the taskbar draws its own text (the clock's). With a 16 px glyph and 12 px
/// levels the strip looked small beside the tray.
const HEIGHT: f32 = 40.0;
const INSET: f32 = 4.0;
const LOWEST: f32 = 24.0;
const RADIUS: f32 = 4.0;
const PAD: f32 = 9.0;
const GLYPH: f32 = 18.0;
const TEXT: f32 = 13.0;
/// Between a device's glyph and its level, and between the level and the lightning
const GLYPH_GAP: f32 = 5.0;
const BOLT_GAP: f32 = 3.0;
/// The lightning of a device that charges, how wide and how high. After a level it is a mark beside the digits. Alone
/// (a device that says nothing but that it charges) it stands where the level would, and is larger and as far from
/// the glyph as a level: at the mark's size it looked lost beside the glyph.
const BOLT: (f32, f32) = (7.0, 12.0);
const BOLT_ALONE: (f32, f32) = (8.0, 14.0);
/// The hairline between two devices: how wide and how high, and what is left clear on each side of it
const RULE: (f32, f32) = (1.0, 16.0);
const RULE_GAP: f32 = 9.0;
/// Left clear between the strip and the arrow of the notification area, and between the last app button and the strip
const TRAY_GAP: f32 = 4.0;
const BUTTONS_GAP: f32 = 8.0;

/// The battery shown while no device is on the air (Segoe Fluent Icons and Segoe MDL2 Assets both have it)
const NO_DEVICES: &str = "\u{E850}";

const TICK: usize = 1;
const ROOM_SOON: usize = 2;
/// The taskbar changed: look again
const WM_STRIP_CHECK: u32 = WM_APP + 0x51;
/// The room thread's answer: wparam 1 and the edge in lparam, or wparam 0 when it couldn't tell
const WM_STRIP_ROOM: u32 = WM_APP + 0x52;
/// From the app: something new to show is waiting, and the strip is no longer wanted
const WM_STRIP_SET: u32 = WM_APP + 0x53;
const WM_STRIP_END: u32 = WM_APP + 0x54;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// The strip on the taskbar, and the window of its thread that is never seen: the timers', the hooks', the system's
/// and the app's messages come to that one, whatever becomes of the taskbar
const CLASS: PCWSTR = w!("BatteryTray.Strip");
const WATCHER: PCWSTR = w!("BatteryTray.StripWatcher");

type Rgb = (u8, u8, u8);
type Rgba = (u8, u8, u8, u8);

/// One device on the strip: its kind's glyph, then its level without "%" ("37", "≈15", empty for a device that only
/// says it is charging), then a lightning while it charges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub glyph: &'static str,
    pub level: String,
    pub tone: Tone,
    pub power: bool,
}

/// Everything the strip shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Content {
    /// The devices, as many as the settings say at most
    pub slots: Vec<Slot>,
    /// The device that needs charging most: the only one left when the apps' buttons leave room for one
    pub urgent: Option<Slot>,
    /// Every device on the air, a line each: the tooltip, and what a screen reader says
    pub tooltip: String,
    /// The taskbar's mode
    pub light: bool,
    /// The app's window is open
    pub active: bool,
}

/// What the user did on the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// A click: the app's window
    Open,
    /// A right click: the tray icon's menu
    Menu,
}

/// The taskbar as Windows has it, in physical pixels of the screen: its window and the notification area in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub bar: RECT,
    pub tray: RECT,
    /// 1.25 at 125%
    pub scale: f32,
}

/// How much of the strip the taskbar has room for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Full,
    /// Only the device that needs charging most
    One,
    None,
}

impl Bar {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    /// Along the bottom or the top of the screen. On Windows 10 it can stand at a side, where there is no row to add to.
    pub fn horizontal(&self) -> bool {
        self.bar.right - self.bar.left > self.bar.bottom - self.bar.top
    }

    /// The notification area is at the taskbar's right end; at its left where Windows runs right to left.
    pub fn tray_on_right(&self) -> bool {
        self.tray.left + self.tray.right >= self.bar.left + self.bar.right
    }

    /// The strip's height: a taskbar item's, 0 on a taskbar too low for one (the small one of a tablet).
    pub fn height(&self) -> i32 {
        let free = self.bar.bottom - self.bar.top - 2 * self.px(INSET);
        if free < self.px(LOWEST) {
            0
        } else {
            free.min(self.px(HEIGHT))
        }
    }

    /// How wide the strip may be: from the last app button (`taken` is its edge on the tray's side; unknown, the whole
    /// taskbar counts as free) to the notification area, less the gaps kept on both sides.
    pub fn room(&self, taken: Option<i32>) -> i32 {
        let gaps = self.px(TRAY_GAP) + taken.map_or(0, |_| self.px(BUTTONS_GAP));
        if self.tray_on_right() {
            self.tray.left - taken.unwrap_or(self.bar.left) - gaps
        } else {
            taken.unwrap_or(self.bar.right) - self.tray.right - gaps
        }
    }

    /// Where a strip `width` wide and `height` high goes: against the notification area, in the middle of the
    /// taskbar's height.
    pub fn origin(&self, width: i32, height: i32) -> (i32, i32) {
        let x = if self.tray_on_right() {
            self.tray.left - self.px(TRAY_GAP) - width
        } else {
            self.tray.right + self.px(TRAY_GAP)
        };
        (x, self.bar.top + (self.bar.bottom - self.bar.top - height) / 2)
    }
}

/// The whole strip while it fits, its one device while that fits, nothing after that.
pub fn fit(room: i32, full: i32, one: i32) -> Fit {
    if full <= room {
        Fit::Full
    } else if one <= room {
        Fit::One
    } else {
        Fit::None
    }
}

/// The edge, on the notification area's side, of the taskbar button that comes closest to it: where the room for the
/// strip starts. `buttons` are all the buttons of the taskbar; the notification area's own are left out here.
pub fn nearest(buttons: &[RECT], bar: &Bar) -> Option<i32> {
    let real = buttons.iter().filter(|b| b.right > b.left && b.bottom > b.top);
    if bar.tray_on_right() {
        real.filter(|b| b.right <= bar.tray.left + 1).map(|b| b.right).max()
    } else {
        real.filter(|b| b.left >= bar.tray.right - 1).map(|b| b.left).min()
    }
}

/// The fonts the strip's text is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Glyph,
    Text,
}

/// What a piece of the strip is: text in one of the fonts, or a shape the strip draws itself.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Mark {
    Text(Face, String),
    /// The lightning of a device that charges, `alone` when no level stands before it. The icon fonts have it only as
    /// an outline, which is a blot at this size
    Bolt {
        alone: bool,
    },
    /// The hairline between two devices
    Rule,
}

/// A text as its font draws it: how wide and how tall what is seen of it is, and how far into its line's box that
/// starts, from the left and from the top. A level is as wide as its line, so that its digits keep their columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    w: i32,
    tall: i32,
    left: i32,
    above: i32,
}

/// One piece at its place in the strip: `x` and `w` are its ink's (a text's line starts `left` before and `above`
/// it). The strip's middle goes through the middle of the ink, not of the font's line: digits leave the lower part of
/// theirs empty, and every glyph of the icon font fills its square in its own way - the keyboard, flat, sits above the
/// middle of it, the mouse leaves a part of it empty at each side. Centered by their lines, a level stood a pixel
/// below its glyph and the keyboard's stuck out above its level; spaced by their squares, a mouse's level stood
/// farther from it than a keyboard's.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Piece {
    x: i32,
    w: i32,
    tall: i32,
    left: i32,
    above: i32,
    mark: Mark,
    color: Rgba,
}

impl Piece {
    /// The row its ink starts at in a strip `h` high.
    fn top(&self, h: i32) -> i32 {
        (h - self.tall).div_euclid(2)
    }
}

/// The taskbar's colors: its text (white on a dark taskbar), the plate under the pointer, and the app's own orange
/// and red for a device below its levels. In a contrast theme everything is the theme's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Paint {
    light: bool,
    text: Rgb,
    /// Set in a contrast theme: one color for every level, and this one on the plate under the pointer
    contrast: Option<Rgb>,
    hover: (Rgba, Rgba),
    pressed: (Rgba, Rgba),
}

impl Paint {
    fn of(light: bool, contrast: Option<(Rgb, Rgb, Rgb)>) -> Self {
        if let Some((text, mark, on_mark)) = contrast {
            let plate = (mark.0, mark.1, mark.2, 255);
            return Paint { light, text, contrast: Some(on_mark), hover: (plate, plate), pressed: (plate, plate) };
        }
        if light {
            Paint {
                light,
                text: (0x1A, 0x1A, 0x1A),
                contrast: None,
                hover: ((255, 255, 255, 179), (0, 0, 0, 15)),
                pressed: ((255, 255, 255, 115), (0, 0, 0, 10)),
            }
        } else {
            Paint {
                light,
                text: (255, 255, 255),
                contrast: None,
                hover: ((255, 255, 255, 20), (255, 255, 255, 15)),
                pressed: ((255, 255, 255, 13), (255, 255, 255, 10)),
            }
        }
    }

    /// A level's color: the taskbar's text, orange or red only below the device's reminder or critical level.
    fn level(&self, tone: Tone) -> Rgb {
        match tone {
            Tone::Warn | Tone::Bad if self.contrast.is_none() => tone.rgb(self.light),
            _ => self.text,
        }
    }

    /// The hairline between two devices: the taskbar's text, faint; a contrast theme's text as it is.
    fn rule(&self) -> Rgba {
        let (r, g, b) = self.text;
        (r, g, b, if self.contrast.is_some() { 255 } else { 56 })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Rest,
    Hover,
    Pressed,
}

/// The pieces of a row of devices, left to right, and the row's width with its padding. `measure` says how a text
/// comes out in a face.
fn layout(slots: &[Slot], scale: f32, paint: &Paint, measure: &mut dyn FnMut(Face, &str) -> Span) -> (Vec<Piece>, i32) {
    let px = |v: f32| (v * scale).round() as i32;
    let opaque = |c: Rgb| (c.0, c.1, c.2, 255);
    let glyph = |text: &str| Mark::Text(Face::Glyph, text.to_string());
    let level = |text: &str| Mark::Text(Face::Text, text.to_string());
    let shape = |(w, tall): (f32, f32)| Span { w: px(w).max(1), tall: px(tall), left: 0, above: 0 };
    let mut pieces = Vec::new();
    let mut x = px(PAD);
    let mut add = |x: &mut i32, mark: Mark, color: Rgba| {
        let span = match &mark {
            Mark::Text(face, text) => measure(*face, text),
            Mark::Bolt { alone } => shape(if *alone { BOLT_ALONE } else { BOLT }),
            Mark::Rule => shape(RULE),
        };
        let Span { w, tall, left, above } = span;
        pieces.push(Piece { x: *x, w, tall, left, above, mark, color });
        *x += w;
    };
    if slots.is_empty() {
        // Nothing on the air: a battery and two dashes, dimmed, as the tray icon's "--"
        let dim = (paint.text.0, paint.text.1, paint.text.2, 153);
        add(&mut x, glyph(NO_DEVICES), dim);
        x += px(GLYPH_GAP);
        add(&mut x, level("--"), dim);
    }
    for (i, slot) in slots.iter().enumerate() {
        if i > 0 {
            x += px(RULE_GAP);
            add(&mut x, Mark::Rule, paint.rule());
            x += px(RULE_GAP);
        }
        add(&mut x, glyph(slot.glyph), opaque(paint.text));
        if !slot.level.is_empty() {
            x += px(GLYPH_GAP);
            add(&mut x, level(&slot.level), opaque(paint.level(slot.tone)));
        }
        if slot.power {
            let alone = slot.level.is_empty();
            x += px(if alone { GLYPH_GAP } else { BOLT_GAP });
            add(&mut x, Mark::Bolt { alone }, opaque(paint.text));
        }
    }
    (pieces, x + px(PAD))
}

/// How much of each pixel of a box `w` by `h` the lightning covers, 0 to 255, row after row.
fn bolt(w: i32, h: i32) -> Vec<u8> {
    // Its corners in a box 7 by 12
    const SHAPE: [(f32, f32); 6] = [(4.5, 0.0), (0.0, 7.0), (3.0, 7.0), (2.0, 12.0), (7.0, 4.5), (4.0, 4.5)];
    // Points looked at along each side of a pixel
    const FINE: i32 = 4;
    let (sx, sy) = (w as f32 / 7.0, h as f32 / 12.0);
    let inside = |x: f32, y: f32| {
        let mut odd = false;
        for (i, a) in SHAPE.iter().enumerate() {
            let b = SHAPE[(i + 1) % SHAPE.len()];
            let (ax, ay, bx, by) = (a.0 * sx, a.1 * sy, b.0 * sx, b.1 * sy);
            if (ay > y) != (by > y) && x < ax + (y - ay) / (by - ay) * (bx - ax) {
                odd = !odd;
            }
        }
        odd
    };
    let mut cover = Vec::with_capacity((w * h).max(0) as usize);
    for row in 0..h {
        for col in 0..w {
            let at = |cell: i32, part: i32| cell as f32 + (part as f32 + 0.5) / FINE as f32;
            let hits = (0..FINE * FINE).filter(|n| inside(at(col, n % FINE), at(row, n / FINE))).count() as i32;
            cover.push((hits * 255 / (FINE * FINE)) as u8);
        }
    }
    cover
}

/// Draws the strip's own shapes into the picture the way GDI draws its text there: white where covered.
fn shapes(bits: &mut [u8], w: i32, h: i32, pieces: &[Piece]) {
    for p in pieces {
        let cover = match p.mark {
            Mark::Text(..) => continue,
            Mark::Bolt { .. } => bolt(p.w, p.tall),
            Mark::Rule => vec![255; (p.w * p.tall).max(0) as usize],
        };
        let top = p.top(h);
        for row in 0..p.tall {
            for col in 0..p.w {
                let (x, y) = (p.x + col, top + row);
                if x >= 0 && y >= 0 && x < w && y < h {
                    let at = ((y * w + x) * 4) as usize;
                    bits[at..at + 3].fill(cover[(row * p.w + col) as usize]);
                }
            }
        }
    }
}

/// The box of the ink in a picture drawn white on black: its first column and row, its last column and row. What is
/// fainter than a third is the smoothing around the ink, not the ink.
fn inked(bits: &[u8], w: i32, h: i32) -> Option<[i32; 4]> {
    const SEEN: u8 = 96;
    let seen = |x: i32, y: i32| bits[((y * w + x) * 4) as usize..][..3].iter().any(|c| *c >= SEEN);
    let column = |x: &i32| (0..h).any(|y| seen(*x, y));
    let row = |y: &i32| (0..w).any(|x| seen(x, *y));
    Some([(0..w).find(column)?, (0..h).find(row)?, (0..w).rev().find(column)?, (0..h).rev().find(row)?])
}

/// How much of the pixel at (x, y) a rounded rectangle w by h covers, 0 to 1.
fn cover(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let r = r.max(0.0).min(w / 2.0).min(h / 2.0);
    let (cx, cy) = (x.clamp(r, w - r), y.clamp(r, h - r));
    let outside = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r;
    (0.5 - outside).clamp(0.0, 1.0)
}

/// Turns what GDI drew (white text on black, its gray the text's coverage) into the strip's pixels, BGRA with the
/// color multiplied by the alpha: the pieces in their colors over the plate. At rest the plate is all but
/// transparent, not quite: a layered window takes the mouse only where its alpha isn't 0, and the whole strip is one
/// target.
fn compose(
    bits: &mut [u8],
    w: i32,
    h: i32,
    pieces: &[Piece],
    plate: Option<(Rgba, Rgba)>,
    on_plate: Option<Rgb>,
    scale: f32,
) {
    // Each column's color: a piece's own columns, and a pixel or two beside them where antialiasing spills over
    let mut columns: Vec<Option<Rgba>> = vec![None; w.max(0) as usize];
    for p in pieces {
        let color = on_plate.map_or(p.color, |c| (c.0, c.1, c.2, p.color.3));
        for x in (p.x - 2).max(0)..(p.x + p.w + 2).min(w) {
            let own = x >= p.x && x < p.x + p.w;
            if own || columns[x as usize].is_none() {
                columns[x as usize] = Some(color);
            }
        }
    }
    let (fill, edge) = plate.unwrap_or(((0, 0, 0, 1), (0, 0, 0, 1)));
    let (radius, line) = ((RADIUS * scale).round(), scale.round().max(1.0));
    let (wf, hf) = (w as f32, h as f32);
    for y in 0..h {
        for x in 0..w {
            let at = ((y * w + x) * 4) as usize;
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let outer = cover(fx, fy, wf, hf, radius);
            let inner = cover(fx - line, fy - line, wf - 2.0 * line, hf - 2.0 * line, radius - line);
            let rim = (outer - inner).max(0.0);
            // The plate, multiplied by its alpha: the fill inside the line, the line around it
            let part = |c: Rgba, share: f32| {
                let a = c.3 as f32 / 255.0 * share;
                [c.2 as f32 * a, c.1 as f32 * a, c.0 as f32 * a, 255.0 * a]
            };
            let (f, e) = (part(fill, inner), part(edge, rim));
            let mut under = [f[0] + e[0], f[1] + e[1], f[2] + e[2], f[3] + e[3]];
            if outer > 0.0 {
                under[3] = under[3].max(1.0);
            }
            let gray = bits[at].max(bits[at + 1]).max(bits[at + 2]) as f32 / 255.0;
            let (r, g, b, a) = columns[x as usize].unwrap_or((0, 0, 0, 0));
            let over = gray * a as f32 / 255.0;
            let mix =
                |text: u8, under: f32| (text as f32 * over + under * (1.0 - over)).round().clamp(0.0, 255.0) as u8;
            bits[at] = mix(b, under[0]);
            bits[at + 1] = mix(g, under[1]);
            bits[at + 2] = mix(r, under[2]);
            bits[at + 3] = (255.0 * over + under[3] * (1.0 - over)).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// The strip's fonts at a scale: the icon font for glyphs, the taskbar's text font for levels.
struct Fonts {
    dpi: u32,
    glyph: HFONT,
    text: HFONT,
    /// The rows digits take in the text font's line. Every level stands at these, whatever is in it
    digits: Span,
}

impl Fonts {
    unsafe fn new(dpi: u32) -> Self {
        let scale = dpi as f32 / 96.0;
        let make = |size: f32, face: &str| {
            let name = crate::win::wide(face);
            CreateFontW(
                -((size * scale).round() as i32),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_TT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                (VARIABLE_PITCH.0 | FF_SWISS.0) as u32,
                PCWSTR(name.as_ptr()),
            )
        };
        // Windows 11's own text font; Windows 10 has Segoe UI
        let letters = if crate::win::windows_build() >= 22000 { "Segoe UI Variable Text" } else { "Segoe UI" };
        let (glyph, text) = (make(GLYPH, crate::gui::icon_font()), make(TEXT, letters));
        Fonts { dpi, glyph, text, digits: ink(text, "0123456789") }
    }

    fn of(&self, face: Face) -> HFONT {
        match face {
            Face::Glyph => self.glyph,
            Face::Text => self.text,
        }
    }

    /// A text of the strip as it comes out in its face: a glyph by its own ink, a level by its line's width and its
    /// digits' rows.
    unsafe fn span(&self, face: Face, text: &str) -> Span {
        match face {
            Face::Glyph => ink(self.glyph, text),
            Face::Text => Span { w: extent(self.text, text).cx, left: 0, ..self.digits },
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for font in [self.glyph, self.text] {
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
        }
    }
}

/// A memory picture GDI can draw text into and Windows can take a layered window's pixels from.
struct Picture {
    dc: HDC,
    bitmap: HBITMAP,
    before: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl Picture {
    unsafe fn new(w: i32, h: i32) -> Option<Self> {
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return None;
        };
        let before = SelectObject(dc, HGDIOBJ(bitmap.0));
        Some(Picture { dc, bitmap, before, bits: bits as *mut u8, w, h })
    }

    fn pixels(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h * 4) as usize) }
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.before);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

/// How wide a text comes out in a font, and how high the font's line is.
unsafe fn extent(font: HFONT, text: &str) -> SIZE {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let dc = CreateCompatibleDC(None);
    let before = SelectObject(dc, HGDIOBJ(font.0));
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
    SelectObject(dc, before);
    let _ = DeleteDC(dc);
    size
}

/// Where the ink of a text is in its line: the text is drawn the way the strip draws it, and looked at. Nothing of it
/// seen (a font without the glyph): its whole line.
unsafe fn ink(font: HFONT, text: &str) -> Span {
    // Room around the line, for what a glyph draws outside it
    const EDGE: i32 = 4;
    let size = extent(font, text);
    let line = Span { w: size.cx, tall: size.cy, left: 0, above: 0 };
    let (w, h) = (size.cx + 2 * EDGE, size.cy + 2 * EDGE);
    let Some(mut picture) = Picture::new(w, h) else { return line };
    SetBkMode(picture.dc, TRANSPARENT);
    SetTextColor(picture.dc, COLORREF(0x00FF_FFFF));
    let own = SelectObject(picture.dc, HGDIOBJ(font.0));
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT { left: EDGE, top: EDGE, right: w - EDGE, bottom: h - EDGE };
    DrawTextW(picture.dc, &mut wide, &mut rect, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX | DT_NOCLIP);
    SelectObject(picture.dc, own);
    let _ = GdiFlush();
    match inked(picture.pixels(), w, h) {
        Some([left, top, right, bottom]) => {
            Span { w: right - left + 1, tall: bottom - top + 1, left: left - EDGE, above: top - EDGE }
        }
        None => line,
    }
}

/// A row of devices laid out with the real fonts.
unsafe fn measured(slots: &[Slot], scale: f32, paint: &Paint, fonts: &Fonts) -> (Vec<Piece>, i32) {
    layout(slots, scale, paint, &mut |face, text| fonts.span(face, text))
}

/// The strip's pixels: the pieces drawn white on black, the text by GDI, then made into their colors over the plate.
unsafe fn draw(frame: &Frame, fonts: &Fonts) -> Option<Picture> {
    let mut picture = Picture::new(frame.w, frame.h)?;
    SetBkMode(picture.dc, TRANSPARENT);
    SetTextColor(picture.dc, COLORREF(0x00FF_FFFF));
    let own = SelectObject(picture.dc, HGDIOBJ(fonts.text.0));
    for p in &frame.pieces {
        let Mark::Text(face, text) = &p.mark else { continue };
        SelectObject(picture.dc, HGDIOBJ(fonts.of(*face).0));
        let mut text: Vec<u16> = text.encode_utf16().collect();
        // The line's corner, so that its ink starts where the piece does
        let (left, top) = (p.x - p.left, p.top(frame.h) - p.above);
        let mut rect = RECT { left, top, right: frame.w, bottom: frame.h };
        DrawTextW(picture.dc, &mut text, &mut rect, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX | DT_NOCLIP);
    }
    SelectObject(picture.dc, own);
    let _ = GdiFlush();
    let plate = match frame.look {
        Look::Rest => None,
        Look::Hover => Some(frame.paint.hover),
        Look::Pressed => Some(frame.paint.pressed),
    };
    let on_plate = plate.and(frame.paint.contrast);
    let (w, h, scale) = (frame.w, frame.h, frame.scale);
    shapes(picture.pixels(), w, h, &frame.pieces);
    compose(picture.pixels(), w, h, &frame.pieces, plate, on_plate, scale);
    Some(picture)
}

/// What is on screen: redrawn only when any of it changes.
#[derive(Clone, Debug, PartialEq)]
struct Frame {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: f32,
    pieces: Vec<Piece>,
    look: Look,
    paint: Paint,
}

/// A row of devices as the strip draws it, on a taskbar's own color: for a screenshot, and to see it without a
/// taskbar. BGRA, top down.
pub fn picture(content: &Content, scale: f32, hover: bool) -> Option<(i32, i32, Vec<u8>)> {
    unsafe {
        let fonts = Fonts::new((96.0 * scale).round() as u32);
        let paint = Paint::of(content.light, None);
        let (pieces, w) = measured(&content.slots, scale, &paint, &fonts);
        let h = (HEIGHT * scale).round() as i32;
        let look = if hover { Look::Hover } else { Look::Rest };
        let mut drawn = draw(&Frame { x: 0, y: 0, w, h, scale, pieces, look, paint }, &fonts)?;
        let taskbar: Rgb = if content.light { (0xE9, 0xEE, 0xF5) } else { (0x1C, 0x1C, 0x1C) };
        let mut pixels = drawn.pixels().to_vec();
        for px in pixels.as_chunks_mut::<4>().0 {
            let rest = 255 - px[3] as u32;
            px[0] = (px[0] as u32 + taskbar.2 as u32 * rest / 255).min(255) as u8;
            px[1] = (px[1] as u32 + taskbar.1 as u32 * rest / 255).min(255) as u8;
            px[2] = (px[2] as u32 + taskbar.0 as u32 * rest / 255).min(255) as u8;
            px[3] = 255;
        }
        Some((w, h, pixels))
    }
}

/// The whole row and the row of the one device that needs charging most, laid out for a content, a scale and a mode.
struct Rows {
    slots: Vec<Slot>,
    urgent: Option<Slot>,
    dpi: u32,
    paint: Paint,
    full: (Vec<Piece>, i32),
    one: Option<(Vec<Piece>, i32)>,
}

struct State {
    /// The thread's own window, never seen
    hwnd: HWND,
    /// The strip: a child of `taskbar`, with its tooltip. Neither is there until a taskbar is found, and the taskbar
    /// takes the strip with it when Explorer ends
    face: HWND,
    tip: HWND,
    on_action: Rc<dyn Fn(Action)>,
    /// What the app wants shown, the newest last
    contents: Receiver<Content>,
    content: Content,
    hover: bool,
    pressed: bool,
    /// The taskbar the strip is on and the hook watches
    taskbar: HWND,
    /// A taskbar that wouldn't take the strip's window: written to the log once
    refused: HWND,
    hooks: Vec<HWINEVENTHOOK>,
    /// Where the apps' buttons end, as the room thread last said
    taken: Option<i32>,
    room: Sender<(isize, Bar)>,
    fonts: Option<Fonts>,
    /// The rows as last laid out: measured again only when what they show changes
    rows: Option<Rows>,
    frame: Option<Frame>,
    /// The text and the mode the tooltip has
    told: Option<(String, bool)>,
    ticks: u32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// The thread's window, and the taskbar and its notification area it watches, for the event hook; and whether a
    /// look is already on its way
    static WATCH: Cell<(isize, isize, isize)> = const { Cell::new((0, 0, 0)) };
    static CHECK_POSTED: Cell<bool> = const { Cell::new(false) };
}

/// The strip, for the app: what to show goes to the strip's thread, which ends when this is dropped.
pub struct Strip {
    /// The window of the strip's thread
    watcher: isize,
    contents: Sender<Content>,
}

impl Strip {
    /// Starts the strip's thread. The strip itself shows up once it has something to show and a taskbar to be on.
    /// `on_action` is called on that thread: what it starts in the app, it hands over to the app's own.
    pub fn new(on_action: impl Fn(Action) + Send + 'static) -> Option<Self> {
        let (contents, queue) = channel();
        let (made, answer) = channel();
        let spawned = std::thread::Builder::new().name("strip".into()).spawn(move || run(on_action, queue, made));
        if let Err(e) = spawned {
            crate::log::write(&format!("taskbar strip: {e}"));
            return None;
        }
        let watcher = answer.recv().ok()??;
        Some(Strip { watcher, contents })
    }

    /// New levels, another mode of the taskbar, the app's window opened or closed.
    pub fn set(&self, content: Content) {
        if self.contents.send(content).is_ok() {
            self.post(WM_STRIP_SET);
        }
    }

    fn post(&self, message: u32) {
        unsafe {
            let _ = PostMessageW(Some(HWND(self.watcher as *mut _)), message, WPARAM(0), LPARAM(0));
        }
    }
}

impl Drop for Strip {
    /// Not waited for: the thread takes its windows off the taskbar and ends by itself.
    fn drop(&mut self) {
        self.post(WM_STRIP_END);
    }
}

/// The strip's thread: its unseen window, then the messages of both windows until the app is done with the strip.
/// `made` gets the unseen window, or nothing when it couldn't be made.
fn run(on_action: impl Fn(Action) + 'static, contents: Receiver<Content>, made: Sender<Option<isize>>) {
    unsafe {
        let hwnd = GetModuleHandleW(None).ok().and_then(|instance| {
            for (name, cursor) in
                [(CLASS, LoadCursorW(None, IDC_ARROW).unwrap_or_default()), (WATCHER, Default::default())]
            {
                let class = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance.into(),
                    hCursor: cursor,
                    lpszClassName: name,
                    ..Default::default()
                };
                // Fails the second time, with the class already there
                RegisterClassExW(&class);
            }
            // A window of the desktop rather than a message-only one: those the system's news (another scale, another
            // theme) doesn't reach
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                WATCHER,
                w!("BatteryTray"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()
        });
        let Some(hwnd) = hwnd else {
            let _ = made.send(None);
            return;
        };
        // Windows opening and closing: the apps' buttons moved
        let _ = RegisterShellHookWindow(hwnd);
        let state = State {
            hwnd,
            face: HWND::default(),
            tip: HWND::default(),
            on_action: Rc::new(on_action),
            contents,
            content: Content::default(),
            hover: false,
            pressed: false,
            taskbar: HWND::default(),
            refused: HWND::default(),
            hooks: Vec::new(),
            taken: None,
            room: room_thread(hwnd.0 as isize),
            fonts: None,
            rows: None,
            frame: None,
            told: None,
            ticks: 0,
        };
        STATE.with(|s| *s.borrow_mut() = Some(state));
        SetTimer(Some(hwnd), TICK, 1000, None);
        let _ = made.send(Some(hwnd.0 as isize));
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            DispatchMessageW(&message);
        }
    }
}

/// The app is done with the strip: its windows, hooks and timers go, and the thread's loop ends.
unsafe fn end() {
    let Some(mut state) = STATE.with(|s| s.borrow_mut().take()) else { return };
    WATCH.with(|w| w.set((0, 0, 0)));
    let _ = KillTimer(Some(state.hwnd), TICK);
    let _ = KillTimer(Some(state.hwnd), ROOM_SOON);
    for hook in &state.hooks {
        let _ = UnhookWinEvent(*hook);
    }
    let _ = DeregisterShellHookWindow(state.hwnd);
    leave(&mut state);
    let _ = DestroyWindow(state.hwnd);
    // The room thread ends with its channel, the fonts with the state
    PostQuitMessage(0);
}

/// Puts the strip's window on a taskbar, as its child and still unseen, with the tooltip. False when the taskbar
/// wouldn't take it.
unsafe fn settle(state: &mut State, taskbar: HWND) -> bool {
    let instance = GetModuleHandleW(None).unwrap_or_default();
    // No word to the parent about the child's making, end and clicks: those would be waited for in Explorer
    let made = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_NOPARENTNOTIFY,
        CLASS,
        w!("BatteryTray"),
        WS_CHILD,
        0,
        0,
        0,
        0,
        Some(taskbar),
        None,
        Some(instance.into()),
        None,
    );
    let face = match made {
        Ok(face) => face,
        Err(e) => {
            if state.refused != taskbar {
                state.refused = taskbar;
                crate::log::write(&format!("taskbar strip: no window on the taskbar: {e}"));
            }
            return false;
        }
    };
    // The tooltip is the thread's own window's, not the taskbar's: it stays the app's to end
    let tip = CreateWindowExW(
        WS_EX_TOPMOST,
        TOOLTIPS_CLASSW,
        PCWSTR::null(),
        WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX),
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        Some(state.hwnd),
        None,
        Some(instance.into()),
        None,
    )
    .unwrap_or_default();
    if !tip.is_invalid() {
        // A width to wrap at makes the tooltip take line breaks: a line per device
        SendMessageW(tip, TTM_SETMAXTIPWIDTH, None, Some(LPARAM(1200)));
        let mut empty = [0u16; 1];
        let tool = tool(face, &mut empty);
        SendMessageW(tip, TTM_ADDTOOLW, None, Some(LPARAM(&tool as *const _ as isize)));
    }
    (state.face, state.tip) = (face, tip);
    true
}

/// Takes the strip's window off its taskbar, or forgets one that went with its taskbar; the tooltip goes with it.
unsafe fn leave(state: &mut State) {
    for window in [state.tip, state.face] {
        if !window.is_invalid() && IsWindow(Some(window)).as_bool() {
            let _ = DestroyWindow(window);
        }
    }
    (state.tip, state.face) = (HWND::default(), HWND::default());
    (state.frame, state.told) = (None, None);
    (state.hover, state.pressed) = (false, false);
}

/// The tooltip's one tool: the whole strip, its text above the strip's middle.
fn tool(hwnd: HWND, text: &mut [u16]) -> TTTOOLINFOW {
    TTTOOLINFOW {
        // Without the last field: the size both versions of the common controls take
        cbSize: (std::mem::size_of::<TTTOOLINFOW>() - std::mem::size_of::<*mut std::ffi::c_void>()) as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS | TTF_CENTERTIP,
        hwnd,
        uId: hwnd.0 as usize,
        lpszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    }
}

/// The strip's state, unless it is in use further up the stack (a message that came while the strip was being drawn).
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|cell| cell.try_borrow_mut().ok().and_then(|mut state| state.as_mut().map(f)))
}

/// Windows' own name of the message a shell hook window gets.
fn shell_hook() -> u32 {
    thread_local!(static MESSAGE: Cell<u32> = const { Cell::new(0) });
    MESSAGE.with(|m| {
        if m.get() == 0 {
            m.set(unsafe { RegisterWindowMessageW(w!("SHELLHOOK")) });
        }
        m.get()
    })
}

fn post_check(hwnd: HWND) {
    if !CHECK_POSTED.with(|c| c.replace(true)) {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_STRIP_CHECK, WPARAM(0), LPARAM(0));
        }
    }
}

/// The taskbar got another size (the screen or its scale changed), or its notification area got wider or narrower
/// (an icon came or went).
unsafe extern "system" fn on_event(_: HWINEVENTHOOK, _: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    let (watcher, taskbar, tray) = WATCH.with(|w| w.get());
    let theirs = hwnd.0 as isize == taskbar || hwnd.0 as isize == tray;
    if watcher == 0 || object != 0 || child != 0 || !theirs {
        return;
    }
    post_check(HWND(watcher as *mut _));
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            with(|state| {
                if !state.hover {
                    state.hover = true;
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                    refresh(state);
                }
            });
        }
        WM_MOUSELEAVE => {
            with(|state| {
                state.hover = false;
                state.pressed = false;
                refresh(state);
            });
        }
        WM_LBUTTONDOWN => {
            with(|state| {
                state.pressed = true;
                refresh(state);
            });
        }
        // On the button's way up, as everything on the taskbar
        WM_LBUTTONUP => {
            let pressed = with(|state| {
                let pressed = std::mem::take(&mut state.pressed);
                refresh(state);
                pressed
            });
            if pressed == Some(true) {
                act(Action::Open);
            }
        }
        WM_RBUTTONUP => act(Action::Menu),
        WM_TIMER if wparam.0 == TICK => {
            with(|state| {
                state.ticks = state.ticks.wrapping_add(1);
                // Now and then also without a hook's word: a pinned app added or taken off moves the buttons too
                if state.ticks % 10 == 0 {
                    ask_room(state);
                }
                refresh(state);
            });
        }
        WM_TIMER if wparam.0 == ROOM_SOON => {
            let _ = KillTimer(Some(hwnd), ROOM_SOON);
            with(ask_room);
        }
        WM_STRIP_CHECK => {
            CHECK_POSTED.with(|c| c.set(false));
            with(refresh);
        }
        WM_STRIP_SET => {
            with(|state| {
                // Only the last of several
                if let Some(content) = state.contents.try_iter().last().filter(|c| *c != state.content) {
                    state.content = content;
                    refresh(state);
                }
            });
        }
        WM_STRIP_END => end(),
        // Explorer ended, and the taskbar took the strip's window with it
        WM_NCDESTROY => {
            with(|state| {
                if state.face == hwnd {
                    state.face = HWND::default();
                    leave(state);
                }
            });
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        WM_STRIP_ROOM => {
            with(|state| {
                let taken = (wparam.0 != 0).then_some(lparam.0 as i32);
                if state.taken != taken {
                    state.taken = taken;
                    refresh(state);
                }
            });
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
            with(|state| {
                state.rows = None;
                state.frame = None;
                refresh(state);
            });
        }
        m if m != 0 && m == shell_hook() => {
            // A window made or closed (1, 2): the buttons move once the taskbar has drawn them
            if matches!(wparam.0 & 0x7FFF, 1 | 2) {
                SetTimer(Some(hwnd), ROOM_SOON, 500, None);
            }
            post_check(hwnd);
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

/// Tells the app what the user did, with the strip's state free.
fn act(action: Action) {
    if let Some(on_action) = with(|state| state.on_action.clone()) {
        on_action(action);
    }
}

/// The taskbar of the main screen, where the notification area is.
unsafe fn find_taskbar() -> Option<(HWND, Bar)> {
    let taskbar = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok()?;
    let tray_window = FindWindowExW(Some(taskbar), None, w!("TrayNotifyWnd"), PCWSTR::null()).ok()?;
    let (mut bar, mut tray) = (RECT::default(), RECT::default());
    GetWindowRect(taskbar, &mut bar).ok()?;
    GetWindowRect(tray_window, &mut tray).ok()?;
    let dpi = GetDpiForWindow(taskbar);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    Some((taskbar, Bar { bar, tray, scale }))
}

/// A contrast theme's own colors: its text, and the plate and text of what is marked.
unsafe fn contrast_colors() -> Option<(Rgb, Rgb, Rgb)> {
    let mut info = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let asked = SystemParametersInfoW(
        SPI_GETHIGHCONTRAST,
        info.cbSize,
        Some(&mut info as *mut HIGHCONTRASTW as *mut std::ffi::c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    if asked.is_err() || info.dwFlags.0 & HCF_HIGHCONTRASTON.0 == 0 {
        return None;
    }
    let color = |index: SYS_COLOR_INDEX| {
        let c = GetSysColor(index);
        (c as u8, (c >> 8) as u8, (c >> 16) as u8)
    };
    Some((color(COLOR_WINDOWTEXT), color(COLOR_HIGHLIGHT), color(COLOR_HIGHLIGHTTEXT)))
}

/// Goes over to another taskbar: Explorer started again, or the strip was just made.
unsafe fn watch(state: &mut State, taskbar: HWND) {
    for hook in state.hooks.drain(..) {
        let _ = UnhookWinEvent(hook);
    }
    leave(state);
    state.taskbar = taskbar;
    state.taken = None;
    let tray = if taskbar.is_invalid() {
        HWND::default()
    } else {
        FindWindowExW(Some(taskbar), None, w!("TrayNotifyWnd"), PCWSTR::null()).unwrap_or_default()
    };
    WATCH.with(|w| w.set((state.hwnd.0 as isize, taskbar.0 as isize, tray.0 as isize)));
    if taskbar.is_invalid() {
        return;
    }
    let mut process = 0u32;
    let thread = GetWindowThreadProcessId(taskbar, Some(&mut process));
    let moved = SetWinEventHook(
        EVENT_OBJECT_LOCATIONCHANGE,
        EVENT_OBJECT_LOCATIONCHANGE,
        None,
        Some(on_event),
        process,
        thread,
        WINEVENT_OUTOFCONTEXT,
    );
    state.hooks.extend(Some(moved).filter(|h| !h.is_invalid()));
}

fn ask_room(state: &mut State) {
    if state.taskbar.is_invalid() {
        return;
    }
    if let Some((taskbar, bar)) = unsafe { find_taskbar() } {
        let _ = state.room.send((taskbar.0 as isize, bar));
    }
}

/// Nothing to show, or nowhere to: the strip's window stays on its taskbar, unseen.
fn hide(state: &mut State) {
    state.frame = None;
    if !state.face.is_invalid() {
        unsafe {
            let _ = ShowWindow(state.face, SW_HIDE);
        }
    }
}

/// Looks at the taskbar and puts on it what belongs there now: the strip at its place, or nothing.
fn refresh(state: &mut State) {
    unsafe {
        // Gone with its taskbar, and nobody was there to hear of it
        if !state.face.is_invalid() && !IsWindow(Some(state.face)).as_bool() {
            leave(state);
        }
        let Some((taskbar, bar)) = find_taskbar() else {
            // Explorer is starting again
            if !state.taskbar.is_invalid() {
                watch(state, HWND::default());
            }
            return;
        };
        if taskbar != state.taskbar {
            watch(state, taskbar);
            ask_room(state);
        }
        if state.face.is_invalid() && !settle(state, taskbar) {
            return;
        }
        let height = bar.height();
        if !bar.horizontal() || height == 0 {
            hide(state);
            return;
        }
        let dpi = (bar.scale * 96.0).round() as u32;
        if state.fonts.as_ref().is_none_or(|f| f.dpi != dpi) {
            state.fonts = Some(Fonts::new(dpi));
        }
        let Some(fonts) = state.fonts.as_ref() else { return };
        let paint = Paint::of(state.content.light, contrast_colors());
        let content = &state.content;
        let laid_out =
            |r: &Rows| r.slots == content.slots && r.urgent == content.urgent && r.dpi == dpi && r.paint == paint;
        if !state.rows.as_ref().is_some_and(laid_out) {
            // With no room for all of it: the device that needs charging most, alone
            let one: Vec<Slot> = content.urgent.iter().chain(&content.slots).take(1).cloned().collect();
            state.rows = Some(Rows {
                slots: content.slots.clone(),
                urgent: content.urgent.clone(),
                dpi,
                paint,
                full: measured(&content.slots, bar.scale, &paint, fonts),
                one: (!one.is_empty()).then(|| measured(&one, bar.scale, &paint, fonts)),
            });
        }
        let Some(rows) = state.rows.as_ref() else { return };
        let narrow = rows.one.as_ref().map_or(i32::MAX, |one| one.1);
        let row = match fit(bar.room(state.taken), rows.full.1, narrow) {
            Fit::Full => Some(rows.full.clone()),
            Fit::One => rows.one.clone(),
            Fit::None => None,
        };
        let Some((pieces, width)) = row else {
            hide(state);
            return;
        };
        // The strip's place on the screen, then in the taskbar's own coordinates, which a child's place is given in
        // (as a rectangle: mirrored where Windows runs right to left)
        let (x, y) = bar.origin(width, height);
        let mut corners = [POINT { x, y }, POINT { x: x + width, y: y + height }];
        MapWindowPoints(None, Some(taskbar), &mut corners);
        let (x, y) = (corners[0].x.min(corners[1].x), corners[0].y.min(corners[1].y));
        let look = match (state.pressed || state.content.active, state.hover) {
            (true, _) => Look::Pressed,
            (false, true) => Look::Hover,
            _ => Look::Rest,
        };
        let frame = Frame { x, y, w: width, h: height, scale: bar.scale, pieces, look, paint };
        if state.frame.as_ref() != Some(&frame) {
            let Some(drawn) = draw(&frame, fonts) else { return };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let (size, from) = (SIZE { cx: width, cy: height }, POINT::default());
            let _ = SetWindowPos(state.face, None, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
            let put = UpdateLayeredWindow(
                state.face,
                None,
                None,
                Some(&size),
                Some(drawn.dc),
                Some(&from),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            if put.is_err() {
                return;
            }
            if state.frame.is_none() {
                let _ = ShowWindow(state.face, SW_SHOWNA);
            }
            state.frame = Some(frame);
        }
        // In front of the taskbar's own children: Explorer makes them anew now and then, and a new one is in front.
        // (With Start open nobody is told the taskbar's children; the strip is where it was.)
        if GetTopWindow(Some(taskbar)).is_ok_and(|first| first != state.face) {
            let _ = SetWindowPos(state.face, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        tell(state);
    }
}

/// The tooltip's text, in the taskbar's mode; the same text is the window's name, which a screen reader says.
unsafe fn tell(state: &mut State) {
    let now = (state.content.tooltip.clone(), state.content.light);
    if state.told.as_ref() == Some(&now) {
        return;
    }
    let (text, light) = (now.0.clone(), now.1);
    state.told = Some(now);
    let name = crate::win::wide(&format!("BatteryTray\n{text}"));
    let _ = SetWindowTextW(state.face, PCWSTR(name.as_ptr()));
    if state.tip.is_invalid() {
        return;
    }
    let _ = SetWindowTheme(state.tip, if light { w!("Explorer") } else { w!("DarkMode_Explorer") }, PCWSTR::null());
    let mut text = crate::win::wide(&text.replace('\n', "\r\n"));
    let tool = tool(state.face, &mut text);
    SendMessageW(state.tip, TTM_UPDATETIPTEXTW, None, Some(LPARAM(&tool as *const _ as isize)));
}

/// Every button on the taskbar, from its accessibility tree: Windows 11 draws the taskbar in XAML, where no window
/// says where the apps' buttons end.
unsafe fn buttons(automation: &IUIAutomation, taskbar: HWND) -> windows::core::Result<Vec<RECT>> {
    let cache = automation.CreateCacheRequest()?;
    cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
    cache.AddProperty(UIA_ControlTypePropertyId)?;
    let all = automation.ElementFromHandle(taskbar)?.FindAllBuildCache(
        TreeScope_Descendants,
        &automation.CreateTrueCondition()?,
        &cache,
    )?;
    let mut found = Vec::new();
    for i in 0..all.Length()? {
        let element = all.GetElement(i)?;
        let kind = element.CachedControlType()?;
        if kind == UIA_ButtonControlTypeId || kind == UIA_MenuItemControlTypeId || kind == UIA_SplitButtonControlTypeId
        {
            found.push(element.CachedBoundingRectangle()?);
        }
    }
    Ok(found)
}

/// Where the apps' buttons end on the taskbar right now, asked on the spot: for the probe (the strip asks on a thread
/// of its own, which the caller isn't held up by).
pub fn buttons_end() -> Option<i32> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let (taskbar, bar) = find_taskbar()?;
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        nearest(&buttons(&automation, taskbar).ok()?, &bar)
    }
}

/// The thread that asks Explorer where the apps' buttons end, and posts the answer to the strip's window. It ends
/// when the strip does.
fn room_thread(strip: isize) -> Sender<(isize, Bar)> {
    let (ask, asked) = channel::<(isize, Bar)>();
    let spawned = std::thread::Builder::new().name("strip-room".into()).spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let automation: Option<IUIAutomation> = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok();
        while let Ok(mut request) = asked.recv() {
            // Only the last of several asks
            while let Ok(newer) = asked.try_recv() {
                request = newer;
            }
            let (taskbar, bar) = request;
            let edge = automation
                .as_ref()
                .and_then(|a| buttons(a, HWND(taskbar as *mut _)).ok())
                .and_then(|found| nearest(&found, &bar));
            let _ = PostMessageW(
                Some(HWND(strip as *mut _)),
                WM_STRIP_ROOM,
                WPARAM(usize::from(edge.is_some())),
                LPARAM(edge.unwrap_or(0) as isize),
            );
        }
    });
    if let Err(e) = spawned {
        crate::log::write(&format!("taskbar strip: {e}"));
    }
    ask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    /// A 2560 x 1440 screen at 125%: the taskbar along the bottom, the notification area at its right end.
    fn bottom() -> Bar {
        Bar { bar: rect(0, 1380, 2560, 1440), tray: rect(2200, 1380, 2560, 1440), scale: 1.25 }
    }

    fn slot(glyph: &'static str, level: &str, tone: Tone, power: bool) -> Slot {
        Slot { glyph, level: level.into(), tone, power }
    }

    #[test]
    fn the_strip_stands_against_the_notification_area() {
        let bar = bottom();
        // A 48 px taskbar at 125% is 60 high: the strip is an item's 40 px, 50 here, in the middle of it
        assert_eq!(bar.height(), 50);
        // 4 px (5 here) clear of the arrow
        assert_eq!(bar.origin(200, 50), (2200 - 5 - 200, 1385));

        // Right to left: the notification area is at the left end, the strip to its right
        let mirrored = Bar { tray: rect(0, 1380, 360, 1440), ..bar };
        assert!(!mirrored.tray_on_right());
        assert_eq!(mirrored.origin(200, 50), (365, 1385));
    }

    #[test]
    fn a_taskbar_at_a_side_or_too_low_has_no_strip() {
        let bar = bottom();
        assert!(bar.horizontal());
        // Hidden by itself, all but two pixels of it below the screen: the strip is its child and goes with it, at
        // the same place in it
        let hidden = Bar { bar: rect(0, 1438, 2560, 1498), tray: rect(2200, 1438, 2560, 1498), ..bar };
        assert_eq!((hidden.height(), hidden.origin(200, 50)), (50, (2200 - 5 - 200, 1385 + 58)));
        // Windows 10, at the left side
        assert!(!Bar { bar: rect(0, 0, 62, 1440), tray: rect(0, 1300, 62, 1440), ..bar }.horizontal());
        // The low taskbar of a tablet: no room for an item's height
        assert_eq!(Bar { bar: rect(0, 1420, 2560, 1440), ..bar }.height(), 0);
        // Windows 10's 40 px taskbar at 100%: an item is what the 4 px above and below leave
        let ten = Bar { bar: rect(0, 1040, 1920, 1080), tray: rect(1700, 1040, 1920, 1080), scale: 1.0 };
        assert_eq!(ten.height(), 32);
    }

    #[test]
    fn the_strip_gives_way_to_the_apps_buttons() {
        let bar = bottom();
        // Nobody said where the buttons end: the whole taskbar is free
        assert_eq!(bar.room(None), 2200 - 5);
        // The last button ends at 1555: from 8 px (10 here) after it to 4 px (5) before the arrow
        assert_eq!(bar.room(Some(1555)), 2200 - 1555 - 15);
        assert_eq!(fit(630, 200, 60), Fit::Full);
        assert_eq!(fit(199, 200, 60), Fit::One);
        assert_eq!(fit(59, 200, 60), Fit::None);

        // The buttons of the taskbar: Start and two apps left of the notification area, whose own are not counted
        let found = [
            rect(1004, 1380, 1060, 1440),
            rect(1060, 1380, 1115, 1440),
            rect(1500, 1380, 1555, 1440),
            rect(2199, 1380, 2239, 1440),
            rect(2459, 1380, 2540, 1440),
            rect(0, 0, 0, 0),
        ];
        assert_eq!(nearest(&found, &bar), Some(1555));
        assert_eq!(nearest(&found[3..], &bar), None);
        let mirrored = Bar { tray: rect(0, 1380, 360, 1440), ..bar };
        assert_eq!(nearest(&found, &mirrored), Some(1004));
        assert_eq!(mirrored.room(Some(1004)), 1004 - 360 - 15);
    }

    #[test]
    fn a_row_is_glyph_level_and_lightning_per_device() {
        let paint = Paint::of(false, None);
        // Every glyph's ink 14 by 14, 2 into its square of 18 from the left and from the top; every character of a
        // level 7 wide, digits 9 rows, 5 below the top of their line
        let mut measure = |face: Face, text: &str| match face {
            Face::Glyph => Span { w: 14, tall: 14, left: 2, above: 2 },
            Face::Text => Span { w: text.chars().count() as i32 * 7, tall: 9, left: 0, above: 5 },
        };
        let slots = [
            slot("h", "37", Tone::Good, false),
            slot("m", "62", Tone::Charge, true),
            slot("k", "≈15", Tone::Warn, false),
            slot("g", "", Tone::Charge, true),
        ];
        let name = |p: &Piece| match &p.mark {
            Mark::Text(_, text) => text.clone(),
            Mark::Bolt { alone: false } => "bolt".into(),
            Mark::Bolt { alone: true } => "bolt alone".into(),
            Mark::Rule => "rule".into(),
        };
        let (pieces, width) = layout(&slots, 1.0, &paint, &mut measure);
        let at: Vec<(i32, String)> = pieces.iter().map(|p| (p.x, name(p))).collect();
        // 9 px of padding; 5 from a glyph's ink to its level, 3 from the level to the lightning; 9 on each side of a
        // hairline. A lightning with no level before it stands where the level would, 5 from the glyph
        let expected = [
            (9, "h"),
            (28, "37"),
            (51, "rule"),
            (61, "m"),
            (80, "62"),
            (97, "bolt"),
            (113, "rule"),
            (123, "k"),
            (142, "≈15"),
            (172, "rule"),
            (182, "g"),
            (201, "bolt alone"),
        ];
        assert_eq!(at, expected.map(|(x, text)| (x, text.to_string())));
        assert_eq!(width, 201 + 8 + 9);
        // A glyph's line is drawn from where its square starts, a level's from where the level does
        assert_eq!((pieces[0].left, pieces[0].above, pieces[1].left, pieces[1].above), (2, 2, 0, 5));
        // The strip's middle goes through the middle of each piece's ink: in 40 rows a glyph's 14 start at 13, a
        // level's 9 at 15 (a row above the middle where it can't be exact), the lightning's 12 at 14, the hairline's
        // 16 at 12
        let tops: Vec<i32> = pieces[..6].iter().map(|p| p.top(40)).collect();
        assert_eq!(tops, [13, 15, 12, 13, 15, 14]);
        assert_eq!((pieces[5].w, pieces[5].tall, pieces[2].w, pieces[2].tall), (7, 12, 1, 16));
        // Alone the lightning is larger: 8 by 14, from row 13
        assert_eq!((pieces[11].w, pieces[11].tall, pieces[11].top(40)), (8, 14, 13));
        // The taskbar's white; orange only for the level of the device below its reminder level; the hairlines faint
        let white = (255, 255, 255, 255);
        let rule = |p: &&Piece| p.mark == Mark::Rule;
        assert!(pieces.iter().filter(|p| !rule(p) && name(p) != "≈15").all(|p| p.color == white));
        assert!(pieces.iter().filter(rule).all(|p| p.color == (255, 255, 255, 56)));
        assert_eq!(pieces[8].color, (0xF7, 0xC0, 0x4A, 255));
        // On a light taskbar, its dark text and the darker orange
        let light = layout(&slots, 1.0, &Paint::of(true, None), &mut measure).0;
        assert_eq!((light[0].color, light[8].color), ((0x1A, 0x1A, 0x1A, 255), (0x9D, 0x5D, 0x00, 255)));
        assert_eq!(light[2].color, (0x1A, 0x1A, 0x1A, 56));
        // A contrast theme: its text color for everything, the hairlines too
        let theme = Paint::of(false, Some(((255, 255, 0), (0, 120, 215), (255, 255, 255))));
        assert!(layout(&slots, 1.0, &theme, &mut measure).0.iter().all(|p| p.color == (255, 255, 0, 255)));
        // At 125% the gaps and the shapes grow with the taskbar; a hairline stays one pixel
        let big = layout(&slots[..2], 1.25, &paint, &mut measure).0;
        assert_eq!((big[0].x, big[2].w, big[2].tall, big[5].w, big[5].tall), (11, 1, 20, 9, 15));

        // Nothing on the air: a dimmed battery and "--", no hairline
        let (none, width) = layout(&[], 1.0, &paint, &mut measure);
        assert_eq!(none.iter().map(name).collect::<Vec<_>>(), [NO_DEVICES, "--"]);
        assert_eq!((none[1].color.3, width), (153, 9 + 14 + 5 + 14 + 9));
    }

    #[test]
    fn the_hairline_and_the_lightning_are_drawn_by_the_strip_itself() {
        let (w, h) = (30, 20);
        let shape = |x, w, tall, mark, color| Piece { x, w, tall, left: 0, above: 0, mark, color };
        let rule = shape(5, 1, 16, Mark::Rule, (255, 255, 255, 56));
        let lightning = shape(12, 7, 11, Mark::Bolt { alone: false }, (255, 255, 255, 255));
        let mut bits = vec![0u8; (w * h * 4) as usize];
        shapes(&mut bits, w, h, &[rule.clone(), lightning]);
        let gray = |x: i32, y: i32| bits[((y * w + x) * 4) as usize] as i32;
        // The hairline: its one column, 16 rows around the middle
        assert_eq!(
            [gray(5, 1), gray(5, 2), gray(5, 17), gray(5, 18), gray(4, 10), gray(6, 10)],
            [0, 255, 255, 0, 0, 0]
        );
        // The lightning: in its box of 7 by 11 from row 4 on, a little over a quarter of it filled, its upper tip to
        // the right of its lower one, its middle solid
        let boxed = |x: i32, y: i32| (12..19).contains(&x) && (4..15).contains(&y);
        let all = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, _)| x > 6);
        assert!(all.clone().all(|(x, y)| boxed(x, y) || gray(x, y) == 0));
        let filled = all.map(|(x, y)| gray(x, y)).sum::<i32>() as f32 / 255.0;
        assert!((20.0..25.0).contains(&filled), "{filled} pixels");
        let weight = |y: i32| {
            (12..19).map(|x| (x - 12) * gray(x, y)).sum::<i32>() as f32
                / (12..19).map(|x| gray(x, y)).sum::<i32>() as f32
        };
        assert!(weight(4) > weight(14) + 1.0, "the tips at {} and {}", weight(4), weight(14));
        assert_eq!(gray(15, 10), 255);

        // Over the taskbar the hairline is its color at a fifth, the rest of the strip still unseen but there
        compose(&mut bits, w, h, &[rule], None, None, 1.0);
        let at = (10 * w + 5) as usize * 4;
        assert_eq!(bits[at..at + 4], [56, 56, 56, 57]);
    }

    #[test]
    fn ink_is_what_is_seen_of_a_text_not_its_smoothing() {
        let (w, h) = (6, 8);
        let mut bits = vec![0u8; (w * h * 4) as usize];
        let mut put = |x: i32, y: i32, gray: u8| bits[((y * w + x) * 4) as usize..][..3].fill(gray);
        // Faint smoothing above and beside two pixels of ink, fainter below
        put(2, 2, 60);
        put(1, 3, 90);
        put(2, 3, 200);
        put(3, 4, 255);
        put(3, 5, 40);
        assert_eq!(inked(&bits, w, h), Some([2, 3, 3, 4]));
        assert_eq!(inked(&vec![0u8; (w * h * 4) as usize], w, h), None);
    }

    /// The fonts' own lines say nothing of where a glyph or a digit is seen: the strip looks at what GDI drew.
    #[test]
    fn every_glyph_and_level_stands_on_the_strips_middle() {
        // A headset (as high as wide), a keyboard (flat, above the middle of its square), a mouse (the whole height),
        // and a gamepad that only says it charges: a lightning in place of its level
        let slots = [
            slot("\u{E7F6}", "49", Tone::Good, false),
            slot("\u{E765}", "18", Tone::Charge, true),
            slot("\u{E962}", "100", Tone::Good, false),
            slot("\u{E7FC}", "", Tone::Charge, true),
        ];
        for scale in [1.0f32, 1.25, 1.5, 1.75, 2.0] {
            unsafe {
                let fonts = Fonts::new((96.0 * scale).round() as u32);
                let paint = Paint::of(false, None);
                let (pieces, w) = measured(&slots, scale, &paint, &fonts);
                let h = (HEIGHT * scale).round() as i32;
                let frame = Frame { x: 0, y: 0, w, h, scale, pieces: pieces.clone(), look: Look::Rest, paint };
                let mut drawn = draw(&frame, &fonts).expect("a picture");
                let bits = drawn.pixels();
                // Every level at the same rows, whatever its digits
                let levels: Vec<(i32, i32)> = pieces
                    .iter()
                    .filter(|p| matches!(p.mark, Mark::Text(Face::Text, _)))
                    .map(|p| (p.above, p.tall))
                    .collect();
                assert!(levels.len() == 3 && levels.iter().all(|l| *l == levels[0] && l.1 > 0), "{levels:?}");
                for p in &pieces {
                    // The piece's rows as drawn: its columns' alpha, from what the faintest ink comes out as (the
                    // piece's own alpha over the unseen plate's)
                    let over = 96.0 / 255.0 * p.color.3 as f32 / 255.0;
                    let seen = (255.0 * over + (1.0 - over)).round() as i32;
                    let alpha = |x: i32, y: i32| bits[((y * w + x) * 4 + 3) as usize] as i32;
                    let rows: Vec<i32> = (0..h).filter(|&y| (p.x..p.x + p.w).any(|x| alpha(x, y) >= seen)).collect();
                    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
                        panic!("{:?} at {scale}: nothing drawn", p.mark)
                    };
                    // Twice the middle of its rows against twice the strip's: half a pixel apart at most
                    let off = first + last + 1 - h;
                    assert!((-1..=0).contains(&off), "{:?} at {scale}: rows {first}..={last} of {h}", p.mark);
                    // A glyph's ink starts and ends where the piece says, with nothing of its square around it
                    if matches!(p.mark, Mark::Text(Face::Glyph, _)) {
                        let columns: Vec<i32> =
                            (p.x - 4..p.x + p.w + 4).filter(|&x| (0..h).any(|y| alpha(x, y) >= seen)).collect();
                        assert_eq!((columns.first(), columns.last()), (Some(&p.x), Some(&(p.x + p.w - 1))));
                    }
                }
            }
        }
    }

    #[test]
    fn the_plate_takes_the_mouse_everywhere_and_shows_only_under_it() {
        // GDI's part: a white block where a piece is, black elsewhere
        let (w, h) = (40, 20);
        let text = Mark::Text(Face::Text, "1".into());
        let piece = Piece { x: 10, w: 6, tall: 8, left: 0, above: 0, mark: text, color: (0xF7, 0xC0, 0x4A, 255) };
        let drawn = || {
            let mut bits = vec![0u8; (w * h * 4) as usize];
            for y in 6..14 {
                for x in 10..16 {
                    let at = ((y * w + x) * 4) as usize;
                    bits[at..at + 3].fill(255);
                }
            }
            bits
        };
        let pixel = |bits: &[u8], x: i32, y: i32| {
            let at = ((y * w + x) * 4) as usize;
            (bits[at], bits[at + 1], bits[at + 2], bits[at + 3])
        };

        // At rest: the text in its color, everything else unseen but there for the mouse
        let mut rest = drawn();
        compose(&mut rest, w, h, std::slice::from_ref(&piece), None, None, 1.0);
        assert_eq!(pixel(&rest, 12, 10), (0x4A, 0xC0, 0xF7, 255));
        assert_eq!(pixel(&rest, 30, 10), (0, 0, 0, 1));
        // The corners are cut round: outside them nothing at all
        assert_eq!(pixel(&rest, 0, 0).3, 0);

        // Under the pointer on a dark taskbar: a faint white plate, a line around it
        let mut hover = drawn();
        compose(&mut hover, w, h, std::slice::from_ref(&piece), Some(Paint::of(false, None).hover), None, 1.0);
        assert_eq!(pixel(&hover, 30, 10), (20, 20, 20, 20));
        assert_eq!(pixel(&hover, 30, 0), (15, 15, 15, 15));
        assert_eq!(pixel(&hover, 12, 10), (0x4A, 0xC0, 0xF7, 255));
    }

    #[link(name = "user32", kind = "raw-dylib")]
    extern "system" {
        fn GetGuiResources(process: isize, flags: u32) -> u32;
    }

    /// The strip is drawn again whenever a level changes, for as long as the app runs.
    #[test]
    fn drawing_the_strip_leaves_no_gdi_objects_behind() {
        const CURRENT_PROCESS: isize = -1;
        const GDI_OBJECTS: u32 = 0;
        let count = || unsafe { GetGuiResources(CURRENT_PROCESS, GDI_OBJECTS) };
        let content = |level: i32| Content {
            slots: vec![slot("\u{E7F6}", &level.to_string(), Tone::Good, level % 2 == 0)],
            ..Default::default()
        };
        let first = picture(&content(100), 1.25, false).expect("a picture"); // sets up what GDI keeps for the thread
        assert!(first.0 > 0 && first.1 == 50 && first.2.len() == (first.0 * first.1 * 4) as usize);
        let before = count();
        for level in 0..100 {
            picture(&content(level), 1.25, level % 3 == 0);
        }
        assert!(count() <= before + 2, "{before} GDI objects before, {} after", count());
    }
}
