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
    /// Cards sit 20 pt from the screen edge on a Duo, 16 on an iPhone.
    static func margin(_ rail: RailMetrics) -> CGFloat { rail.hasRail ? 20 : 16 }
}

struct SettingsView: View {
    @Environment(AccountsModel.self) private var accounts
    @Environment(Router.self) private var router
    @Environment(\.openURL) private var openURL
    @AppStorage(ThemePreference.storageKey) private var theme: ThemePreference = .auto

    private var session: AccountSession? { accounts.defaultSession }

    var body: some View {
        RailScaffold {
            SettingsScroll(title: "Settings") {
                // The Mac app is offered by the account's own server; without one, by the project's page.
                SettingsBanner(title: "Get Cereal for Mac", systemImage: "laptopcomputer") {
                    openURL(session?.notes.instance?.url.appendingPathComponent("download") ?? SettingsLinks.help)
                }
                SettingsSection("Account") {
                    SettingsRow("Profile", systemImage: "person") { router.path.append(.settings(.profile)) }
                    SettingsRow("Notifications", systemImage: "bell") { router.path.append(.settings(.notifications)) }
                    SettingsRow("Server and accounts", systemImage: "server.rack") { router.showConnection = true }
                }
                SettingsSection("Appearance") {
                    ThemeRow(theme: $theme)
                    HapticsRow()
                }
                SettingsSection("Support") {
                    SettingsRow("Send feedback", systemImage: "envelope") { openURL(SettingsLinks.feedback) }
                    SettingsRow("Report a bug", systemImage: "exclamationmark.triangle") { openURL(SettingsLinks.bug) }
                    SettingsRow("Help center", systemImage: "book.closed", trailing: .external) { openURL(SettingsLinks.help) }
                    ShareLink(item: SettingsLinks.help) {
                        SettingsRowLabel("Share Cereal for iPhone", systemImage: "square.and.arrow.up", trailing: .chevron)
                    }
                    .buttonStyle(RowPressStyle())
                }
            }
        } leading: {
            BackButton()
        } rail: {
            EmptyView()
        }
    }
}

enum SettingsLinks {
    static let help = URL(string: "https://github.com/raghavpillai/cereal#readme")!
    static let bug = URL(string: "https://github.com/raghavpillai/cereal/issues/new")!
    static let feedback = URL(string: "https://github.com/raghavpillai/cereal/issues/new?labels=feedback")!
}

/// The Theme row: the current choice with its symbol and an up-down chevron,
/// opening a menu of Light, Dark and Auto.
private struct ThemeRow: View {
    @Binding var theme: ThemePreference

    var body: some View {
        Menu {
            Picker("Theme", selection: $theme) {
                ForEach(ThemePreference.allCases) { choice in
                    Label(choice.title, systemImage: choice.symbol).tag(choice)
                }
            }
        } label: {
            SettingsRowLabel("Theme", systemImage: "circle.lefthalf.filled", trailing: .value(theme.title, symbol: theme.symbol))
        }
        .accessibilityIdentifier("settings-theme")
        .accessibilityValue(theme.title)
        .haptic(.selection, trigger: theme)
    }
}

/// Haptics: on unless turned off. Off silences the app's own haptics, the
/// recording's signatures included; the system's own (keyboard, menus) follow
/// iOS's System Haptics setting as they always do.
private struct HapticsRow: View {
    @AppStorage(HapticGate.storageKey) private var enabled = true

    var body: some View {
        Toggle(isOn: $enabled) {
            HStack(spacing: 14) {
                Image(systemName: "hand.tap")
                    .font(.system(size: 16))
                    .foregroundStyle(Theme.textPrimary.opacity(0.85))
                    .frame(width: SettingsMetrics.iconTile, height: SettingsMetrics.iconTile)
                    .background(Theme.iconTile, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                Text("Haptics").font(.system(size: SettingsMetrics.label)).foregroundStyle(Theme.textPrimary)
            }
        }
        .tint(Theme.accent)
        .padding(.leading, 11).padding(.trailing, 18)
        .frame(minHeight: SettingsMetrics.rowHeight)
        // Turned on, it says so in the hand.
        .haptic(.selection, trigger: enabled) { _, on in on }
        .accessibilityIdentifier("settings-haptics")
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

    private var record: AccountRecord? { accounts.defaultSession?.record }
    static let userTypes = ["Startup/Generalist", "Engineering", "Product", "Design", "Sales", "Investor", "Recruiting", "Other"]

    var body: some View {
        RailScaffold {
            SettingsScroll(title: nil) {
                    Text("Your profile")
                        .font(.display(30)).foregroundStyle(Theme.textPrimary)
                        .accessibilityAddTraits(.isHeader)
                        .padding(.top, 24)
                    Text("Cereal works best when it knows a little about you. These details stay on this iPhone.")
                        .font(.system(size: 14)).foregroundStyle(Theme.textPrimary.opacity(0.85))
                        .padding(.top, 10)
                    VStack(spacing: 10) {
                        AvatarView(name: record?.name ?? "Me", size: 96)
                        Text(record?.email ?? "Not signed in")
                            .font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 20).padding(.bottom, 16)

                    SettingsCard {
                        ProfileField("Name") { Text(record?.name ?? "").foregroundStyle(Theme.textPrimary) }
                        ProfileField("Job Title") { field($jobTitle, prompt: "Your job title") }
                        ProfileField("Company Name") { field($company, prompt: "Your company") }
                        ProfileField("User Type", divider: false) {
                            Menu {
                                Picker("User Type", selection: $userType) {
                                    ForEach(Self.userTypes, id: \.self) { Text($0).tag($0) }
                                }
                                .haptic(.selection, trigger: userType)
                            } label: {
                                HStack(spacing: 4) {
                                    Text(userType)
                                    Image(systemName: "chevron.up.chevron.down").font(.system(size: 11, weight: .medium))
                                }.foregroundStyle(Theme.textPrimary)
                            }
                        }
                    }
                    SettingsCard {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Company Description").font(.system(size: 14)).foregroundStyle(Theme.textPrimary)
                            TextField("", text: $companyDescription, axis: .vertical)
                                .font(.system(size: 14)).foregroundStyle(Theme.textPrimary)
                                .lineLimit(4, reservesSpace: true)
                        }
                        .padding(.horizontal, 14).padding(.vertical, 14)
                    }
                    .padding(.top, 12)
                    SettingsCard {
                        ProfileField("LinkedIn Username") { field($linkedIn, prompt: "username") }
                        Text("linkedin.com/in/\(linkedIn)")
                            .font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                            .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                            .padding(.horizontal, 14)
                    }
                    .padding(.top, 12)
            }
            .scrollDismissesKeyboard(.interactively)
        } leading: {
            BackButton()
        } rail: {
            EmptyView()
        }
    }

    private func field(_ text: Binding<String>, prompt: String) -> some View {
        TextField("", text: text, prompt: Text(prompt).foregroundStyle(Theme.textTertiary))
            .multilineTextAlignment(.trailing)
            .foregroundStyle(Theme.textPrimary)
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
        .font(.system(size: 14))
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
            content.body = "Starting now. Open Cereal to take notes."
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

    var body: some View {
        RailScaffold {
            SettingsScroll(title: "Notifications") {
                Toggle(isOn: Binding(get: { upcoming }, set: { set($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Upcoming meetings").font(.system(size: 17)).foregroundStyle(Theme.textPrimary)
                        Text("Notify me for upcoming meetings on the lock screen")
                            .font(.system(size: 15)).foregroundStyle(Theme.textSecondary)
                    }
                }
                .tint(Theme.accent)
                .accessibilityIdentifier("notifications-upcoming")
                // Asked for and refused: the line under it says how to turn them on.
                .haptic(.warning, trigger: denied) { _, now in now }
                .padding(.horizontal, 6).padding(.top, 4)
                if denied {
                    Button("Notifications are off for Cereal. Turn them on in Settings.") {
                        if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                    }
                    .font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                    .padding(.horizontal, 6)
                }
                Toggle(isOn: $meetingCards) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Take a note button").font(.system(size: 17)).foregroundStyle(Theme.textPrimary)
                        Text("Show a Live Activity with a Take a note button as each meeting starts")
                            .font(.system(size: 15)).foregroundStyle(Theme.textSecondary)
                    }
                }
                .tint(Theme.accent)
                .accessibilityIdentifier("notifications-meeting-cards")
                .onChange(of: meetingCards) { _, on in if !on { Task { await MeetingLiveActivities.endAll() } } }
                .padding(.horizontal, 6).padding(.top, 14)
                UpcomingMeetingPreview()
                    .padding(.top, 6)
            }
        } leading: {
            BackButton()
        } rail: {
            EmptyView()
        }
    }

    private func set(_ on: Bool) {
        guard on else { upcoming = false; UpcomingMeetingNotifications.clear(); return }
        Task {
            let granted = await UpcomingMeetingNotifications.requestAuthorization()
            denied = !granted
            upcoming = granted
        }
    }
}

/// What a reminder looks like: a white card with a meeting and a button to start Cereal.
private struct UpcomingMeetingPreview: View {
    var body: some View {
        let start = Calendar.current.date(bySetting: .second, value: 0, of: .now) ?? .now
        let end = start.addingTimeInterval(3600)
        VStack(alignment: .leading, spacing: 18) {
            HStack(spacing: 10) {
                RoundedRectangle(cornerRadius: 1.5).fill(Color(hex: 0xC62AE0)).frame(width: 3)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Catch up with the team").font(.system(size: 15, weight: .semibold)).foregroundStyle(Color(hex: 0x1C1C17))
                    Label("\(start.formatted(date: .omitted, time: .shortened)) – \(end.formatted(date: .omitted, time: .shortened))", systemImage: "clock")
                        .font(.system(size: 13, weight: .medium)).foregroundStyle(Color(hex: 0x1C1C17))
                }
            }
            .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                Image(systemName: "waveform").font(.system(size: 15, weight: .semibold))
                Text("Start Cereal").font(.system(size: 16, weight: .semibold))
            }
            .foregroundStyle(Color(hex: 0x1C1C17))
            .frame(maxWidth: .infinity).frame(height: 40)
            .background(Color(hex: 0xE3E3E1), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .padding(.horizontal, 11).padding(.top, 12).padding(.bottom, 9)
        .background(Color(hex: 0xFCFCFA), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .shadow(color: .black.opacity(0.08), radius: 14, y: 4)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Example reminder")
    }
}

// MARK: - Building blocks

/// The scrolling column of a Settings page. Beside a Duo rail the title sits
/// centred at the top of the page and scrolls with it; on an iPhone it is the
/// navigation bar's title.
private struct SettingsScroll<Content: View>: View {
    let title: String?
    @ViewBuilder var content: Content
    @Environment(\.railMetrics) private var rail

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if rail.hasRail, let title {
                    Text(title).font(.system(size: 17, weight: .semibold)).foregroundStyle(Theme.textPrimary)
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
        // The navigation bar's title on an iPhone. Beside a rail the page draws its own.
        .navigationTitle(rail.hasRail ? "" : (title ?? ""))
    }
}

/// A section: a 15 pt grey header, 9 pt above its card.
private struct SettingsSection<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content
    init(_ title: String, @ViewBuilder content: () -> Content) { self.title = title; self.content = content() }

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(title).font(.system(size: SettingsMetrics.header)).foregroundStyle(Theme.textSecondary)
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
                Text(title).font(.system(size: SettingsMetrics.label))
                Spacer(minLength: 8)
                Image(systemName: "chevron.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
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
    case chevron, external, comingSoon
    case value(String, symbol: String)
}

/// A row's look: an icon in a rounded square tile, the label, and what trails it.
struct SettingsRowLabel: View {
    let title: String
    let systemImage: String
    var trailing: SettingsTrailing = .chevron

    init(_ title: String, systemImage: String, trailing: SettingsTrailing = .chevron) {
        self.title = title; self.systemImage = systemImage; self.trailing = trailing
    }

    private var muted: Bool { if case .comingSoon = trailing { true } else { false } }

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: systemImage)
                .font(.system(size: 17))
                .foregroundStyle(Theme.textPrimary.opacity(0.85))
                .frame(width: SettingsMetrics.iconTile, height: SettingsMetrics.iconTile)
                .background(Theme.iconTile, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            Text(title).font(.system(size: SettingsMetrics.label))
                .foregroundStyle(muted ? Theme.textSecondary : Theme.textPrimary)
            Spacer(minLength: 8)
            switch trailing {
            case .chevron:
                Image(systemName: "chevron.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
            case .external:
                Image(systemName: "arrow.up.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
            case .comingSoon:
                Text("Coming soon").font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                    .padding(.horizontal, 8).frame(height: 24)
                    .background(Theme.iconTile, in: Capsule())
            case .value(let text, let symbol):
                HStack(spacing: 6) {
                    Image(systemName: symbol).font(.system(size: 15))
                    Text(text).font(.system(size: SettingsMetrics.label))
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

private struct SettingsRow: View {
    let title: String
    let systemImage: String
    var trailing: SettingsTrailing
    var action: (() -> Void)?

    init(_ title: String, systemImage: String, trailing: SettingsTrailing = .chevron, comingSoon: Bool = false, action: (() -> Void)? = nil) {
        self.title = title; self.systemImage = systemImage
        self.trailing = comingSoon ? .comingSoon : trailing
        self.action = comingSoon ? nil : action
    }

    var body: some View {
        if let action {
            Button(action: action) { SettingsRowLabel(title, systemImage: systemImage, trailing: trailing) }
                .buttonStyle(RowPressStyle())
        } else {
            SettingsRowLabel(title, systemImage: systemImage, trailing: trailing)
                .accessibilityElement(children: .combine)
        }
    }
}
