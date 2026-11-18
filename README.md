# Kladde

Kladde is a study app for notes, homework and a weekly study plan. It runs as a normal Windows program in its own window.

- **Everything stays on your PC.** There's no account, no login and no AI.
- **It doesn't go online on its own.** The only exception is a calendar link that you add yourself under "Importér fra skole".
- **Your data is one file** that you can back up and move to another computer.

---

## For the person using Kladde

### 1. Get the installer

The person who made Kladde for you sends you a file called **`Kladde_1.0.0_x64-setup.exe`** (the numbers may differ). They might send it by email, OneDrive or a USB stick. Save it somewhere easy to find, for example **Downloads**.

### 2. Install Kladde

1. Double-click **`Kladde_…_x64-setup.exe`**.
2. If you see **"Windows protected your PC"**, follow the steps in the next section first.
3. Click through the installer. You don't need to be an administrator.
4. On the last page, leave **"Create desktop shortcut"** ticked, then click **Finish**.
5. You now have a **Kladde** icon on the **desktop** and in the **Start menu**.

Windows 10 and 11 already include the small part Kladde needs to show its window (Microsoft Edge WebView2). On a very old PC that doesn't have it, the installer downloads it once.

### "Windows protected your PC"

You might see a blue box that says **"Windows protected your PC"**. This happens because the installer isn't signed with a paid code-signing certificate. It doesn't mean anything is wrong. To continue:

1. Click **More info**.
2. Click **Run anyway**.

Some antivirus programs may also ask about it the first time; choose to allow it.

### 3. Your data, and how to back it up

All your notes, homework and plans are in **one file**:

```
%APPDATA%\Kladde\kladde.json
```

To open the folder, press **Windows key + R**, type `%APPDATA%\Kladde` and press **Enter**. The full path is normally `C:\Users\<your name>\AppData\Roaming\Kladde\kladde.json`.

- **Updating Kladde or uninstalling it does not delete this file.**

**Make a backup.** Do one of these regularly, for example every week:

- **In Kladde:** click **"Gem backup"** at the bottom of the window. A save window opens in **Documents**. Save the file there, or on OneDrive or a USB stick.
- **By hand:** copy `kladde.json` from the folder above to somewhere safe.

**Restore a backup:**

1. Close Kladde.
2. Open the `%APPDATA%\Kladde` folder.
3. Put your backup file there and rename it to exactly `kladde.json`, replacing the old one.
4. Start Kladde again.

**Move to a new PC:** install Kladde on the new PC and open it once, then close it. Restore your backup as described above.

### Updating and uninstalling

- **New version:** just run the new installer. Your data is kept.
- **Uninstall:** open **Settings → Apps → Installed apps → Kladde → Uninstall**.
  - The uninstaller may offer to "delete the application data". That only removes small window settings such as language and your name.
  - Your notes in `%APPDATA%\Kladde` are kept. If you want to remove them too, delete that folder yourself.

### If something goes wrong

- **Nothing happens when I open Kladde.** Kladde only runs once at a time, so it may already be open. Look for it in the taskbar.
- **"Kladde kunne ikke starte".** The data file couldn't be read. Kladde doesn't change the file in that case. Restore a backup (see above). Details are in `%APPDATA%\Kladde\kladde.log`.

---

## For the developer: making a new installer

The installer is built automatically on GitHub every time you push to `main`.

1. Change the code and push it to `main` on GitHub.
2. Open the repository on GitHub and click the **Actions** tab. The run **"Build Windows installer"** starts by itself and takes about 10–20 minutes.
   - To start a build without pushing, click **Build Windows installer**, then **Run workflow**, then **Run workflow** again.
3. When the run has a green tick, click **Releases** on the repository's main page (in the right-hand column). The newest release is at the top. It's called *Kladde 1.0.0 (build N)*.
4. Under **Assets**, click **`Kladde_1.0.0_x64-setup.exe`** to download it. Releases don't expire.
5. Send that file to your friend.

The repository is private, so your friend can't download from GitHub unless you add them as a collaborator. Sending the file is simpler.

If you raise `"version"` in `src-tauri/tauri.conf.json` (for example to `1.0.1`), the installer and release are named with the new number.

### Working on the code

You need [Node.js](https://nodejs.org) (LTS) and [Rust](https://rustup.rs). On Linux you also need the WebKitGTK packages that Tauri lists.

| Command | What it does |
| --- | --- |
| `npm install` | Installs the frontend packages (run once) |
| `npm run app` | Starts Kladde in development mode with live reload |
| `npm test` | Runs the Rust unit tests (import parser, study plan, weekly summary, data file, calendar links) |
| `npm run dist:win` | Builds the Windows installer locally (on Windows) into `src-tauri/target/release/bundle/nsis/` |

**Project layout**

- `src/` — the React + Tailwind frontend. `src/lib/api.ts` turns the app's calls into Tauri commands.
- `src-tauri/src/` — the Rust backend:
  - `commands.rs` — one command per feature.
  - `store.rs` — `kladde.json`. It's written safely: first to a temp file, then renamed.
  - `importers.rs` — CSV, semicolon CSV, JSON, copied Lectio tables and `.ics`.
  - `planner.rs` — the study plan and weekly summary.
  - `feeds.rs` — calendar links. This is the only code that uses the internet.
  - `lib.rs` — the window, single instance, outside links and the data folder.
- `.github/workflows/build-windows.yml` — the automatic Windows build.

The environment variable `KLADDE_DATA_DIR` makes Kladde use another data folder. It's only meant for tests.
