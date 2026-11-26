//! Puts the taskbar strip on the real taskbar with the demo's devices, without the app around it, and says every
//! second where its window is and whether the mouse would reach it: `cargo run --example strip_probe -- [folder]
//! [seconds] [light|dark]`. With a folder ("-" for none), it first draws the strip to PNG files there, at rest and
//! under the pointer, in both modes of the taskbar and at its scale. The strip stays for 10 seconds unless told
//! otherwise; a click on it and its menu are only printed. For looking at the strip's place, size and colors on a
//! taskbar, and at how it goes with the taskbar (under a full-screen app, in front with Start open) and gives way to
//! the apps' buttons, with nothing installed. While the app itself runs with its strip on, the two are at one place:
//! an app of before the strip became the taskbar's child covers this one, except while Start is open.

use batterytray::main_view::glyph;
use batterytray::reading::DeviceKind;
use batterytray::strip::{self, Content, Slot, Strip};
use batterytray::view::Tone;
use std::path::Path;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowExW, FindWindowW, GetMessageW, GetWindowRect, IsWindowVisible, PostQuitMessage,
    SetTimer, TranslateMessage, WindowFromPoint, MSG, WM_TIMER,
};

fn slot(kind: DeviceKind, level: &str, tone: Tone, power: bool) -> Slot {
    Slot { glyph: glyph(kind), level: level.into(), tone, power }
}

/// The demo's devices: a headset, a mouse that charges, a keyboard below its reminder level with an estimated level.
fn demo(light: bool) -> Content {
    Content {
        slots: vec![
            slot(DeviceKind::Headset, "37", Tone::Good, false),
            slot(DeviceKind::Mouse, "62", Tone::Charge, true),
            slot(DeviceKind::Keyboard, "≈15", Tone::Warn, false),
        ],
        urgent: Some(slot(DeviceKind::Keyboard, "≈15", Tone::Warn, false)),
        tooltip: "Razer Barracuda X: 37%\nAttack Shark R3: 62% ⚡\nRainy 75: ≈15%".into(),
        light,
        active: false,
    }
}

fn save(folder: &Path, name: &str, content: &Content, scale: f32, hover: bool) {
    let Some((w, h, pixels)) = strip::picture(content, scale, hover) else {
        eprintln!("{name}: not drawn");
        return;
    };
    let path = folder.join(name);
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let bytes: Vec<u8> = pixels.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    writer.write_image_data(&bytes).unwrap();
    println!("{} ({w} x {h})", path.display());
}

fn rect_of(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    r
}

pub fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let folder = args.first().filter(|a| !a.is_empty() && *a != "-");
    let seconds: u32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(10);
    let light = args.get(2).is_some_and(|a| a == "light");
    // The scale of the screen and the right to a layered child window come from the app's manifest, which build.rs
    // gives the examples too
    let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.unwrap_or_default();
    let dpi = unsafe { GetDpiForWindow(taskbar) };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    println!("taskbar {:?}, scale {scale}", rect_of(taskbar));
    println!("the apps' buttons end at {:?}", strip::buttons_end());

    if let Some(folder) = folder {
        let folder = Path::new(folder);
        std::fs::create_dir_all(folder).unwrap();
        for (mode, light) in [("dark", false), ("light", true)] {
            save(folder, &format!("strip-{mode}.png"), &demo(light), scale, false);
            save(folder, &format!("strip-{mode}-hover.png"), &demo(light), scale, true);
        }
        let none = Content { light: false, ..Default::default() };
        save(folder, "strip-none.png", &none, scale, false);
        // The same row at 100% and 200%, to see the sizes hold
        save(folder, "strip-dark-100.png", &demo(false), 1.0, true);
        save(folder, "strip-dark-200.png", &demo(false), 2.0, true);
    }
    if seconds == 0 {
        return;
    }

    let Some(strip) = Strip::new(|action| println!("{action:?}")) else {
        eprintln!("the strip's window couldn't be made");
        return;
    };
    strip.set(demo(light));
    unsafe {
        SetTimer(None, 0, 1000, None);
        let mut left = seconds;
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            // The loop's own timer: the strip's go to its window
            if message.message == WM_TIMER && message.hwnd.is_invalid() {
                // The strip is a child of the taskbar
                let window =
                    FindWindowExW(Some(taskbar), None, w!("BatteryTray.Strip"), PCWSTR::null()).unwrap_or_default();
                let tray = FindWindowExW(Some(taskbar), None, w!("TrayNotifyWnd"), PCWSTR::null()).unwrap_or_default();
                let (at, shown) = (rect_of(window), IsWindowVisible(window).as_bool());
                // What the mouse would reach: the strip in its padding, where nothing is drawn, and whatever is
                // under it at its cut corner
                let over = |x: i32, y: i32| WindowFromPoint(POINT { x, y }) == window;
                let middle = (at.top + at.bottom) / 2;
                println!(
                    "strip {}..{} x {}..{}, visible {shown}; tray from {}; takes the mouse in its padding: {}, at its corner: {}",
                    at.left,
                    at.right,
                    at.top,
                    at.bottom,
                    rect_of(tray).left,
                    over(at.left + 4, middle),
                    over(at.left, at.top)
                );
                left -= 1;
                if left == 0 {
                    PostQuitMessage(0);
                }
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    drop(strip);
}
