import Foundation
import Observation
import APIClient
import Contracts

/// Saved chats on the account's server (`/api/v1/chats`): the same threads the
/// person has on the Mac and the web, and nobody else's.
///
/// A conversation here becomes a server thread the first time a question is
/// asked in it (`ServerChatSession.threadID`). The server then saves each
/// question and its answer as the answer arrives; this device only reads them,
/// and keeps no copy of its own on disk.
enum ChatThreadTurns {
    /// A thread from the server as this app shows a conversation.
    static func messages(of thread: ChatThreadDTO) -> [ChatMessage] {
        thread.messages.sorted { ($0.turn, $0.role == .user ? 0 : 1) < ($1.turn, $1.role == .user ? 0 : 1) }.compactMap { message in
            let id = UUID(uuidString: message.id) ?? UUID()
            if message.role == .user { return ChatMessage(id: id, role: .user, text: message.content) }
            var annotations = ChatTurnAnnotations(saved: message.reasoning, elapsedMs: message.elapsedMs)
            for tool in message.tools { annotations.apply(ChatStreamEventDTO(text: "", tool: tool, type: .tool)) }
            if !message.citations.isEmpty { annotations.apply(ChatStreamEventDTO(citations: message.citations, text: "", type: .citations)) }
            var text = message.content
            // An answer the server failed is not one the person stopped: it carries no "Stopped".
            if message.status == .failed {
                annotations.markFailed()
                if text.isEmpty { text = message.error ?? "The answer could not be completed." }
            }
            guard !text.isEmpty || annotations.hasContent else { return nil }
            // How long it thought is kept even with nothing else to show: its line says so.
            let kept = annotations.hasContent || annotations.elapsedMs != nil || annotations.failed
            return ChatMessage(id: id, role: .assistant, text: text, isInterrupted: message.status == .interrupted || message.status == .streaming,
                               annotations: kept ? annotations : nil)
        }
    }

    /// What a saved chat is called in a list. The server names a chat once its first answer is
    /// complete, so one whose first question failed or was stopped has no name there. `asked` is
    /// what this iPhone knows it was asked (the first question of each chat started here, by
    /// thread), and names it meanwhile, as a chat on screen is named by its question.
    static func title(_ summary: ChatThreadSummaryDTO, asked: [String: String] = [:]) -> String {
        if let title = summary.title, !title.trimmingCharacters(in: .whitespaces).isEmpty { return title }
        if let question = asked[summary.id.lowercased()].map(questionTitle), !question.isEmpty { return question }
        return untitled
    }

    /// Nothing else in the app calls a chat a "thread".
    static let untitled = "Untitled chat"

    /// A question as a title: one line, as the server cuts it (`questionTitle` in apps/api/src/chats.ts).
    static func questionTitle(_ question: String) -> String {
        let line = question.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        return line.count <= 70 ? line : line.prefix(69).trimmingCharacters(in: .whitespaces) + "…"
    }

    /// The first question of each chat this iPhone has asked in, by the thread it is saved as.
    static func asked(in conversations: [ChatConversation]) -> [String: String] {
        var asked: [String: String] = [:]
        for conversation in conversations {
            guard let thread = conversation.threadID?.lowercased(),
                  let question = conversation.messages.first(where: { $0.role == .user })?.text, !question.isEmpty else { continue }
            asked[thread] = question
        }
        return asked
    }

    /// A thread nobody asked anything in and nobody named: a chat whose first
    /// question never reached the server. It is not worth a row.
    static func isEmpty(_ summary: ChatThreadSummaryDTO) -> Bool {
        summary.turnCount == 0 && (summary.title ?? "").trimmingCharacters(in: .whitespaces).isEmpty
    }

    /// How long ago, as the Mac's sidebar writes it: "now", "5m", "6h", "1d".
    static func age(of date: Date, now: Date = Date()) -> String {
        let minutes = Int(max(0, now.timeIntervalSince(date)) / 60)
        if minutes < 1 { return "now" }
        if minutes < 60 { return "\(minutes)m" }
        if minutes < 24 * 60 { return "\(minutes / 60)h" }
        return "\(minutes / (24 * 60))d"
    }

    /// The same age read aloud: "5 minutes ago".
    static func spokenAge(of date: Date, now: Date = Date()) -> String {
        let minutes = Int(max(0, now.timeIntervalSince(date)) / 60)
        func count(_ value: Int, _ unit: String) -> String { "\(value) \(unit)\(value == 1 ? "" : "s") ago" }
        if minutes < 1 { return "just now" }
        if minutes < 60 { return count(minutes, "minute") }
        if minutes < 24 * 60 { return count(minutes / 60, "hour") }
        return count(minutes / (24 * 60), "day")
    }
}

/// The saved threads of one account for one note, or for all notes: newest
/// first, a page at a time, with how many more there are.
@MainActor @Observable final class ServerChatThreadList {
    private(set) var threads: [ChatThreadSummaryDTO] = []
    private(set) var total = 0
    private(set) var loading = false
    var error: String?
    @ObservationIgnored private var nextCursor: String?
    @ObservationIgnored private var generation = 0

    /// The threads worth a row: every one but the empty, unnamed ones.
    var listed: [ChatThreadSummaryDTO] { threads.filter { !ChatThreadTurns.isEmpty($0) } }

    /// "Show N more".
    var more: Int { max(0, total - threads.count) }

    func refresh(server: ServerNotesModel, noteID: String?) async {
        generation += 1
        let mine = generation
        loading = true
        defer { if mine == generation { loading = false } }
        do {
            let page = try await server.chatThreads(noteID: noteID)
            guard mine == generation else { return }
            threads = page.threads; total = page.total; nextCursor = page.nextCursor; error = nil
        } catch is CancellationError {
        } catch { if mine == generation { self.error = error.localizedDescription } }
    }

    func loadMore(server: ServerNotesModel, noteID: String?) async {
        guard let cursor = nextCursor, !loading else { return }
        let mine = generation
        loading = true
        defer { if mine == generation { loading = false } }
        do {
            let page = try await server.chatThreads(noteID: noteID, cursor: cursor)
            guard mine == generation else { return }
            let known = Set(threads.map(\.id))
            threads += page.threads.filter { !known.contains($0.id) }
            total = page.total; nextCursor = page.nextCursor
        } catch is CancellationError {
        } catch { if mine == generation { self.error = error.localizedDescription } }
    }

    func delete(_ id: String, server: ServerNotesModel) async {
        do {
            try await server.deleteChatThread(id: id)
        } catch APIClientError.notFound {
        } catch { self.error = error.localizedDescription; return }
        threads.removeAll { $0.id == id }
        total = max(0, total - 1)
    }
}
