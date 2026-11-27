import AppKit
import SwiftUI

/// The device list: the update, login and permission banners, the pinned devices, the connected ones, the ones not
/// connected, and the hidden ones folded away; at the bottom when the levels were last read. A device is reordered by
/// dragging it within its block, and managed from its "⋯" menu.
struct DevicesView: View {
    @EnvironmentObject var model: AppModel
    var showHeader: Bool = false
    var onHeaderHeightChange: ((CGFloat) -> Void)? = nil
    var onFooterHeightChange: ((CGFloat) -> Void)? = nil
    var onContentChange: (() -> Void)? = nil
    /// Folded whenever the window opens
    @StateObject private var hiddenExpanded = Local(false)

    var body: some View {
        VStack(spacing: 0) {
            if showHeader {
                header
                Divider()
            }
            if let snapshot = model.snapshot {
                // One edge on each side for everything: the cards, the headings, the rows and the footer's text
                List {
                    // Room under the header's line: the first card's border doesn't run into it (16 points with the
                    // row's own 3)
                    Color.clear
                        .frame(height: 13)
                        .listRowSeparator(.hidden)
                        .listRowInsets(EdgeInsets())
                        .accessibilityHidden(true)
                    banners(snapshot)
                    // The pinned devices, then the connected ones, one list; a drag never leaves its block
                    block(snapshot.devices.pinned, card: true)
                    block(snapshot.devices.connected, card: true)
                    if snapshot.devices.noneOnline {
                        Text(model.t("none.online"))
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .center)
                            .padding(.vertical, 18)
                            .listRowSeparator(.hidden)
                            .listRowInsets(Self.insets(top: 3, bottom: 3))
                    }
                    if !snapshot.devices.offline.isEmpty {
                        Text(model.t("offline.header"))
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)
                            .padding(.top, 14)
                            .listRowSeparator(.hidden)
                            .listRowInsets(Self.insets(top: 3, bottom: 1))
                        block(snapshot.devices.offline, card: false)
                    }
                    if !snapshot.devices.hidden.isEmpty {
                        // Not a disclosure group: a list with one keeps a column for its arrow on the left of every row
                        Button { withAnimation(.easeInOut(duration: 0.2)) { hiddenExpanded.value.toggle() } } label: {
                            HStack(spacing: 5) {
                                Text("\(model.t("hidden.header")) · \(snapshot.devices.hidden.count)")
                                Image(systemName: hiddenExpanded.value ? "chevron.up" : "chevron.down")
                                    .font(.system(size: 9, weight: .bold))
                            }
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.top, 10)
                        .listRowSeparator(.hidden)
                        .listRowInsets(Self.insets(top: 3, bottom: 3))
                        if hiddenExpanded.value {
                            ForEach(snapshot.devices.hidden) { device in
                                HiddenRow(device: device)
                                    .listRowSeparator(.hidden)
                                    .listRowInsets(Self.insets(top: 1, bottom: 1))
                            }
                        }
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .environment(\.defaultMinListRowHeight, 10)
                // A banner that comes or goes moves the list smoothly
                .animation(.easeInOut(duration: 0.2), value: [
                    snapshot.update.bannerVisible,
                    !snapshot.autostart && !snapshot.settings.autostartBannerDismissed,
                    snapshot.needsPermission,
                ])
                footer(snapshot)
            }
        }
        .frame(minWidth: 380)
        .onAppear { onContentChange?() }
        .onChange(of: hiddenExpanded.value) { _ in onContentChange?() }
    }

    /// The window's edge on the left and the right, as the footer has it
    static let margin: CGFloat = 16

    /// A row's insets that put its content on the window's edge: the list keeps 8 points of its own on the left of
    /// every row and 9 on the right.
    static func insets(top: CGFloat, bottom: CGFloat) -> EdgeInsets {
        EdgeInsets(top: top, leading: margin - 8, bottom: bottom, trailing: margin - 9)
    }

    /// A block of devices, reordered by dragging; a drag never leaves its block.
    private func block(_ devices: [Device], card: Bool) -> some View {
        ForEach(devices) { device in
            DeviceRowView(device: device, card: card)
                .listRowSeparator(.hidden)
                .listRowInsets(Self.insets(top: 3, bottom: 3))
        }
        .onMove { from, to in
            guard let start = from.first, devices.indices.contains(start) else { return }
            // Dragged down it lands right below the device it was dropped on, dragged up right above it
            let onto = to > start ? to - 1 : to
            guard devices.indices.contains(onto), onto != start else { return }
            model.send(["cmd": "move", "id": devices[start].id, "onto": devices[onto].id])
        }
    }

    @ViewBuilder
    private func banners(_ s: Snapshot) -> some View {
        if s.update.bannerVisible {
            Banner(symbol: "arrow.down.circle", tint: .accentColor, text: s.update.bannerText) {
                if s.update.bannerButtonVisible {
                    Button(model.t("upd.banner.button")) { model.send(["cmd": "update_install"]) }
                        .buttonStyle(.borderedProminent)
                }
            }
        }
        if !s.autostart && !s.settings.autostartBannerDismissed {
            Banner(symbol: "power", tint: .orange, text: model.t("autostart.banner")) {
                Button(model.t("autostart.banner.enable")) { Installer.enableAutostart(model) }
                    .buttonStyle(.borderedProminent)
                Button(model.t("autostart.banner.dismiss")) { model.send(["cmd": "dismiss_autostart_banner"]) }
            }
        }
        if s.needsPermission {
            Banner(symbol: "hand.raised", tint: .orange, text: model.t("permission.banner")) {
                Button(model.t("permission.allow")) { Permissions.askForInputMonitoring(model) }
                    .buttonStyle(.borderedProminent)
            }
        }
    }

    private var header: some View {
        HStack(spacing: 4) {
            Text(model.t("app.title"))
                .font(.headline)
            Spacer()
            Button { model.send(["cmd": "refresh"]) } label: {
                Image(systemName: "arrow.clockwise")
                    .frame(width: 24, height: 24)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .help(model.t("refresh"))
            .accessibilityLabel(model.t("refresh"))
            Button { AppDelegate.shared?.openSettings() } label: {
                Image(systemName: "gearshape")
                    .frame(width: 24, height: 24)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .help(model.t("settings"))
            .accessibilityLabel(model.t("settings"))
        }
        .padding(.horizontal, 16)
        .padding(.top, 12)
        .padding(.bottom, 8)
        .background(GeometryReader { proxy in
            Color.clear
                .onAppear { onHeaderHeightChange?(proxy.size.height) }
                .onChange(of: proxy.size.height) { onHeaderHeightChange?($0) }
        })
    }

    private func footer(_ s: Snapshot) -> some View {
        HStack {
            Text(s.updatedText)
            Spacer()
            Text("BatteryTray \(s.version)")
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(.bar)
        .background(GeometryReader { proxy in
            Color.clear
                .onAppear { onFooterHeightChange?(proxy.size.height) }
                .onChange(of: proxy.size.height) { onFooterHeightChange?($0) }
        })
    }
}

/// A notice above the list with its buttons: an update, the login item, the Input Monitoring permission.
struct Banner<Buttons: View>: View {
    let symbol: String
    let tint: Color
    let text: String
    @ViewBuilder let buttons: () -> Buttons

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: symbol).foregroundStyle(tint).font(.title3)
                Text(text).fixedSize(horizontal: false, vertical: true)
            }
            HStack { buttons() }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 10).fill(tint.opacity(0.08)))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(tint.opacity(0.6)))
        .listRowSeparator(.hidden)
        .listRowInsets(DevicesView.insets(top: 4, bottom: 4))
    }
}

/// The kind of device, with a green dot while it is connected.
struct KindIcon: View {
    let kind: String
    let connected: Bool
    let size: CGFloat

    var body: some View {
        Image(systemName: symbol(kind))
            .font(.system(size: size))
            .foregroundStyle(connected ? .primary : .secondary)
            .frame(width: 34, height: 30)
            .overlay(alignment: .bottomTrailing) {
                if connected {
                    Circle()
                        .fill(Tone.color("good"))
                        .frame(width: 9, height: 9)
                        .overlay(Circle().stroke(Color(nsColor: .controlBackgroundColor), lineWidth: 2))
                        .offset(x: -1, y: -1)
                        .accessibilityLabel(AppModel.shared.t("state.online"))
                }
            }
    }
}

/// A device as a card (pinned or connected: the level and its bar) or as a row (not connected).
struct DeviceRowView: View {
    @EnvironmentObject var model: AppModel
    let device: Device
    let card: Bool

    private var renaming: Bool { model.renamingID == device.id }

    var body: some View {
        // Every card is as tall as one with a level, a bar and one line under the name, the most usual one; a card
        // grows only for a text that takes two lines. A card without a level has its content in the middle.
        ZStack(alignment: .leading) {
            if card {
                Self.template.hidden()
            }
            content
        }
        .padding(.vertical, card ? 12 : 4)
        // Rows without a card keep the cards' inner edge: their icons and menus line up with the cards'
        .padding(.horizontal, 12)
        .background {
            if card {
                RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color(nsColor: .separatorColor)))
            }
        }
        .contentShape(Rectangle())
        .accessibilityElement(children: .contain)
    }

    /// A card's usual content, in the same fonts: what gives every card its height.
    private static var template: some View {
        VStack(spacing: 10) {
            HStack(spacing: 10) {
                VStack(spacing: 2) {
                    Text(verbatim: "Ag").font(.system(size: 15, weight: .semibold))
                    Text(verbatim: "Ag").font(.caption)
                }
                Text(verbatim: "0%").font(.system(size: 24, weight: .semibold))
            }
            LevelBar(fill: 0, color: .clear, faint: false)
        }
        .accessibilityHidden(true)
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .center, spacing: 10) {
                KindIcon(kind: device.kind, connected: device.connected, size: card ? 20 : 15)
                VStack(alignment: .leading, spacing: renaming ? 6 : 2) {
                    if renaming {
                        RenameField(device: device, size: card ? 15 : 13)
                    } else {
                        HStack(spacing: 5) {
                            Text(device.name)
                                .font(.system(size: card ? 15 : 13, weight: card ? .semibold : .regular))
                                .foregroundStyle(device.connected ? .primary : .secondary)
                                .lineLimit(1)
                                .truncationMode(.tail)
                            if device.pinned {
                                Image(systemName: "pin.fill")
                                    .font(.system(size: 10))
                                    .foregroundStyle(device.connected ? .primary : .secondary)
                                    .accessibilityLabel(model.t("device.pinned"))
                            }
                        }
                    }
                    Text(renaming ? device.source : device.subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                // The text takes all the room up to the level, so a longer line under the name still fits
                .frame(maxWidth: .infinity, alignment: .leading)
                if card && device.connected {
                    HStack(spacing: 2) {
                        Text(device.levelText)
                            .font(.system(size: 24, weight: .semibold))
                            .monospacedDigit()
                        if device.charging {
                            Image(systemName: "bolt.fill").font(.system(size: 16, weight: .semibold))
                        }
                    }
                    .foregroundStyle(Tone.color(device.tone))
                    .fixedSize()
                }
                Button { model.menuID = device.id } label: {
                    Image(systemName: "ellipsis").frame(width: 24, height: 24).contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .help(model.t("device.menu"))
                .accessibilityLabel(model.t("device.menu"))
                .popover(isPresented: Binding(
                    get: { model.menuID == device.id },
                    set: { open in if !open && model.menuID == device.id { model.menuID = nil } }
                ), arrowEdge: .bottom) {
                    DeviceMenu(device: device) { model.menuID = nil }.environmentObject(model)
                }
            }
            if card && device.connected {
                LevelBar(fill: device.indeterminate ? 1 : device.fill, color: Tone.color(device.tone),
                         faint: device.indeterminate)
                    .padding(.leading, 44)
            }
        }
    }
}

/// The level as a thin bar; full and faint while charging with no level known.
struct LevelBar: View {
    let fill: Double
    let color: Color
    let faint: Bool

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.secondary.opacity(0.25))
                Capsule().fill(color.opacity(faint ? 0.45 : 1))
                    .frame(width: geometry.size.width * min(max(fill, 0), 1))
            }
        }
        .frame(height: 4)
        .accessibilityHidden(true)
    }
}

/// A hidden device: its name and state, and an open eye that puts it back (the word is its help tag).
struct HiddenRow: View {
    @EnvironmentObject var model: AppModel
    let device: Device

    var body: some View {
        HStack(spacing: 10) {
            KindIcon(kind: device.kind, connected: false, size: 15)
            VStack(alignment: .leading, spacing: 2) {
                Text(device.name).foregroundStyle(.secondary).lineLimit(1)
                Text(device.subtitle).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            // An image button without a border; its frame is what takes the click, about 10 points around the eye
            Button {
                model.send(["cmd": "hide", "id": device.id, "hidden": false])
            } label: {
                Image(systemName: "eye").frame(width: 36, height: 30).contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .help(model.t("device.show"))
            .accessibilityLabel("\(model.t("device.show")): \(device.name)")
        }
        .padding(.vertical, 3)
        // As the rows without a card; on the right 6 points less, so the eye stands under their "⋯" (24 points wide)
        .padding(.leading, 12)
        .padding(.trailing, 6)
    }
}

/// The name typed in place of the name: Return or leaving the field keeps it (empty: the device's own name), Esc
/// drops it.
struct RenameField: View {
    @EnvironmentObject var model: AppModel
    let device: Device
    let size: CGFloat
    @StateObject private var text = Local("")
    @FocusState private var focused: Bool
    @StateObject private var done = Local(false)

    var body: some View {
        TextField(device.ownName, text: $text.value)
            .textFieldStyle(.roundedBorder)
            .font(.system(size: size, weight: .semibold))
            .focused($focused)
            .accessibilityLabel(model.t("device.name"))
            .onAppear {
                text.value = device.customName
                DispatchQueue.main.async { focused = true }
            }
            .onSubmit(commit)
            .onExitCommand(perform: cancel)
            .onChange(of: focused) { isFocused in
                if !isFocused { commit() }
            }
    }

    private func commit() {
        guard !done.value else { return }
        done.value = true
        model.renamingID = nil
        model.send(["cmd": "rename", "id": device.id, "text": text.value])
    }

    private func cancel() {
        guard !done.value else { return }
        done.value = true
        model.renamingID = nil
        model.refresh()
    }
}

/// A device's menu: rename, pin, hide; its low battery reminders, at its own levels or the global ones.
struct DeviceMenu: View {
    @EnvironmentObject var model: AppModel
    let device: Device
    let close: () -> Void

    var body: some View {
        let s = model.snapshot?.settings
        VStack(alignment: .leading, spacing: 2) {
            action("pencil", model.t("device.rename")) {
                close()
                model.renamingID = device.id
            }
            action(device.pinned ? "pin.slash" : "pin", model.t(device.pinned ? "device.unpin" : "device.pin")) {
                close()
                model.send(["cmd": "pin", "id": device.id, "pinned": !device.pinned])
            }
            action("eye.slash", model.t("device.hide")) {
                close()
                model.send(["cmd": "hide", "id": device.id, "hidden": true])
            }
            Divider().padding(.vertical, 4)
            Toggle(isOn: Binding(
                get: { device.alerts },
                set: { model.send(["cmd": "device_alerts", "id": device.id, "on": $0]) }
            )) {
                HStack(spacing: 10) {
                    Image(systemName: "bell").frame(width: 18)
                    Text(model.t("device.alerts"))
                    Spacer(minLength: 0)
                }
            }
            .toggleStyle(.switch)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            if device.alerts {
                VStack(spacing: 6) {
                    levelRow(model.t("device.low"), device.low, "device_low")
                    levelRow(model.t("device.critical"), device.critical, "device_critical")
                    if device.ownLevels, let s {
                        Button {
                            model.send(["cmd": "device_levels_reset", "id": device.id])
                        } label: {
                            Label("\(model.t("device.levels.reset")) · \(s.low)% / \(s.critical)%",
                                  systemImage: "arrow.uturn.backward")
                                .font(.caption)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.borderless)
                        .foregroundStyle(.secondary)
                    }
                }
                .padding(.leading, 36)
                .padding(.trailing, 8)
                .padding(.bottom, 4)
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .animation(.easeOut(duration: 0.18), value: device.alerts)
        .animation(.easeOut(duration: 0.16), value: device.ownLevels)
        .padding(6)
        .frame(width: 290)
        .onAppear {
            // A menu that opens doesn't put the caret in a level field
            DispatchQueue.main.async { NSApp.keyWindow?.makeFirstResponder(nil) }
        }
    }

    /// The device's level in effect, as the app has it now.
    private func current(_ command: String) -> Int? {
        let devices = model.snapshot.map { $0.devices.pinned + $0.devices.connected + $0.devices.offline } ?? []
        let now = devices.first { $0.id == device.id }
        return command == "device_low" ? now?.low : now?.critical
    }

    private func action(_ symbol: String, _ title: String, _ run: @escaping () -> Void) -> some View {
        Button(action: run) {
            HStack(spacing: 10) {
                Image(systemName: symbol).frame(width: 18)
                Text(title)
                Spacer(minLength: 0)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(MenuRowStyle())
    }

    private func levelRow(_ title: String, _ value: Int, _ command: String) -> some View {
        HStack {
            Text(title)
            Spacer()
            // The value already shown is no change: it doesn't give the device levels of its own
            NumberField(value: value, unit: "%", label: title, current: { current(command) ?? value }) { typed in
                model.send(["cmd": command, "id": device.id, "value": typed])
            }
        }
    }
}

/// A row of a menu drawn in a popover: highlighted under the pointer.
struct MenuRowStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        MenuRow(configuration: configuration)
    }

    private struct MenuRow: View {
        let configuration: Configuration
        @StateObject private var hovering = Local(false)

        var body: some View {
            let lit = hovering.value || configuration.isPressed
            configuration.label
                .padding(.horizontal, 8)
                .padding(.vertical, 5)
                .background(RoundedRectangle(cornerRadius: 5).fill(lit ? Color.accentColor.opacity(0.85) : Color.clear))
                .foregroundStyle(lit ? Color.white : Color.primary)
                .onHover { hovering.value = $0 }
        }
    }
}

/// A number typed in a field with its unit: kept on Return or when the field is left, put back on Esc or when it
/// isn't a number. The app keeps it in range; the field then shows what the app kept.
struct NumberField: View {
    let value: Int
    let unit: String
    let label: String
    /// The value as the app has it right now (the view's `value` is the one it was drawn with)
    let current: () -> Int
    let commit: (Int) -> Void
    @StateObject private var text = Local("")
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 4) {
            TextField("", text: $text.value)
                .textFieldStyle(.roundedBorder)
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .lineLimit(1)
                // A form would lay a field with a label (an empty one too) out as label and field, and put a long
                // value's field on a line of its own under the unit
                .labelsHidden()
                .frame(width: Self.width(text.value))
                .focused($focused)
                .accessibilityLabel(label)
                .onSubmit {
                    apply()
                    // Return is done with the field, as a click elsewhere is: its window takes the focus back, once
                    // the field has finished with Return (it would take the focus again, its text selected)
                    let window = NSApp.keyWindow
                        ?? NSApp.windows.first { ($0.firstResponder as? NSTextView)?.isFieldEditor == true }
                    DispatchQueue.main.async { window?.makeFirstResponder(nil) }
                }
                .onExitCommand { text.value = "\(current())" }
                .onChange(of: focused) { isFocused in
                    if !isFocused { apply() }
                }
                .onChange(of: value) { fresh in
                    text.value = "\(fresh)"
                }
                .onAppear { text.value = "\(value)" }
            Text(unit).foregroundStyle(.secondary).fixedSize()
        }
        // As wide and as tall as it needs: a row never wraps it under its label or puts the unit above it
        .fixedSize()
    }

    /// Wide enough for the digits it holds, three at the least (100%), up to the five a repeat or a closing time has.
    private static func width(_ text: String) -> CGFloat {
        CGFloat(min(max(text.count, 3), 6)) * 8 + 22
    }

    private func apply() {
        let digits = text.value.filter(\.isNumber)
        if let typed = Int(digits), !digits.isEmpty, typed != current() {
            commit(typed)
        }
        text.value = "\(current())"
    }
}

/// The Input Monitoring permission some keyboards and mice need: asked for, and where it is changed.
@MainActor
enum Permissions {
    static func askForInputMonitoring(_ model: AppModel) {
        let allowed = Core.call(["cmd": "request_input_monitoring"])["allowed"] as? Bool ?? false
        if !allowed {
            openInputMonitoringSettings()
        }
        model.send(["cmd": "refresh"])
    }

    static func openInputMonitoringSettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent") {
            NSWorkspace.shared.open(url)
        }
    }
}
