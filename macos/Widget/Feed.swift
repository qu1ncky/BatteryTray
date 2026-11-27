import Foundation

/// What the app writes for the widgets (macos/core/src/widget.rs) into its folder: the devices of its list but the
/// hidden ones, the one to charge next, those a widget with four places and one with six shows, and the texts the
/// widgets take from the app, in its language.
struct Feed: Decodable {
    /// False once the app has quit: the widgets say it is closed
    var running: Bool
    /// The app's language: "ru", "en" or "zh"
    var lang: String
    var devices: [Device]
    /// Indices into `devices`
    var next: Int?
    var four: [Int]
    var six: [Int]
    var texts: Texts

    struct Device: Decodable, Identifiable {
        var id: String
        var name: String
        /// "headset", "mouse", "keyboard", "gamepad", "phone", "tablet" or "other"
        var kind: String
        var connected: Bool
        /// "37%", "≈15%" for an estimate, empty when the device only says it is charging, "—" without a level; for a
        /// device that is off its last level
        var level: String
        /// How much of the ring or the bar is filled, 0 to 1
        var fill: Double
        var estimate: Bool
        /// Charging or full on its charger: a lightning follows the level
        var power: Bool
        /// Charging with no readable level: the ring is full and faint with the lightning in it
        var indeterminate: Bool
        /// "good", "warn" (below its reminder level), "bad" (critical), "charge" or "offline" (view.rs: Tone)
        var tone: String
        /// Under the name: whether it charges and how it is connected, or why it is off
        var detail: String
        /// For a device that is off: when its last level was read (seconds since 1970), and the date to say when that
        /// is a day ago or more
        var seen: Double?
        var seenDate: String?
    }

    /// The app's own texts, in its language (src/loc.rs)
    struct Texts: Decodable {
        var charging: String
        var offline: String
        var noneOnline: String
        /// How long ago: "just now", "{0} min ago", "{0} h ago"
        var now: String
        var min: String
        var hours: String
    }

    /// The devices at these places
    func devices(at places: [Int]) -> [Device] {
        places.compactMap { devices.indices.contains($0) ? devices[$0] : nil }
    }

    var nextDevice: Device? {
        next.flatMap { devices.indices.contains($0) ? devices[$0] : nil }
    }

    /// How long ago a device that is off was seen, at the moment it is drawn, as the app's list says it (view.rs: ago)
    func ago(_ device: Device, at date: Date) -> String? {
        guard let seen = device.seen else { return nil }
        let seconds = date.timeIntervalSince1970 - seen
        let (minutes, hours) = (Int(seconds / 60), Int(seconds / 3600))
        if minutes < 1 {
            return texts.now
        } else if hours < 1 {
            return texts.min.replacingOccurrences(of: "{0}", with: "\(minutes)")
        } else if hours < 24 {
            return texts.hours.replacingOccurrences(of: "{0}", with: "\(hours)")
        }
        return device.seenDate
    }

    /// Under a device's name: how it is connected and whether it charges, or for one that is off why, and since when
    func detail(_ device: Device, at date: Date) -> String {
        guard !device.connected, let ago = ago(device, at: date) else { return device.detail }
        return device.detail.isEmpty ? ago : "\(device.detail) · \(ago)"
    }

    /// The file as it is now; nil when the app hasn't written one
    static func read() -> Feed? {
        guard let data = try? Data(contentsOf: file) else { return nil }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try? decoder.decode(Feed.self, from: data)
    }

    /// widget/devices.json in the app's folder, ~/Library/Application Support/BatteryTray: the sandbox gives the widget
    /// a home of its own and lets it read this one folder of the user's (Widget.entitlements).
    static var file: URL {
        let home = getpwuid(getuid()).map { String(cString: $0.pointee.pw_dir) } ?? NSHomeDirectory()
        return URL(fileURLWithPath: home, isDirectory: true)
            .appendingPathComponent("Library/Application Support/BatteryTray/widget/devices.json")
    }
}

extension Feed {
    /// What WidgetKit draws while a widget loads: grey rings and bars, its texts as placeholders
    static var placeholder: Feed {
        let devices = (0..<6).map {
            Device(id: "\($0)", name: "Device name", kind: "other", connected: true, level: "00%", fill: 0,
                   estimate: false, power: false, indeterminate: false, tone: "offline", detail: "Connected")
        }
        let texts = Texts(charging: "", offline: "", noneOnline: "", now: "", min: "", hours: "")
        return Feed(running: true, lang: "", devices: devices, next: 0, four: [0, 1, 2, 3], six: Array(0..<6),
                    texts: texts)
    }

    /// What the widget gallery shows before the app has seen a device: the devices of the app's pictures
    static func sample(_ texts: Texts, lang: String) -> Feed {
        let now = Date().timeIntervalSince1970
        func device(_ name: String, _ kind: String, _ level: Int, _ tone: String, estimate: Bool = false,
                    power: Bool = false, offlineHours: Double? = nil) -> Device {
            let off = offlineHours != nil
            return Device(id: name, name: name, kind: kind, connected: !off,
                          level: "\(estimate ? "≈" : "")\(level)%", fill: Double(level) / 100, estimate: estimate,
                          power: power, indeterminate: false, tone: off ? "offline" : tone,
                          detail: off ? texts.offline : power ? "\(texts.charging) · Bluetooth" : "Bluetooth",
                          seen: offlineHours.map { now - $0 * 3600 })
        }
        let devices = [
            device("Razer Barracuda X", "headset", 37, "good"),
            device("Attack Shark R3", "mouse", 62, "charge", power: true),
            device("Rainy 75", "keyboard", 15, "warn", estimate: true),
            device("AirPods Pro", "headset", 8, "bad"),
            device("Xbox Wireless Controller", "gamepad", 80, "offline", offlineHours: 2),
            device("Logitech MX Keys", "keyboard", 64, "offline", offlineHours: 5),
        ]
        return Feed(running: true, lang: lang, devices: devices, next: 3, four: [0, 1, 2, 3], six: Array(0..<6),
                    texts: texts)
    }
}

extension Feed.Device {
    init(id: String, name: String, kind: String, connected: Bool, level: String, fill: Double, estimate: Bool,
         power: Bool, indeterminate: Bool, tone: String, detail: String, seen: Double? = nil) {
        self.id = id
        self.name = name
        self.kind = kind
        self.connected = connected
        self.level = level
        self.fill = fill
        self.estimate = estimate
        self.power = power
        self.indeterminate = indeterminate
        self.tone = tone
        self.detail = detail
        self.seen = seen
        self.seenDate = nil
    }
}

/// The widget's own texts (its *.lproj/Localizable.strings): in the app's language when the app has said it, in the
/// system's otherwise, as the widget gallery shows them.
enum Words {
    static func text(_ key: String, _ lang: String?) -> String {
        let folders = ["ru": "ru", "en": "en", "zh": "zh-Hans"]
        if let folder = lang.flatMap({ folders[$0] }), let path = Bundle.main.path(forResource: folder, ofType: "lproj"),
           let bundle = Bundle(path: path) {
            return bundle.localizedString(forKey: key, value: nil, table: nil)
        }
        return Bundle.main.localizedString(forKey: key, value: nil, table: nil)
    }
}
