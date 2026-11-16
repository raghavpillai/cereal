import SwiftUI

extension ToolbarContent {
    /// Custom glass controls still participate in Duo's native vertical toolbar.
    @ToolbarContentBuilder func cerealAdaptiveAxis() -> some ToolbarContent {
        // The vertical toolbar APIs are in the iOS 27.1 SDK (Xcode 27.1, Swift 6.4). Older Xcodes build without them.
        #if compiler(>=6.4)
        if #available(iOS 27.1, *) {
            self.axisBehavior(.verticalPreferred)
        } else {
            self
        }
        #else
        self
        #endif
    }
}

private struct AdaptiveToolbarPlacement: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        #if compiler(>=6.4)
        if #available(iOS 27.1, *) {
            content.toolbarVerticalBehavior(.automatic)
        } else {
            content
        }
        #else
        content
        #endif
    }
}

extension View {
    /// Lets the system move toolbar controls into Duo's trailing vertical rail
    /// in both postures. An ordinary iPhone keeps its top bar.
    func cerealAdaptiveToolbarPlacement() -> some View {
        modifier(AdaptiveToolbarPlacement())
    }

    /// Close and action controls of the recording cover.
    func captureChrome<Actions: View>(close: @escaping () -> Void, @ViewBuilder actions: () -> Actions) -> some View {
        modifier(CaptureChrome(close: close, actions: actions()))
    }
}

/// Measures the window for a screen that is not inside a `RailScaffold`
/// (the recording cover) and publishes the same `RailMetrics`.
struct RailMetricsReader<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        GeometryReader { proxy in
            content.environment(\.railMetrics, RailMetrics(contentWidth: proxy.size.width,
                                                           leadingInset: proxy.safeAreaInsets.leading,
                                                           trailingInset: proxy.safeAreaInsets.trailing,
                                                           bottomInset: proxy.safeAreaInsets.bottom))
        }
    }
}

/// On an iPhone the recording cover's controls are native toolbar items.
/// Beside a Duo rail the reference keeps only the OS status items in the rail
/// and draws the controls in the cover's own top bar: a 44 pt close circle at
/// the leading edge and a 44 pt capsule of actions at the trailing edge.
private struct CaptureChrome<Actions: View>: ViewModifier {
    let close: () -> Void
    let actions: Actions
    @Environment(\.railMetrics) private var rail

    func body(content: Content) -> some View {
        content
            .safeAreaInset(edge: .top, spacing: 0) {
                if rail.hasRail {
                    HStack {
                        GlassCircleButton(systemImage: "chevron.down", size: 44, action: close)
                        Spacer()
                        HStack(spacing: 0) { actions }
                            .padding(.horizontal, 10)
                            .frame(height: 44)
                            .glassSurface(Capsule(), interactive: true)
                    }
                    .padding(.leading, 24).padding(.trailing, 20)
                    .padding(.top, 20)
                }
            }
            .toolbar {
                if !rail.hasRail {
                    ToolbarItemGroup(placement: .topBarLeading) {
                        GlassCircleButton(systemImage: "chevron.down", action: close)
                            .environment(\.cerealNativeNavigationChrome, true)
                    }
                    ToolbarItemGroup(placement: .topBarTrailing) { actions }
                }
            }
            .toolbarVisibility(rail.hasRail ? .hidden : .visible, for: .navigationBar)
            .navigationBarTitleDisplayMode(.inline)
            .cerealAdaptiveToolbarPlacement()
    }
}

private struct NativeNavigationChromeKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    var cerealNativeNavigationChrome: Bool {
        get { self[NativeNavigationChromeKey.self] }
        set { self[NativeNavigationChromeKey.self] = newValue }
    }
}

/// The system owns toolbar glass. Custom floating controls share this material
/// and accessibility policy, avoiding a second lens inside a native bar.
private struct AdaptiveGlass<S: Shape>: ViewModifier {
    let shape: S
    let interactive: Bool
    let tint: Color?
    var respectsNativeChrome = true
    @Environment(\.cerealNativeNavigationChrome) private var nativeChrome
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.glassMorphTarget) private var morph
    @Namespace private var unmatched

    @ViewBuilder func body(content: Content) -> some View {
        if nativeChrome && respectsNativeChrome {
            content
        } else if reduceTransparency {
            content
                .background(tint ?? .clear, in: shape)
                .background(Theme.surfaceRaised, in: shape)
                .overlay(shape.stroke(.white.opacity(contrast == .increased ? 0.32 : 0.14), lineWidth: 1))
        } else if #available(iOS 26.0, *) {
            // A named surface morphs with its neighbours in a GlassChromeGroup.
            // Reduce Motion drops the name, leaving a plain opacity change.
            content.glassEffect(
                interactive && !reduceMotion
                    ? .regular.tint(tint ?? Theme.glassTint).interactive()
                    : .regular.tint(tint ?? Theme.glassTint),
                in: shape
            )
            .glassEffectID(reduceMotion ? nil : morph?.id, in: morph?.namespace ?? unmatched)
            .glassEffectTransition(reduceMotion ? .identity : .matchedGeometry)
        } else {
            content.background(.ultraThinMaterial, in: shape)
                .overlay(shape.stroke(Theme.stroke, lineWidth: 0.5))
        }
    }
}

// MARK: - Glass

extension View {
    /// A control lens. Native toolbar placement supplies its own material.
    func glass<S: Shape>(_ shape: S, interactive: Bool = true, tint: Color? = nil) -> some View {
        modifier(AdaptiveGlass(shape: shape, interactive: interactive, tint: tint))
    }

    /// Floating chrome outside native bars, such as a composer or recording
    /// pill. The caller keeps inner actions on one shared lens.
    func glassSurface<S: Shape>(_ shape: S, interactive: Bool = false, tint: Color? = nil) -> some View {
        modifier(AdaptiveGlass(shape: shape, interactive: interactive, tint: tint, respectsNativeChrome: false))
    }

    /// Leave the native sheet presentation in charge of its material. Only the
    /// accessibility opaque preference overrides the system's background.
    ///
    /// `background` is for a sheet the system material tints: the transcript
    /// over a recording in progress rendered green-black where the
    /// same sheet over a finished note, and every other sheet, is neutral.
    func glassSheet(background: Color? = nil) -> some View { modifier(GlassSheetPresentation(background: background)) }
}

private struct GlassSheetPresentation: ViewModifier {
    var background: Color?
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @ViewBuilder func body(content: Content) -> some View {
        if reduceTransparency { content.presentationBackground(Theme.surfaceRaised) }
        else if let background { content.presentationBackground(background) }
        else { content }
    }
}

/// Adjacent custom controls render together. The builder retains the caller's
/// layout; this container never invents a rail on an ordinary iPhone.
struct GlassChromeGroup<Content: View>: View {
    var spacing: CGFloat = 8
    @ViewBuilder var content: Content
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @ViewBuilder var body: some View {
        if #available(iOS 26.0, *), !reduceTransparency {
            GlassEffectContainer(spacing: spacing) { content }
        } else { content }
    }
}

/// Sizes of controls hosted by the system toolbar (the top bar, or a Duo's
/// trailing rail), measured from the reference recordings at 3 px per point.
/// The system adds 8 pt around a toolbar item and 14 pt between grouped items,
/// so these produce a 48 x 46.5 pt circle and a 52.5 pt pitch inside a capsule.
enum RailChrome {
    static let itemWidth: CGFloat = 40
    static let itemHeight: CGFloat = 38.5
    static let glyphOpacity = 0.58
    static let avatarSize: CGFloat = 30

    /// Every rail and top-bar glyph shares one size, weight and dimness.
    static let glyphSize: CGFloat = 15
    static let glyphWeight: Font.Weight = .medium
}

/// A rail or top-bar glyph at the reference size and weight.
struct RailGlyph: View {
    let systemImage: String
    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: RailChrome.glyphSize, weight: RailChrome.glyphWeight))
            .foregroundStyle(Theme.textPrimary.opacity(RailChrome.glyphOpacity))
            .frame(width: RailChrome.itemWidth, height: RailChrome.itemHeight)
            .contentShape(Rectangle())
    }
}

/// A single circular glass control. In a toolbar the system supplies the
/// glass and the control takes the rail's size; elsewhere it is `size` points.
struct GlassCircleButton: View {
    let systemImage: String
    var size: CGFloat = 48
    var glyphSize: CGFloat = 15
    var action: () -> Void
    @Environment(\.cerealNativeNavigationChrome) private var nativeChrome

    var body: some View {
        Button(action: action) {
            if nativeChrome {
                RailGlyph(systemImage: systemImage)
            } else {
                Image(systemName: systemImage)
                    .font(.system(size: glyphSize, weight: .medium))
                    .foregroundStyle(Theme.textPrimary.opacity(0.85))
                    .frame(width: size, height: size)
                    .contentShape(Circle())
            }
        }
        .buttonStyle(.plain)
        .glass(Circle())
        .accessibilityLabel(railLabel(systemImage))
    }
}

/// A vertical glass capsule that groups several rail buttons.
struct RailGroup<Content: View>: View {
    @ViewBuilder var content: Content
    @Environment(\.cerealNativeNavigationChrome) private var nativeChrome
    @ViewBuilder var body: some View {
        if nativeChrome {
            Group { content }
        } else {
            VStack(spacing: 4) { content }
                .padding(.vertical, 6)
                .frame(width: 52)
                .glass(Capsule())
        }
    }
}

struct RailIcon: View {
    let systemImage: String
    var action: () -> Void
    var body: some View {
        Button(action: action) { RailGlyph(systemImage: systemImage) }
            .buttonStyle(.plain)
            .accessibilityLabel(railLabel(systemImage))
    }
}

/// The account avatar as the last item of a rail capsule: 30 pt, inset evenly.
struct RailAvatar: View {
    let name: String
    var action: () -> Void
    var body: some View {
        Button(action: action) {
            AvatarView(name: name, size: RailChrome.avatarSize)
                .frame(width: RailChrome.itemWidth, height: RailChrome.itemHeight)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Connection settings")
    }
}

// MARK: - Screen scaffold

/// Where the navigation controls sit, measured from the window rather than
/// from a device name. An ordinary iPhone has a top bar; both Duo postures
/// have a trailing vertical rail, which the system reports as a trailing-only
/// safe-area inset.
struct RailMetrics: Equatable, Sendable {
    static let railThreshold: CGFloat = 40
    static let wideContentWidth: CGFloat = 600
    /// The + button sits this far from the trailing screen edge when it lives in the rail column.
    static let addButtonEdgeMargin: CGFloat = 24
    static let addButtonSize: CGFloat = 64

    let hasRail: Bool
    /// Width of the rail column beyond the content area. Zero without a rail.
    let railInset: CGFloat
    /// Leading and trailing margin of lists, documents and floating bars.
    let gutter: CGFloat

    /// An unfolded Duo: the text column is inset further than the floating bars.
    let isWide: Bool
    /// Bottom safe-area inset of the window, used to place floating bars
    /// a fixed distance from the screen edge on a rail layout.
    let bottomInset: CGFloat

    init(contentWidth: CGFloat = 0, leadingInset: CGFloat = 0, trailingInset: CGFloat = 0, bottomInset: CGFloat = 0) {
        // A landscape iPhone has matching side insets; only a rail is one-sided.
        hasRail = trailingInset - leadingInset > Self.railThreshold
        railInset = hasRail ? trailingInset - leadingInset : 0
        isWide = hasRail && contentWidth >= Self.wideContentWidth
        gutter = !hasRail ? Theme.gutter : isWide ? 82 : 24
        self.bottomInset = bottomInset
    }

    /// Screens already pad their content by `Theme.gutter`; this is the remainder.
    var extraLeading: CGFloat { gutter - Theme.gutter }
    /// Screens pad the trailing edge by 8; this is the balance, so text has the
    /// same margin on both sides on every layout (16 on an iPhone).
    var extraTrailing: CGFloat { gutter - 8 }
    /// Beside a rail the Ask pill and + end 28 pt above the screen's bottom
    /// edge, which is inside the home-indicator inset. Otherwise 8 pt above it.
    /// While the keyboard is up the inset is the keyboard, and the bar rides
    /// 12 pt above it instead, like the chat composer.
    var bottomPadding: CGFloat {
        guard hasRail else { return 8 }
        return bottomInset > Self.keyboardInsetThreshold ? 12 : Self.barEdgeMargin - bottomInset
    }
    static let barEdgeMargin: CGFloat = 28
    static let keyboardInsetThreshold: CGFloat = 80
    /// The chat composer is wider than the text column: 17 pt from the content
    /// edges beside a rail (7 pt outside the gutter), 12 pt on an iPhone.
    var composerMargin: CGFloat { hasRail ? gutter - 7 : 12 }
    var composerLeading: CGFloat { composerMargin - extraLeading }
    var composerTrailing: CGFloat { composerMargin - extraTrailing }
    static let composerBottom: CGFloat = 12
    /// Recording cover: text column and floating controls (28 / 24 pt beside a rail).
    var captureGutter: CGFloat { hasRail ? 28 : Theme.gutter }
    /// Vertical insets of the chat title and composer field. A landscape
    /// iPhone has about 135 pt above the keyboard, so they tighten there.
    static func chatTitleInsets(compact: Bool) -> (top: CGFloat, bottom: CGFloat) { compact ? (4, 4) : (20, 12) }
    static func composerFieldInsets(compact: Bool) -> (top: CGFloat, bottom: CGFloat) { compact ? (9, 6) : (18, 20) }
    /// The pill hangs 4 pt outside the text column on every layout, level with the meta chips.
    var captureBarLeading: CGFloat { captureGutter - 4 }
    /// Leading and trailing inset of the Ask pill and search field. On a wide
    /// layout they are 7 pt wider than the text column, like the chat composer.
    var barInset: CGFloat { isWide ? gutter - 7 : gutter }
    /// Gap from the Ask pill to a + button pinned in the rail column.
    var addButtonGap: CGFloat {
        max(14, railInset + barInset - Self.addButtonEdgeMargin - Self.addButtonSize)
    }
}

extension EnvironmentValues {
    @Entry var railMetrics = RailMetrics()
    /// Height that the floating bottom bar covers above the safe area, as laid out.
    @Entry var bottomChromeHeight: CGFloat = 0
}

private struct BottomChromeClearance: ViewModifier {
    var gap: CGFloat
    @Environment(\.bottomChromeHeight) private var chrome
    func body(content: Content) -> some View { content.padding(.bottom, chrome + gap) }
}

extension View {
    /// Pads scrolling content so its last line clears the floating bottom bar
    /// by `gap`, whatever that bar currently contains.
    func clearsBottomChrome(gap: CGFloat = 24) -> some View { modifier(BottomChromeClearance(gap: gap)) }
}

/// Native toolbars adapt to ordinary iPhone navigation and Duo's vertical rail.
/// `standalone` is the single circular control at the head of the trailing
/// controls (Folders on Home); `rail` is the grouped capsule after it; and
/// `leading` is the back control. The document stays in the safe area;
/// floating recording/chat controls remain independent of navigation so a fold
/// never replaces their state owners.
struct RailScaffold<Content: View, Rail: View, Bottom: View>: View {
    var content: Content
    var rail: Rail
    var bottom: Bottom
    var leading: AnyView
    var standalone: AnyView?

    init(@ViewBuilder content: () -> Content, @ViewBuilder rail: () -> Rail, @ViewBuilder bottom: () -> Bottom) {
        self.content = content(); self.rail = rail(); self.bottom = bottom()
        self.leading = AnyView(EmptyView())
    }

    init<Leading: View>(@ViewBuilder content: () -> Content, @ViewBuilder leading: () -> Leading,
                       @ViewBuilder rail: () -> Rail, @ViewBuilder bottom: () -> Bottom) {
        self.content = content(); self.rail = rail(); self.bottom = bottom()
        self.leading = AnyView(leading())
    }

    init<Standalone: View>(@ViewBuilder content: () -> Content, @ViewBuilder standalone: () -> Standalone,
                          @ViewBuilder rail: () -> Rail, @ViewBuilder bottom: () -> Bottom) {
        self.content = content(); self.rail = rail(); self.bottom = bottom()
        self.leading = AnyView(EmptyView())
        self.standalone = AnyView(standalone())
    }

    init<Leading: View, Standalone: View>(@ViewBuilder content: () -> Content, @ViewBuilder leading: () -> Leading,
                                         @ViewBuilder standalone: () -> Standalone,
                                         @ViewBuilder rail: () -> Rail, @ViewBuilder bottom: () -> Bottom) {
        self.content = content(); self.rail = rail(); self.bottom = bottom()
        self.leading = AnyView(leading())
        self.standalone = AnyView(standalone())
    }

    @State private var bottomHeight: CGFloat = 0

    var body: some View {
        GeometryReader { proxy in
            let metrics = RailMetrics(contentWidth: proxy.size.width,
                                      leadingInset: proxy.safeAreaInsets.leading,
                                      trailingInset: proxy.safeAreaInsets.trailing,
                                      bottomInset: proxy.safeAreaInsets.bottom)
            ZStack(alignment: .bottom) {
                Theme.background
                content
                    .safeAreaPadding(.leading, metrics.extraLeading)
                    .safeAreaPadding(.trailing, metrics.extraTrailing)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                // Text that scrolls up behind the status bar is gone before it reaches the clock.
                TopFade(height: proxy.safeAreaInsets.top)
                    .frame(maxHeight: .infinity, alignment: .top)
                    .ignoresSafeArea()
                if Bottom.self != EmptyView.self {
                    // Edge to edge, so the fade has no visible sides on the flat background.
                    BottomFade(height: 64)
                        .frame(maxHeight: .infinity, alignment: .bottom)
                        .ignoresSafeArea()
                }
                bottom
                    .padding(.horizontal, metrics.barInset)
                    .padding(.bottom, metrics.bottomPadding)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { bottomHeight = $0 }
            }
            // Content taller than the space left by the keyboard overflows upward,
            // so the bottom bar always stays above the keyboard.
            .frame(width: proxy.size.width, height: proxy.size.height, alignment: .bottom)
            .environment(\.railMetrics, metrics)
            .environment(\.bottomChromeHeight, max(0, bottomHeight))
        }
        .background(SwipeBackEnabler())
        .overlay(alignment: .leading) { EdgeFallbackStrip() }
        .toolbar {
            ToolbarItemGroup(placement: .topBarLeading) {
                leading.environment(\.cerealNativeNavigationChrome, true)
            }
            .cerealAdaptiveAxis()
            if let standalone {
                // Its own glass shape, apart from the grouped capsule.
                ToolbarItemGroup(placement: .topBarTrailing) {
                    standalone.environment(\.cerealNativeNavigationChrome, true)
                }
                .cerealAdaptiveAxis()
                ToolbarSpacer(.fixed, placement: .topBarTrailing)
            }
            if Rail.self != EmptyView.self {
                ToolbarItemGroup(placement: .topBarTrailing) {
                    rail.environment(\.cerealNativeNavigationChrome, true)
                }
                .cerealAdaptiveAxis()
            }
        }
        .toolbarVisibility(.visible, for: .navigationBar)
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden()
    }
}

extension RailScaffold where Bottom == EmptyView {
    init(@ViewBuilder content: () -> Content, @ViewBuilder rail: () -> Rail) {
        self.content = content()
        self.rail = rail()
        self.bottom = EmptyView()
        self.leading = AnyView(EmptyView())
    }

    init<Leading: View>(@ViewBuilder content: () -> Content, @ViewBuilder leading: () -> Leading,
                       @ViewBuilder rail: () -> Rail) {
        self.content = content(); self.rail = rail(); self.bottom = EmptyView()
        self.leading = AnyView(leading())
    }
}

extension RailScaffold where Rail == EmptyView {
    /// A page whose only navigation control is Back.
    init<Leading: View>(@ViewBuilder content: () -> Content, @ViewBuilder leading: () -> Leading,
                       @ViewBuilder bottom: () -> Bottom) {
        self.content = content(); self.rail = EmptyView(); self.bottom = bottom()
        self.leading = AnyView(leading())
    }
}

// MARK: - Bottom bar

struct AskAnythingPill: View {
    var prominent = false
    var action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: 5) {
                Image(systemName: "bubble.left.fill")
                    .font(.system(size: 20))
                    .foregroundStyle(Theme.textPrimary)
                Text("Ask anything")
                    .font(.system(size: 18))
                    .foregroundStyle(Theme.textPrimary)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 64)
            .contentShape(Capsule())
        }
        .buttonStyle(.squash)
        .glassSurface(Capsule(), interactive: true, tint: .white.opacity(prominent ? 0.20 : 0.08))
    }
}

struct AccentFAB: View {
    /// A request is in flight. The glyph turns while the control stays put.
    var busy = false
    /// False while another control (the recording indicator) is what the
    /// recording screen grows out of and returns to.
    var zoomsToCapture = true
    var action: () -> Void
    @Environment(\.motion) private var motion
    var body: some View {
        Button(action: action) {
            Image(systemName: "plus")
                .font(.system(size: 20, weight: .medium))
                .foregroundStyle(Theme.onAccent)
                .symbolEffect(.rotate, options: .repeat(.continuous), isActive: busy && !motion.reduceMotion)
                .frame(width: 64, height: 64)
                .contentShape(Circle())
                .zoomSource(ZoomID.capture, cornerRadius: 32, enabled: zoomsToCapture)
        }
        .buttonStyle(.squash)
        .glassSurface(Circle(), interactive: true, tint: Theme.accent)
        .accessibilityLabel("New note")
    }
}

/// Home's floating bar. With a top bar the + button sits beside the Ask pill.
/// With a trailing rail the pill fills the content area and the + button is
/// pinned in the rail column, at the bottom trailing corner of the screen.
///
/// `accessory` is a control that floats above the pill's leading edge while it
/// is needed (the recording indicator). The bar grows upward for it: the pill
/// and the + button keep their place.
struct HomeBottomBar<Accessory: View, Ask: View, Add: View>: View {
    var accessory: Accessory
    var ask: Ask
    var add: Add
    @Environment(\.railMetrics) private var rail

    init(@ViewBuilder accessory: () -> Accessory, @ViewBuilder ask: () -> Ask, @ViewBuilder add: () -> Add) {
        self.accessory = accessory(); self.ask = ask(); self.add = add()
    }

    var body: some View {
        VStack(spacing: 0) {
            GlassChromeGroup(spacing: 14) {
                if Accessory.self == EmptyView.self {
                    row
                } else {
                    VStack(alignment: .leading, spacing: Self.accessoryGap) { accessory; row }
                }
            }
        }
    }

    /// The bar's own gap: the same 14 pt that separates the pill from the + button.
    static var accessoryGap: CGFloat { 14 }

    @ViewBuilder private var row: some View {
        if rail.hasRail {
            let gap = rail.addButtonGap
            ask.overlay(alignment: .trailing) {
                add.alignmentGuide(.trailing) { $0[.leading] - gap }
            }
        } else {
            HStack(spacing: 14) { ask; add }
        }
    }
}

extension HomeBottomBar where Accessory == EmptyView {
    init(@ViewBuilder ask: () -> Ask, @ViewBuilder add: () -> Add) {
        self.init(accessory: { EmptyView() }, ask: ask, add: add)
    }
}

// MARK: - Home list pieces

/// Sizes of the Home list, measured from the reference recordings.
enum HomeMetrics {
    static let titleSize: CGFloat = 28
    static let titleCapHeight: CGFloat = 19.7
    /// From the top of the title's line box to the top of the caption beneath it.
    static let captionOffset: CGFloat = 35
    static let pagerGlyph: CGFloat = 13.5
    static let eventsText: CGFloat = 15.5
    static let sectionHeader: CGFloat = 15.5
    static let rowTitle: CGFloat = 15
    static let rowSubtitle: CGFloat = 14
    static let tile: CGFloat = 44
    /// The tile hangs 4 pt outside the text column; the divider 3 pt.
    static let tileOutdent: CGFloat = 4
    static let dividerOutdent: CGFloat = 3
}

/// "Today" with the day pager. `caption` (the local-demo label) hangs below
/// the title without moving the list.
struct HomeHeader: View {
    let title: String
    var caption: String? = nil
    let canGoBack: Bool
    let back: () -> Void
    let forward: () -> Void
    @Environment(\.motion) private var motion

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title)
                .font(.display(HomeMetrics.titleSize))
                .foregroundStyle(Theme.textPrimary)
                // Words fade; a letter-by-letter roll is for numbers.
                .contentTransition(.opacity)
                .accessibilityAddTraits(.isHeader)
                .padding(.leading, 1)
                .overlay(alignment: .topLeading) {
                    if let caption {
                        // Hangs clear of the title's descenders without moving the list.
                        Text(caption).font(.system(size: 11)).foregroundStyle(Theme.textTertiary)
                            .lineLimit(1).fixedSize()
                            .offset(x: 1, y: HomeMetrics.captionOffset)
                    }
                }
            Spacer()
            // 33 x 44 pt targets keep the measured 33 pt between the glyph centres.
            HStack(spacing: 0) {
                Button(action: back) { Image(systemName: "chevron.left").frame(width: 33, height: 44).contentShape(Rectangle()) }
                    .disabled(!canGoBack).opacity(canGoBack ? 1 : 0.3).accessibilityLabel("Previous day")
                Button(action: forward) { Image(systemName: "chevron.right").frame(width: 33, height: 44).contentShape(Rectangle()) }
                    .accessibilityLabel("Next day")
            }
            .font(.system(size: HomeMetrics.pagerGlyph, weight: .medium))
            .foregroundStyle(Theme.textSecondary)
            .buttonStyle(SquashButtonStyle(scale: 0.8))
            // Centred on the title's capitals rather than sitting on its baseline.
            .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + HomeMetrics.titleCapHeight / 2 }
            .padding(.trailing, -7)
            .padding(.vertical, -16)
        }
        .padding(.top, 35)
        .padding(.bottom, 23)
    }
}

/// The upcoming-events block on both Homes: a 4 pt bar, 10 pt gap, and rows
/// in the measured events text style. It keeps the empty line's height while
/// loading, so the list below does not move when events arrive.
struct HomeEventsBlock<Rows: View>: View {
    var barColor = Color(hex: 0x464640)
    @ViewBuilder var rows: Rows
    var body: some View {
        HStack(spacing: 10) {
            RoundedRectangle(cornerRadius: 2).fill(barColor).frame(width: 4)
            VStack(alignment: .leading, spacing: 3) { rows }
                .padding(.vertical, 9)
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(.leading, 1)
    }
}

/// The muted line shown when there is nothing more on the calendar.
struct HomeEventsEmpty: View {
    let text: String
    var body: some View {
        HomeEventsBlock {
            Text(text)
                .font(.system(size: HomeMetrics.eventsText, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .contentTransition(.opacity)
        }
    }
}

struct HomeDivider: View {
    @Environment(\.displayScale) private var displayScale
    var body: some View {
        // A one-pixel hairline, as in the reference.
        Rectangle().fill(Theme.divider).frame(height: 1 / displayScale)
            .padding(.horizontal, -HomeMetrics.dividerOutdent)
            .padding(.top, 47).padding(.bottom, 45)
    }
}

struct HomeSectionHeader: View {
    let title: String
    var body: some View {
        Text(title)
            .font(.system(size: HomeMetrics.sectionHeader))
            .foregroundStyle(Theme.textSecondary)
            .padding(.bottom, 4)
            .accessibilityAddTraits(.isHeader)
    }
}

// MARK: - Note rows

/// The square at the head of a note row.
enum NoteTile: Equatable {
    case document
    /// Audio for this note was captured on this phone.
    case phone
    /// A meeting with other people, shown as the other party's initial.
    case letter(String)

    static func meeting(with name: String) -> NoteTile {
        guard let first = name.trimmingCharacters(in: .whitespacesAndNewlines).first else { return .document }
        return .letter(String(first).uppercased())
    }

    /// Pastels for the letter tiles: lavender, yellow, rose, peach, sky. The
    /// third was a mint. Its gold counterpart sat 11 degrees of hue from the
    /// yellow and the tiles were hard to tell apart, so it is a rose instead:
    /// the mint's lightness and chroma (OKLCH 0.903, 0.033) at hue 352, which
    /// is 55 degrees from its nearest neighbours and nowhere near green.
    private static let palette: [UInt32] = [0xD9D2F0, 0xF3E3A1, 0xF2D6E1, 0xF2D3C4, 0xCFE0F3]

    /// Background of a letter tile. Stable across launches, unlike `hashValue`.
    static func colour(for letter: String) -> UInt32 {
        let sum = letter.unicodeScalars.reduce(0) { $0 + Int($1.value) }
        return palette[sum % palette.count]
    }
}

struct NoteTileView: View {
    let tile: NoteTile

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 10, style: .continuous)
        switch tile {
        case .document, .phone:
            Image(systemName: tile == .phone ? "iphone" : "doc.text")
                .font(.system(size: 19, weight: .light))
                .foregroundStyle(Theme.textSecondary)
                .frame(width: 44, height: 44)
                .background(Theme.surface, in: shape)
                .overlay(shape.stroke(Theme.stroke, lineWidth: 0.5))
        case .letter(let letter):
            Text(letter)
                .font(.system(size: 21, weight: .medium, design: .serif))
                .foregroundStyle(Theme.onAccent)
                .frame(width: 44, height: 44)
                .background(Color(hex: NoteTile.colour(for: letter)), in: shape)
        }
    }
}

/// One line of Home, Search and the notes library.
struct NoteListRow: View {
    let tile: NoteTile
    let title: String
    let subtitle: String

    var body: some View {
        HStack(spacing: 11) {
            NoteTileView(tile: tile)
                .accessibilityHidden(tile != .phone)
                .accessibilityLabel("Recorded on this iPhone")
            VStack(alignment: .leading, spacing: 0) {
                Text(title).readingFont(.rowTitle)
                    .foregroundStyle(Theme.textPrimary).lineLimit(1)
                Text(subtitle).readingFont(.rowSubtitle)
                    .foregroundStyle(Theme.textSecondary).lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 10)
        .padding(.leading, -HomeMetrics.tileOutdent)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        // Rows grow with the text size up to the first accessibility size; the tile stays 44 pt.
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }
}

private func railLabel(_ symbol: String) -> String {
    switch symbol {
    case "chevron.left": "Back"
    case "chevron.down": "Close note"
    case "xmark": "Close"
    case "square.and.arrow.up": "Share"
    case "square.and.pencil": "New chat"
    case "clock.arrow.circlepath": "Chat history"
    case "magnifyingglass": "Search"
    case "gearshape": "Connection settings"
    case "folder": "Folders"
    case "ellipsis": "More options"
    default: symbol
    }
}

/// Fades content out under floating bottom chrome.
struct BottomFade: View {
    var height: CGFloat = 64
    /// Measured in the reference: the last 50 pt or so of the screen lighten to about #2A2A2A.
    private static let end = Color(hex: 0x2A2A2A)
    var body: some View {
        LinearGradient(colors: [Self.end.opacity(0), Self.end], startPoint: .top, endPoint: .bottom)
            .frame(height: height)
            .allowsHitTesting(false)
    }
}

/// Covers content that has scrolled up behind the status bar and thins out
/// across the row of navigation controls beneath it. The system's own edge
/// effect only dims: on this dark page a row title stayed readable through the
/// clock. Flat `bgTop`, so the fade has no visible edge on the page.
struct TopFade: View {
    /// The top safe-area inset: the status bar and the navigation controls.
    let height: CGFloat
    /// The share of `height` that is fully covered (the status bar).
    static let solid = 0.5
    var body: some View {
        LinearGradient(stops: [
            .init(color: Theme.bgTop, location: 0),
            .init(color: Theme.bgTop, location: Self.solid),
            .init(color: Theme.bgTop.opacity(0), location: 1),
        ], startPoint: .top, endPoint: .bottom)
            .frame(height: max(0, height))
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

/// A wait with its reason beside it: the spinner leads the line it belongs to.
/// A labelled `ProgressView` centres its spinner over the words, which reads as
/// misplaced above a line of leading-aligned text.
struct InlineProgress: View {
    let text: String
    var body: some View {
        HStack(spacing: 10) {
            ProgressView()
            Text(text).foregroundStyle(Theme.textSecondary)
        }
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Meta chips

/// The date/attendees capsule and the folder circle under a note title.
/// Outlined rather than filled, 36 pt tall, hanging 4 pt outside the text
/// column like the Home tiles (measured from the reference).
struct NoteMetaChips: View {
    let date: Date
    let attendeeCount: Int
    var showCalendarIcon = true
    var onMetadata: (() -> Void)? = nil
    private static let outline = Color.white.opacity(0.10)

    var body: some View {
        HStack(spacing: 10) {
            if let onMetadata {
                Button(action: onMetadata) { metadataCapsule }
                    .buttonStyle(.squash)
                    .accessibilityLabel("Meeting date and attendees")
                    .accessibilityIdentifier("note-metadata-open")
            } else {
                metadataCapsule
            }

            Button {} label: {
                Image(systemName: "folder.badge.plus")
                    .font(.system(size: 15))
                    .foregroundStyle(Theme.textSecondary)
                    .frame(width: 36, height: 36)
                    .overlay(Circle().strokeBorder(Self.outline, lineWidth: 1))
            }
            .buttonStyle(.plain)
            .disabled(true)
            .accessibilityLabel("Add to folder")
            .accessibilityHint("Folder organization is not available yet.")
            .accessibilityIdentifier("note-folder-unavailable")
        }
        .padding(.leading, -HomeMetrics.tileOutdent)
    }

    private var metadataCapsule: some View {
        NoteMetaCapsule {
            if showCalendarIcon {
                Image(systemName: "calendar").font(.system(size: 14.5))
            }
            Text(date.noteMetaString)
            Image(systemName: "person.2").font(.system(size: 14)).padding(.leading, 9)
            Text(attendeeCount > 0 ? "+\(attendeeCount)" : "Me")
        }
    }
}

/// The outlined capsule of facts under a note title: 36 pt tall, 14 pt text.
/// Shared by a note's own page and by a note someone else shared.
struct NoteMetaCapsule<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        HStack(spacing: 6) { content }
            .font(.system(size: 14))
            .foregroundStyle(Theme.textSecondary)
            .padding(.leading, 14).padding(.trailing, 17)
            .frame(height: 36)
            .contentShape(Rectangle().inset(by: -4))
            .overlay(Capsule().strokeBorder(Color.white.opacity(0.10), lineWidth: 1))
    }
}

// MARK: - Avatar

struct AvatarView: View {
    let name: String
    var size: CGFloat = 44

    private var initials: String {
        name.split(separator: " ").prefix(2).compactMap(\.first).map(String.init).joined()
    }

    var body: some View {
        Text(initials)
            .font(.system(size: size * 0.36, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(
                LinearGradient(colors: [Color(hex: 0x4A6B8A), Color(hex: 0x2E3E52)], startPoint: .top, endPoint: .bottom),
                in: Circle()
            )
    }
}

// MARK: - Dates

extension Date {
    var noteMetaString: String {
        let cal = Calendar.current
        let time = formatted(date: .omitted, time: .shortened)
        if cal.isDateInToday(self) { return "Today \(time)" }
        if cal.isDateInYesterday(self) { return "Yesterday \(time)" }
        return formatted(.dateTime.day().month(.abbreviated)) + " \(time)"
    }
}


// MARK: - Capture editor

/// Shared by local capture and connected capture; recording/persistence stay in
/// their respective controllers rather than leaking into presentation code.
enum CaptureEditorField: Hashable { case title, body }

struct CaptureNoteEditor: View {
    @Binding var title: String
    @Binding var text: String
    var focused: FocusState<CaptureEditorField?>.Binding
    let date: Date
    var attendeeCount = 0
    var identifierPrefix = "capture"
    var onMetadata: (() -> Void)?
    @Environment(\.motion) private var motion
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    /// A landscape iPhone with the keyboard up has room for the title and one
    /// line of notes only; the meta chips return when the keyboard goes.
    private var cramped: Bool { verticalSizeClass == .compact && focused.wrappedValue != nil }

    var body: some View {
        TextField("", text: $title, prompt: Text("Untitled note").foregroundStyle(Theme.textPrimary.opacity(0.45)), axis: .vertical)
            .font(.noteTitle).foregroundStyle(Theme.textPrimary)
            .focused(focused, equals: .title)
            .submitLabel(.next).onSubmit { focused.wrappedValue = .body }
            .lineLimit(cramped ? 1 : nil)
            .padding(.top, cramped ? 0 : 16).accessibilityIdentifier("\(identifierPrefix)-title")
        if !cramped {
            NoteMetaChips(date: date, attendeeCount: attendeeCount, showCalendarIcon: false, onMetadata: onMetadata)
                .padding(.top, 14).padding(.bottom, 18)
                .transition(motion.fade)
        }
        ZStack(alignment: .topLeading) {
            if text.isEmpty && focused.wrappedValue != .body {
                Text("Write notes here...").readingFont(.body).foregroundStyle(Theme.textTertiary)
                    .padding(.top, 8).allowsHitTesting(false)
                    .transition(motion.fade)
            }
            TextEditor(text: $text).readingFont(.body).foregroundStyle(Theme.textPrimary)
                .scrollContentBackground(.hidden).focused(focused, equals: .body)
                .padding(.horizontal, -5).accessibilityLabel("Meeting notes")
                .accessibilityIdentifier("\(identifierPrefix)-body")
        }.frame(maxHeight: .infinity)
    }
}

struct HideKeyboardButton: View {
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Image(systemName: "keyboard.chevron.compact.down").font(.system(size: 17))
                .foregroundStyle(Theme.textPrimary.opacity(0.85)).frame(width: 48, height: 48)
                .contentShape(Circle())
        }.buttonStyle(.squash)
            .glassSurface(Circle(), interactive: true, tint: .black.opacity(0.25))
            .accessibilityLabel("Hide keyboard")
    }
}

extension Date {
    /// The API permits ISO-8601 timestamps with or without fractional seconds.
    static func serverTimestamp(_ value: String) -> Date? {
        ServerTimestampParser.fractional.date(from: value) ?? ServerTimestampParser.whole.date(from: value)
    }
}

/// Built once. Creating an ISO 8601 formatter per call was the largest cost
/// of sorting the server Home list. The formatters are safe to share across threads.
private enum ServerTimestampParser {
    nonisolated(unsafe) static let fractional: ISO8601DateFormatter = {
        let parser = ISO8601DateFormatter()
        parser.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return parser
    }()
    nonisolated(unsafe) static let whole: ISO8601DateFormatter = {
        let parser = ISO8601DateFormatter()
        parser.formatOptions = [.withInternetDateTime]
        return parser
    }()
}
