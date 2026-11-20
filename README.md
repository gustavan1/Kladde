# Kladde

Kladde is a study app for notes, homework and a weekly study plan. It runs on Windows x64 and Apple Silicon Macs.

- Notes, homework and plans are saved on your computer. You can use them offline.
- Optional AI uses Cloudflare Workers AI. On a Mac, you can also sign into ChatGPT for Codex or share selected work with Claude Desktop. See section 4.
- **It only goes online when you use AI, for a calendar link that you add yourself** under "Importér fra skole", **and to check for updates** (it asks GitHub whether a newer version exists; nothing about you is sent).
- **Your data is one file** that you can back up and move to another computer.

---

## For the person using Kladde

### 1. Get the installer

Download the installer directly from the official [Kladde releases](https://github.com/gustavan1/Kladde/releases). Before opening it, verify it against that release using the instructions below.

| Computer | Download |
| --- | --- |
| Windows 10 or 11, x64 | `Kladde_…_x64-setup.exe` |
| Mac with an Apple chip (M1 or newer), macOS 13 or later | `Kladde_…_aarch64.dmg` |

### 2. Install Kladde

**Mac.** After verifying the DMG, open it, drag **Kladde** into **Applications**, eject the disk image and open the app from Applications. This private build uses an ad-hoc signature. If macOS asks you to verify the developer, first try opening Kladde, then go to **System Settings → Privacy & Security → Open Anyway** for Kladde. This applies to this app only. [Apple’s instructions](https://support.apple.com/en-us/102445)

**Windows**

1. Double-click **`Kladde_…_x64-setup.exe`**.
2. If you see **"Windows protected your PC"**, follow the steps in the next section first.
3. Click through the installer. You don't need to be an administrator.
4. On the last page, leave **"Create desktop shortcut"** ticked, then click **Finish**.
5. You now have a **Kladde** icon on the **desktop** and in the **Start menu**.

Windows 10 and 11 already include the small part Kladde needs to show its window (Microsoft Edge WebView2). On a very old PC that doesn't have it, the installer downloads it once.

### "Windows protected your PC"

The Windows installer currently has no publisher certificate. If SmartScreen appears, verify the download first. If verification succeeds, choose **More info** and **Run anyway** for that verified file. If antivirus reports a threat, stop and report the release and filename through GitHub Security.

### Is the download safe?

- **Only download Kladde from the [releases page](https://github.com/gustavan1/Kladde/releases)**.
- **To check that a file is genuine:**
  1. Open PowerShell.
  2. Run `Get-FileHash` on the file, for example `Get-FileHash .\Kladde_1.3.40_x64-setup.exe`.
  3. Compare the result with the line for that file in `SHA256SUMS.txt` on the same release page.
  - On macOS, run `shasum -a 256` with the path to the DMG in Terminal and compare the matching DMG entry.
  - If the hashes differ, discard the download. The release attestation in SECURITY.md can also verify the publishing workflow.
- **Updates are checked before they're installed:** Kladde only installs updates signed with the project's own key.
- **More details:** see [SECURITY.md](SECURITY.md).

### 3. Your data, and how to back it up

On a Mac, use **Finder → Go → Go to Folder** and enter `~/Library/Application Support/Kladde`. The study file is `kladde.json`. On Windows, the same file lives here.

```
%APPDATA%\Kladde\kladde.json
```

To open the folder, press **Windows key + R**, type `%APPDATA%\Kladde` and press **Enter**. The full path is normally `C:\Users\<your name>\AppData\Roaming\Kladde\kladde.json`.

- **Updating Kladde or uninstalling it does not delete this file.**

**Make a backup.** Do one of these regularly, for example every week:

- **In Kladde:** click **"Gem backup"** at the bottom of the window. A save window opens in **Documents**. Save the file there, or on OneDrive or a USB stick. Export keeps your timetable but omits private calendar subscription links. After restoring, open Calendar links and choose **Reconnect link** for each calendar.
- **By hand:** copy `kladde.json` from the folder above to private storage. This full copy includes any private calendar subscription URLs.

**Restore a backup:**

1. Close Kladde.
2. Open the Kladde data folder for your computer.
3. Put your backup file there and rename it to exactly `kladde.json`, replacing the old one.
4. Start Kladde again.

**Move to a new PC:** install Kladde on the new PC and open it once, then close it. Restore your backup as described above.

### Get your assignments from school

1. In Kladde, open **Importér fra skole** and click **Åbn Mit Niels Brock** or **Åbn Ludus**. The school site opens in its own Kladde window.
2. **Log in as usual.** Kladde never sees or stores your password. The school window forgets the login when you close it.
3. **Go to the page with your assignments** (on Mit Niels Brock: **Deadlines**). Kladde finds the list by itself; you don't need to press anything.
4. Back in Kladde, the assignments are listed with ticks. Untick anything you don't want and click **Gem valgte**. Assignments you already have are left unticked.

On a page without a clear list (for example Ludus **Lektier**), press **"Hent afleveringer fra denne side"** in the bottom-right corner. The AI then reads that one page and picks out what you have to hand in and when. This needs AI to be switched on (section 4), and Kladde asks for confirmation before sending the page to your AI provider.

Other ways (a calendar link, a file or copied text) are under **Andre måder** at the bottom of the page.

### 4. AI (optional)

**Cloudflare on both computers.** Open **AI helper → Set up your free account** (Danish: **AI-hjælper → Opsæt din gratis konto**). Each person can use their own free account and quota. The app walks you through these steps.

1. Create a Cloudflare account, verify your email and stay on Workers Free. You can skip adding a domain or deploying a Worker. [Sign up](https://dash.cloudflare.com/sign-up)
2. Copy your Account ID from the Cloudflare dashboard. [Find your Account ID](https://developers.cloudflare.com/fundamentals/account/find-account-and-zone-ids/)
3. In **Workers AI → Use REST API → Create a Workers AI API Token**, select your account and copy the token. A custom token needs Workers AI Read and Edit for that account. [Cloudflare walkthrough](https://developers.cloudflare.com/workers-ai/get-started/rest-api/) · [Manage tokens](https://dash.cloudflare.com/profile/api-tokens)
4. Paste both values into Kladde and choose **Test and use Cloudflare**. The connection test sends “Say OK”. The token is kept in Windows Credential Manager or macOS Keychain. Backups contain study data only.

Kladde uses the hosted `@cf/openai/gpt-oss-20b` model. Workers Free includes 10,000 neurons per account per day, resetting at 00:00 UTC. The number of messages this covers varies with the length of the prompts and replies. [Current pricing](https://developers.cloudflare.com/workers-ai/platform/pricing/)

**ChatGPT on Mac.** In **AI helper → AI settings**, choose **Connect and use ChatGPT** and finish the browser sign-in. The bundled Codex client uses your subscription’s Codex access and limits. Choose an available model in the same panel. To switch back, use **Test and use Cloudflare** with your saved account. Both services support the chat, note tools and project planner.

**Claude on Mac.** Move Kladde to Applications first. Install Claude Desktop and sign in with your Claude account. In **AI helper → Claude Desktop**, choose **Add Kladde to Claude Desktop** and quit and reopen Claude Desktop. Select one saved note or homework item in Kladde and click **Share with Claude**. In Claude Desktop, ask “Read my shared Kladde item and suggest a draft note.” Allow the Kladde tool when prompted. Back in Kladde, review the full draft, then choose **Save as new** or **Discard**. **Stop sharing** removes the local snapshot. Shared text that Claude has already read can remain in the Claude conversation. [Download Claude Desktop](https://claude.com/download)

**What the in-app AI receives.** Note tools send the note you are working on. Project planning sends your description and deadline. Chat sends the question, recent messages for the selected subject and provider, and homework context. Selecting a subject also includes excerpts from its notes. Review proposed text and project steps before saving them.

### Updating and uninstalling

- **New version:** Kladde checks for updates a few seconds after it starts. When there's a new version, a blue bar says **"Kladde 1.3.x er klar"**. Click **Opdatér og genstart**: Kladde downloads the update, installs it and opens again. Your notes and homework are not touched.
  - You can also click **Søg efter opdateringer** at the bottom of the window.
  - Nothing is installed until you click. Each update is signed, and Kladde only installs updates with the right signature.
  - Versions older than 1.3 can't update themselves: install 1.3 (or newer) by hand once, and after that it updates itself. You can also always install a new version by downloading it and running the installer. Your data is kept either way.
- **Uninstall:** open **Settings → Apps → Installed apps → Kladde → Uninstall**.
  - The uninstaller may offer to "delete the application data". That only removes small window settings such as language and your name.
  - Your notes in `%APPDATA%\Kladde` are kept. If you want to remove them too, delete that folder yourself.

### If something goes wrong

- **Nothing happens when I open Kladde.** Kladde only runs once at a time, so it may already be open. Look for it in the taskbar.
- **"Kladde kunne ikke starte".** The data file couldn't be read. Kladde doesn't change the file in that case. Restore a backup (see above). Details are in `%APPDATA%\Kladde\kladde.log`.

---

## For the developer: making a new installer

The **Build Kladde installers** GitHub Actions workflow builds a Windows x64 NSIS installer and an Apple Silicon DMG. Pull requests produce downloadable artifacts. A successful push to `main` publishes both installers as release assets. Use the workflow artifacts to test branches. The manual prerelease option is available on protected `main`.

**Updates reach installed copies automatically.**

- Every push to `main` makes a release with a new version number: 1.3.&lt;build number&gt;. The minor version comes from `src-tauri/tauri.conf.json`, so raise it there for bigger releases (for example to 1.4.0).
- The release includes `latest.json`, which installed copies read through `https://github.com/gustavan1/Kladde/releases/latest/download/latest.json`. They then offer the update.
- Previews (prereleases) are never offered as updates.

**Release signing.** Configure the GitHub environment `release-signing` to allow deployments only from protected `main`. Store the existing updater private key there as `KLADDE_UPDATE_SIGNING_KEY` and its password, if used, as `KLADDE_UPDATE_SIGNING_PASSWORD`. Until then, the release job signs with the repository secret `TAURI_SIGNING_PRIVATE_KEY` and shows a warning in the run. Remove the old repository-level signing secrets once the environment copies are in place. Installed copies trust the matching public key in `src-tauri/tauri.conf.json`.

- Keep a backup of the private key file somewhere safe, for example a password manager.
- If it's lost, installed copies can't verify new updates. You'd then need a new key pair, and everyone would install once by hand.
- Never commit the private key.

### Working on the code

You need [Node.js](https://nodejs.org) (LTS) and [Rust](https://rustup.rs). On Linux you also need the WebKitGTK packages that Tauri lists.

| Command | What it does |
| --- | --- |
| `npm install` | Installs the frontend packages (run once) |
| `npm run app` | Starts Kladde in development mode with live reload |
| `npm test` | Runs the Rust unit tests (import parser, study plan, weekly summary, data file, calendar links, AI helpers) |
| `npm run dist:mac` | Fetches the pinned Codex binary and builds the Apple Silicon DMG on an Apple Silicon Mac |
| `npm run dist:win` | Builds the Windows installer locally (on Windows) into `src-tauri/target/release/bundle/nsis/` |

**Project layout**

- `src/` — the React + Tailwind frontend. `src/lib/api.ts` turns the app's calls into Tauri commands.
- `src-tauri/src/` — the Rust backend:
  - `commands.rs` — one command per feature.
  - `store.rs` — `kladde.json`. It's written safely: first to a temp file, then renamed.
  - `importers.rs` — CSV, semicolon CSV, JSON, copied Lectio tables and `.ics`.
  - `planner.rs` — the study plan and weekly summary.
  - `feeds.rs` — calendar links.
  - `ai.rs` — hosted AI for the chat, note tools and project planner.
  - `codex_client.rs` — ChatGPT sign-in and study requests through the bundled Codex app server.
  - `claude.rs` — Claude Desktop MCP tools, shared snapshots and the draft inbox.
  - `lib.rs` — the window, single instance, outside links and the data folder.
- `.github/workflows/build-windows.yml` — the Windows and Mac builds.

Tests can use `KLADDE_DATA_DIR` for an isolated data folder. On a Mac, run `bash scripts/prepare-mac.sh` before the first development build.
