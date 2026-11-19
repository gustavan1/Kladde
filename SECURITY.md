# Security

## Reporting a problem

Please report security problems privately via **Security → Report a vulnerability** on
GitHub (not as a public issue). Include what you found and how to reproduce it.

## How Kladde protects its users

**Downloads you can verify**
- Installers are only built by the GitHub Actions workflow in this repository, from code on `main`.
- Every release has `SHA256SUMS.txt` and a signed GitHub build attestation that proves which
  workflow and commit produced each file (see "Check a download" below).
- The workflow uses GitHub Actions pinned to exact commits, and the bundled Codex program
  (Mac) is pinned to one version and checked against a SHA-512 checksum before it is included.

**Updates**
- The in-app updater only installs updates signed with Kladde's private signing key (stored as
  `KLADDE_UPDATE_SIGNING_KEY` in the main-only `release-signing` environment). An update with a missing or wrong
  signature is refused, even if someone could place a file on the download page.
- Kladde checks the HTTPS release manifest on GitHub. The manifest points to release assets. Installing requires a user click and a signature binding the package to the advertised version.

**Dependencies**
- Every build checks npm packages (`npm audit`) and Rust crates (`cargo audit`, RustSec) for known
  vulnerabilities; a release is not published if a known vulnerability is found. Notices about unmaintained crates are shown as warnings in the build log.
- Dependabot opens weekly update pull requests, and CodeQL scans the code on every change and weekly.

**The app itself**
- The main window can call Kladde commands, listen for events, close IPC resources, check/install updates and restart. Rust restricts study commands to the main window. Calendar requests use HTTPS and reject local and special-purpose addresses.
- A strict Content Security Policy: the window only loads Kladde's own files, no remote scripts.
- Outside links from the main window open in the normal browser, never inside the app, and only `http`, `https` and `mailto` links are allowed.
- The school window (Ludus / Mit Niels Brock) has no access to Kladde's commands or data; it can only hand over the page
  the user is on, only on those two sites (checked against the window's real address), and the result is always shown
  as a preview that the user must confirm. The window requests a nonpersistent browser session and permits school sign-in navigation.
  Sending page text to AI requires separate confirmation in a native dialog.
- Imported files and calendar data are only parsed as text (CSV, JSON, iCalendar) with size limits; nothing imported is ever run.
- API tokens (Cloudflare) are kept in Windows Credential Manager / macOS Keychain, not in files or backups.
- Saving uses a flushed temporary file and atomic replacement. Kladde refuses invalid JSON, unsupported schemas, ambiguous record IDs and incompatible fields. Backups use atomic replacement and omit private feed URLs.

## Release signing setup

Restrict the GitHub environment `release-signing` to the protected `main` branch. Store the existing updater private key there as `KLADDE_UPDATE_SIGNING_KEY` and its password, if set, as `KLADDE_UPDATE_SIGNING_PASSWORD`. Use the existing key so installed clients continue trusting updates. Until the environment copy exists, the release job falls back to the repository secret `TAURI_SIGNING_PRIVATE_KEY` and warns in the run. Remove the old repository-level secret once the environment copy has been verified. Build and PR jobs do not use the production key. Only the separate release job signs the tested artifacts after the dependency audits.

## Check a download

**Windows (PowerShell):**

```
Get-FileHash .\Kladde_1.3.40_x64-setup.exe -Algorithm SHA256
```

Compare the result with the line for that file in `SHA256SUMS.txt` on the same release page.

**With the GitHub CLI** (proves the file was built by this repository's workflow):

```
gh attestation verify Kladde_1.3.40_x64-setup.exe --repo gustavan1/Kladde
```

## What is not covered (yet)

The installers are not signed with a paid Windows code-signing certificate or an Apple Developer ID,
so Windows SmartScreen and macOS Gatekeeper show a warning the first time. Signing them would remove
the warning and add a publisher name; it requires a certificate (Windows) and an Apple Developer
membership (Mac).
