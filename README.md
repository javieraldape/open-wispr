

<div align="center">
  <img src="assets/openwhisper-brand/svg/openwhisper-lockup-black.svg" alt="OpenWhisper logo" width="260">
  <p><strong>Private, local dictation that learns the words you actually use.</strong></p>
  <p>
    <a href="https://github.com/javieraldape/open-wispr/releases/latest">
      <img alt="Download for Mac" src="https://img.shields.io/badge/Download_for_Mac-111111?style=for-the-badge&logo=apple&logoColor=white">
    </a>
  </p>
</div>

<!-- TODO: When the first public macOS artifact name is stable, replace the CTA link with the direct DMG URL under https://github.com/javieraldape/open-wispr/releases/latest/download/. -->

OpenWispr is a Mac-first, offline speech-to-text app for people who dictate into their existing tools. Press a shortcut, speak, and OpenWispr types the result into the app you were already using.

The difference is memory. When a transcript gets a name, product, command, or bilingual phrase wrong, fix it once. OpenWispr learns that correction pair and applies it automatically next time, without sending audio or text to the cloud.

## What It Does

- Transcribes locally on your computer.
- Works from a global shortcut and pastes text into the active app.
- Recommends Parakeet V3 for fast English and Spanish-friendly dictation on Apple Silicon Macs.
- Lets you fix the last transcript and save the correction in one step.
- Keeps a reviewable "Words it learned" list for personal vocabulary and custom terms.
- Preserves Handy's cross-platform foundation while focusing this fork on the Mac v1 workflow first.

## Quick Start

### Download for Mac

Use the **Download for Mac** button above to get the latest macOS build, then drag OpenWispr into your Applications folder.

**First launch (important):** this early release is not yet code-signed with an Apple Developer certificate, so macOS Gatekeeper will warn you the first time you open it. To get past it:

1. Right-click (or Control-click) **OpenWispr** in Applications and choose **Open**.
2. Click **Open** again in the dialog that appears.

You only need to do this once. If macOS still refuses, run this in Terminal and then open the app:

```bash
xattr -dr com.apple.quarantine /Applications/OpenWispr.app
```

Code signing and notarization are planned for a future release, which will remove this step.

If there is not a published release yet, build from source with the steps below. The app installs as **OpenWispr.app**; some internal package names and the CLI binary still use `handy` (see [CLI and Automation](#cli-and-automation)).

### Build from Source

Prerequisites:

- [Rust](https://rustup.rs/) latest stable
- [Bun](https://bun.sh/)
- macOS microphone and accessibility permissions

Install dependencies:

```bash
bun install
```

Download the required voice activity detection model:

```bash
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

Run the app in development mode:

```bash
bun run tauri dev
```

If CMake fails on macOS, retry with the minimum policy override:

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

## First Run

1. Launch OpenWispr.
2. Grant microphone access when macOS asks.
3. Grant accessibility access so OpenWispr can type into other apps.
4. Choose **Parakeet V3** unless you already know you need a different model.
5. Press your recording shortcut, speak, then release or toggle off.
6. If the transcript is wrong, use **Fix Last Transcript**, save the corrected text, and let OpenWispr learn the changed words; the fix window closes itself after saving.
7. Open **Words it learned** in settings to review or delete learned correction pairs.

## Correction Learning

OpenWispr stores correction pairs such as:

```text
open whisper -> OpenWispr
cascad -> Cascade
dev kit -> devkit
```

The correction engine runs after transcription and before text is pasted or copied. It is intentionally narrow: it learns targeted term corrections from your edits, not broad rewrites. This keeps the app predictable and makes personal vocabulary useful for names, commands, products, repos, and English/Spanish code-switching.

Corrections are stored locally in a SQLite database next to the app data. They are not uploaded to a service.

## Development

OpenWispr is built on the Handy desktop app foundation:

- **Frontend:** React, TypeScript, Tailwind CSS, Zustand, i18next
- **Backend:** Rust, Tauri 2, cpal audio I/O, local model managers
- **Speech models:** `transcribe-cpp` for Whisper-family models and `transcribe-rs` for ONNX models such as Parakeet
- **Correction layer:** token-level extraction, validation, persistence, and final text application in Rust

Useful commands:

```bash
bun run dev              # Frontend-only Vite dev server
bun run tauri dev        # Full Tauri app in development
bun run build            # TypeScript + Vite production build
bun run tauri build      # Production desktop build
bun run lint             # ESLint
bun run format:check     # Prettier + cargo fmt check
bun run check:translations
```

For platform-specific build details, see [BUILD.md](BUILD.md). For contribution workflow, see [CONTRIBUTING.md](CONTRIBUTING.md). For translation guidance, see [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).

## CLI and Automation

The app installs as **OpenWispr.app**, but the executable inside it is still named `handy` (renaming it is deferred to a later release). The CLI can control a running app instance:

```bash
handy --toggle-transcription
handy --toggle-post-process
handy --cancel
handy --start-hidden
handy --no-tray
handy --debug
```

On macOS app bundles, call the binary directly (note the executable name is `handy`):

```bash
/Applications/OpenWispr.app/Contents/MacOS/handy --toggle-transcription
```

The internal `handy` executable name may change in a future release.

## Troubleshooting

### Permissions on macOS

OpenWispr needs:

- **Microphone** access to hear speech.
- **Accessibility** access to type the finished transcript into other apps.

If typing does not work, reopen System Settings and confirm the app is enabled under Privacy & Security -> Accessibility.

### Manual Model Installation

If model downloads are blocked by a proxy or restricted network, place models in the app data `models` directory.

Typical app data locations inherited from Handy:

- macOS: `~/Library/Application Support/com.openwispr.app/`
- Windows: `C:\Users\{username}\AppData\Roaming\com.openwispr.app\`
- Linux: `~/.config/com.openwispr.app/`

Whisper `.bin` files go directly in `models/`. Parakeet `.tar.gz` downloads must be extracted, and the extracted directory must keep its expected model directory name.

### Linux Input Tools

Linux support is inherited from Handy, but this fork is Mac-first for v1. For reliable text insertion on Linux, install the tool that matches your display server:

| Display Server | Recommended Tool | Example Command            |
| -------------- | ---------------- | -------------------------- |
| X11            | `xdotool`        | `sudo apt install xdotool` |
| Wayland        | `wtype`          | `sudo apt install wtype`   |
| Both           | `dotool`         | `sudo apt install dotool`  |

If the recording overlay interferes with pasting on Linux, disable the overlay in settings and use audio feedback instead.

## Release Verification

OpenWispr inherits Tauri updater signing from Handy. Release artifacts use Tauri's updater signature format, and the public key is stored in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) under `plugins.updater.pubkey`.

Use `minisign` to verify downloaded artifacts with their matching `.sig` files. Do not use `gpg` for these signatures.

## License and Attribution

OpenWispr is released under the MIT License. See [LICENSE](LICENSE).

This project is a derivative work of **Handy** by CJ Pais and contributors:

- Upstream: <https://github.com/cjpais/Handy>
- Fork notice: [NOTICE.md](NOTICE.md)
- License: MIT, with upstream copyright preserved

All upstream Handy code remains under the MIT License. OpenWispr modifications are also released under the MIT License.

The Handy name, logo, icon, and brand assets belong to the Handy project. OpenWispr is an independent fork and does not imply endorsement by or affiliation with Handy or its maintainers.
