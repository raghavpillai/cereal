import Foundation
import Testing
@testable import Cereal

struct RecordingClockTests {
    @Test func pauseAndResumeExcludeWaitingTime() {
        var clock = RecordingClock()
        clock.prepare(durations: [12, 8])
        clock.start(at: 100)
        #expect(clock.elapsed(at: 105) == 25)
        clock.pause(at: 106)
        #expect(clock.elapsed(at: 900) == 26)
        clock.resume(at: 900)
        #expect(clock.stop(at: 904) == 10)
        #expect(clock.completed == 30)
        #expect(clock.elapsed(at: 2_000) == 30)
    }

    @Test func deviceTimeWinsOverSuspendedWallTime() {
        var clock = RecordingClock()
        clock.start(at: 100)
        #expect(clock.stop(at: 3_000, capturedDuration: 23.5) == 23.5)
        #expect(clock.completed == 23.5)
        clock.start(at: 5_000)
        #expect(clock.stop(at: 5_010, capturedDuration: 9) == 9)
        #expect(clock.completed == 32.5)
    }

    @Test func invalidPriorDurationsAndBackwardClockDoNotCorruptElapsed() {
        var clock = RecordingClock()
        clock.prepare(durations: [10, -5, .nan, .infinity])
        clock.start(at: 100)
        #expect(clock.elapsed(at: 90) == 10)
        #expect(clock.stop(at: 110) == 10)
        #expect(clock.stop(at: 200) == 0)
        #expect(clock.completed == 20)
    }

    @Test func simulationRequiresExplicitDebugSimulatorOptIn() {
        #expect(!RecordingLaunchPolicy.requestsSimulation(arguments: [], debugSimulator: true))
        #expect(!RecordingLaunchPolicy.requestsSimulation(arguments: ["-simulate-capture"], debugSimulator: false))
        #expect(RecordingLaunchPolicy.requestsSimulation(arguments: ["-simulate-capture"], debugSimulator: true))
    }

    @Test func activityHeartbeatIsThrottledButStateChangesAreImmediate() {
        var gate = RecordingActivityUpdateGate()
        let decisions = [gate.shouldUpdate(at: 1), gate.shouldUpdate(at: 1.08),
                         gate.shouldUpdate(at: 30.99), gate.shouldUpdate(at: 31),
                         gate.shouldUpdate(at: 32, force: true), gate.shouldUpdate(at: 33)]
        #expect(decisions == [true, false, false, true, true, false])
    }
}
