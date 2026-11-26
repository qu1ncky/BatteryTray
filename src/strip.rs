//! The taskbar strip (Settings > General > Taskbar): the first devices of the list as their glyphs and levels, on the
//! taskbar to the left of the notification area, in sight without a click.
//!
//! Windows gives an app no place of its own on the taskbar but its notification icon, and Windows 11 keeps a new icon
//! under the arrow until the user drags it out. So the strip is a window of the app's own, made a child of the
//! taskbar's window and drawn to look like a part of it: the tray's glyph and text sizes, the taskbar's text color,
//! one hover target for the whole of it. The window is layered with an alpha of its own for every pixel (the taskbar
//! shows through), never takes the focus, and is off unless the user turns it on.
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
    DT_SINGLELINE, DT_VCENTER, FF_SWISS, FW_NORMAL, HBITMAP, HDC, HFONT, HGDIOBJ, OUT_TT_PRECIS, SYS_COLOR_INDEX,
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

/// Sizes in Windows' own pixels at 100%, a taskbar item's: on a 48 px taskbar its hover target is 40 px high with
/// corners of 4, a tray icon is a 16 px glyph, the tray's text is 12 px (Caption in Windows 11's type ramp).
const HEIGHT: f32 = 40.0;
const INSET: f32 = 4.0;
const LOWEST: f32 = 24.0;
const RADIUS: f32 = 4.0;
const PAD: f32 = 8.0;
const GLYPH: f32 = 16.0;
const TEXT: f32 = 12.0;
/// Between a device's glyph and its level, between the level and the lightning, and between two devices
const GLYPH_GAP: f32 = 4.0;
const BOLT_GAP: f32 = 1.0;
const DEVICE_GAP: f32 = 12.0;
/// Left clear between the strip and the arrow of the notification area, and between the last app button and the strip
const TRAY_GAP: f32 = 4.0;
const BUTTONS_GAP: f32 = 8.0;

/// The lightning after the level of a device that charges, and the battery shown while no device is on the air
/// (Segoe Fluent Icons and Segoe MDL2 Assets have both)
const BOLT: &str = "\u{E945}";
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

/// The fonts a piece of the strip is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Glyph,
    Text,
    Bolt,
}

/// One run of text at its place in the strip.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Piece {
    x: i32,
    w: i32,
    face: Face,
    text: String,
    color: Rgba,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Rest,
    Hover,
    Pressed,
}

/// The pieces of a row of devices, left to right, and the row's width with its padding. `measure` says how wide a
/// text comes out in a face.
fn layout(slots: &[Slot], scale: f32, paint: &Paint, measure: &mut dyn FnMut(Face, &str) -> i32) -> (Vec<Piece>, i32) {
    let px = |v: f32| (v * scale).round() as i32;
    let opaque = |c: Rgb| (c.0, c.1, c.2, 255);
    let mut pieces = Vec::new();
    let mut x = px(PAD);
    let mut add = |x: &mut i32, face: Face, text: &str, color: Rgba| {
        let w = measure(face, text);
        pieces.push(Piece { x: *x, w, face, text: text.to_string(), color });
        *x += w;
    };
    if slots.is_empty() {
        // Nothing on the air: a battery and two dashes, dimmed, as the tray icon's "--"
        let dim = (paint.text.0, paint.text.1, paint.text.2, 153);
        add(&mut x, Face::Glyph, NO_DEVICES, dim);
        x += px(GLYPH_GAP);
        add(&mut x, Face::Text, "--", dim);
    }
    for (i, slot) in slots.iter().enumerate() {
        if i > 0 {
            x += px(DEVICE_GAP);
        }
        add(&mut x, Face::Glyph, slot.glyph, opaque(paint.text));
        if !slot.level.is_empty() {
            x += px(GLYPH_GAP);
            add(&mut x, Face::Text, &slot.level, opaque(paint.level(slot.tone)));
        }
        if slot.power {
            x += px(BOLT_GAP);
            add(&mut x, Face::Bolt, BOLT, opaque(paint.text));
        }
    }
    (pieces, x + px(PAD))
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
    bolt: HFONT,
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
        let text = if crate::win::windows_build() >= 22000 { "Segoe UI Variable Text" } else { "Segoe UI" };
        let icons = crate::gui::icon_font();
        Fonts { dpi, glyph: make(GLYPH, icons), text: make(TEXT, text), bolt: make(TEXT, icons) }
    }

    fn of(&self, face: Face) -> HFONT {
        match face {
            Face::Glyph => self.glyph,
            Face::Text => self.text,
            Face::Bolt => self.bolt,
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for font in [self.glyph, self.text, self.bolt] {
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

unsafe fn measure(dc: HDC, fonts: &Fonts, face: Face, text: &str) -> i32 {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let before = SelectObject(dc, HGDIOBJ(fonts.of(face).0));
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
    SelectObject(dc, before);
    size.cx
}

/// A row of devices laid out with the real fonts.
unsafe fn measured(slots: &[Slot], scale: f32, paint: &Paint, fonts: &Fonts) -> (Vec<Piece>, i32) {
    let dc = CreateCompatibleDC(None);
    let row = layout(slots, scale, paint, &mut |face, text| measure(dc, fonts, face, text));
    let _ = DeleteDC(dc);
    row
}

/// The strip's pixels: the pieces drawn by GDI, white on black, then made into their colors over the plate.
unsafe fn draw(frame: &Frame, fonts: &Fonts) -> Option<Picture> {
    let mut picture = Picture::new(frame.w, frame.h)?;
    SetBkMode(picture.dc, TRANSPARENT);
    SetTextColor(picture.dc, COLORREF(0x00FF_FFFF));
    let own = SelectObject(picture.dc, HGDIOBJ(fonts.text.0));
    for p in &frame.pieces {
        SelectObject(picture.dc, HGDIOBJ(fonts.of(p.face).0));
        let mut text: Vec<u16> = p.text.encode_utf16().collect();
        let mut rect = RECT { left: p.x, top: 0, right: p.x + p.w, bottom: frame.h };
        DrawTextW(picture.dc, &mut text, &mut rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_NOCLIP);
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
        // Every glyph 16 wide, every character of a level 7
        let mut measure = |face: Face, text: &str| match face {
            Face::Glyph => 16,
            Face::Bolt => 12,
            Face::Text => text.chars().count() as i32 * 7,
        };
        let slots = [
            slot("h", "37", Tone::Good, false),
            slot("m", "62", Tone::Charge, true),
            slot("k", "≈15", Tone::Warn, false),
            slot("g", "", Tone::Charge, true),
        ];
        let (pieces, width) = layout(&slots, 1.0, &paint, &mut measure);
        let at: Vec<(i32, &str)> = pieces.iter().map(|p| (p.x, p.text.as_str())).collect();
        assert_eq!(
            at,
            [
                (8, "h"),
                (28, "37"),
                (54, "m"),
                (74, "62"),
                (89, BOLT),
                (113, "k"),
                (133, "≈15"),
                (166, "g"),
                (183, BOLT)
            ]
        );
        assert_eq!(width, 183 + 12 + 8);
        // The taskbar's white; orange only for the level of the device below its reminder level
        let white = (255, 255, 255, 255);
        assert!(pieces.iter().filter(|p| p.text != "≈15").all(|p| p.color == white));
        assert_eq!(pieces[6].color, (0xF7, 0xC0, 0x4A, 255));
        // On a light taskbar, its dark text and the darker orange
        let light = layout(&slots, 1.0, &Paint::of(true, None), &mut measure).0;
        assert_eq!((light[0].color, light[6].color), ((0x1A, 0x1A, 0x1A, 255), (0x9D, 0x5D, 0x00, 255)));
        // A contrast theme: its text color for everything
        let theme = Paint::of(false, Some(((255, 255, 0), (0, 120, 215), (255, 255, 255))));
        assert!(layout(&slots, 1.0, &theme, &mut measure).0.iter().all(|p| p.color == (255, 255, 0, 255)));

        // Nothing on the air: a dimmed battery and "--"
        let (none, width) = layout(&[], 1.0, &paint, &mut measure);
        assert_eq!(none.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), [NO_DEVICES, "--"]);
        assert_eq!((none[1].color.3, width), (153, 8 + 16 + 4 + 14 + 8));
    }

    #[test]
    fn the_plate_takes_the_mouse_everywhere_and_shows_only_under_it() {
        // GDI's part: a white block where a piece is, black elsewhere
        let (w, h) = (40, 20);
        let piece = Piece { x: 10, w: 6, face: Face::Text, text: "1".into(), color: (0xF7, 0xC0, 0x4A, 255) };
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
