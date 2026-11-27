//! The Slint side: the compiled windows, the winit backend with the software renderer, and the Win32 touches Slint
//! has no API for (a window that never takes focus, click-through, translucency, the work area).
//! The panel, reminder and overlay get their styles the moment Windows creates them (a CBT hook spots their
//! window classes), before they could ever be seen, and keep them: winit writes a window's extended style afresh
//! whenever it changes the window state. The layered reminder and overlay stay invisible until `set_alpha`.

use crate::loc;
use slint::winit_030::{winit, WinitWindowAccessor};
use slint::ComponentHandle;
use std::cell::Cell;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
    GWL_STYLE, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_APPWINDOW, WS_EX_DLGMODALFRAME,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
};

slint::include_modules!();

/// What kind of window the next component created is: winit is told before the window exists.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The tray panel, owned by the hidden tray window so it can retain the standard caption.
    Panel(HWND),
    /// The low battery reminder: shown without taking focus, no taskbar button, no frame.
    Reminder,
    /// The overlay: as the reminder, and the mouse goes through it to whatever is underneath.
    Overlay,
}

const PANEL_CLASS: &str = "BatteryTray.Panel";
const REMINDER_CLASS: &str = "BatteryTray.Reminder";
const OVERLAY_CLASS: &str = "BatteryTray.Overlay";

thread_local! {
    static NEXT: Cell<Option<Kind>> = const { Cell::new(None) };
}

/// Creates a component of the given kind (the attributes hook reads the kind while the component is created).
pub fn create<C>(kind: Kind, new: impl FnOnce() -> Result<C, slint::PlatformError>) -> Result<C, slint::PlatformError> {
    let previous = NEXT.with(|n| n.replace(Some(kind)));
    let created = new();
    NEXT.with(|n| n.set(previous));
    created
}

/// The winit backend with Slint's software renderer: no GPU context, little memory, freed with the window.
pub fn init() -> Result<(), slint::PlatformError> {
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_CBT};
    use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
    // For as long as the app runs, on the thread that creates the windows
    unsafe {
        let _ = SetWindowsHookExW(WH_CBT, Some(on_create), None, GetCurrentThreadId());
    }
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attributes| {
            let popup = |a: winit::window::WindowAttributes, class: &str| {
                a.with_class_name(class)
                    .with_active(false)
                    .with_skip_taskbar(true)
                    .with_corner_preference(CornerPreference::Round)
            };
            match NEXT.with(|n| n.get()) {
                Some(Kind::Panel(owner)) => attributes
                    .with_class_name(PANEL_CLASS)
                    .with_owner_window(owner.0 as isize)
                    .with_enabled_buttons(winit::window::WindowButtons::CLOSE)
                    .with_skip_taskbar(true)
                    .with_theme(Some(title_bar(crate::theme::current().apps_light)))
                    .with_corner_preference(CornerPreference::Round),
                Some(Kind::Reminder) => popup(attributes, REMINDER_CLASS),
                // The overlay keeps its dark contents and frame in either app mode.
                Some(Kind::Overlay) => popup(attributes, OVERLAY_CLASS).with_theme(Some(winit::window::Theme::Dark)),
                None => attributes,
            }
        })
        .select()
}

struct WindowStyles {
    extended: u32,
    remove: u32,
}

static PANEL_STYLES: WindowStyles =
    WindowStyles { extended: WS_EX_DLGMODALFRAME.0, remove: WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 };
const PASSIVE_POPUP: u32 = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_LAYERED.0;
static REMINDER_STYLES: WindowStyles = WindowStyles { extended: PASSIVE_POPUP, remove: 0 };
static OVERLAY_STYLES: WindowStyles = WindowStyles { extended: PASSIVE_POPUP | WS_EX_TRANSPARENT.0, remove: 0 };

/// The panel uses a dialog caption without an app icon. Only reminders and overlays are passive.
fn popup_styles(hwnd: HWND) -> Option<&'static WindowStyles> {
    let mut name = [0u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut name) };
    let class = String::from_utf16_lossy(&name[..n.max(0) as usize]);
    match class.as_str() {
        PANEL_CLASS => Some(&PANEL_STYLES),
        REMINDER_CLASS => Some(&REMINDER_STYLES),
        OVERLAY_CLASS => Some(&OVERLAY_STYLES),
        _ => None,
    }
}

fn popup_style(style: u32, bits: u32) -> u32 {
    (style | bits) & !WS_EX_APPWINDOW.0
}

/// A window is being created on the UI thread: a popup gets its styles now, and keeps them.
unsafe extern "system" fn on_create(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::SetWindowSubclass;
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, HCBT_CREATEWND};
    if code == HCBT_CREATEWND as i32 {
        let hwnd = HWND(wparam.0 as *mut _);
        if let Some(styles) = popup_styles(hwnd) {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, popup_style(style, styles.extended) as isize);
            if styles.remove != 0 {
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                SetWindowLongPtrW(hwnd, GWL_STYLE, (style & !styles.remove) as isize);
            }
            // The class-specific style policy is static and outlives this window.
            let _ = SetWindowSubclass(hwnd, Some(keep_styles), KEEP_STYLES_ID, styles as *const WindowStyles as usize);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Gives a window's T global the app's texts and icon font.
pub fn setup_texts(t: &T<'_>) {
    t.on_tr(|language, key| loc::t_in(&language, &key).into());
    t.set_lang(loc::current().into());
    t.set_icon_font(icon_font().into());
}

/// Gives a window the light or the dark colours: the app's palette, and the std widgets' (its scroll bars). Without
/// this a window stays dark, as the overlay does.
pub fn set_theme<C>(ui: &C, light: bool)
where
    C: ComponentHandle,
    for<'a> Palette<'a>: slint::Global<'a, C>,
    for<'a> WidgetPalette<'a>: slint::Global<'a, C>,
{
    use slint::language::ColorScheme;
    ui.global::<Palette>().set_light(light);
    ui.global::<WidgetPalette>().set_color_scheme(if light { ColorScheme::Light } else { ColorScheme::Dark });
}

fn title_bar(light: bool) -> winit::window::Theme {
    if light {
        winit::window::Theme::Light
    } else {
        winit::window::Theme::Dark
    }
}

/// Switches an open window's title bar to the light or the dark mode. Its frame is drawn again right away: the new
/// colours could otherwise wait for the window's next activation. Windows 10 repaints the caption's colours only on
/// an activation change, so the caption is drawn inactive and back (as Tauri's tao does), without the window's own
/// handler seeing it.
pub fn set_title_bar(window: &slint::Window, light: bool) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, SWP_FRAMECHANGED, SWP_NOZORDER, WM_NCACTIVATE};
    window.with_winit_window(|w| w.set_theme(Some(title_bar(light))));
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags);
            let active = GetActiveWindow() == hwnd;
            let _ = DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(usize::from(!active)), LPARAM(0));
            let _ = DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(usize::from(active)), LPARAM(0));
        }
    }
}

/// Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets on Windows 10.
fn icon_font() -> &'static str {
    let fonts = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_default().join("Fonts");
    if fonts.join("SegoeIcons.ttf").exists() {
        "Segoe Fluent Icons"
    } else {
        "Segoe MDL2 Assets"
    }
}

/// The Win32 handle of a shown window.
pub fn hwnd(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window.with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    })?
}

/// Windows' own "Open" dialog for a sound file, listing WAV and MP3; None when it is cancelled. It runs a loop of its
/// own until it closes, so the app's state has to be free while it is open (as for the tray's menu).
pub fn pick_sound_file(owner: Option<HWND>) -> Option<std::path::PathBuf> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, SIGDN_FILESYSPATH,
    };
    let name = crate::win::wide(&crate::loc::t("sounds.files"));
    // COM is set up on the UI thread already: winit does it for drag and drop
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetFileTypes(&[COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: w!("*.wav;*.mp3") }])
            .ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM).ok()?;
        dialog.Show(owner).ok()?;
        let path = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        text.map(std::path::PathBuf::from)
    }
}

/// Puts a file into the Recycle Bin, without asking and without a window; false when Windows couldn't (a drive
/// without a bin, a file that is gone).
pub fn recycle(path: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW,
    };
    // The function takes a list of paths, each ended by a NUL and the list by one more
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    unsafe { SHFileOperationW(&mut operation) == 0 && !operation.fAnyOperationsAborted.as_bool() }
}

/// Colors a popup's border (the reminder's, in its accent).
pub fn set_border(window: &slint::Window, border: Option<(u8, u8, u8)>) {
    use winit::platform::windows::WindowExtWindows;
    window.with_winit_window(|w| {
        w.set_border_color(border.map(|(r, g, b)| winit::platform::windows::Color::from_rgb(r, g, b)));
    });
}

/// How opaque a popup is drawn: 0 unseen (it is a layered window, see the module's notes), 255 opaque.
pub fn set_alpha(window: &slint::Window, alpha: u8) {
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }
}

const KEEP_STYLES_ID: usize = 0xB7;

/// Retains the native popup styles when winit or Slint changes the window state.
unsafe extern "system" fn keep_styles(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        STYLESTRUCT, WINDOWPOS, WM_NCDESTROY, WM_STYLECHANGING, WM_WINDOWPOSCHANGING,
    };
    let styles = &*(data as *const WindowStyles);
    match msg {
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = popup_style(change.styleNew, styles.extended);
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_STYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew &= !styles.remove;
        }
        WM_WINDOWPOSCHANGING if std::ptr::eq(styles, &PANEL_STYLES) => {
            place_initial_panel(hwnd, &mut *(lparam.0 as *mut WINDOWPOS));
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(keep_styles), KEEP_STYLES_ID);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Anchors the panel while it is sized or moved before its first visible frame.
fn place_initial_panel(hwnd: HWND, position: &mut windows::Win32::UI::WindowsAndMessaging::WINDOWPOS) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible, SWP_HIDEWINDOW, SWP_SHOWWINDOW};
    let showing = position.flags.contains(SWP_SHOWWINDOW);
    if position.flags.contains(SWP_HIDEWINDOW)
        || (!showing
            && (unsafe { IsWindowVisible(hwnd).as_bool() } || position.flags.contains(SWP_NOMOVE | SWP_NOSIZE)))
    {
        return;
    }
    unsafe {
        let mut outer = RECT::default();
        if GetWindowRect(hwnd, &mut outer).is_err() {
            return;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let area = work_area(monitor);
        let no_size = position.flags.contains(SWP_NOSIZE);
        let height = if no_size { outer.bottom - outer.top } else { position.cy };
        let capped = height.min(area.bottom - area.top);
        if position.flags.contains(SWP_NOMOVE) {
            position.x = outer.left;
        }
        position.y = (area.bottom - (12.0 * scale_of(monitor)).round() as i32 - capped).max(area.top);
        position.flags &= !SWP_NOMOVE;
        if capped < height {
            if no_size {
                position.cx = outer.right - outer.left;
            }
            position.cy = capped;
            position.flags &= !SWP_NOSIZE;
        }
    }
}

/// Where a shown window really is, and its monitor's scale, as Windows has them. Right after show() Slint can still
/// report the size and scale the window was created with (100%), while the window itself already has its own.
pub fn window_rect(window: &slint::Window) -> Option<(RECT, f32)> {
    let hwnd = hwnd(window)?;
    let mut r = RECT::default();
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut r).ok()?;
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd);
        Some((r, if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }))
    }
}

/// Moves a window right away (not through winit, which may do it later), without activating it.
pub fn move_window(window: &slint::Window, x: i32, y: i32) {
    use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Fits the panel to its content while keeping its bottom edge near the tray.
pub(crate) fn fit_panel(window: &slint::Window, logical_height: f32, first_show: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, SWP_NOZORDER};
    let Some(hwnd) = hwnd(window) else { return };
    let Some((outer, scale)) = window_rect(window) else { return };
    unsafe {
        let mut client = RECT::default();
        if GetClientRect(hwnd, &mut client).is_err() {
            return;
        }
        let area = work_area(MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY));
        let frame = (outer.bottom - outer.top) - (client.bottom - client.top);
        let height = ((logical_height * scale).ceil() as i32 + frame).min(area.bottom - area.top);
        let anchor = if first_show { area.bottom - (12.0 * scale).round() as i32 } else { outer.bottom };
        let bottom = anchor.clamp(area.top + height, area.bottom);
        let top = bottom - height;
        if top != outer.top || bottom != outer.bottom {
            let _ = SetWindowPos(
                hwnd,
                None,
                outer.left,
                top,
                outer.right - outer.left,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// Puts a popup back on top: full-screen apps and other topmost windows can push it back.
pub fn keep_on_top(window: &slint::Window) {
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// The work area (screen without the taskbar), in physical pixels, of the primary monitor.
pub fn primary_work_area() -> RECT {
    unsafe { work_area(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)) }
}

/// The primary monitor's work area and scale (1.25 at 125%).
pub fn primary_monitor() -> (RECT, f32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        (work_area(monitor), scale_of(monitor))
    }
}

/// The work area and scale of the monitor the active app is on.
pub fn active_monitor() -> (RECT, f32) {
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        (work_area(monitor), scale_of(monitor))
    }
}

/// A monitor's own scale: a window opened on it is laid out at this one, before it has said so itself.
unsafe fn scale_of(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> f32 {
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let (mut x, mut y) = (96u32, 96u32);
    if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_err() || x == 0 {
        x = windows::Win32::UI::HiDpi::GetDpiForSystem();
    }
    x as f32 / 96.0
}

unsafe fn work_area(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        info.rcWork
    } else {
        RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
    }
}

/// Opens a component's window and brings it to the front (the app was asked to by a click or a second launch).
pub fn show_and_activate<C: ComponentHandle>(component: &C) {
    let _ = component.show();
    component.window().set_minimized(false);
    if let Some(hwnd) = hwnd(component.window()) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
        }
    }
}
