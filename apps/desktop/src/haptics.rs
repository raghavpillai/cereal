//! Trackpad haptics, used sparingly, as macOS itself uses them: a tap when a
//! drag meets a detent or a limit, and a single confirmation when the person
//! starts or stops a recording. They are felt only on a Force Touch trackpad
//! and only while a finger rests on it; everywhere else they do nothing.

/// The three patterns macOS offers (`NSHapticFeedbackPattern`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    /// A plain tap that confirms an action.
    Generic = 0,
    /// Something being dragged lines up with, or stops at, a mark.
    Alignment = 1,
    /// A step from one level or state to another.
    LevelChange = 2,
}

/// Plays `haptic` with the next frame drawn, so the tap lands with what the
/// person sees change.
#[allow(unexpected_cfgs)]
pub fn play(haptic: Haptic) {
    if cfg!(test) {
        return;
    }
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::base::{id, nil};
        use objc::{class, msg_send, sel, sel_impl};
        // NSHapticFeedbackPerformanceTimeDrawCompleted.
        const DRAW_COMPLETED: usize = 2;
        let performer: id = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];
        if performer != nil {
            let _: () = msg_send![performer, performFeedbackPattern: haptic as isize performanceTime: DRAW_COMPLETED];
        }
    }
}

/// Whether moving a dragged edge from `before` to `after` should tap: when it
/// passes or lands on `detent` (its usual place), and when it first comes to
/// rest against `low` or `high`, the furthest it goes. Holding still against
/// a limit, or moving away from one, is silent.
pub fn edge_tap(before: f32, after: f32, detent: f32, low: f32, high: f32) -> bool {
    if before == after || !before.is_finite() || !after.is_finite() {
        return false;
    }
    let crossed = (before < detent && after >= detent) || (before > detent && after <= detent);
    let pinned = (after <= low && before > low) || (after >= high && before < high);
    crossed || pinned
}

#[cfg(test)]
mod tests {
    use super::edge_tap;

    #[test]
    fn a_dragged_edge_taps_at_its_detent_and_at_its_limits_only() {
        let (detent, low, high) = (192., 168., 320.);
        // Passing the usual width either way, or landing exactly on it.
        assert!(edge_tap(180., 200., detent, low, high));
        assert!(edge_tap(200., 180., detent, low, high));
        assert!(edge_tap(190., 192., detent, low, high));
        // Leaving it again is silent: the tap was on the way in.
        assert!(!edge_tap(192., 193., detent, low, high));
        // Ordinary movement between marks.
        assert!(!edge_tap(200., 240., detent, low, high));
        assert!(!edge_tap(170., 175., detent, low, high));
        // Arriving at either limit, once.
        assert!(edge_tap(172., 168., detent, low, high));
        assert!(edge_tap(310., 320., detent, low, high));
        assert!(!edge_tap(168., 168., detent, low, high));
        assert!(!edge_tap(320., 320., detent, low, high));
        // Pulling away from a limit.
        assert!(!edge_tap(168., 175., detent, low, high));
        assert!(!edge_tap(320., 300., detent, low, high));
        assert!(!edge_tap(f32::NAN, 200., detent, low, high));
    }
}
