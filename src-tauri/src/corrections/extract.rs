//! E1 "fix last transcript" extraction — turn a single human edit into
//! candidate correction pairs.
//!
//! PURITY: no Tauri imports. Uses the ONE shared tokenizer
//! (`corrections::tokenizer`) — the same one the apply pass uses — so a pair
//! learned here tokenizes identically to how it will later be matched. Never
//! introduce a second tokenizer.
//!
//!   shown_text  ─┐                    diff (token-level LCS on normalized
//!   edited_text ─┤─▶ tokenize both ─▶ tokens) ─▶ changed hunks (merged) ─▶
//!                                     guards ─▶ candidate CorrectionPairs
//!
//! This module only PROPOSES candidates. It never validates idempotency or
//! calls `validate_pair` — `CorrectionSet::build` (downstream) does that. The
//! guards here shape WHAT we offer to learn (a targeted single fix, not a
//! rewrite):
//!   a. zero-diff canary  — identical normalized tokens → learn nothing.
//!   b. rewrite guard     — >50% of shown tokens changed → discard the whole
//!                          fix (it's a rewrite, not a term correction).
//!   c. insert/delete-only — a hunk with an empty side yields no pair.
//!   d. max 4 tokens/span — a heard span longer than 4 tokens is dropped.
//!   e. stoplist          — a heard span whose tokens are ALL common function
//!                          words is dropped (e.g. "de"→"the").
//!   f. max 3 pairs/fix    — keep the first 3 in document order.

use super::apply::CorrectionPair;
use super::tokenizer::{normalize, tokenize, Token};

/// Max tokens allowed on the `heard` side of a candidate span (E1 guard d).
/// Stricter than the apply-side `MAX_PATTERN_TOKENS` (8) — a one-tap fix is a
/// short term correction, not a phrase rewrite.
const MAX_HEARD_TOKENS: usize = 4;

/// Max candidate pairs proposed from a single fix (E1 guard f). If more
/// survive, the first 3 in document order (by shown-text position) are kept.
const MAX_PAIRS_PER_FIX: usize = 3;

/// If more than this fraction of the shown tokens changed, the edit is a
/// rewrite, not a set of term corrections (E1 guard b). Discard everything.
const REWRITE_FRACTION: f64 = 0.5;

/// Why a whole fix was discarded (surfaced to the user as an amber note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteGuard {
    /// More than 50% of the shown tokens changed — treated as a rewrite.
    TooMuchChanged,
}

/// The result of extracting candidate corrections from one edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionOutcome {
    /// Candidate pairs to learn, all `verbatim: true` (matches `add_learned`).
    pub pairs: Vec<CorrectionPair>,
    /// `Some` when the whole fix was rejected by a guard (currently only the
    /// rewrite guard); `None` when extraction succeeded (possibly with zero
    /// pairs, e.g. a pure-casing or zero-diff edit).
    pub rejected_reason: Option<RewriteGuard>,
}

/// One contiguous changed region of the diff: the shown-side token index
/// range `[shown_start, shown_end)` and the edited-side range
/// `[edited_start, edited_end)` (both half-open, in token indices). Either
/// side may be empty (pure insertion or deletion).
#[derive(Debug, Clone, Copy)]
struct Hunk {
    shown_start: usize,
    shown_end: usize,
    edited_start: usize,
    edited_end: usize,
}

impl Hunk {
    fn shown_len(&self) -> usize {
        self.shown_end - self.shown_start
    }
    fn edited_len(&self) -> usize {
        self.edited_end - self.edited_start
    }
}

/// Extract candidate correction pairs from a single human edit of the shown
/// transcript. Returns proposals only — validation happens downstream.
pub fn extract_corrections(shown_text: &str, edited_text: &str) -> ExtractionOutcome {
    let shown_tokens = tokenize(shown_text);
    let edited_tokens = tokenize(edited_text);

    let shown_norm: Vec<String> = shown_tokens
        .iter()
        .map(|t| normalize(t.text(shown_text)))
        .collect();
    let edited_norm: Vec<String> = edited_tokens
        .iter()
        .map(|t| normalize(t.text(edited_text)))
        .collect();

    // Guard a (part 1): identical normalized token sequences → nothing to
    // learn. This also covers pure-casing edits (normalize folds case) and
    // trailing whitespace/punctuation that tokenizes away.
    if shown_norm == edited_norm {
        return ExtractionOutcome {
            pairs: Vec::new(),
            rejected_reason: None,
        };
    }

    // Token-level LCS diff → merged changed hunks.
    let hunks = diff_hunks(&shown_norm, &edited_norm);

    // Guard a (part 2): the LCS found no changed hunks (defensive — the
    // sequence-equality check above already handles the exact case, but a
    // diff quirk must never invent phantom pairs).
    if hunks.is_empty() {
        return ExtractionOutcome {
            pairs: Vec::new(),
            rejected_reason: None,
        };
    }

    // Guard b: rewrite guard. Sum shown-side tokens across ALL changed hunks
    // as a fraction of total shown tokens. If > 50%, discard the whole fix.
    let total_shown = shown_norm.len();
    let changed_shown: usize = hunks.iter().map(|h| h.shown_len()).sum();
    if total_shown > 0 && (changed_shown as f64) > REWRITE_FRACTION * (total_shown as f64) {
        return ExtractionOutcome {
            pairs: Vec::new(),
            rejected_reason: Some(RewriteGuard::TooMuchChanged),
        };
    }

    // Per-hunk guards (c, d, e), then trim to 3 (f).
    let mut pairs: Vec<CorrectionPair> = Vec::new();
    for h in &hunks {
        // Guard c: insert-only or delete-only → no pair (need non-empty heard
        // AND non-empty correct).
        if h.shown_len() == 0 || h.edited_len() == 0 {
            continue;
        }
        // Guard d: heard span longer than 4 tokens is dropped.
        if h.shown_len() > MAX_HEARD_TOKENS {
            continue;
        }
        // Guard e: stoplist — drop if EVERY heard token is a stopword.
        let heard_norm = &shown_norm[h.shown_start..h.shown_end];
        if heard_norm.iter().all(|t| is_stopword(t)) {
            continue;
        }

        // Original (un-normalized) byte spans for heard/correct.
        let heard = span_text(shown_text, &shown_tokens, h.shown_start, h.shown_end);
        let correct = span_text(edited_text, &edited_tokens, h.edited_start, h.edited_end);

        pairs.push(CorrectionPair {
            heard,
            correct,
            // Learned pairs are verbatim (matches `add_learned`'s convention).
            verbatim: true,
        });
    }

    // Guard f: keep the first 3 in document order (hunks are already in
    // shown-text position order). Tie-break: earliest shown position wins.
    pairs.truncate(MAX_PAIRS_PER_FIX);

    ExtractionOutcome {
        pairs,
        rejected_reason: None,
    }
}

/// Original-casing substring spanning tokens `[start, end)` of `source`
/// (byte slice from the first token's start to the last token's end).
fn span_text(source: &str, tokens: &[Token], start: usize, end: usize) -> String {
    debug_assert!(start < end && end <= tokens.len());
    let byte_start = tokens[start].start;
    let byte_end = tokens[end - 1].end;
    source[byte_start..byte_end].to_string()
}

/// Token-level diff via LCS, producing merged changed hunks. Adjacent changed
/// regions with no matched (unchanged) token between them are merged into one
/// hunk (guard-merge requirement) — this falls out naturally from walking the
/// LCS backtrace and coalescing consecutive non-match steps.
fn diff_hunks(a: &[String], b: &[String]) -> Vec<Hunk> {
    let n = a.len();
    let m = b.len();

    // LCS length table. `lcs[i][j]` = LCS length of a[i..] and b[j..].
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    // Backtrace: classify each step as Match (advance both) or a change
    // (advance one side). Collect the raw sequence of matched positions so we
    // can carve hunks between them.
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    // Start of the current pending change region on each side.
    let mut ci = 0usize;
    let mut cj = 0usize;
    let mut in_change = false;

    let flush = |hunks: &mut Vec<Hunk>, si: usize, ei: usize, sj: usize, ej: usize| {
        if ei > si || ej > sj {
            hunks.push(Hunk {
                shown_start: si,
                shown_end: ei,
                edited_start: sj,
                edited_end: ej,
            });
        }
    };

    while i < n && j < m {
        if a[i] == b[j] {
            // Matched token: close any pending change region first.
            if in_change {
                flush(&mut hunks, ci, i, cj, j);
                in_change = false;
            }
            i += 1;
            j += 1;
        } else {
            if !in_change {
                ci = i;
                cj = j;
                in_change = true;
            }
            // Prefer the direction that keeps the longer remaining LCS —
            // standard LCS backtrace, coalescing consecutive change steps
            // into one hunk (adjacent changes with no match between merge).
            if lcs[i + 1][j] >= lcs[i][j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    // Tail: whatever remains on either side is one trailing change region,
    // merged with any pending region.
    if !in_change && (i < n || j < m) {
        ci = i;
        cj = j;
        in_change = true;
    }
    if in_change {
        flush(&mut hunks, ci, n, cj, m);
    }

    hunks
}

/// Common English + Spanish function words. A candidate heard span is dropped
/// only when EVERY one of its normalized tokens is in this set — so a fix on a
/// content word survives even when it sits among stopwords (e.g. the heard
/// span "the cascad" keeps its pair because "cascad" is not a stopword).
/// All entries are lowercase (compared against `normalize`d tokens).
const STOPLIST: &[&str] = &[
    // ── English (~100 common function words) ──────────────────────────────
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "am", "of", "to", "in",
    "on", "at", "for", "with", "by", "from", "this", "that", "these", "those", "it", "its", "he",
    "she", "they", "them", "we", "us", "you", "your", "i", "me", "my", "and", "or", "but", "nor",
    "not", "no", "yes", "do", "does", "did", "done", "have", "has", "had", "will", "would",
    "shall", "should", "can", "could", "may", "might", "must", "what", "when", "where", "why",
    "how", "who", "whom", "whose", "which", "there", "here", "all", "some", "any", "each", "every",
    "other", "another", "more", "most", "such", "only", "own", "same", "so", "very", "just",
    "also", "than", "then", "too", "because", "if", "about", "into", "onto", "over", "under",
    "again", "further", "once", "up", "down", "out", "off", "above", "below", "between", "through",
    "during", "before", "after", "while", "as", "an", "his", "her", "our", "their",
    // ── Spanish (~100 common function words) ──────────────────────────────
    "el", "la", "los", "las", "un", "una", "unos", "unas", "de", "del", "que", "y", "e", "en",
    "con", "por", "para", "es", "son", "fue", "fueron", "era", "eran", "ser", "estar", "está",
    "están", "se", "su", "sus", "lo", "le", "les", "te", "me", "nos", "os", "mi", "mis", "tu",
    "tus", "este", "esta", "estos", "estas", "esto", "ese", "esa", "esos", "esas", "eso", "aquel",
    "aquella", "pero", "o", "u", "si", "sí", "como", "cuando", "donde", "dónde", "porque", "muy",
    "más", "menos", "mucho", "poco", "todo", "toda", "todos", "todas", "nada", "algo", "alguien",
    "nadie", "también", "tampoco", "entonces", "pues", "aunque", "desde", "hasta", "sobre",
    "entre", "sin", "hacia", "durante", "mientras", "ya", "aún", "todavía", "yo", "él", "ella",
    "ellos", "ellas", "nosotros", "vosotros", "usted", "ustedes", "al", "ni", "cada", "otro",
    "otra", "mismo",
];

/// True if `token` (already normalized) is a common function word.
fn is_stopword(token: &str) -> bool {
    STOPLIST.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corrections::CorrectionSet;

    fn pairs(shown: &str, edited: &str) -> Vec<(String, String)> {
        extract_corrections(shown, edited)
            .pairs
            .into_iter()
            .map(|p| (p.heard, p.correct))
            .collect()
    }

    #[test]
    fn single_word_fix_yields_one_pair() {
        let got = pairs("deploy to cascad now", "deploy to Cascade now");
        assert_eq!(got, vec![("cascad".to_string(), "Cascade".to_string())]);
    }

    #[test]
    fn two_adjacent_word_fixes_merge_into_one_pair() {
        // "open whisper" → "OpenWhisper Two" : both changed tokens are
        // adjacent with no matched token between them → one merged hunk.
        let got = pairs("run open whisper today", "run OpenWhisper Two today");
        assert_eq!(
            got,
            vec![("open whisper".to_string(), "OpenWhisper Two".to_string())]
        );
    }

    #[test]
    fn heard_span_over_four_tokens_is_dropped() {
        // 5 contiguous shown tokens change → span exceeds 4 → dropped. Anchor
        // with plenty of unchanged tokens on both sides so the rewrite guard
        // (50% of total shown) does not fire first — the per-hunk 4-token
        // guard is what must drop this span.
        let shown = "keep a b c d e f keep p q r s t u v w x y z";
        let edited = "keep V W X Y Z f keep p q r s t u v w x y z";
        // a b c d e (5 tokens) change; total shown = 19, changed = 5 → 26%.
        let outcome = extract_corrections(shown, edited);
        assert_eq!(
            outcome.rejected_reason, None,
            "should not trip the rewrite guard"
        );
        assert!(
            outcome.pairs.is_empty(),
            "5-token heard span must be dropped, got {:?}",
            outcome.pairs
        );
    }

    #[test]
    fn more_than_three_pairs_trimmed_to_first_three_in_order() {
        // Four well-separated single-word fixes; anchors keep total shown
        // large so the rewrite guard does not fire (4 changed / 12 = 33%).
        let shown = "alpha zero beta zero gamma zero delta zero omega ok done";
        let edited = "alpha ONE beta TWO gamma THREE delta FOUR omega ok done";
        let got = pairs(shown, edited);
        assert_eq!(got.len(), 3, "trimmed to 3, got {got:?}");
        // First three in document order.
        assert_eq!(
            got,
            vec![
                ("zero".to_string(), "ONE".to_string()),
                ("zero".to_string(), "TWO".to_string()),
                ("zero".to_string(), "THREE".to_string()),
            ]
        );
    }

    #[test]
    fn rewrite_guard_fires_over_fifty_percent() {
        // 3 of 4 shown tokens change (real content changes, not just casing,
        // which normalize would fold to zero-diff) → 75% > 50% → discard.
        let outcome = extract_corrections("alpha beta gamma delta", "one two three delta");
        assert!(outcome.pairs.is_empty());
        assert_eq!(outcome.rejected_reason, Some(RewriteGuard::TooMuchChanged));
    }

    #[test]
    fn insert_only_hunk_is_dropped() {
        // Edited adds a token, no shown token changed → insertion-only.
        let outcome = extract_corrections("deploy to cascade", "deploy now to cascade");
        assert!(
            outcome.pairs.is_empty(),
            "insert-only must not produce a pair, got {:?}",
            outcome.pairs
        );
        assert_eq!(outcome.rejected_reason, None);
    }

    #[test]
    fn delete_only_hunk_is_dropped() {
        // Edited removes a token, no substitution → deletion-only.
        let outcome = extract_corrections("deploy now to cascade", "deploy to cascade");
        assert!(
            outcome.pairs.is_empty(),
            "delete-only must not produce a pair, got {:?}",
            outcome.pairs
        );
        assert_eq!(outcome.rejected_reason, None);
    }

    #[test]
    fn zero_diff_identical_yields_nothing() {
        let outcome = extract_corrections("nothing to fix here", "nothing to fix here");
        assert!(outcome.pairs.is_empty());
        assert_eq!(outcome.rejected_reason, None);
    }

    #[test]
    fn zero_diff_trailing_punctuation_and_whitespace_tokenize_identically() {
        // Trailing period/spaces do not change the token sequence.
        let outcome = extract_corrections("nothing to fix", "nothing to fix.  ");
        assert!(outcome.pairs.is_empty());
        assert_eq!(outcome.rejected_reason, None);
    }

    #[test]
    fn pure_casing_change_is_zero_diff() {
        // "the API" → "the api": normalized tokens identical → no pair.
        let outcome = extract_corrections("call the API now", "call the api now");
        assert!(outcome.pairs.is_empty());
        assert_eq!(outcome.rejected_reason, None);
    }

    #[test]
    fn stoplist_all_stopword_heard_is_dropped() {
        // "de" → "the": heard is entirely a Spanish stopword → dropped.
        // Anchor with content words so the rewrite guard does not fire.
        let outcome = extract_corrections("acme de cascade omega", "acme the cascade omega");
        assert!(
            outcome.pairs.is_empty(),
            "all-stopword heard must be dropped, got {:?}",
            outcome.pairs
        );
        assert_eq!(outcome.rejected_reason, None);
    }

    #[test]
    fn stoplist_survives_when_one_heard_token_is_content() {
        // A fix whose heard span contains a NON-stopword survives, even when
        // it also contains stopwords — the guard requires ALL tokens to be
        // stopwords to drop the pair.
        //
        // Single-token change: heard = "cascad" (content) → survives.
        let shown = "acme el cascad omega alpha beta gamma delta";
        let edited = "acme el Cascade omega alpha beta gamma delta";
        let got = pairs(shown, edited);
        assert_eq!(got, vec![("cascad".to_string(), "Cascade".to_string())]);

        // Now the explicit "the cascad" merged-span case: make both the
        // article and the content word change together so they merge.
        let shown2 = "acme de cascad omega alpha beta gamma delta";
        let edited2 = "acme the Cascade omega alpha beta gamma delta";
        let got2 = pairs(shown2, edited2);
        assert_eq!(
            got2,
            vec![("de cascad".to_string(), "the Cascade".to_string())],
            "merged span with one content token must survive the stoplist"
        );
    }

    #[test]
    fn extracted_pair_actually_applies_downstream() {
        // The extracted pair, once built and applied to the shown text,
        // enacts the correction. This is the extraction↔apply contract.
        let outcome = extract_corrections("deploy to cascad now", "deploy to Cascade now");
        let set = CorrectionSet::build(&outcome.pairs).set;
        assert_eq!(
            set.apply("deploy to cascad now").text,
            "deploy to Cascade now"
        );
    }

    // ── Property test: extract → build → apply round-trip ─────────────────
    use proptest::prelude::*;

    /// Small controlled content-word vocabulary — NO stopwords, so a random
    /// single-token swap always produces a learnable (non-stoplist) pair.
    const CONTENT_WORDS: &[&str] = &[
        "cascad",
        "acme",
        "devkit",
        "openwhisper",
        "deploy",
        "branch",
        "commit",
        "staging",
        "review",
        "config",
    ];

    fn content_text() -> impl Strategy<Value = Vec<usize>> {
        // 3..8 word indices into CONTENT_WORDS.
        proptest::collection::vec(0..CONTENT_WORDS.len(), 3..8)
    }

    proptest! {
        /// A single-token swap at a chosen position produces a pair that,
        /// when built and applied to the shown text, actually enacts the swap.
        /// Vocabulary is all content words (no stoplist hits), and the swap
        /// always yields a distinct token, so a pair is always learnable —
        /// unless the swap collides with an existing token elsewhere in a way
        /// that makes apply idempotency reject it, in which case we assert the
        /// weaker "if a pair survived build, it fires" claim.
        #[test]
        fn single_swap_round_trip(
            indices in content_text(),
            pos in 0usize..7,
            replacement in 0..CONTENT_WORDS.len(),
        ) {
            let n = indices.len();
            let pos = pos % n;
            let orig = indices[pos];
            // Force a DIFFERENT word at pos.
            let repl = if replacement == orig {
                (replacement + 1) % CONTENT_WORDS.len()
            } else {
                replacement
            };

            let shown_words: Vec<&str> = indices.iter().map(|&i| CONTENT_WORDS[i]).collect();
            let mut edited_words = shown_words.clone();
            edited_words[pos] = CONTENT_WORDS[repl];

            let shown = shown_words.join(" ");
            let edited = edited_words.join(" ");

            let outcome = extract_corrections(&shown, &edited);
            // No rewrite guard: exactly one token of >=3 changed → <=33%.
            prop_assert_eq!(outcome.rejected_reason.clone(), None);

            let build = CorrectionSet::build(&outcome.pairs);
            // If the pair survived the build's idempotency checks, applying it
            // to the shown text must produce the swapped word at that spot.
            if !build.set.is_empty() {
                let applied = build.set.apply(&shown).text;
                let applied_tokens: Vec<String> =
                    crate::corrections::tokenizer::normalized_tokens(&applied);
                prop_assert_eq!(
                    &applied_tokens[pos],
                    &crate::corrections::tokenizer::normalize(CONTENT_WORDS[repl])
                );
            }
        }
    }
}
