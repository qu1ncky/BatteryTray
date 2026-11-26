import AppKit
import SwiftUI

/// A window that floats above other apps' windows and full-screen apps, on every Space, and never takes the focus or
/// activates the app: the reminder and the overlay.
final class FloatingPanel: NSPanel {
    init(clickThrough: Bool) {
        super.init(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        isFloatingPanel = true
        level = .statusBar
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        hidesOnDeactivate = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        ignoresMouseEvents = clickThrough
        isReleasedWhenClosed = false
        becomesKeyOnlyIfNeeded = true
        animationBehavior = .utilityWindow
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    /// Takes the size its contents want.
    func fit(_ host: NSView) {
        let size = host.fittingSize
        setContentSize(size)
        host.frame = NSRect(origin: .zero, size: size)
    }
}

// MARK: - The reminder

/// What the reminder shows, and how much of its time is left when it closes on its own.
final class ReminderState: ObservableObject {
    @Published var effect: [String: Any] = [:]
    /// It closes on its own: the line along its bottom edge shows the share of its time left, 1 to 0
    @Published var autoClose = false
    @Published var remaining: Double = 1
}

/// The low battery reminder: in the top right corner, where macOS shows its notifications, on top of everything and
/// never taking the focus, until the user snoozes it or mutes it, or, when the settings say so, until it closes on its
/// own as if snoozed. Its time runs only while someone is at the Mac, so it is still there for a user who was away, and
/// while the pointer is off it, so it doesn't close under a hand on its way to a button. The sample (`test`) only
/// closes.
@MainActor
final class ReminderController {
    /// How often the line of the time left moves
    private static let tick: TimeInterval = 1.0 / 30
    /// The README's pictures: the line stands still at this share of the time
    static var pictureRemaining: Double?
    /// The reminder on the screen, whose line the README's frames move (`--snapshot … frames`)
    static weak var shown: ReminderController?

    private let test: Bool
    private var panel: FloatingPanel?
    private let state = ReminderState()
    private var keepOnTop: Timer?
    /// The devices shown, each with its severity: one it didn't show starts its time again
    private var shown: Set<String> = []
    /// Its whole time when it closes on its own (nil: it waits for an answer), the time left, and when the time last
    /// ran
    private var total: TimeInterval?
    private var left: TimeInterval = 0
    private var lastTick: TimeInterval?
    private var countdown: Timer?
    private let presence = Presence()

    init(test: Bool) {
        self.test = test
    }

    /// Where it is, for the overlay to stay clear of it.
    var frame: NSRect? {
        panel.flatMap { $0.isVisible ? $0.frame : nil }
    }

    func show(_ effect: [String: Any]) {
        Self.shown = self
        let items = effect["items"] as? [[String: Any]] ?? []
        let keys = Set(items.map { "\($0["id"] as? String ?? "")|\($0["critical"] as? Bool ?? false)" })
        let autoClose = (effect["auto_close"] as? NSNumber)?.doubleValue
        if !keys.isSubset(of: shown) || autoClose != total {
            total = autoClose
            left = autoClose ?? 0
            lastTick = nil
            state.remaining = 1
        }
        shown = keys
        state.autoClose = autoClose != nil
        state.effect = effect
        if let picture = Self.pictureRemaining {
            state.remaining = picture
        } else if autoClose == nil {
            stopCountdown()
        } else if countdown == nil {
            countdown = Timer.scheduledTimer(withTimeInterval: Self.tick, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.runCountdown() }
            }
        }

        let panel = self.panel ?? makePanel()
        self.panel = panel
        if let host = panel.contentView {
            panel.fit(host)
        }
        place()
        panel.orderFrontRegardless()
        // Full-screen apps and other floating windows can push it back; reasserted every few seconds
        keepOnTop?.invalidate()
        keepOnTop = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.panel?.orderFrontRegardless()
                self?.place()
            }
        }
    }

    /// The README's frames: the line of the time left at this share.
    func picture(_ remaining: Double) {
        state.remaining = remaining
    }

    func close() {
        keepOnTop?.invalidate()
        keepOnTop = nil
        stopCountdown()
        shown = []
        total = nil
        lastTick = nil
        panel?.orderOut(nil)
    }

    private func stopCountdown() {
        countdown?.invalidate()
        countdown = nil
    }

    /// The time runs while the reminder is seen, someone is at the Mac and the pointer is off it; at its end the
    /// reminder snoozes, as by the button.
    private func runCountdown() {
        guard let total, let panel else { return }
        let now = ProcessInfo.processInfo.systemUptime
        let last = lastTick
        lastTick = now
        guard panel.isVisible, !panel.frame.contains(NSEvent.mouseLocation), presence.someoneThere(now),
              let last
        else { return }
        left = max(0, left - (now - last))
        state.remaining = left / total
        if left == 0 {
            self.total = nil
            stopCountdown()
            DispatchQueue.main.async { [weak self] in self?.answer(snooze: true, onItsOwn: true) }
        }
    }

    /// "Remind me in N min" (also when its time ran out) or "Don't remind me until it's charged".
    private func answer(snooze: Bool, onItsOwn: Bool = false) {
        if test {
            close()
            // The sample's sound ends with it, as the reminder's does
            Sounds.stop()
        } else if snooze {
            AppModel.shared.send(["cmd": "snooze", "auto": onItsOwn])
        } else {
            AppModel.shared.send(["cmd": "mute"])
        }
    }

    private func makePanel() -> FloatingPanel {
        let panel = FloatingPanel(clickThrough: false)
        let view = ReminderView(state: state) { [weak self] snooze in self?.answer(snooze: snooze) }
        let host = NSHostingView(rootView: view.environmentObject(AppModel.shared))
        panel.contentView = host
        return panel
    }

    /// The top right corner of the main screen's free area (16 points in), as tall as its contents.
    private func place() {
        guard let panel, let host = panel.contentView, let area = NSScreen.main?.visibleFrame else { return }
        panel.fit(host)
        let size = panel.frame.size
        panel.setFrameOrigin(NSPoint(x: area.maxX - size.width - 16, y: area.maxY - size.height - 16))
    }
}

/// Whether someone is at the Mac, for the reminder's time: not while the screen is locked, the screen saver runs, the
/// displays sleep or another user has the screen; yes while an app in front fills a screen (a game played with a
/// controller, whose buttons macOS doesn't count as the user's input, or a film); otherwise when the keyboard, the
/// mouse or the trackpad was used in the last half minute. Asked of macOS at most once a second.
@MainActor
final class Presence {
    private static let awayAfter: TimeInterval = 30
    private var asked: (at: TimeInterval, there: Bool)?
    private var screenSaver = false
    private var locked = false
    private var observers: [NSObjectProtocol] = []

    init() {
        let names = ["com.apple.screensaver.didstart", "com.apple.screensaver.didstop", "com.apple.screenIsLocked",
                     "com.apple.screenIsUnlocked"]
        for name in names {
            observers.append(DistributedNotificationCenter.default().addObserver(
                forName: Notification.Name(name), object: nil, queue: .main
            ) { [weak self] note in
                let heard = note.name.rawValue
                MainActor.assumeIsolated { self?.heard(heard) }
            })
        }
    }

    private func heard(_ name: String) {
        switch name {
        case "com.apple.screensaver.didstart": screenSaver = true
        case "com.apple.screensaver.didstop": screenSaver = false
        case "com.apple.screenIsLocked": locked = true
        case "com.apple.screenIsUnlocked": locked = false
        default: return
        }
        asked = nil
    }

    func someoneThere(_ now: TimeInterval) -> Bool {
        if let asked, now - asked.at < 1 {
            return asked.there
        }
        let there = ask()
        asked = (now, there)
        return there
    }

    private func ask() -> Bool {
        if locked || screenSaver {
            return false
        }
        if let session = CGSessionCopyCurrentDictionary() as? [String: Any] {
            if session["CGSSessionScreenIsLocked"] as? Bool == true {
                return false
            }
            if session[kCGSessionOnConsoleKey as String] as? Bool == false {
                return false
            }
        }
        if CGDisplayIsAsleep(CGMainDisplayID()) != 0 {
            return false
        }
        if Self.fullScreenAppInFront() {
            return true
        }
        // kCGAnyInputEventType
        guard let anyInput = CGEventType(rawValue: ~0) else { return true }
        return CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: anyInput) < Self.awayAfter
    }

    /// The app in front has a window that fills one of the screens (in full screen, the part under the camera housing
    /// of a notched display stays out of it).
    private static func fullScreenAppInFront() -> Bool {
        guard let app = NSWorkspace.shared.frontmostApplication, app.processIdentifier != getpid(),
              let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
                as? [[String: Any]],
              let primary = NSScreen.screens.first
        else { return false }
        for window in windows {
            guard (window[kCGWindowOwnerPID as String] as? pid_t) == app.processIdentifier,
                  (window[kCGWindowLayer as String] as? Int) == 0,
                  let b = window[kCGWindowBounds as String] as? [String: CGFloat]
            else { continue }
            let bounds = CGRect(x: b["X"] ?? 0, y: b["Y"] ?? 0, width: b["Width"] ?? 0, height: b["Height"] ?? 0)
            for screen in NSScreen.screens {
                // Window bounds count down from the top of the main display; screens count up from its bottom
                let frame = screen.frame
                let top = primary.frame.maxY - frame.maxY
                if abs(bounds.minX - frame.minX) < 1, abs(bounds.width - frame.width) < 1,
                   bounds.minY <= top + screen.safeAreaInsets.top + 1, bounds.maxY >= top + frame.height - 1 {
                    return true
                }
            }
        }
        return false
    }
}

/// The reminder, as on Windows: a header with the app's name and the cross (which snoozes); each device due on a tile,
/// its level bar under its name and its level in the severity's color; the two answers. The border stays neutral; a
/// line along the bottom edge shows the time left when it closes on its own.
struct ReminderView: View {
    @ObservedObject var state: ReminderState
    let answer: (Bool) -> Void
    static let width: CGFloat = 340

    var body: some View {
        let items = state.effect["items"] as? [[String: Any]] ?? []
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 7) {
                Image(systemName: "battery.100")
                    .font(.system(size: 12))
                Text("BatteryTray")
                    .font(.system(size: 12))
                Spacer()
                Button { answer(true) } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 11, weight: .semibold))
                        .frame(width: 26, height: 26)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .help(AppModel.shared.t("alert.snooze.tip"))
                .accessibilityLabel(AppModel.shared.t("alert.snooze.tip"))
            }
            .foregroundStyle(.secondary)
            .padding(.leading, 16)
            .padding(.trailing, 7)
            .padding(.top, 6)
            VStack(spacing: 12) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    let color = Tone.color(item["critical"] as? Bool == true ? "bad" : "warn")
                    HStack(spacing: 12) {
                        Image(systemName: symbol(item["kind"] as? String ?? ""))
                            .font(.system(size: 18))
                            .frame(width: 40, height: 40)
                            .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.08)))
                        VStack(alignment: .leading, spacing: 8) {
                            Text(item["name"] as? String ?? "")
                                .font(.system(size: 13, weight: .semibold))
                                .lineLimit(1)
                                .truncationMode(.tail)
                            LevelBar(fill: ((item["percent"] as? NSNumber)?.doubleValue ?? 0) / 100, color: color, faint: false)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        Text(item["level"] as? String ?? "")
                            .font(.system(size: 24, weight: .semibold))
                            .monospacedDigit()
                            .foregroundStyle(color)
                            .frame(minWidth: 56, alignment: .trailing)
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 4)
            VStack(spacing: 8) {
                Button { answer(true) } label: { Text(state.effect["snooze"] as? String ?? "") }
                    .buttonStyle(ReminderButtonStyle(prominent: true))
                Button { answer(false) } label: { Text(AppModel.shared.t("alert.mute")) }
                    .buttonStyle(ReminderButtonStyle(prominent: false))
            }
            .padding(.horizontal, 16)
            .padding(.top, 16)
        }
        .padding(.bottom, 16)
        .frame(width: Self.width)
        .background(VisualEffect())
        .overlay(alignment: .bottomLeading) {
            if state.autoClose {
                Rectangle()
                    .fill(Color.accentColor)
                    .frame(width: Self.width * min(max(state.remaining, 0), 1), height: 3)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.primary.opacity(0.18), lineWidth: 1))
    }
}

/// The reminder's two buttons, the snooze one in the accent color. Drawn by hand: the panel is never the key window,
/// and macOS shows a prominent button of a window that isn't grey.
struct ReminderButtonStyle: ButtonStyle {
    let prominent: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13, weight: prominent ? .semibold : .regular))
            .foregroundStyle(prominent ? Color.white : Color.primary)
            .frame(maxWidth: .infinity, minHeight: 32)
            .background(
                RoundedRectangle(cornerRadius: 7)
                    .fill(prominent ? Color.accentColor : Color.primary.opacity(0.1))
            )
            .opacity(configuration.isPressed ? 0.75 : 1)
            .contentShape(Rectangle())
    }
}

/// The blurred material of macOS' own floating panels.
struct VisualEffect: NSViewRepresentable {
    var material: NSVisualEffectView.Material = .popover

    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = material
        view.blendingMode = .behindWindow
        view.state = .active
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {
        view.material = material
    }
}

// MARK: - The overlay

/// Visibility on a monotonic clock, as on Windows: mode 0 shows for 5 seconds (again from the start on each press),
/// mode 1 toggles; a preview always times out.
struct OverlaySession {
    static let duration: TimeInterval = 5
    private(set) var visible = false
    private(set) var timed = false
    private var started: TimeInterval = 0

    var isTimed: Bool { visible && timed }

    mutating func press(mode: Int, now: TimeInterval) {
        if mode == 1 && visible {
            visible = false
        } else {
            show(expire: mode != 1, now: now)
        }
    }

    mutating func preview(now: TimeInterval) {
        show(expire: true, now: now)
    }

    private mutating func show(expire: Bool, now: TimeInterval) {
        visible = true
        timed = expire
        started = now
    }

    mutating func hide() {
        visible = false
    }

    mutating func expire(now: TimeInterval) {
        if visible && timed && now - started >= Self.duration {
            visible = false
        }
    }
}

final class OverlayState: ObservableObject {
    @Published var entries: [OverlayEntry] = []
    @Published var rowLimit: CGFloat = 480
}

/// The battery overlay: a click-through panel on the screen of the app in front, in its corner, shown by the global
/// shortcut for 5 seconds or until pressed again; it stays clear of the reminder.
@MainActor
final class OverlayController {
    let binding = HotkeyBinding()
    private var session = OverlaySession()
    private var panel: FloatingPanel?
    private let state = OverlayState()
    private var expiry: Timer?
    private var placement: Timer?
    private var mode = 0
    private var corner = 1
    private var enabled = false
    /// The reminder's frame, which only the app knows
    var reminderFrame: () -> NSRect? = { nil }

    var isTimedVisible: Bool { session.isTimed }

    /// The settings as the snapshot has them: the shortcut registered or released (and why it doesn't work), a changed
    /// mode or turning it off hides the panel; the contents follow the devices.
    func apply(_ snapshot: Snapshot?) {
        guard let snapshot else { return }
        let s = snapshot.settings
        if mode != s.overlayMode || (enabled && !s.overlayEnabled) {
            hide()
        }
        (mode, corner, enabled) = (s.overlayMode, s.overlayCorner, s.overlayEnabled)
        let model = AppModel.shared
        if enabled {
            if !binding.tryChange(snapshot.overlay.shortcut), model.shortcutError.isEmpty {
                model.shortcutError = model.t("overlay.unavailable")
            }
        } else {
            binding.clear()
            if model.shortcutError == model.t("overlay.unavailable") {
                model.shortcutError = ""
            }
        }
        if state.entries != snapshot.overlay.entries {
            state.entries = snapshot.overlay.entries
        }
        place()
    }

    func hotkeyPressed(_ id: UInt32) {
        // While the shortcut is being recorded, pressing it does nothing
        guard !AppModel.shared.recordingShortcut, binding.isCurrent(id) else { return }
        session.press(mode: mode, now: ProcessInfo.processInfo.systemUptime)
        present()
    }

    func preview() {
        session.preview(now: ProcessInfo.processInfo.systemUptime)
        present()
    }

    private func present() {
        guard session.visible else { return hide() }
        let panel = self.panel ?? makePanel()
        self.panel = panel
        if !panel.isVisible {
            // On the screen of the app in front, where the user is looking
            let screen = activeScreen()
            state.rowLimit = rowLimit(screen.visibleFrame.width)
            place(on: screen)
            panel.alphaValue = 0.93
            panel.orderFrontRegardless()
        }
        expiry?.invalidate()
        if session.isTimed {
            expiry = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.tick() }
            }
        }
        // Device lists change size and displays come and go: placed again every few seconds while shown
        placement?.invalidate()
        placement = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.panel?.orderFrontRegardless()
                self?.place()
            }
        }
        AppDelegate.shared?.reportWindows()
    }

    private func tick() {
        session.expire(now: ProcessInfo.processInfo.systemUptime)
        if !session.visible {
            hide()
        }
    }

    func hide() {
        let was = session.isTimed
        session.hide()
        expiry?.invalidate()
        placement?.invalidate()
        panel?.orderOut(nil)
        if was {
            AppDelegate.shared?.reportWindows()
        }
    }

    /// Lets the shortcut go (the app is quitting).
    func release() {
        binding.clear()
    }

    private func makePanel() -> FloatingPanel {
        let panel = FloatingPanel(clickThrough: true)
        panel.hasShadow = false
        panel.contentView = NSHostingView(rootView: OverlayView(state: state).environmentObject(AppModel.shared))
        return panel
    }

    /// Into its corner of the screen it is on, off the reminder.
    func place() {
        guard let panel, panel.isVisible else { return }
        let screen = NSScreen.screens.first { $0.frame.contains(NSPoint(x: panel.frame.midX, y: panel.frame.midY)) }
        place(on: screen ?? activeScreen())
    }

    private func place(on screen: NSScreen) {
        guard let panel, let host = panel.contentView else { return }
        panel.fit(host)
        let origin = OverlayPlacement.place(area: screen.visibleFrame, size: panel.frame.size, corner: corner,
                                            margin: 12, obstruction: reminderFrame())
        panel.setFrameOrigin(origin)
    }

    /// The screen of the frontmost app's frontmost window; the one with the pointer when that can't be told.
    private func activeScreen() -> NSScreen {
        let pointer = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) } ?? NSScreen.main ?? NSScreen.screens[0]
        guard let app = NSWorkspace.shared.frontmostApplication,
              let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
                as? [[String: Any]],
              let window = windows.first(where: {
                  ($0[kCGWindowOwnerPID as String] as? pid_t) == app.processIdentifier
                      && ($0[kCGWindowLayer as String] as? Int) == 0
              }),
              let bounds = window[kCGWindowBounds as String] as? [String: CGFloat],
              let primary = NSScreen.screens.first
        else { return pointer }
        // Window bounds count down from the top of the main display; screens count up from its bottom
        let center = NSPoint(x: (bounds["X"] ?? 0) + (bounds["Width"] ?? 0) / 2,
                             y: primary.frame.maxY - ((bounds["Y"] ?? 0) + (bounds["Height"] ?? 0) / 2))
        return NSScreen.screens.first { $0.frame.contains(center) } ?? pointer
    }

    /// The widest a row of entries gets: half the screen, as on Windows (never under 240, never past the screen), less
    /// the panel's padding.
    private func rowLimit(_ width: CGFloat) -> CGFloat {
        min(width - 24, max(width * 0.5, 240)) - 12
    }
}

/// Where the overlay goes: its corner of the free area inside a margin, moved off the reminder when they would overlap
/// (to the nearest place that fits). As overlay::place on Windows, in a Mac screen's coordinates (y counts up).
enum OverlayPlacement {
    static func place(area: NSRect, size: NSSize, corner: Int, margin: CGFloat, obstruction: NSRect?) -> NSPoint {
        let inset = min(margin, min(area.width, area.height) / 2)
        let inner = area.insetBy(dx: inset, dy: inset)
        let (w, h) = (min(size.width, inner.width), min(size.height, inner.height))
        let right = corner == 1 || corner == 3
        let top = corner == 0 || corner == 1
        let point = NSPoint(x: right ? inner.maxX - w : inner.minX, y: top ? inner.maxY - h : inner.minY)
        guard let blocked = obstruction else { return point }
        let at = { (p: NSPoint) in NSRect(x: p.x, y: p.y, width: w, height: h) }
        // Touching edges count, as WPF's IntersectsWith
        let touches = { (a: NSRect, b: NSRect) in a.minX <= b.maxX && b.minX <= a.maxX && a.minY <= b.maxY && b.minY <= a.maxY }
        if !touches(at(point), blocked) {
            return point
        }
        let alternatives = [
            NSPoint(x: point.x, y: blocked.minY - margin - h),
            NSPoint(x: point.x, y: blocked.maxY + margin),
            NSPoint(x: blocked.minX - margin - w, y: point.y),
            NSPoint(x: blocked.maxX + margin, y: point.y),
        ]
        let fitting = alternatives.filter { inner.contains(at($0)) && !touches(at($0), blocked) }
        return fitting.min { a, b in
            hypot(a.x - point.x, a.y - point.y) < hypot(b.x - point.x, b.y - point.y)
        } ?? point
    }
}

/// The overlay's contents: each device's icon, name when shown, and level in its color, in rows no wider than the
/// limit.
struct OverlayView: View {
    @ObservedObject var state: OverlayState
    @EnvironmentObject var model: AppModel

    var body: some View {
        Group {
            if state.entries.isEmpty {
                Text(model.t("none.online")).padding(.horizontal, 8).padding(.vertical, 5)
            } else {
                FlowLayout(limit: state.rowLimit) {
                    ForEach(Array(state.entries.enumerated()), id: \.offset) { _, entry in
                        HStack(spacing: 7) {
                            Image(systemName: symbol(entry.kind)).font(.system(size: 17))
                                .foregroundStyle(Tone.color(entry.tone, alwaysDark: true))
                            if entry.showName {
                                Text(entry.name).lineLimit(1).truncationMode(.tail).frame(maxWidth: 140, alignment: .leading)
                            }
                            HStack(spacing: 3) {
                                Text(entry.level).font(.system(size: 17, weight: .semibold)).monospacedDigit()
                                if entry.power {
                                    Image(systemName: "bolt.fill").font(.system(size: 15, weight: .semibold))
                                }
                            }
                            .foregroundStyle(Tone.color(entry.tone, alwaysDark: true))
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 5)
                    }
                }
            }
        }
        .padding(6)
        .fixedSize()
        // Dark and nearly opaque whatever is underneath, a game or a document, as on Windows (#202020 at 93%)
        .background(Color(white: 0x20 / 255))
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .environment(\.colorScheme, .dark)
    }
}

/// Lays its children out in rows, starting a new row where the next one would pass `limit`.
struct FlowLayout: Layout {
    let limit: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        rows(subviews).reduce(CGSize.zero) { size, row in
            CGSize(width: max(size.width, row.width), height: size.height + row.height)
        }
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in rows(subviews) {
            var x = bounds.minX
            for index in row.indices {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: y + (row.height - size.height) / 2), proposal: .unspecified)
                x += size.width
            }
            y += row.height
        }
    }

    private func rows(_ subviews: Subviews) -> [(indices: [Int], width: CGFloat, height: CGFloat)] {
        var rows: [(indices: [Int], width: CGFloat, height: CGFloat)] = []
        for (index, subview) in subviews.enumerated() {
            let size = subview.sizeThatFits(.unspecified)
            if let last = rows.last, !last.indices.isEmpty, last.width + size.width <= limit {
                rows[rows.count - 1] = (last.indices + [index], last.width + size.width, max(last.height, size.height))
            } else {
                rows.append(([index], size.width, size.height))
            }
        }
        return rows
    }
}
