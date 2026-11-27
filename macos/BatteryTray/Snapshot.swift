import AppKit

/// `--snapshot <folder> [ru|en|zh] [light|dark]`: the app with the demo's devices draws each of its windows to a PNG
/// file and quits - the device list, a device's menu, a name being edited, the menu bar item and its menu, each settings
/// tab, the reminder and the overlay - in that appearance (the system's without it), for the README's pictures and for
/// checking a layout. An app may always take pictures of its own windows, so no Screen Recording permission is needed.
@MainActor
final class Snapshots {
    private let folder: URL
    private let language: String?
    private weak var app: AppDelegate?
    private var steps: [() -> Void] = []

    init(folder: String, language: String?, app: AppDelegate) {
        self.folder = URL(fileURLWithPath: folder)
        self.language = language
        self.app = app
    }

    func run() {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let model = AppModel.shared
        // A reminder that closes on its own, with 40% of its time gone, as the Windows pictures have it
        ReminderController.pictureRemaining = 0.6
        steps = [
            { if let language = self.language { model.send(["cmd": "language", "value": language]) } },
            { self.app?.openMain() },
            { self.take(self.window(titled: model.t("app.title")), "main") },
            { model.menuID = model.snapshot?.devices.pinned.first?.id },
            { self.take(self.popover(), "menu") },
            { model.menuID = nil },
            { model.renamingID = model.snapshot?.devices.connected.first?.id },
            { self.take(self.window(titled: model.t("app.title")), "rename") },
            { model.renamingID = nil },
            {
                if let image = self.app?.statusItem?.button?.image {
                    self.drawMenuBarItem(image, "menu-bar-item")
                }
            },
            { self.app?.statusItem?.snapshotMenu { self.take($0, "menu-bar", wait: 0) } },
            { self.app?.openSettings() },
            { self.take(self.settings(0), "settings-general") },
            { self.take(self.settings(1), "settings-reminders") },
            { self.take(self.settings(2), "settings-overlay") },
            { model.send(["cmd": "test_alert"]) },
            { self.take(self.floating(clickThrough: false), "reminder") },
            // The other kind: every device below its reminder level, none below its critical one
            { self.app?.perform([self.lowReminder(model)]) },
            { self.take(self.floating(clickThrough: false), "reminder-low") },
            { self.app?.overlay.preview() },
            { self.take(self.floating(clickThrough: true), "overlay") },
            { NSApp.terminate(nil) },
        ]
        next()
    }

    /// A reminder that isn't critical: the demo's connected devices that have a level, each just below its own
    /// reminder level, as the sample reminder in the settings has them.
    private func lowReminder(_ model: AppModel) -> [String: Any] {
        let devices = (model.snapshot?.devices.pinned ?? []) + (model.snapshot?.devices.connected ?? [])
        let items: [[String: Any]] = devices.filter { $0.connected && !$0.levelText.isEmpty }.map {
            ["id": $0.id, "name": $0.name, "kind": $0.kind, "level": "\($0.low - 2)%", "percent": $0.low - 2,
             "critical": false]
        }
        return ["type": "alert", "test": true, "critical": false, "snooze": model.tf("alert.snooze", 10),
                "auto_close": 10, "items": items]
    }

    /// One step, then the next one once the windows have drawn.
    private func next() {
        guard !steps.isEmpty else { return }
        let step = steps.removeFirst()
        step()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { self.next() }
    }

    private func window(titled title: String) -> NSWindow? {
        NSApp.windows.first { $0.isVisible && $0.title == title }
    }

    private func popover() -> NSWindow? {
        NSApp.windows.first { $0.isVisible && String(describing: type(of: $0)).contains("Popover") }
    }

    private func settings(_ tab: Int) -> NSWindow? {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController is NSTabViewController }),
              let tabs = window.contentViewController as? NSTabViewController
        else { return nil }
        tabs.selectedTabViewItemIndex = tab
        window.displayIfNeeded()
        return window
    }

    private func floating(clickThrough: Bool) -> NSWindow? {
        NSApp.windows.first { $0.isVisible && $0 is FloatingPanel && $0.ignoresMouseEvents == clickThrough }
    }

    /// Draws the window after `wait` seconds (a tab just selected resizes its window first).
    private func take(_ window: NSWindow?, _ name: String, wait: TimeInterval = 0.6) {
        guard let number = window?.windowNumber else {
            FileHandle.standardError.write(Data("snapshot: no window for \(name)\n".utf8))
            return
        }
        if wait > 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + wait) {
                MainActor.assumeIsolated { self.draw(number, name) }
            }
        } else {
            draw(number, name)
        }
    }

    /// The menu bar item as it draws itself, on a plain menu bar of the pictures' appearance: its own window belongs to
    /// the system and can't be taken.
    private func drawMenuBarItem(_ image: NSImage, _ name: String) {
        let appearance = NSApp.effectiveAppearance
        let dark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let size = NSSize(width: (image.size.width + 20).rounded(.up), height: 24)
        guard let bitmap = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width * 2), pixelsHigh: Int(size.height * 2), bitsPerSample: 8,
            samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .calibratedRGB, bytesPerRow: 0,
            bitsPerPixel: 0)
        else { return }
        bitmap.size = size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        appearance.performAsCurrentDrawingAppearance {
            NSColor(white: dark ? 0.16 : 0.9, alpha: 1).setFill()
            NSRect(origin: .zero, size: size).fill()
            image.draw(in: NSRect(x: 10, y: ((size.height - image.size.height) / 2).rounded(),
                                  width: image.size.width, height: image.size.height))
        }
        NSGraphicsContext.restoreGraphicsState()
        try? bitmap.representation(using: .png, properties: [:])?.write(to: folder.appendingPathComponent("\(name).png"))
    }

    private func draw(_ number: Int, _ name: String) {
        guard let image = CGWindowListCreateImage(.null, .optionIncludingWindow, CGWindowID(number),
                                                  [.boundsIgnoreFraming, .bestResolution]),
              let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
        else {
            FileHandle.standardError.write(Data("snapshot: \(name) couldn't be drawn\n".utf8))
            return
        }
        try? png.write(to: folder.appendingPathComponent("\(name).png"))
    }
}
