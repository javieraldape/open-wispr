#!/usr/bin/env bash
# Verify that an explicit Spanish transcription run stays Spanish.
#
# Usage:
#   bash eval/check_language_preservation.sh [model-id]
#
# MODEL_ID can also be provided via the environment. The default targets the
# GGUF Parakeet TDT v3 path used for Spanish-capable onboarding.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODEL_ID="${MODEL_ID:-${1:-handy-computer/parakeet-tdt-0.6b-v3-gguf/parakeet-tdt-0.6b-v3-Q8_0.gguf}}"
HANDY_BIN="${HANDY_BIN:-src-tauri/target/debug/handy}"
WAV="${WAV:-eval/audio/neg-09.wav}"
OUTPUT=".context/language-preservation-output.log"

command -v node >/dev/null || { echo "node required" >&2; exit 1; }

if [ ! -x "$HANDY_BIN" ] || ! "$HANDY_BIN" --help 2>&1 | grep -q -- "--language"; then
  if [ "$HANDY_BIN" != "src-tauri/target/debug/handy" ]; then
    echo "$HANDY_BIN does not support --language; rebuild it or unset HANDY_BIN" >&2
    exit 1
  fi
  cargo build --manifest-path src-tauri/Cargo.toml --bin handy
fi

if [ ! -f "$WAV" ]; then
  ./eval/generate_audio.sh
fi

mkdir -p .context
RUST_LOG=info "$HANDY_BIN" \
  --transcribe-file "$WAV" \
  --model "$MODEL_ID" \
  --language es \
  --json \
  --repeat 1 | tee "$OUTPUT"

node - "$OUTPUT" <<'NODE'
const fs = require("fs");

const file = process.argv[2];
const stdout = fs.readFileSync(file, "utf8");
const jsonLine = stdout
  .trim()
  .split(/\r?\n/)
  .reverse()
  .find((line) => line.trim().startsWith("{"));

if (!jsonLine) {
  console.error("No JSON result found in CLI output");
  process.exit(1);
}

const result = JSON.parse(jsonLine);
const text = String(result.text || "").trim();
const normalized = text
  .toLowerCase()
  .normalize("NFD")
  .replace(/\p{Diacritic}/gu, "");
const tokens = normalized.match(/[a-z]+/g) || [];
const count = (words) => words.reduce((total, word) => total + tokens.filter((token) => token === word).length, 0);

const spanishScore = count([
  "cuando",
  "termino",
  "frase",
  "larga",
  "espanol",
  "espero",
  "transcripcion",
  "conserve",
  "idioma",
  "cambie",
  "automaticamente",
  "ingles",
]);
const englishScore = count([
  "when",
  "finish",
  "long",
  "sentence",
  "spanish",
  "expect",
  "transcription",
  "preserve",
  "language",
  "change",
  "automatically",
  "english",
]);

const failures = [];
if (result.language_intent !== "es") {
  failures.push(`language_intent=${JSON.stringify(result.language_intent)}, expected "es"`);
}
if (result.effective_language !== "es") {
  failures.push(`effective_language=${JSON.stringify(result.effective_language)}, expected "es"`);
}
if (result.language_override !== "es") {
  failures.push(`language_override=${JSON.stringify(result.language_override)}, expected "es"`);
}
if (result.translate_to_english !== false) {
  failures.push(`translate_to_english=${JSON.stringify(result.translate_to_english)}, expected false`);
}
if (result.post_process_enabled !== false) {
  failures.push(`post_process_enabled=${JSON.stringify(result.post_process_enabled)}, expected false`);
}
if (spanishScore < 5 || englishScore > spanishScore) {
  failures.push(`text does not look predominantly Spanish (spanishScore=${spanishScore}, englishScore=${englishScore})`);
}

if (failures.length > 0) {
  console.error("Language preservation check failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  console.error(`text=${JSON.stringify(text)}`);
  process.exit(1);
}

console.log(`language preservation ok: spanishScore=${spanishScore}, englishScore=${englishScore}`);
NODE
