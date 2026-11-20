//! The overlay's global shortcut. RegisterHotKey on the app's
//! hidden window, which receives WM_HOTKEY even when every visible window is closed. A new shortcut is registered
//! before the old one is released, so an occupied combination never costs the working one.

use crate::settings::{hotkey_valid, MOD_ALT, MOD_CONTROL, MOD_SHIFT};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyNameTextW, GetKeyState, MapVirtualKeyW, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
    MAPVK_VK_TO_VSC_EX, MAPVK_VSC_TO_VK_EX, MOD_NOREPEAT,
};

/// Win32 virtual key and modifiers (MOD_ALT, MOD_CONTROL, MOD_SHIFT; 8 = a Windows key, never valid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gesture {
    pub key: i32,
    pub modifiers: i32,
}

impl Gesture {
    pub fn is_valid(&self) -> bool {
        hotkey_valid(self.key, self.modifiers)
    }
}

pub trait Registration {
    fn register(&mut self, id: i32, gesture: Gesture) -> bool;
    fn unregister(&mut self, id: i32);
}

pub struct Binding<R: Registration> {
    registration: R,
    active_id: i32,
    pub active: Option<Gesture>,
}

impl<R: Registration> Binding<R> {
    pub fn new(registration: R) -> Self {
        Self { registration, active_id: 0, active: None }
    }

    /// True when a WM_HOTKEY with this id is the current shortcut (not one queued before a change).
    pub fn is_current(&self, id: i32) -> bool {
        self.active.is_some() && id == self.active_id
    }

    pub fn try_change(&mut self, gesture: Gesture) -> bool {
        if !gesture.is_valid() {
            return false;
        }
        if self.active == Some(gesture) {
            return true;
        }
        let next = if self.active_id == 1 { 2 } else { 1 };
        if !self.registration.register(next, gesture) {
            return false;
        }
        if self.active.is_some() {
            self.registration.unregister(self.active_id);
        }
        self.active_id = next;
        self.active = Some(gesture);
        true
    }

    pub fn clear(&mut self) {
        if self.active.is_some() {
            self.registration.unregister(self.active_id);
        }
        self.active = None;
    }
}

impl<R: Registration> Drop for Binding<R> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// RegisterHotKey on a window of this thread.
pub struct WindowHotkeys(pub HWND);

impl Registration for WindowHotkeys {
    fn register(&mut self, id: i32, gesture: Gesture) -> bool {
        let modifiers = HOT_KEY_MODIFIERS(gesture.modifiers as u32) | MOD_NOREPEAT;
        unsafe { RegisterHotKey(Some(self.0), id, modifiers, gesture.key as u32).is_ok() }
    }

    fn unregister(&mut self, id: i32) {
        unsafe {
            let _ = UnregisterHotKey(Some(self.0), id);
        }
    }
}

/// The virtual key of a key's scan code (as winit reports it, with 0xE0 in front of the extended keys), so a shortcut
/// is recorded the same on every keyboard layout.
pub fn virtual_key(scancode: u32) -> u32 {
    unsafe { MapVirtualKeyW(scancode, MAPVK_VSC_TO_VK_EX) }
}

/// The modifiers held right now: Alt, Ctrl, Shift, and 8 for a Windows key.
pub fn modifiers_down() -> i32 {
    let down = |vk: i32| unsafe { GetKeyState(vk) } < 0;
    (if down(0x12) { MOD_ALT } else { 0 })
        | (if down(0x11) { MOD_CONTROL } else { 0 })
        | (if down(0x10) { MOD_SHIFT } else { 0 })
        | (if down(0x5B) || down(0x5C) { 8 } else { 0 })
}

/// Shift, Ctrl, Alt and Windows keys, left or right: what a shortcut needs more than.
pub fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

/// The parts of a shortcut as the keyboard calls them: ["Ctrl", "Alt", "V"]. Key caps are drawn from these: a key's
/// own name may contain "+", so the joined label cannot be split again.
pub fn parts(gesture: Gesture) -> Vec<String> {
    let mut parts = Vec::new();
    if gesture.modifiers & MOD_CONTROL != 0 {
        parts.push("Ctrl".to_string());
    }
    if gesture.modifiers & MOD_ALT != 0 {
        parts.push("Alt".to_string());
    }
    if gesture.modifiers & MOD_SHIFT != 0 {
        parts.push("Shift".to_string());
    }
    parts.push(key_name(gesture.key as u32));
    parts
}

/// "Ctrl+Alt+V", "F5", "Ctrl+Shift+Пробел": as the keyboard calls the key.
pub fn label(gesture: Gesture) -> String {
    parts(gesture).join("+")
}

fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => unsafe {
            let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX);
            // Bit 24 of the lParam layout: an extended key (arrows, Insert, Home... have their own names then)
            let extended = if scan & 0xFF00 == 0xE000 { 1 << 24 } else { 0 };
            let mut buf = [0u16; 64];
            let n = GetKeyNameTextW((((scan & 0xFF) << 16) | extended) as i32, &mut buf);
            if n > 0 {
                String::from_utf16_lossy(&buf[..n as usize])
            } else {
                format!("0x{vk:02X}")
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        fail: bool,
        calls: Vec<String>,
    }

    impl Registration for &mut Fake {
        fn register(&mut self, id: i32, _: Gesture) -> bool {
            self.calls.push(format!("register:{id}"));
            !self.fail
        }
        fn unregister(&mut self, id: i32) {
            self.calls.push(format!("remove:{id}"));
        }
    }

    const DEFAULT: Gesture = Gesture { key: 0x56, modifiers: 3 };

    #[test]
    fn validates_supported_shortcuts() {
        let cases = [
            (0x56, 3, true),
            (0x74, 0, true),
            (0x74, 1, true),
            (0x74, 4, true),
            (0x87, 0, true),
            (0x56, 4, false),
            (0x56, 5, true),
            (0x56, 6, true),
            (0x56, 2, false),
            (0x43, 2, false),
            (0x58, 2, false),
            (0x73, 1, false),
            (0x56, 0, false),
            (0x56, 8, false),
            (0x7B, 3, false),
            (0x11, 3, false),
            (0xA0, 3, false),
            (0x5B, 3, false),
            (0, 3, false),
            (0xFF, 3, false),
            (0x56, -1, false),
        ];
        for (key, modifiers, valid) in cases {
            assert_eq!(Gesture { key, modifiers }.is_valid(), valid, "{key:#x} {modifiers}");
        }
    }

    #[test]
    fn a_conflict_keeps_the_previous_registration_and_invalid_input_never_registers() {
        let mut fake = Fake::default();
        {
            let mut binding = Binding::new(&mut fake);
            assert!(binding.try_change(DEFAULT));
            binding.registration.fail = true;
            assert!(!binding.try_change(Gesture { key: 0x74, modifiers: 0 }));
            assert_eq!(binding.active, Some(DEFAULT));
            let calls = binding.registration.calls.len();
            assert!(!binding.try_change(Gesture { key: 0x7B, modifiers: 3 }));
            assert_eq!(binding.registration.calls.len(), calls);
            binding.registration.fail = false;
        }
        assert!(!fake.calls.iter().take(2).any(|c| c.starts_with("remove")), "{:?}", fake.calls);
    }

    #[test]
    fn replacement_registers_before_releasing_the_old_id() {
        let mut fake = Fake::default();
        {
            let mut binding = Binding::new(&mut fake);
            assert!(binding.try_change(DEFAULT));
            assert!(binding.try_change(Gesture { key: 0x74, modifiers: 0 }));
            assert!(!binding.is_current(1) && binding.is_current(2));
            assert!(binding.try_change(Gesture { key: 0x74, modifiers: 0 }), "the same shortcut again");
            binding.clear();
            binding.clear();
            assert!(!binding.is_current(2));
        }
        assert_eq!(fake.calls, ["register:1", "register:2", "remove:1", "remove:2"]);
    }

    #[test]
    fn the_parts_are_the_label_s() {
        assert_eq!(parts(DEFAULT), ["Ctrl", "Alt", "V"]);
        assert_eq!(label(DEFAULT), "Ctrl+Alt+V");
        assert_eq!(parts(Gesture { key: 0x74, modifiers: 0 }), ["F5"]);
        assert_eq!(label(Gesture { key: 0x74, modifiers: 0 }), "F5");
    }
}
