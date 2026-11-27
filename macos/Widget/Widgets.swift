import SwiftUI
import WidgetKit

/// BatteryTray's desktop widgets: the device to charge next, rings, and a list, each in the colors of the menu bar
/// (a level in color only when it is low) and as a colored one (every level in its color). They read the devices the
/// app writes for them (Feed); the app asks WidgetKit to draw them again when they change.
@main
struct BatteryTrayWidgets: WidgetBundle {
    var body: some Widget {
        NextWidget()
        NextColorWidget()
        RingsWidget()
        RingsColorWidget()
        ListWidget()
        ListColorWidget()
    }
}

struct NextWidget: Widget {
    var body: some WidgetConfiguration { configuration(.next, colorful: false) }
}

struct NextColorWidget: Widget {
    var body: some WidgetConfiguration { configuration(.next, colorful: true) }
}

struct RingsWidget: Widget {
    var body: some WidgetConfiguration { configuration(.rings, colorful: false) }
}

struct RingsColorWidget: Widget {
    var body: some WidgetConfiguration { configuration(.rings, colorful: true) }
}

struct ListWidget: Widget {
    var body: some WidgetConfiguration { configuration(.list, colorful: false) }
}

struct ListColorWidget: Widget {
    var body: some WidgetConfiguration { configuration(.list, colorful: true) }
}

/// A widget of the gallery: its kind ("rings", "rings.color"...), the sizes it comes in, its name and description in
/// the system's language. The layouts keep their own margins, as drawn.
private func configuration(_ style: Style, colorful: Bool) -> some WidgetConfiguration {
    let (name, families): (String, [WidgetFamily])
    switch style {
    case .next: (name, families) = ("next", [.systemSmall])
    case .rings: (name, families) = ("rings", [.systemSmall, .systemMedium, .systemLarge])
    case .list: (name, families) = ("list", [.systemLarge])
    }
    let kind = colorful ? "\(name).color" : name
    return StaticConfiguration(kind: kind, provider: Provider()) { entry in
        EntryView(entry: entry, style: style, colorful: colorful)
    }
    .configurationDisplayName(Words.text("\(kind).name", nil))
    .description(Words.text("\(kind).description", nil))
    .supportedFamilies(families)
    .contentMarginsDisabled()
}

struct Entry: TimelineEntry {
    let date: Date
    let feed: Feed?
}

struct Provider: TimelineProvider {
    func placeholder(in context: Context) -> Entry {
        Entry(date: Date(), feed: .placeholder)
    }

    /// For the gallery: the user's devices, or the pictures' ones before the app has seen any
    func getSnapshot(in context: Context, completion: @escaping (Entry) -> Void) {
        var feed = Feed.read()
        if context.isPreview, let shown = feed, shown.running, shown.devices.isEmpty {
            feed = .sample(shown.texts, lang: shown.lang)
        }
        completion(Entry(date: Date(), feed: feed))
    }

    /// The devices as the app last wrote them. A device that is off says how long ago it was seen, which changes as it
    /// is shown: then an entry every 5 minutes for an hour and every 15 for five more, after which WidgetKit asks
    /// again. Anything else changes when the app asks for the widgets to be drawn again.
    func getTimeline(in context: Context, completion: @escaping (Timeline<Entry>) -> Void) {
        let now = Date()
        let feed = Feed.read()
        guard let feed, feed.running, feed.devices.contains(where: { !$0.connected && $0.seen != nil }) else {
            completion(Timeline(entries: [Entry(date: now, feed: feed)], policy: .never))
            return
        }
        let minutes = Array(stride(from: 0, to: 60, by: 5)) + Array(stride(from: 60, through: 360, by: 15))
        let entries = minutes.map { Entry(date: now.addingTimeInterval(Double($0) * 60), feed: feed) }
        completion(Timeline(entries: entries, policy: .atEnd))
    }
}

struct EntryView: View {
    let entry: Entry
    let style: Style
    let colorful: Bool
    @Environment(\.widgetFamily) private var family
    @Environment(\.widgetRenderingMode) private var mode
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        WidgetContent(style: style, family: family, feed: entry.feed, date: entry.date,
                      palette: Palette(full: mode == .fullColor, dark: scheme == .dark, colorful: colorful))
            .containerBackground(for: .widget) {
                Palette.background(dark: scheme == .dark)
            }
    }
}
