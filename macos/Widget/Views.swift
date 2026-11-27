import SwiftUI
import WidgetKit

/// What a widget shows its devices as: the one to charge next, rings, or a list.
enum Style {
    case next, rings, list
}

/// A widget's contents for its devices, as drawn at `date`: the layout of its style and size, or what it says when
/// there is nothing to show.
struct WidgetContent: View {
    let style: Style
    let family: WidgetFamily
    let feed: Feed?
    let date: Date
    let palette: Palette

    var body: some View {
        if let feed, feed.running {
            if feed.devices.isEmpty {
                Notice(symbol: "battery.0percent", title: Words.text("empty.title", feed.lang),
                       hint: Words.text("empty.hint", feed.lang), family: family, palette: palette)
            } else {
                devices(feed)
            }
        } else {
            // Not written yet, or the app has quit: levels that would only grow old aren't shown
            Notice(symbol: "power", title: Words.text("closed.title", feed?.lang),
                   hint: Words.text("closed.hint", feed?.lang), family: family, palette: palette)
        }
    }

    @ViewBuilder
    private func devices(_ feed: Feed) -> some View {
        switch style {
        case .next:
            if let device = feed.nextDevice {
                NextView(device: device, feed: feed, date: date, palette: palette)
            } else {
                Notice(symbol: "battery.0percent", title: feed.texts.noneOnline, hint: nil, family: family,
                       palette: palette)
            }
        case .rings:
            switch family {
            case .systemSmall: SmallRings(devices: feed.devices(at: feed.four), feed: feed, palette: palette)
            case .systemMedium: MediumRings(devices: feed.devices(at: feed.four), feed: feed, palette: palette)
            default: LargeRings(devices: feed.devices(at: feed.six), feed: feed, date: date, palette: palette)
            }
        case .list:
            DeviceList(devices: feed.devices(at: feed.six), feed: feed, date: date, palette: palette)
        }
    }
}

/// The colors a widget is drawn in. In full color the app's own (Model.swift: Tone), a level in its color only when it
/// is low unless the widget is a colored one. When macOS tints the widgets or dims them, shades of one color, which
/// macOS colors itself: then a state is told by the rings, the lightning and the "!" alone, as always too.
struct Palette {
    /// Drawn in its own colors (WidgetRenderingMode.fullColor)
    let full: Bool
    let dark: Bool
    /// A colored widget: every level in its tone's color
    let colorful: Bool

    static func background(dark: Bool) -> Color {
        dark ? Color(rgb: 0x1E1E20) : .white
    }

    var fg: Color {
        full ? Color(rgb: dark ? 0xF5F5F7 : 0x1D1D1F) : .primary
    }

    var fg2: Color {
        guard full else { return .secondary }
        return dark ? Color(rgb: 0xEBEBF5).opacity(0.6) : Color(rgb: 0x3C3C43).opacity(0.62)
    }

    var track: Color {
        full ? (dark ? Color.white.opacity(0.14) : Color.black.opacity(0.09)) : Color.primary.opacity(0.22)
    }

    var chip: Color {
        full ? (dark ? Color.white.opacity(0.07) : Color.black.opacity(0.05)) : Color.primary.opacity(0.1)
    }

    /// The color of a level's ring or bar and of its number
    func color(_ device: Feed.Device) -> Color {
        guard full else { return fg }
        switch device.tone {
        case "warn", "bad", "offline": return tone(device.tone)
        case "good", "charge": return colorful ? tone(device.tone) : fg
        default: return fg
        }
    }

    func number(_ device: Feed.Device) -> Color {
        device.connected ? color(device) : fg2
    }

    func name(_ device: Feed.Device) -> Color {
        device.connected ? fg : fg2
    }

    /// A device that is off and a guessed level are fainter; charging without a level, the full ring faintest
    func opacity(_ device: Feed.Device) -> Double {
        !device.connected ? 0.5 : device.indeterminate ? 0.3 : device.estimate ? 0.7 : 1
    }

    var badge: Color {
        full ? tone("bad") : fg
    }

    var badgeMark: Color {
        Self.background(dark: dark)
    }

    /// view.rs: Tone, in the light and in the dark appearance (Model.swift: Tone)
    private func tone(_ name: String) -> Color {
        let shades: (light: Int, dark: Int)
        switch name {
        case "good": shades = (0x008932, 0x6CCB5F)
        case "warn": shades = (0xC55300, 0xF7C04A)
        case "bad": shades = (0xE9152D, 0xFF6B6B)
        case "charge": shades = (0x1E6EF4, 0x60CDFF)
        default: shades = (0x8A8A8A, 0x7A7A7A)
        }
        return Color(rgb: dark ? shades.dark : shades.light)
    }
}

extension Color {
    init(rgb: Int) {
        self.init(.sRGB, red: Double((rgb >> 16) & 0xFF) / 255, green: Double((rgb >> 8) & 0xFF) / 255,
                  blue: Double(rgb & 0xFF) / 255)
    }
}

/// The SF Symbol for a kind of device, as the app's (Model.swift: symbol)
func symbol(_ kind: String) -> String {
    switch kind {
    case "headset": return "headphones"
    case "mouse": return "computermouse"
    case "keyboard": return "keyboard"
    case "gamepad": return "gamecontroller"
    case "phone": return "iphone"
    case "tablet": return "ipad"
    default: return "dot.radiowaves.left.and.right"
    }
}

/// A device's kind as its symbol, `size` points across
struct KindSymbol: View {
    let device: Feed.Device
    let size: CGFloat
    let palette: Palette

    var body: some View {
        Image(systemName: symbol(device.kind))
            .font(.system(size: size * 0.75, weight: .medium))
            .frame(width: size, height: size)
            .foregroundStyle(palette.name(device))
    }
}

struct Bolt: View {
    let size: CGFloat

    var body: some View {
        Image(systemName: "bolt.fill").font(.system(size: size * 0.85, weight: .semibold))
    }
}

/// The level in its color, followed by the lightning while it charges; "Charging" in its place without a level
struct LevelText: View {
    let device: Feed.Device
    let feed: Feed
    let size: CGFloat
    /// Without the "%", inside a small ring
    var percent = true
    /// "Charging" without a level; in the list the line under the name says it already
    var word = true
    let palette: Palette

    var body: some View {
        HStack(spacing: size * 0.15) {
            if device.indeterminate {
                if word {
                    Text(feed.texts.charging).font(.system(size: size * 0.8, weight: .semibold)).lineLimit(1)
                        .minimumScaleFactor(0.6)
                }
            } else {
                Text(percent ? device.level : device.level.replacingOccurrences(of: "%", with: ""))
                    .font(.system(size: size, weight: .semibold)).monospacedDigit().lineLimit(1)
                    .minimumScaleFactor(0.6)
            }
            if device.power {
                Bolt(size: size * 0.7)
            }
        }
        .foregroundStyle(palette.number(device))
    }
}

/// A critical level's "!", at the corner of its ring or symbol
struct Badge: View {
    let size: CGFloat
    let palette: Palette

    var body: some View {
        Circle()
            .fill(palette.badge)
            .overlay {
                if palette.full {
                    mark.foregroundStyle(palette.badgeMark)
                } else {
                    // Tinted or dimmed, the mark is a hole: whatever color macOS gives the badge, the mark stays seen
                    mark.blendMode(.destinationOut)
                }
            }
            .compositingGroup()
            .frame(width: size, height: size)
    }

    private var mark: some View {
        Text("!").font(.system(size: size * 0.68, weight: .bold))
    }
}

/// A device's level as a ring around what is inside: its line's middle at `radius` of the size from the centre, the
/// ring as wide as `line` of it, the "!" of a critical level at its corner
struct Ring<Inside: View>: View {
    let device: Feed.Device
    let palette: Palette
    let size: CGFloat
    var radius: CGFloat = 0.44
    let line: CGFloat
    let badge: CGFloat
    @ViewBuilder let inside: () -> Inside

    var body: some View {
        ZStack {
            Circle().stroke(palette.track, lineWidth: size * line)
            Circle()
                .trim(from: 0, to: device.indeterminate ? 1 : device.fill)
                .stroke(palette.color(device), style: StrokeStyle(lineWidth: size * line, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .opacity(palette.opacity(device))
                .widgetAccentable()
        }
        .padding(size * (0.5 - radius))
        .overlay { inside() }
        .frame(width: size, height: size)
        .overlay(alignment: .topTrailing) {
            if device.tone == "bad" {
                Badge(size: badge, palette: palette).offset(x: badge * 0.2, y: -badge * 0.18)
            }
        }
        .accessibilityElement(children: .ignore)
    }
}

/// What is inside a ring: the kind's symbol, or the lightning alone while it charges without a level
struct RingInside: View {
    let device: Feed.Device
    let palette: Palette
    let size: CGFloat

    var body: some View {
        if device.indeterminate {
            Bolt(size: size).foregroundStyle(palette.fg)
        } else {
            KindSymbol(device: device, size: size, palette: palette)
        }
    }
}

// MARK: - The layouts

/// Small, "Charge Next": the device to charge next, large
struct NextView: View {
    let device: Feed.Device
    let feed: Feed
    let date: Date
    let palette: Palette

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Ring(device: device, palette: palette, size: 56, radius: 0.43, line: 0.1, badge: 18) {
                RingInside(device: device, palette: palette, size: 24)
            }
            Spacer(minLength: 4)
            VStack(alignment: .leading, spacing: 2) {
                LevelText(device: device, feed: feed, size: 28, palette: palette)
                Text(device.name).font(.system(size: 13, weight: .semibold)).foregroundStyle(palette.name(device))
                    .lineLimit(1)
                Text(feed.detail(device, at: date)).font(.system(size: 11)).foregroundStyle(palette.fg2).lineLimit(1)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(16)
        .accessibilityElement(children: .combine)
    }
}

/// Small rings: four, each with its kind and its level inside
struct SmallRings: View {
    let devices: [Feed.Device]
    let feed: Feed
    let palette: Palette

    var body: some View {
        Grid(horizontalSpacing: 12, verticalSpacing: 12) {
            ForEach(Array(stride(from: 0, to: devices.count, by: 2)), id: \.self) { start in
                GridRow {
                    ForEach(devices[start..<min(start + 2, devices.count)]) { device in
                        Ring(device: device, palette: palette, size: 62, line: 0.09, badge: 16) {
                            if device.indeterminate {
                                Bolt(size: 18).foregroundStyle(palette.fg)
                            } else {
                                VStack(spacing: 1) {
                                    KindSymbol(device: device, size: 16, palette: palette)
                                    LevelText(device: device, feed: feed, size: 12, percent: false, palette: palette)
                                }
                            }
                        }
                        .accessibilityLabel("\(device.name), \(device.level)")
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(12)
    }
}

/// Medium rings: four in a row, each with its level and name under it
struct MediumRings: View {
    let devices: [Feed.Device]
    let feed: Feed
    let palette: Palette

    var body: some View {
        HStack(spacing: 0) {
            ForEach(devices) { device in
                VStack(spacing: 6) {
                    Ring(device: device, palette: palette, size: 62, line: 0.09, badge: 17) {
                        RingInside(device: device, palette: palette, size: 24)
                    }
                    VStack(spacing: 1) {
                        LevelText(device: device, feed: feed, size: 15, palette: palette)
                        Text(device.name).font(.system(size: 11)).foregroundStyle(palette.fg2).lineLimit(1)
                    }
                }
                .frame(maxWidth: .infinity)
                .accessibilityElement(children: .combine)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(.horizontal, 14)
        .padding(.vertical, 16)
    }
}

/// Large rings: six, three in a row, a device that is off saying since when
struct LargeRings: View {
    let devices: [Feed.Device]
    let feed: Feed
    let date: Date
    let palette: Palette

    var body: some View {
        Grid(horizontalSpacing: 0, verticalSpacing: 20) {
            ForEach(Array(stride(from: 0, to: devices.count, by: 3)), id: \.self) { start in
                GridRow(alignment: .top) {
                    ForEach(devices[start..<min(start + 3, devices.count)]) { device in
                        VStack(spacing: 7) {
                            Ring(device: device, palette: palette, size: 76, line: 0.08, badge: 19) {
                                RingInside(device: device, palette: palette, size: 28)
                            }
                            VStack(spacing: 1) {
                                LevelText(device: device, feed: feed, size: 15, palette: palette)
                                Text(device.name).font(.system(size: 11)).foregroundStyle(palette.fg2).lineLimit(1)
                                if !device.connected, let ago = feed.ago(device, at: date) {
                                    Text(ago).font(.system(size: 11)).foregroundStyle(palette.fg2).lineLimit(1)
                                }
                            }
                        }
                        .frame(maxWidth: .infinity)
                        .accessibilityElement(children: .combine)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(.horizontal, 12)
        .padding(.vertical, 18)
    }
}

/// Large list: six rows, each the kind, the name, the level, how it is connected and a bar
struct DeviceList: View {
    let devices: [Feed.Device]
    let feed: Feed
    let date: Date
    let palette: Palette

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(devices.enumerated()), id: \.element.id) { index, device in
                if index > 0 {
                    Spacer(minLength: 0)
                }
                row(device)
            }
            if devices.count < 6 {
                Spacer(minLength: 0)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
    }

    private func row(_ device: Feed.Device) -> some View {
        HStack(spacing: 10) {
            KindSymbol(device: device, size: 22, palette: palette)
                .frame(width: 32, height: 32)
                .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(palette.chip))
                .overlay(alignment: .topTrailing) {
                    if device.tone == "bad" {
                        Badge(size: 14, palette: palette).offset(x: 4, y: -4)
                    }
                }
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(device.name).font(.system(size: 13, weight: .semibold)).foregroundStyle(palette.name(device))
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    LevelText(device: device, feed: feed, size: 15, word: false, palette: palette).fixedSize()
                }
                Text(feed.detail(device, at: date)).font(.system(size: 11)).foregroundStyle(palette.fg2).lineLimit(1)
                Capsule()
                    .fill(palette.track)
                    .frame(height: 3)
                    .overlay(alignment: .leading) {
                        GeometryReader { space in
                            Capsule()
                                .fill(palette.color(device))
                                .opacity(palette.opacity(device))
                                .frame(width: space.size.width * (device.indeterminate ? 1 : device.fill))
                                .widgetAccentable()
                        }
                    }
            }
        }
        .frame(height: 50)
        .accessibilityElement(children: .combine)
    }
}

/// What a widget says instead of devices: none yet, none on the air, or the app is closed
struct Notice: View {
    let symbol: String
    let title: String
    let hint: String?
    let family: WidgetFamily
    let palette: Palette

    var body: some View {
        if family == .systemSmall {
            VStack(alignment: .leading, spacing: 4) {
                Image(systemName: symbol).font(.system(size: 22, weight: .medium)).foregroundStyle(palette.fg2)
                Spacer(minLength: 4)
                Text(title).font(.system(size: 13, weight: .semibold)).foregroundStyle(palette.fg).lineLimit(3)
                if let hint {
                    Text(hint).font(.system(size: 11)).foregroundStyle(palette.fg2).lineLimit(3)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .padding(16)
        } else {
            HStack(spacing: 16) {
                Image(systemName: symbol).font(.system(size: 30, weight: .regular)).foregroundStyle(palette.fg2)
                    .frame(width: 40)
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(.system(size: 15, weight: .semibold)).foregroundStyle(palette.fg)
                    if let hint {
                        Text(hint).font(.system(size: 12)).foregroundStyle(palette.fg2)
                    }
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.horizontal, 20)
        }
    }
}
