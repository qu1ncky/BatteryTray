//! Renders the windows with sample data to PNG files, off screen, with the same software renderer the app uses:
//! `cargo run --example snapshot -- <folder> [ru|en|zh] [scale]`. For checking the layout without running the app.
//! Each picture of the main window is a window of its own, so nothing opened or typed for one shows in the next. Its
//! devices go through the app's own code, in the language asked for.

use batterytray::gui::{self, AlertRow, AlertWindow, MainWindow, OverlayEntry, OverlayWindow};
use batterytray::hotkey::{self, Gesture};
use batterytray::loc::{self, t, tf};
use batterytray::main_view;
use batterytray::reading::{DeviceKind, DeviceReading};
use batterytray::rows::{self, DeviceRow};
use batterytray::settings::{AppSettings, DEFAULT_HOTKEY};
use batterytray::updater::Version;
use chrono::{DateTime, Local, TimeDelta};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    static LAST: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
    /// The time animations and timers go by: moved on by hand before each drawing, so whatever started has ended
    static CLOCK: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

struct Offscreen;

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        LAST.with(|l| *l.borrow_mut() = Some(window.clone()));
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        CLOCK.with(Cell::get)
    }
}

fn last() -> Rc<MinimalSoftwareWindow> {
    LAST.with(|l| l.borrow().clone().unwrap())
}

/// Lays the window out at its size and draws it once, without keeping the picture: its rows exist afterwards.
fn lay_out(window: &MinimalSoftwareWindow, scale: f32, logical: (f32, f32)) -> (u32, u32) {
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    let (w, h) = ((logical.0 * scale) as u32, (logical.1 * scale) as u32);
    window.set_size(slint::PhysicalSize::new(w, h));
    draw(window, w, h);
    (w, h)
}

/// A second on, the timers, animations and change handlers that are due, then a full drawing.
fn draw(window: &MinimalSoftwareWindow, w: u32, h: u32) -> Vec<PremultipliedRgbaColor> {
    CLOCK.with(|c| c.set(c.get() + Duration::from_secs(1)));
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    let mut buffer = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, w as usize);
    });
    buffer
}

/// Drawn twice: the first drawing creates the rows, and what they tell the window (where an open menu goes) is in
/// the second.
fn render(window: &MinimalSoftwareWindow, scale: f32, logical: (f32, f32), path: &Path) {
    let (w, h) = lay_out(window, scale, logical);
    let buffer = draw(window, w, h);
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let bytes: Vec<u8> = buffer.iter().flat_map(|p| [p.red, p.green, p.blue, 255]).collect();
    writer.write_image_data(&bytes).unwrap();
    println!("{}", path.display());
}

fn rgb(r: u8, g: u8, b: u8) -> slint::Color {
    slint::Color::from_rgb_u8(r, g, b)
}

const BARRACUDA: &str = "razer:barracuda-x";
const XBOX: &str = "xinput:xbox-wireless-controller";
const ATTACK_SHARK: &str = "rongyuan:attack-shark-r3";
const RAINY: &str = "sonix:rainy-75";
const MX_KEYS: &str = "logitech:mx-keys";
const BARRACUDA_BT: &str = "bt:barracuda-x";

/// The devices of the mockups: two pinned (one of them away, with its own levels on the other), two connected (one
/// charging without a level, one estimated and low), one not connected, one hidden.
fn sample_rows(settings: &AppSettings, now: DateTime<Local>) -> Vec<DeviceRow> {
    let seen = now - TimeDelta::seconds(7);
    let reading = |id: &str, name: &str, kind: DeviceKind, via: String, level: Option<i32>| DeviceReading {
        online: true,
        level,
        time: seen,
        ..DeviceReading::new(id, name, kind, via)
    };
    let current = vec![
        reading(BARRACUDA, "Razer Barracuda X", DeviceKind::Headset, t("via.24"), Some(37)),
        DeviceReading {
            charging: true,
            detail: Some(tf("d.wasBefore", &[&47])),
            ..reading(ATTACK_SHARK, "Attack Shark R3", DeviceKind::Mouse, t("via.cable"), None)
        },
        DeviceReading {
            estimate: true,
            detail: Some(t("d.estimate")),
            ..reading(RAINY, "Rainy 75", DeviceKind::Keyboard, t("via.24"), Some(15))
        },
        reading(BARRACUDA_BT, "Razer Barracuda X (BT)", DeviceKind::Headset, "Bluetooth".into(), Some(70)),
    ];
    let away = |id: &str, name: &str, kind: DeviceKind, via: String, level: i32, time: DateTime<Local>| {
        let last = DeviceReading { level: Some(level), time, ..DeviceReading::new(id, name, kind, via) };
        (id.to_string(), Some(last))
    };
    let remembered = [
        away(XBOX, "Xbox Wireless Controller", DeviceKind::Gamepad, t("via.adapter"), 80, now - TimeDelta::days(3)),
        away(MX_KEYS, "Logitech MX Keys", DeviceKind::Keyboard, t("via.receiver"), 64, now - TimeDelta::hours(2)),
    ];
    rows::rows(current, &remembered, settings)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let folder = std::path::PathBuf::from(args.first().cloned().unwrap_or_else(|| ".".into()));
    let language = args.get(1).map(String::as_str).unwrap_or("ru");
    loc::set(language);
    let scale: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.25);
    slint::platform::set_platform(Box::new(Offscreen)).unwrap();

    let mut settings = AppSettings::default();
    settings.device_mut(BARRACUDA).pinned = true;
    settings.device_mut(BARRACUDA).low_percent = Some(30);
    settings.device_mut(BARRACUDA).critical_percent = Some(15);
    settings.device_mut(XBOX).pinned = true;
    settings.device_mut(RAINY).alerts = false;
    settings.device_mut(BARRACUDA_BT).hidden = true;
    let now = Local::now()
        .date_naive()
        .and_hms_opt(15, 41, 0)
        .and_then(|t| t.and_local_timezone(Local).earliest())
        .unwrap_or_else(Local::now);
    let rows = sample_rows(&settings, now);
    let version = Version::current();
    let latest = Version(version.0, version.1, version.2 + 1);
    let shortcut = Gesture { key: DEFAULT_HOTKEY.0, modifiers: DEFAULT_HOTKEY.1 };
    let size = (440.0, 640.0);

    // A new window with everything set as the app sets it: the devices, an update known, the settings
    let open = |rows: &[DeviceRow]| {
        let main = MainWindow::new().unwrap();
        let window = last();
        gui::setup_texts(&main.global::<gui::T>());
        let lists = main_view::device_lists(rows, &settings, now);
        main.set_pinned_devices(ModelRc::from(lists.pinned.as_slice()));
        main.set_connected_devices(ModelRc::from(lists.connected.as_slice()));
        main.set_offline_devices(ModelRc::from(lists.offline.as_slice()));
        main.set_hidden_devices(ModelRc::from(lists.hidden.as_slice()));
        main.set_none_online(lists.none_online);
        let newest = rows.iter().filter_map(|r| r.current.as_ref().map(|c| c.time)).max();
        main.set_updated_text(
            match newest {
                Some(time) => tf("updated.at", &[&time.format("%H:%M:%S")]),
                None => t("polling"),
            }
            .into(),
        );
        main.set_version_text(format!("BatteryTray {version}").into());
        main.set_banner_visible(true);
        main.set_banner_text(tf("upd.banner", &[&latest]).into());
        main.set_banner_button_visible(true);
        main.set_autostart(true);

        main.set_language(language.into());
        main.set_elevation_text(t("elev.portable").into());
        main.set_update_status(tf("upd.uptodate", &[&version]).into());
        main.set_update_checked_text(tf("upd.checked", &[&now.format("%H:%M")]).into());
        main.set_install_text(tf("upd.install", &[&latest]).into());
        main.set_auto_update(settings.auto_update);
        main.set_low(settings.low_percent);
        main.set_critical(settings.critical_percent);
        main.set_repeat(settings.repeat_minutes);
        main.set_sound(settings.sound);
        main.set_notify_charged(settings.notify_charged);
        main.set_overlay_enabled(true);
        let keys: Vec<SharedString> = hotkey::parts(shortcut).into_iter().map(SharedString::from).collect();
        main.set_shortcut_keys(ModelRc::new(VecModel::from(keys)));
        main.set_shortcut_text(hotkey::label(shortcut).into());
        main.set_overlay_mode(settings.overlay_mode);
        main.set_overlay_corner(settings.overlay_corner);
        main.set_overlay_names(settings.overlay_show_names);
        (main, window)
    };

    let (_main, window) = open(&rows);
    render(&window, scale, size, &folder.join("main-devices.png"));

    // The README's picture: the devices with no update banner
    let (main, window) = open(&rows);
    main.set_banner_visible(false);
    render(&window, scale, size, &folder.join("readme.png"));

    // The menu of a pinned device with its own levels: the levels and "back to the general ones" are open
    let (main, window) = open(&rows);
    lay_out(&window, scale, size);
    main.invoke_open_menu(BARRACUDA.into());
    render(&window, scale, size, &folder.join("main-devices-menu.png"));

    // The menu of a device that is not pinned and not reminded of: "Pin", and nothing below the switch
    let (main, window) = open(&rows);
    lay_out(&window, scale, size);
    main.invoke_open_menu(RAINY.into());
    render(&window, scale, size, &folder.join("main-devices-menu-small.png"));

    let (main, window) = open(&rows);
    lay_out(&window, scale, size);
    main.invoke_start_rename(BARRACUDA.into());
    render(&window, scale, size, &folder.join("main-devices-rename.png"));

    let (main, window) = open(&rows);
    main.set_hidden_open(true);
    render(&window, scale, size, &folder.join("main-devices-hidden.png"));

    // The first start on a PC without devices, autostart off
    let (main, window) = open(&[]);
    main.set_banner_visible(false);
    main.set_autostart(false);
    render(&window, scale, size, &folder.join("main-devices-empty.png"));

    let (main, window) = open(&rows);
    main.set_screen(1);
    main.set_settings_tab(0);
    render(&window, scale, size, &folder.join("main-settings-general.png"));

    let (main, window) = open(&rows);
    main.set_screen(1);
    main.set_settings_tab(0);
    main.set_update_status(tf("upd.available", &[&latest, &version]).into());
    main.set_update_checked_text("".into());
    main.set_update_notes(
        "Pinned devices, your own order and a menu for each device.\nSettings in three tabs, with a hint on every \
         switch that needs one."
            .into(),
    );
    main.set_install_visible(true);
    render(&window, scale, size, &folder.join("main-settings-update.png"));

    let (main, window) = open(&rows);
    main.set_screen(1);
    main.set_settings_tab(1);
    render(&window, scale, size, &folder.join("main-settings-reminders.png"));

    let (main, window) = open(&rows);
    main.set_screen(1);
    main.set_settings_tab(2);
    main.set_shortcut_error(t("overlay.conflict").into());
    render(&window, scale, size, &folder.join("main-settings-overlay.png"));

    let alert = AlertWindow::new().unwrap();
    let window = last();
    gui::setup_texts(&alert.global::<gui::T>());
    alert.set_items(ModelRc::from(Rc::new(VecModel::from(vec![
        AlertRow { glyph: "\u{E7F6}".into(), name: "Razer Barracuda X".into(), level: "18%".into(), critical: false },
        AlertRow { glyph: "\u{E962}".into(), name: "Attack Shark R3".into(), level: "9%".into(), critical: true },
    ]))));
    alert.set_critical(true);
    alert.set_title_text(loc::t("alert.title.critical").into());
    alert.set_snooze_text("Напомнить через 10 мин".into());
    alert.set_hint_text(loc::t("alert.hint.critical").into());
    let height = alert.window().size().height as f32 / alert.window().scale_factor();
    render(&window, scale, (376.0, if height > 10.0 { height } else { 300.0 }), &folder.join("alert.png"));

    let overlay = OverlayWindow::new().unwrap();
    let window = last();
    gui::setup_texts(&overlay.global::<gui::T>());
    let entries = vec![
        OverlayEntry {
            glyph: "\u{E7F6}".into(),
            name: "Razer Barracuda X".into(),
            show_name: false,
            level: "37%".into(),
            color: rgb(0x6C, 0xCB, 0x5F),
            power: false,
        },
        OverlayEntry {
            glyph: "\u{E962}".into(),
            name: "Attack Shark R3".into(),
            show_name: true,
            level: "".into(),
            color: rgb(0x60, 0xCD, 0xFF),
            power: true,
        },
        OverlayEntry {
            glyph: "\u{E962}".into(),
            name: "VXE R1".into(),
            show_name: true,
            level: "82%".into(),
            color: rgb(0x6C, 0xCB, 0x5F),
            power: false,
        },
    ];
    let rows = |entries: Vec<OverlayEntry>, limit: f32| {
        ModelRc::from(Rc::new(VecModel::from(batterytray::overlay::into_rows(entries, limit))))
    };
    overlay.set_rows(rows(entries.clone(), batterytray::overlay::row_limit(1920.0)));
    render(&window, scale, (420.0, 44.0), &folder.join("overlay.png"));

    // Many devices with their names, on a 1280 px wide screen: the list goes on in the next row
    let many: Vec<OverlayEntry> = (0..7)
        .map(|i| OverlayEntry {
            name: ["Razer Barracuda X", "Attack Shark R3", "VXE R1 Pro Max", "Клавиатура AULA F75"][i % 4].into(),
            show_name: true,
            level: ["37%", "100%", "≈60%", "8%"][i % 4].into(),
            ..entries[i % 3].clone()
        })
        .collect();
    overlay.set_rows(rows(many, batterytray::overlay::row_limit(1280.0)));
    // Drawn once for the rows to exist (off screen they only do once drawn), then at the size they ask for
    render(&window, scale, (640.0, 200.0), &folder.join("overlay-rows.png"));
    let size = (overlay.get_content_width(), overlay.get_content_height());
    render(&window, scale, size, &folder.join("overlay-rows.png"));
}
