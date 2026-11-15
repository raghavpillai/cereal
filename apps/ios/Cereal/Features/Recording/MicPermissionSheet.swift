import SwiftUI

struct MicPermissionSheet: View {
    /// Gives a 342 pt sheet once the system adds the bottom inset.
    static let detentHeight: CGFloat = 302

    var onGranted: (() -> Void)? = nil
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @Environment(\.motion) private var motion
    @State private var requesting = false
    /// Plays the icon's effect once as the sheet arrives; a permission prompt needs no loop.
    @State private var appeared = false
    @State private var denied = false

    var body: some View {
        // Positions measured from the reference: the sheet is 342 pt tall,
        // the close circle is centred 39 pt below its top edge and 24 pt from
        // the screen's trailing edge (inside a Duo's rail column).
        VStack(spacing: 0) {
                Image(systemName: "waveform")
                    .font(.system(size: 41, weight: .light))
                    .foregroundStyle(Theme.textSecondary)
                    .symbolEffect(.variableColor.iterative, options: .nonRepeating, isActive: appeared && !motion.reduceMotion)
                    .onAppear { appeared = true }
                    .padding(.top, 42)
                    .padding(.bottom, 24)

                Text(denied ? "Microphone access is off" : onGranted == nil ? "Enable your microphone\nto record on this device" : "Enable your microphone\nto start transcribing")
                    .font(.display(29))
                    .multilineTextAlignment(.center)
                    .foregroundStyle(Theme.textPrimary)
                    .padding(.horizontal, 20)
                    .contentTransition(.opacity)

                if denied {
                    Text("Turn it on in Settings to record meetings.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.textSecondary)
                        .padding(.top, 10)
                        .transition(motion.rise)
                }

                Spacer(minLength: 0)

                Button(action: proceed) {
                    Text(denied ? "Open Settings" : "Continue")
                        .font(.system(size: 17, weight: .medium))
                        .foregroundStyle(Theme.onAccent)
                        .contentTransition(.opacity)
                        .frame(maxWidth: .infinity)
                        .frame(height: 53)
                        .glassSurface(Capsule(), interactive: true, tint: Theme.lightButton)
                }
                .buttonStyle(.squash)
                .disabled(requesting)
                .padding(.leading, 24).padding(.trailing, 24)
                .padding(.bottom, 14)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .overlay(alignment: .topTrailing) {
                // Pinned to the sheet's own edge, which on a Duo lies inside the rail column.
                GlassCircleButton(systemImage: "xmark", size: 48, glyphSize: 20) { dismiss() }
                    .padding(.top, 17)
                    .padding(.trailing, 16)
                    .frame(maxWidth: .infinity, alignment: .trailing)
                    .ignoresSafeArea(.container, edges: .horizontal)
            }
    }

    private func proceed() {
        if denied {
            if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
            return
        }
        requesting = true
        Task {
            let granted: Bool
            if await MicPermission.isGranted {
                granted = true
            } else if MicPermission.isUndetermined {
                granted = await MicPermission.request()
            } else {
                granted = false
            }
            requesting = false
            if granted {
                dismiss()
                try? await Task.sleep(for: .milliseconds(350))
                if let onGranted { onGranted() }
                else { router.newNoteID = store.createNote().id }
            } else {
                withAnimation(motion.settle) { denied = true }
            }
        }
    }
}
