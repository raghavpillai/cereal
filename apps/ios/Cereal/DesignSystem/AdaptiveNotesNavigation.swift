import SwiftUI

/// One navigation stack on every layout. A Duo keeps the same single column
/// whether folded or unfolded, with its controls in the trailing vertical
/// rail, so the open note or chat survives a fold untouched. Recording remains
/// owned by CerealApp, outside navigation.
struct AdaptiveNotesNavigation<Content: View>: View {
    @Binding var path: [Route]
    @ViewBuilder var content: Content

    var body: some View {
        NavigationStack(path: $path) { content }
            .cerealAdaptiveToolbarPlacement()
    }
}
