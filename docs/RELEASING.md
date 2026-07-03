# Releasing (macOS signing + notarization)

This covers the maintainer setup needed for `.github/workflows/release.yml` to
produce a **signed, notarized, stapled** macOS build for Apple Silicon
(`aarch64-apple-darwin`, run on the `macos-14`/`macos-26` family of GitHub
runners). Intel (`x86_64-apple-darwin`) is out of scope for this guide — it
builds unsigned unless the same secrets happen to also cover it.

If the secrets below are not configured, the same workflow still runs and
produces an **unsigned** build (ad-hoc signed, per `signingIdentity: "-"` in
`src-tauri/tauri.conf.json`). The build log will contain the line:

```
unsigned build (signing secrets not configured)
```

## Prerequisite: Apple Developer Program

You need a paid **Apple Developer Program** membership — **$99/year**,
enrolled at <https://developer.apple.com/programs/>. Without it you cannot
create a "Developer ID Application" certificate or use `notarytool`, and CI
will always fall back to the unsigned path above.

## 1. Create a Developer ID Application certificate

1. Sign in at <https://developer.apple.com/account/resources/certificates/list>.
2. Click **+** to create a new certificate, choose **Developer ID
   Application**, and follow the CSR (Certificate Signing Request) flow using
   Keychain Access on a Mac you control (Keychain Access → Certificate
   Assistant → Request a Certificate from a Certificate Authority).
3. Download the issued certificate (`.cer`) and double-click it to install it
   into your login keychain. It will pair with the private key generated
   alongside the CSR.

## 2. Export the certificate + private key as a `.p12`, then base64-encode it

1. Open **Keychain Access**, find the new "Developer ID Application: Your
   Name (TEAMID)" identity under **My Certificates** (it should show a
   disclosure triangle with the private key nested under it).
2. Right-click the certificate (not just the key) → **Export**. Save as
   `.p12`, and set an export password — this becomes `APPLE_CERTIFICATE_PASSWORD`.
3. Base64-encode it for storage as a GitHub secret:

   ```bash
   base64 -i DeveloperIDApplication.p12 | pbcopy
   ```

   The clipboard now holds the value for the `APPLE_CERTIFICATE` secret.

## 3. Create an app-specific password for notarization

`notarytool` (invoked internally by `tauri-action`) authenticates with your
Apple ID, not your main account password:

1. Go to <https://appleid.apple.com/account/manage> → **Sign-In and
   Security** → **App-Specific Passwords** → generate one.
2. This is the value for the `APPLE_PASSWORD` secret.

## 4. Find your Team ID

<https://developer.apple.com/account> → **Membership** → **Team ID** (a
10-character alphanumeric string, e.g. `A1B2C3D4E5`).

## 5. Add the six GitHub secrets

Repo → **Settings** → **Secrets and variables** → **Actions** → **New
repository secret**. Add all six (these are the exact names
`.github/workflows/build.yml` already reads — inherited by `release.yml` via
`secrets: inherit`):

| Secret                       | Value                                                                                                 |
| ---------------------------- | ----------------------------------------------------------------------------------------------------- |
| `APPLE_CERTIFICATE`          | base64 contents of the `.p12` from step 2                                                             |
| `APPLE_CERTIFICATE_PASSWORD` | the export password you set in step 2                                                                 |
| `KEYCHAIN_PASSWORD`          | any password of your choosing — protects the CI-local temp keychain only, not a real Apple credential |
| `APPLE_ID`                   | your Apple ID email                                                                                   |
| `APPLE_PASSWORD`             | the app-specific password from step 3 (**not** your Apple ID password)                                |
| `APPLE_TEAM_ID`              | your Team ID from step 4                                                                              |

There is no `APPLE_SIGNING_IDENTITY` secret to add — CI derives it at build
time from the imported certificate's common name (see the "verify
certificate" step in `build.yml`) and passes it to `tauri-action` as an
environment variable, not a secret.

All six must be present for CI to attempt signing. If even one is missing,
the workflow logs the unsigned-build line above and continues — it does not
fail the job.

## 6. Trigger a release

`release.yml` runs on `workflow_dispatch` only. From the repo's **Actions**
tab, select **Release** → **Run workflow**. It creates a draft GitHub
release, builds all matrix platforms (including `aarch64-apple-darwin`), and
uploads signed/notarized artifacts to the draft when secrets are configured.
Publish the draft manually once you've reviewed it.

## 7. Verify the shipped artifact locally

Download the `.app` (or `.dmg`) from the release and run, from a Terminal:

```bash
# Confirms the notarization ticket is stapled to the bundle
xcrun stapler validate /path/to/Handy.app

# Confirms Gatekeeper accepts it for the given path (works offline once stapled)
spctl -a -vv -t install /path/to/Handy.app
```

A successful `stapler validate` prints `The validate action worked!`. A
successful `spctl` run prints `accepted` and `source=Notarized Developer ID`.

CI runs both checks automatically as part of the build (see the "Verify
notarization and staple (macOS)" step in `build.yml`), so a green release run
is already evidence this passed — this section is for spot-checking after
the fact, or after re-signing locally.

## `MACOSX_DEPLOYMENT_TARGET`

Handy/this fork does not set the `MACOSX_DEPLOYMENT_TARGET` environment
variable anywhere in the repo or in CI. The effective minimum macOS version
for the shipped bundle currently comes entirely from one place:

```jsonc
// src-tauri/tauri.conf.json
"bundle": {
  "macOS": {
    "minimumSystemVersion": "10.15"
  }
}
```

Tauri's bundler passes this value through to the compiled binary as its
effective deployment target (functionally equivalent to setting
`MACOSX_DEPLOYMENT_TARGET=10.15` for the Rust/Cargo build), independent of
whatever macOS SDK version the CI runner ships. It is not currently pinned as
an explicit `env:` in any workflow — it is inherited solely from this Tauri
config value.

If you want CI to pin it explicitly (e.g. to catch a future drift between
`tauri.conf.json` and the actual toolchain default, or to override it for a
specific job without editing the shared config), add to the relevant step(s)
in `.github/workflows/build.yml`:

```yaml
env:
  MACOSX_DEPLOYMENT_TARGET: "10.15"
```

This repo deliberately does **not** add that env var today — `10.15` is
sourced from `tauri.conf.json` alone, and duplicating it in CI risks the two
values silently drifting apart. Treat this section as the documented,
single source of truth for that number until/unless CI pinning is added.
