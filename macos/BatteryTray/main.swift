import AppKit

// BatteryTray for macOS: the battery levels of wireless headsets, mice, keyboards and gamepads in the menu bar.
// The switches are the Windows app's where they mean the same:
//   --background   started at login: no window
//   --moved [--autostart]   started by the copy that moved itself into Applications (and turns on the login item)
//   --update       install the latest release over this app; exit 0 = updated, 2 = up to date, 1 = failed
//   --dump [file]  poll every device once and write what was read
//   --demo         the devices of the README's pictures instead of the real ones, with settings of their own
//   --snapshot <folder> [ru|en|zh] [light|dark] [frames]   with the demo's devices, draw each window to a PNG file
//                  and quit; `frames` adds the README's animations (macos/readme-pictures.sh)

let arguments = Array(CommandLine.arguments.dropFirst())

bt_init { DispatchQueue.main.async { AppDelegate.shared?.coreHasNews() } }

switch arguments.first {
case "--update":
    exit(Int32(Core.call(["cmd": "update_now"])["code"] as? Int ?? 1))
case "--dump":
    var command: [String: Any] = ["cmd": "dump"]
    if arguments.count > 1 {
        command["file"] = arguments[1]
    }
    exit(Int32(Core.call(command)["code"] as? Int ?? 1))
default:
    break
}

let background = arguments.contains("--background")
let snapshot = arguments.firstIndex(of: "--snapshot").flatMap { at in
    arguments.indices.contains(at + 1) ? (arguments[at + 1], arguments.indices.contains(at + 2) ? arguments[at + 2] : nil) : nil
}

// One copy at a time. A second launch asks the running copy to open its window, unless it is the login start. Drawing
// the snapshots isn't a launch: it has the demo's devices and settings, and runs beside the app
if snapshot == nil, let id = Bundle.main.bundleIdentifier {
    var others: [NSRunningApplication] {
        NSRunningApplication.runningApplications(withBundleIdentifier: id)
            .filter { $0.processIdentifier != getpid() && !$0.isTerminated }
    }
    // Started by the copy that moved itself into Applications: that one is on its way out
    if arguments.contains("--moved") {
        for _ in 0..<100 where !others.isEmpty {
            Thread.sleep(forTimeInterval: 0.1)
        }
    }
    if let running = others.first {
        if !background {
            DistributedNotificationCenter.default().postNotificationName(
                AppDelegate.openNotification, object: nil, userInfo: nil, deliverImmediately: true)
            running.activate()
        }
        exit(0)
    }
}

// The process starts on the main thread, which is the app's
MainActor.assumeIsolated {
    let app = NSApplication.shared
    let delegate = AppDelegate(background: background || snapshot != nil, demo: arguments.contains("--demo") || snapshot != nil)
    delegate.snapshot = snapshot
    app.delegate = delegate
    if snapshot != nil, let at = arguments.firstIndex(of: "--snapshot"), arguments.indices.contains(at + 3) {
        app.appearance = NSAppearance(named: arguments[at + 3] == "light" ? .aqua : .darkAqua)
    }
    app.setActivationPolicy(.accessory)
    // The tooltips of the icon buttons (⋯, the cross, the shortcut's reset) half a second after the pointer comes, as
    // on Windows, not after macOS' second or two
    UserDefaults.standard.register(defaults: ["NSInitialToolTipDelay": 500])
    app.run()
}
