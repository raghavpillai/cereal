//! The version row of Settings → About: where an update is taken on
//! purpose, and so the update's stage.
//!
//! At rest it is a settings row like the ones under it: the mark in its
//! tile, "Version", a sentence, the version and "Check for updates".
//! Checking, the mark in the tile turns (the loader's loop) and the button
//! says so; nothing newer, and the mark nods, a check is drawn and the
//! sentence says "You're up to date." for a few seconds.
//!
//! A version that is found makes the row the update's card. It grows: the
//! tile into a plate, the mark in it into a bowl three and a half times the
//! size, the rows under it sliding down. Beside the bowl are the notice's
//! own title, a line under it and the two versions, old and new; at the
//! trailing edge "Later" and the one capsule the notice has, which is the
//! row's own button: "Update to 0.2.0", then the share that has arrived,
//! then "Restart to update". The bowl plays what it plays in the notice
//! (`update_bowl.rs`) with room for more: the megabytes roll, the pace of
//! the download is said when it is known, and a line of gold runs along the
//! row's bottom edge. When the update is over or put off, the row is a row
//! again.
//!
//! Everything it shows is read from `Updates` (`update_view.rs`), which the
//! notice in the corner reads too; that one is not shown while this page is
//! on screen. Sizes are springs on the view's stage and are clamped where
//! they are heights; the bowl, the plate, the check and the line are
//! painted. Reduced motion shows every state at once and at rest, in plain
//! words.
use crate::{
    app::Cereal,
    motion::{self, GOOEY, SNAPPY, TrackKey},
    screens::{
        ROW_HEIGHT, ROW_ICON, ROW_PAD, ROW_TILE, ROW_TILE_RADIUS, settings_row_shell,
        settings_row_text, settings_row_title,
    },
    ui::*,
    update,
    update_bowl::{self, Act, Counter, Look},
    update_view::{self, Seat},
};
use gpui::{prelude::*, *};
use std::{cell::RefCell, time::Instant};

/// The plate the tile grows into, and the bowl the mark grows into.
pub const PLATE: f32 = 72.;
pub const BOWL: f32 = 56.;
/// How thick the download's line along the row's bottom edge is.
const LINE: f32 = 2.;
/// A check mark beside 12 px text, and the gap after it.
const CHECK: f32 = 12.;
const CHECK_GAP: f32 = 4.;
/// The arrow between the two versions, and the gap either side of it.
const ARROW: f32 = 12.;
const ARROW_GAP: f32 = 6.;
/// How long after the bowl pops the new version gets its check, in seconds.
const RESOLVED_CUE: f32 = 0.3;

/// The row's measures part of the way from a settings row to the update's
/// card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// The row's least height.
    pub row: f32,
    /// The tile's side and corner.
    pub plate: f32,
    pub radius: f32,
    /// The mark's side.
    pub bowl: f32,
}

/// The row `grown` of the way to the card (0 a settings row, 1 the card; a
/// spring's value, so briefly past either). The tile keeps its distance
/// from the row's edges and its corner keeps its proportion; neither goes
/// past its end, which would open a gap under the row and close it again.
/// Only the mark follows the spring past its end, inside its plate.
pub fn shape(grown: f32) -> Shape {
    let open = grown.clamp(0., 1.);
    let plate = ROW_TILE + (PLATE - ROW_TILE) * open;
    Shape {
        row: plate + (ROW_HEIGHT - ROW_TILE),
        plate,
        radius: ROW_TILE_RADIUS * plate / ROW_TILE,
        bowl: (ROW_ICON + (BOWL - ROW_ICON) * grown).clamp(ROW_ICON * 0.8, plate),
    }
}

/// A wash over a ground, as one opaque colour.
fn over(ground: Hsla, wash: Hsla) -> Hsla {
    motion::mix(ground, Hsla { a: 1., ..wash }, wash.a)
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

thread_local! {
    /// The two versions last shown, kept so their line can finish leaving.
    static PAIR: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

impl Cereal {
    /// The first row of Settings → About: the version this app is, and the
    /// whole of an update from the question to the restart.
    pub fn render_version_row(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        const GROWN: TrackKey = ("about-update", 0);
        const PAIRED: TrackKey = ("about-update", 1);
        const RESOLVED: TrackKey = ("about-update", 2);
        const CHECKED: TrackKey = ("about-update", 3);
        const LINED: TrackKey = ("about-update", 4);
        const PACED: TrackKey = ("about-update", 5);
        const TITLE: TrackKey = ("about-update", 6);
        const DETAIL: TrackKey = ("about-update", 7);
        const LATER: TrackKey = ("about-update", 8);
        let t = self.theme();
        let now = Instant::now();
        let reduced = motion::reduce_motion();
        let row = self.updates.about_row(now);
        // Coming to the page, the row is simply as the update is now.
        let fresh = !self.stage.continuous(GROWN);
        if fresh {
            for key in [GROWN, PAIRED, RESOLVED, CHECKED, LINED, PACED] {
                self.stage.forget(key);
            }
        }
        let aim = |key: TrackKey, on: bool, spring| {
            self.stage.aim(key, if on { 1. } else { 0. }, spring)
        };
        let grown = aim(GROWN, row.open, GOOEY);
        let open = grown.clamp(0., 1.);
        let shape = shape(grown);
        let (pose, since) = self.updates.mark(now);
        let (level, speed) = self.updates.level();

        // The plate, the bowl in it and the download's line: painted, at
        // fractions of a pixel, over the whole of the row.
        let plate_ink = t.hover;
        let ground = over(t.card, plate_ink);
        // At rest the mark is the settings icon it has always been, all in
        // the accent; grown, the notice's bowl in ink with its gold.
        let look = Look {
            ground,
            ink: t.accent,
            gilt: t.accent,
            dull: t.muted,
            grand: false,
        }
        .towards(
            Look {
                ground,
                grand: true,
                ..Look::of(t)
            },
            open,
        );
        let lined = aim(LINED, row.line, SNAPPY).clamp(0., 1.);
        let stage = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let (left, top, height) = (
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.height),
                );
                // In the middle of the row's height, which is its inset
                // from the left as well unless a sentence has wrapped.
                let slack = (height - shape.plate) / 2.;
                let inset = if (slack - ROW_PAD.0).abs() < 1. { ROW_PAD.0 } else { slack };
                let (x, y) = (left + ROW_PAD.0, top + inset);
                window.paint_quad(
                    fill(
                        Bounds::new(point(px(x), px(y)), size(px(shape.plate), px(shape.plate))),
                        plate_ink,
                    )
                    .corner_radii(px(shape.radius)),
                );
                let within = (shape.plate - shape.bowl) / 2.;
                update_bowl::paint_bowl(window, (x + within, y + within), shape.bowl, &pose, &look);
                if lined > 0.01 {
                    let across = f32::from(bounds.size.width) - 2. * ROW_PAD.0;
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                point(px(x), px(top + height - LINE)),
                                size(px((across * level.clamp(0., 1.)).max(LINE)), px(LINE)),
                            ),
                            update_bowl::gold().opacity(lined),
                        )
                        .corner_radii(px(LINE / 2.)),
                    );
                }
            },
        )
        .absolute()
        .inset_0();

        // The title: "Version", or the notice's own.
        let title = settings_row_title(t)
            .truncate()
            .when(row.open, |title| title.font_weight(FontWeight::SEMIBOLD))
            .map(|title| {
                if row.sheen && !reduced {
                    title.child(motion::sheen_label(
                        "about-title-sheen",
                        row.title.clone(),
                        13.,
                        t.muted,
                        t.text,
                        cx,
                    ))
                } else {
                    title.child(row.title.clone())
                }
            });

        // The line under it. While the download is counted its figures
        // roll, and its pace is said after them once it is known.
        let detail = match row.counts.filter(|_| !reduced) {
            Some(total) => {
                let (decimals, most) = update_view::counted(total);
                let counter = Counter {
                    value: f64::from(level) * most,
                    speed: speed * most as f32,
                    decimals,
                    most,
                };
                let tail = format!(" of {:.*} MB", decimals as usize, total as f64 / 1_000_000.);
                let pace = row.pace.as_ref().map(|pace| format!(" · {pace}"));
                let paced = aim(PACED, pace.is_some(), SNAPPY).clamp(0., 1.);
                let ink = t.muted;
                div()
                    .h(px(16.))
                    .w_full()
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, cx| {
                                const METRICS: (f32, f32, f32) = (12., 16., 13.);
                                update_bowl::paint_counter(
                                    Some(&counter),
                                    &tail,
                                    bounds.origin,
                                    METRICS,
                                    ink,
                                    window,
                                    cx,
                                );
                                let Some(pace) = pace.as_deref() else {
                                    return;
                                };
                                // Said whole or not at all.
                                let counted = update_bowl::counter_width(Some(&counter), &tail, 12., window);
                                let needs = update_bowl::counter_width(None, pace, 12., window);
                                if counted + needs > f32::from(bounds.size.width) {
                                    return;
                                }
                                update_bowl::paint_counter(
                                    None,
                                    pace,
                                    point(bounds.origin.x + px(counted), bounds.origin.y),
                                    METRICS,
                                    ink.opacity(paced),
                                    window,
                                    cx,
                                );
                            },
                        )
                        .size_full(),
                    )
                    .into_any_element()
            }
            // Asking the server: the sentence that says so has the sheen of
            // work going on.
            None if row.act == Act::Seeking && !row.open && !reduced => div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(t.muted)
                .child(motion::sheen_label(
                    "about-detail-sheen",
                    row.detail.clone(),
                    12.,
                    t.muted,
                    t.text,
                    cx,
                ))
                .into_any_element(),
            None => {
                let plain = match (&row.pace, row.counts) {
                    (Some(pace), Some(_)) => format!("{} · {pace}", row.detail),
                    _ => row.detail.clone(),
                };
                small(plain, t).into_any_element()
            }
        };
        // A line that counts is one line however it reads: it does not
        // come in again with every figure.
        let detail_kind = if row.counts.is_some() {
            hash("counted")
        } else {
            hash(&row.detail)
        };
        // The check before "You're up to date.": it makes its own room.
        let checked = aim(CHECKED, row.checked, SNAPPY).clamp(0., 1.);
        let check = |room: f32, drawn: f32| {
            let color = motion::mix(t.card, t.accent, room);
            div()
                .w(px((CHECK + CHECK_GAP) * room))
                .h(px(16.))
                .flex_shrink_0()
                .overflow_hidden()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            update_bowl::paint_check(
                                window,
                                (
                                    f32::from(bounds.origin.x),
                                    f32::from(bounds.origin.y) + (16. - CHECK) / 2.,
                                ),
                                CHECK,
                                drawn,
                                color,
                            );
                        },
                    )
                    .size_full(),
                )
        };
        let check_drawn = |after: Option<f32>| match after {
            Some(after) if !reduced => update_bowl::check_drawn(after),
            _ => 1.,
        };
        let detail = div()
            .mt(px(2.))
            .flex()
            .items_start()
            .when(checked > 0.001, |line| {
                line.child(check(
                    checked,
                    check_drawn(self.updates.pleased_since(now).map(|since| since - 0.12)),
                ))
            })
            .child(
                div().flex_1().min_w_0().child(motion::swapped(
                    div().w_full().child(detail),
                    self.stage.swap(DETAIL, detail_kind),
                )),
            );

        // The two versions, old and new; in hand, only the new one.
        if row.pair.is_some() {
            PAIR.with(|pair| *pair.borrow_mut() = row.pair.clone());
        }
        let paired = aim(PAIRED, row.pair.is_some(), GOOEY).clamp(0., 1.);
        // Leaving, it keeps the look it had.
        let resolved = if row.open {
            aim(RESOLVED, row.resolved, GOOEY)
        } else {
            self.stage.target(RESOLVED).unwrap_or(0.)
        }
        .clamp(0., 1.);
        let pair = PAIR
            .with(|pair| pair.borrow().clone())
            .filter(|_| paired > 0.001)
            .map(|(old, new)| {
                let lead = update_view::text_width(&old, 12., FontWeight::NORMAL, window).ceil()
                    + ARROW_GAP
                    + ARROW
                    + ARROW_GAP;
                let unresolved = 1. - resolved;
                div()
                    .h(px(18. * paired))
                    .overflow_hidden()
                    .opacity(motion::fade_for(paired))
                    .child(
                        div()
                            .mt(px(2.))
                            .h(px(16.))
                            .flex()
                            .items_center()
                            .when(unresolved > 0.001, |line| {
                                line.child(
                                    div()
                                        .w(px(lead * unresolved))
                                        .flex_shrink_0()
                                        .overflow_hidden()
                                        .opacity(motion::fade_for(unresolved))
                                        .flex()
                                        .items_center()
                                        .gap(px(ARROW_GAP))
                                        .child(small(old, t).flex_shrink_0().whitespace_nowrap())
                                        .child(icon("arrow-left", ARROW, t.muted).with_transformation(
                                            Transformation::rotate(radians(std::f32::consts::PI)),
                                        )),
                                )
                            })
                            .when(resolved > 0.001, |line| {
                                line.child(check(
                                    resolved,
                                    check_drawn((row.act == Act::Ready).then_some(since - RESOLVED_CUE)),
                                ))
                            })
                            .child(
                                small(new, t)
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .text_color(t.text)
                                    .font_weight(FontWeight::MEDIUM),
                            ),
                    )
            });

        // At the trailing edge: the version this is, which the two
        // versions take the place of; "Later"; and the capsule.
        let version = update::current_version();
        let closed = 1. - open;
        let later = self.render_update_later(
            LATER,
            "about-update-later",
            row.open && self.updates.row_faces.1.filled(),
            fresh,
            window,
            cx,
        );
        let capsule = self.render_update_capsule(Seat::About, fresh, window, cx);
        let trailing = div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .when(closed > 0.001, |trailing| {
                let label = small(version, t).flex_shrink_0().whitespace_nowrap();
                if closed >= 1. {
                    return trailing.child(label.mr(px(crate::screens::ROW_GAP)));
                }
                // Folding: clipped from its far side, and gone by the time
                // "Later" is half way in beside it.
                let room = update_view::text_width(version, 12., FontWeight::NORMAL, window).ceil()
                    + crate::screens::ROW_GAP;
                trailing.child(
                    div()
                        .w(px(room * closed))
                        .flex_shrink_0()
                        .overflow_hidden()
                        .opacity(motion::fade_curve(((closed - 0.5) / 0.5).clamp(0., 1.)))
                        .flex()
                        .justify_end()
                        .child(label.mr(px(crate::screens::ROW_GAP))),
                )
            })
            .children(later)
            .children(capsule);

        settings_row_shell()
            .min_h(px(shape.row))
            .child(stage)
            // The plate's room in the row.
            .child(div().w(px(shape.plate)).h(px(ROW_TILE)).flex_shrink_0())
            .child(
                settings_row_text()
                    .child(motion::swapped(
                        title,
                        self.stage.swap(TITLE, hash(&row.title)),
                    ))
                    .child(detail)
                    .children(pair),
            )
            .child(trailing)
    }
}

#[cfg(test)]
mod tests {
    use super::{BOWL, PLATE, shape};
    use crate::{
        motion::GOOEY,
        screens::{ROW_HEIGHT, ROW_ICON, ROW_PAD, ROW_TILE, ROW_TILE_RADIUS},
    };

    #[test]
    fn at_rest_the_row_is_a_settings_row() {
        let rest = shape(0.);
        assert_eq!(rest.row, ROW_HEIGHT);
        assert_eq!(rest.plate, ROW_TILE);
        assert_eq!(rest.radius, ROW_TILE_RADIUS);
        assert_eq!(rest.bowl, ROW_ICON);
    }

    #[test]
    fn grown_it_holds_the_bowl_with_the_tiles_own_inset_and_corner() {
        let grown = shape(1.);
        assert_eq!((grown.plate, grown.bowl), (PLATE, BOWL));
        // As far from the row's top and bottom as from its left edge.
        assert_eq!((grown.row - grown.plate) / 2., ROW_PAD.0);
        // The corner in the tile's proportion.
        assert_eq!(grown.radius / grown.plate, ROW_TILE_RADIUS / ROW_TILE);
        // The bowl at its biggest (the pop, a fifth over) stays on the
        // plate.
        assert!(grown.bowl * 1.2 <= grown.plate);
    }

    #[test]
    fn growing_never_jumps_and_never_opens_a_gap_under_the_row() {
        let mut before = shape(0.);
        for step in 1..=200 {
            // The spring the row grows on, overshoot and all.
            let grown = GOOEY.at(step as f32 / 200.);
            let now = shape(grown);
            // The tile's inset is the same all the way.
            assert!(((now.row - now.plate) / 2. - ROW_PAD.0).abs() < 1e-4);
            // Heights stop at their ends; only the mark swings past its
            // own, and never off its plate.
            assert!((ROW_HEIGHT..=PLATE + 2. * ROW_PAD.0).contains(&now.row));
            assert!(now.bowl <= now.plate);
            assert!(now.bowl <= BOWL * 1.1, "{} px", now.bowl);
            // A frame's worth of the spring moves it a few px at most.
            assert!((now.row - before.row).abs() < 3., "step {step}");
            assert!((now.bowl - before.bowl).abs() < 3., "step {step}");
            before = now;
        }
        assert_eq!(before, shape(1.));
        // Shrinking, the spring dips under nothing: the row stays a row.
        assert_eq!(shape(-0.05).row, ROW_HEIGHT);
        assert!(shape(-0.05).bowl >= ROW_ICON * 0.8);
    }
}
