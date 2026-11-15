import Foundation

/// Monotonic capture time. Pauses and time spent between segments never count as audio.
struct RecordingClock {
    private(set) var completed: TimeInterval = 0
    private(set) var segment: TimeInterval = 0
    private var resumedAt: TimeInterval?

    mutating func prepare(durations: [TimeInterval]) {
        completed = durations.reduce(0) { $0 + Self.valid($1) }
        segment = 0
        resumedAt = nil
    }

    mutating func start(at uptime: TimeInterval) {
        segment = 0
        resumedAt = uptime
    }

    mutating func pause(at uptime: TimeInterval) {
        segment = segmentDuration(at: uptime)
        resumedAt = nil
    }

    mutating func resume(at uptime: TimeInterval) {
        guard resumedAt == nil else { return }
        resumedAt = uptime
    }

    func segmentDuration(at uptime: TimeInterval) -> TimeInterval {
        segment + (resumedAt.map { Self.valid(uptime - $0) } ?? 0)
    }

    func elapsed(at uptime: TimeInterval) -> TimeInterval { completed + segmentDuration(at: uptime) }

    @discardableResult
    mutating func stop(at uptime: TimeInterval, capturedDuration: TimeInterval? = nil) -> TimeInterval {
        let duration = capturedDuration.map(Self.valid) ?? segmentDuration(at: uptime)
        completed += duration
        segment = 0
        resumedAt = nil
        return duration
    }

    private static func valid(_ value: TimeInterval) -> TimeInterval {
        value.isFinite ? max(0, value) : 0
    }
}

enum RecordingLaunchPolicy {
    static func requestsSimulation(arguments: [String], debugSimulator: Bool) -> Bool {
        debugSimulator && arguments.contains("-simulate-capture")
    }
}

struct RecordingActivityUpdateGate {
    private var lastUpdate: TimeInterval?
    mutating func shouldUpdate(at uptime: TimeInterval, force: Bool = false) -> Bool {
        guard force || lastUpdate.map({ uptime - $0 >= 30 }) ?? true else { return false }
        lastUpdate = uptime
        return true
    }
}
