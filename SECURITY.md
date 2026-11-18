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
  the repository secret `TAURI_SIGNING_PRIVATE_KEY`). An update with a missing or wrong
  signature is refused, even if someone could place a file on the download page.
- Updates are only downloaded over HTTPS from this repository's releases, and only after the user clicks.

**Dependencies**
- Every build checks npm packages (`npm audit`) and Rust crates (`cargo audit`, RustSec) for known
  vulnerabilities; a release is not published if a known vulnerability is found. Notices about unmaintained crates are shown as warnings in the build log.
- Dependabot opens weekly update pull requests, and CodeQL scans the code on every change and weekly.

**The app itself**
- The window can only call Kladde's own commands plus checking for/installing updates and
  restarting (Tauri capabilities); it has no general file, shell or network access.
- A strict Content Security Policy: the window only loads Kladde's own files, no remote scripts.
- Outside links open in the normal browser, never inside the app, and only `http`, `https` and `mailto` links are allowed.
- Imported files and calendar data are only parsed as text (CSV, JSON, iCalendar) with size limits; nothing imported is ever run.
- API tokens (Cloudflare) are kept in Windows Credential Manager / macOS Keychain, not in files or backups.
- The data file is written safely (temporary file, then rename) and a damaged file is never overwritten.

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
