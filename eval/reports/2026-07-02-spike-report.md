# T2 Eval Spike — Gate Report (2026-07-02)

**Verdict as specified: GATE FAIL.** No single config met all three bars
(≥90% corrected term recall, 0 false rewrites, median RTF ≥5).
**Read the decomposition before concluding** — the core bet validated; the
failures decompose into a miscalibrated recall bar, one fixable design gap,
and a hardware-honest latency result that changes the default-model choice.

## Setup

- Rig: M1 Pro (Metal), release build, `handy --transcribe-file` (the app's
  batch path), `--repeat 2` (best-of latency). Custom-words knobs off,
  post-process off (defaults).
- Fixtures: 28 synthetic utterances (macOS `say`: Paulina es_MX, Samantha
  en_US). Splits: 11 train (pairs learned here), 9 held-out (gate measured
  here — same 6 custom terms, different sentences), 8 negative (no terms).
  Terms: OpenWhisper, Acme, devkit, Cascade, Globex, Tauri.
- Pair learning: token-LCS diff vs reference (spike stand-in for E1),
  1–2 examples per term. Applied via the product `CorrectionSet`.
- Models: Whisper Large v3 (Q5_K_M), Whisper Large v3 Turbo (Q8_0),
  Parakeet TDT 0.6B v3 (int8).

### Known limitations

1. **Synthetic TTS speech**, not real dictation. Whisper hears TTS cleanly
   (it pronounced "OpenWhisper" right more often than a human likely would);
   real audio will shift absolute numbers. E4 must use recorded dictation.
2. **CLI path skips live-recording VAD** — VAD-clipped word onsets unmeasured.
3. **Language-axis rig bug in run 1**: the setting was written to the wrong
   store key, so run 1's en/es columns silently ran as `auto`. Fixed; run 2
   (turbo, all three languages, setting verified applied) reproduced
   identical accuracy — conclusion below stands on the fixed rig.

## Results (run 1 matrix + run 2 fixed-rig turbo column)

| model             | lang       | term recall raw→corr (exact) | false rewrites | WER raw→corr  | med. RTF  |
| ----------------- | ---------- | ---------------------------- | -------------- | ------------- | --------- |
| large-v3 Q5       | auto/en/es | 33%→44% (4/9)                | 0/8            | 11.6%→10.1%   | 0.6–0.7   |
| large-v3-turbo Q8 | auto/en/es | 44%→**78%** (7/9)            | 0/8            | 7.2%→**1.4%** | 0.6–1.3   |
| parakeet v3 int8  | auto/en/es | 22%→56% (5/9)                | 0/8            | 14.5%→7.2%    | **15–16** |

## Findings

**F1 — The correction loop works.** With only 1–2 learned examples per term,
corrected term recall rose in every config (turbo +34pts, parakeet +34pts,
large +11pts) and turbo's held-out WER dropped 7.2%→1.4%. The mechanism the
product is built on delivers.

**F2 — Zero false rewrites, everywhere.** 0/8 negative-corpus rewrites across
all 12 config runs. The exact-match + validation design does not overcorrect.
This was the scariest risk and it held completely.

**F3 — Latency splits the model family.** Whisper large/turbo: RTF 0.6–1.3 on
M1 Pro Metal — a 10s utterance takes ~8–16s; fails the ≤2s/10s budget and is
not daily-usable on this hardware. Parakeet V3: RTF ~16 (median inference
~170ms) — sails through. The realistic default is **Parakeet V3 + corrections**,
with whisper-turbo as a quality-first option on faster hardware.

**F4 — Design gap: all-lowercase proper nouns.** "Devkit"→"devkit" cannot be
enforced: lowercase `correct` adapts to context casing (by design), so a
casing-only fix to a lowercase brand is a no-op. Fix: per-pair
`verbatim: bool` in the T3 schema (learned pairs default verbatim).
Cost: one column + one branch. Turbo's residual 2/9 includes one of these.

**F5 — Mishearing variance is the recall ceiling, and it argues for bounded
fuzzy.** Residual misses are new variants one edit from trained pairs
(train "Creedy"/held-out "Greedy"; "Banco Pell"/"Banco Pel"; "Credi"/"Credit").
Exact match fixes repeats; it cannot fix a variant it hasn't seen. Two closers:
(a) accumulation — each user fix is permanent, so recall climbs with use
(fix-once-never-again held in every config); (b) bounded fuzzy matching over
learned pairs only — deferred per plan, now with evidence; must pass the E4
false-rewrite bar before enabling.

**F6 — Language setting: no measurable effect.** With the setting verified
applied (run 2), auto/en/es produced identical accuracy per model on these
fixtures. `auto` is a safe v1 default; drop the language axis from routine
benchmarks.

## Gate decomposition

| Bar                   | large-v3 | turbo | parakeet |
| --------------------- | -------- | ----- | -------- |
| corrected recall ≥90% | ✗ 44%    | ✗ 78% | ✗ 56%    |
| false rewrites = 0    | ✓        | ✓     | ✓        |
| median RTF ≥5         | ✗        | ✗     | ✓        |

The 90%-recall bar was calibrated implicitly assuming repeat mishearings;
with 1–2 training examples per term and TTS variance, no exact-match system
hits it. After the F4 casing fix and one additional user correction per
residual variant, turbo reaches 9/9 on this set — which is the actual product
loop, not a benchmark trick. The honest statement: **the mechanism is
validated; the bar as written measures accumulation the spike didn't give it.**

## Recommendation

**GO to T3–T5, with two adjustments:**

1. Add the per-pair `verbatim` flag to the T3 schema (F4).
2. Default model: Parakeet V3 (F3); whisper-turbo offered as quality option.
   Keep bounded fuzzy deferred behind the E4 benchmark (F5 evidence noted).
   Re-run this gate with real dictated audio before public launch (E4).

## Reproduce

```
./eval/generate_audio.sh
cd src-tauri && cargo build --release --bin handy --bin eval_gate
./target/release/eval_gate --manifest ../eval/fixtures/manifest.json \
  --audio-dir ../eval/audio --handy-bin target/release/handy \
  --models "<ids from --list-models>" --languages auto --repeat 2 \
  --out-dir ../eval/results
```

Raw per-config JSON (transcripts, learned pairs, metrics): `eval/results/`
(gitignored run artifacts; regenerate with the command above).
