import Foundation
import os
#if DEBUG
import QuartzCore
import UIKit
#endif

/// Signpost intervals for a profiling session (Instruments > os_signpost, or
/// the Points of Interest lane), named as in the performance audit. They cost
/// nothing when no tool is recording.
enum Signpost {
    static let ui = OSSignposter(subsystem: AppIdentity.subsystem, category: "ui")

    /// Runs `work` inside a named interval.
    @discardableResult
    static func interval<T>(_ name: StaticString, _ work: () throws -> T) rethrows -> T {
        let state = ui.beginInterval(name)
        defer { ui.endInterval(name, state) }
        return try work()
    }

    static func event(_ name: StaticString) { ui.emitEvent(name) }
}

/// Debug-only counters for evidence that does not depend on machine speed:
/// how many times a view body ran, a file was written or text was parsed.
/// Launch with `-perf-probe` to print the counts once a second.
enum PerfProbe {
    #if DEBUG
    nonisolated(unsafe) private static var counts: [String: Int] = [:]
    private static let lock = NSLock()
    nonisolated(unsafe) private static var printer: Task<Void, Never>?
    static let enabled = ProcessInfo.processInfo.arguments.contains("-perf-probe")

    static func hit(_ name: String, _ amount: Int = 1) {
        lock.lock(); counts[name, default: 0] += amount; lock.unlock()
        if enabled, printer == nil { startPrinting() }
    }

    static func count(_ name: String) -> Int {
        lock.lock(); defer { lock.unlock() }
        return counts[name] ?? 0
    }

    static func reset() { lock.lock(); counts = [:]; lock.unlock() }

    private static func snapshot() -> [String: Int] { lock.withLock { counts } }

    private static func startPrinting() {
        // A file or a pipe is block-buffered: without this the counts of a scripted run never arrive.
        setvbuf(stdout, nil, _IOLBF, 0)
        Task { @MainActor in FrameGapProbe.shared.start() }
        printer = Task.detached {
            var previous: [String: Int] = [:]
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                let now = snapshot()
                let changed = now.filter { $0.value != previous[$0.key] ?? 0 }
                if !changed.isEmpty {
                    let line = changed.keys.sorted().map { "\($0)=+\(now[$0]! - (previous[$0] ?? 0)) (\(now[$0]!))" }.joined(separator: " ")
                    print("PERF " + line)
                }
                previous = now
            }
        }
    }
    #else
    @inline(__always) static func hit(_ name: String, _ amount: Int = 1) {}
    #endif
}

#if DEBUG
/// With `-perf-probe`: counts the frames the main thread delivered late. A display link
/// fires once per refresh while the main run loop is free; a gap of more than 50 ms
/// between two callbacks is a stall someone would see. `MainStall` is the number of
/// stalls and `MainStallMs` their total length. A busy host stretches these too, so
/// they are read beside the process's own CPU time, never alone.
@MainActor
final class FrameGapProbe: NSObject {
    static let shared = FrameGapProbe()
    private var link: CADisplayLink?
    private var last: CFTimeInterval = 0

    func start() {
        guard link == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = CACurrentMediaTime()
        if last > 0, now - last > 0.05 {
            PerfProbe.hit("MainStall")
            PerfProbe.hit("MainStallMs", Int((now - last) * 1000))
        }
        last = now
    }
}

/// With `-frame-log`: one stdout line per late frame, stamped in wall-clock
/// time so a host script can attribute it to the touch it sent. A frame is
/// late when the display link callback arrives more than half a refresh after
/// it was due. `FRAMES` lines give the delivered count per second, so a quiet
/// stretch (no lines) is distinguishable from a stalled one.
@MainActor
final class FrameLog: NSObject {
    static let shared = FrameLog()
    static let enabled = ProcessInfo.processInfo.arguments.contains("-frame-log")
    private var link: CADisplayLink?
    private var last: CFTimeInterval = 0
    private var frames = 0
    private var second = 0

    func start() {
        guard Self.enabled, link == nil else { return }
        setvbuf(stdout, nil, _IOLBF, 0)
        TouchMarker.shared.install()
        let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        let period = link.targetTimestamp - link.timestamp
        let wall = Date().timeIntervalSince1970
        if last > 0, period > 0, now - last > period * 1.5 {
            print(String(format: "HITCH %.3f %.1f %.1f", wall, (now - last) * 1000, period * 1000))
        }
        last = now
        frames += 1
        let s = Int(wall)
        if s != second {
            if second != 0 { print("FRAMES \(second) \(frames)") }
            second = s; frames = 0
        }
    }
}

/// With `-frame-log`: a 6 pt square in the top-leading corner changes colour
/// (green, blue, red in turn) on every touch-up, drawn in its own window. In a screen
/// recording it marks when each touch landed in the same clock as the frames,
/// so the delay to the UI's first visible response can be read off the video.
@MainActor
final class TouchMarker: NSObject, UIGestureRecognizerDelegate {
    static let shared = TouchMarker()
    private var window: UIWindow?
    private let square = UIView(frame: CGRect(x: 0, y: 0, width: 6, height: 6))
    private var ups = 0

    func install() {
        guard window == nil else { return }
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first,
              let host = scene.windows.first(where: \.isKeyWindow) ?? scene.windows.first else {
            Task { @MainActor in try? await Task.sleep(for: .milliseconds(300)); self.install() }
            return
        }
        let overlay = UIWindow(windowScene: scene)
        overlay.windowLevel = .statusBar + 10
        overlay.isUserInteractionEnabled = false
        overlay.backgroundColor = .clear
        square.backgroundColor = .black
        overlay.addSubview(square)
        overlay.isHidden = false
        window = overlay
        let watcher = TouchWatcher(target: nil, action: nil)
        watcher.cancelsTouchesInView = false
        watcher.delaysTouchesBegan = false
        watcher.delaysTouchesEnded = false
        watcher.delegate = self
        watcher.onPhase = { [weak self] down in
            // A tap's down and up arrive in one frame; only the up is drawn, in the next colour of a cycle.
            if !down, let self {
                self.ups += 1
                self.square.backgroundColor = [UIColor.green, .blue, .red][self.ups % 3]
            }
            print(String(format: "TOUCH %@ %.3f", down ? "down" : "up", Date().timeIntervalSince1970))
        }
        host.addGestureRecognizer(watcher)
    }

    nonisolated func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
}

private final class TouchWatcher: UIGestureRecognizer {
    var onPhase: ((Bool) -> Void)?
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) { onPhase?(true) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) { onPhase?(false); state = .failed }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { onPhase?(false); state = .failed }
}
#endif
