//! The notification-area icon: the level of the device that most needs charging as a colored
//! number or battery icon, a tooltip with every device, balloon notices and a small menu. The number and the menu are in the
//! taskbar's mode, light or dark, as they sit on the taskbar.

use crate::loc::t;
use crate::view::{Rgb, Tone};
use windows::core::{w, PCSTR};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush,
    SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_SWISS,
    FW_SEMIBOLD, HBITMAP, HGDIOBJ, OUT_TT_PRECIS, TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, GetSystemMetrics,
    PostMessageW, SetForegroundWindow, TrackPopupMenuEx, HICON, ICONINFO, MF_CHECKED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, SM_CXSMICON, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

pub const MENU_OPEN: u32 = 1;
pub const MENU_REFRESH: u32 = 2;
pub const MENU_EXIT: u32 = 3;
pub const MENU_SETTINGS: u32 = 4;
pub const MENU_PRIORITY_MOUSE: u32 = 10;
pub const MENU_PRIORITY_KEYBOARD: u32 = 11;
pub const MENU_PRIORITY_BLUETOOTH: u32 = 12;
pub const MENU_PRIORITY_HEADSET: u32 = 13;
pub const MENU_PRIORITY_LOWEST: u32 = 14;

pub fn priority_for_menu(id: u32) -> Option<&'static str> {
    match id {
        MENU_PRIORITY_MOUSE => Some("mouse"),
        MENU_PRIORITY_KEYBOARD => Some("keyboard"),
        MENU_PRIORITY_BLUETOOTH => Some("bluetooth"),
        MENU_PRIORITY_HEADSET => Some("headset"),
        MENU_PRIORITY_LOWEST => Some("lowest"),
        _ => None,
    }
}

pub struct Tray {
    hwnd: HWND,
    callback: u32,
    icon: HICON,
    tip: String,
}

fn copy(dst: &mut [u16], text: &str) {
    let n = dst.len() - 1;
    let mut i = 0;
    for c in text.encode_utf16().take(n) {
        dst[i] = c;
        i += 1;
    }
    dst[i] = 0;
}

impl Tray {
    /// Adds the icon; `callback` is the message Explorer sends the window for clicks on it.
    pub fn new(hwnd: HWND, callback: u32) -> Self {
        let mut tray = Self { hwnd, callback, icon: HICON::default(), tip: t("app.title") };
        tray.icon = render_icon("…", Tone::Offline, false, true);
        tray.add();
        tray
    }

    pub(crate) fn window(&self) -> HWND {
        self.hwnd
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut d = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: self.callback,
            hIcon: self.icon,
            ..Default::default()
        };
        copy(&mut d.szTip, &self.tip);
        d
    }

    fn add(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &self.data());
        }
    }

    /// Explorer restarted (TaskbarCreated): the icon has to be added again.
    pub fn readd(&self) {
        self.add();
    }

    pub fn update(&mut self, label: &str, tone: Tone, charging: bool, tooltip: &str, battery_icon: bool) {
        // Up to 127 characters; Windows cuts the rest
        self.tip = String::from_utf16_lossy(&tooltip.encode_utf16().take(127).collect::<Vec<u16>>());
        let old = self.icon;
        self.icon = render_icon(label, tone, charging, battery_icon);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data());
            if !old.is_invalid() {
                let _ = DestroyIcon(old);
            }
        }
    }

    /// A Windows notification (a toast on Windows 10 and 11). `silent`: without Windows' own sound, when the app plays
    /// the one chosen for the notice or none was asked for.
    pub fn show_notice(&self, title: &str, text: &str, silent: bool) {
        let mut d = self.data();
        d.uFlags = NIF_INFO;
        d.dwInfoFlags = if silent { NIIF_INFO | NIIF_NOSOUND } else { NIIF_INFO };
        d.Anonymous.uTimeout = 8000;
        copy(&mut d.szInfoTitle, title);
        copy(&mut d.szInfo, text);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
        }
    }
}

/// Shows the tray menu at the cursor and returns the chosen item (0 = none). A free function: the menu runs a modal
/// loop, during which the app's state must stay free for the messages it dispatches.
pub fn show_menu(hwnd: HWND, current_priority: &str) -> u32 {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let open = crate::win::wide(&t("tray.open"));
        let settings = crate::win::wide(&t("settings"));
        let refresh = crate::win::wide(&t("refresh"));
        let exit = crate::win::wide(&t("tray.exit"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN as usize, windows::core::PCWSTR(open.as_ptr()));
        if let Ok(priority_menu) = CreatePopupMenu() {
            let options = [
                (MENU_PRIORITY_MOUSE, "mouse", "tray.priority.mouse"),
                (MENU_PRIORITY_KEYBOARD, "keyboard", "tray.priority.keyboard"),
                (MENU_PRIORITY_BLUETOOTH, "bluetooth", "tray.priority.bluetooth"),
                (MENU_PRIORITY_HEADSET, "headset", "tray.priority.headset"),
                (MENU_PRIORITY_LOWEST, "lowest", "tray.priority.lowest"),
            ];
            for (id, value, key) in options {
                let label = crate::win::wide(&t(key));
                let flags = if current_priority == value { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(priority_menu, flags, id as usize, windows::core::PCWSTR(label.as_ptr()));
            }
            let priority_label = crate::win::wide(&t("tray.priority"));
            let _ =
                AppendMenuW(menu, MF_POPUP, priority_menu.0 as usize, windows::core::PCWSTR(priority_label.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS as usize, windows::core::PCWSTR(settings.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, MENU_REFRESH as usize, windows::core::PCWSTR(refresh.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, MENU_EXIT as usize, windows::core::PCWSTR(exit.as_ptr()));
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        // Without this the menu stays open when clicking elsewhere
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, at.x, at.y, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(menu);
        chosen.0 as u32
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

/// Light or dark popup menus, as the taskbar the tray menu opens from (uxtheme's SetPreferredAppMode, what Windows'
/// own apps use; nothing happens on a Windows without it). Again after every change of the mode.
pub fn set_menu_mode(light: bool) {
    unsafe {
        let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        // Ordinals 135 SetPreferredAppMode(ForceDark = 2, ForceLight = 3) and 136 FlushMenuThemes
        if let Some(set_mode) = GetProcAddress(uxtheme, PCSTR(135usize as *const u8)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            set_mode(if light { 3 } else { 2 });
        }
        if let Some(flush) = GetProcAddress(uxtheme, PCSTR(136usize as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

const OVERSAMPLE: i32 = 4;

fn dib(size: i32) -> Option<(HBITMAP, *mut u8)> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    Some((bitmap, bits.cast()))
}

/// Rasterize at four times the icon size and fit the visible ink, so "1", "100" and the bolt all center.
fn label_mask(text: &str, size: i32) -> Option<Vec<u8>> {
    let (bitmap, bits) = dib(size)?;
    unsafe { bits.write_bytes(0, (size * size * 4) as usize) };
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return None;
    }
    let mut label: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT { left: 0, top: 0, right: size, bottom: size };
    let mask = unsafe {
        let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
        let font = CreateFontW(
            -size / 2,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (VARIABLE_PITCH.0 | FF_SWISS.0) as u32,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(dc, HGDIOBJ(font.0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FF_FFFF));
        DrawTextW(dc, &mut label, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        let _ = GdiFlush();

        let pixels = std::slice::from_raw_parts(bits, (size * size * 4) as usize);
        let (pixels, _) = pixels.as_chunks::<4>();
        let mask = pixels.iter().map(|pixel| pixel[0]).collect();
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
        mask
    };
    Some(mask)
}

/// Fit the glyph's ink to the battery body in a 16×16 logical coordinate space.
fn fit_label(
    mask: &[u8],
    size: i32,
    center: (f32, f32),
    max_width: f32,
    max_height: f32,
) -> impl Fn(f32, f32) -> f32 + '_ {
    let (mut left, mut top, mut right, mut bottom) = (size, size, 0, 0);
    for (i, _) in mask.iter().enumerate().filter(|(_, ink)| **ink != 0) {
        let (x, y) = (i as i32 % size, i as i32 / size);
        (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x + 1), bottom.max(y + 1));
    }
    let width = (right - left).max(1) as f32;
    let height = (bottom - top).max(1) as f32;
    let scale = (max_width / width).min(max_height / height);
    let (mid_x, mid_y) = ((left + right) as f32 / 2.0, (top + bottom) as f32 / 2.0);
    move |x, y| {
        let sx = ((x - center.0) / scale + mid_x).floor() as i32;
        let sy = ((y - center.1) / scale + mid_y).floor() as i32;
        if (left..right).contains(&sx) && (top..bottom).contains(&sy) {
            mask[(sy * size + sx) as usize] as f32 / 255.0
        } else {
            0.0
        }
    }
}

fn rounded_rect(x: f32, y: f32, [left, top, right, bottom]: [f32; 4], radius: f32) -> bool {
    let dx = ((x - (left + right) / 2.0).abs() - (right - left) / 2.0 + radius).max(0.0);
    let dy = ((y - (top + bottom) / 2.0).abs() - (bottom - top) / 2.0 + radius).max(0.0);
    dx * dx + dy * dy <= radius * radius
}

fn mix((r, g, b): Rgb, (tr, tg, tb): Rgb, a: f32) -> Rgb {
    let channel = |base: u8, tint: u8| (base as f32 + (tint as f32 - base as f32) * a).round() as u8;
    (channel(r, tr), channel(g, tg), channel(b, tb))
}

/// The white bolt overlays the upper corner of the battery without a backing plate.
fn lightning(x: f32, y: f32) -> bool {
    const POINTS: [(f32, f32); 6] =
        [(14.65, 0.25), (12.25, 3.6), (13.65, 3.6), (13.2, 6.25), (15.75, 2.95), (14.35, 2.95)];
    let edges = POINTS.iter().zip(POINTS.iter().cycle().skip(1));
    edges.fold(false, |inside, (&(ax, ay), &(bx, by))| {
        inside ^ ((ay > y) != (by > y) && x < ax + (bx - ax) * (y - ay) / (by - ay))
    })
}

fn render_icon(text: &str, tone: Tone, charging: bool, battery_icon: bool) -> HICON {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    if battery_icon {
        render_at_size(text, tone, charging, size)
    } else {
        render_number(text, tone, size)
    }
}

fn render_number(text: &str, tone: Tone, size: i32) -> HICON {
    let raster = size * OVERSAMPLE;
    let Some(ink) = label_mask(text, raster) else { return HICON::default() };
    let mark = fit_label(&ink, raster, (8.0, 8.0), 15.0, 13.0);
    let Some((bitmap, bits)) = dib(size) else { return HICON::default() };
    let pixels = unsafe { std::slice::from_raw_parts_mut(bits, (size * size * 4) as usize) };
    let (r, g, b) = tone.rgb(crate::theme::current().taskbar_light);
    let unit = 16.0 / raster as f32;
    for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (i as i32 % size) * OVERSAMPLE;
        let y = (i as i32 / size) * OVERSAMPLE;
        let mut coverage = 0.0;
        for dy in 0..OVERSAMPLE {
            for dx in 0..OVERSAMPLE {
                coverage += mark((x + dx) as f32 * unit + unit / 2.0, (y + dy) as f32 * unit + unit / 2.0);
            }
        }
        let alpha = coverage / (OVERSAMPLE * OVERSAMPLE) as f32;
        *pixel = [b, g, r, 255].map(|channel| (channel as f32 * alpha).round() as u8);
    }
    icon_from_bitmap(bitmap, size)
}

/// Draw a vertical battery with proportional fill, a legible number and an optional white bolt.
fn render_at_size(text: &str, tone: Tone, charging: bool, size: i32) -> HICON {
    const WHITE: Rgb = (255, 255, 255);
    const SHELL: Rgb = (36, 36, 36);
    let level = text.parse::<i32>().ok().map(|value| value.clamp(0, 100));
    let shows_bolt = charging && level.is_some();
    let (label, tint) = match (text, tone, charging) {
        (_, _, true) => (text, WHITE),
        ("--", _, _) => ("–", (186, 186, 186)),
        ("…", _, _) => (text, (186, 186, 186)),
        (_, Tone::Good, _) => (text, WHITE),
        _ => (text, tone.rgb(crate::theme::current().taskbar_light)),
    };
    let border = if level.is_some() || charging { WHITE } else { mix(WHITE, tint, 0.52) };
    let fill = if tint == WHITE { mix(SHELL, WHITE, 0.30) } else { tint };
    let fill_top = 14.5 - level.map_or(0.0, |value| 10.55 * value as f32 / 100.0);
    let raster = size * OVERSAMPLE;
    let Some(ink) = label_mask(label, raster) else { return HICON::default() };
    let (center, max_width) = match (level, shows_bolt) {
        (_, true) => ((7.7, 9.6), 10.4),
        (Some(_), false) => ((8.0, 9.15), 11.15),
        (None, false) => ((8.0, 9.15), 6.3),
    };
    let mark = fit_label(&ink, raster, center, max_width, 8.35);
    let Some((bitmap, bits)) = dib(size) else { return HICON::default() };
    let pixels = unsafe { std::slice::from_raw_parts_mut(bits, (size * size * 4) as usize) };
    let (pixels, _) = pixels.as_chunks_mut::<4>();
    let unit = 16.0 / raster as f32;

    for (i, pixel) in pixels.iter_mut().enumerate() {
        let px = i as i32 % size;
        let py = i as i32 / size;
        let mut sum = [0u32; 4];
        for sample_y in 0..OVERSAMPLE {
            for sample_x in 0..OVERSAMPLE {
                let x = (px * OVERSAMPLE + sample_x) as f32 * unit + unit / 2.0;
                let y = (py * OVERSAMPLE + sample_y) as f32 * unit + unit / 2.0;
                let (r, g, b) = if shows_bolt && x > 12.0 && y < 6.5 && lightning(x, y) {
                    WHITE
                } else if rounded_rect(x, y, [2.08, 3.95, 13.92, 14.5], 1.65) {
                    let background = if y >= fill_top { fill } else { SHELL };
                    let foreground = if level.is_some() || charging { WHITE } else { tint };
                    mix(background, foreground, mark(x, y))
                } else if rounded_rect(x, y, [1.05, 2.9, 14.95, 15.55], 2.65)
                    || rounded_rect(x, y, [5.6, 0.5, 10.4, 3.9], 0.85)
                {
                    border
                } else {
                    continue;
                };
                for (total, channel) in sum.iter_mut().zip([b, g, r, 255]) {
                    *total += channel as u32;
                }
            }
        }
        // Averaging transparent samples as zero produces the premultiplied BGRA Explorer expects.
        let samples = (OVERSAMPLE * OVERSAMPLE) as u32;
        for (channel, total) in pixel.iter_mut().zip(sum) {
            *channel = ((total + samples / 2) / samples) as u8;
        }
    }

    icon_from_bitmap(bitmap, size)
}

fn icon_from_bitmap(bitmap: HBITMAP, size: i32) -> HICON {
    unsafe {
        let mask_bytes = vec![0u8; (((size + 15) / 16) * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bytes.as_ptr().cast()));
        let info = ICONINFO { fIcon: true.into(), hbmMask: mask, hbmColor: bitmap, ..Default::default() };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "user32", kind = "raw-dylib")]
    extern "system" {
        fn GetGuiResources(process: isize, flags: u32) -> u32;
    }

    /// The icon is drawn again after every poll: anything it doesn't give back adds up to a blank tray icon.
    #[test]
    fn drawing_the_icon_leaves_no_gdi_objects_behind() {
        const CURRENT_PROCESS: isize = -1;
        const GDI_OBJECTS: u32 = 0;
        let count = || unsafe { GetGuiResources(CURRENT_PROCESS, GDI_OBJECTS) };
        let draw = |label: &str, charging: bool| unsafe {
            let tone = if charging { Tone::Charge } else { Tone::Good };
            let icon = render_at_size(label, tone, charging, 16);
            assert!(!icon.is_invalid(), "could not draw {label}");
            let _ = DestroyIcon(icon);
        };
        let draw_number = |label: &str| unsafe {
            let icon = render_number(label, Tone::Good, 16);
            assert!(!icon.is_invalid(), "could not draw {label}");
            let _ = DestroyIcon(icon);
        };
        draw("100", false); // the first draw sets up what GDI keeps for the thread
        draw_number("100");
        let before = count();
        for level in 0..=100 {
            draw(&level.to_string(), false);
            draw(&level.to_string(), true);
            draw_number(&level.to_string());
        }
        assert!(count() <= before + 2, "{before} GDI objects before, {} after", count());
    }
}
