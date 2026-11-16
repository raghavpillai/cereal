import Foundation
import os
#if DEBUG
import QuartzCore
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
#endif
