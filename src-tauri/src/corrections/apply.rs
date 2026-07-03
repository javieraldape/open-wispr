//! Token-level correction apply pass.
//!
//! PIPELINE POSITION: this is the FINAL text transformation before emit, in
//! both the UI paste path and the CLI path (see plan: "corrections always
//! last"). It performs exact, token-level, case-insensitive (Unicode-aware)
//! matching of learned pairs — no fuzzy matching, no prompt injection.
//!
//!   input text ─▶ tokenize (shared tokenizer) ─▶ scan left→right
//!        │                                            │
//!        │              longest pattern first (leftmost-longest)
//!        │              multi-token patterns require whitespace-only gaps
//!        ▼                                            ▼
//!   untouched spans copied verbatim          matched span → replacement
//!   (punctuation/casing preserved)           (casing adapted, see below)
//!
//! CASING RULE: each pair carries a `verbatim` flag (eval spike finding F4 —
//! all-lowercase brand names like "devkit" are unenforceable without it):
//! - `verbatim: true`  → emit `correct` exactly as stored, always.
//! - `verbatim: false` → adapt to the matched text's case pattern via
//!   `audio_toolkit::text::preserve_case_pattern` (ALL CAPS / Capitalized).
//!   If `correct` contains any uppercase (mixed case like "OpenWhisper"),
//!   fall back to verbatim emission — preserve_case_pattern cannot express
//!   mixed case. Learned pairs default to `verbatim: true`.
//!
//! IDEMPOTENCY INVARIANT: `apply(apply(x)) == apply(x)`. Enforced two ways:
//! 1. Structurally — a single left→right pass never rescans replaced output.
//! 2. At build time — a pair is rejected (`NotIdempotent`) unless:
//!    a. the built set maps the pair's own output (in all casing variants
//!       the apply pass can emit) to itself — every replacement is a fixed
//!       point in isolation; AND
//!    b. no OVERLAP PROBE around the pair breaks `apply²(x) == apply(x)`.
//!       A replacement's output tokens can combine with ADJACENT text to
//!       form a new pattern match on a second pass (e.g. "review"→"open"
//!       plus a pattern "open push": "review push" → "open push" → "el").
//!       For every pattern whose edge overlaps the pair's output tokens, a
//!       probe text (heard + the pattern's remaining tokens, and
//!       heard-adjacency combos with other pairs) is tested empirically —
//!       which correctly EXONERATES benign pair-contains-pair sets like
//!       {"whisper"→"Whisper", "open whisper"→"OpenWhisper"}, where
//!       leftmost-longest matching absorbs the junction on the first pass.
//!    Both are checked iteratively so mutually-cancelling pairs (a→b, b→a)
//!    are both rejected rather than ping-ponging.

use std::collections::HashMap;

use super::tokenizer::{normalize, normalized_tokens, tokenize, Token};
use crate::audio_toolkit::text::preserve_case_pattern;

/// Maximum tokens allowed in a pattern (`heard` side). The E1 extraction
/// guard is stricter (4); this is the apply-side hard cap for manual entries.
pub const MAX_PATTERN_TOKENS: usize = 8;

/// A learned or manual correction: "when you hear X, write Y".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionPair {
    pub heard: String,
    pub correct: String,
    /// `true` → emit `correct` exactly as stored; `false` → adapt casing to
    /// the matched text (see the module-level CASING RULE). Learned pairs
    /// default to `true`.
    pub verbatim: bool,
}

/// Why a pair was rejected at build time. Rejected pairs are surfaced to the
/// caller (eventually the E6 panel) — never silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    /// `heard` has no tokens (empty/whitespace/punctuation-only).
    EmptyHeard,
    /// `correct` is empty/whitespace — deletion pairs are not allowed.
    EmptyCorrect,
    /// `heard` and `correct` are exactly identical — a no-op.
    Identical,
    /// `heard` exceeds MAX_PATTERN_TOKENS tokens.
    TooManyTokens,
    /// Applying the built set to this pair's own output changes it again —
    /// the pair would break `apply(apply(x)) == apply(x)`.
    NotIdempotent,
    /// Another pair with the same normalized `heard` was added later and
    /// wins (last-write-wins conflict rule).
    SupersededByNewer,
}

impl std::fmt::Display for PairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairError::EmptyHeard => write!(f, "the heard text contains no words"),
            PairError::EmptyCorrect => write!(f, "the replacement text is empty"),
            PairError::Identical => write!(f, "heard and replacement are identical"),
            PairError::TooManyTokens => {
                write!(f, "the heard text exceeds {MAX_PATTERN_TOKENS} words")
            }
            PairError::NotIdempotent => {
                write!(
                    f,
                    "this correction conflicts with another and would re-trigger on its own output"
                )
            }
            PairError::SupersededByNewer => {
                write!(
                    f,
                    "a newer correction for the same heard text replaces this one"
                )
            }
        }
    }
}

/// A pair rejected during `CorrectionSet::build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedPair {
    pub pair: CorrectionPair,
    pub error: PairError,
}

/// Per-pair validation shared by [`CorrectionSet::build`] and the SQLite
/// store (which must reject invalid pairs BEFORE they are persisted).
/// Returns the pair's normalized heard tokens on success.
pub fn validate_pair(pair: &CorrectionPair) -> Result<Vec<String>, PairError> {
    let heard_tokens = normalized_tokens(&pair.heard);
    if heard_tokens.is_empty() {
        return Err(PairError::EmptyHeard);
    }
    if pair.correct.trim().is_empty() {
        return Err(PairError::EmptyCorrect);
    }
    if pair.heard == pair.correct {
        return Err(PairError::Identical);
    }
    if heard_tokens.len() > MAX_PATTERN_TOKENS {
        return Err(PairError::TooManyTokens);
    }
    Ok(heard_tokens)
}

/// One correction that was applied to a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCorrection {
    /// Normalized heard key of the pair that fired (join of normalized tokens).
    pub heard_key: String,
    /// Byte range in the ORIGINAL text that was replaced.
    pub start: usize,
    pub end: usize,
}

/// Result of applying a `CorrectionSet` to a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub text: String,
    pub applied: Vec<AppliedCorrection>,
}

#[derive(Debug, Clone)]
struct Pattern {
    /// Normalized token sequence to match.
    tokens: Vec<String>,
    /// Replacement text (original casing as stored).
    correct: String,
    /// Emit `correct` exactly as stored: the pair is marked verbatim, OR
    /// `correct` contains uppercase (preserve_case_pattern can't express
    /// mixed case, so non-verbatim uppercase pairs fall back to verbatim).
    emit_verbatim: bool,
    /// Cached key (tokens joined by space) for reporting.
    key: String,
}

/// A validated, immutable set of correction patterns ready to apply.
#[derive(Debug, Clone, Default)]
pub struct CorrectionSet {
    patterns: Vec<Pattern>,
    /// first normalized token → indices into `patterns`, longest-first.
    index: HashMap<String, Vec<usize>>,
}

/// Result of building a `CorrectionSet` from raw pairs.
#[derive(Debug, Clone, Default)]
pub struct BuildOutcome {
    pub set: CorrectionSet,
    pub rejected: Vec<RejectedPair>,
}

impl CorrectionSet {
    /// Build a validated set from pairs in insertion order (oldest first).
    /// Later pairs win conflicts on the same normalized heard key.
    pub fn build(pairs: &[CorrectionPair]) -> BuildOutcome {
        let mut rejected: Vec<RejectedPair> = Vec::new();

        // ── Stage 1: per-pair validation (shared with the store) ─────────
        let mut candidates: Vec<(CorrectionPair, Vec<String>)> = Vec::new();
        for pair in pairs {
            match validate_pair(pair) {
                Ok(heard_tokens) => candidates.push((pair.clone(), heard_tokens)),
                Err(error) => rejected.push(RejectedPair {
                    pair: pair.clone(),
                    error,
                }),
            }
        }

        // ── Stage 2: last-write-wins dedup on normalized heard key ───────
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let mut keep: Vec<bool> = vec![true; candidates.len()];
        for (i, (_, toks)) in candidates.iter().enumerate() {
            let key = toks.join(" ");
            if let Some(&prev) = by_key.get(&key) {
                keep[prev] = false;
                rejected.push(RejectedPair {
                    pair: candidates[prev].0.clone(),
                    error: PairError::SupersededByNewer,
                });
            }
            by_key.insert(key, i);
        }
        let mut live: Vec<(CorrectionPair, Vec<String>)> = candidates
            .into_iter()
            .zip(keep)
            .filter_map(|(c, k)| k.then_some(c))
            .collect();

        // ── Stage 3: iterative fixed-point (idempotency) validation ──────
        // A pair is safe iff (a) the set maps every casing variant of its
        // own replacement output to itself, and (b) every overlap probe
        // around its output is stable under a second apply (see the module
        // docs). Reject offenders and re-check until stable (bounded by the
        // number of pairs).
        loop {
            let set = Self::assemble(&live);
            let mut offenders: Vec<usize> = Vec::new();
            'pairs: for (i, (pair, _)) in live.iter().enumerate() {
                for variant in Self::reachable_outputs(&pair.correct, pair.verbatim) {
                    if set.apply(&variant).text != variant {
                        offenders.push(i);
                        continue 'pairs;
                    }
                }
                for probe in Self::overlap_probes(pair, &live, &set) {
                    let once = set.apply(&probe).text;
                    if set.apply(&once).text != once {
                        offenders.push(i);
                        continue 'pairs;
                    }
                }
            }
            if offenders.is_empty() {
                return BuildOutcome { set, rejected };
            }
            for &i in offenders.iter().rev() {
                let (pair, _) = live.remove(i);
                rejected.push(RejectedPair {
                    pair,
                    error: PairError::NotIdempotent,
                });
            }
        }
    }

    /// Probe texts that could reveal a cross-boundary rewrite enabled by
    /// this pair's replacement output: the output's edge tokens overlapping
    /// a pattern's edge, completed with that pattern's remaining tokens as
    /// raw context. Three geometries, plus heard-adjacency combos:
    ///
    ///   output: [... o1 o2]   pattern: [o2 t2 t3]   → probe "heard t2 t3"
    ///   output: [o1 o2 ...]   pattern: [t1 t2 o1]   → probe "t1 t2 heard"
    ///   output: [o1]          pattern: [t1 o1 t3]   → probe "t1 heard t3"
    ///   output suffix starts a longer pattern       → probe "heard heardY"
    ///                                                  for every live Y
    ///                                                  (adjacent outputs
    ///                                                  can supply the
    ///                                                  pattern's tail)
    ///
    /// Each probe is tested empirically (`apply²(p) == apply(p)`), so benign
    /// overlaps absorbed by leftmost-longest matching on the first pass are
    /// kept, and only real second-pass rewrites reject the pair.
    fn overlap_probes(
        pair: &CorrectionPair,
        live: &[(CorrectionPair, Vec<String>)],
        set: &CorrectionSet,
    ) -> Vec<String> {
        let out = normalized_tokens(&pair.correct);
        let heard = pair.heard.trim();
        let mut probes: Vec<String> = Vec::new();
        let mut suffix_starts_longer_pattern = false;

        for pat in &set.patterns {
            let t = &pat.tokens;
            let max_k = out.len().min(t.len().saturating_sub(1));
            // Right overlap: output suffix == pattern prefix, pattern longer.
            for k in 1..=max_k {
                if out[out.len() - k..] == t[..k] {
                    suffix_starts_longer_pattern = true;
                    probes.push(format!("{heard} {}", t[k..].join(" ")));
                }
            }
            // Left overlap: output prefix == pattern suffix, pattern longer.
            for k in 1..=max_k {
                if out[..k] == t[t.len() - k..] {
                    probes.push(format!("{} {heard}", t[..t.len() - k].join(" ")));
                }
            }
            // Strict infix: the pattern extends beyond the output both ways.
            if t.len() > out.len() + 1 {
                for start in 1..t.len() - out.len() {
                    if t[start..start + out.len()] == out[..] {
                        probes.push(format!(
                            "{} {heard} {}",
                            t[..start].join(" "),
                            t[start + out.len()..].join(" ")
                        ));
                    }
                }
            }
        }

        // Adjacent replacements: if this output's suffix can start a longer
        // pattern, another pair's output immediately after it may supply the
        // tail — probe heard-adjacency against every live pair (incl. self).
        if suffix_starts_longer_pattern {
            for (other, _) in live {
                probes.push(format!("{heard} {}", other.heard.trim()));
            }
        }

        probes
    }

    /// The replacement outputs the apply pass can actually emit for a pair —
    /// the exact set the fixed-point check must verify.
    ///
    /// Verbatim pairs — flagged `verbatim`, or non-verbatim with an uppercase
    /// `correct` (the mixed-case fallback) — always emit `correct` itself.
    /// Adaptive lowercase pairs go through `preserve_case_pattern`, which can
    /// emit as-is, Capitalized-first, or ALL CAPS. Checking casings the pass
    /// can never produce would wrongly reject casing corrections like
    /// "acme" → "Acme" (whose ALL-CAPS form would re-trigger).
    fn reachable_outputs(correct: &str, verbatim: bool) -> Vec<String> {
        let trimmed = correct.trim();
        if verbatim || trimmed.chars().any(|c| c.is_uppercase()) {
            return vec![trimmed.to_string()];
        }
        let mut variants = vec![trimmed.to_string()];
        let mut chars: Vec<char> = trimmed.chars().collect();
        if let Some(first) = chars.first_mut() {
            *first = first.to_uppercase().next().unwrap_or(*first);
        }
        variants.push(chars.into_iter().collect());
        variants.push(trimmed.to_uppercase());
        variants.dedup();
        variants
    }

    fn assemble(live: &[(CorrectionPair, Vec<String>)]) -> CorrectionSet {
        let mut patterns: Vec<Pattern> = Vec::with_capacity(live.len());
        for (pair, toks) in live {
            patterns.push(Pattern {
                key: toks.join(" "),
                tokens: toks.clone(),
                correct: pair.correct.trim().to_string(),
                emit_verbatim: pair.verbatim || pair.correct.chars().any(|c| c.is_uppercase()),
            });
        }
        let mut index: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, p) in patterns.iter().enumerate() {
            index.entry(p.tokens[0].clone()).or_default().push(i);
        }
        // Longest pattern first per bucket → leftmost-longest matching.
        for bucket in index.values_mut() {
            bucket.sort_by_key(|&i| std::cmp::Reverse(patterns[i].tokens.len()));
        }
        CorrectionSet { patterns, index }
    }

    /// Number of active patterns in the set.
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Apply the set to `text` in a single left→right pass.
    ///
    /// Replaced output is never rescanned. Multi-token patterns only match
    /// when the gap between consecutive tokens is whitespace-only, so pairs
    /// never match across punctuation ("voy, a" is not "voy a").
    pub fn apply(&self, text: &str) -> ApplyOutcome {
        if self.patterns.is_empty() {
            return ApplyOutcome {
                text: text.to_string(),
                applied: Vec::new(),
            };
        }

        let tokens: Vec<Token> = tokenize(text);
        let norm: Vec<String> = tokens.iter().map(|t| normalize(t.text(text))).collect();

        let mut out = String::with_capacity(text.len());
        let mut applied: Vec<AppliedCorrection> = Vec::new();
        let mut copied_up_to = 0usize; // byte offset into `text`
        let mut i = 0usize; // token index

        while i < tokens.len() {
            let mut matched = false;
            if let Some(bucket) = self.index.get(&norm[i]) {
                for &pi in bucket {
                    let pat = &self.patterns[pi];
                    let n = pat.tokens.len();
                    if i + n > tokens.len() {
                        continue;
                    }
                    if !sequence_matches(&norm, &tokens, text, i, &pat.tokens) {
                        continue;
                    }
                    // Match: copy the untouched span, emit the replacement.
                    let span_start = tokens[i].start;
                    let span_end = tokens[i + n - 1].end;
                    out.push_str(&text[copied_up_to..span_start]);
                    let matched_text = &text[span_start..span_end];
                    let replacement = if pat.emit_verbatim {
                        pat.correct.clone()
                    } else {
                        preserve_case_pattern(matched_text, &pat.correct)
                    };
                    out.push_str(&replacement);
                    applied.push(AppliedCorrection {
                        heard_key: pat.key.clone(),
                        start: span_start,
                        end: span_end,
                    });
                    copied_up_to = span_end;
                    i += n;
                    matched = true;
                    break;
                }
            }
            if !matched {
                i += 1;
            }
        }

        out.push_str(&text[copied_up_to..]);
        ApplyOutcome { text: out, applied }
    }
}

/// Does the normalized token window starting at `i` equal `pattern`, with
/// whitespace-only gaps between consecutive matched tokens?
fn sequence_matches(
    norm: &[String],
    tokens: &[Token],
    text: &str,
    i: usize,
    pattern: &[String],
) -> bool {
    for (k, ptok) in pattern.iter().enumerate() {
        if &norm[i + k] != ptok {
            return false;
        }
        if k > 0 {
            let gap = &text[tokens[i + k - 1].end..tokens[i + k].start];
            if !gap.chars().all(char::is_whitespace) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim pair — the learned-pair default (`verbatim: true`).
    fn pair(heard: &str, correct: &str) -> CorrectionPair {
        CorrectionPair {
            heard: heard.to_string(),
            correct: correct.to_string(),
            verbatim: true,
        }
    }

    /// Case-adaptive pair (`verbatim: false`) — manual pairs may opt into
    /// context casing via preserve_case_pattern.
    fn adaptive(heard: &str, correct: &str) -> CorrectionPair {
        CorrectionPair {
            heard: heard.to_string(),
            correct: correct.to_string(),
            verbatim: false,
        }
    }

    fn set(pairs: &[CorrectionPair]) -> CorrectionSet {
        CorrectionSet::build(pairs).set
    }

    fn apply(pairs: &[CorrectionPair], text: &str) -> String {
        set(pairs).apply(text).text
    }

    // ── Basic matching ──────────────────────────────────────────────────

    #[test]
    fn single_token_exact_replace() {
        assert_eq!(
            apply(&[pair("cascad", "Cascade")], "deploy to cascad now"),
            "deploy to Cascade now"
        );
    }

    #[test]
    fn no_pairs_is_identity() {
        assert_eq!(apply(&[], "hola mundo"), "hola mundo");
    }

    #[test]
    fn no_match_is_identity() {
        assert_eq!(
            apply(&[pair("cascad", "Cascade")], "nothing to fix here"),
            "nothing to fix here"
        );
    }

    #[test]
    fn empty_text_is_identity() {
        assert_eq!(apply(&[pair("a b", "c")], ""), "");
    }

    #[test]
    fn punctuation_around_match_is_preserved() {
        assert_eq!(
            apply(&[pair("cascad", "Cascade")], "ship it: cascad, hoy."),
            "ship it: Cascade, hoy."
        );
    }

    #[test]
    fn multiple_matches_in_one_text() {
        assert_eq!(apply(&[pair("foo", "bar")], "foo y foo"), "bar y bar");
    }

    // ── Multi-token patterns and adjacency ──────────────────────────────

    #[test]
    fn multi_word_pair_matches_across_whitespace() {
        assert_eq!(
            apply(
                &[pair("open whisper", "OpenWhisper")],
                "el proyecto open whisper va bien"
            ),
            "el proyecto OpenWhisper va bien"
        );
    }

    #[test]
    fn multi_word_pair_does_not_match_across_punctuation() {
        // "voy, a" must NOT be treated as "voy a"
        assert_eq!(
            apply(&[pair("voy a", "voy a")], "voy, a casa"),
            "voy, a casa"
        );
        assert_eq!(
            apply(&[pair("open whisper", "OpenWhisper")], "open, whisper"),
            "open, whisper"
        );
    }

    #[test]
    fn multi_word_matches_across_newline_gap() {
        // Newlines are whitespace; the replacement collapses the span.
        assert_eq!(
            apply(&[pair("open whisper", "OpenWhisper")], "open\nwhisper"),
            "OpenWhisper"
        );
    }

    // ── Leftmost-longest / overlapping patterns ─────────────────────────

    #[test]
    fn longest_pattern_wins_at_same_position() {
        let pairs = [
            pair("whisper", "Whisper"),
            pair("open whisper", "OpenWhisper"),
        ];
        assert_eq!(apply(&pairs, "open whisper is here"), "OpenWhisper is here");
        assert_eq!(apply(&pairs, "the whisper app"), "the Whisper app");
    }

    #[test]
    fn no_rescan_of_replaced_output_within_one_pass() {
        // "abc" -> "xyz q"; a pattern for "q" must not fire on the emitted q.
        let pairs = [pair("abc", "xyz q"), pair("q", "QUEUE")];
        // NOTE: "abc" -> "xyz q" contains "q", which the fixed-point check
        // rejects (applying the set to "xyz q" yields "xyz QUEUE").
        let outcome = CorrectionSet::build(&pairs);
        assert_eq!(outcome.rejected.len(), 1);
        assert_eq!(outcome.rejected[0].error, PairError::NotIdempotent);
        assert_eq!(outcome.rejected[0].pair.heard, "abc");
        // The surviving set still corrects standalone "q".
        assert_eq!(outcome.set.apply("a q b").text, "a QUEUE b");
    }

    // ── Casing ──────────────────────────────────────────────────────────

    #[test]
    fn proper_noun_correct_is_verbatim() {
        let pairs = [pair("open whisper", "OpenWhisper")];
        assert_eq!(apply(&pairs, "OPEN WHISPER rocks"), "OpenWhisper rocks");
        assert_eq!(apply(&pairs, "Open whisper rocks"), "OpenWhisper rocks");
    }

    #[test]
    fn verbatim_true_forces_lowercase_brand() {
        // Eval spike finding F4: "Devkit"→"devkit" is unenforceable with
        // case adaptation (the adapted output IS the input). Verbatim wins.
        let pairs = [pair("Devkit", "devkit")];
        assert_eq!(apply(&pairs, "deploy Devkit now"), "deploy devkit now");
        assert_eq!(apply(&pairs, "DEVKIT rocks"), "devkit rocks");
        assert_eq!(apply(&pairs, "use devkit"), "use devkit");
    }

    #[test]
    fn verbatim_false_lowercase_correct_adapts_to_context() {
        let pairs = [adaptive("pushear", "push")];
        assert_eq!(apply(&pairs, "Pushear el branch"), "Push el branch");
        assert_eq!(apply(&pairs, "PUSHEAR YA"), "PUSH YA");
        assert_eq!(apply(&pairs, "voy a pushear"), "voy a push");
    }

    #[test]
    fn verbatim_false_mixed_case_correct_falls_back_to_verbatim() {
        // preserve_case_pattern cannot express mixed case, so a non-verbatim
        // pair whose correct contains uppercase is emitted as stored.
        let pairs = [adaptive("open whisper", "OpenWhisper")];
        assert_eq!(apply(&pairs, "OPEN WHISPER rocks"), "OpenWhisper rocks");
        assert_eq!(apply(&pairs, "open whisper rocks"), "OpenWhisper rocks");
    }

    #[test]
    fn idempotency_holds_for_verbatim_and_adaptive() {
        // The fixed-point check must use each pair's actual reachable
        // outputs: [correct] for verbatim, casing variants for adaptive.
        let pairs = [pair("Devkit", "devkit"), adaptive("pushear", "push")];
        let s = set(&pairs);
        assert_eq!(s.len(), 2, "neither pair may be rejected");
        for sample in [
            "Devkit y DEVKIT",
            "Pushear el branch PUSHEAR",
            "devkit push",
        ] {
            let once = s.apply(sample).text;
            let twice = s.apply(&once).text;
            assert_eq!(once, twice, "not idempotent for: {sample}");
        }
    }

    #[test]
    fn casing_only_pair_works_and_is_idempotent() {
        let pairs = [pair("cascade", "Cascade")];
        let s = set(&pairs);
        assert_eq!(s.len(), 1, "casing pair must NOT be rejected");
        let once = s.apply("vamos a cascade").text;
        assert_eq!(once, "vamos a Cascade");
        assert_eq!(s.apply(&once).text, once);
    }

    // ── Unicode: Spanish diacritics, NFC/NFD ────────────────────────────

    #[test]
    fn diacritic_case_folding_matches() {
        let pairs = [pair("está", "está")];
        // ESTÁ should match está (Unicode lowercase, not ASCII)
        let s = set(&pairs);
        // pair is identical → rejected as no-op; use a real correction:
        assert_eq!(s.len(), 0);
        let pairs = [pair("esta lista", "está lista")];
        assert_eq!(
            CorrectionSet::build(&pairs)
                .set
                .apply("la app esta lista")
                .text,
            "la app está lista"
        );
    }

    #[test]
    fn uppercase_accented_input_matches() {
        let pairs = [adaptive("cafe", "café")];
        assert_eq!(apply(&pairs, "un CAFE por favor"), "un CAFÉ por favor");
    }

    #[test]
    fn nfd_input_matches_nfc_pattern() {
        let pairs = [pair("café", "coffee")];
        let nfd_text = "un cafe\u{0301} grande"; // café in NFD
        assert_eq!(apply(&pairs, nfd_text), "un coffee grande");
    }

    #[test]
    fn word_boundaries_do_not_match_inside_words() {
        // "as" must not fire inside "más"; "an" not inside "año"
        let pairs = [pair("as", "AS"), pair("an", "AN")];
        assert_eq!(apply(&pairs, "más de un año"), "más de un año");
    }

    #[test]
    fn spanish_inverted_punctuation_boundaries() {
        let pairs = [pair("push", "pushear")];
        assert_eq!(apply(&pairs, "¿push? ¡push!"), "¿pushear? ¡pushear!");
    }

    #[test]
    fn contraction_is_a_single_token() {
        // heard "dont" must not match the token "don't"
        let pairs = [pair("dont", "don't")];
        assert_eq!(apply(&pairs, "don't wait"), "don't wait");
        assert_eq!(apply(&pairs, "dont wait"), "don't wait");
    }

    // ── Validation ──────────────────────────────────────────────────────

    #[test]
    fn rejects_empty_and_whitespace_heard() {
        let outcome = CorrectionSet::build(&[pair("", "x"), pair("  ¿? ", "y")]);
        assert_eq!(outcome.set.len(), 0);
        assert!(outcome
            .rejected
            .iter()
            .all(|r| r.error == PairError::EmptyHeard));
    }

    #[test]
    fn rejects_empty_correct() {
        let outcome = CorrectionSet::build(&[pair("foo", ""), pair("bar", "   ")]);
        assert_eq!(outcome.set.len(), 0);
        assert!(outcome
            .rejected
            .iter()
            .all(|r| r.error == PairError::EmptyCorrect));
    }

    #[test]
    fn rejects_identical_pair() {
        let outcome = CorrectionSet::build(&[pair("same", "same")]);
        assert_eq!(outcome.rejected[0].error, PairError::Identical);
    }

    #[test]
    fn rejects_too_many_tokens() {
        let heard = "a b c d e f g h i"; // 9 tokens > 8
        let outcome = CorrectionSet::build(&[pair(heard, "x")]);
        assert_eq!(outcome.rejected[0].error, PairError::TooManyTokens);
    }

    #[test]
    fn last_write_wins_on_same_heard_key() {
        let pairs = [pair("acme", "Acmy"), pair("ACME", "Acme")];
        let outcome = CorrectionSet::build(&pairs);
        assert_eq!(outcome.set.len(), 1);
        assert_eq!(outcome.rejected[0].error, PairError::SupersededByNewer);
        assert_eq!(outcome.set.apply("acme app").text, "Acme app");
    }

    #[test]
    fn self_growing_pair_is_rejected() {
        // "beta" -> "beta max" would re-trigger on its own output forever.
        let outcome = CorrectionSet::build(&[pair("beta", "beta max")]);
        assert_eq!(outcome.set.len(), 0);
        assert_eq!(outcome.rejected[0].error, PairError::NotIdempotent);
    }

    #[test]
    fn mutually_cancelling_pairs_are_both_rejected() {
        let outcome = CorrectionSet::build(&[pair("a", "b"), pair("b", "a")]);
        assert_eq!(outcome.set.len(), 0);
        assert_eq!(
            outcome
                .rejected
                .iter()
                .filter(|r| r.error == PairError::NotIdempotent)
                .count(),
            2
        );
    }

    #[test]
    fn cross_boundary_rewrite_pair_is_rejected() {
        // Found by the idempotency property test: the output "open" combines
        // with a FOLLOWING raw token to form "open push" on a second pass
        // ("review push" → "open push" → "el"). The overlap probe must
        // reject the enabling pair; the multi-token pair stays.
        let outcome =
            CorrectionSet::build(&[adaptive("review", "open"), adaptive("open push", "el")]);
        assert_eq!(outcome.set.len(), 1);
        assert_eq!(outcome.rejected.len(), 1);
        assert_eq!(outcome.rejected[0].pair.heard, "review");
        assert_eq!(outcome.rejected[0].error, PairError::NotIdempotent);
        // And the surviving set is idempotent on the original trigger text.
        let once = outcome.set.apply("review push").text;
        assert_eq!(outcome.set.apply(&once).text, once);
    }

    #[test]
    fn adjacent_outputs_forming_a_pattern_are_rejected() {
        // Two single-token outputs can sit adjacent in pass-1 output and
        // form a pattern only pass 2 would see: "el la" → "open push" → …
        let outcome = CorrectionSet::build(&[
            adaptive("el", "open"),
            adaptive("la", "push"),
            adaptive("open push", "acme"),
        ]);
        let s = outcome.set;
        let once = s.apply("el la").text;
        assert_eq!(s.apply(&once).text, once, "must be idempotent");
    }

    #[test]
    fn pair_contained_in_longer_pair_is_not_falsely_rejected() {
        // Benign overlap: leftmost-longest matching absorbs "open whisper"
        // on the FIRST pass, so the contained pair can never re-trigger.
        // The probe check must keep both (over-rejection would break the
        // plan's pair-contains-pair semantics).
        let outcome = CorrectionSet::build(&[
            pair("whisper", "Whisper"),
            pair("open whisper", "OpenWhisper"),
        ]);
        assert_eq!(outcome.set.len(), 2, "both pairs must stay active");
        assert!(outcome.rejected.is_empty());
    }

    #[test]
    fn chained_pairs_offender_is_rejected_survivor_kept() {
        // x -> beta (offender: output re-triggers), beta -> y (fine alone)
        let outcome = CorrectionSet::build(&[pair("x", "beta"), pair("beta", "y")]);
        assert_eq!(outcome.set.len(), 1);
        assert_eq!(outcome.rejected[0].pair.heard, "x");
        assert_eq!(outcome.set.apply("beta test").text, "y test");
    }

    // ── Idempotency (unit-level; property tests cover it broadly) ───────

    #[test]
    fn apply_twice_equals_apply_once() {
        let pairs = [
            pair("cascad", "Cascade"),
            pair("open whisper", "OpenWhisper"),
            pair("pushear", "push"),
            pair("cafe", "café"),
        ];
        let s = set(&pairs);
        let samples = [
            "voy a pushear el branch de open whisper en cascad",
            "¿Un CAFE? Open Whisper. BIARITS!",
            "nothing matches here",
            "",
        ];
        for sample in samples {
            let once = s.apply(sample).text;
            let twice = s.apply(&once).text;
            assert_eq!(once, twice, "not idempotent for: {sample}");
        }
    }

    // ── ApplyOutcome metadata ────────────────────────────────────────────

    #[test]
    fn applied_metadata_reports_key_and_range() {
        let s = set(&[pair("open whisper", "OpenWhisper")]);
        let text = "use open whisper hoy";
        let outcome = s.apply(text);
        assert_eq!(outcome.applied.len(), 1);
        let a = &outcome.applied[0];
        assert_eq!(a.heard_key, "open whisper");
        assert_eq!(&text[a.start..a.end], "open whisper");
    }
}
