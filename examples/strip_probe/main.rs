//! The Windows app's taskbar strip, which only exists on Windows (macOS has the menu bar item): see imp.rs.

#[cfg(windows)]
mod imp;

fn main() {
    #[cfg(windows)]
    imp::main();
    #[cfg(not(windows))]
    eprintln!("strip_probe works with the Windows taskbar; it runs on Windows");
}
