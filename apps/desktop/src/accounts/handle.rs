//! Note ids in memory. A note of an account is known in memory by a handle:
//! the account's slot in the bits above the id its file gives it. Nothing
//! here defaults: an id that is not a handle is refused, in release builds
//! too, and a handle is never written to disk (a file that holds one is
//! refused loudly at the next load, because it is 2^40 or more).
//!
//! Local mode ("this Mac only") has no accounts and never calls these: its
//! ids are the ones in its file.

pub const FILE_ID_BITS: u32 = 40;
/// The highest id a file may give a note. `2^40 - 1` is never allocated.
pub const MAX_FILE_ID: u64 = (1 << FILE_ID_BITS) - 2;
pub const MAX_SLOT: u16 = 4095;
/// Chat: a conversation across meetings. Never tagged, never a note.
pub const NO_NOTE: u64 = 0;
/// The read-only page of a shared note. Never tagged, never a note.
pub const SHARED_NOTE_ID: u64 = u64::MAX;

/// The id note `file_id` of the account in `slot` has in memory.
pub fn handle(slot: u16, file_id: u64) -> Option<u64> {
    ((1..=MAX_SLOT).contains(&slot) && (1..=MAX_FILE_ID).contains(&file_id))
        .then(|| (u64::from(slot) << FILE_ID_BITS) | file_id)
}

thread_local! {
    static ENCODING_ACCOUNT_FILE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `encode` with note ids written as file ids: the encoding of an
/// account's file. Outside it (local mode's file) an id is written as it is,
/// so local mode's files stay byte for byte what they were.
pub fn encoding_account_file<T>(encode: impl FnOnce() -> T) -> T {
    let before = ENCODING_ACCOUNT_FILE.with(|flag| flag.replace(true));
    let result = encode();
    ENCODING_ACCOUNT_FILE.with(|flag| flag.set(before));
    result
}

/// How a note id is written to disk right now (see
/// [`encoding_account_file`]).
pub fn on_disk(id: u64) -> u64 {
    if ENCODING_ACCOUNT_FILE.with(|flag| flag.get()) {
        file_id(id)
    } else {
        id
    }
}

/// The slot and file id of a handle. `None` for 0, `u64::MAX`, any bit above
/// the slot set, slot 0, and a file id of 0 or above [`MAX_FILE_ID`].
pub fn split(id: u64) -> Option<(u16, u64)> {
    if id == NO_NOTE || id == SHARED_NOTE_ID || id >> (FILE_ID_BITS + 12) != 0 {
        return None;
    }
    let slot = (id >> FILE_ID_BITS) as u16;
    let file_id = id & ((1 << FILE_ID_BITS) - 1);
    ((1..=MAX_SLOT).contains(&slot) && (1..=MAX_FILE_ID).contains(&file_id))
        .then_some((slot, file_id))
}

/// The slot a handle belongs to.
pub fn slot_of(id: u64) -> Option<u16> {
    split(id).map(|(slot, _)| slot)
}

/// The id a note has in its file and in every name on disk: the file id of a
/// handle, or the id itself in local mode, where ids are not handles.
pub fn file_id(id: u64) -> u64 {
    split(id).map_or(id, |(_, file_id)| file_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2_handles_refuse_the_reserved_ids_bad_slots_and_any_high_bit() {
        assert_eq!(handle(1, 1), Some((1 << 40) | 1));
        assert_eq!(handle(MAX_SLOT, MAX_FILE_ID), Some((4095 << 40) | MAX_FILE_ID));
        assert_eq!(split((1 << 40) | 1), Some((1, 1)));
        assert_eq!(split((4095 << 40) | MAX_FILE_ID), Some((4095, MAX_FILE_ID)));
        // Slot 0, slot 4096, file id 0 and 2^40 - 1 are not handles.
        assert_eq!(handle(0, 1), None);
        assert_eq!(handle(4096, 1), None);
        assert_eq!(handle(1, 0), None);
        assert_eq!(handle(1, (1 << 40) - 1), None);
        assert_eq!(handle(1, 1 << 40), None);
        // The two reserved ids, a bare file id (slot 0), and a file id of 0.
        assert_eq!(split(NO_NOTE), None);
        assert_eq!(split(SHARED_NOTE_ID), None);
        assert_eq!(split(7), None);
        assert_eq!(split(1 << 40), None);
        assert_eq!(split((1 << 40) | ((1 << 40) - 1)), None);
        // Any bit above the slot.
        for bit in 52..64 {
            assert_eq!(split((1 << bit) | (1 << 40) | 1), None, "bit {bit}");
        }
        // Every handle round-trips and stays below 2^53, so it passes
        // through JSON to the native side exactly.
        for (slot, file) in [(1, 1), (2, 5), (4095, MAX_FILE_ID), (17, 1 << 39)] {
            let id = handle(slot, file).unwrap();
            assert_eq!(split(id), Some((slot, file)));
            assert!(id < 1 << 53);
            assert_eq!(file_id(id), file);
            assert_eq!(slot_of(id), Some(slot));
        }
        // Local mode ids are not handles and are their own file ids.
        assert_eq!(file_id(7), 7);
    }
}
