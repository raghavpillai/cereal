import SwiftUI
import APIClient
import Contracts

/// What is sent to the server as the earlier turns of a conversation.
enum ChatHistoryRule {
    /// The server continues a chat thread by matching this history against the
    /// thread it remembers: every question, and the start of every answer. An
    /// answer it never saw complete can only be the last one, and is then taken
    /// from the text sent here. So:
    ///
    ///  - an answer that was cut short after text had appeared is sent as that
    ///    text, with its question;
    ///  - a question whose answer never showed anything is left out with it.
    ///    The thread then matches at the exchange before, which the server
    ///    allows; a question with nothing after it would match no thread.
    ///
    /// An answer still arriving is not history yet.
    static func history(from messages: [ChatMessage], limit: Int = 20) -> [ChatMessageDTO] {
        var turns: [ChatMessageDTO] = []
        var question: ChatMessage?
        for message in messages {
            switch message.role {
            case .user:
                question = message.text.isEmpty ? nil : message
            case .assistant:
                guard let asked = question, !message.isStreaming, !message.text.isEmpty else { question = nil; continue }
                turns.append(ChatMessageDTO(content: asked.text, role: .user))
                turns.append(ChatMessageDTO(content: message.text, role: .assistant))
                question = nil
            }
        }
        // Whole exchanges only, so what is sent always starts with a question.
        return Array(turns.suffix(limit - limit % 2))
    }
}

/// One conversation with the server and the answer that may be arriving in it.
///
/// It is owned by `ServerChatSessions`, not by a screen, so an answer keeps
/// arriving when the reader leaves the chat, leaves the note, or the app is
/// briefly in the background, and is there (or still arriving) on return.
/// Only Stop, a new chat, choosing another conversation, signing out and
/// deleting the note end it early.
///
/// A session belongs to one account and one note for its whole life: `key` is
/// made of both. Text that arrives is written into this object only, and only
/// while the account that asked is still the one signed in.
@MainActor @Observable final class ServerChatSession {
    struct Queued: Identifiable, Equatable {
        let id = UUID()
        let text: String
        let model: String?
    }

    typealias Events = @MainActor (_ noteID: String?, _ message: String, _ history: [ChatMessageDTO], _ model: String?) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error>
    /// A question asked as exchange `turn` of the saved thread `create.id` (see `ServerChatThreads.swift`).
    typealias ThreadEvents = @MainActor (_ create: CreateChatThreadInput, _ turn: Int, _ replace: Bool, _ message: String, _ model: String?) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error>

    let key: String
    let accountIdentity: String
    let noteID: String?

    private(set) var messages: [ChatMessage]
    private(set) var history: [ChatConversation]
    /// The server thread the conversation on screen is saved as; nil until its first question.
    private(set) var threadID: String?
    /// An answer is arriving.
    private(set) var busy = false
    /// Questions typed while an answer was arriving, in the order they will be asked.
    private(set) var queue: [Queued] = []
    var error: String?
    /// Counts questions that went out, for the haptic on send.
    private(set) var sentCount = 0

    @ObservationIgnored private let events: Events
    @ObservationIgnored private let threadEvents: ThreadEvents?
    /// The next question replaces the saved exchange it takes the place of (a retry).
    @ObservationIgnored private var replacing = false
    @ObservationIgnored private let currentAccount: @MainActor () -> String?
    @ObservationIgnored private let persist: @MainActor (ServerChatSession) -> Void
    @ObservationIgnored private let background: ChatBackgroundTime
    /// The Live Activity of the answer arriving (`ChatLiveActivity.swift`).
    @ObservationIgnored private let activity: ChatActivityHooks
    @ObservationIgnored private var activityID: String?
    @ObservationIgnored private var request: Task<Void, Never>?
    @ObservationIgnored private var requestID: UUID?
    @ObservationIgnored private var deltaBuffer: ChatDeltaBuffer?
    @ObservationIgnored private var ended = false

    init(key: String, accountIdentity: String, noteID: String?, state: ServerChatSessions.ConversationState,
         events: @escaping Events, threadEvents: ThreadEvents? = nil, currentAccount: @escaping @MainActor () -> String?,
         background: ChatBackgroundTime = .none, activity: ChatActivityHooks = .none, persist: @escaping @MainActor (ServerChatSession) -> Void) {
        self.key = key; self.accountIdentity = accountIdentity; self.noteID = noteID
        messages = state.messages; history = state.history; threadID = state.threadID
        self.events = events; self.threadEvents = threadEvents; self.currentAccount = currentAccount; self.background = background; self.activity = activity; self.persist = persist
    }

    static let maximumQuestionLength = 10_000

    /// Sends the question, or queues it behind the answer that is arriving.
    /// False when it is empty, too long, or the account is no longer signed in.
    @discardableResult
    func ask(_ raw: String, model: String?) -> Bool {
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !ended, !text.isEmpty, text.count <= Self.maximumQuestionLength else { return false }
        guard currentAccount() == accountIdentity else { error = "Sign in to ask about your notes."; return false }
        if busy { queue.append(Queued(text: text, model: model)); return true }
        start(text, model: model)
        return true
    }

    func removeQueued(_ id: UUID) { queue.removeAll { $0.id == id } }

    /// Stop: ends the answer that is arriving, keeping what was shown, and gives
    /// back the questions that were waiting so they can return to the composer.
    @discardableResult
    func stop() -> [String] {
        let waiting = queue.map(\.text)
        queue = []
        cancelRequest()
        return waiting
    }

    /// Starts an empty conversation; the one on screen goes to the history.
    @discardableResult
    func newConversation() -> [String] {
        let waiting = stop()
        history = ServerChatSessions.archive(messages, in: history, threadID: threadID)
        messages = []; error = nil; threadID = nil
        persist(self)
        return waiting
    }

    /// Brings an earlier conversation back; the one on screen goes to the history.
    func select(_ conversation: ChatConversation) {
        stop()
        let selected = ServerChatSessions.select(conversation, current: messages, history: ServerChatSessions.archive(messages, in: history, threadID: threadID))
        messages = selected.messages; history = selected.history; threadID = selected.threadID; error = nil
        persist(self)
    }

    /// Brings a saved thread from the server; the one on screen goes to the history. Asking in it continues it.
    func open(_ thread: ChatThreadDTO) {
        stop()
        history = ServerChatSessions.archive(messages, in: history, threadID: threadID)
        messages = ChatThreadTurns.messages(of: thread)
        threadID = thread.id.lowercased(); error = nil
        history.removeAll { $0.threadID == threadID }
        persist(self)
    }

    func retryLastQuestion(model: String?) {
        guard !busy else { return }
        if messages.last?.role == .assistant { messages.removeLast() }
        guard let last = messages.last, last.role == .user else { return }
        messages.removeLast()
        replacing = true
        ask(last.text, model: model)
        replacing = false
    }

    /// The account signed out or the note was deleted: nothing more may arrive
    /// and nothing more is saved from here.
    func end() {
        guard !ended else { return }
        queue = []
        cancelRequest(saving: false)
        ended = true
    }

    var conversationsForHistory: [ChatConversation] { ServerChatSessions.archive(messages, in: history, threadID: threadID) }

    // MARK: The request

    private func start(_ text: String, model: String?) {
        let context = ChatHistoryRule.history(from: messages)
        // The exchange this question is (it matters for a retry, which replaces it).
        let turn = messages.filter { $0.role == .user }.count
        let replace = replacing; replacing = false
        if threadEvents != nil, threadID == nil { threadID = UUID().uuidString.lowercased() }
        error = nil
        messages.append(ChatMessage(role: .user, text: text))
        let reply = ChatMessage(role: .assistant, text: "", isStreaming: true)
        messages.append(reply)
        busy = true
        sentCount += 1
        let id = UUID(); requestID = id
        // The question is on disk before its answer starts; a streaming answer is saved as interrupted.
        persist(self)
        let token = background.begin()
        // Stop in the Live Activity ends this answer; questions waiting behind it then go out.
        activityID = activity.begin { [weak self] in
            guard let self, requestID == id else { return }
            cancelRequest(); askNextQueued()
        }
        #if DEBUG
        let timing = ChatTimingLog(question: text)
        #endif
        let asked = ContinuousClock.now
        // Tokens are gathered and shown about once a frame.
        let buffer = ChatDeltaBuffer { [weak self] delta in
            guard let self, requestID == id, let index = messages.firstIndex(where: { $0.id == reply.id }) else { return }
            if messages[index].text.isEmpty, !delta.isEmpty {
                #if DEBUG
                timing.mark("first-text")
                #endif
                // The first text ends the thinking: its line says how long that took.
                markThought(index, since: asked)
                if let activityID { activity.writing(activityID) }
            }
            messages[index].text += delta
        }
        deltaBuffer = buffer
        Signpost.event("Chat.Send")
        // The request is made here, not inside the task, so it is on its way before this returns.
        let stream: AsyncThrowingStream<ChatStreamEventDTO, Error>
        do {
            if let threadEvents, let threadID {
                stream = try threadEvents(CreateChatThreadInput(id: threadID, noteId: noteID?.lowercased()), turn, replace, text, model)
            } else { stream = try events(noteID, text, context, model) }
        }
        catch {
            background.end(token)
            finish(id, failure: error)
            return
        }
        request = Task { [weak self] in
            defer { self?.background.end(token) }
            var failure: Error?
            do {
                for try await event in stream {
                    guard let self, !Task.isCancelled, requestID == id else { return }
                    // The answer belongs to the account that asked. If another is signed in now, it goes nowhere.
                    guard currentAccount() == accountIdentity else { end(); return }
                    guard let index = messages.firstIndex(where: { $0.id == reply.id }) else { return }
                    if event.type == .delta {
                        PerfProbe.hit("Chat.token")
                        buffer.append(event.text)
                    } else if event.type == .reasoning {
                        // Reasoning that trails in after the answer began is not shown, as on the desktop.
                        buffer.flushNow()
                        guard messages[index].text.isEmpty, !event.text.isEmpty else { continue }
                        var annotations = messages[index].annotations ?? ChatTurnAnnotations()
                        annotations.addReasoning(event.text)
                        messages[index].annotations = annotations
                    } else if event.type == .tool || event.type == .citations {
                        // Keep order: text received so far is shown before the annotation.
                        buffer.flushNow()
                        var annotations = messages[index].annotations ?? ChatTurnAnnotations()
                        annotations.apply(event)
                        messages[index].annotations = annotations
                    }
                }
                buffer.flushNow()
            } catch is CancellationError { }
            catch { failure = error }
            #if DEBUG
            if self?.requestID == id { timing.mark(failure == nil ? "finished" : "failed") }
            #endif
            // An answer that ended before any text still says how long it thought.
            if let self, requestID == id, let index = messages.firstIndex(where: { $0.id == reply.id }) { markThought(index, since: asked) }
            self?.finish(id, failure: failure)
        }
    }

    private func markThought(_ index: Int, since asked: ContinuousClock.Instant) {
        let elapsed = ContinuousClock.now - asked
        var annotations = messages[index].annotations ?? ChatTurnAnnotations()
        annotations.markThought(elapsedMs: Int(elapsed.components.seconds * 1000 + elapsed.components.attoseconds / 1_000_000_000_000_000))
        messages[index].annotations = annotations
    }

    /// The answer ended by itself: it finished, or the request failed.
    private func finish(_ id: UUID, failure: Error?) {
        guard requestID == id else { return }
        if let failure {
            error = failure.localizedDescription
            markInterruptedReply()
        }
        finishReply(); busy = false; requestID = nil; request = nil; deltaBuffer = nil
        endActivity(finished: failure == nil)
        history = ServerChatSessions.archive(messages, in: history, threadID: threadID)
        persist(self)
        askNextQueued()
    }

    /// The question that waited longest goes out when the answer before it has ended,
    /// whether that answer finished or failed.
    private func askNextQueued() {
        guard !busy, !ended, !queue.isEmpty else { return }
        guard currentAccount() == accountIdentity else { end(); return }
        let next = queue.removeFirst()
        start(next.text, model: next.model)
    }

    private func cancelRequest(saving: Bool = true) {
        // Show everything received before stopping.
        deltaBuffer?.flushNow()
        deltaBuffer = nil
        markInterruptedReply()
        request?.cancel(); request = nil; requestID = nil; busy = false
        endActivity(finished: false)
        finishReply()
        guard saving else { return }
        history = ServerChatSessions.archive(messages, in: history, threadID: threadID)
        persist(self)
    }

    private func endActivity(finished: Bool) {
        guard let id = activityID else { return }
        activityID = nil
        activity.end(id, finished)
    }

    private func markInterruptedReply() {
        guard messages.last?.role == .assistant, messages.last?.isStreaming == true else { return }
        messages[messages.count - 1].isInterrupted = true
    }

    private func finishReply() {
        guard messages.last?.role == .assistant else { return }
        if messages.last?.text.isEmpty == true && messages.last?.annotations?.hasContent != true { messages.removeLast() }
        else { messages[messages.count - 1].isStreaming = false }
    }
}

/// A little more running time for an answer when the app leaves the foreground.
/// iOS grants about half a minute; after that the connection is cut and the
/// answer is kept as far as it got.
struct ChatBackgroundTime {
    var begin: @MainActor () -> Int
    var end: @MainActor (Int) -> Void

    static let none = ChatBackgroundTime(begin: { 0 }, end: { _ in })

    @MainActor static let application = ChatBackgroundTime(
        begin: {
            var identifier = UIBackgroundTaskIdentifier.invalid
            identifier = UIApplication.shared.beginBackgroundTask(withName: "chat-answer") {
                UIApplication.shared.endBackgroundTask(identifier)
            }
            return identifier.rawValue
        },
        end: { raw in
            let identifier = UIBackgroundTaskIdentifier(rawValue: raw)
            if identifier != .invalid { UIApplication.shared.endBackgroundTask(identifier) }
        })
}
