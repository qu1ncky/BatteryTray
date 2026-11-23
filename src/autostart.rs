//! Starts the app at logon through a Task Scheduler task. When installed in Program Files the
//! task runs it with highest privileges: on some PCs a third-party driver makes a dongle's vendor HID collection
//! admin-only (e.g. the G-SPY driver vkbms.inf claims every FF00:0001 collection, the Razer one included), and a
//! logon task gets admin rights without a UAC prompt. Anywhere else the task runs with normal rights: an elevated
//! task must never start an exe that any program could replace. Same task, name and switches as the C# app, so an
//! install that moves from one to the other keeps working.
//! A copy started from anywhere else installs itself when the user grants it admin rights in the window (install()),
//! as install.cmd does: the logon task then starts the copy in Program Files with admin rights.
//! The task is read and written through the Task Scheduler's own interface, not schtasks.exe: the elevated app never
//! runs an exe found through the environment, and the task's XML never passes through a file another program could
//! change between writing and reading it.

use crate::log;
use crate::win::{self, wide};
use std::path::{Path, PathBuf};
use windows::core::{Interface, BSTR, GUID, PCWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::TaskScheduler::{
    ITaskFolder, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_ProgramFiles, IShellLinkW, SHGetKnownFolderPath, ShellExecuteExW, ShellLink,
    KF_FLAG_DEFAULT, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NOZONECHECKS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const TASK_NAME: &str = "BatteryTray";
/// Passed by the task: start in the tray without opening the window.
pub const BACKGROUND_SWITCH: &str = "--background";

fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        text.map(PathBuf::from)
    }
}

/// True when the path is in Program Files, which only administrators can write to.
fn in_program_files(path: &Path) -> bool {
    let Some(pf) = known_folder(&FOLDERID_ProgramFiles) else { return false };
    let path = path.to_string_lossy().to_lowercase();
    path.starts_with(&format!("{}\\", pf.to_string_lossy().to_lowercase()))
}

/// True when this exe sits in Program Files.
pub fn is_installed() -> bool {
    in_program_files(&exe())
}

/// Where install() puts the exe: the same place as install.cmd.
pub fn installed_exe() -> Option<PathBuf> {
    Some(known_folder(&FOLDERID_ProgramFiles)?.join("BatteryTray").join("BatteryTray.exe"))
}

/// Installs this copy as install.cmd does: the exe into Program Files, the logon task that starts it from there with
/// admin rights, a Start menu shortcut. Without admin rights an elevated copy of this exe does it (UAC prompt). False
/// when the prompt was declined or the install failed.
pub fn install() -> bool {
    if win::is_elevated() {
        install_here()
    } else {
        run_self_elevated("--install")
    }
}

/// install()'s work, in a process with admin rights.
pub fn install_here() -> bool {
    let (source, Some(target)) = (exe(), installed_exe()) else { return false };
    if source != target {
        let copied =
            target.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::copy(&source, &target));
        if let Err(e) = copied {
            log::write(&format!("install to {}: {e}", target.display()));
            return false;
        }
        // A downloaded exe carries the "from the internet" mark, and the copy keeps it
        let _ = std::fs::remove_file(format!("{}:Zone.Identifier", target.display()));
        log::write(&format!("installed to {}", target.display()));
    }
    if !register_exe(&target) {
        return false;
    }
    // Only a way to open the window: the app runs without it
    if let Err(e) = add_shortcut(&target) {
        log::write(&format!("Start menu shortcut: {}", e.message()));
    }
    true
}

/// The Start menu entry install.cmd makes (uninstall.cmd removes it): launching the exe opens the window of the copy
/// running in the tray.
fn add_shortcut(exe: &Path) -> windows::core::Result<()> {
    let folder = known_folder(&FOLDERID_CommonPrograms).ok_or_else(windows::core::Error::empty)?;
    let lnk = wide(&folder.join("BatteryTray.lnk").display().to_string());
    let target = wide(&exe.display().to_string());
    let dir = wide(&exe.parent().map(|d| d.display().to_string()).unwrap_or_default());
    let description = wide("Battery level of wireless devices");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(PCWSTR(target.as_ptr()))?;
        link.SetWorkingDirectory(PCWSTR(dir.as_ptr()))?;
        link.SetDescription(PCWSTR(description.as_ptr()))?;
        link.cast::<IPersistFile>()?.Save(PCWSTR(lnk.as_ptr()), true)
    }
}

/// The Task Scheduler's root folder, where the task lives.
fn tasks() -> windows::core::Result<ITaskFolder> {
    unsafe {
        // Single-threaded, as the UI thread already is (winit sets it up for drag and drop); elsewhere this sets it up
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let local = VARIANT::default();
        service.Connect(&local, &local, &local, &local)?;
        service.GetFolder(&BSTR::from("\\"))
    }
}

pub fn is_registered() -> bool {
    tasks().and_then(|f| unsafe { f.GetTask(&BSTR::from(TASK_NAME)) }).is_ok()
}

/// Starts the task now. The user owns it, so this needs no elevation and shows no UAC prompt, yet the app it starts
/// runs with admin rights.
pub fn run_task() -> bool {
    tasks().and_then(|f| unsafe { f.GetTask(&BSTR::from(TASK_NAME))?.Run(&VARIANT::default()) }).is_ok()
}

/// Registers or removes the task. An elevated task needs admin rights to create, so outside an elevated process
/// that asks for them (UAC); a normal-rights task for a portable copy doesn't.
pub fn set(enabled: bool) -> bool {
    let apply = || if enabled { register() } else { unregister() };
    if win::is_elevated() {
        return apply();
    }
    if !is_installed() && apply() {
        return true;
    }
    run_self_elevated(if enabled { "--register-task" } else { "--unregister-task" })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

pub fn register() -> bool {
    register_exe(&exe())
}

/// The task for the given exe: with admin rights only when it sits in Program Files.
fn register_exe(exe: &Path) -> bool {
    let user = user_name();
    let run_level = if in_program_files(exe) { "HighestAvailable" } else { "LeastPrivilege" };
    let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>BatteryTray: battery levels of wireless devices in the tray, with low battery reminders</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT5S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>{run_level}</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>6</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{BACKGROUND_SWITCH}</Arguments>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>"#,
        user = escape(&user),
        command = escape(&exe.display().to_string()),
        dir = escape(&dir)
    );

    // Created or replaced, as schtasks /Create /F did; the XML says who it runs as
    let none = VARIANT::default();
    let result = tasks().and_then(|f| unsafe {
        f.RegisterTask(
            &BSTR::from(TASK_NAME),
            &BSTR::from(xml),
            TASK_CREATE_OR_UPDATE.0,
            &none,
            &none,
            TASK_LOGON_INTERACTIVE_TOKEN,
            &none,
        )
    });
    log::write(&format!("register logon task: {}", result.as_ref().map_or_else(|e| e.message(), |_| "ok".into())));
    result.is_ok()
}

pub fn unregister() -> bool {
    let result = tasks().and_then(|f| unsafe { f.DeleteTask(&BSTR::from(TASK_NAME), 0) });
    log::write(&format!("unregister logon task: {}", result.as_ref().map_or_else(|e| e.message(), |_| "ok".into())));
    result.is_ok()
}

/// Runs this exe with admin rights (UAC prompt) and the given switch; true when it exits with 0. Without the zone
/// check: a downloaded exe carries the "from the internet" mark, and SmartScreen would stop its elevated copy with
/// another "Windows protected your PC" although the user has already let this very file run.
pub fn run_self_elevated(argument: &str) -> bool {
    let file = wide(&exe().display().to_string());
    let params = wide(argument);
    let verb = wide("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOZONECHECKS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        if ShellExecuteExW(&mut info).is_err() || info.hProcess.is_invalid() {
            return false; // the user declined the UAC prompt
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        let got = GetExitCodeProcess(info.hProcess, &mut code).is_ok();
        let _ = CloseHandle(info.hProcess);
        got && code == 0
    }
}

/// DOMAIN\user of this process, as .NET's WindowsIdentity.Name.
fn user_name() -> String {
    const NAME_SAM_COMPATIBLE: i32 = 2;
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    if unsafe { GetUserNameExW(NAME_SAM_COMPATIBLE, buf.as_mut_ptr(), &mut len) } != 0 {
        return win::from_wide(&buf);
    }
    format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default())
}

#[link(name = "secur32", kind = "raw-dylib")]
extern "system" {
    fn GetUserNameExW(format: i32, buffer: *mut u16, size: *mut u32) -> u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only reads: connects to the Task Scheduler and looks the task up (it may or may not be registered here).
    #[test]
    fn the_task_scheduler_answers() {
        assert!(tasks().is_ok());
        println!("logon task registered: {}", is_registered());
    }
}
