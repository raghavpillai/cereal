// Cereal's notifications, delivered by macOS (UserNotifications): meeting
// reminders planned ahead for the time Rust chose, and notices posted when
// something happens that matters while the person is in another app (notes
// ready, a recording that stopped, a session that ended). Rust decides what
// is posted and when (see `src/notifications.rs`); this file hands it to
// macOS and reports what was chosen, through the menu bar's callback.
import AppKit
import UserNotifications

struct ReminderEntry: Decodable, Sendable {
    let id: String
    let title: String
    let body: String
    /// Seconds since 1970.
    let fireAt: Double
    let eventId: String
    /// The account the event was read through. Absent before accounts.
    let account: String?
}

struct ReminderChange: Decodable, Sendable {
    let remove: [String]
    let add: [ReminderEntry]
}

/// A notice delivered at once.
struct NoticeEntry: Decodable, Sendable {
    let id: String
    /// Its category: "notes", "recording" or "account".
    let kind: String
    let title: String
    let body: String
    /// What a click opens, read back by Rust (`notifications::route`).
    let target: String
    /// Posted only while Cereal is not the app in front with its window
    /// on screen: then the window already shows it.
    let background: Bool
    /// A notice that replaces one already said is delivered without sound.
    let sound: Bool?
}

@MainActor
final class Notifications: NSObject, UNUserNotificationCenterDelegate {
    static let shared = Notifications()

    /// Notifications belong to an app bundle; a bare executable has none.
    var available: Bool {
        Bundle.main.bundleURL.pathExtension == "app" && Bundle.main.bundleIdentifier != nil
    }

    /// The buttons each kind of notification offers beside a click on it.
    /// A click opens what it is about; an action brings Cereal forward too.
    private static var categories: Set<UNNotificationCategory> {
        func category(_ id: String, _ action: String, _ title: String) -> UNNotificationCategory {
            UNNotificationCategory(
                identifier: id,
                actions: [UNNotificationAction(identifier: action, title: title, options: [.foreground])],
                intentIdentifiers: [],
                options: [])
        }
        return [
            category("meeting", "record", "Start recording"),
            category("notes", "open", "Open"),
            category("recording", "open", "Open note"),
            category("account", "sign-in", "Sign in"),
        ]
    }

    func listen() {
        guard available else { return }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.setNotificationCategories(Self.categories)
    }

    private func report(_ status: UNAuthorizationStatus) {
        let word: String
        switch status {
        case .authorized, .provisional: word = "granted"
        case .denied: word = "denied"
        default: word = "undetermined"
        }
        MenuBar.shared.send("notifications", word)
    }

    /// Say what macOS allows now, without asking.
    func status() {
        guard available else {
            MenuBar.shared.send("notifications", "unavailable")
            return
        }
        UNUserNotificationCenter.current().getNotificationSettings { settings in
            let status = settings.authorizationStatus
            DispatchQueue.main.async { MainActor.assumeIsolated { Notifications.shared.report(status) } }
        }
    }

    /// Ask macOS for permission (it asks the person only the first time).
    /// The answer comes as "notifications-request". On macOS 26 a "denied"
    /// can come while macOS's own Allow banner is still on screen, so Rust
    /// does not take it as final; the status read later says what holds.
    func request() {
        guard available else {
            MenuBar.shared.send("notifications-request", "unavailable")
            return
        }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { granted, _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    MenuBar.shared.send("notifications-request", granted ? "granted" : "denied")
                }
            }
        }
    }

    /// Cereal is the app in front and one of its windows is on screen (it
    /// stays "active" for a moment after its window is hidden).
    private var inFront: Bool {
        NSApp.isActive
            && NSApp.windows.contains { $0.isVisible && !$0.isMiniaturized && $0.canBecomeMain && $0.isOnActiveSpace }
    }

    func apply(_ change: ReminderChange) {
        guard available else { return }
        let center = UNUserNotificationCenter.current()
        if !change.remove.isEmpty {
            center.removePendingNotificationRequests(withIdentifiers: change.remove)
        }
        for reminder in change.add {
            let content = UNMutableNotificationContent()
            content.title = reminder.title
            content.body = reminder.body
            content.sound = .default
            content.categoryIdentifier = "meeting"
            content.threadIdentifier = "meeting"
            // A reminder names its account: "<account>/<event>".
            let account = reminder.account ?? ""
            content.userInfo = [
                "kind": "meeting",
                "target": account.isEmpty ? reminder.eventId : account + "/" + reminder.eventId,
            ]
            let wait = reminder.fireAt - Date().timeIntervalSince1970
            let trigger: UNNotificationTrigger
            if wait <= 2 {
                trigger = UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)
            } else {
                // A date, so the reminder keeps its time across sleep.
                let parts = Calendar.current.dateComponents(
                    [.year, .month, .day, .hour, .minute, .second],
                    from: Date(timeIntervalSince1970: reminder.fireAt))
                trigger = UNCalendarNotificationTrigger(dateMatching: parts, repeats: false)
            }
            center.add(UNNotificationRequest(identifier: reminder.id, content: content, trigger: trigger))
        }
    }

    /// Deliver `notice` now. One posted again under the same identifier
    /// replaces the one in Notification Centre.
    func post(_ notice: NoticeEntry) {
        guard available, !(notice.background && inFront) else { return }
        let content = UNMutableNotificationContent()
        content.title = notice.title
        content.body = notice.body
        content.sound = (notice.sound ?? true) ? .default : nil
        content.categoryIdentifier = notice.kind
        content.threadIdentifier = notice.kind
        content.userInfo = ["kind": notice.kind, "target": notice.target]
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: notice.id, content: content, trigger: nil))
    }

    /// Take notifications out of Notification Centre, and any still waiting.
    func remove(_ ids: [String]) {
        guard available else { return }
        let center = UNUserNotificationCenter.current()
        center.removePendingNotificationRequests(withIdentifiers: ids)
        center.removeDeliveredNotifications(withIdentifiers: ids)
    }

    /// Everything this app has in Notification Centre, for checks:
    /// "<identifier>\t<category>\t<title>" lines.
    func delivered() {
        guard available else { return }
        UNUserNotificationCenter.current().getDeliveredNotifications { notifications in
            let lines = notifications.map {
                [$0.request.identifier, $0.request.content.categoryIdentifier, $0.request.content.title]
                    .joined(separator: "\t")
            }
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    MenuBar.shared.send("notifications-delivered", lines.joined(separator: "\n"))
                }
            }
        }
    }

    func clear() {
        guard available else { return }
        UNUserNotificationCenter.current().removeAllPendingNotificationRequests()
    }

    func clearDelivered() {
        guard available else { return }
        UNUserNotificationCenter.current().removeAllDeliveredNotifications()
    }

    /// Notifications → Cereal in System Settings, where a denial is undone.
    func openSettings() {
        let id = Bundle.main.bundleIdentifier ?? "dev.cereal.desktop"
        if let url = URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=\(id)") {
            NSWorkspace.shared.open(url)
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        // Shown even while Cereal is the app in front: a notice that only
        // matters in the background was not posted then.
        completionHandler([.banner, .sound, .list])
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let info = response.notification.request.content.userInfo
        let action: String
        switch response.actionIdentifier {
        case UNNotificationDefaultActionIdentifier: action = "open"
        case UNNotificationDismissActionIdentifier:
            completionHandler()
            return
        default: action = response.actionIdentifier
        }
        // A reminder delivered by an older build has only its event and
        // account.
        let kind = info["kind"] as? String ?? "meeting"
        let target: String
        if let named = info["target"] as? String {
            target = named
        } else {
            let event = info["eventId"] as? String ?? ""
            let account = info["account"] as? String ?? ""
            target = account.isEmpty ? event : account + "/" + event
        }
        // The title last: a meeting's, for a recording of its own when its
        // event can no longer be opened.
        let title = response.notification.request.content.title.replacingOccurrences(of: "\t", with: " ")
        let argument = [kind, action, target, title].joined(separator: "\t")
        DispatchQueue.main.async {
            MainActor.assumeIsolated { MenuBar.shared.send("notification", argument) }
        }
        completionHandler()
    }
}

@_cdecl("cereal_notifications_status")
public func cerealNotificationsStatus() {
    MainActor.assumeIsolated { Notifications.shared.status() }
}

@_cdecl("cereal_notifications_request")
public func cerealNotificationsRequest() {
    MainActor.assumeIsolated { Notifications.shared.request() }
}

@_cdecl("cereal_notifications_apply")
public func cerealNotificationsApply(_ json: UnsafePointer<CChar>) -> Bool {
    guard let change = decoded(ReminderChange.self, json) else { return false }
    MainActor.assumeIsolated { Notifications.shared.apply(change) }
    return true
}

@_cdecl("cereal_notifications_post")
public func cerealNotificationsPost(_ json: UnsafePointer<CChar>) -> Bool {
    guard let notice = decoded(NoticeEntry.self, json) else { return false }
    MainActor.assumeIsolated { Notifications.shared.post(notice) }
    return true
}

@_cdecl("cereal_notifications_remove")
public func cerealNotificationsRemove(_ json: UnsafePointer<CChar>) -> Bool {
    guard let ids = decoded([String].self, json) else { return false }
    MainActor.assumeIsolated { Notifications.shared.remove(ids) }
    return true
}

@_cdecl("cereal_notifications_delivered")
public func cerealNotificationsDelivered() {
    MainActor.assumeIsolated { Notifications.shared.delivered() }
}

@_cdecl("cereal_notifications_clear")
public func cerealNotificationsClear() {
    MainActor.assumeIsolated { Notifications.shared.clear() }
}

@_cdecl("cereal_notifications_clear_delivered")
public func cerealNotificationsClearDelivered() {
    MainActor.assumeIsolated { Notifications.shared.clearDelivered() }
}

@_cdecl("cereal_notifications_open_settings")
public func cerealNotificationsOpenSettings() {
    MainActor.assumeIsolated { Notifications.shared.openSettings() }
}
