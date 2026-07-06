//! T2 eval spike — the go/no-go gate for the correction-learning wedge.
//!
//! Measures, per (model × language) config, through the REAL CLI pipeline
//! (`handy --transcribe-file`, which shares the app's batch transcription
//! path — known limitation: it skips live-recording VAD):
//!
//!   1. Learn correction pairs from the TRAIN split: token-level LCS diff of
//!      reference vs hypothesis (using the product's shared tokenizer), spans
//!      overlapping a custom term become (heard → term) pairs.
//!   2. Build the product `CorrectionSet` and apply it to HELD-OUT and
//!      NEGATIVE hypotheses.
//!   3. Report:
//!        - term recall on held-out, raw vs corrected (exact casing = the
//!          gate metric; normalized = spelling-only)
//!        - false-rewrite rate on the negative corpus (must be ~0)
//!        - token-level WER raw vs corrected (secondary)
//!        - latency: median inference ms + real-time factor vs the budget
//!          (≤2s per 10s utterance ⇒ RTF ≥ 5)
//!
//! GATE (from the reviewed plan):
//!   corrected exact term recall ≥ 90% on held-out
//!   AND false-rewrite rate == 0 on negative corpus
//!   AND median RTF ≥ 5 at an acceptable model size.
//!
//! Usage (from src-tauri/):
//!   cargo run --bin eval_gate -- \
//!     --manifest ../eval/fixtures/manifest.json --audio-dir ../eval/audio \
//!     --handy-bin target/debug/handy \
//!     --models "handy-computer/whisper-large-v3-gguf/whisper-large-v3-Q5_K_M.gguf,handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf" \
//!     --languages auto,en,es --out-dir ../eval/results

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use handy_app_lib::corrections::{tokenizer, CorrectionPair, CorrectionSet};

const GATE_TERM_RECALL: f64 = 0.90;
const GATE_MIN_RTF: f64 = 5.0;

#[derive(Debug, serde::Deserialize)]
struct Manifest {
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct Fixture {
    id: String,
    split: String,
    text: String,
    terms: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CliJson {
    text: String,
    best_ms: u64,
    load_ms: u64,
    audio_secs: f64,
    rtf: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct Transcript {
    id: String,
    split: String,
    reference: String,
    hypothesis: String,
    best_ms: u64,
    load_ms: u64,
    audio_secs: f64,
    rtf: f64,
}

#[derive(Debug, serde::Serialize)]
struct ConfigReport {
    model: String,
    language: String,
    files: usize,
    failures: Vec<String>,
    pairs_learned: Vec<PairRecord>,
    pairs_rejected: usize,
    heldout_terms_total: usize,
    heldout_term_exact_raw: usize,
    heldout_term_exact_corrected: usize,
    heldout_term_norm_raw: usize,
    heldout_term_norm_corrected: usize,
    negative_files: usize,
    negative_rewrites: usize,
    wer_heldout_raw: f64,
    wer_heldout_corrected: f64,
    median_infer_ms: u64,
    median_rtf: f64,
    gate_term_recall: f64,
    gate_false_rewrite_rate: f64,
    gate_pass: bool,
    transcripts: Vec<Transcript>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct PairRecord {
    heard: String,
    correct: String,
    source_fixture: String,
}

fn main() {
    let args = parse_args();
    let manifest: Manifest =
        serde_json::from_str(&fs::read_to_string(&args.manifest).expect("cannot read manifest"))
            .expect("manifest parse failed");

    fs::create_dir_all(&args.out_dir).expect("cannot create out dir");

    let mut summaries: Vec<(String, String, ConfigReport)> = Vec::new();

    for language in &args.languages {
        for model in &args.models {
            let short = model.rsplit('/').next().unwrap_or(model);
            eprintln!("\n=== config: model={short} language={language} ===");
            let report = run_config(&args, &manifest, model, language);
            let path = args
                .out_dir
                .join(format!("{}-{}-{}.json", args.run_tag, short, language));
            fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).expect("write report");
            eprintln!("wrote {}", path.display());
            summaries.push((short.to_string(), language.clone(), report));
        }
    }

    // ── Summary table ────────────────────────────────────────────────────
    println!("\n| model | lang | term recall raw→corr (exact) | false-rewrites | WER raw→corr | med. infer ms | med. RTF | GATE |");
    println!("|---|---|---|---|---|---|---|---|");
    for (model, lang, r) in &summaries {
        println!(
            "| {} | {} | {:.0}%→{:.0}% ({}/{}) | {}/{} | {:.1}%→{:.1}% | {} | {:.1}x | {} |",
            model,
            lang,
            pct(r.heldout_term_exact_raw, r.heldout_terms_total),
            pct(r.heldout_term_exact_corrected, r.heldout_terms_total),
            r.heldout_term_exact_corrected,
            r.heldout_terms_total,
            r.negative_rewrites,
            r.negative_files,
            r.wer_heldout_raw * 100.0,
            r.wer_heldout_corrected * 100.0,
            r.median_infer_ms,
            r.median_rtf,
            if r.gate_pass { "PASS" } else { "fail" }
        );
    }
    let any_pass = summaries.iter().any(|(_, _, r)| r.gate_pass);
    println!(
        "\nOVERALL GATE: {}",
        if any_pass {
            "PASS — at least one config meets the bar"
        } else {
            "FAIL — no config meets the bar"
        }
    );
    std::process::exit(if any_pass { 0 } else { 3 });
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64 * 100.0
    }
}

fn run_config(args: &Args, manifest: &Manifest, model: &str, language: &str) -> ConfigReport {
    let mut transcripts: Vec<Transcript> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for f in &manifest.fixtures {
        let wav = args.audio_dir.join(format!("{}.wav", f.id));
        match transcribe(&args.handy_bin, &wav, model, language, args.repeat) {
            Ok(j) => {
                eprintln!("  {}: {} ms  \"{}\"", f.id, j.best_ms, j.text.trim());
                transcripts.push(Transcript {
                    id: f.id.clone(),
                    split: f.split.clone(),
                    reference: f.text.clone(),
                    hypothesis: j.text.trim().to_string(),
                    best_ms: j.best_ms,
                    load_ms: j.load_ms,
                    audio_secs: j.audio_secs,
                    rtf: j.rtf,
                });
            }
            Err(e) => {
                eprintln!("  {}: FAILED — {e}", f.id);
                failures.push(format!("{}: {e}", f.id));
            }
        }
    }

    // ── Learn pairs from the train split ─────────────────────────────────
    let mut pairs: Vec<PairRecord> = Vec::new();
    for t in transcripts.iter().filter(|t| t.split == "train") {
        let fixture = manifest.fixtures.iter().find(|f| f.id == t.id).unwrap();
        for term in &fixture.terms {
            for (heard, correct) in learn_pairs(&t.reference, &t.hypothesis, term) {
                pairs.push(PairRecord {
                    heard,
                    correct,
                    source_fixture: t.id.clone(),
                });
            }
        }
    }
    let correction_pairs: Vec<CorrectionPair> = pairs
        .iter()
        .map(|p| CorrectionPair {
            heard: p.heard.clone(),
            correct: p.correct.clone(),
            // Learned pairs default to verbatim (eval spike finding F4).
            verbatim: true,
        })
        .collect();
    let build = CorrectionSet::build(&correction_pairs);
    let set = build.set;

    // ── Held-out metrics ─────────────────────────────────────────────────
    let mut terms_total = 0usize;
    let (mut exact_raw, mut exact_corr, mut norm_raw, mut norm_corr) = (0, 0, 0, 0);
    let (mut wer_raw_num, mut wer_corr_num, mut wer_den) = (0usize, 0usize, 0usize);

    for t in transcripts.iter().filter(|t| t.split == "heldout") {
        let fixture = manifest.fixtures.iter().find(|f| f.id == t.id).unwrap();
        let corrected = set.apply(&t.hypothesis).text;
        for term in &fixture.terms {
            terms_total += 1;
            if term_present_exact(&t.hypothesis, term) {
                exact_raw += 1;
            }
            if term_present_exact(&corrected, term) {
                exact_corr += 1;
            }
            if term_present_normalized(&t.hypothesis, term) {
                norm_raw += 1;
            }
            if term_present_normalized(&corrected, term) {
                norm_corr += 1;
            }
        }
        let ref_toks = tokenizer::normalized_tokens(&t.reference);
        wer_den += ref_toks.len();
        wer_raw_num += edit_distance(&ref_toks, &tokenizer::normalized_tokens(&t.hypothesis));
        wer_corr_num += edit_distance(&ref_toks, &tokenizer::normalized_tokens(&corrected));
    }

    // ── Negative corpus: false rewrites ──────────────────────────────────
    let mut negative_files = 0usize;
    let mut negative_rewrites = 0usize;
    for t in transcripts.iter().filter(|t| t.split == "negative") {
        negative_files += 1;
        let corrected = set.apply(&t.hypothesis);
        if corrected.text != t.hypothesis {
            negative_rewrites += 1;
            eprintln!(
                "  FALSE REWRITE on {}: \"{}\" -> \"{}\"",
                t.id, t.hypothesis, corrected.text
            );
        }
    }

    // ── Latency ──────────────────────────────────────────────────────────
    let mut infer: Vec<u64> = transcripts.iter().map(|t| t.best_ms).collect();
    infer.sort();
    let median_infer_ms = infer.get(infer.len() / 2).copied().unwrap_or(0);
    let mut rtfs: Vec<f64> = transcripts.iter().map(|t| t.rtf).collect();
    rtfs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_rtf = rtfs.get(rtfs.len() / 2).copied().unwrap_or(0.0);

    let gate_term_recall = if terms_total == 0 {
        0.0
    } else {
        exact_corr as f64 / terms_total as f64
    };
    let gate_false_rewrite_rate = if negative_files == 0 {
        1.0
    } else {
        negative_rewrites as f64 / negative_files as f64
    };
    let gate_pass = gate_term_recall >= GATE_TERM_RECALL
        && negative_rewrites == 0
        && median_rtf >= GATE_MIN_RTF
        && failures.is_empty();

    ConfigReport {
        model: model.to_string(),
        language: language.to_string(),
        files: transcripts.len(),
        failures,
        pairs_rejected: build.rejected.len(),
        pairs_learned: pairs,
        heldout_terms_total: terms_total,
        heldout_term_exact_raw: exact_raw,
        heldout_term_exact_corrected: exact_corr,
        heldout_term_norm_raw: norm_raw,
        heldout_term_norm_corrected: norm_corr,
        negative_files,
        negative_rewrites,
        wer_heldout_raw: ratio(wer_raw_num, wer_den),
        wer_heldout_corrected: ratio(wer_corr_num, wer_den),
        median_infer_ms,
        median_rtf,
        gate_term_recall,
        gate_false_rewrite_rate,
        gate_pass,
        transcripts,
    }
}

fn ratio(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
}

// ── Pair learning: token-LCS diff, spans overlapping the term ────────────

/// Learn (heard → term) pairs by diffing normalized reference tokens against
/// hypothesis tokens. Any replaced region whose reference side overlaps the
/// term's tokens yields a pair from the hypothesis-side ORIGINAL text.
/// Spike-grade stand-in for the E1 extraction (same tokenizer, same guards).
fn learn_pairs(reference: &str, hypothesis: &str, term: &str) -> Vec<(String, String)> {
    let term_norm = tokenizer::normalized_tokens(term);
    if term_norm.is_empty() {
        return Vec::new();
    }
    let ref_tokens = tokenizer::tokenize(reference);
    let ref_norm: Vec<String> = ref_tokens
        .iter()
        .map(|t| tokenizer::normalize(t.text(reference)))
        .collect();
    let hyp_tokens = tokenizer::tokenize(hypothesis);
    let hyp_norm: Vec<String> = hyp_tokens
        .iter()
        .map(|t| tokenizer::normalize(t.text(hypothesis)))
        .collect();

    // Term already correct (normalized) in hypothesis? Then a casing-only pair
    // may still help: emit (exact hyp span → term) when casing differs.
    if let Some((s, e)) = find_subsequence(&hyp_norm, &term_norm) {
        let span = &hypothesis[hyp_tokens[s].start..hyp_tokens[e - 1].end];
        if span != term {
            return vec![(span.to_string(), term.to_string())];
        }
        return Vec::new();
    }

    // Locate the term in the reference.
    let Some((rs, re)) = find_subsequence(&ref_norm, &term_norm) else {
        return Vec::new();
    };

    // LCS alignment between ref and hyp normalized tokens.
    let lcs = lcs_table(&ref_norm, &hyp_norm);
    // Walk back to collect aligned (ref_i, hyp_j) matches.
    let mut matches: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (ref_norm.len(), hyp_norm.len());
    while i > 0 && j > 0 {
        if ref_norm[i - 1] == hyp_norm[j - 1] {
            matches.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if lcs[i - 1][j] >= lcs[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    matches.reverse();

    // Hypothesis token range aligned to the replaced ref region [rs, re):
    // between the last match before rs and the first match at/after re.
    let hyp_start = matches
        .iter()
        .rev()
        .find(|(ri, _)| *ri < rs)
        .map(|(_, hj)| hj + 1)
        .unwrap_or(0);
    let hyp_end = matches
        .iter()
        .find(|(ri, _)| *ri >= re)
        .map(|(_, hj)| *hj)
        .unwrap_or(hyp_norm.len());

    // Guards (mirror the E1 extraction guards): non-empty, ≤4 tokens.
    if hyp_start >= hyp_end || hyp_end - hyp_start > 4 {
        return Vec::new();
    }
    let heard = &hypothesis[hyp_tokens[hyp_start].start..hyp_tokens[hyp_end - 1].end];
    vec![(heard.to_string(), term.to_string())]
}

fn lcs_table(a: &[String], b: &[String]) -> Vec<Vec<usize>> {
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    dp
}

fn find_subsequence(haystack: &[String], needle: &[String]) -> Option<(usize, usize)> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|&i| haystack[i..i + needle.len()] == *needle)
        .map(|i| (i, i + needle.len()))
}

/// Exact-casing presence: the term's normalized token n-gram occurs AND the
/// original text span equals the term byte-for-byte.
fn term_present_exact(text: &str, term: &str) -> bool {
    let term_norm = tokenizer::normalized_tokens(term);
    let tokens = tokenizer::tokenize(text);
    let norm: Vec<String> = tokens
        .iter()
        .map(|t| tokenizer::normalize(t.text(text)))
        .collect();
    if term_norm.is_empty() {
        return false;
    }
    let n = term_norm.len();
    if norm.len() < n {
        return false;
    }
    (0..=norm.len() - n).any(|i| {
        norm[i..i + n] == *term_norm && &text[tokens[i].start..tokens[i + n - 1].end] == term
    })
}

/// Spelling-only presence: normalized token n-gram match, casing ignored.
fn term_present_normalized(text: &str, term: &str) -> bool {
    let term_norm = tokenizer::normalized_tokens(term);
    let norm = tokenizer::normalized_tokens(text);
    find_subsequence(&norm, &term_norm).is_some()
}

fn edit_distance(a: &[String], b: &[String]) -> usize {
    let mut dp: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = dp[0];
        dp[0] = i;
        for j in 1..=b.len() {
            let tmp = dp[j];
            dp[j] = if a[i - 1] == b[j - 1] {
                prev
            } else {
                1 + prev.min(dp[j - 1]).min(dp[j])
            };
            prev = tmp;
        }
    }
    dp[b.len()]
}

// ── Subprocess plumbing ───────────────────────────────────────────────────

fn transcribe(
    handy_bin: &Path,
    wav: &Path,
    model: &str,
    language: &str,
    repeat: usize,
) -> Result<CliJson, String> {
    if !wav.exists() {
        return Err(format!("missing wav {}", wav.display()));
    }
    let out = Command::new(handy_bin)
        .args([
            "--transcribe-file",
            wav.to_str().unwrap(),
            "--model",
            model,
            "--language",
            language,
            "--json",
            "--repeat",
            &repeat.to_string(),
        ])
        .output()
        .map_err(|e| format!("spawn failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "exit {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Parse the last stdout line that is a JSON object (logs may precede it).
    stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<CliJson>(l.trim()).ok())
        .or_else(|| serde_json::from_str::<CliJson>(stdout.trim()).ok())
        .ok_or_else(|| {
            format!(
                "no JSON in stdout: {}",
                stdout.chars().take(200).collect::<String>()
            )
        })
}

// ── Arg parsing (std-only; keep the bin dependency-free) ─────────────────

struct Args {
    manifest: PathBuf,
    audio_dir: PathBuf,
    handy_bin: PathBuf,
    models: Vec<String>,
    languages: Vec<String>,
    out_dir: PathBuf,
    repeat: usize,
    run_tag: String,
}

fn parse_args() -> Args {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        if let Some(name) = k.strip_prefix("--") {
            if let Some(v) = it.next() {
                map.insert(name.to_string(), v);
            }
        }
    }
    Args {
        manifest: map
            .get("manifest")
            .cloned()
            .unwrap_or_else(|| "../eval/fixtures/manifest.json".into())
            .into(),
        audio_dir: map
            .get("audio-dir")
            .cloned()
            .unwrap_or_else(|| "../eval/audio".into())
            .into(),
        handy_bin: map
            .get("handy-bin")
            .cloned()
            .unwrap_or_else(|| "target/debug/handy".into())
            .into(),
        models: map
            .get("models")
            .expect("--models required (comma-separated ids)")
            .split(',')
            .map(|s| s.trim().to_string())
            .collect(),
        languages: map
            .get("languages")
            .cloned()
            .unwrap_or_else(|| "auto".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .collect(),
        out_dir: map
            .get("out-dir")
            .cloned()
            .unwrap_or_else(|| "../eval/results".into())
            .into(),
        repeat: map.get("repeat").and_then(|s| s.parse().ok()).unwrap_or(1),
        run_tag: map
            .get("run-tag")
            .cloned()
            .unwrap_or_else(|| "spike".into()),
    }
}
