//! Who wrote each block of a note's generated notes: the model, or the
//! person reading them. A block (a heading line, a bullet at any depth, a
//! paragraph) the person typed or changed is drawn in the full ink; one that
//! still reads as it was generated stays in the softer grey, and goes back to
//! it when an edit is undone or typed back.
//!
//! Nothing is recorded as it is typed. What is kept with the note is the
//! generated original (one hash per block) and a fingerprint of the notes as
//! they were last edited here; which blocks are the person's is then worked
//! out by comparing, so undo and redo need nothing of their own, and the
//! answer is the same after the app is opened again.
//!
//! The fingerprint is what ties the original to the text. Notes replaced by
//! anything but the editor here (generated again, changed on another device)
//! no longer match it: every block is then the model's as far as this device
//! knows, and the next edit starts from that text as the new original.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The generated original of a note's notes that have been edited here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Authorship {
    /// A hash of each block of the notes as they were generated, in order.
    pub generated: Vec<u64>,
    /// The fingerprint of the notes as they were last edited here.
    pub edited: u64,
}

/// FNV-1a: the same on every launch and every build, which the standard
/// hasher does not promise and a value kept in a file needs.
fn hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Each block of `source` as (its row, the hash of its text). Blank lines
/// and rules are not blocks; a block's marker and indentation are not its
/// text, so nesting a bullet deeper does not make it someone else's.
fn blocks(source: &str) -> Vec<(usize, u64)> {
    source
        .lines()
        .enumerate()
        .filter_map(|(row, line)| {
            let text = crate::summary::block_text(line);
            (!text.is_empty()).then(|| (row, hash(text.bytes())))
        })
        .collect()
}

/// One number for a whole sequence of blocks.
fn fingerprint(blocks: impl IntoIterator<Item = u64>) -> u64 {
    hash(blocks.into_iter().flat_map(u64::to_le_bytes))
}

/// The members of `current` that are not the model's: those left over once
/// the blocks both sides share, in order, are paired off. The unchanged
/// start and end are skipped first, so an edit costs what it changed and not
/// the length of the notes; a middle too large to pair exactly (far more
/// than one edit leaves) is paired by count instead.
fn unpaired(generated: &[u64], current: &[u64]) -> Vec<usize> {
    let start = generated
        .iter()
        .zip(current)
        .take_while(|(a, b)| a == b)
        .count();
    let end = generated[start..]
        .iter()
        .rev()
        .zip(current[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let was = &generated[start..generated.len() - end];
    let now = &current[start..current.len() - end];
    if was.is_empty() || now.is_empty() {
        return (start..start + now.len()).collect();
    }
    let mut left = Vec::new();
    if was.len().saturating_mul(now.len()) <= 1 << 18 {
        // The longest common subsequence, by the usual table.
        let width = now.len() + 1;
        let mut table = vec![0u32; (was.len() + 1) * width];
        for i in (0..was.len()).rev() {
            for j in (0..now.len()).rev() {
                table[i * width + j] = if was[i] == now[j] {
                    table[(i + 1) * width + j + 1] + 1
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while j < now.len() {
            if i < was.len() && was[i] == now[j] {
                i += 1;
                j += 1;
            } else if i < was.len() && table[(i + 1) * width + j] >= table[i * width + j + 1] {
                i += 1;
            } else {
                left.push(start + j);
                j += 1;
            }
        }
    } else {
        let mut counts: HashMap<u64, u32> = HashMap::new();
        for block in was {
            *counts.entry(*block).or_default() += 1;
        }
        for (j, block) in now.iter().enumerate() {
            match counts.get_mut(block) {
                Some(count) if *count > 0 => *count -= 1,
                _ => left.push(start + j),
            }
        }
    }
    left
}

impl Authorship {
    /// The original of notes nobody has edited yet.
    pub fn of_generated(source: &str) -> Self {
        let generated: Vec<u64> = blocks(source).into_iter().map(|(_, block)| block).collect();
        let edited = fingerprint(generated.iter().copied());
        Self { generated, edited }
    }

    /// The rows of `source` whose blocks the person wrote: typed, changed,
    /// or added. Empty when `source` is not the text this original was last
    /// edited into (see the module documentation).
    pub fn written(&self, source: &str) -> HashSet<usize> {
        let blocks = blocks(source);
        let current: Vec<u64> = blocks.iter().map(|(_, block)| *block).collect();
        if fingerprint(current.iter().copied()) != self.edited {
            return HashSet::new();
        }
        unpaired(&self.generated, &current)
            .into_iter()
            .map(|index| blocks[index].0)
            .collect()
    }
}

/// The notes went from `before` to `after` in the editor here. `kept` is
/// what the note has stored: it keeps its original while it describes
/// `before`, starts from `before` as the original when it does not (or there
/// is none), and is dropped again once the notes read exactly as generated,
/// so notes nobody has changed store nothing.
pub fn edited(kept: &mut Option<Authorship>, before: &str, after: &str) {
    let now = fingerprint(blocks(after).into_iter().map(|(_, block)| block));
    let mut authorship = kept
        .take()
        .filter(|kept| kept.edited == fingerprint(blocks(before).into_iter().map(|(_, block)| block)))
        .unwrap_or_else(|| Authorship::of_generated(before));
    authorship.edited = now;
    *kept = (fingerprint(authorship.generated.iter().copied()) != now).then_some(authorship);
}

/// Whether the block that began on `row` when its editing started is the
/// person's, given the rows that are (`written`, of the notes as they are
/// now) and how many lines the block has since gained: a block being edited
/// may have been typed into several lines, and any of them makes it theirs.
pub fn block_written(written: &HashSet<usize>, row: usize, gained_lines: usize) -> bool {
    written
        .iter()
        .any(|written| *written >= row && *written - row <= gained_lines)
}

#[cfg(test)]
mod tests {
    use super::{Authorship, block_written, edited, unpaired};
    use std::collections::HashSet;

    const GENERATED: &str = "# Decisions\n\n- Ship Friday\n  - Alex owns mobile\n    - Test on device\n- TBD\n\n---\n\n# Next steps\n- TBD\n";

    fn rows(kept: &Option<Authorship>, source: &str) -> Vec<usize> {
        let mut rows: Vec<usize> = kept
            .as_ref()
            .map(|kept| kept.written(source))
            .unwrap_or_default()
            .into_iter()
            .collect();
        rows.sort();
        rows
    }

    #[test]
    fn typing_makes_a_block_the_persons_and_typing_it_back_or_undoing_returns_it() {
        let mut kept = None;
        // Type "f" at the end of a bullet.
        let typed = GENERATED.replace("- Ship Friday", "- Ship Fridayf");
        edited(&mut kept, GENERATED, &typed);
        assert_eq!(rows(&kept, &typed), [2]);
        // Another letter: still that block only, against the same original.
        let more = GENERATED.replace("- Ship Friday", "- Ship Fridayff");
        edited(&mut kept, &typed, &more);
        assert_eq!(rows(&kept, &more), [2]);
        // Undo one step, then the other (or backspace twice): generated again.
        edited(&mut kept, &more, &typed);
        assert_eq!(rows(&kept, &typed), [2]);
        edited(&mut kept, &typed, GENERATED);
        assert_eq!(rows(&kept, GENERATED), [] as [usize; 0]);
        // Notes that read as generated keep nothing.
        assert_eq!(kept, None);
        // Redo: the same answer as the first time.
        edited(&mut kept, GENERATED, &typed);
        assert_eq!(rows(&kept, &typed), [2]);
    }

    #[test]
    fn headings_and_nested_bullets_are_blocks_of_their_own() {
        let mut kept = None;
        let heading = GENERATED.replace("# Decisions", "# Decisions made");
        edited(&mut kept, GENERATED, &heading);
        assert_eq!(rows(&kept, &heading), [0]);
        // A sub-sub-bullet edited as well: its parents stay the model's.
        let nested = heading.replace("    - Test on device", "    - Test on two devices");
        edited(&mut kept, &heading, &nested);
        assert_eq!(rows(&kept, &nested), [0, 4]);
        // The heading typed back: only the nested bullet is left.
        let back = nested.replace("# Decisions made", "# Decisions");
        edited(&mut kept, &nested, &back);
        assert_eq!(rows(&kept, &back), [4]);
    }

    #[test]
    fn a_block_is_itself_and_not_another_that_reads_the_same() {
        // Two bullets read "TBD". Changing the second leaves the first alone,
        let mut kept = None;
        let second = "# Decisions\n\n- Ship Friday\n  - Alex owns mobile\n    - Test on device\n- TBD\n\n---\n\n# Next steps\n- Maya to confirm\n";
        edited(&mut kept, GENERATED, second);
        assert_eq!(rows(&kept, second), [10]);
        // and a bullet retyped as a copy of another is still the person's.
        let copy = GENERATED.replace("- Ship Friday", "- TBD");
        let mut kept = None;
        edited(&mut kept, GENERATED, &copy);
        assert_eq!(rows(&kept, &copy), [2]);
    }

    #[test]
    fn lines_added_inside_a_block_are_the_persons_and_shift_nothing_else() {
        let mut kept = None;
        let longer = GENERATED.replace("- Ship Friday", "- Ship Friday\nand tell the team\nby noon");
        edited(&mut kept, GENERATED, &longer);
        // The bullet's first line is as generated; the two typed under it are not.
        assert_eq!(rows(&kept, &longer), [3, 4]);
        let written = kept.as_ref().unwrap().written(&longer);
        // While it is being edited the block is one field: theirs as a whole.
        assert!(block_written(&written, 2, 2));
        assert!(!block_written(&written, 2, 0));
        assert!(!block_written(&written, 5, 0));
        assert!(!block_written(&HashSet::new(), 0, usize::MAX));
        // Emptied again.
        edited(&mut kept, &longer, GENERATED);
        assert_eq!(kept, None);
    }

    #[test]
    fn a_removed_block_coming_back_is_still_the_models() {
        let mut kept = None;
        let removed = GENERATED.replace("  - Alex owns mobile\n", "\n");
        edited(&mut kept, GENERATED, &removed);
        // Nothing left is the person's, but the original is kept: the notes differ.
        assert_eq!(rows(&kept, &removed), [] as [usize; 0]);
        assert!(kept.is_some());
        edited(&mut kept, &removed, GENERATED);
        assert_eq!(rows(&kept, GENERATED), [] as [usize; 0]);
        assert_eq!(kept, None);
    }

    #[test]
    fn notes_replaced_from_elsewhere_are_the_models_until_they_are_edited_again() {
        let mut kept = None;
        let typed = GENERATED.replace("- Ship Friday", "- Ship Monday");
        edited(&mut kept, GENERATED, &typed);
        assert_eq!(rows(&kept, &typed), [2]);
        // Generated again (or changed on another device): not this text any more.
        let regenerated = "# Summary\n- Ship Monday\n- Budget approved\n";
        assert_eq!(rows(&kept, regenerated), [] as [usize; 0]);
        // The next edit starts from what is there now.
        let again = regenerated.replace("- Budget approved", "- Budget approved in full");
        edited(&mut kept, regenerated, &again);
        assert_eq!(rows(&kept, &again), [2]);
        // Whitespace the server tidies is not a change of text.
        let tidied = format!("{}\n\n", again.trim_end().replace("- Ship Monday", "-   Ship Monday  "));
        assert_eq!(rows(&kept, &tidied), [2]);
    }

    #[test]
    fn what_is_kept_survives_being_saved_and_read_back() {
        let mut kept = None;
        let typed = GENERATED.replace("- TBD\n\n---", "- Zoë decides 🦊\n\n---");
        edited(&mut kept, GENERATED, &typed);
        let saved = serde_json::to_string(&kept).unwrap();
        let read: Option<Authorship> = serde_json::from_str(&saved).unwrap();
        assert_eq!(read, kept);
        assert_eq!(rows(&read, &typed), [5]);
        // An older file has no such field.
        let none: Option<Authorship> = serde_json::from_str("null").unwrap();
        assert_eq!(rows(&none, &typed), [] as [usize; 0]);
    }

    #[test]
    fn pairing_is_exact_for_an_edit_and_by_count_for_a_rewrite() {
        assert_eq!(unpaired(&[1, 2, 3], &[1, 2, 3]), [] as [usize; 0]);
        assert_eq!(unpaired(&[1, 2, 3], &[1, 9, 3]), [1]);
        assert_eq!(unpaired(&[1, 2, 3], &[1, 2, 8, 9, 3]), [2, 3]);
        assert_eq!(unpaired(&[1, 2, 3], &[1, 3]), [] as [usize; 0]);
        assert_eq!(unpaired(&[], &[4, 5]), [0, 1]);
        assert_eq!(unpaired(&[4, 5], &[]), [] as [usize; 0]);
        // The first of two equal blocks changed into the second's text.
        assert_eq!(unpaired(&[7, 1, 2, 7], &[7, 2, 2, 7]), [1]);
        // Past the exact table's size: by count, which still finds what is new.
        let was: Vec<u64> = (0..600).collect();
        let mut now: Vec<u64> = (0..600).rev().collect();
        now[10] = 9_999;
        assert_eq!(unpaired(&was, &now), [10]);
    }
}
