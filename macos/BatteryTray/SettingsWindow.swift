import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// The settings, as a Mac app has them: a window of their own (⌘,) with the tabs in its toolbar - General, Reminders,
/// Overlay - each one as tall as its contents. The window follows its page's height smoothly, over the moment the page
/// animates the same change: a part that opens or closes (a row, a section), another tab. Its top edge stays put.
@MainActor
final class SettingsWindowController: NSObject, NSWindowDelegate {
    static let duration: TimeInterval = 0.22
    private let window: NSWindow
    private let tabs = SettingsTabs()
    private let model: AppModel
    private let closed: () -> Void
    /// Each page's height, as it last told it
    private var heights: [Int: CGFloat] = [:]

    init(model: AppModel, closed: @escaping () -> Void) {
        self.model = model
        self.closed = closed
        tabs.tabStyle = .toolbar
        window = NSWindow(contentViewController: tabs)
        window.styleMask = [.titled, .closable]
        window.isReleasedWhenClosed = false
        // Comes up where the user is, over an app in full screen too, also when it was opened before in another space
        window.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary]
        super.init()
        let pages: [(AnyView, String)] = [
            (AnyView(GeneralSettings()), "gearshape"),
            (AnyView(ReminderSettings()), "bell"),
            (AnyView(OverlaySettings()), "rectangle.inset.topright.filled"),
        ]
        for (index, (page, symbol)) in pages.enumerated() {
            let measured = MeasuredPage(page: page.environmentObject(model)) { [weak self] height in
                self?.pageHeight(index, height)
            }
            let host = NSHostingController(rootView: measured)
            // The window's size is this controller's to set, smoothly: the page's own would set it at once
            host.sizingOptions = []
            let item = NSTabViewItem(viewController: host)
            item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
            tabs.addTabViewItem(item)
        }
        tabs.selected = { [weak self] index in
            guard let self, let height = self.heights[index] else { return }
            self.resize(to: height)
        }
        window.delegate = self
        languageChanged()
        // Opens as tall as its first page
        if let first = tabs.tabViewItems.first?.viewController?.view.fittingSize, first.height > 0 {
            window.setContentSize(NSSize(width: 500, height: first.height))
        }
        window.center()
    }

    var isVisible: Bool {
        window.isVisible
    }

    func show() {
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    func close() {
        window.close()
    }

    private func pageHeight(_ index: Int, _ height: CGFloat) {
        heights[index] = height
        if index == tabs.selectedTabViewItemIndex {
            resize(to: height)
        }
    }

    /// The window as tall as the open page, its top edge where it was; smoothly while it is seen.
    private func resize(to height: CGFloat) {
        let content = window.contentRect(forFrameRect: window.frame)
        guard height > 0, abs(content.height - height) > 0.5 else { return }
        var frame = window.frameRect(forContentRect: NSRect(x: content.minX, y: content.minY, width: content.width,
                                                            height: height))
        frame.origin.y = window.frame.maxY - frame.height
        guard window.isVisible else {
            window.setFrame(frame, display: false)
            return
        }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = Self.duration
            context.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            window.animator().setFrame(frame, display: true)
        }
    }

    func languageChanged() {
        // The window's title is the open tab's, which the tab view takes from its page
        let labels = ["general.header", "alerts.header", "overlay.header"].map(model.t)
        for (item, label) in zip(tabs.tabViewItems, labels) {
            item.label = label
            item.viewController?.title = label
        }
        if let selected = tabs.tabViewItems[safe: tabs.selectedTabViewItemIndex] {
            window.title = selected.label
        }
    }

    func windowWillClose(_ notification: Notification) {
        // A value being typed is kept, as when the field is left
        window.makeFirstResponder(nil)
        closed()
    }

    /// Another window or app comes to the front: a value being typed is applied, as on Windows.
    func windowDidResignKey(_ notification: Notification) {
        window.makeFirstResponder(nil)
    }

    /// Back in front: the Sounds folder may have changed meanwhile (a file removed in Finder), and the menus with it.
    func windowDidBecomeKey(_ notification: Notification) {
        model.refresh()
    }
}

/// The tabs; they say which one was chosen, for the window to take its height.
final class SettingsTabs: NSTabViewController {
    var selected: (Int) -> Void = { _ in }

    override func tabView(_ tabView: NSTabView, didSelect tabViewItem: NSTabViewItem?) {
        super.tabView(tabView, didSelect: tabViewItem)
        selected(selectedTabViewItemIndex)
    }
}

/// A settings page that tells its height, at the top of the window while the window catches up with it.
private struct MeasuredPage<Page: View>: View {
    let page: Page
    let height: (CGFloat) -> Void

    var body: some View {
        page
            .frame(width: 500)
            .background(GeometryReader { proxy in
                Color.clear.preference(key: PageHeight.self, value: proxy.size.height)
            })
            .onPreferenceChange(PageHeight.self) { height($0) }
            .frame(maxHeight: .infinity, alignment: .top)
    }
}

private struct PageHeight: PreferenceKey {
    static let defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}

/// An ⓘ after a label. Its hint opens next to it a moment after the pointer comes (and goes when the pointer leaves),
/// or at once on a click, and then stays until a click on the ⓘ or anywhere else. macOS' own tooltip comes after a
/// second or two and never on a click.
struct Hint: View {
    let text: String
    @StateObject private var state = HintState()

    var body: some View {
        Button { state.click() } label: {
            Image(systemName: "info.circle")
                .foregroundStyle(state.hovering || state.shown ? Color.accentColor : Color.secondary)
                .padding(2)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { state.hover($0) }
        .popover(isPresented: $state.shown, arrowEdge: .bottom) {
            Text(text)
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
                .frame(width: 260, alignment: .leading)
                .padding(12)
        }
        .accessibilityLabel(text)
    }
}

/// Whether a hint is open, and whether a click opened it (then the pointer leaving doesn't close it).
@MainActor
final class HintState: ObservableObject {
    private static let delay: TimeInterval = 0.3
    @Published var shown = false {
        didSet {
            if !shown { pinned = false }
        }
    }
    @Published private(set) var hovering = false
    private var pinned = false
    private var pending: DispatchWorkItem?

    func hover(_ inside: Bool) {
        hovering = inside
        pending?.cancel()
        if inside {
            let open = DispatchWorkItem { [weak self] in
                guard let self, self.hovering else { return }
                self.shown = true
            }
            pending = open
            DispatchQueue.main.asyncAfter(deadline: .now() + Self.delay, execute: open)
        } else if !pinned {
            shown = false
        }
    }

    func click() {
        pending?.cancel()
        if shown && pinned {
            shown = false
        } else {
            shown = true
            pinned = true
        }
    }
}

/// A button that is a whole row of a settings list, clicked anywhere on it: its text and symbol in the accent color,
/// as a link's.
struct RowButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(Color.accentColor)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .opacity(configuration.isPressed ? 0.6 : 1)
    }
}

/// "New" in a small outlined capsule next to a section that is new in this version; taken away in the next one.
struct NewBadge: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        Text(model.t("settings.new"))
            .font(.caption.weight(.semibold))
            .foregroundStyle(Color.accentColor)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .overlay(Capsule().strokeBorder(Color.accentColor, lineWidth: 1))
    }
}

private struct SettingsRowIcon: View {
    let symbol: String

    var body: some View {
        Image(systemName: symbol)
            .resizable()
            .scaledToFit()
            .frame(width: 16, height: 16)
    }
}

private struct SettingsRowLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 8) {
            configuration.icon.frame(width: 22, height: 18)
            configuration.title.multilineTextAlignment(.leading)
        }
    }
}

/// General: the login item, the Input Monitoring permission when a device waits for it, the language and the theme, how
/// many devices the menu bar item shows, the updates, the folder of the log and settings, a suggestion.
struct GeneralSettings: View {
    @EnvironmentObject var model: AppModel

    /// "Checked at 15:41" after a "·" reads "checked at 15:41".
    static func lowerFirst(_ text: String) -> String {
        guard let first = text.first else { return text }
        return first.lowercased() + text.dropFirst()
    }

    private func settingsLabel(_ key: String, systemImage: String) -> some View {
        Label {
            Text(model.t(key))
        } icon: {
            SettingsRowIcon(symbol: systemImage)
        }
    }

    var body: some View {
        if let s = model.snapshot {
            Form {
                Section {
                    Toggle(isOn: Binding(get: { s.autostart }, set: { on in
                        if on {
                            Installer.enableAutostart(model)
                        } else {
                            model.send(["cmd": "autostart", "on": false])
                        }
                    })) {
                        settingsLabel("autostart", systemImage: "power")
                    }
                }
                if s.needsPermission {
                    Section {
                        Label {
                            VStack(alignment: .leading, spacing: 8) {
                                Text(model.t("permission.text")).fixedSize(horizontal: false, vertical: true)
                                HStack {
                                    Button(model.t("permission.allow")) { Permissions.askForInputMonitoring(model) }
                                    Button(model.t("permission.settings")) { Permissions.openInputMonitoringSettings() }
                                }
                            }
                        } icon: {
                            SettingsRowIcon(symbol: "hand.raised").foregroundStyle(.orange)
                        }
                    }
                }
                Section {
                    Picker(selection: Binding(get: { s.settings.language }, set: { model.send(["cmd": "language", "value": $0]) })) {
                        Text(model.t("lang.auto")).tag("auto")
                        Text("Русский").tag("ru")
                        Text("English").tag("en")
                        Text("中文").tag("zh")
                    } label: {
                        settingsLabel("lang.header", systemImage: "globe")
                    }
                    Picker(selection: Binding(get: { s.settings.theme }, set: { model.send(["cmd": "theme", "value": $0]) })) {
                        Text(model.t("theme.auto")).tag("auto")
                        Text(model.t("theme.light")).tag("light")
                        Text(model.t("theme.dark")).tag("dark")
                    } label: {
                        settingsLabel("theme.header", systemImage: "circle.lefthalf.filled")
                    }
                }
                Section {
                    Picker(selection: Binding(get: { s.settings.menuBarDevices }, set: { model.set("menu_bar_devices", $0) })) {
                        ForEach(1...5, id: \.self) { Text("\($0)").tag($0) }
                    } label: {
                        settingsLabel("menubar.devices", systemImage: "menubar.rectangle")
                    }
                    if s.settings.menuBarDevices == 1 {
                        Picker(selection: Binding(get: { s.settings.trayPriority }, set: { model.set("tray_priority", $0) })) {
                            Text(model.t("tray.priority.mouse")).tag("mouse")
                            Text(model.t("tray.priority.keyboard")).tag("keyboard")
                            Text(model.t("tray.priority.bluetooth")).tag("bluetooth")
                            Text(model.t("tray.priority.headset")).tag("headset")
                            Text(model.t("tray.priority.lowest")).tag("lowest")
                        } label: {
                            settingsLabel("tray.priority", systemImage: "list.number")
                        }
                    }
                    Picker(selection: Binding(get: { s.settings.trayBatteryIcon },
                                              set: { model.set("tray_battery_icon", $0) })) {
                        Text(model.t("tray.style.icon")).tag(true)
                        Text(model.t("tray.style.number")).tag(false)
                    } label: {
                        settingsLabel("tray.style", systemImage: "battery.100percent")
                    }
                    Toggle(isOn: Binding(get: { s.settings.hideOffline }, set: { model.set("hide_offline", $0) })) {
                        settingsLabel("hide.offline", systemImage: "eye.slash")
                    }
                } header: {
                    HStack(spacing: 8) {
                        Text(model.t("menubar.header"))
                        NewBadge()
                    }
                }
                Section(model.t("updates.header")) {
                    VStack(alignment: .leading, spacing: 4) {
                        // The version and when it was asked, one line: "You have the latest version 2.2.1 · checked
                        // at 15:41"
                        (Text(s.update.status)
                            + Text(s.update.checked.isEmpty ? "" : " · " + Self.lowerFirst(s.update.checked))
                            .foregroundColor(.secondary))
                            .fixedSize(horizontal: false, vertical: true)
                        if !s.update.notes.isEmpty {
                            Text(s.update.notes)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                                .padding(.top, 4)
                        }
                        HStack {
                            if s.update.installVisible {
                                Button(s.update.installText) { model.send(["cmd": "update_install"]) }
                                    .buttonStyle(.borderedProminent)
                            }
                            Button(model.t("updates.check")) { model.send(["cmd": "update_check"]) }
                                .disabled(!s.update.checkEnabled)
                        }
                        .padding(.top, 4)
                    }
                    Toggle(isOn: Binding(get: { s.settings.autoUpdate }, set: { model.set("auto_update", $0) })) {
                        settingsLabel("updates.auto", systemImage: "arrow.clockwise")
                    }
                }
                Section {
                    Button {
                        if let path = Core.call(["cmd": "log_folder"])["path"] as? String {
                            try? FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
                            NSWorkspace.shared.open(URL(fileURLWithPath: path))
                        }
                    } label: {
                        settingsLabel("open.folder", systemImage: "folder")
                    }
                    Button {
                        if let text = Core.call(["cmd": "suggestion_url"])["url"] as? String, let url = URL(string: text) {
                            NSWorkspace.shared.open(url)
                        }
                    } label: {
                        Label {
                            HStack(spacing: 4) {
                                Text(model.t("feedback"))
                                Image(systemName: "arrow.up.forward.square")
                                    .foregroundStyle(.secondary)
                                    .accessibilityLabel(model.t("feedback.browser"))
                            }
                        } icon: {
                            SettingsRowIcon(symbol: "lightbulb")
                        }
                    }
                }
                .buttonStyle(RowButtonStyle())
            }
            .formStyle(.grouped)
            .labelStyle(SettingsRowLabelStyle())
            .fixedSize(horizontal: false, vertical: true)
            .animation(.easeInOut(duration: SettingsWindowController.duration), value: s.needsPermission)
            .animation(.easeInOut(duration: SettingsWindowController.duration), value: s.update)
        }
    }
}

/// Reminders: the low and critical levels and how often a reminder comes back, whether it closes on its own and after
/// how many seconds, the "charged" notice, the sounds of the reminder and of that notice, a sample.
struct ReminderSettings: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        if let s = model.snapshot?.settings {
            Form {
                Section {
                    LevelSetting(title: model.t("alerts.low"), hint: nil, value: s.low, range: 10...50,
                                 unit: "%", current: { model.snapshot?.settings.low ?? s.low }) { model.set("low", $0) }
                    LevelSetting(title: model.t("alerts.critical"), hint: model.t("alerts.critical.hint"),
                                 value: s.critical, range: 5...45, unit: "%",
                                 current: { model.snapshot?.settings.critical ?? s.critical }) { model.set("critical", $0) }
                    LevelSetting(title: model.t("alerts.repeat"), hint: nil, value: s.repeatMinutes, range: 1...60,
                                 unit: model.t("alerts.repeat.unit"), step: 1,
                                 current: { model.snapshot?.settings.repeatMinutes ?? s.repeatMinutes }) { model.set("repeat", $0) }
                }
                Section {
                    Toggle(model.t("alerts.autoclose"), isOn: Binding(get: { s.autoClose }, set: { model.set("auto_close", $0) }))
                    if s.autoClose {
                        LevelSetting(title: model.t("alerts.autoclose.after"), hint: nil, value: s.autoCloseSeconds,
                                     range: 1...15, unit: model.t("alerts.autoclose.unit"), step: 1,
                                     current: { model.snapshot?.settings.autoCloseSeconds ?? s.autoCloseSeconds }) {
                            model.set("auto_close_seconds", $0)
                        }
                    }
                }
                Section {
                    Toggle(isOn: Binding(get: { s.notifyCharged }, set: { model.set("notify_charged", $0) })) {
                        HStack(spacing: 6) {
                            Text(model.t("charged.toggle"))
                            Hint(text: model.t("charged.hint"))
                        }
                    }
                }
                Section {
                    SoundSetting(event: "low", title: model.t("sounds.low"), chosen: s.soundLow)
                    SoundSetting(event: "critical", title: model.t("sounds.critical"), chosen: s.soundCritical)
                    // The notice's sound is here while the notice is
                    if s.notifyCharged {
                        SoundSetting(event: "charged", title: model.t("sounds.charged"), chosen: s.soundCharged)
                    }
                } header: {
                    HStack(spacing: 8) {
                        Text(model.t("sounds.header"))
                        NewBadge()
                    }
                } footer: {
                    // What there is to say about a chosen file: that only its beginning will sound
                    let shown = s.notifyCharged ? s.soundNotes : Array(s.soundNotes.prefix(2))
                    if let note = shown.first(where: { !$0.isEmpty }) {
                        Text(note)
                    }
                }
                Section {
                    Button {
                        model.send(["cmd": "test_alert"])
                    } label: {
                        Label {
                            Text(model.t("alerts.test"))
                        } icon: {
                            SettingsRowIcon(symbol: "bell.badge")
                        }
                        .labelStyle(SettingsRowLabelStyle())
                    }
                    .buttonStyle(RowButtonStyle())
                }
            }
            .formStyle(.grouped)
            .fixedSize(horizontal: false, vertical: true)
            // "Close after" opens and closes smoothly with its switch, the notice's sound with the notice's
            .animation(.easeInOut(duration: SettingsWindowController.duration), value: s.autoClose)
            .animation(.easeInOut(duration: SettingsWindowController.duration), value: s.notifyCharged)
        }
    }
}

/// A sound: a pop-up menu of the sounds, the chosen one checked. Choosing one plays it, as the system's own lists of
/// sounds do (Mail's, System Settings'), so there is no button for hearing it. The menu's last item, "Custom file…", is
/// no sound: it opens the system's open panel.
struct SoundSetting: View {
    @EnvironmentObject var model: AppModel
    /// "low", "critical" or "charged"
    let event: String
    let title: String
    /// The chosen sound's `SoundOption.id`
    let chosen: String
    /// The menu items that are no sounds (no sound has these ids): the one that adds a file, and the one that shows
    /// the user's files in Finder, which is where one is removed
    private static let add = "add"
    private static let reveal = "reveal"

    var body: some View {
        let sounds = model.snapshot?.sounds ?? []
        Picker(title, selection: Binding(get: { chosen }, set: { picked in
            if picked == Self.add {
                // Once the menu has closed
                DispatchQueue.main.async {
                    MainActor.assumeIsolated { SoundFiles.choose(for: event, model: model) }
                }
            } else if picked == Self.reveal {
                SoundFiles.reveal(chosen: chosen)
            } else {
                model.send(["cmd": "set_sound", "event": event, "value": picked])
            }
        })) {
            ForEach(sounds.filter { $0.group == 0 }) { Text($0.name).tag($0.id) }
            Divider()
            ForEach(sounds.filter { $0.group == 1 }) { Text($0.name).tag($0.id) }
            Divider()
            ForEach(sounds.filter { $0.group == 2 }) { Text($0.name).tag($0.id) }
            Text(model.t("sounds.file")).tag(Self.add)
            // Only while there is a file of the user's to show
            if sounds.contains(where: { $0.group == 2 }) {
                Text(model.t("sounds.reveal")).tag(Self.reveal)
            }
        }
    }
}

/// The system's open panel for a sound file, as a sheet of the window the menu was in: a WAV or an MP3 can be picked,
/// and the button says "Choose". The core copies the file into the app's Sounds folder, has it chosen and played, or
/// says that it can't take it.
@MainActor
enum SoundFiles {
    static func choose(for event: String, model: AppModel) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.wav, .mp3]
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = model.t("sounds.choose")
        let picked: (NSApplication.ModalResponse) -> Void = { response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated {
                model.send(["cmd": "add_sound", "event": event, "path": url.path])
            }
        }
        if let window = NSApp.keyWindow {
            panel.beginSheetModal(for: window, completionHandler: picked)
        } else {
            picked(panel.runModal())
        }
    }

    /// Shows the Sounds folder in Finder, with the chosen file selected when it is one of the user's. A file moved to
    /// the Trash there is gone from the menus when the settings window is back in front.
    static func reveal(chosen: String) {
        guard let folder = Core.call(["cmd": "sounds_folder"])["path"] as? String else { return }
        let own = "file:"
        if chosen.hasPrefix(own) {
            let file = URL(fileURLWithPath: folder).appendingPathComponent(String(chosen.dropFirst(own.count)))
            NSWorkspace.shared.activateFileViewerSelecting([file])
        } else {
            NSWorkspace.shared.open(URL(fileURLWithPath: folder))
        }
    }
}

/// A level: the number as typed (accent colored) and a slider that moves in steps (of 5 unless told otherwise). The
/// field takes more than the slider covers (settings.rs keeps a value in what it accepts: LEVEL_PERCENT,
/// REPEAT_MINUTES, AUTO_CLOSE_SECONDS); the slider then stands at its end.
struct LevelSetting: View {
    let title: String
    let hint: String?
    let value: Int
    let range: ClosedRange<Int>
    let unit: String
    var step = 5
    let current: () -> Int
    let change: (Int) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(title)
                if let hint {
                    Hint(text: hint)
                }
                Spacer()
                NumberField(value: value, unit: unit, label: title, current: current, commit: change)
                    .foregroundStyle(Color.accentColor)
            }
            let position = Binding(
                get: { Double(min(max(value, range.lowerBound), range.upperBound)) },
                set: { (fresh: Double) in
                    let stepped = Int(fresh.rounded())
                    if stepped != current() { change(stepped) }
                })
            let bounds = Double(range.lowerBound)...Double(range.upperBound)
            Group {
                if step > 1 {
                    Slider(value: position, in: bounds, step: Double(step)) { Text(title) }
                } else {
                    // A step of 1 would draw a tick mark for every value: the slider runs smoothly instead and the
                    // value is the nearest whole number
                    Slider(value: position, in: bounds) { Text(title) }
                }
            }
            .labelsHidden()
        }
    }
}

/// Overlay: the shortcut that shows the battery panel over any app, how long it stays, its corner, the names.
struct OverlaySettings: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        if let snapshot = model.snapshot {
            let s = snapshot.settings
            Form {
                Section {
                    Toggle(isOn: Binding(get: { s.overlayEnabled }, set: { model.set("overlay_enabled", $0) })) {
                        HStack(spacing: 6) {
                            Text(model.t("overlay.enabled"))
                            Hint(text: model.t("overlay.compatibility"))
                        }
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Text(model.t("overlay.shortcut"))
                            Spacer()
                            ShortcutRecorder(shortcut: snapshot.overlay.shortcut)
                            Button {
                                ShortcutRecorder.take(Core.decode(Shortcut.self, ["cmd": "default_shortcut"]))
                            } label: {
                                Image(systemName: "arrow.uturn.backward")
                            }
                            .help(model.t("overlay.reset"))
                            .accessibilityLabel(model.t("overlay.reset"))
                        }
                        if !model.shortcutError.isEmpty {
                            Text(model.shortcutError)
                                .font(.caption)
                                .foregroundStyle(.red)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    Picker(model.t("overlay.mode"), selection: Binding(get: { s.overlayMode }, set: { model.set("overlay_mode", $0) })) {
                        Text(model.t("overlay.timed")).tag(0)
                        Text(model.t("overlay.toggle")).tag(1)
                    }
                    Picker(model.t("overlay.corner"), selection: Binding(get: { s.overlayCorner }, set: { model.set("overlay_corner", $0) })) {
                        Text(model.t("overlay.top.left")).tag(0)
                        Text(model.t("overlay.top.right")).tag(1)
                        Text(model.t("overlay.bottom.left")).tag(2)
                        Text(model.t("overlay.bottom.right")).tag(3)
                    }
                    Toggle(model.t("overlay.names"), isOn: Binding(get: { s.overlayNames }, set: { model.set("overlay_names", $0) }))
                }
                Section {
                    Button {
                        AppDelegate.shared?.overlay.preview()
                    } label: {
                        Label {
                            Text(model.t("overlay.preview"))
                        } icon: {
                            SettingsRowIcon(symbol: "eye")
                        }
                        .labelStyle(SettingsRowLabelStyle())
                    }
                    .buttonStyle(RowButtonStyle())
                }
            }
            .formStyle(.grouped)
            .fixedSize(horizontal: false, vertical: true)
            // Why a shortcut doesn't work comes and goes smoothly
            .animation(.easeInOut(duration: SettingsWindowController.duration), value: model.shortcutError)
        }
    }
}
