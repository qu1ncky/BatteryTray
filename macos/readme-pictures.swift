// The README's pictures, put together around what the apps draw themselves: run by macos/readme-pictures.sh, which
// says where each input comes from. Every picture is dark, as the README shows them.
//   hero <windows main.png> <mac main.png> <out.png>      the device window on Windows (150%) and on a Mac (200%)
//   menubar <frames dir> <lang> <out dir>                 the Mac menu bar with the item's bar-NN.png (400%)
//   taskbar <frames dir> <lang> <out dir>                 the Windows taskbar with the strip's readme-NN.png (400%)
//   reminder <reminder-70.png> <lang> <out.png>           the reminder (200%) in the screen's corner
//   overlay <windows overlay.png> <out.png>               the Windows overlay (150%) over a game
//   widgets <widgets.png> <lang> <out.png>                the Mac desktop widgets (readme-widgets, 200%) on the desktop
//   install <lang> <out dir>                              where to click on the first start, Windows and macOS
import AppKit
import ImageIO
import UniformTypeIdentifiers

let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!

func load(_ path: String) -> CGImage {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { fatalError("can't read \(path)") }
    return image
}

func save(_ image: CGImage, _ path: String) {
    let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("can't write \(path)") }
}

func rgb(_ hex: Int, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 255) / 255, green: CGFloat((hex >> 8) & 255) / 255,
            blue: CGFloat(hex & 255) / 255, alpha: alpha)
}

/// The frames `<prefix><number>.png` of a folder, in order (reminder-low.png, say, isn't one).
func frames(_ dir: String, _ prefix: String) -> [String] {
    let names = (try? FileManager.default.contentsOfDirectory(atPath: dir)) ?? []
    let found = names.filter {
        guard $0.hasPrefix(prefix), $0.hasSuffix(".png") else { return false }
        let number = $0.dropFirst(prefix.count).dropLast(4)
        return !number.isEmpty && number.allSatisfy(\.isNumber)
    }.sorted().map { dir + "/" + $0 }
    guard !found.isEmpty else { fatalError("no \(prefix)*.png in \(dir)") }
    return found
}

func folder(_ path: String) {
    try? FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
}

/// A picture measured in points, `scale` pixels to the point, y going down.
final class Canvas {
    let w: CGFloat, h: CGFloat, scale: CGFloat
    let ctx: CGContext

    init(_ w: CGFloat, _ h: CGFloat, scale: CGFloat) {
        self.w = w
        self.h = h
        self.scale = scale
        ctx = CGContext(data: nil, width: Int((w * scale).rounded()), height: Int((h * scale).rounded()),
                        bitsPerComponent: 8, bytesPerRow: 0, space: sRGB,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: 0, y: h)
        ctx.scaleBy(x: 1, y: -1)
        ctx.interpolationQuality = .high
        ctx.setShouldSmoothFonts(false)
    }

    func image(_ image: CGImage, _ r: CGRect) {
        ctx.saveGState()
        ctx.translateBy(x: r.minX, y: r.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: r.width, height: r.height))
        ctx.restoreGState()
    }

    /// With a shadow under it (a shadow's offset is in device space, y up), cut to `clip` when given.
    func shadowed(_ image: CGImage, _ r: CGRect, blur: CGFloat = 28, drop: CGFloat = 12, alpha: CGFloat = 0.5,
                  clip: CGPath? = nil) {
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 0, height: -drop * scale), blur: blur * scale, color: CGColor(gray: 0, alpha: alpha))
        ctx.beginTransparencyLayer(auxiliaryInfo: nil)
        if let clip {
            ctx.addPath(clip)
            ctx.clip()
        }
        self.image(image, r)
        ctx.endTransparencyLayer()
        ctx.restoreGState()
    }

    /// AppKit drawing (text, SF Symbols) into this picture, in the dark appearance.
    func appKit(_ draw: () -> Void) {
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
        NSAppearance(named: .darkAqua)!.performAsCurrentDrawingAppearance(draw)
        NSGraphicsContext.restoreGraphicsState()
    }

    func result() -> CGImage { ctx.makeImage()! }
}

func roundedPath(_ r: CGRect, _ radius: CGFloat) -> CGPath {
    CGPath(roundedRect: r, cornerWidth: radius, cornerHeight: radius, transform: nil)
}

/// The picture without the rows from..<from+height: a window made shorter where it is empty.
func cut(_ image: CGImage, from: Int, height: Int) -> CGImage {
    let W = image.width, H = image.height - height
    let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0, space: sRGB,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    let top = image.cropping(to: CGRect(x: 0, y: 0, width: W, height: from))!
    let bottom = image.cropping(to: CGRect(x: 0, y: from + height, width: W, height: image.height - from - height))!
    ctx.draw(top, in: CGRect(x: 0, y: H - from, width: W, height: from))
    ctx.draw(bottom, in: CGRect(x: 0, y: 0, width: W, height: bottom.height))
    return ctx.makeImage()!
}

// MARK: - Backgrounds

/// A dark abstract wallpaper: a deep gradient with large soft glows.
func wallpaper(_ c: Canvas, seed: Int = 0) {
    let g = CGGradient(colorsSpace: sRGB, colors: [rgb(0x0B1026), rgb(0x1A1240), rgb(0x2B0F3A)] as CFArray,
                       locations: [0, 0.55, 1])!
    c.ctx.drawLinearGradient(g, start: .zero, end: CGPoint(x: c.w, y: c.h), options: [])
    let glows: [(CGFloat, CGFloat, CGFloat, Int, CGFloat)] = [
        (0.15, 0.25, 0.75, 0x2F5BFF, 0.55), (0.85, 0.15, 0.6, 0xC0389A, 0.45), (0.65, 0.95, 0.8, 0x1FA2C9, 0.35),
        (0.35, 0.85, 0.5, 0x7B3FE4, 0.40),
    ]
    let size = max(c.w, c.h)
    for (i, (x, y, r, color, a)) in glows.enumerated() {
        let center = CGPoint(x: (x + CGFloat((seed + i) % 3) * 0.04) * c.w, y: y * c.h)
        let grad = CGGradient(colorsSpace: sRGB, colors: [rgb(color, a), rgb(color, 0)] as CFArray, locations: [0, 1])!
        c.ctx.drawRadialGradient(grad, startCenter: center, startRadius: 0, endCenter: center, endRadius: r * size,
                                 options: [])
    }
}

/// A dusk landscape for "over a game": sky, stars, the sun, ridges.
func gameScene(_ c: Canvas) {
    let sky = CGGradient(colorsSpace: sRGB, colors: [rgb(0x0E0B2B), rgb(0x3A1D5C), rgb(0xC2477A), rgb(0xF59A5B)] as CFArray,
                         locations: [0, 0.45, 0.78, 1])!
    c.ctx.drawLinearGradient(sky, start: .zero, end: CGPoint(x: 0, y: c.h * 0.78), options: [.drawsAfterEndLocation])
    var r: UInt64 = 7
    func random() -> CGFloat {
        r = r &* 6364136223846793005 &+ 1442695040888963407
        return CGFloat((r >> 33) % 10000) / 10000
    }
    for _ in 0..<70 {
        let p = CGPoint(x: random() * c.w, y: random() * c.h * 0.42)
        let s = 0.4 + random() * 0.9
        c.ctx.setFillColor(CGColor(gray: 1, alpha: 0.35 + random() * 0.5))
        c.ctx.fillEllipse(in: CGRect(x: p.x, y: p.y, width: s, height: s))
    }
    let sun = CGPoint(x: c.w * 0.68, y: c.h * 0.62)
    let glow = CGGradient(colorsSpace: sRGB, colors: [rgb(0xFFD9A0, 0.9), rgb(0xFF8A5B, 0.35), rgb(0xFF8A5B, 0)] as CFArray,
                          locations: [0, 0.25, 1])!
    c.ctx.drawRadialGradient(glow, startCenter: sun, startRadius: 0, endCenter: sun, endRadius: c.w * 0.45, options: [])
    c.ctx.setFillColor(rgb(0xFFE2B8))
    c.ctx.fillEllipse(in: CGRect(x: sun.x - 26, y: sun.y - 26, width: 52, height: 52))
    let ridges: [(CGFloat, CGFloat, Int, CGFloat)] = [(0.60, 34, 0x4A2357, 1.3), (0.70, 30, 0x2E1745, 2.1),
                                                     (0.80, 26, 0x1B0F30, 3.4), (0.90, 20, 0x0D0819, 5.0)]
    for (base, amp, color, freq) in ridges {
        let path = CGMutablePath()
        path.move(to: CGPoint(x: 0, y: c.h))
        let phase = random() * 10
        var x: CGFloat = 0
        while x <= c.w + 4 {
            let t = x / c.w
            path.addLine(to: CGPoint(x: x, y: c.h * base - amp * (0.55 * sin(t * freq * 6.28 + phase)
                + 0.3 * sin(t * freq * 15.1 + phase * 2) + 0.15 * sin(t * freq * 33 + phase * 3))))
            x += 2
        }
        path.addLine(to: CGPoint(x: c.w, y: c.h))
        path.closeSubpath()
        c.ctx.addPath(path)
        c.ctx.setFillColor(rgb(color))
        c.ctx.fillPath()
    }
}

// MARK: - The menu bar and the taskbar

func symbol(_ name: String, _ size: CGFloat) -> NSImage {
    let config = NSImage.SymbolConfiguration(pointSize: size, weight: .regular)
        .applying(NSImage.SymbolConfiguration(paletteColors: [.white]))
    return NSImage(systemSymbolName: name, accessibilityDescription: nil)!.withSymbolConfiguration(config)!
}

/// The right end of a menu bar, 24 pt tall at the top: the clock and three of the system's items. Returns where they
/// begin.
@discardableResult
func menuBar(_ c: Canvas, lang: String, right: CGFloat) -> CGFloat {
    c.ctx.setFillColor(CGColor(gray: 0, alpha: 0.22))
    c.ctx.fill(CGRect(x: 0, y: 0, width: c.w, height: 24))
    let clock = lang == "ru" ? "Чт 26 нояб.  15:41" : lang == "zh" ? "11月26日 周四  15:41" : "Thu Nov 26  3:41 PM"
    var x = right
    c.appKit {
        let text = NSAttributedString(string: clock, attributes: [.font: NSFont.menuBarFont(ofSize: 0),
                                                                   .foregroundColor: NSColor.white])
        let size = text.size()
        x -= size.width
        text.draw(at: NSPoint(x: x, y: (24 - size.height) / 2))
        for name in ["switch.2", "magnifyingglass", "wifi"] {
            let image = symbol(name, 14)
            x -= 18 + image.size.width
            image.draw(in: NSRect(x: x, y: (24 - image.size.height) / 2, width: image.size.width, height: image.size.height),
                       from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
        }
    }
    return x
}

/// The Mac menu bar over the wallpaper with the item's frames, right before the system's items.
func menubarScenes(_ dir: String, _ lang: String, _ outDir: String) {
    folder(outDir)
    for (i, path) in frames(dir, "bar-").enumerated() {
        let bar = load(path)
        let c = Canvas(415, 44, scale: 4)
        wallpaper(c)
        let left = menuBar(c, lang: lang, right: 415 - 14)
        let size = CGSize(width: CGFloat(bar.width) / 4, height: CGFloat(bar.height) / 4)
        c.image(bar, CGRect(x: left - 18 - size.width, y: (24 - size.height) / 2, width: size.width, height: size.height))
        save(c.result(), outDir + String(format: "/f%03d.png", i))
    }
}

/// The right end of a dark Windows 11 taskbar, 48 px tall: the strip's frames, the notification area's arrow and the
/// clock (drawn here: the system's own glyphs and font aren't on a Mac).
func taskbarScenes(_ dir: String, _ lang: String, _ outDir: String) {
    folder(outDir)
    let (time, date) = lang == "ru" ? ("15:41", "26.11.2026") : lang == "zh" ? ("15:41", "2026/11/26") : ("3:41 PM", "11/26/2026")
    for (i, path) in frames(dir, "readme-").enumerated() {
        let strip = load(path)
        let c = Canvas(415, 48, scale: 4)
        c.ctx.setFillColor(rgb(0x1B1B1B))
        c.ctx.fill(CGRect(x: 0, y: 0, width: 415, height: 48))
        c.ctx.setFillColor(CGColor(gray: 1, alpha: 0.08))
        c.ctx.fill(CGRect(x: 0, y: 0, width: 415, height: 1))
        var x: CGFloat = 415 - 14
        c.appKit {
            let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.white]
            let t = NSAttributedString(string: time, attributes: attrs), d = NSAttributedString(string: date, attributes: attrs)
            let w = max(t.size().width, d.size().width)
            x -= w
            t.draw(at: NSPoint(x: x + w - t.size().width, y: 8))
            d.draw(at: NSPoint(x: x + w - d.size().width, y: 24))
        }
        x -= 22
        let chevron = CGMutablePath()
        chevron.move(to: CGPoint(x: x, y: 26.5))
        chevron.addLine(to: CGPoint(x: x + 5, y: 21.5))
        chevron.addLine(to: CGPoint(x: x + 10, y: 26.5))
        c.ctx.addPath(chevron)
        c.ctx.setStrokeColor(CGColor(gray: 1, alpha: 0.9))
        c.ctx.setLineWidth(1)
        c.ctx.setLineCap(.round)
        c.ctx.setLineJoin(.round)
        c.ctx.strokePath()
        x -= 12
        let size = CGSize(width: CGFloat(strip.width) / 4, height: CGFloat(strip.height) / 4)
        c.image(strip, CGRect(x: x - size.width, y: (48 - size.height) / 2, width: size.width, height: size.height))
        save(c.result(), outDir + String(format: "/f%03d.png", i))
    }
}

// MARK: - The windows

/// The Windows window (150%) and the Mac one (200%) side by side, each without the empty band above its status line.
func hero(_ windows: String, _ mac: String, _ output: String) {
    let win = cut(load(windows), from: 820, height: 75), macImage = cut(load(mac), from: 1000, height: 200)
    let winSize = CGSize(width: CGFloat(win.width) / 1.5, height: CGFloat(win.height) / 1.5)
    let macSize = CGSize(width: CGFloat(macImage.width) / 2, height: CGFloat(macImage.height) / 2)
    let margin: CGFloat = 36, gap: CGFloat = 44
    let c = Canvas(margin * 2 + winSize.width + gap + macSize.width, margin * 2 + max(winSize.height, macSize.height) + 8,
                   scale: 1.5)
    let winRect = CGRect(x: margin, y: margin + (macSize.height - winSize.height) / 2, width: winSize.width,
                         height: winSize.height)
    c.shadowed(win, winRect, clip: roundedPath(winRect, 8))
    c.ctx.addPath(roundedPath(winRect.insetBy(dx: 0.5, dy: 0.5), 7.5))
    c.ctx.setStrokeColor(CGColor(gray: 1, alpha: 0.14))
    c.ctx.setLineWidth(1)
    c.ctx.strokePath()
    c.shadowed(macImage, CGRect(x: winRect.maxX + gap, y: margin, width: macSize.width, height: macSize.height))
    save(c.result(), output)
}

/// The reminder in the top right corner under the menu bar (`reminder-70.png`: 70% of its time left).
func reminderScene(_ input: String, _ lang: String, _ output: String) {
    let panel = load(input)
    let c = Canvas(390, 290, scale: 2)
    wallpaper(c, seed: 1)
    menuBar(c, lang: lang, right: 390 - 12)
    let size = CGSize(width: CGFloat(panel.width) / 2, height: CGFloat(panel.height) / 2)
    c.shadowed(panel, CGRect(x: 390 - 12 - size.width, y: 24 + 10, width: size.width, height: size.height),
               blur: 22, drop: 8, alpha: 0.55)
    save(c.result(), output)
}

/// The desktop widgets (`readme-widgets`: laid out, nothing around them) on the wallpaper under the menu bar, each
/// with the shadow macOS gives it.
func widgetsScene(_ input: String, _ lang: String, _ output: String) {
    let widgets = load(input)
    let size = CGSize(width: CGFloat(widgets.width) / 2, height: CGFloat(widgets.height) / 2)
    let margin: CGFloat = 40, top: CGFloat = 24 + 36
    let c = Canvas(size.width + 2 * margin, top + size.height + margin, scale: 2)
    wallpaper(c, seed: 2)
    menuBar(c, lang: lang, right: c.w - 14)
    c.shadowed(widgets, CGRect(x: margin, y: top, width: size.width, height: size.height), blur: 20, drop: 6,
               alpha: 0.4)
    save(c.result(), output)
}

/// The overlay in the top left corner of a game, with its shortcut below.
func overlayScene(_ input: String, _ output: String) {
    let overlay = load(input)
    let c = Canvas(390, 290, scale: 2)
    gameScene(c)
    let width = min(CGFloat(overlay.width) / 1.5, 390 - 24)
    let rect = CGRect(x: 12, y: 12, width: width, height: width * CGFloat(overlay.height) / CGFloat(overlay.width))
    c.shadowed(overlay, rect, blur: 14, drop: 4, alpha: 0.5, clip: roundedPath(rect, 6))
    c.appKit {
        let font = NSFont.systemFont(ofSize: 13, weight: .semibold)
        let labels = ["Ctrl", "Alt", "V"].map {
            NSAttributedString(string: $0, attributes: [.font: font, .foregroundColor: NSColor.white])
        }
        let plus = NSAttributedString(string: "+", attributes: [.font: font, .foregroundColor: NSColor(white: 1, alpha: 0.85)])
        let widths = labels.map { max($0.size().width + 18, 30) }
        var x = (390 - widths.reduce(0, +) - CGFloat(labels.count - 1) * (plus.size().width + 12)) / 2
        let y: CGFloat = 290 - 22 - 30
        for (i, label) in labels.enumerated() {
            let key = NSRect(x: x, y: y, width: widths[i], height: 30)
            let path = NSBezierPath(roundedRect: key, xRadius: 6, yRadius: 6)
            NSColor(white: 0, alpha: 0.45).setFill()
            path.fill()
            NSColor(white: 1, alpha: 0.35).setStroke()
            path.lineWidth = 1
            path.stroke()
            label.draw(at: NSPoint(x: key.midX - label.size().width / 2, y: key.midY - label.size().height / 2))
            x += widths[i]
            if i < labels.count - 1 {
                plus.draw(at: NSPoint(x: x + 6, y: key.midY - plus.size().height / 2))
                x += plus.size().width + 12
            }
        }
    }
    save(c.result(), output)
}

// MARK: - Where to click

/// The systems' own words for the first start, as Windows and macOS show them in each language (macOS' from its
/// SecurityPrivacyExtension and CoreServicesUIAgent string tables). The long texts are drawn as grey lines.
let windowsWords: [String: [String: String]] = [
    "en": ["title": "Windows protected your PC", "more": "More info", "app": "App:", "publisher": "Publisher:",
           "unknown": "Unknown publisher", "run": "Run anyway", "dont": "Don't run"],
    "ru": ["title": "Система Windows защитила ваш компьютер", "more": "Подробнее", "app": "Приложение:",
           "publisher": "Издатель:", "unknown": "Неизвестный издатель", "run": "Выполнить в любом случае",
           "dont": "Не выполнять"],
    "zh": ["title": "Windows 已保护你的电脑", "more": "更多信息", "app": "应用:", "publisher": "发布者:",
           "unknown": "未知发布者", "run": "仍要运行", "dont": "不运行"],
]
let macWords: [String: [String: String]] = [
    "en": ["pane": "Privacy & Security", "section": "Security", "blocked": "“BatteryTray” was blocked to protect your Mac.",
           "open": "Open Anyway"],
    "ru": ["pane": "Конфиденциальность и безопасность", "section": "Безопасность",
           "blocked": "Файл «BatteryTray» заблокирован для защиты Вашего Mac.", "open": "Все равно открыть"],
    "zh": ["pane": "隐私与安全性", "section": "安全性", "blocked": "已阻止“BatteryTray”以保护Mac。", "open": "仍要打开"],
]

let highlight = NSColor(srgbRed: 1, green: 0.69, blue: 0.125, alpha: 1)

func text(_ s: String, _ size: CGFloat, _ weight: NSFont.Weight = .regular, _ color: NSColor = .white,
          underline: Bool = false) -> NSAttributedString {
    var attrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: size, weight: weight), .foregroundColor: color]
    if underline { attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue }
    return NSAttributedString(string: s, attributes: attrs)
}

func height(_ s: NSAttributedString, _ width: CGFloat) -> CGFloat {
    s.boundingRect(with: NSSize(width: width, height: 1000), options: [.usesLineFragmentOrigin, .usesFontLeading]).height.rounded(.up)
}

/// A ring around what to click, with its step's number when there are several.
func ring(_ r: NSRect, _ number: Int?) {
    let path = NSBezierPath(roundedRect: r.insetBy(dx: -4, dy: -4), xRadius: 6, yRadius: 6)
    path.lineWidth = 2.5
    highlight.setStroke()
    path.stroke()
    guard let number else { return }
    let badge = NSRect(x: r.maxX - 8, y: r.minY - 16, width: 20, height: 20)
    highlight.setFill()
    NSBezierPath(ovalIn: badge).fill()
    let n = text("\(number)", 12, .bold, NSColor(white: 0.1, alpha: 1))
    n.draw(at: NSPoint(x: badge.midX - n.size().width / 2, y: badge.midY - n.size().height / 2))
}

func lines(_ x: CGFloat, _ y: CGFloat, _ widths: [CGFloat], _ color: NSColor) {
    color.setFill()
    for (i, w) in widths.enumerated() {
        NSBezierPath(roundedRect: NSRect(x: x, y: y + CGFloat(i) * 14, width: w, height: 7), xRadius: 3.5, yRadius: 3.5).fill()
    }
}

/// A button as the dialogs draw it: an outline (Windows) or a filled rounded one (macOS); returns its frame.
@discardableResult
func button(_ label: NSAttributedString, right: CGFloat, y: CGFloat, fill: NSColor?, outline: NSColor?) -> NSRect {
    let size = label.size()
    let r = NSRect(x: right - size.width - 32, y: y, width: size.width + 32, height: size.height + 10)
    let path = NSBezierPath(roundedRect: r, xRadius: fill == nil ? 2 : 6, yRadius: fill == nil ? 2 : 6)
    if let fill { fill.setFill(); path.fill() }
    if let outline { outline.setStroke(); path.lineWidth = 1.5; path.stroke() }
    label.draw(at: NSPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2))
    return r
}

func installWindows(_ lang: String, _ output: String) {
    let w = windowsWords[lang]!
    let width: CGFloat = 420, pad: CGFloat = 18, blue = NSColor(srgbRed: 0.12, green: 0.36, blue: 0.67, alpha: 1)
    let title = text(w["title"]!, 19, .light)
    let titleHeight = height(title, width - pad * 2)
    let small = text("Ag", 13)
    let first = 16 + titleHeight + 10 + 21 + 8 + small.size().height + 16 + small.size().height + 10 + 14
    let second = 16 + titleHeight + 10 + 21 + 6 + small.size().height * 2 + 16 + small.size().height + 10 + 14
    let c = Canvas(width + 16, first + 12 + second + 16, scale: 2)
    c.appKit {
        var top: CGFloat = 8
        for step in 1...2 {
            let h = step == 1 ? first : second
            let box = NSRect(x: 8, y: top, width: width, height: h)
            blue.setFill()
            NSBezierPath(roundedRect: box, xRadius: 8, yRadius: 8).fill()
            title.draw(with: NSRect(x: box.minX + pad, y: top + 16, width: width - pad * 2, height: titleHeight),
                       options: [.usesLineFragmentOrigin, .usesFontLeading])
            var y = top + 16 + titleHeight + 10
            lines(box.minX + pad, y, [(width - pad * 2) * 0.92, (width - pad * 2) * 0.7], NSColor(white: 1, alpha: 0.28))
            y += 21 + (step == 1 ? 8 : 6)
            let dont = text(w["dont"]!, 13)
            if step == 1 {
                let more = text(w["more"]!, 13, .regular, .white, underline: true)
                more.draw(at: NSPoint(x: box.minX + pad, y: y))
                ring(NSRect(x: box.minX + pad, y: y, width: more.size().width, height: more.size().height), 1)
                y += more.size().height + 16
                button(dont, right: box.maxX - pad, y: y, fill: nil, outline: NSColor(white: 1, alpha: 0.6))
            } else {
                for (label, value) in [(w["app"]!, "BatteryTray.exe"), (w["publisher"]!, w["unknown"]!)] {
                    let l = text(label, 13, .semibold)
                    l.draw(at: NSPoint(x: box.minX + pad, y: y))
                    text(value, 13).draw(at: NSPoint(x: box.minX + pad + l.size().width + 6, y: y))
                    y += small.size().height
                }
                y += 16
                let d = button(dont, right: box.maxX - pad, y: y, fill: nil, outline: NSColor(white: 1, alpha: 0.6))
                let r = button(text(w["run"]!, 13), right: d.minX - 8, y: y, fill: nil, outline: NSColor(white: 1, alpha: 0.9))
                ring(r, 2)
            }
            top += h + 12
        }
    }
    save(c.result(), output)
}

func installMac(_ lang: String, _ output: String) {
    let m = macWords[lang]!
    let width: CGFloat = 420, label = NSColor(white: 0.9, alpha: 1)
    let open = text(m["open"]!, 13)
    let buttonWidth = open.size().width + 24
    let message = text(m["blocked"]!, 13, .regular, label)
    let messageWidth = width - 32 - 24 - buttonWidth - 16
    let messageHeight = height(message, messageWidth)
    let row = max(messageHeight, open.size().height + 8) + 20
    let total: CGFloat = 46 + 14 + 18 + 8 + 44 + row + 12 + 52 + 16
    let c = Canvas(width + 16, total + 16, scale: 2)
    c.appKit {
        let pane = NSRect(x: 8, y: 8, width: width, height: total)
        let shape = NSBezierPath(roundedRect: pane, xRadius: 10, yRadius: 10)
        NSColor(white: 0.118, alpha: 1).setFill()
        shape.fill()
        NSColor(white: 0.23, alpha: 1).setStroke()
        shape.lineWidth = 1
        shape.stroke()
        text("‹   ›", 14, .regular, NSColor(white: 0.55, alpha: 1)).draw(at: NSPoint(x: pane.minX + 16, y: pane.minY + 13))
        text(m["pane"]!, 14, .semibold, label).draw(at: NSPoint(x: pane.minX + 56, y: pane.minY + 13))
        NSColor(white: 0.18, alpha: 1).setFill()
        NSRect(x: pane.minX, y: pane.minY + 45, width: width, height: 1).fill()
        var y = pane.minY + 46 + 14
        text(m["section"]!, 13, .semibold, label).draw(at: NSPoint(x: pane.minX + 20, y: y))
        y += 18 + 8
        let group = NSRect(x: pane.minX + 16, y: y, width: width - 32, height: 44 + row)
        let box = NSBezierPath(roundedRect: group, xRadius: 9, yRadius: 9)
        NSColor(white: 0.165, alpha: 1).setFill()
        box.fill()
        NSColor(white: 0.2, alpha: 1).setStroke()
        box.stroke()
        lines(group.minX + 12, group.minY + 18, [group.width * 0.45], NSColor(white: 0.23, alpha: 1))
        lines(group.maxX - 12 - group.width * 0.25, group.minY + 18, [group.width * 0.25], NSColor(white: 0.23, alpha: 1))
        NSColor(white: 0.21, alpha: 1).setFill()
        NSRect(x: group.minX, y: group.minY + 44, width: group.width, height: 1).fill()
        message.draw(with: NSRect(x: group.minX + 12, y: group.minY + 44 + (row - messageHeight) / 2, width: messageWidth,
                                  height: messageHeight), options: [.usesLineFragmentOrigin, .usesFontLeading])
        let b = NSRect(x: group.maxX - 12 - buttonWidth, y: group.minY + 44 + (row - open.size().height - 8) / 2,
                       width: buttonWidth, height: open.size().height + 8)
        NSColor(white: 0.29, alpha: 1).setFill()
        NSBezierPath(roundedRect: b, xRadius: 6, yRadius: 6).fill()
        open.draw(at: NSPoint(x: b.midX - open.size().width / 2, y: b.midY - open.size().height / 2))
        ring(b, nil)
        let next = NSRect(x: group.minX, y: group.maxY + 12, width: group.width, height: 52)
        let other = NSBezierPath(roundedRect: next, xRadius: 9, yRadius: 9)
        NSColor(white: 0.165, alpha: 1).setFill()
        other.fill()
        NSColor(white: 0.2, alpha: 1).setStroke()
        other.stroke()
        lines(next.minX + 12, next.minY + 16, [next.width * 0.55, next.width * 0.35], NSColor(white: 0.23, alpha: 1))
    }
    save(c.result(), output)
}

let args = CommandLine.arguments
switch args.count > 1 ? args[1] : "" {
case "hero": hero(args[2], args[3], args[4])
case "menubar": menubarScenes(args[2], args[3], args[4])
case "taskbar": taskbarScenes(args[2], args[3], args[4])
case "reminder": reminderScene(args[2], args[3], args[4])
case "overlay": overlayScene(args[2], args[3])
case "widgets": widgetsScene(args[2], args[3], args[4])
case "install":
    folder(args[3])
    installWindows(args[2], args[3] + "/install-windows-\(args[2]).png")
    installMac(args[2], args[3] + "/install-macos-\(args[2]).png")
default:
    FileHandle.standardError.write(Data("readme-pictures hero|menubar|taskbar|reminder|overlay|widgets|install …\n".utf8))
    exit(1)
}
