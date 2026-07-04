//! Corrections engine — the product core of this fork (see NOTICE.md).
//!
//! No Tauri dependencies anywhere in this module. tokenizer.rs and apply.rs
//! are pure text (no storage, no I/O); store.rs is plain rusqlite. Pipeline
//! wiring and the E1 "fix last transcript" extraction build on top via
//! `managers::corrections`. Keep it Tauri-free — the eval CLI and property
//! tests depend on calling it without an app context.
//!
//!   ┌──────────────────────── corrections ────────────────────────┐
//!   │  tokenizer.rs   ONE shared tokenizer (extraction + apply)   │
//!   │  apply.rs       CorrectionSet: validate → index → apply     │
//!   │  store.rs       SQLite persistence (rusqlite only, no Tauri)│
//!   └──────────────────────────────────────────────────────────────┘
//!
//! Invariants (enforced by tests in this module):
//! - `apply(apply(x)) == apply(x)` for any built set (idempotency).
//! - If no pattern occurs in the text, `apply(x) == x` (identity).
//! - Non-matched spans are preserved byte-for-byte (punctuation, casing,
//!   whitespace, NFC/NFD form of untouched text).
//! - Exact token-level matching only — no fuzzy, no prompt injection (v1).

pub mod apply;
pub mod extract;
pub mod store;
pub mod tokenizer;

pub use apply::{
    validate_pair, AppliedCorrection, ApplyOutcome, BuildOutcome, CorrectionPair, CorrectionSet,
    PairError, RejectedPair, MAX_PATTERN_TOKENS,
};
pub use extract::{extract_corrections, ExtractionOutcome, RewriteGuard};
pub use store::{CorrectionSource, CorrectionsStore, StoreError, StoredCorrection};

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    /// Word pool mixing English, Spanish (with diacritics), and dev jargon —
    /// the realistic token space for bilingual developer dictation.
    const WORDS: &[&str] = &[
        "el", "la", "de", "que", "voy", "a", "push", "branch", "deploy", "the", "to", "está",
        "niño", "café", "señor", "más", "año", "PR", "commit", "open", "whisper", "cascad", "acme",
        "staging", "don't", "review",
    ];

    const PUNCT: &[&str] = &[" ", ", ", ". ", " ¿", "? ", " ¡", "! ", " — ", "\n", "  "];

    fn text_strategy() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            (
                proptest::sample::select(WORDS.to_vec()),
                proptest::sample::select(PUNCT.to_vec()),
            ),
            0..12,
        )
        .prop_map(|parts| {
            let mut s = String::new();
            for (w, p) in parts {
                s.push_str(w);
                s.push_str(p);
            }
            s
        })
    }

    fn pairs_strategy() -> impl Strategy<Value = Vec<CorrectionPair>> {
        proptest::collection::vec(
            (
                proptest::collection::vec(proptest::sample::select(WORDS.to_vec()), 1..3),
                proptest::collection::vec(proptest::sample::select(WORDS.to_vec()), 1..3),
                proptest::bool::ANY,
            ),
            0..6,
        )
        .prop_map(|raw| {
            raw.into_iter()
                .map(|(h, c, verbatim)| CorrectionPair {
                    heard: h.join(" "),
                    correct: c.join(" "),
                    verbatim,
                })
                .collect()
        })
    }

    proptest! {
        /// The core invariant: applying a built set twice equals applying once,
        /// for ANY pair soup — the build-time fixed-point validation must
        /// reject whatever would violate this.
        #[test]
        fn apply_is_idempotent(pairs in pairs_strategy(), text in text_strategy()) {
            let set = CorrectionSet::build(&pairs).set;
            let once = set.apply(&text).text;
            let twice = set.apply(&once).text;
            prop_assert_eq!(once, twice);
        }

        /// Identity: an empty set never alters text.
        #[test]
        fn empty_set_is_identity(text in text_strategy()) {
            let set = CorrectionSet::build(&[]).set;
            prop_assert_eq!(set.apply(&text).text, text);
        }

        /// Identity: if nothing matched (no applied records), text is unchanged.
        #[test]
        fn no_match_means_unchanged(pairs in pairs_strategy(), text in text_strategy()) {
            let set = CorrectionSet::build(&pairs).set;
            let outcome = set.apply(&text);
            if outcome.applied.is_empty() {
                prop_assert_eq!(outcome.text, text);
            }
        }

        /// Every reported applied range is within bounds and its heard_key
        /// equals the normalized tokens of the replaced span.
        #[test]
        fn applied_ranges_are_consistent(pairs in pairs_strategy(), text in text_strategy()) {
            let set = CorrectionSet::build(&pairs).set;
            let outcome = set.apply(&text);
            for a in &outcome.applied {
                prop_assert!(a.start < a.end && a.end <= text.len());
                let span = &text[a.start..a.end];
                prop_assert_eq!(tokenizer::normalized_tokens(span).join(" "), a.heard_key.clone());
            }
        }

        /// Build never loses pairs: every input pair is either active or rejected.
        #[test]
        fn build_accounts_for_every_pair(pairs in pairs_strategy()) {
            let outcome = CorrectionSet::build(&pairs);
            prop_assert_eq!(outcome.set.len() + outcome.rejected.len(), pairs.len());
        }

        /// Tokenizer ranges are ordered, non-overlapping, in-bounds, and on
        /// char boundaries.
        #[test]
        fn tokenizer_ranges_are_sound(text in text_strategy()) {
            let tokens = tokenizer::tokenize(&text);
            let mut prev_end = 0usize;
            for t in &tokens {
                prop_assert!(t.start >= prev_end);
                prop_assert!(t.end > t.start);
                prop_assert!(t.end <= text.len());
                prop_assert!(text.is_char_boundary(t.start) && text.is_char_boundary(t.end));
                prev_end = t.end;
            }
        }
    }
}
