import AuthenticationServices
import UIKit
import APIClient

/// The system's sign-in browser. It shares Safari's session, so someone already
/// signed in to their server on the web is offered "Continue as …" there, and
/// it hands the page's `cereal://auth` answer back here instead of opening the app.
@MainActor
final class BrowserSignInSession {
    private var session: ASWebAuthenticationSession?
    private var anchor: Anchor?

    /// Opens `url` and returns the answer the page navigates to. Throws
    /// `BrowserSignInError.cancelled` when the user closes the browser.
    func answer(from url: URL) async throws -> URL {
        session?.cancel()
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        guard let window = scenes.flatMap(\.windows).first(where: \.isKeyWindow) ?? scenes.first?.windows.first else {
            throw BrowserSignInError.browserUnavailable
        }
        let anchor = Anchor(window: window)
        self.anchor = anchor
        let result: Result<URL, BrowserSignInError> = await withCheckedContinuation { continuation in
            let once = Once(continuation)
            let session = ASWebAuthenticationSession(url: url, callback: .customScheme(AppIdentity.urlScheme)) { answer, error in
                if let answer { once.finish(.success(answer)) }
                else if (error as? ASWebAuthenticationSessionError)?.code == .canceledLogin { once.finish(.failure(.cancelled)) }
                else { once.finish(.failure(.browserUnavailable)) }
            }
            session.presentationContextProvider = anchor
            session.prefersEphemeralWebBrowserSession = false
            self.session = session
            if !session.start() { once.finish(.failure(.browserUnavailable)) }
        }
        session = nil
        self.anchor = nil
        return try result.get()
    }
}

/// The window the browser appears over.
@MainActor
private final class Anchor: NSObject, ASWebAuthenticationPresentationContextProviding {
    let window: UIWindow
    init(window: UIWindow) { self.window = window }
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor { window }
}

/// The browser reports its result once, but a session that fails to start may
/// report nothing. Whichever comes first answers; the rest is ignored.
private final class Once: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Result<URL, BrowserSignInError>, Never>?

    init(_ continuation: CheckedContinuation<Result<URL, BrowserSignInError>, Never>) { self.continuation = continuation }

    func finish(_ result: Result<URL, BrowserSignInError>) {
        lock.lock()
        let waiting = continuation
        continuation = nil
        lock.unlock()
        waiting?.resume(returning: result)
    }
}
