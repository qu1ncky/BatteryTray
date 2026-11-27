//! The notification-area icon: the level of the device that most needs charging as a colored
//! number, a tooltip with every device, balloon notices and a small menu. The number and the menu are in the
//! taskbar's mode, light or dark, as they sit on the taskbar.

use crate::loc::t;
use windows::core::{w, PCSTR};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush,
    GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, FF_SWISS, FW_BOLD, HGDIOBJ, OUT_TT_PRECIS, TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, GetSystemMetrics,
    PostMessageW, SetForegroundWindow, TrackPopupMenuEx, HICON, ICONINFO, MF_SEPARATOR, MF_STRING, SM_CXSMICON,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

pub const MENU_OPEN: u32 = 1;
pub const MENU_REFRESH: u32 = 2;
pub const MENU_EXIT: u32 = 3;
pub const MENU_SETTINGS: u32 = 4;
pub const MENU_HIDE_STRIP: u32 = 5;

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
        tray.icon = render_icon("…", (0x80, 0x80, 0x80));
        tray.add();
        tray
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

    pub fn update(&mut self, label: &str, color: (u8, u8, u8), tooltip: &str) {
        // Up to 127 characters; Windows cuts the rest
        self.tip = String::from_utf16_lossy(&tooltip.encode_utf16().take(127).collect::<Vec<u16>>());
        let old = self.icon;
        self.icon = render_icon(label, color);
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
/// loop, during which the app's state must stay free for the messages it dispatches. Opened from the taskbar strip
/// (`on_strip`), it also offers to take the strip off the taskbar, right where the strip is.
pub fn show_menu(hwnd: HWND, on_strip: bool) -> u32 {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let open = crate::win::wide(&t("tray.open"));
        let settings = crate::win::wide(&t("settings"));
        let refresh = crate::win::wide(&t("refresh"));
        let exit = crate::win::wide(&t("tray.exit"));
        let hide_strip = crate::win::wide(&t("tray.hide.strip"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN as usize, windows::core::PCWSTR(open.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS as usize, windows::core::PCWSTR(settings.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, MENU_REFRESH as usize, windows::core::PCWSTR(refresh.as_ptr()));
        if on_strip {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let hide = windows::core::PCWSTR(hide_strip.as_ptr());
            let _ = AppendMenuW(menu, MF_STRING, MENU_HIDE_STRIP as usize, hide);
        }
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

/// The label as large as fits the icon, in the color given, on a transparent background. GDI draws it white on black
/// with grayscale antialiasing; the gray becomes the alpha of the colored text.
pub fn render_icon(text: &str, (r, g, b): (u8, u8, u8)) -> HICON {
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            return HICON::default();
        };
        let old_bitmap = SelectObject(dc, HGDIOBJ(color.0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FF_FFFF));

        let mut label: Vec<u16> = text.encode_utf16().collect();
        let mut em = size;
        let mut font;
        // The DC's own font, put back before ours is deleted: a font still selected into a DC isn't deleted, and one
        // leaked per tray update would use up the process's GDI objects within days
        let mut dc_font = None;
        loop {
            font = CreateFontW(
                -em,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
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
            let previous = SelectObject(dc, HGDIOBJ(font.0));
            dc_font.get_or_insert(previous);
            let mut extent = Default::default();
            let _ = GetTextExtentPoint32W(dc, &label, &mut extent);
            if (extent.cx <= size && extent.cy <= size) || em <= 5 {
                break;
            }
            if let Some(own) = dc_font {
                SelectObject(dc, own);
            }
            let _ = DeleteObject(HGDIOBJ(font.0));
            em -= 1;
        }
        let mut rect = RECT { left: 0, top: 0, right: size, bottom: size };
        DrawTextW(dc, &mut label, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        let _ = GdiFlush();

        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
        for px in pixels.as_chunks_mut::<4>().0 {
            let alpha = px[0].max(px[1]).max(px[2]);
            px[0] = b;
            px[1] = g;
            px[2] = r;
            px[3] = alpha;
        }

        let mask_bytes = vec![0u8; (((size + 15) / 16) * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bytes.as_ptr() as *const _));
        let icon_info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&icon_info).unwrap_or_default();

        SelectObject(dc, old_bitmap);
        if let Some(own) = dc_font {
            SelectObject(dc, own);
        }
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
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
        let draw = |label: &str| unsafe {
            let _ = DestroyIcon(render_icon(label, (0x6C, 0xCB, 0x5F)));
        };
        draw("100"); // the first draw sets up what GDI keeps for the thread
        let before = count();
        for level in 0..100 {
            draw(&level.to_string());
        }
        assert!(count() <= before + 2, "{before} GDI objects before, {} after", count());
    }
}
