import SwiftUI
import APIClient
import UserNotifications

/// The pages under Settings.
enum SettingsPage: Hashable {
    case main, profile, notifications
}

// MARK: - Home navigation

/// The navigation controls of Home, as in the reference: a single folder circle
/// (the notes library) and a capsule of search and the account avatar, which
/// opens Settings. On a Duo the system stacks them in the
/// trailing rail; on an ordinary iPhone they stay in the top bar.
enum HomeNavigation {
    struct Notes: View {
        var body: some View {
            GlassCircleButton(systemImage: "folder") { SidebarModel.shared.open() }
        }
    }

    struct Group: View {
        let name: String
        @Environment(Router.self) private var router

        var body: some View {
            RailGroup {
                RailIcon(systemImage: "magnifyingglass") { router.path.append(.search) }
                RailAvatar(name: name) { router.path.append(.settings(.main)) }
            }
        }
    }
}

// MARK: - Settings

/// Sizes of the Settings pages, measured from the reference at 3 px per point.
enum SettingsMetrics {
    static let cardRadius: CGFloat = 26
    static let rowHeight: CGFloat = 52
    static let iconTile: CGFloat = 31
    static let label: CGFloat = 17
    static let header: CGFloat = 15
    /// Where a row's text starts inside a card: the 11 pt inset, the icon tile and the 14 pt gap.
    static let textLeading: CGFloat = 11 + iconTile + 14
    /// Cards sit 20 pt from the screen edge on a Duo, 16 on an iPhone.
    static func margin(_ rail: RailMetrics) -> CGFloat { rail.hasRail ? 20 : 16 }
}

struct SettingsView: View {
    @Environment(AccountsModel.self) private var accounts
    @Environment(Router.self) private var router
    @Environment(\.openURL) private var openURL
    @AppStorage(ThemePreference.storageKey) private var theme: ThemePreference = .auto
    /// What "Share Cereal for iPhone" shares: the App Store page when the account's server names one.
    @State private var shareURL: URL?

    private var session: AccountSession? { accounts.defaultSession }
    /// Where the account's server offers its apps; without an account, the project's page.
    private var downloadPage: URL { session?.notes.instance?.url.appendingPathComponent("download") ?? SettingsLinks.help }

    var body: some View {
        RailScaffold {
            SettingsScroll(title: "Settings") {
                // The Mac app is offered by the account's own server; without one, by the project's page.
                SettingsBanner(title: "Get Cereal for Mac", systemImage: "laptopcomputer") { openURL(downloadPage) }
                SettingsSection("Account") {
                    SettingsRow("Profile", systemImage: "person") { router.path.append(.settings(.profile)) }
                    SettingsRow("Notifications", systemImage: "bell") { router.path.append(.settings(.notifications)) }
                    SettingsRow(Wording.accountSheet, systemImage: "server.rack") { router.showConnection = true }
                }
                SettingsSection("Appearance") {
                    ThemeRow(theme: $theme)
                    HapticsRow()
                }
                SettingsSection("Support") {
                    // Every row here leaves the app, and says so with the same glyph.
                    SettingsRow("Send feedback", systemImage: "envelope", trailing: .external) { openURL(SettingsLinks.feedback) }
                    SettingsRow("Report a bug", systemImage: "exclamationmark.triangle", trailing: .external) { openURL(SettingsLinks.bug) }
                    SettingsRow("Help center", systemImage: "book.closed", trailing: .external) { openURL(SettingsLinks.help) }
                    ShareLink(item: shareURL ?? downloadPage) {
                        SettingsRowLabel("Share Cereal for iPhone", systemImage: "square.and.arrow.up", trailing: .none)
                    }
                    .buttonStyle(.row)
                }
            }
        } leading: {
            BackButton()
        } rail: {
            EmptyView()
        }
        .task(id: session?.key) {
            shareURL = nil
            guard let session, session.isReady, let answer = try? await session.notes.downloads() else { return }
            shareURL = SettingsLinks.appStore(answer.iosAppStoreUrl)
        }
    }
}

enum SettingsLinks {
    static let help = URL(string: "https://github.com/raghavpillai/cereal#readme")!
    static let bug = URL(string: "https://github.com/raghavpillai/cereal/issues/new")!
    static let feedback = URL(string: "https://github.com/raghavpillai/cereal/issues/new?labels=feedback")!

    /// The App Store page a server names for the iPhone app. Only an https App Store address is shared.
    static func appStore(_ text: String?) -> URL? {
        text.flatMap(URL.init(string:)).flatMap { $0.scheme == "https" && $0.host == "apps.apple.com" ? $0 : nil }
    }
}

/// The Theme row: the current choice with its symbol and an up-down chevron,
/// opening a menu of Light, Dark and Auto. The menu opens from the value
/// (`ValueMenu`); the row stays where it is.
private struct ThemeRow: View {
    @Binding var theme: ThemePreference

    var body: some View {
        HStack(spacing: 14) {
            SettingsIconTile(systemImage: "circle.lefthalf.filled")
            Text("Theme").uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary)
            ValueMenu(value: theme.title, symbol: theme.symbol, size: SettingsMetrics.label) {
                // The choice is applied once the menu has closed, and the window cross-fades.
                Picker("Theme", selection: Binding(get: { theme }, set: { choice in ThemeWindows.choose(choice) { theme = $0 } })) {
                    ForEach(ThemePreference.allCases) { choice in
                        Label(choice.title, systemImage: choice.symbol).tag(choice)
                    }
                }
            }
            .accessibilityIdentifier("settings-theme")
            .accessibilityLabel("Theme")
            .accessibilityValue(theme.title)
        }
        .padding(.leading, 11).padding(.trailing, 18)
        .frame(minHeight: SettingsMetrics.rowHeight)
        .haptic(.selection, trigger: theme)
    }
}

/// Haptics: on unless turned off. Off silences the app's own haptics, the
/// recording's signatures included; the system's own (keyboard, menus) follow
/// iOS's System Haptics setting as they always do.
private struct HapticsRow: View {
    @AppStorage(HapticGate.storageKey) private var enabled = true

    var body: some View {
        SettingsToggleRow("Haptics", systemImage: "hand.tap", isOn: $enabled)
            // Turned on, it says so in the hand.
            .haptic(.selection, trigger: enabled) { _, on in on }
            .accessibilityIdentifier("settings-haptics")
    }
}

/// The rounded square that leads a Settings row.
struct SettingsIconTile: View {
    let systemImage: String
    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: 17))
            .foregroundStyle(Theme.textPrimary.opacity(0.85))
            .frame(width: SettingsMetrics.iconTile, height: SettingsMetrics.iconTile)
            .background(Theme.iconTile, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            .accessibilityHidden(true)
    }
}

/// A switch as a Settings row: the icon tile, the label, an optional line of explanation.
struct SettingsToggleRow: View {
    let title: String
    let systemImage: String
    var detail: String? = nil
    @Binding var isOn: Bool

    init(_ title: String, systemImage: String, detail: String? = nil, isOn: Binding<Bool>) {
        self.title = title; self.systemImage = systemImage; self.detail = detail; _isOn = isOn
    }

    var body: some View {
        Toggle(isOn: $isOn) {
            HStack(spacing: 14) {
                SettingsIconTile(systemImage: systemImage)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary)
                    if let detail { Text(detail).uiFont(14).foregroundStyle(Theme.textSecondary) }
                }
            }
        }
        .tint(Theme.accent)
        .padding(.leading, 11).padding(.trailing, 18)
        .padding(.vertical, detail == nil ? 0 : 8)
        .frame(minHeight: SettingsMetrics.rowHeight)
    }
}

// MARK: - Profile

struct ProfileSettingsView: View {
    @Environment(AccountsModel.self) private var accounts
    @AppStorage("profile.jobTitle") private var jobTitle = ""
    @AppStorage("profile.company") private var company = ""
    @AppStorage("profile.userType") private var userType = "Startup/Generalist"
    @AppStorage("profile.companyDescription") private var companyDescription = ""
    @AppStorage("profile.linkedIn") private var linkedIn = ""

    @FocusState private var linkedInFocused: Bool
    @Environment(\.motion) private var motion

    private var record: AccountRecord? { accounts.defaultSession?.record }
    static let userTypes = ["Startup/Generalist", "Engineering", "Product", "Design", "Sales", "Investor", "Recruiting", "Other"]

    var body: some View {
        RailScaffold {
            ScrollViewReader { scroll in
            SettingsScroll(title: nil) {
                    Text("Your profile")
                        .displayFont(30).foregroundStyle(Theme.textPrimary)
                        .accessibilityAddTraits(.isHeader)
                        .padding(.top, 24)
                    Text("Cereal works best when it knows a little about you. These details stay on this iPhone.")
                        .uiFont(14).foregroundStyle(Theme.textPrimary.opacity(0.85))
                        .padding(.top, 10)
                    VStack(spacing: 10) {
                        AvatarView(name: record?.name ?? "", size: 96)
                        Text(record?.email ?? "Not signed in")
                            .uiFont(14).foregroundStyle(Theme.textSecondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 20).padding(.bottom, 16)

                    SettingsCard {
                        ProfileField("Name") { Text(record?.name ?? "").foregroundStyle(Theme.textPrimary) }
                        // Names of things: sentence case in the label, and the keyboard does not "correct" them.
                        ProfileField("Job title") { field($jobTitle, prompt: "Your job title", capitalization: .sentences) }
                        ProfileField("Company name") { field($company, prompt: "Your company", capitalization: .words) }
                        ProfileField("User type", divider: false) {
                            ValueMenu(value: userType, size: 14) {
                                Picker("User type", selection: $userType) {
                                    ForEach(Self.userTypes, id: \.self) { Text($0).tag($0) }
                                }
                            }
                            .haptic(.selection, trigger: userType)
                        }
                    }
                    SettingsCard {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Company description").uiFont(14).foregroundStyle(Theme.textPrimary)
                            TextField("", text: $companyDescription, axis: .vertical)
                                .uiFont(14).foregroundStyle(Theme.textPrimary)
                                .lineLimit(4, reservesSpace: true)
                                .accessibilityLabel("Company description")
                        }
                        .padding(.horizontal, 14).padding(.vertical, 14)
                    }
                    .padding(.top, 12)
                    SettingsCard {
                        ProfileField("LinkedIn username") {
                            // A username: never capitalised, never corrected.
                            field($linkedIn, prompt: "username", capitalization: .never)
                                .keyboardType(.asciiCapable)
                                .focused($linkedInFocused)
                                .accessibilityIdentifier("profile-linkedin")
                        }
                        Text("linkedin.com/in/\(linkedIn)")
                            .uiFont(14).foregroundStyle(Theme.textSecondary)
                            .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                            .padding(.horizontal, 14)
                    }
                    .padding(.top, 12)
                    .id(Self.linkedInCard)
            }
            .scrollDismissesKeyboard(.interactively)
            // The address being built sits under the field: once the keyboard is up,
            // the whole card is brought above it, not only the line being typed in.
            .onChange(of: linkedInFocused) { _, focused in
                guard focused else { return }
                Task {
                    try? await Task.sleep(for: .milliseconds(350))
                    withAnimation(motion.snappy) { scroll.scrollTo(Self.linkedInCard, anchor: .bottom) }
                }
            }
            }
        } leading: {
            BackButton()
        } rail: {
            EmptyView()
        }
    }

    private static let linkedInCard = "linkedin-card"

    private func field(_ text: Binding<String>, prompt: String, capitalization: TextInputAutocapitalization) -> some View {
        TextField("", text: text, prompt: Text(prompt).foregroundStyle(Theme.textTertiary))
            .multilineTextAlignment(.trailing)
            .foregroundStyle(Theme.textPrimary)
            .textInputAutocapitalization(capitalization)
            .autocorrectionDisabled()
            .accessibilityLabel(prompt)
    }
}

/// One line of the profile card: the label at the leading edge, the value at the trailing.
private struct ProfileField<Value: View>: View {
    let label: String
    var divider = true
    @ViewBuilder var value: Value

    init(_ label: String, divider: Bool = true, @ViewBuilder value: () -> Value) {
        self.label = label; self.divider = divider; self.value = value()
    }

    var body: some View {
        HStack {
            Text(label).foregroundStyle(Theme.textPrimary)
            Spacer(minLength: 12)
            value
        }
        .uiFont(14)
        .padding(.horizontal, 14)
        .frame(minHeight: 40)
        .overlay(alignment: .bottom) {
            if divider { Rectangle().fill(Theme.divider.opacity(0.7)).frame(height: 0.5).padding(.horizontal, 14) }
        }
    }
}

// MARK: - Notifications

/// Reminders of upcoming meetings on the lock screen, at each meeting's start.
/// They are scheduled from the calendar events Home reads for today.
enum UpcomingMeetingNotifications {
    static let storageKey = "notifications.upcomingMeetings"
    private static let prefix = "upcoming-meeting-"
    /// The line under the meeting's title in a reminder. The example in Settings shows the same words.
    static let body = "Starting now. Open Cereal to take notes."

    static var isEnabled: Bool { UserDefaults.standard.bool(forKey: storageKey) }

    /// Asks for permission. False when the person declined.
    static func requestAuthorization() async -> Bool {
        (try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])) ?? false
    }

    /// Replaces the reminders with one for each event yet to start.
    static func schedule(_ events: [(id: String, title: String, start: Date)], now: Date = .now) async {
        let center = UNUserNotificationCenter.current()
        let pending = await center.pendingNotificationRequests().map(\.identifier).filter { $0.hasPrefix(prefix) }
        center.removePendingNotificationRequests(withIdentifiers: pending)
        guard isEnabled else { return }
        for event in events where event.start > now {
            let content = UNMutableNotificationContent()
            content.title = event.title.isEmpty ? "Meeting" : event.title
            content.body = body
            let parts = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute], from: event.start)
            let request = UNNotificationRequest(identifier: prefix + event.id, content: content,
                                                trigger: UNCalendarNotificationTrigger(dateMatching: parts, repeats: false))
            try? await center.add(request)
        }
    }

    static func clear() {
        let center = UNUserNotificationCenter.current()
        Task {
            let pending = await center.pendingNotificationRequests().map(\.identifier).filter { $0.hasPrefix(prefix) }
            center.removePendingNotificationRequests(withIdentifiers: pending)
        }
    }
}

struct NotificationSettingsView: View {
    @AppStorage(UpcomingMeetingNotifications.storageKey) private var upcoming = false
    @AppStorage(MeetingLiveActivities.storageKey) private var meetingCards = true
    @State private var denied = false
    @Environment(\.openURL) private var openURL
    @Environment(\.motion) private var motion

    var body: some View {
        RailScaffold {
            SettingsScroll(title: "Notifications") {
                SettingsSection("Reminders") {
                    SettingsToggleRow("Upcoming meetings", systemImage: "bell",
                                      detail: "A notification on the Lock Screen as each meeting starts",
                                      isOn: Binding(get: { upcoming }, set: { set($0) }))
                        .accessibilityIdentifier("notifications-upcoming")
                    // Asked for and refused: the line under the switch says so and offers the way to turn them on.
                    if denied {
                        VStack(alignment: .leading, spacing: 0) {
                            Text("Notifications are off for Cereal.").uiFont(14).foregroundStyle(Theme.textSecondary)
                            Button("Open Settings") {
                                if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                            }
                            .uiFont(14, weight: .medium).foregroundStyle(Theme.accentText)
                            .frame(minHeight: 44).contentShape(Rectangle())
                            .buttonStyle(.squash)
                            .accessibilityIdentifier("notifications-open-settings")
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.leading, SettingsMetrics.textLeading).padding(.trailing, 18)
                        .transition(motion.rise)
                    }
                    // What this switch sends, under the switch that sends it.
                    UpcomingMeetingPreview()
                        .padding(.leading, SettingsMetrics.textLeading).padding(.trailing, 18)
                        .padding(.top, 4).padding(.bottom, 10)
                }
                SettingsSection("Live Activities") {
                    SettingsToggleRow("Take a note button", systemImage: "waveform",
                                      detail: "A Live Activity with a Take a note button as each meeting starts",
                                      isOn: $meetingCards)
                        .accessibilityIdentifier("notifications-meeting-cards")
                        .onChange(of: meetingCards) { _, on in if !on { Task { await MeetingLiveActivities.endAll() } } }
                }
            }
            // The refusal line opens its own space: the card grows and what is under it glides.
            .animates(denied)
        } leading: {
            BackButton()
        } rail: {
            EmptyView()
        }
        .haptic(.warning, trigger: denied) { _, now in now }
        .haptic(.selection, trigger: upcoming)
        .haptic(.selection, trigger: meetingCards)
    }

    private func set(_ on: Bool) {
        guard on else { upcoming = false; denied = false; UpcomingMeetingNotifications.clear(); return }
        Task {
            let granted = await UpcomingMeetingNotifications.requestAuthorization()
            denied = !granted
            upcoming = granted
        }
    }
}

/// What a reminder looks like, in the words of the real one (`UpcomingMeetingNotifications.schedule`).
private struct UpcomingMeetingPreview: View {
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 16, style: .continuous)
        VStack(alignment: .leading, spacing: 6) {
            Text("Example").uiFont(12, weight: .medium).foregroundStyle(Theme.textTertiary)
            HStack(alignment: .top, spacing: 10) {
                CerealMarkShape().fill(Theme.accent, style: FillStyle(eoFill: true))
                    .frame(width: 18, height: 18)
                    .frame(width: 34, height: 34)
                    .background(Theme.iconTile, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text("Catch up with the team").uiFont(15, weight: .semibold).foregroundStyle(Theme.textPrimary)
                        Spacer(minLength: 8)
                        Text("now").uiFont(13).foregroundStyle(Theme.textTertiary)
                    }
                    Text(UpcomingMeetingNotifications.body).uiFont(14).foregroundStyle(Theme.textSecondary)
                }
            }
            .padding(12)
            .background(Theme.bgTop, in: shape)
            .overlay(shape.strokeBorder(Theme.stroke, lineWidth: 1))
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Example reminder: Catch up with the team. \(UpcomingMeetingNotifications.body)")
    }
}

// MARK: - Building blocks

/// The scrolling column of a Settings page. Beside a Duo rail the title sits
/// centred at the top of the page and scrolls with it; on an iPhone it is the
/// navigation bar's title.
struct SettingsScroll<Content: View>: View {
    let title: String?
    @ViewBuilder var content: Content
    @Environment(\.railMetrics) private var rail

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if rail.hasRail, let title {
                    Text(title).uiFont(17, weight: .semibold).foregroundStyle(Theme.textPrimary)
                        .frame(maxWidth: .infinity).padding(.top, 20).padding(.bottom, 26)
                        .accessibilityAddTraits(.isHeader)
                }
                VStack(alignment: .leading, spacing: 0) { content }
            }
            // The page pads nothing itself; cards keep the measured margin from the screen edge.
            .padding(.leading, SettingsMetrics.margin(rail) - rail.extraLeading)
            .padding(.trailing, SettingsMetrics.margin(rail) - rail.extraTrailing)
            .padding(.top, rail.hasRail ? 0 : 12)
            .clearsBottomChrome()
        }
        .scrollIndicators(.hidden)
        // Settings text follows the text size up to the first accessibility size, where its rows still hold.
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
        // The navigation bar's title on an iPhone. Beside a rail the page draws its own.
        .navigationTitle(rail.hasRail ? "" : (title ?? ""))
    }
}

/// A section: a 15 pt grey header, 9 pt above its card.
struct SettingsSection<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content
    init(_ title: String, @ViewBuilder content: () -> Content) { self.title = title; self.content = content() }

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(title).uiFont(SettingsMetrics.header).foregroundStyle(Theme.textSecondary)
                .padding(.leading, 12)
                .accessibilityAddTraits(.isHeader)
            SettingsCard { content }
        }
        .padding(.top, 22)
    }
}

/// The rounded card that groups rows: #F6F6F2 on light, #242424 on dark, a hairline edge.
struct SettingsCard<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: SettingsMetrics.cardRadius, style: .continuous)
        VStack(alignment: .leading, spacing: 0) { content }
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.settingsCard, in: shape)
            .overlay(shape.strokeBorder(Theme.stroke, lineWidth: 1))
    }
}

/// The desktop-app banner at the head of Settings, tinted with the brand colour.
private struct SettingsBanner: View {
    let title: String
    let systemImage: String
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: 14) {
                Image(systemName: systemImage).font(.system(size: 18)).frame(width: 31)
                Text(title).uiFont(SettingsMetrics.label)
                Spacer(minLength: 8)
                // It opens the server's download page in the browser.
                Image(systemName: "arrow.up.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
            }
            .foregroundStyle(Theme.textPrimary)
            .padding(.leading, 14).padding(.trailing, 18)
            .frame(minHeight: 56)
            .background(Theme.accentTint, in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.stroke, lineWidth: 1))
            .contentShape(Capsule())
        }
        .buttonStyle(.squash)
    }
}

enum SettingsTrailing {
    /// Pushes a page or opens a sheet inside the app.
    case chevron
    /// Leaves the app (the browser, another app).
    case external
    case comingSoon
    /// Nothing trails the label (a share sheet, an action in place).
    case none
    case value(String, symbol: String)
    /// A fact at the trailing edge, with no control.
    case text(String)
}

/// A row's look: an icon in a rounded square tile, the label, and what trails it.
struct SettingsRowLabel: View {
    let title: String
    let systemImage: String
    var trailing: SettingsTrailing = .chevron

    init(_ title: String, systemImage: String, trailing: SettingsTrailing = .chevron, destructive: Bool = false) {
        self.title = title; self.systemImage = systemImage; self.trailing = trailing; self.destructive = destructive
    }

    /// A destructive row (Sign out, Remove) writes its label in the danger colour.
    var destructive = false

    private var muted: Bool { if case .comingSoon = trailing { true } else { false } }

    var body: some View {
        HStack(spacing: 14) {
            SettingsIconTile(systemImage: systemImage)
            Text(title).uiFont(SettingsMetrics.label)
                .foregroundStyle(destructive ? Theme.danger : muted ? Theme.textSecondary : Theme.textPrimary)
                .multilineTextAlignment(.leading)
            Spacer(minLength: 8)
            switch trailing {
            case .none: EmptyView()
            case .text(let text):
                Text(text).uiFont(SettingsMetrics.label).foregroundStyle(Theme.textSecondary).lineLimit(1)
            case .chevron:
                Image(systemName: "chevron.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
            case .external:
                Image(systemName: "arrow.up.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
            case .comingSoon:
                Text("Coming soon").uiFont(14).foregroundStyle(Theme.textSecondary)
                    .padding(.horizontal, 8).frame(height: 24)
                    .background(Theme.iconTile, in: Capsule())
            case .value(let text, let symbol):
                HStack(spacing: 6) {
                    Image(systemName: symbol).font(.system(size: 15))
                    Text(text).uiFont(SettingsMetrics.label)
                    Image(systemName: "chevron.up.chevron.down").font(.system(size: 12, weight: .medium))
                }
                .foregroundStyle(Theme.textPrimary)
            }
        }
        .padding(.leading, 11).padding(.trailing, 18)
        .frame(minHeight: SettingsMetrics.rowHeight)
        .contentShape(Rectangle())
    }
}

/// A Settings row that does something: `RowButton`'s press around `SettingsRowLabel`.
struct SettingsRow: View {
    let title: String
    let systemImage: String
    var trailing: SettingsTrailing
    var destructive = false
    var action: (() -> Void)?

    init(_ title: String, systemImage: String, trailing: SettingsTrailing = .chevron, comingSoon: Bool = false,
         destructive: Bool = false, action: (() -> Void)? = nil) {
        self.title = title; self.systemImage = systemImage
        self.trailing = comingSoon ? .comingSoon : trailing
        self.destructive = destructive
        self.action = comingSoon ? nil : action
    }

    var body: some View {
        if let action {
            RowButton(action: action) { SettingsRowLabel(title, systemImage: systemImage, trailing: trailing, destructive: destructive) }
        } else {
            SettingsRowLabel(title, systemImage: systemImage, trailing: trailing, destructive: destructive)
                .accessibilityElement(children: .combine)
        }
    }
}

/// A text field as a line of a card, for forms drawn with the Settings cards
/// (sign-in, a new workspace, an invitation).
struct CardField<Field: View>: View {
    var divider = true
    @ViewBuilder var field: Field

    var body: some View {
        field
            .uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary)
            .padding(.horizontal, 18)
            .frame(minHeight: SettingsMetrics.rowHeight)
            .overlay(alignment: .bottom) {
                if divider { Rectangle().fill(Theme.divider.opacity(0.7)).frame(height: 0.5).padding(.leading, 18) }
            }
    }
}

/// The small print under a card.
struct CardNote: View {
    let text: String
    var color = Theme.textSecondary
    init(_ text: String, color: Color = Theme.textSecondary) { self.text = text; self.color = color }
    var body: some View {
        Text(text).uiFont(13).foregroundStyle(color)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 14).padding(.top, 8)
    }
}
