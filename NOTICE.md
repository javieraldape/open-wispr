# NOTICE

This project is a derivative work of **Handy** by CJ Pais and contributors.

- Upstream: https://github.com/cjpais/Handy
- Imported at commit: `f13597061ad36b1a4430d61a48aa15a5d4b96e14` (2026-07-02)
- License: MIT (see `LICENSE` — upstream copyright notice preserved intact)

## What this fork changes

This fork builds a correction-learning dictation engine on top of Handy's
speech-to-text foundation:

- A `corrections` manager: personal vocabulary + learned correction pairs,
  applied as the final text stage before emit (UI paste and CLI output).
- A one-tap "fix last transcript" flow that learns persistent corrections.
- A "words it learned" settings panel.
- An evaluation harness for English/Spanish code-switched dictation and
  custom-term accuracy.
- Handy's custom-words mechanisms (initial_prompt injection and fuzzy
  post-processing) are disabled in this fork's v1 in favor of the
  corrections engine.
- `src-tauri/build.rs`: the Apple Intelligence bridge selection now also
  probes for the FoundationModelsMacros compiler plugin (absent on
  Command Line Tools-only toolchains whose SDK still ships the framework)
  and supports an `APPLE_INTELLIGENCE_BRIDGE=real|stub` override. Upstream
  candidate fix.

All upstream code remains under the MIT license. Modifications are also
released under the MIT license.
