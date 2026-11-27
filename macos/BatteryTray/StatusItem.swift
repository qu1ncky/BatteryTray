import AppKit

/// The menu bar item, its device tooltip and its commands.
@MainActor
final class StatusItemController: NSObject, NSMenuDelegate {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let menu = NSMenu()
    private let model: AppModel
    /// An unchanged snapshot doesn't draw the item again.
    private var shown: [BarSlot]?
    private var showsBatteryIcon = true
    private var appearanceObservation: NSKeyValueObservation?

    init(model: AppModel) {
        self.model = model
        super.init()
        menu.delegate = self
        menu.autoenablesItems = false
        if let button = item.button {
            button.setAccessibilityLabel("BatteryTray")
            button.target = self
            button.action = #selector(clicked(_:))
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
        }
        item.behavior = []
        item.isVisible = true
        item.button?.imagePosition = .imageOnly
        update(model.snapshot?.tray, batteryIcon: model.snapshot?.settings.trayBatteryIcon ?? true)
        // The menu bar is light or dark by the wallpaper under it: drawn again in the colors of the new one
        appearanceObservation = item.button?.observe(\.effectiveAppearance) { [weak self] _, _ in
            Task { @MainActor in self?.draw() }
        }
    }

    var button: NSStatusBarButton? {
        item.button
    }

    func update(_ tray: Tray?, batteryIcon: Bool) {
        guard let button = item.button else { return }
        button.toolTip = tray?.tooltip
        // VoiceOver reads each device by name with its level, not the symbols
        let devices = tray?.tooltip.replacingOccurrences(of: "\n", with: ", ")
        button.setAccessibilityLabel(["BatteryTray", devices].compactMap { $0 }.joined(separator: ": "))
        let slots = tray?.bar ?? []
        if slots != shown || batteryIcon != showsBatteryIcon {
            shown = slots
            showsBatteryIcon = batteryIcon
            draw()
        }
    }

    private func draw() {
        item.button?.image = MenuBarLabel.image(shown ?? [], batteryIcon: showsBatteryIcon)
    }

    @objc private func clicked(_ sender: NSStatusBarButton) {
        guard let app = AppDelegate.shared, !app.closeSettingsIfVisible() else { return }
        let event = NSApp.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.control) == true {
            showContextMenu()
        } else {
            app.toggleMain()
        }
    }

    private func showContextMenu() {
        AppDelegate.shared?.closeMain()
        guard let button = item.button else { return }
        // The status button can inherit the menu bar's wallpaper-dependent appearance.
        menu.appearance = NSApp.effectiveAppearance
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: button.bounds.height + 5), in: button)
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
        showContextMenu()
    }

    /// Built afresh each time the right-click context menu opens, in the current language.
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        menu.addItem(command(model.t("tray.open"), #selector(open), ""))
        if model.snapshot?.settings.menuBarDevices == 1 {
            menu.addItem(priorityMenu())
        }
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

    private func priorityMenu() -> NSMenuItem {
        let root = NSMenuItem(title: model.t("tray.priority"), action: nil, keyEquivalent: "")
        let sub = NSMenu()
        let current = model.snapshot?.settings.trayPriority ?? "mouse"
        let options = [
            ("mouse", "tray.priority.mouse"),
            ("keyboard", "tray.priority.keyboard"),
            ("bluetooth", "tray.priority.bluetooth"),
            ("headset", "tray.priority.headset"),
            ("lowest", "tray.priority.lowest"),
        ]
        for (value, key) in options {
            let item = NSMenuItem(title: model.t(key), action: #selector(pickPriority(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = value
            item.state = current == value ? .on : .off
            sub.addItem(item)
        }
        root.submenu = sub
        return root
    }

    @objc private func pickPriority(_ sender: NSMenuItem) {
        if let value = sender.representedObject as? String {
            model.set("tray_priority", value)
        }
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

/// Each device's symbol followed by its battery level in the selected style.
/// The glyphs and battery outlines use the menu bar's appearance, and low levels color the battery fill.
enum MenuBarLabel {
    private static let symbolGap: CGFloat = 3
    private static let deviceGap: CGFloat = 8

    private struct Part {
        let symbol: NSImage
        let level: NSImage

        var width: CGFloat { symbol.size.width + MenuBarLabel.symbolGap + level.size.width }
        var height: CGFloat { max(symbol.size.height, level.size.height) }
    }

    static func image(_ slots: [BarSlot], batteryIcon: Bool) -> NSImage {
        // The menu bar's own size, larger with Accessibility > Display > Menu bar size
        let size = NSFont.menuBarFont(ofSize: 0).pointSize
        let symbolSize = NSImage.SymbolConfiguration(pointSize: size * 1.22, weight: .medium)
        let parts: [Part] = slots.compactMap { slot in
            guard let glyph = NSImage(systemSymbolName: symbol(slot.kind), accessibilityDescription: nil)?
                .withSymbolConfiguration(symbolSize)
            else { return nil }
            let level = batteryIcon ? batteryImage(for: slot) : numberImage(for: slot, size: size)
            return Part(symbol: glyph, level: level)
        }
        guard let height = parts.map(\.height).max() else {
            let text = "--" as NSString
            let font = NSFont.monospacedDigitSystemFont(ofSize: size, weight: .semibold)
            let width = text.size(withAttributes: [.font: font]).width.rounded(.up)
            let height = (font.ascender - font.descender).rounded(.up)
            return NSImage(size: NSSize(width: width, height: height), flipped: false) { _ in
                let grey = Tone.nsColor("offline").usingColorSpace(.sRGB) ?? .secondaryLabelColor
                text.draw(at: .zero, withAttributes: [.font: font, .foregroundColor: grey])
                return true
            }
        }
        let width = parts.map(\.width).reduce(0, +) + deviceGap * CGFloat(parts.count - 1)
        let image = NSImage(size: NSSize(width: width, height: height), flipped: false) { _ in
            let label = NSColor.labelColor.usingColorSpace(.sRGB) ?? .labelColor
            let tint = NSImage.SymbolConfiguration(paletteColors: [label])
            var x: CGFloat = 0
            for part in parts {
                let symbol = part.symbol.withSymbolConfiguration(symbolSize.applying(tint)) ?? part.symbol
                symbol.draw(at: NSPoint(x: x, y: ((height - symbol.size.height) / 2).rounded()),
                            from: .zero, operation: .sourceOver, fraction: 1)
                let levelX = x + part.symbol.size.width + symbolGap
                part.level.draw(at: NSPoint(x: levelX, y: ((height - part.level.size.height) / 2).rounded()),
                                from: .zero, operation: .sourceOver, fraction: 1)
                x += part.width + deviceGap
            }
            return true
        }
        image.isTemplate = false
        image.accessibilityDescription = "BatteryTray"
        return image
    }

    private static func numberImage(for slot: BarSlot, size: CGFloat) -> NSImage {
        let label = slot.level as NSString
        let font = NSFont.monospacedDigitSystemFont(ofSize: size, weight: .semibold)
        let textWidth = label.size(withAttributes: [.font: font]).width.rounded(.up)
        let textHeight = (font.ascender - font.descender).rounded(.up)
        let bolt = slot.power
            ? NSImage(systemSymbolName: "bolt.fill", accessibilityDescription: nil)?
                .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: size * 0.8, weight: .semibold))
            : nil
        let boltX = textWidth + (textWidth > 0 ? 1 : 0)
        let width = bolt.map { boltX + $0.size.width } ?? textWidth
        let height = max(textHeight, bolt?.size.height ?? 0)
        return NSImage(size: NSSize(width: width, height: height), flipped: false) { _ in
            let color = slot.tone == "warn" || slot.tone == "bad" ? Tone.nsColor(slot.tone) : NSColor.labelColor
            label.draw(at: NSPoint(x: 0, y: ((height - textHeight) / 2).rounded()),
                       withAttributes: [.font: font, .foregroundColor: color])
            if let bolt {
                Self.tintedSymbol(bolt, color: .labelColor).draw(at: NSPoint(x: boltX, y: ((height - bolt.size.height) / 2).rounded()),
                                                               from: .zero, operation: .sourceOver, fraction: 1)
            }
            return true
        }
    }

    /// Draws a compact horizontal battery menu bar icon with the percentage number inside (or a centered bolt when
    /// charging without a readable percentage), proportional fill, and a top-right corner bolt when charging.
    private static func batteryImage(for slot: BarSlot) -> NSImage {
        let label = slot.level
        let level = Int(label.first == "≈" ? label.dropFirst() : label[...])
        let isCharging = slot.power
        let foregroundColor = NSColor.labelColor
        let fillColor = slot.tone == "warn" || slot.tone == "bad" ? Tone.nsColor(slot.tone) : foregroundColor
        let boltInside = isCharging && level == nil
        let showsCornerBolt = isCharging && !boltInside
        let boltPointSize: CGFloat = boltInside ? 9.5 : 6.8
        let boltSymbol = isCharging
            ? NSImage(systemSymbolName: "bolt.fill", accessibilityDescription: nil)?
                .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: boltPointSize, weight: .bold))
            : nil
        let size = NSSize(width: 27, height: 18)
        let bodyRect = NSRect(x: 1.0, y: 2.5, width: 22.0, height: 13.0)
        let innerRect = bodyRect.insetBy(dx: 1.8, dy: 1.8)
        let percent = min(max(level ?? 0, 0), 100)
        let fraction = CGFloat(percent) / 100.0
        let fillWidth: CGFloat = fraction > 0 ? max(2.0, innerRect.width * fraction) : 0
        let draw: (NSRect) -> Bool = { _ in
            let outline = NSBezierPath(roundedRect: bodyRect.insetBy(dx: 0.6, dy: 0.6), xRadius: 2.6, yRadius: 2.6)
            outline.lineWidth = 1.2
            foregroundColor.withAlphaComponent(0.9).setStroke()
            outline.stroke()

            let capRect = NSRect(x: bodyRect.maxX + 1.0, y: (size.height - 4.6) / 2.0, width: 1.5, height: 4.6)
            foregroundColor.withAlphaComponent(0.9).setFill()
            NSBezierPath(roundedRect: capRect, xRadius: 0.75, yRadius: 0.75).fill()

            if boltInside, let boltSymbol {
                let tinted = Self.tintedSymbol(boltSymbol, color: foregroundColor)
                let boltRect = NSRect(
                    x: bodyRect.midX - boltSymbol.size.width / 2.0,
                    y: bodyRect.midY - boltSymbol.size.height / 2.0,
                    width: boltSymbol.size.width,
                    height: boltSymbol.size.height
                )
                tinted.draw(in: boltRect)
                return true
            }

            let fontSize: CGFloat
            switch label.count {
            case 4...: fontSize = 7.0
            case 3: fontSize = 7.8
            default: fontSize = 9.0
            }
            let font = NSFont.monospacedDigitSystemFont(ofSize: fontSize, weight: .bold)
            let textSize = (label as NSString).size(withAttributes: [.font: font])
            let textShift: CGFloat = showsCornerBolt ? 0.4 : 0
            let textX = bodyRect.midX - textSize.width / 2.0 - textShift
            let textY = bodyRect.midY - textSize.height / 2.0
            let textPoint = NSPoint(x: textX, y: textY)

            let innerClip = NSBezierPath(roundedRect: innerRect, xRadius: 1.3, yRadius: 1.3)
            NSGraphicsContext.saveGraphicsState()
            innerClip.addClip()

            if fillWidth > 0 {
                let fillRect = NSRect(x: innerRect.minX, y: innerRect.minY, width: fillWidth, height: innerRect.height)
                NSGraphicsContext.saveGraphicsState()
                NSBezierPath(rect: fillRect).addClip()
                fillColor.setFill()
                fillRect.fill()
                NSGraphicsContext.current?.compositingOperation = .destinationOut
                (label as NSString).draw(at: textPoint, withAttributes: [.font: font, .foregroundColor: NSColor.black])
                NSGraphicsContext.restoreGraphicsState()
            }

            if fillWidth < innerRect.width {
                let unfilledRect = NSRect(
                    x: innerRect.minX + fillWidth,
                    y: innerRect.minY,
                    width: innerRect.width - fillWidth,
                    height: innerRect.height
                )
                NSGraphicsContext.saveGraphicsState()
                NSBezierPath(rect: unfilledRect).addClip()
                (label as NSString).draw(at: textPoint, withAttributes: [.font: font, .foregroundColor: foregroundColor])
                NSGraphicsContext.restoreGraphicsState()
            }

            NSGraphicsContext.restoreGraphicsState()

            if showsCornerBolt, let boltSymbol {
                let boltSize = boltSymbol.size
                let rightEdgeX = size.width - boltSize.width - 0.5
                let bodyEdgeX = bodyRect.maxX - boltSize.width * 0.45
                let boltX = min(rightEdgeX, bodyEdgeX)
                let boltY = size.height - boltSize.height - 0.6
                let boltRect = NSRect(x: boltX, y: boltY, width: boltSize.width, height: boltSize.height)
                // True alpha cutout (.destinationOut) around the bolt so the menu bar background shows through.
                let gap: CGFloat = 0.75
                let offsets: [(CGFloat, CGFloat)] = [
                    (-gap, 0), (gap, 0), (0, -gap), (0, gap),
                    (-gap, -gap), (gap, -gap), (-gap, gap), (gap, gap),
                ]
                for (dx, dy) in offsets {
                    boltSymbol.draw(
                        in: boltRect.offsetBy(dx: dx, dy: dy),
                        from: .zero,
                        operation: .destinationOut,
                        fraction: 1.0
                    )
                }

                Self.tintedSymbol(boltSymbol, color: foregroundColor).draw(in: boltRect)
            }
            return true
        }
        let image = NSImage(size: size, flipped: false, drawingHandler: draw)
        image.isTemplate = false
        return image
    }

    private static func tintedSymbol(_ symbol: NSImage, color: NSColor) -> NSImage {
        NSImage(size: symbol.size, flipped: false) { rect in
            color.setFill()
            rect.fill()
            symbol.draw(in: rect, from: .zero, operation: .destinationIn, fraction: 1.0)
            return true
        }
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
