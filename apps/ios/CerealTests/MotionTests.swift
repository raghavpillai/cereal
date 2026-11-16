import Foundation
import SwiftUI
import Testing
import APIClient
@testable import Cereal

struct MotionTests {
    @Test func tokensResolveToTheSharedSprings() {
        #expect(MotionResolver.spec(.snappy, reduceMotion: false) == MotionSpec(curve: .spring, duration: 0.28, bounce: 0.10))
        #expect(MotionResolver.spec(.gooey, reduceMotion: false) == MotionSpec(curve: .spring, duration: 0.42, bounce: 0.28))
        #expect(MotionResolver.spec(.settle, reduceMotion: false) == MotionSpec(curve: .spring, duration: 0.55, bounce: 0.15))
    }

    @Test func exitsAreQuickerThanEntrancesAndNeverBounce() {
        for token in MotionToken.allCases {
            let enter = MotionResolver.spec(token, phase: .enter, reduceMotion: false)
            let exit = MotionResolver.spec(token, phase: .exit, reduceMotion: false)
            #expect(exit.bounce == 0)
            #expect(exit.curve == .spring)
            #expect(abs(exit.duration - enter.duration * 0.65) < 0.0001)
            #expect((0.6...0.7).contains(exit.duration / enter.duration))
        }
    }

    @Test func reduceMotionReplacesEverySpringWithOneShortEase() {
        for token in MotionToken.allCases {
            for phase in [MotionPhase.enter, .exit] {
                let spec = MotionResolver.spec(token, phase: phase, reduceMotion: true)
                #expect(spec == MotionSpec(curve: .easeOut, duration: 0.16, bounce: 0))
            }
        }
    }

    @Test func staggerIsCappedAndAbsentUnderReduceMotion() {
        #expect(MotionResolver.staggerDelay(index: 0, reduceMotion: false) == 0)
        #expect(abs(MotionResolver.staggerDelay(index: 3, reduceMotion: false) - 0.105) < 0.0001)
        let cap = MotionResolver.staggerDelay(index: MotionResolver.staggerCap, reduceMotion: false)
        #expect(MotionResolver.staggerDelay(index: 500, reduceMotion: false) == cap)
        #expect(cap < 0.3)
        #expect(MotionResolver.staggerDelay(index: -4, reduceMotion: false) == 0)
        #expect(MotionResolver.staggerDelay(index: 5, reduceMotion: true) == 0)
    }

    @Test func pressingSquashesOrDimsButNeverBoth() {
        #expect(MotionResolver.pressScale(reduceMotion: false) == 0.96)
        #expect(MotionResolver.pressOpacity(reduceMotion: false) == 1)
        #expect(MotionResolver.pressScale(reduceMotion: true) == 1)
        #expect(MotionResolver.pressOpacity(reduceMotion: true) < 1)
    }

    @Test func reduceMotionRemovesContentInterpolation() {
        let reduced = Motion(reduceMotion: true)
        #expect(reduced.numericText == .identity)
        #expect(reduced.symbolReplace == .identity)
        #expect(reduced.waveform == nil)
        #expect(reduced.stagger(4) == 0)
        let full = Motion(reduceMotion: false)
        #expect(full.numericText != .identity)
        #expect(full.waveform != nil)
    }

    @MainActor @Test func entranceGateAdmitsOnlyTheFirstMoments() async throws {
        let gate = EntranceGate()
        #expect(gate.admit())
        #expect(gate.admit())
        try await Task.sleep(for: .milliseconds(600))
        #expect(!gate.admit())
        #expect(!EntranceGate(open: false).admit())
    }

    @MainActor @Test func theOriginRowIsReleasedAfterReturningHome() async throws {
        let router = Router()
        let id = UUID()
        router.path = [.note(id), .chat(noteID: id)]
        #expect(router.origin == .note(id))
        router.path = []
        #expect(router.origin == .note(id))
        try await Task.sleep(for: .milliseconds(600))
        #expect(router.origin == nil)
    }

    @MainActor @Test func reopeningBeforeReleaseKeepsTheNewOrigin() async throws {
        let router = Router()
        let account = AccountID(address: "https://example.test", userID: "00000000-0000-4000-8000-000000000001")
        let a = NoteRef(account: account, id: "a"), b = NoteRef(account: account, id: "b")
        router.path = [.serverNote(a)]
        router.path = []
        router.path = [.serverNote(b)]
        try await Task.sleep(for: .milliseconds(600))
        #expect(router.origin == .serverNote(b))
    }
}

struct RailLayoutTests {
    @Test func anOrdinaryIPhoneHasNoRail() {
        let portrait = RailMetrics(contentWidth: 402, leadingInset: 0, trailingInset: 0, bottomInset: 34)
        #expect(!portrait.hasRail)
        #expect(portrait.gutter == Theme.gutter)
        #expect(portrait.barInset == Theme.gutter)
        #expect(portrait.extraLeading == 0)
        // Text has the same 16 pt margin on both sides.
        #expect(portrait.extraTrailing == 8)
        #expect(8 + portrait.extraTrailing == Theme.gutter)
        #expect(portrait.extraTrailing + portrait.composerTrailing == 12)
        #expect(portrait.captureBarLeading == 12)
        #expect(portrait.bottomPadding == 8)
        #expect(portrait.composerMargin == 12)
        #expect(portrait.captureGutter == Theme.gutter)
        // Matching side insets in landscape are the sensor housing, not a rail.
        let landscape = RailMetrics(contentWidth: 750, leadingInset: 62, trailingInset: 62)
        #expect(!landscape.hasRail)
        #expect(landscape.gutter == Theme.gutter)
    }

    @Test func aFoldedDuoMatchesTheMeasuredReference() {
        let folded = RailMetrics(contentWidth: 382, leadingInset: 0, trailingInset: 84, bottomInset: 34)
        #expect(folded.hasRail)
        #expect(!folded.isWide)
        #expect(folded.railInset == 84)
        #expect(folded.gutter == 24)
        #expect(folded.barInset == 24)
        // Ask pill 24...358, + button 378...442, both ending 28 pt above the screen edge.
        let pillTrailing = 382 - folded.barInset
        #expect(pillTrailing == 358)
        #expect(pillTrailing + folded.addButtonGap == 378)
        #expect(34 + folded.bottomPadding == RailMetrics.barEdgeMargin)
        // Composer 17...365 inside a content area the scaffold has already padded.
        #expect(folded.composerMargin == 17)
        #expect(folded.extraLeading + folded.composerLeading == 17)
        #expect(folded.extraTrailing + folded.composerTrailing == 17)
        #expect(folded.captureGutter == 28)
        #expect(folded.captureBarLeading == 24)
    }

    @Test func anUnfoldedDuoKeepsTheRailAndWidensTheColumn() {
        let unfolded = RailMetrics(contentWidth: 867, leadingInset: 0, trailingInset: 84, bottomInset: 21)
        #expect(unfolded.hasRail)
        #expect(unfolded.isWide)
        #expect(unfolded.gutter == 82)
        #expect(unfolded.barInset == 75)
        #expect(unfolded.composerMargin == unfolded.barInset)
        let pillTrailing = 867 - unfolded.barInset
        #expect(pillTrailing + unfolded.addButtonGap + RailMetrics.addButtonSize == 867 + 84 - RailMetrics.addButtonEdgeMargin)
        #expect(21 + unfolded.bottomPadding == RailMetrics.barEdgeMargin)
    }

    @Test func aFloatingBarRidesAboveTheKeyboardInsteadOfSlidingUnderIt() {
        let keyboardUp = RailMetrics(contentWidth: 382, leadingInset: 0, trailingInset: 84, bottomInset: 310)
        #expect(keyboardUp.bottomPadding == 12)
        let unfoldedKeyboardUp = RailMetrics(contentWidth: 867, leadingInset: 0, trailingInset: 84, bottomInset: 290)
        #expect(unfoldedKeyboardUp.bottomPadding > 0)
    }

    @Test func theAddButtonNeverOverlapsThePill() {
        let narrowRail = RailMetrics(contentWidth: 382, leadingInset: 0, trailingInset: 48)
        #expect(narrowRail.addButtonGap >= 14)
    }

    @Test func railControlsUseTheReferenceSizes() {
        #expect(RailChrome.glyphSize == 15)
        #expect(RailChrome.glyphWeight == .medium)
        #expect(RailChrome.avatarSize == 30)
        // The system adds 8 pt around an item and 14 pt between grouped items.
        #expect(RailChrome.itemHeight + 8 == 46.5)
        #expect(RailChrome.itemHeight + 14 == 52.5)
    }
}

struct SidebarTests {
    @Test func thePanelTracksTheFingerAndClampsToItsTravel() {
        #expect(SidebarModel.progress(start: 0, translation: 150, width: 300) == 0.5)
        #expect(SidebarModel.progress(start: 0, translation: 900, width: 300) == 1)
        #expect(SidebarModel.progress(start: 0, translation: -40, width: 300) == 0)
        #expect(SidebarModel.progress(start: 1, translation: -75, width: 300) == 0.75)
        #expect(SidebarModel.progress(start: 0, translation: 10, width: 0) == 1)
    }

    @Test func releasingSettlesByFlickDirectionOrNearerSide() {
        #expect(SidebarModel.shouldOpen(progress: 0.1, velocity: 900))
        #expect(!SidebarModel.shouldOpen(progress: 0.9, velocity: -900))
        #expect(SidebarModel.shouldOpen(progress: 0.6, velocity: 0))
        #expect(!SidebarModel.shouldOpen(progress: 0.3, velocity: 0))
    }

    @MainActor @Test func aDragOpensAndTheSameTouchCannotPressARow() async throws {
        let model = SidebarModel()
        model.width = 300
        #expect(!model.isVisible)
        model.beginDrag()
        model.drag(translation: 200)
        #expect(model.isDragging)
        #expect(model.suppressesRowTaps)
        model.endDrag(velocity: 0)
        #expect(model.isOpen)
        #expect(model.suppressesRowTaps)
        try await Task.sleep(for: .milliseconds(500))
        #expect(!model.suppressesRowTaps)
        model.close()
        #expect(!model.isVisible)
    }
}

struct NoteSearchTests {
    private let me = Person(name: "Alex Rivera")
    private var notes: [Note] {
        [
            Note(title: "Lumen pitch", createdAt: .now, sections: [NoteSection(heading: "Traction", bullets: ["Two paid pilots"])]),
            Note(title: "Weekly sync", createdAt: .now, attendees: [me, Person(name: "Mike Zhang")], userNotes: "Ship the capture pill"),
            Note(title: "Scratch", createdAt: .now, transcriptRevision: 1),
        ]
    }

    @Test func anEmptyQueryFindsNothing() {
        #expect(NoteSearch.local(notes, query: "   ", currentUser: me).isEmpty)
        #expect(NoteSearch.server([], account: AccountID(address: "https://example.test", userID: "00000000-0000-4000-8000-000000000001"), onThisPhone: { _ in true }).isEmpty)
    }

    @Test func searchCoversTitleNotesAndPeople() {
        let byTitle = NoteSearch.local(notes, query: "lumen", currentUser: me)
        #expect(byTitle.map(\.title) == ["Lumen pitch"])
        #expect(byTitle.first?.excerpt == nil)

        let byEnhanced = NoteSearch.local(notes, query: "pilots", currentUser: me)
        #expect(byEnhanced.first?.excerpt == "Two paid pilots")

        let byTyped = NoteSearch.local(notes, query: "capture pill", currentUser: me)
        #expect(byTyped.map(\.title) == ["Weekly sync"])

        let byPerson = NoteSearch.local(notes, query: "zhang", currentUser: me)
        #expect(byPerson.map(\.title) == ["Weekly sync"])
        #expect(byPerson.first?.route == .note(notes[1].id) || byPerson.first?.title == "Weekly sync")
    }

    @Test func rowsShowAMeetingInitialAPhoneOrADocument() {
        #expect(notes[0].tile(currentUser: me) == .document)
        #expect(notes[1].tile(currentUser: me) == .letter("M"))
        #expect(notes[2].tile(currentUser: me) == .phone)
        #expect(NoteTile.meeting(with: "  ") == .document)
        #expect(NoteTile.colour(for: "M") == NoteTile.colour(for: "M"))
        #expect(NoteTile.colour(for: "M") != NoteTile.colour(for: "S"))
    }
}
