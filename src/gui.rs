//! The Slint side: the compiled windows, the winit backend with the software renderer, and the Win32 touches Slint
//! has no API for (a window that never takes focus, click-through, translucency, the work area).
//! The reminder and the overlay get their extended styles the moment Windows creates them (a CBT hook spots their
//! window classes), before they could ever be seen, and keep them: winit writes a window's extended style afresh
//! whenever it changes the window's state, which Slint does several times after showing it. A layered window is not
//! drawn until it is given an opacity, so they stay unseen until `set_alpha` - once they stand where they belong.

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
    HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};

slint::include_modules!();

/// What kind of window the next component created is: winit is told before the window exists.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The main window: a title bar in the app mode, the app icon, a taskbar button.
    Normal,
    /// The low battery reminder: shown without taking focus, no taskbar button, no frame.
    Reminder,
    /// The overlay: as the reminder, and the mouse goes through it to whatever is underneath.
    Overlay,
}

const REMINDER_CLASS: &str = "BatteryTray.Reminder";
const OVERLAY_CLASS: &str = "BatteryTray.Overlay";

thread_local! {
    static NEXT: Cell<Kind> = const { Cell::new(Kind::Normal) };
}

/// Creates a component of the given kind (the attributes hook reads the kind while the component is created).
pub fn create<C>(kind: Kind, new: impl FnOnce() -> Result<C, slint::PlatformError>) -> Result<C, slint::PlatformError> {
    NEXT.with(|n| n.set(kind));
    let created = new();
    NEXT.with(|n| n.set(Kind::Normal));
    created
}

/// The winit backend with Slint's software renderer: no GPU context, little memory, freed with the window.
pub fn init() -> Result<(), slint::PlatformError> {
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_CBT};
    use winit::platform::windows::{CornerPreference, IconExtWindows, WindowAttributesExtWindows};
    // For as long as the app runs, on the thread that creates the windows
    unsafe {
        let _ = SetWindowsHookExW(WH_CBT, Some(on_create), None, GetCurrentThreadId());
    }
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attributes| {
            let icon = winit::window::Icon::from_resource(1, None).ok();
            let popup = |a: winit::window::WindowAttributes, class: &str| {
                a.with_class_name(class)
                    .with_active(false)
                    .with_skip_taskbar(true)
                    .with_corner_preference(CornerPreference::Round)
            };
            match NEXT.with(|n| n.get()) {
                Kind::Normal => attributes
                    .with_theme(Some(title_bar(crate::theme::current().apps_light)))
                    .with_window_icon(icon.clone())
                    .with_taskbar_icon(icon),
                Kind::Reminder => popup(attributes, REMINDER_CLASS),
                // Dark in any mode, as its contents: also the frame Windows 11 draws around a rounded window
                Kind::Overlay => popup(attributes, OVERLAY_CLASS).with_theme(Some(winit::window::Theme::Dark)),
            }
        })
        .select()
}

/// The extended style a popup window class gets: no focus, no taskbar button, layered (unseen until it has an
/// opacity); the overlay also lets the mouse through.
fn popup_bits(hwnd: HWND) -> Option<u32> {
    let mut name = [0u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut name) };
    let class = String::from_utf16_lossy(&name[..n.max(0) as usize]);
    let base = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_LAYERED.0;
    match class.as_str() {
        REMINDER_CLASS => Some(base),
        OVERLAY_CLASS => Some(base | WS_EX_TRANSPARENT.0),
        _ => None,
    }
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
        if let Some(bits) = popup_bits(hwnd) {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | bits as isize);
            let _ = SetWindowSubclass(hwnd, Some(keep_styles), KEEP_STYLES_ID, bits as usize);
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

/// Gives a window the light or the dark colours, at once: the app's palette, and the std widgets' (its scroll bars).
/// Without this a window stays dark, as the overlay does.
pub fn set_theme<C>(ui: &C, light: bool)
where
    C: ComponentHandle,
    for<'a> Palette<'a>: slint::Global<'a, C>,
    for<'a> WidgetPalette<'a>: slint::Global<'a, C>,
{
    use slint::language::ColorScheme;
    let palette = ui.global::<Palette>();
    palette.set_light(light);
    palette.set_mix(mix_of(light));
    ui.global::<WidgetPalette>().set_color_scheme(if light { ColorScheme::Light } else { ColorScheme::Dark });
}

/// Has an open window change to the light or the dark colours over the time Windows takes for its title bar
/// (`title_bar_fade`), so that the two change together; at once where Windows has no such fade.
pub fn fade_theme<C>(ui: &C, light: bool)
where
    C: ComponentHandle + 'static,
    for<'a> Palette<'a>: slint::Global<'a, C>,
    for<'a> WidgetPalette<'a>: slint::Global<'a, C>,
{
    use slint::language::ColorScheme;
    let fade = title_bar_fade();
    if fade.is_zero() {
        return set_theme(ui, light);
    }
    let palette = ui.global::<Palette>();
    if palette.get_light() == light {
        // In these colours already, or on its way to them
        return;
    }
    palette.set_light(light);
    ui.global::<WidgetPalette>().set_color_scheme(if light { ColorScheme::Light } else { ColorScheme::Dark });
    fade_on(ui.as_weak(), light, palette.get_mix(), std::time::Instant::now(), fade);
}

/// The next step of a window's way from `from` to the colours of `light`, and the ones after it. A change the other
/// way made meanwhile ends it: that one has steps of its own, from where this one got to.
fn fade_on<C>(ui: slint::Weak<C>, light: bool, from: f32, began: std::time::Instant, fade: std::time::Duration)
where
    C: ComponentHandle + 'static,
    for<'a> Palette<'a>: slint::Global<'a, C>,
{
    slint::Timer::single_shot(FADE_STEP, move || {
        let Some(window) = ui.upgrade() else { return };
        let palette = window.global::<Palette>();
        if palette.get_light() != light {
            return;
        }
        let done = fade_done(began.elapsed(), fade);
        palette.set_mix(from + (mix_of(light) - from) * done);
        if done < 1.0 {
            fade_on(ui, light, from, began, fade);
        }
    });
}

/// Between two steps of a fade: a frame of a 240 Hz screen.
const FADE_STEP: std::time::Duration = std::time::Duration::from_millis(4);

/// `Palette.mix` of a window in the light or the dark colours.
fn mix_of(light: bool) -> f32 {
    if light {
        1.0
    } else {
        0.0
    }
}

/// How much of a fade is done `elapsed` after the title bar was told to change, from 0 to 1: nothing until the
/// title bar starts on its own, then evenly.
fn fade_done(elapsed: std::time::Duration, fade: std::time::Duration) -> f32 {
    (elapsed.saturating_sub(TITLE_BAR_LAG).as_secs_f32() / fade.as_secs_f32()).clamp(0.0, 1.0)
}

/// From telling Windows of a title bar's new mode to the first frame its fade shows in (measured as `title_bar_fade`).
const TITLE_BAR_LAG: std::time::Duration = std::time::Duration::from_millis(10);

/// How long Windows takes to recolour a title bar when its window goes from dark to light or back. Windows 11 fades
/// its background, evenly (measured on 25H2 at 240 Hz: for 250 ms, the text changing at once); Windows 10 does not.
pub fn title_bar_fade() -> std::time::Duration {
    std::time::Duration::from_millis(if crate::win::windows_build() >= 22000 { 250 } else { 0 })
}

fn title_bar(light: bool) -> winit::window::Theme {
    if light {
        winit::window::Theme::Light
    } else {
        winit::window::Theme::Dark
    }
}

/// Switches an open window's title bar to the light or the dark mode. Its frame is drawn again right away: the new
/// colours could otherwise wait for the window's next activation.
/// Windows 11 then fades the caption to them by itself, and is left to: the window's own colours take the same time
/// (`fade_theme`). Windows 10 repaints the caption's colours only on an activation change, so there the caption is
/// drawn in its other state and back (as Tauri's tao does), without the window's own handler seeing it. On Windows 11
/// that redraw cuts the fade short, and the caption then lands a frame after the window's colours - a light window
/// under a dark title bar for that frame - and at times in the other state's colours.
pub fn set_title_bar(window: &slint::Window, light: bool) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, SWP_FRAMECHANGED, SWP_NOZORDER, WM_NCACTIVATE};
    window.with_winit_window(|w| w.set_theme(Some(title_bar(light))));
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags);
            if title_bar_fade().is_zero() {
                // The window in front, not the thread's active one: a window opened behind another app is that too,
                // and would be left with an active caption
                let active = GetForegroundWindow() == hwnd;
                let _ = DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(usize::from(!active)), LPARAM(0));
                let _ = DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(usize::from(active)), LPARAM(0));
            }
        }
    }
}

/// Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets on Windows 10.
pub(crate) fn icon_font() -> &'static str {
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

/// Puts the popup's extended style bits (in `bits`) back into every new extended style before it is applied.
unsafe extern "system" fn keep_styles(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    bits: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{STYLESTRUCT, WM_NCDESTROY, WM_STYLECHANGING};
    match msg {
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew |= bits as u32;
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(keep_styles), KEEP_STYLES_ID);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_fade_waits_for_the_title_bar_then_goes_evenly_to_its_end() {
        let fade = Duration::from_millis(250);
        let at = |ms| fade_done(Duration::from_millis(ms), fade);
        assert_eq!(at(0), 0.0);
        assert_eq!(at(10), 0.0);
        assert!((at(135) - 0.5).abs() < 1e-6);
        assert_eq!(at(260), 1.0);
        assert_eq!(at(5000), 1.0);
    }

    #[test]
    fn a_window_in_its_colours_is_at_one_end_of_the_mix() {
        assert_eq!((mix_of(false), mix_of(true)), (0.0, 1.0));
    }
}
