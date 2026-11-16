import Foundation

/// The live socket over `URLSessionWebSocketTask`. The request authenticates
/// with the bearer header alone: the session keeps no cookies and never sends
/// an `Origin`, which the server would refuse.
final class URLSessionLiveConnector: LiveSocketConnector {
    private let request: URLRequest
    private let openTimeout: Duration

    init(request: URLRequest, openTimeout: Duration = .seconds(10)) {
        self.request = request
        self.openTimeout = openTimeout
    }

    func connect() async throws -> any LiveSocket {
        let socket = URLSessionLiveSocket(request: request)
        try await socket.open(timeout: openTimeout)
        return socket
    }
}

final class URLSessionLiveSocket: NSObject, LiveSocket, URLSessionWebSocketDelegate, @unchecked Sendable {
    private let lock = NSLock()
    private var session: URLSession!
    private var task: URLSessionWebSocketTask!
    private var opening: CheckedContinuation<Void, Error>?
    private var opened = false
    private var closeCode: Int?
    private var closed = false

    init(request: URLRequest) {
        super.init()
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieAcceptPolicy = .never
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.waitsForConnectivity = false
        // A paused stream is quiet for minutes. Opening has its own shorter limit.
        configuration.timeoutIntervalForRequest = 600
        session = URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
        task = session.webSocketTask(with: request)
        task.maximumMessageSize = 1 << 20
    }

    func open(timeout: Duration) async throws {
        let limit = Task { [weak self] in
            try? await Task.sleep(for: timeout)
            if !Task.isCancelled { self?.failOpening() }
        }
        defer { limit.cancel() }
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                lock.lock(); opening = continuation; lock.unlock()
                task.resume()
            }
        } onCancel: { failOpening() }
    }

    private func failOpening() {
        lock.lock(); let pending = opened ? nil : opening; if !opened { opening = nil }; lock.unlock()
        guard let pending else { return }
        close(code: 1000)
        pending.resume(throwing: LiveSocketError.transport)
    }

    func send(text: String) async throws {
        do { try await task.send(.string(text)) } catch { throw failure() }
    }

    func send(data: Data) async throws {
        do { try await task.send(.data(data)) } catch { throw failure() }
    }

    func receive() async throws -> LiveSocketMessage {
        do {
            switch try await task.receive() {
            case .string(let text): return .text(text)
            case .data(let data): return .data(data)
            @unknown default: return .data(Data())
            }
        } catch {
            // The server's close code can reach the delegate a moment after the failed receive. It
            // matters: only an orderly close after `mark: end` ends a capture's stream.
            if lock.withLock({ closeCode == nil && !closed }), task.closeCode == .invalid {
                try? await Task.sleep(for: .milliseconds(150))
            }
            throw failure()
        }
    }

    func ping() async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            task.sendPing { error in
                if error == nil { continuation.resume() } else { continuation.resume(throwing: LiveSocketError.transport) }
            }
        }
    }

    func close(code: Int) {
        lock.lock(); let already = closed; closed = true; lock.unlock()
        guard !already else { return }
        task.cancel(with: URLSessionWebSocketTask.CloseCode(rawValue: code) ?? .normalClosure, reason: nil)
        session.invalidateAndCancel()
    }

    /// The server's close code when it sent one, otherwise a lost connection.
    private func failure() -> LiveSocketError {
        lock.lock(); let recorded = closeCode; lock.unlock()
        if let recorded { return .closed(code: recorded) }
        let code = task.closeCode.rawValue
        return code > 0 ? .closed(code: code) : .transport
    }

    // MARK: URLSessionWebSocketDelegate

    func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didOpenWithProtocol protocol: String?) {
        lock.lock(); opened = true; let pending = opening; opening = nil; lock.unlock()
        pending?.resume()
    }

    func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didCloseWith closeCode: URLSessionWebSocketTask.CloseCode, reason: Data?) {
        lock.lock(); self.closeCode = closeCode.rawValue; lock.unlock()
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: (any Error)?) {
        lock.lock(); let pending = opening; opening = nil; lock.unlock()
        // A refused upgrade arrives here as an ordinary HTTP response.
        if let pending {
            let status = (task.response as? HTTPURLResponse)?.statusCode
            pending.resume(throwing: status.map { $0 == 101 ? LiveSocketError.transport : LiveSocketError.refused(status: $0) } ?? LiveSocketError.transport)
        }
        session.finishTasksAndInvalidate()
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil) // An authenticated request never follows a redirect.
    }
}
