import Cocoa
import Darwin
import Foundation
import SwiftUI

private func redirectStdoutStderrToLogFile() {
    // When launched from Finder, stdout/stderr often isn't visible.
    // Mirror logs to ~/Library/Logs/SiglusLauncher/console.log for debugging.
    let fm = FileManager.default
    guard let library = fm.urls(for: .libraryDirectory, in: .userDomainMask).first else { return }
    let dir = library.appendingPathComponent("Logs/SiglusLauncher", isDirectory: true)
    try? fm.createDirectory(at: dir, withIntermediateDirectories: true)

    let logFile = dir.appendingPathComponent("console.log")
    logFile.path.withCString { path in
        _ = freopen(path, "a+", stdout)
        _ = freopen(path, "a+", stderr)
    }

    setvbuf(stdout, nil, _IONBF, 0)
    setvbuf(stderr, nil, _IONBF, 0)
    print("\n---- SiglusLauncher start: \(Date()) ----")
}

typealias SiglusMessageboxCallback = @convention(c) (
    UnsafeMutableRawPointer?,
    UInt64,
    Int32,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?
) -> Void

@_silgen_name("siglus_pump_create")
private func siglus_pump_create(_ gameRootUtf8: UnsafePointer<CChar>, _ nlsUtf8: UnsafePointer<CChar>) -> UnsafeMutableRawPointer?

@_silgen_name("siglus_pump_set_native_messagebox_callback")
private func siglus_pump_set_native_messagebox_callback(
    _ handle: UnsafeMutableRawPointer?,
    _ callback: SiglusMessageboxCallback?,
    _ userData: UnsafeMutableRawPointer?
) -> Void

@_silgen_name("siglus_pump_submit_messagebox_result")
private func siglus_pump_submit_messagebox_result(_ handle: UnsafeMutableRawPointer?, _ requestId: UInt64, _ value: Int64) -> Void

@_silgen_name("siglus_pump_step")
private func siglus_pump_step(_ handle: UnsafeMutableRawPointer?, _ timeoutMs: UInt32) -> Int32

@_silgen_name("siglus_pump_destroy")
private func siglus_pump_destroy(_ handle: UnsafeMutableRawPointer?) -> Void

private final class PumpMessageboxContext {
    var handle: UnsafeMutableRawPointer?
    private var pendingMessageboxResults: [(UInt64, Int64)] = []

    func enqueueMessageboxResult(requestId: UInt64, value: Int64) {
        pendingMessageboxResults.append((requestId, value))
    }

    func drainMessageboxResults() -> [(UInt64, Int64)] {
        let results = pendingMessageboxResults
        pendingMessageboxResults.removeAll(keepingCapacity: true)
        return results
    }
}

private func runMessagebox(kind: Int32, title: String, message: String) -> Int64 {
    let alert = NSAlert()
    alert.alertStyle = .warning
    alert.messageText = title.isEmpty ? "Siglus" : title
    alert.informativeText = message

    switch kind {
    case 0:
        alert.addButton(withTitle: "OK")
    case 1:
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
    case 2:
        alert.addButton(withTitle: "Yes")
        alert.addButton(withTitle: "No")
    case 3:
        alert.addButton(withTitle: "Yes")
        alert.addButton(withTitle: "No")
        alert.addButton(withTitle: "Cancel")
    default:
        alert.addButton(withTitle: "OK")
    }

    let response = alert.runModal()
    let offset = response.rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
    switch kind {
    case 0:
        return 0
    case 1:
        return offset == 0 ? 0 : 1
    case 2:
        return offset == 0 ? 0 : 1
    case 3:
        if offset == 0 { return 0 }
        if offset == 1 { return 1 }
        return 2
    default:
        return 0
    }
}

@_cdecl("siglus_macos_messagebox_callback")
func siglus_macos_messagebox_callback(
    userData: UnsafeMutableRawPointer?,
    requestId: UInt64,
    kind: Int32,
    titleUtf8: UnsafePointer<CChar>?,
    messageUtf8: UnsafePointer<CChar>?
) {
    guard let userData else { return }
    let context = Unmanaged<PumpMessageboxContext>.fromOpaque(userData).takeUnretainedValue()
    let title = titleUtf8.map { String(cString: $0) } ?? "Siglus"
    let message = messageUtf8.map { String(cString: $0) } ?? ""

    let collect: () -> Void = {
        let value = runMessagebox(kind: kind, title: title, message: message)
        context.enqueueMessageboxResult(requestId: requestId, value: value)
    }

    if Thread.isMainThread {
        collect()
    } else {
        DispatchQueue.main.sync(execute: collect)
    }
}

final class LauncherWindowDelegate: NSObject, NSWindowDelegate {
    private let onClose: () -> Void

    init(onClose: @escaping () -> Void) {
        self.onClose = onClose
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        onClose()
        return true
    }
}

@MainActor
final class LauncherHost {
    let library: GameLibrary

    private(set) var shouldQuit: Bool = false
    private var selected: GameEntry? = nil

    private let window: NSWindow
	// Must be a stored property to keep the delegate alive.
	// Optional with a default value so `self` can be captured safely later in `init`.
	private var windowDelegate: LauncherWindowDelegate? = nil

    init() {
        self.library = GameLibrary()

        let rootView = ContentView().environmentObject(library)
        let hosting = NSHostingView(rootView: rootView)

        self.window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 980, height: 620),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        self.window.title = "Siglus"
        self.window.isReleasedWhenClosed = false
        self.window.center()
        self.window.contentView = hosting

		self.windowDelegate = LauncherWindowDelegate { [weak self] in
            guard let self else { return }
            Task { @MainActor in
                self.shouldQuit = true
            }
        }
		self.window.delegate = self.windowDelegate

        self.library.onLaunchRequest = { [weak self] game in
            guard let self else { return }
            Task { @MainActor in
                self.selected = game
            }
        }
    }

    /// Selection stage event-loop integration.
    ///
    /// Important: we intentionally **avoid** calling `NSApp.run()` / `NSApp.runModal()` / `NSApp.finishLaunching()` here.
    /// Winit expects to own the AppKit lifecycle when the pump host starts; pre-launching the app via AppKit
    /// can prevent winit from receiving its expected launch notifications.
    func runPumpSelection() -> GameEntry? {
        selected = nil
        shouldQuit = false

        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)

        while selected == nil && !shouldQuit {
            autoreleasepool {
                // Wait briefly for the next event.
                let until = Date(timeIntervalSinceNow: 1.0 / 60.0)
                if let event = NSApp.nextEvent(matching: .any,
                                              until: until,
                                              inMode: .default,
                                              dequeue: true) {
                    NSApp.sendEvent(event)
                }
                NSApp.updateWindows()
            }
        }

        window.orderOut(nil)
        return selected
    }

    func runGame(_ game: GameEntry) -> Int32 {
        let context = PumpMessageboxContext()
        let contextPtr = Unmanaged.passRetained(context).toOpaque()
        defer {
            Unmanaged<PumpMessageboxContext>.fromOpaque(contextPtr).release()
        }

        return game.rootPath.withCString { gameC in
            game.nls.withCString { nlsC in
                guard let handle = siglus_pump_create(gameC, nlsC) else {
                    return 1
                }
                context.handle = handle
                siglus_pump_set_native_messagebox_callback(handle, siglus_macos_messagebox_callback, contextPtr)
                defer {
                    siglus_pump_set_native_messagebox_callback(handle, nil, nil)
                    siglus_pump_destroy(handle)
                    context.handle = nil
                }

                while true {
                    let status = siglus_pump_step(handle, 16)
                    for (requestId, value) in context.drainMessageboxResults() {
                        siglus_pump_submit_messagebox_result(handle, requestId, value)
                    }
                    if status != 0 {
                        return 0
                    }
                }
            }
        }
    }
}

@main
@MainActor
struct SiglusLauncherMain {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)

        redirectStdoutStderrToLogFile()
        // Do NOT call `finishLaunching()` here.
        // The Rust side (winit) expects to own the AppKit launch sequence.

        let host = LauncherHost()

        if host.shouldQuit {
            app.terminate(nil)
            return
        }

        guard let game = host.runPumpSelection(), !host.shouldQuit else {
            app.terminate(nil)
            return
        }

        print("[launcher] -> siglus_pump_create/game loop(game_root=\(game.rootPath), nls=\(game.nls)); NSApp.isRunning=\(NSApp.isRunning)")
        let rc = host.runGame(game)
        print("[launcher] <- siglus pump loop returned \(rc); exiting process")

        // IMPORTANT:
        // We do NOT return to the launcher UI after a game finishes.
        // Returning to AppKit/SwiftUI after winit has owned the macOS app lifecycle is fragile and
        // can crash during teardown (double-runloop / invalid NSApp state / release ordering).
        // Exit the process immediately after the game returns.
        fflush(stdout)
        fflush(stderr)
        _exit(rc)
    }
}
