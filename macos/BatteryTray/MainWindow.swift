import AppKit
import Combine
import SwiftUI

/// The native menu bar popover for the device list, with refresh and settings in its header.
/// The status item handles its own clicks. The popover dismisses when the user clicks elsewhere or changes apps.
@MainActor
final class MainWindowController: NSObject, NSPopoverDelegate {
    private static let panelWidth: CGFloat = 440
    private static let maxContentHeight: CGFloat = 640
    private static let minContentHeight: CGFloat = 150

    private let popover = NSPopover()
    private let model: AppModel
    private weak var anchorButton: NSStatusBarButton?
    private let visibilityChanged: () -> Void
    private var headerHeight: CGFloat = 45
    private var footerHeight: CGFloat = 32
    private var fitScheduled = false
    private var subscriptions = Set<AnyCancellable>()
    private var documentObserver: NSObjectProtocol?
    private weak var observedDocument: NSView?
    private var globalClickMonitor: Any?
    private var localClickMonitor: Any?
    private var resignActiveObserver: NSObjectProtocol?
    private var appearanceObservation: NSKeyValueObservation?

    init(model: AppModel, button: NSStatusBarButton?, visibilityChanged: @escaping () -> Void) {
        self.model = model
        self.anchorButton = button
        self.visibilityChanged = visibilityChanged
        super.init()

        popover.behavior = .applicationDefined
        popover.animates = true
        popover.delegate = self
        popover.contentSize = NSSize(width: Self.panelWidth, height: 260)

        let rootView = DevicesView(
            showHeader: true,
            onHeaderHeightChange: { [weak self] height in
                guard let self, height > 0, abs(self.headerHeight - height) > 0.5 else { return }
                self.headerHeight = height
                self.scheduleFit()
            },
            onFooterHeightChange: { [weak self] height in
                guard let self, height > 0, abs(self.footerHeight - height) > 0.5 else { return }
                self.footerHeight = height
                self.scheduleFit()
            },
            onContentChange: { [weak self] in
                self?.scheduleFit()
            }
        ).environmentObject(model)
        popover.contentViewController = NSHostingController(rootView: rootView)

        appearanceObservation = NSApp.observe(\.effectiveAppearance) { [weak self] _, _ in
            Task { @MainActor in self?.updateAppearance() }
        }
        model.$snapshot
            .dropFirst()
            .sink { [weak self] _ in self?.scheduleFit() }
            .store(in: &subscriptions)
    }

    deinit {
        if let documentObserver {
            NotificationCenter.default.removeObserver(documentObserver)
        }
        if let globalClickMonitor {
            NSEvent.removeMonitor(globalClickMonitor)
        }
        if let localClickMonitor {
            NSEvent.removeMonitor(localClickMonitor)
        }
        if let resignActiveObserver {
            NotificationCenter.default.removeObserver(resignActiveObserver)
        }
    }

    var isVisible: Bool {
        popover.isShown
    }

    func show() {
        guard let button = anchorButton, button.window != nil else { return }
        updateAppearance()
        if !popover.isShown {
            popover.contentViewController?.view.layoutSubtreeIfNeeded()
            fitPanel()
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        }
        NSApp.activate(ignoringOtherApps: true)
        popover.contentViewController?.view.window?.makeKeyAndOrderFront(nil)
        languageChanged()
        scheduleFit()
        DispatchQueue.main.async { [weak self] in
            guard let self, self.isVisible else { return }
            self.installDismissMonitors()
        }
    }

    private func updateAppearance() {
        // The menu bar button can have a different appearance from the app.
        popover.appearance = NSApp.effectiveAppearance
    }

    func close() {
        guard popover.isShown else { return }
        // Finish closing before another status item click can open the popover again.
        popover.animates = false
        popover.close()
        popover.animates = true
    }

    func languageChanged() {
        popover.contentViewController?.view.window?.title = model.t("app.title")
    }

    func popoverWillClose(_ notification: Notification) {
        popover.contentViewController?.view.window?.makeFirstResponder(nil)
        model.renamingID = nil
        model.menuID = nil
    }

    func popoverDidClose(_ notification: Notification) {
        removeDismissMonitors()
        visibilityChanged()
    }

    private var anchorFrame: NSRect? {
        guard let button = anchorButton, let window = button.window else { return nil }
        return window.convertToScreen(button.convert(button.bounds, to: nil))
    }

    private func installDismissMonitors() {
        removeDismissMonitors()
        globalClickMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] event in
            MainActor.assumeIsolated {
                guard let self, self.anchorFrame?.contains(event.locationInWindow) != true else { return }
                self.close()
            }
        }
        localClickMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] event in
            guard let self, self.isVisible else { return event }
            let popoverWindow = self.popover.contentViewController?.view.window
            let buttonWindow = self.anchorButton?.window
            if let win = event.window,
               win !== popoverWindow,
               win !== buttonWindow,
               win.parent !== popoverWindow,
               !(popoverWindow?.childWindows?.contains(win) ?? false)
            {
                self.close()
            }
            return event
        }
        resignActiveObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didResignActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                // Menu bar tracking can deactivate the app before the status button receives mouse-up.
                if NSEvent.pressedMouseButtons & 0b11 != 0,
                   self.anchorFrame?.contains(NSEvent.mouseLocation) == true {
                    return
                }
                self.close()
            }
        }
    }

    private func removeDismissMonitors() {
        if let globalClickMonitor {
            NSEvent.removeMonitor(globalClickMonitor)
            self.globalClickMonitor = nil
        }
        if let localClickMonitor {
            NSEvent.removeMonitor(localClickMonitor)
            self.localClickMonitor = nil
        }
        if let resignActiveObserver {
            NotificationCenter.default.removeObserver(resignActiveObserver)
            self.resignActiveObserver = nil
        }
    }

    private func scheduleFit() {
        guard !fitScheduled else { return }
        fitScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.fitScheduled = false
            self.attachDocumentObserverIfNeeded()
            self.fitPanel()
        }
    }

    private func attachDocumentObserverIfNeeded() {
        guard let scrollView = findScrollView(in: popover.contentViewController?.view),
              let documentView = scrollView.documentView,
              documentView !== observedDocument else { return }
        if let documentObserver {
            NotificationCenter.default.removeObserver(documentObserver)
        }
        documentView.postsFrameChangedNotifications = true
        observedDocument = documentView
        documentObserver = NotificationCenter.default.addObserver(
            forName: NSView.frameDidChangeNotification,
            object: documentView,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.scheduleFit() }
        }
    }

    private func findScrollView(in view: NSView?) -> NSScrollView? {
        guard let view else { return nil }
        if let scrollView = view as? NSScrollView {
            return scrollView
        }
        for subview in view.subviews {
            if let found = findScrollView(in: subview) {
                return found
            }
        }
        return nil
    }

    /// Measures the SwiftUI List's actual row height plus header and footer, matching Windows's content-fit panel logic.
    private func measuredContentHeight() -> CGFloat? {
        guard let scrollView = findScrollView(in: popover.contentViewController?.view),
              let documentView = scrollView.documentView else { return nil }
        let listHeight: CGFloat
        if let table = documentView as? NSTableView {
            let rows = table.numberOfRows
            if rows > 0 {
                listHeight = table.rect(ofRow: rows - 1).maxY
                    + scrollView.contentInsets.top
                    + scrollView.contentInsets.bottom
                    + 6
            } else {
                listHeight = 0
            }
        } else {
            listHeight = documentView.frame.height
                + scrollView.contentInsets.top
                + scrollView.contentInsets.bottom
        }
        guard listHeight > 0 else { return nil }
        return ceil(listHeight + headerHeight + footerHeight + 1)
    }

    private func fitPanel() {
        attachDocumentObserverIfNeeded()
        guard let contentHeight = measuredContentHeight() else { return }
        let screenHeight = (anchorButton?.window?.screen ?? NSScreen.main)?.visibleFrame.height ?? Self.maxContentHeight
        let cap = min(Self.maxContentHeight, max(Self.minContentHeight, screenHeight - 40))
        let clampedContent = min(cap, max(Self.minContentHeight, contentHeight))
        if abs(clampedContent - popover.contentSize.height) > 1 {
            popover.contentSize = NSSize(width: Self.panelWidth, height: clampedContent)
        }
    }
}
