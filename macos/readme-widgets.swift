// The README's picture of the Mac desktop widgets, before readme-pictures.swift puts it on the wallpaper: the
// demo's devices as the core writes them for the widgets, in a language, each widget drawn by the widgets' own views
// (macos/Widget) - the monochrome ones, in the dark appearance - laid out as on a desktop, at 200%, with nothing
// around them. Built and run by macos/readme-pictures.sh, against the core it builds:
//   readme-widgets <ru|en|zh> <out.png>
import AppKit
import ImageIO
import SwiftUI
import UniformTypeIdentifiers
import WidgetKit

@main
struct ReadmeWidgets {
    @MainActor
    static func main() {
        let arguments = CommandLine.arguments
        guard arguments.count == 3 else { fail("usage: readme-widgets <ru|en|zh> <out.png>") }
        let renderer = ImageRenderer(content: Desktop(feed: demoFeed(language: arguments[1]), date: Date()))
        renderer.scale = 2
        guard let image = renderer.cgImage else { fail("can't draw the widgets") }
        let url = URL(fileURLWithPath: arguments[2])
        guard let file = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)
        else { fail("can't write \(url.path)") }
        CGImageDestinationAddImage(file, image, nil)
        guard CGImageDestinationFinalize(file) else { fail("can't write \(url.path)") }
    }
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

/// The demo's devices (macos/core/src/demo.rs) as the core writes them for the widgets, into the demo's folder, once
/// its provider has been polled in the language.
@MainActor
func demoFeed(language: String) -> Feed {
    Core.call(["cmd": "start", "demo": true])
    Core.call(["cmd": "language", "value": language])
    for _ in 0..<30 {
        Thread.sleep(forTimeInterval: 0.1)
        Core.call(["cmd": "pump"])
    }
    let temporary = ProcessInfo.processInfo.environment["TMPDIR"] ?? "/tmp"
    let file = URL(fileURLWithPath: temporary).appendingPathComponent("BatteryTray-demo/widget/devices.json")
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    guard let data = try? Data(contentsOf: file), let feed = try? decoder.decode(Feed.self, from: data),
          feed.lang == language, feed.devices.contains(where: \.connected)
    else { fail("no demo devices in \(file.path)") }
    return feed
}

/// The monochrome widgets as on a desktop, 16 points apart as macOS places them: the two small ones and the medium
/// one above, the list and the large rings below.
struct Desktop: View {
    let feed: Feed
    let date: Date

    var body: some View {
        VStack(spacing: 16) {
            HStack(spacing: 16) {
                widget(.next, .systemSmall)
                widget(.rings, .systemSmall)
                widget(.rings, .systemMedium)
            }
            HStack(spacing: 16) {
                widget(.list, .systemLarge)
                widget(.rings, .systemLarge)
            }
        }
        .environment(\.colorScheme, .dark)
    }

    private func widget(_ style: Style, _ family: WidgetFamily) -> some View {
        let shape = RoundedRectangle(cornerRadius: 22, style: .continuous)
        return WidgetContent(style: style, family: family, feed: feed, date: date,
                             palette: Palette(full: true, dark: true, colorful: false))
            .frame(width: family == .systemSmall ? 164 : 344, height: family == .systemLarge ? 344 : 164)
            .background(Palette.background(dark: true))
            .clipShape(shape)
            .overlay(shape.strokeBorder(Color.white.opacity(0.07), lineWidth: 1))
    }
}
