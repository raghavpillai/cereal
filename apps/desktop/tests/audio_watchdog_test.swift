@main
struct WatchdogTests {
    static func main() {
        var watchdog = SystemAudioWatchdog()
        watchdog.reset(now: 0)
        watchdog.observe(outputActive: false, now: 1)
        // Starting a meeting before anyone speaks must remain resumable indefinitely.
        assert(!watchdog.timedOut(framesReceived: 0, startedAt: 0, lastBufferAt: 0, now: 600))
        watchdog.observe(outputActive: true, now: 600)
        assert(!watchdog.timedOut(framesReceived: 0, startedAt: 0, lastBufferAt: 0, now: 609))
        assert(watchdog.timedOut(framesReceived: 0, startedAt: 0, lastBufferAt: 0, now: 611))
        // A source pausing after real PCM must not be mistaken for a broken tap.
        watchdog.observe(outputActive: false, now: 612)
        assert(!watchdog.timedOut(framesReceived: 48000, startedAt: 0, lastBufferAt: 611, now: 900))
        watchdog.observe(outputActive: true, now: 900)
        assert(!watchdog.timedOut(framesReceived: 48000, startedAt: 0, lastBufferAt: 611, now: 903))
        assert(watchdog.timedOut(framesReceived: 48000, startedAt: 0, lastBufferAt: 611, now: 905))
        // Unsupported/racing HAL properties must not hide a missing-stream failure.
        watchdog.reset(now: 1000)
        watchdog.observe(outputActive: nil, now: 1001)
        assert(watchdog.timedOut(framesReceived: 0, startedAt: 1000, lastBufferAt: 0, now: 1011))
        watchdog.observe(outputActive: false, now: 1012)
        watchdog.observe(outputActive: nil, now: 1020)
        assert(!watchdog.timedOut(framesReceived: 0, startedAt: 1000, lastBufferAt: 0, now: 1029))
        assert(watchdog.timedOut(framesReceived: 0, startedAt: 1000, lastBufferAt: 0, now: 1031))
        // Normal callback progress resets the baseline without resetting activity.
        assert(!watchdog.timedOut(framesReceived: 96000, startedAt: 1000, lastBufferAt: 1030, now: 1033))
        print("System audio watchdog transitions passed")
    }
}
