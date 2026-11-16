//! Windowing for long columns that live inside an ordinary scroll container.
//!
//! GPUI lays out and paints every child of a scrolling `div` on every frame,
//! whether or not it is on screen. A list of 450 notes costs about 22 ms a
//! frame that way and a 1,500-utterance transcript 40–60 ms (release build,
//! see `bench/`), which every animation on the page then inherits.
//!
//! A [`column`] builds only the items near the viewport. It needs no height
//! estimates and no knowledge of what the items are:
//!
//! - An item whose height is not yet known is always built, with a probe that
//!   records the height it was given. So the first frame after the content or
//!   the width changes costs what every frame used to cost, and measures
//!   everything exactly.
//! - From then on an item is built only if it is within `overscan` of the
//!   viewport; runs of the others collapse into one spacer of exactly their
//!   measured height. The content keeps its true size, so the scroll position,
//!   `scroll_to_bottom` and the scrollbar range are unchanged, and nothing
//!   shifts as items enter and leave the window.
//!
//! The scroll container, its `ScrollHandle` and the items themselves stay as
//! they were; only the children of the column are chosen differently.
//!
//! That first frame is too expensive for long content (an hour of transcript,
//! a 1,500-bullet summary: 50 to 100 ms). An [`estimated`] column is given a
//! guess of each item's height and a stable identity for each item instead:
//!
//! - An item that has never been built stands in the spacers at its guessed
//!   height, and is measured when it first comes near the viewport. Opening
//!   long content builds one screenful.
//! - A guess is rarely exact. So that a correction above the reader never
//!   moves what they are reading, the column remembers which item sat at the
//!   top of the viewport and how far into it, and after any change above it
//!   (a guess replaced by a measurement, rows inserted or removed) puts that
//!   same point back at the top of the viewport by adjusting the scroll
//!   offset in the same frame. An item first built above the viewport is held
//!   at its guessed height for that one frame, so its real height and the
//!   offset that compensates for it arrive together.

use gpui::{prelude::*, *};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    ops::Range,
};

/// How far beyond the viewport items are kept built, so a fast scroll does
/// not outrun them. About one window's height either side.
pub const OVERSCAN: f32 = 700.;

#[derive(Default)]
struct Column {
    /// The width the heights were measured at; they are forgotten when it
    /// changes, because wrapped text changes height with it.
    width: f32,
    heights: HashMap<u64, f32>,
    /// Guessed heights of items that have not been measured, by key.
    guesses: HashMap<u64, f32>,
    /// Top of the column within its scroll container's content.
    top: f32,
    /// An item to bring into view once its position is known.
    reveal: Option<usize>,
    /// The caller is scrolling its container to the bottom this frame.
    to_end: bool,
    scroll: Option<ScrollHandle>,
    /// The items as the last frame laid them out (estimated columns only).
    laid: Vec<Laid>,
}

/// One item of the previous frame's layout.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Laid {
    id: u64,
    key: u64,
    /// The height the frame was planned with.
    height: f32,
    /// The item was built at its natural height without that being known
    /// beforehand: its real height is whatever its probe then recorded.
    natural: bool,
}

thread_local! {
    static COLUMNS: RefCell<HashMap<&'static str, Column>> = RefCell::new(HashMap::new());
}

/// What a column last knew, for the scripted tour: how many heights it has
/// measured, their sum and where the column starts.
#[cfg(debug_assertions)]
pub fn describe(name: &'static str) -> String {
    COLUMNS.with(|columns| {
        columns.borrow().get(name).map_or("no such column".into(), |column| {
            let mut heights: Vec<f32> = column.heights.values().copied().collect();
            heights.sort_by(f32::total_cmp);
            format!(
                "{} heights {:?}, top {}",
                heights.len(),
                heights,
                column.top
            )
        })
    })
}

/// A key for one item: everything that can change its height.
pub fn key(parts: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut hasher);
    hasher.finish()
}

/// A scroll handle that lives as long as the column called `name`, for pages
/// whose scroll container did not need one before.
pub fn scroll(name: &'static str) -> ScrollHandle {
    COLUMNS.with(|columns| {
        columns
            .borrow_mut()
            .entry(name)
            .or_default()
            .scroll
            .get_or_insert_with(ScrollHandle::new)
            .clone()
    })
}

/// Bring item `index` of the column into view on its next render (the
/// equivalent of `ScrollHandle::scroll_to_item` for a windowed column, whose
/// children are no longer one per item).
pub fn reveal(name: &'static str, index: usize) {
    COLUMNS.with(|columns| columns.borrow_mut().entry(name).or_default().reveal = Some(index));
}

/// The caller is about to scroll the column's container to its bottom (as
/// `ScrollHandle::scroll_to_bottom` does, after this frame's layout): build
/// the end of the column rather than what the old offset points at.
pub fn to_end(name: &'static str) {
    COLUMNS.with(|columns| columns.borrow_mut().entry(name).or_default().to_end = true);
}

/// Where each item starts in column coordinates, and where the column ends.
pub fn tops(heights: &[f32], gap: f32) -> (Vec<f32>, f32) {
    let mut tops = Vec::with_capacity(heights.len());
    let mut top = 0.;
    for height in heights {
        tops.push(top);
        top += height + gap;
    }
    (tops, if heights.is_empty() { 0. } else { top - gap })
}

/// The item of a layout that holds position `at` (the last one starting at
/// or before it), and how far into the item that position is.
fn anchor(tops: &[f32], at: f32) -> Option<(usize, f32)> {
    let index = tops.partition_point(|top| *top <= at).checked_sub(1)?;
    Some((index, at - tops[index]))
}

/// Where the item `id` is now, looking first where it was (`hint`): rows are
/// mostly added at the end, which leaves every earlier one in place.
fn locate(ids: &[u64], id: u64, hint: usize) -> Option<usize> {
    if ids.get(hint) == Some(&id) {
        return Some(hint);
    }
    (1..ids.len().max(hint + 1)).find_map(|distance| {
        let after = hint + distance;
        let before = hint.checked_sub(distance);
        if ids.get(after) == Some(&id) {
            Some(after)
        } else {
            before.filter(|before| ids.get(*before) == Some(&id))
        }
    })
}

/// The position in the new layout of what was at position `at` of the old
/// one: the same item, the same distance into it. `None` when that item is
/// gone, or `at` is above the column. This is what keeps a reader's place
/// while rows above them are inserted, removed or measured.
pub fn carried(
    old_ids: &[u64],
    old_tops: &[f32],
    ids: &[u64],
    new_tops: &[f32],
    at: f32,
) -> Option<f32> {
    if at <= 0. {
        return None;
    }
    let (index, within) = anchor(old_tops, at)?;
    let now = locate(ids, *old_ids.get(index)?, index)?;
    Some(new_tops.get(now)? + within)
}

/// One child of a windowed column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slot {
    /// Items that are out of view, as one spacer of their combined height.
    Gap(f32),
    /// An item to build.
    Item(usize),
}

/// Which items to build: those of unknown height and those intersecting
/// `visible` (in column coordinates). `gap` is the column's flex gap, which
/// a spacer standing in for several items has to include.
pub fn slots(heights: &[Option<f32>], gap: f32, visible: Range<f32>) -> Vec<Slot> {
    let mut slots = Vec::new();
    let mut top = 0.;
    // Height of the pending spacer, and how many items it stands for.
    let mut hidden: Option<f32> = None;
    for (index, height) in heights.iter().enumerate() {
        let shown = match height {
            None => true,
            Some(height) => top + height >= visible.start && top <= visible.end,
        };
        if shown {
            if let Some(spacer) = hidden.take() {
                slots.push(Slot::Gap(spacer));
            }
            slots.push(Slot::Item(index));
        } else {
            let height = height.unwrap_or(0.);
            hidden = Some(match hidden {
                Some(spacer) => spacer + gap + height,
                None => height,
            });
        }
        top += height.unwrap_or(0.) + gap;
    }
    if let Some(spacer) = hidden {
        slots.push(Slot::Gap(spacer));
    }
    slots
}

/// Where item `index` starts and ends in column coordinates, if every item
/// up to and including it has been measured.
pub fn position(heights: &[Option<f32>], gap: f32, index: usize) -> Option<Range<f32>> {
    let mut top = 0.;
    for height in heights.get(..index)? {
        top += (*height)? + gap;
    }
    Some(top..top + (*heights.get(index)?)?)
}

/// The scroll offset that brings `item` (in content coordinates) into a
/// viewport `viewport` tall currently scrolled to `scrolled`, moving as
/// little as possible; `None` when it is already fully in view.
pub fn offset_to_show(item: Range<f32>, scrolled: f32, viewport: f32) -> Option<f32> {
    if item.start < scrolled {
        Some(item.start)
    } else if item.end > scrolled + viewport {
        Some((item.end - viewport).max(0.))
    } else {
        None
    }
}

/// A flex column of the items identified by `keys`, building only those near
/// the viewport of `scroll` (the handle tracked by the scroll container the
/// column sits in). `width` is whatever the items' heights depend on; pass a
/// constant when they do not wrap. `build` makes item `index`.
pub fn column(
    name: &'static str,
    keys: &[u64],
    width: f32,
    gap: f32,
    scroll: &ScrollHandle,
    window: &Window,
    build: impl FnMut(usize) -> AnyElement,
) -> Div {
    windowed(name, keys, None, width, gap, scroll, window, build)
}

/// A [`column`] whose items need not all be built once to be measured.
/// `ids` names each item across changes of the list (its key says how tall
/// it is; its ID says which one it is), and `estimate` guesses the height of
/// item `index`; it is asked once per key. See the module documentation.
#[allow(clippy::too_many_arguments)]
pub fn estimated(
    name: &'static str,
    ids: &[u64],
    keys: &[u64],
    width: f32,
    gap: f32,
    scroll: &ScrollHandle,
    window: &Window,
    mut estimate: impl FnMut(usize) -> f32,
    build: impl FnMut(usize) -> AnyElement,
) -> Div {
    debug_assert_eq!(ids.len(), keys.len());
    windowed(
        name,
        keys,
        Some((ids, &mut estimate)),
        width,
        gap,
        scroll,
        window,
        build,
    )
}

/// Where the viewport starts within a column `total` tall, for choosing what
/// to build. A wheel event moves the scroll offset by whatever it carries and
/// the container only brings it back into range when it is next laid out,
/// after the column has been built: an offset far past either end would
/// otherwise select no items at all, and the frame that then shows the end
/// of the list would show an empty spacer in its place.
fn within(start: f32, total: f32, view_height: f32) -> f32 {
    start.clamp(-view_height, (total - view_height).max(-view_height))
}

/// How an item is to be built this frame.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Its height is known: build it and measure it again.
    Measured,
    /// Never built before, in or below the viewport: at its natural height.
    Natural,
    /// Never built before, entirely above the viewport: held at its guessed
    /// height (the number given) for this frame while it is measured.
    Held(f32),
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn windowed(
    name: &'static str,
    keys: &[u64],
    mut estimates: Option<(&[u64], &mut dyn FnMut(usize) -> f32)>,
    width: f32,
    gap: f32,
    scroll: &ScrollHandle,
    window: &Window,
    mut build: impl FnMut(usize) -> AnyElement,
) -> Div {
    let viewport = scroll.bounds();
    let scrolled = -f32::from(scroll.offset().y);
    // Before the container's first layout its height is not known.
    let view_height = match f32::from(viewport.size.height) {
        height if height > 0. => height,
        _ => 900.,
    };
    let (plan, modes, again) = COLUMNS.with(|columns| {
        let mut columns = columns.borrow_mut();
        let column = columns.entry(name).or_default();
        if (column.width - width).abs() > 0.5 {
            column.width = width;
            column.heights.clear();
            column.guesses.clear();
        }
        // Forget items that no longer exist, now and then.
        if column.heights.len() + column.guesses.len() > keys.len() * 2 + 64 {
            let live: HashSet<u64> = keys.iter().copied().collect();
            column.heights.retain(|key, _| live.contains(key));
            column.guesses.retain(|key, _| live.contains(key));
        }
        let to_end = std::mem::take(&mut column.to_end);
        let mut scrolled = scrolled;
        let mut again = false;
        let Some((ids, estimate)) = estimates.as_mut() else {
            // Without estimates an unmeasured item is always built.
            let heights: Vec<Option<f32>> = keys
                .iter()
                .map(|key| column.heights.get(key).copied())
                .collect();
            if let Some(index) = column.reveal {
                match position(&heights, gap, index) {
                    Some(item) => {
                        column.reveal = None;
                        let item = item.start + column.top..item.end + column.top;
                        if let Some(target) = offset_to_show(item, scrolled, view_height) {
                            scroll.set_offset(point(scroll.offset().x, px(-target)));
                            scrolled = target;
                        }
                    }
                    // Not measured yet: this frame measures it, the next shows it.
                    None if index < keys.len() => again = true,
                    None => column.reveal = None,
                }
            }
            let total: f32 = heights.iter().flatten().sum::<f32>()
                + gap * keys.len().saturating_sub(1) as f32;
            let start = if to_end {
                (total - view_height).max(0.)
            } else {
                within(scrolled - column.top, total, view_height)
            };
            let visible = start - OVERSCAN..start + view_height + OVERSCAN;
            let plan = slots(&heights, gap, visible);
            return (plan, vec![Mode::Measured; keys.len()], again);
        };
        let measured: Vec<bool> = keys
            .iter()
            .map(|key| column.heights.contains_key(key))
            .collect();
        let heights: Vec<f32> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| match column.heights.get(key) {
                Some(height) => *height,
                None => *column
                    .guesses
                    .entry(*key)
                    // A whole number of pixels. Layout puts every row on
                    // the pixel grid, so a column of fractional guesses
                    // would be laid out a fraction of a pixel away from
                    // where their sum says, and each correction of the
                    // scroll offset (which is made in these numbers) would
                    // leave the reader's text that fraction out of place.
                    .or_insert_with(|| estimate(index).round().max(1.)),
            })
            .collect();
        let (new_tops, total) = tops(&heights, gap);
        // Keep the reader's place: whatever was at the top of the viewport
        // in the last frame's layout is put back there.
        if !to_end && !column.laid.is_empty() {
            // That layout as it really was: an item built at its natural
            // height for the first time had the height its probe recorded.
            let was: Vec<f32> = column
                .laid
                .iter()
                .map(|laid| match laid.natural {
                    true => column.heights.get(&laid.key).copied().unwrap_or(laid.height),
                    false => laid.height,
                })
                .collect();
            let old_ids: Vec<u64> = column.laid.iter().map(|laid| laid.id).collect();
            let (old_tops, _) = tops(&was, gap);
            let at = scrolled - column.top;
            if let Some(now) = carried(&old_ids, &old_tops, ids, &new_tops, at)
                && (now - at).abs() > 0.25
            {
                scrolled += now - at;
                scroll.set_offset(point(scroll.offset().x, px(-scrolled)));
            }
        }
        if let Some(index) = column.reveal {
            match (new_tops.get(index), heights.get(index)) {
                (Some(top), Some(height)) => {
                    let item = top + column.top..top + height + column.top;
                    if let Some(target) = offset_to_show(item, scrolled, view_height) {
                        scroll.set_offset(point(scroll.offset().x, px(-target)));
                        scrolled = target;
                    }
                    // Its place was a guess until it and what is around it
                    // have been measured: look once more next frame.
                    if measured[index] {
                        column.reveal = None;
                    } else {
                        again = true;
                    }
                }
                _ => column.reveal = None,
            }
        }
        let start = if to_end {
            (total - view_height).max(0.)
        } else {
            within(scrolled - column.top, total, view_height)
        };
        let visible = start - OVERSCAN..start + view_height + OVERSCAN;
        let known: Vec<Option<f32>> = heights.iter().copied().map(Some).collect();
        let plan = slots(&known, gap, visible);
        let modes: Vec<Mode> = (0..keys.len())
            .map(|index| {
                if measured[index] {
                    Mode::Measured
                } else if new_tops[index] + heights[index] <= start {
                    Mode::Held(heights[index])
                } else {
                    Mode::Natural
                }
            })
            .collect();
        let mut built = vec![false; keys.len()];
        for slot in &plan {
            if let Slot::Item(index) = slot {
                built[*index] = true;
            }
        }
        // A held item gets its real height, and the offset that makes up for
        // it, in the next frame: make sure there is one.
        again |= (0..keys.len()).any(|index| built[index] && matches!(modes[index], Mode::Held(_)));
        column.laid.clear();
        column.laid.extend((0..keys.len()).map(|index| Laid {
            id: ids[index],
            key: keys[index],
            height: heights[index],
            natural: built[index] && modes[index] == Mode::Natural,
        }));
        (plan, modes, again)
    });
    if again {
        window.request_animation_frame();
    }
    crate::motion::stats::listed(
        plan.iter()
            .filter(|slot| matches!(slot, Slot::Item(_)))
            .count(),
        keys.len(),
    );
    // Marks where the column starts within the scrolled content.
    let handle = scroll.clone();
    let anchor = canvas(
        move |bounds, _, _| {
            let top = f32::from(bounds.top() - handle.bounds().top() - handle.offset().y);
            COLUMNS.with(|columns| {
                if let Some(column) = columns.borrow_mut().get_mut(name) {
                    column.top = top;
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size(px(0.));
    let mut result = div()
        .relative()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(gap))
        .child(anchor);
    for slot in plan {
        result = match slot {
            Slot::Gap(height) => result.child(div().flex_shrink_0().h(px(height))),
            Slot::Item(index) => {
                let key = keys[index];
                // A flex wrapper, so the item's own margins count as part of
                // its measured height.
                let item = div()
                    .relative()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .child(build(index))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                let height = f32::from(bounds.size.height);
                                COLUMNS.with(|columns| {
                                    if let Some(column) = columns.borrow_mut().get_mut(name) {
                                        // The first measurement of a guessed
                                        // row says how good the guess was.
                                        if column.heights.insert(key, height).is_none()
                                            && let Some(guess) = column.guesses.get(&key)
                                        {
                                            // Layout rounds a row's edges to
                                            // whole pixels (57.75 px of text
                                            // and padding measures 57 or 58,
                                            // depending on where the row
                                            // starts), so a pixel either way
                                            // is the same height.
                                            crate::motion::stats::guessed(
                                                (guess - height).abs() <= 1.,
                                            );
                                        }
                                    }
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    );
                match modes[index] {
                    // Out of sight above the viewport: the frame keeps the
                    // height it was planned with while the item is measured.
                    Mode::Held(height) => result.child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .h(px(height))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(item),
                    ),
                    _ => result.child(item),
                }
            }
        };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn unmeasured_items_are_always_built_so_the_first_frame_measures_everything() {
        let unknown = vec![None; 5];
        assert_eq!(
            slots(&unknown, 0., 0.0..100.),
            (0..5).map(Slot::Item).collect::<Vec<_>>()
        );
        // One new item among measured ones is built wherever it is.
        let mut heights = vec![Some(48.); 100];
        heights[90] = None;
        let plan = slots(&heights, 0., 0.0..200.);
        assert!(plan.contains(&Slot::Item(90)));
        assert!(!plan.contains(&Slot::Item(60)));
    }

    #[test]
    fn an_offset_past_either_end_still_builds_that_end() {
        // 500 rows of 48 px in a 800 px viewport: the last screenful starts
        // at 23,200.
        assert_eq!(within(900_000., 24_000., 800.), 23_200.);
        assert_eq!(within(-900_000., 24_000., 800.), -800.);
        // In range, the offset is what it is, also above the column's top.
        assert_eq!(within(1_000., 24_000., 800.), 1_000.);
        assert_eq!(within(-300., 24_000., 800.), -300.);
        // A column shorter than the viewport.
        assert_eq!(within(5_000., 200., 800.), -600.);
    }

    #[test]
    fn only_items_near_the_viewport_are_built_and_the_rest_keep_their_exact_height() {
        // 500 rows on a 48 px pitch; the viewport shows 1000..1800.
        let heights = vec![Some(48.); 500];
        let plan = slots(&heights, 0., 1000.0..1800.);
        let built: Vec<usize> = plan
            .iter()
            .filter_map(|slot| match slot {
                Slot::Item(index) => Some(*index),
                Slot::Gap(_) => None,
            })
            .collect();
        // Rows 20 (ends at 1008) through 37 (starts at 1776).
        assert_eq!(built.first(), Some(&20));
        assert_eq!(built.last(), Some(&37));
        assert_eq!(plan.first(), Some(&Slot::Gap(20. * 48.)));
        assert_eq!(plan.last(), Some(&Slot::Gap((500 - 38) as f32 * 48.)));
        // The column is exactly as tall as if every row had been built.
        let total: f32 = plan
            .iter()
            .map(|slot| match slot {
                Slot::Gap(height) => *height,
                Slot::Item(_) => 48.,
            })
            .sum();
        assert_eq!(total, 500. * 48.);
    }

    #[test]
    fn a_spacer_includes_the_gaps_between_the_items_it_stands_for() {
        // Ten 60 px cards with an 8 px gap; only the last two are in view.
        let heights = vec![Some(60.); 10];
        let plan = slots(&heights, 8., 560.0..700.);
        // Eight hidden cards and the seven gaps between them; the gap to the
        // first built card is the container's own.
        assert_eq!(plan[0], Slot::Gap(8. * 60. + 7. * 8.));
        assert_eq!(&plan[1..], &[Slot::Item(8), Slot::Item(9)]);
        assert_eq!(position(&heights, 8., 8), Some(544.0..604.));
    }

    #[test]
    fn mixed_heights_window_by_their_real_positions() {
        // A caption (43), three rows (48), a caption, two rows.
        let heights = [43., 48., 48., 48., 43., 48., 48.].map(Some);
        assert_eq!(position(&heights, 0., 4), Some(187.0..230.));
        // The second caption (187..230) and the row after it (230..278).
        let plan = slots(&heights, 0., 190.0..240.);
        assert_eq!(
            plan,
            vec![
                Slot::Gap(187.),
                Slot::Item(4),
                Slot::Item(5),
                Slot::Gap(48.)
            ]
        );
        // A position is unknown until everything above it has been measured.
        let mut partly = heights.to_vec();
        partly[1] = None;
        assert_eq!(position(&partly, 0., 4), None);
        assert_eq!(position(&partly, 0., 0), Some(0.0..43.));
    }

    #[test]
    fn revealing_an_item_moves_the_view_as_little_as_possible() {
        // Above the view: its top comes to the top.
        assert_eq!(offset_to_show(100.0..140., 400., 300.), Some(100.));
        // Below: its bottom comes to the bottom.
        assert_eq!(offset_to_show(900.0..940., 400., 300.), Some(640.));
        // Already in view: nothing moves.
        assert_eq!(offset_to_show(450.0..490., 400., 300.), None);
    }

    #[test]
    fn a_readers_place_is_carried_across_everything_that_can_change_above_it() {
        // Ten rows of 40 px; the viewport top is 15 px into row 6.
        let ids: Vec<u64> = (100..110).collect();
        let (old, _) = tops(&[40.; 10], 0.);
        let at = 6. * 40. + 15.;
        // Nothing changed: it stays where it is.
        assert_eq!(carried(&ids, &old, &ids, &old, at), Some(at));
        // Rows appended at the end (the usual case while recording).
        let mut longer = ids.clone();
        longer.extend([200, 201]);
        let (new, _) = tops(&[40.; 12], 0.);
        assert_eq!(carried(&ids, &old, &longer, &new, at), Some(at));
        // Two rows inserted above the reader, 30 and 55 px tall (a network
        // gap filled in): the reader's row is 85 px further down, and so is
        // the offset.
        let mut filled = ids.clone();
        filled.splice(3..3, [300, 301]);
        let mut heights = vec![40.; 12];
        (heights[3], heights[4]) = (30., 55.);
        let (new, _) = tops(&heights, 0.);
        assert_eq!(carried(&ids, &old, &filled, &new, at), Some(at + 85.));
        // A row above turns out taller than its guess, one below shorter.
        let mut heights = vec![40.; 10];
        (heights[2], heights[8]) = (57.875, 22.);
        let (new, _) = tops(&heights, 0.);
        assert_eq!(carried(&ids, &old, &ids, &new, at), Some(at + 17.875));
        // A row above removed.
        let mut fewer = ids.clone();
        fewer.remove(1);
        let (new, _) = tops(&[40.; 9], 0.);
        assert_eq!(carried(&ids, &old, &fewer, &new, at), Some(at - 40.));
        // The reader's own row is gone, or other content replaced the list:
        // there is nothing to hold on to.
        let mut without = ids.clone();
        without.remove(6);
        assert_eq!(carried(&ids, &old, &without, &new, at), None);
        assert_eq!(carried(&ids, &old, &[1, 2, 3], &old[..3], at), None);
        // The viewport starts above the column: nothing above it can change.
        assert_eq!(carried(&ids, &old, &filled, &new, -20.), None);
        // Past the end of the column the last row is the anchor.
        assert_eq!(carried(&ids, &old, &ids, &old, 1000.), Some(1000.));
        // Gaps between rows are part of the positions.
        let (gapped, total) = tops(&[40.; 3], 8.);
        assert_eq!((gapped, total), (vec![0., 48., 96.], 136.));
        assert_eq!(tops(&[], 8.), (vec![], 0.));
    }

    #[test]
    fn an_item_is_found_again_wherever_it_moved_to() {
        let ids = [5, 6, 7, 8, 9];
        assert_eq!(locate(&ids, 7, 2), Some(2));
        assert_eq!(locate(&ids, 9, 2), Some(4));
        assert_eq!(locate(&ids, 5, 4), Some(0));
        assert_eq!(locate(&ids, 5, 40), Some(0));
        assert_eq!(locate(&ids, 4, 2), None);
        assert_eq!(locate(&[], 4, 0), None);
    }

    #[test]
    fn keys_change_with_anything_that_changes_an_items_height() {
        assert_eq!(key((3usize, "text", true)), key((3usize, "text", true)));
        assert_ne!(key((3usize, "text", true)), key((3usize, "text!", true)));
        assert_ne!(key((3usize, "text", true)), key((3usize, "text", false)));
    }
}
