//! Gives BatteryTray.exe what the C# exe had: the app icon, the manifest (asInvoker, per-monitor DPI) and a version
//! resource. The version resource is not cosmetic: the C# updater installs a downloaded exe only when its FileVersion
//! equals the release's version, so this is what lets installed C# copies update themselves to the Rust app.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // The macOS core has neither: its app is built from macos/ (see docs/development.md)
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    windows_exe();
}

fn windows_exe() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = root.join("app.ico");
    let manifest = root.join("app.manifest");
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed={}", manifest.display());

    let v = |name: &str| env::var(name).unwrap();
    let (major, minor, patch) =
        (v("CARGO_PKG_VERSION_MAJOR"), v("CARGO_PKG_VERSION_MINOR"), v("CARGO_PKG_VERSION_PATCH"));
    let path = |p: &PathBuf| p.display().to_string().replace('\\', "/");
    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0
FILEOS 0x40004
FILETYPE 1
FILESUBTYPE 0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "BatteryTray"
      VALUE "FileDescription", "BatteryTray"
      VALUE "FileVersion", "{major}.{minor}.{patch}.0"
      VALUE "InternalName", "BatteryTray"
      VALUE "LegalCopyright", "MIT License"
      VALUE "OriginalFilename", "BatteryTray.exe"
      VALUE "ProductName", "BatteryTray"
      VALUE "ProductVersion", "{major}.{minor}.{patch}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = path(&icon),
        manifest = path(&manifest)
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app.rc");
    fs::write(&out, rc).unwrap();
    embed_resource::compile_for(&out, ["BatteryTray"], embed_resource::NONE).manifest_required().unwrap();
    // The examples stand in for the app, so they get the same: without the manifest Windows takes an exe for an app
    // of before Windows 8, which may not have a layered child window - what the taskbar strip is
    embed_resource::compile_for_examples(&out, embed_resource::NONE).manifest_required().unwrap();

    // The windows; the std widgets used (the scroll view) in their Windows 11 dark style
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).unwrap();
}
