#!/usr/bin/env bash
# Generate eval fixture audio from fixtures/manifest.json using macOS `say`.
#
# Output: eval/audio/<id>.wav — 16 kHz mono 16-bit PCM (what the handy CLI
# requires). Regenerable at any time; eval/audio/ is gitignored.
#
# LIMITATION (documented in the gate report): synthetic TTS speech, not real
# dictation. Custom terms are pronounced by the TTS engine, which may be
# cleaner OR weirder than a human. The production benchmark (E4) must use
# real recorded dictation.
set -euo pipefail
cd "$(dirname "$0")"

command -v say >/dev/null || { echo "macOS 'say' required" >&2; exit 1; }
command -v afconvert >/dev/null || { echo "afconvert required" >&2; exit 1; }
mkdir -p audio

python3 - <<'PY' | while IFS=$'\t' read -r id voice text; do
import json
m = json.load(open("fixtures/manifest.json"))
for f in m["fixtures"]:
    print(f"{f['id']}\t{f['voice']}\t{f['text']}")
PY
  aiff="audio/${id}.aiff"
  wav="audio/${id}.wav"
  say -v "$voice" -o "$aiff" "$text"
  afconvert -f WAVE -d LEI16@16000 -c 1 "$aiff" "$wav" >/dev/null
  rm -f "$aiff"
  echo "generated $wav ($voice)"
done

echo "done: $(ls audio/*.wav | wc -l | tr -d ' ') files"
