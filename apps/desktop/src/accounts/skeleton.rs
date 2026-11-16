//! Telling look-alike names apart. `skeleton(s)` is NFKD, default-ignorable
//! code points and combining marks removed, mapped through the Unicode
//! confusables table (UTS 39), lower-cased. Two names with the same
//! skeleton look the same to a person, whatever they are made of.
use unicode_normalization::UnicodeNormalization;

/// A code point that renders as nothing (zero-width characters, joiners,
/// direction marks, variation selectors, tags, the soft hyphen).
fn default_ignorable(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD
            | 0x034F
            | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFF8
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

pub fn skeleton(text: &str) -> String {
    use unicode_normalization::char::is_combining_mark;
    use unicode_security::confusable_detection::skeleton as confusables;
    let visible = |c: &char| !default_ignorable(*c) && !is_combining_mark(*c);
    let decomposed: String = text.nfkd().filter(visible).collect();
    // The table maps to prototypes that may themselves decompose or differ
    // in case, so the same steps run on its output. Lower-casing comes
    // last, and what it produces is mapped once more (an upper-case
    // look-alike must meet the lower-case letter it imitates).
    let mapped: String = confusables(&decomposed).collect::<String>().nfkd().filter(visible).collect();
    let lowered = mapped.to_lowercase();
    confusables(&lowered)
        .collect::<String>()
        .nfkd()
        .filter(visible)
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A label a person types for an account: one line, at most 24 characters,
/// and not something that looks like another account's label or host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelError {
    Empty,
    TooLong,
    /// It looks like this other label or host.
    Confusable(String),
}

pub fn check_label(label: &str, others: &[String]) -> Result<String, LabelError> {
    let label = label.trim();
    if label.is_empty() {
        return Err(LabelError::Empty);
    }
    if label.chars().count() > 24 || label.chars().any(|c| c.is_control()) {
        return Err(LabelError::TooLong);
    }
    let wanted = skeleton(label);
    match others.iter().find(|other| skeleton(other) == wanted) {
        Some(other) => Err(LabelError::Confusable(other.clone())),
        None => Ok(label.to_owned()),
    }
}

/// What the sheet shown before a sign-in to a host says about it, beside
/// the hosts of the accounts already here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSheet {
    /// The new host, in full. Never shortened.
    pub host: String,
    pub existing: Vec<String>,
    /// The existing host the new one looks like, or contains, or is
    /// contained in. Then the sheet warns and its default button is Cancel.
    pub resembles: Option<String>,
}

impl HostSheet {
    pub fn default_is_cancel(&self) -> bool {
        self.resembles.is_some()
    }
}

/// Compare a host about to be signed in to with the hosts of existing
/// accounts. An address read from a plain file on disk is shown here, never
/// trusted.
pub fn host_sheet(host: &str, existing: &[String]) -> HostSheet {
    let new = skeleton(host);
    let resembles = existing
        .iter()
        .find(|other| {
            let old = skeleton(other);
            // The same host is not a look-alike of itself.
            other.as_str() != host && (old == new || old.contains(&new) || new.contains(&old))
        })
        .cloned();
    HostSheet {
        host: host.to_owned(),
        existing: existing.to_vec(),
        resembles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l1_look_alikes_of_a_label_or_host_have_its_skeleton_and_are_refused() {
        // (the look-alike, what it imitates)
        let vectors = [
            // Zero-width characters and joiners.
            ("no\u{200B}tes.acme.com", "notes.acme.com"),
            ("notes\u{200D}.acme.com", "notes.acme.com"),
            ("\u{FEFF}Personal", "Personal"),
            ("Per\u{00AD}sonal", "Personal"),
            // A trailing no-break space, doubled and odd white space.
            ("Personal\u{00A0}", "Personal"),
            ("Acme\u{2003}Notes", "Acme Notes"),
            ("Acme  Notes", "Acme Notes"),
            // Cyrillic look-alikes: а с е о р.
            ("notes.\u{0430}\u{0441}me.com", "notes.acme.com"),
            ("n\u{043E}tes.acme.com", "notes.acme.com"),
            ("\u{0420}ersonal", "Personal"),
            ("Pers\u{043E}nal", "Personal"),
            // Greek look-alikes: ο ν Α.
            ("n\u{03BF}tes.acme.com", "notes.acme.com"),
            ("\u{0391}cme", "Acme"),
            // Full-width and other compatibility forms.
            ("\u{FF21}cme", "Acme"),
            ("ｎｏｔｅｓ.acme.com", "notes.acme.com"),
            // Combining marks and case.
            ("Acme\u{0301}", "Acme"),
            ("ACME", "acme"),
            ("PERSONAL", "Personal"),
        ];
        for (imitation, original) in vectors {
            assert_eq!(skeleton(imitation), skeleton(original), "{imitation:?} against {original:?}");
            assert_eq!(
                check_label(imitation, &[original.to_owned()]),
                if imitation.trim().chars().count() > 24 {
                    Err(LabelError::TooLong)
                } else {
                    Err(LabelError::Confusable(original.to_owned()))
                },
                "{imitation:?}"
            );
        }
        // Names that only resemble each other in meaning are different.
        for (a, b) in [("Personal", "Private"), ("notes.acme.com", "notes.acme.co"), ("Acme", "Acme 2")] {
            assert_ne!(skeleton(a), skeleton(b), "{a} / {b}");
            assert!(check_label(a, &[b.to_owned()]).is_ok());
        }
        // A label is one line of at most 24 characters.
        assert_eq!(check_label("  ", &[]), Err(LabelError::Empty));
        assert_eq!(check_label(&"a".repeat(25), &[]), Err(LabelError::TooLong));
        assert_eq!(check_label("two\nlines", &[]), Err(LabelError::TooLong));
        assert_eq!(check_label(" Work ", &[]), Ok("Work".into()));
    }

    #[test]
    fn k4_a_host_that_looks_like_an_existing_one_warns_and_defaults_to_cancel() {
        let existing = vec!["notes.acme.com".to_owned(), "cereal.zlt.dev".to_owned()];
        // The same letters from another alphabet.
        let sheet = host_sheet("notes.\u{0430}cme.com", &existing);
        assert_eq!(sheet.resembles.as_deref(), Some("notes.acme.com"));
        assert!(sheet.default_is_cancel());
        assert_eq!(sheet.host, "notes.\u{0430}cme.com", "shown in full, as it is");
        // One contains the other.
        let longer = host_sheet("notes.acme.com.evil.io", &existing);
        assert_eq!(longer.resembles.as_deref(), Some("notes.acme.com"));
        assert!(longer.default_is_cancel());
        let shorter = host_sheet("zlt.dev", &existing);
        assert_eq!(shorter.resembles.as_deref(), Some("cereal.zlt.dev"));
        // An unrelated host, and the very same host, do not warn.
        assert!(!host_sheet("old.example.com", &existing).default_is_cancel());
        assert!(!host_sheet("notes.acme.com", &existing).default_is_cancel());
        assert_eq!(host_sheet("old.example.com", &existing).existing, existing);
    }
}
