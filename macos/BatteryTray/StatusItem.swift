import AppKit

/// The menu bar item: the first devices of the list, each as its kind's symbol and its level (`MenuBarLabel`), the
/// devices in a tooltip, and a menu with each device's level and the app's commands.
@MainActor
final class StatusItemController: NSObject, NSMenuDelegate {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let menu = NSMenu()
    private let model: AppModel
    /// The devices the item shows now: an unchanged snapshot doesn't draw it again
    private var shown: [BarSlot]?

    init(model: AppModel) {
        self.model = model
        super.init()
        menu.delegate = self
        menu.autoenablesItems = false
        item.menu = menu
        item.button?.setAccessibilityLabel("BatteryTray")
        item.behavior = []
        item.isVisible = true
        item.button?.imagePosition = .imageOnly
        update(model.snapshot?.tray)
        // Nothing watches the menu bar's appearance (light or dark by the wallpaper under it, each display's its
        // own): the picture takes its colors as it is drawn (`MenuBarLabel`), in the appearance AppKit draws it in.
        // A new picture on every change of the button's effectiveAppearance kept the main thread busy for good with
        // a second display, whose copy of the item AppKit draws in that display's appearance: drawing the copy changed
        // the appearance, the new picture had the copy drawn again (#57).
    }

    /// Where the item is on the screen: the device window opens under it.
    var buttonFrame: NSRect? {
        guard let button = item.button, let window = button.window else { return nil }
        return window.convertToScreen(button.convert(button.bounds, to: nil))
    }

    func update(_ tray: Tray?) {
        guard let button = item.button else { return }
        button.toolTip = tray?.tooltip
        // VoiceOver reads each device by name with its level, not the symbols
        let devices = tray?.tooltip.replacingOccurrences(of: "\n", with: ", ")
        button.setAccessibilityLabel(["BatteryTray", devices].compactMap { $0 }.joined(separator: ": "))
        let slots = tray?.bar ?? []
        if slots != shown {
            shown = slots
            draw()
        }
    }

    private func draw() {
        item.button?.image = MenuBarLabel.image(shown ?? [])
    }

    /// Opens the menu and hands its window to `take` while it is open, then closes it (`--snapshot`).
    func snapshotMenu(_ take: @escaping (NSWindow?) -> Void) {
        let timer = Timer(timeInterval: 0.8, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                take(NSApp.windows.first { $0.isVisible && String(describing: type(of: $0)).contains("Menu") })
                self?.menu.cancelTracking()
            }
        }
        // Menu tracking runs the event loop in its own mode
        RunLoop.main.add(timer, forMode: .common)
        item.button?.performClick(nil)
    }

    /// Built afresh each time it opens, in the current language and with the current levels.
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let items = model.snapshot?.tray.items ?? []
        if items.isEmpty {
            let none = NSMenuItem(title: model.t("tray.nodata"), action: nil, keyEquivalent: "")
            none.isEnabled = false
            menu.addItem(none)
        }
        for device in items {
            let entry = NSMenuItem(title: device.name, action: #selector(open), keyEquivalent: "")
            entry.target = self
            entry.image = NSImage(systemSymbolName: symbol(device.kind), accessibilityDescription: nil)
            entry.attributedTitle = title(device)
            menu.addItem(entry)
        }
        menu.addItem(.separator())
        menu.addItem(command(model.t("tray.open"), #selector(open), ""))
        menu.addItem(command(model.t("settings") + "…", #selector(settings), ","))
        menu.addItem(command(model.t("refresh"), #selector(refresh), "r"))
        menu.addItem(.separator())
        menu.addItem(command(model.t("tray.exit"), #selector(quit), "q"))
    }

    /// "Razer Barracuda X   85% ⚡": the level in its color, after a tab so the levels line up.
    private func title(_ device: TrayItem) -> NSAttributedString {
        let text = NSMutableAttributedString(string: device.name + "\t")
        let level = [device.level, device.charging ? "⚡" : ""].filter { !$0.isEmpty }.joined(separator: " ")
        let bold = NSFont.monospacedDigitSystemFont(ofSize: NSFont.menuFont(ofSize: 0).pointSize, weight: .semibold)
        let color = Tone.nsColor(device.tone)
        text.append(NSAttributedString(string: level, attributes: [.foregroundColor: color, .font: bold]))
        let style = NSMutableParagraphStyle()
        style.tabStops = [NSTextTab(textAlignment: .right, location: 260)]
        text.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: text.length))
        return text
    }

    private func command(_ title: String, _ action: Selector, _ key: String) -> NSMenuItem {
        let entry = NSMenuItem(title: title, action: action, keyEquivalent: key)
        entry.target = self
        return entry
    }

    @objc private func open() {
        AppDelegate.shared?.openMain()
    }

    @objc private func settings() {
        AppDelegate.shared?.openSettings()
    }

    @objc private func refresh() {
        model.send(["cmd": "refresh"])
    }

    @objc private func quit() {
        AppDelegate.shared?.quit()
    }
}

/// The menu bar item's picture: each device's kind as its symbol, then its level without "%" and a lightning while it
/// charges, in the menu bar's own text color; a level below the device's reminder or critical level in the app's
/// orange or red, the only color in it. "--" when no device has a level. Drawn whenever it is shown, so its colors are
/// those of the menu bar it is shown in at that moment (light or dark by the wallpaper, not the windows' theme): on
/// each display its own, without drawing it anew.
enum MenuBarLabel {
    /// Between a symbol and its level, a level and its lightning, and two devices
    static let symbolGap: CGFloat = 3
    static let boltGap: CGFloat = 1
    static let deviceGap: CGFloat = 8

    /// One device, measured.
    private struct Part {
        var symbol: NSImage
        var level: String
        var levelWidth: CGFloat
        var bolt: NSImage?
        var color: NSColor?

        var width: CGFloat {
            symbol.size.width + (levelWidth > 0 ? MenuBarLabel.symbolGap + levelWidth : 0)
                + (bolt.map { MenuBarLabel.boltGap + $0.size.width } ?? 0)
        }
    }

    static func image(_ slots: [BarSlot]) -> NSImage {
        // The menu bar's own size, larger with Accessibility > Display > Menu bar size
        let size = NSFont.menuBarFont(ofSize: 0).pointSize
        let font = NSFont.monospacedDigitSystemFont(ofSize: size, weight: .semibold)
        let symbolSize = NSImage.SymbolConfiguration(pointSize: size, weight: .medium)
        let boltSize = NSImage.SymbolConfiguration(pointSize: size * 0.8, weight: .semibold)
        let measure = { (text: String) in (text as NSString).size(withAttributes: [.font: font]).width.rounded(.up) }
        let parts: [Part] = slots.compactMap { slot in
            guard let glyph = NSImage(systemSymbolName: symbol(slot.kind), accessibilityDescription: nil)?
                .withSymbolConfiguration(symbolSize)
            else { return nil }
            let low = slot.tone == "warn" || slot.tone == "bad"
            return Part(
                symbol: glyph,
                level: slot.level,
                levelWidth: slot.level.isEmpty ? 0 : measure(slot.level),
                bolt: slot.power
                    ? NSImage(systemSymbolName: "bolt.fill", accessibilityDescription: nil)?.withSymbolConfiguration(boltSize)
                    : nil,
                color: low ? Tone.nsColor(slot.tone) : nil
            )
        }
        let textHeight = (font.ascender - font.descender).rounded(.up)
        if parts.isEmpty {
            let none = "--"
            return NSImage(size: NSSize(width: measure(none), height: textHeight), flipped: false) { _ in
                let grey = Tone.nsColor("offline").usingColorSpace(.sRGB) ?? .secondaryLabelColor
                (none as NSString).draw(at: .zero, withAttributes: [.font: font, .foregroundColor: grey])
                return true
            }
        }
        let height = max(textHeight, parts.map(\.symbol.size.height).max() ?? 0)
        let width = parts.map(\.width).reduce(0, +) + deviceGap * CGFloat(parts.count - 1)
        let image = NSImage(size: NSSize(width: width, height: height), flipped: false) { _ in
            // The colors as they are in the appearance the image is being drawn in
            let resolve = { (color: NSColor) in color.usingColorSpace(.sRGB) ?? color }
            let label = resolve(.labelColor)
            let tint = NSImage.SymbolConfiguration(paletteColors: [label])
            var x: CGFloat = 0
            for part in parts {
                let color = part.color.map(resolve) ?? label
                let symbol = part.symbol.withSymbolConfiguration(symbolSize.applying(tint)) ?? part.symbol
                symbol.draw(in: NSRect(x: x, y: ((height - symbol.size.height) / 2).rounded(),
                                       width: symbol.size.width, height: symbol.size.height))
                x += symbol.size.width
                if part.levelWidth > 0 {
                    x += symbolGap
                    (part.level as NSString).draw(at: NSPoint(x: x, y: ((height - textHeight) / 2).rounded()),
                                                  withAttributes: [.font: font, .foregroundColor: color])
                    x += part.levelWidth
                }
                if let bolt = part.bolt {
                    x += boltGap
                    let tinted = bolt.withSymbolConfiguration(boltSize.applying(tint)) ?? bolt
                    tinted.draw(in: NSRect(x: x, y: ((height - tinted.size.height) / 2).rounded(),
                                           width: tinted.size.width, height: tinted.size.height))
                    x += tinted.size.width
                }
                x += deviceGap
            }
            return true
        }
        image.isTemplate = false
        image.accessibilityDescription = "BatteryTray"
        return image
    }
}

/// The menus at the top of the screen while a window of the app is open.
@MainActor
enum MainMenu {
    static func install(_ model: AppModel) {
        let t = model.t
        let main = NSMenu()

        let app = NSMenu()
        app.addItem(withTitle: t("menu.about"), action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)),
                    keyEquivalent: "")
        app.addItem(.separator())
        let settings = app.addItem(withTitle: t("settings") + "…", action: #selector(MenuActions.settings),
                                   keyEquivalent: ",")
        settings.target = MenuActions.shared
        app.addItem(.separator())
        app.addItem(withTitle: t("menu.hide"), action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let others = app.addItem(withTitle: t("menu.hideOthers"),
                                 action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
        others.keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: t("menu.showAll"), action: #selector(NSApplication.unhideAllApplications(_:)),
                    keyEquivalent: "")
        app.addItem(.separator())
        let quit = app.addItem(withTitle: t("tray.exit"), action: #selector(MenuActions.quit), keyEquivalent: "q")
        quit.target = MenuActions.shared
        main.addItem(submenu(app, title: "BatteryTray"))

        let edit = NSMenu(title: t("menu.edit"))
        edit.addItem(withTitle: t("menu.undo"), action: Selector(("undo:")), keyEquivalent: "z")
        let redo = edit.addItem(withTitle: t("menu.redo"), action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: t("menu.cut"), action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: t("menu.copy"), action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: t("menu.paste"), action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: t("menu.selectAll"), action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        main.addItem(submenu(edit, title: t("menu.edit")))

        let window = NSMenu(title: t("menu.window"))
        window.addItem(withTitle: t("menu.minimize"), action: #selector(NSWindow.performMiniaturize(_:)),
                       keyEquivalent: "m")
        window.addItem(withTitle: t("menu.close"), action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        main.addItem(submenu(window, title: t("menu.window")))
        NSApp.windowsMenu = window

        NSApp.mainMenu = main
    }

    private static func submenu(_ menu: NSMenu, title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.submenu = menu
        return item
    }
}

/// The targets of the main menu's own commands.
@MainActor
final class MenuActions: NSObject {
    static let shared = MenuActions()

    @objc func settings() {
        AppDelegate.shared?.openSettings()
    }

    @objc func quit() {
        AppDelegate.shared?.quit()
    }
}
