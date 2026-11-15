import Foundation

/// A process tap may legitimately have no callbacks while no other process is
/// outputting to its device. Unknown HAL activity keeps the bounded timeout;
/// only a positively observed idle device suspends it. No audio/content is read.
struct SystemAudioWatchdog {
    private var expectedSince: Double?

    mutating func reset(now: Double) { expectedSince = now }

    mutating func observe(outputActive: Bool?, now: Double) {
        if outputActive == false { expectedSince = nil }
        else if expectedSince == nil { expectedSince = now }
    }

    func timedOut(framesReceived: UInt64, startedAt: Double, lastBufferAt: Double, now: Double) -> Bool {
        guard let expectedSince else { return false }
        let baseline = framesReceived == 0 ? startedAt : lastBufferAt
        let limit = framesReceived == 0 ? 10.0 : 4.0
        return now - max(baseline, expectedSince) > limit
    }
}
