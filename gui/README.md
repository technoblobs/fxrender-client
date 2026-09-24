# FXRender desktop app — user manual

Source: [https://github.com/technoblobs/fxrender-client](https://github.com/technoblobs/fxrender-client)

Native app for [FXRender](https://fxrender.com): sign in with an API token, watch folders of `.blend` files, upload assets, and follow jobs — without the browser.

```bash
git clone https://github.com/technoblobs/fxrender-client.git
cd fxrender-client
```

This is a [Tauri 2](https://v2.tauri.app/) + React app. It shares login with the [`fxr` CLI](../cli/README.md): the same config file and the same OS keychain entry.

---

## 1. Create an access token

The app does not use your website password. It uses a personal API token.

1. Open [https://fxrender.com](https://fxrender.com) and sign in.
2. Open your account’s **API Tokens** page.
3. Create a token and copy it immediately — it is shown **once**.
4. It looks like `fxr_live_…`.

If you lose it, revoke it on the site, create a new one, and sign in again in the app.

---

## 2. Requirements

To **run** a packaged build you only need the app installer and a token.

To **compile** from source:

- [Rust](https://rustup.rs/) 1.77+
- [Node.js](https://nodejs.org/) 20+
- [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS
  - macOS: Xcode Command Line Tools
  - Windows: WebView2 (usually already installed on Windows 10/11) and a C++ build toolchain
  - Linux: WebKitGTK and the packages listed in the Tauri Linux guide

---

## 3. Compile

From this `gui/` directory:

```bash
npm install
```

### Development (hot reload)

```bash
npm run tauri dev
```

The window opens against a local Vite server. Sign in with a real token from [fxrender.com](https://fxrender.com); there is no separate “dev login”.

### Packaged installers

From `gui/`:

```bash
npm run tauri build
```

Tauri writes installers under `gui/src-tauri/target/release/bundle/` (and related `target/` paths):

| OS | Typical artifacts |
|---|---|
| macOS | `.dmg`, `.app` |
| Windows | `.msi`, `.exe` (NSIS) |
| Ubuntu / Debian | `.deb` (`.AppImage` when enabled) |

`tauri.conf.json` has `"targets": "all"` for the current platform. You must build each OS **on that OS** (a Mac cannot produce a signed Windows installer).

The CLI is a separate binary. To build that too, from the workspace root:

```bash
cargo build -p fxr --release
```

See [`../cli/README.md`](../cli/README.md).

---

## 4. Sign in

On first launch you get a **Sign in** card.

1. Paste the token from [https://fxrender.com](https://fxrender.com) (**API Tokens**).
2. Leave **API URL** empty unless you have been told to use another host. The default is `https://api.fxrender.com/v1`.
3. Click **Sign in**.

The app calls the API immediately. A bad token is rejected here instead of failing later on every page.

**Sign out** is at the bottom of the sidebar. That removes the token from the OS keychain. It does not revoke the token on the website — do that on [fxrender.com](https://fxrender.com) if the token leaked.

Login is stored the same way as the CLI:

| OS | Config | Token |
|---|---|---|
| macOS | `~/Library/Application Support/fxrender-client/config.json` | Keychain (`fxrender-client`) |
| Windows | `%APPDATA%\fxrender-client\config.json` | Credential Manager |
| Linux | `~/.config/fxrender-client/config.json` | Secret Service |

---

## 5. The four tabs

After sign-in the sidebar has **Jobs**, **Watch**, **Assets**, and **Account**. Your email and **Sign out** sit at the bottom. A green dot on **Watch** means auto-process is on and at least one folder is enabled.

### Jobs

Lists recent render jobs (filename + status). Click a row for:

- Frame range and billed GPU time
- Progress (`frames done / total`, ETA when available)
- Live logs (the pane tails the job)
- **Cancel job** — stops after the current frame; finished frames stay billed

There is no download button on this tab. Finished frames from **Watch** land in the output folder you chose. For other jobs, download with the CLI:

```bash
fxr jobs files <job-id> --download ./out
```

### Assets

Your kept source files (`.blend` / `.zip`).

- **Upload .blend / .zip** — pick a file. Identical content is reused (nothing is re-sent).
- **Keep on shelf** — check this *before* upload if you want the file listed here like a web upload.

Uploads are **ephemeral** by default: not listed for long, deleted after a grace period once jobs using them finish. Watch-folder uploads follow the same rule unless the folder’s render spec has “Keep the .blend in the library” on.

After upload, FXRender inspects the scene (frame range, resolution). Status moves off `queued` / `inspecting` when that report is ready.

This tab does not start a render. Use **Watch** (drop a file, or **Render now**) or the CLI `fxr render`.

### Account

Name, email, remaining render time (bar + percent used), and storage quota / plan if the API returns it. Buy more time on [https://fxrender.com](https://fxrender.com).

### Watch

This is the main desktop workflow: point the app at a directory of shots. When a `.blend` or `.zip` appears, it is uploaded, inspected, rendered, and the **frames** (not the source `.blend`) are saved to an output folder you pick.

---

## 6. Watch folders — step by step

### Add a folder

1. Open the **Watch** tab.
2. Click **Add folder**.
3. **Name** — optional label (defaults from the folder name).
4. **Watch this folder** — Browse to the directory where shots appear.
5. **Save results here** — Browse to where finished frames should go (can be a different disk).
6. **Include subfolders** — scan nested directories too.
7. **When to render** (see below).
8. **Render defaults** — Blender version, engine, format, samples, resolution, frame range, movie, keep-on-shelf.
9. Optionally check **Write these as `settings.json` in the watch folder**.
10. **Start watching**.

### Auto-process (master switch)

The switch at the top of Watch is global:

- **Auto-process on** — enabled folders process files according to their schedule.
- **Paused** — new files are listed only. Nothing is sent to the farm except a file you click **Render now** on.

Each folder also has **Pause folder** / **Resume folder**.

### When to render

| Mode | Behaviour |
|---|---|
| **As files arrive** | Upload and render as soon as a `.blend`/`.zip` is stable on disk |
| **Only in a time window** | Files wait until local clock is inside the window. Overnight ranges wrap midnight (e.g. 22:00–08:00) |
| **Manual only** | List files. Send nothing until **Render now** or **Scan now** |

**Scan now** re-reads the directory (the app also watches and rescans on its own).

### Render defaults vs `settings.json`

Priority, closest wins:

1. `settings.json` in the **same folder as the shot**
2. `settings.json` in a **parent folder** (walking up toward the watch root)
3. The watch folder’s defaults in the app

Uncheck **Prefer settings.json in a shot folder** if you want the app defaults to always win.

You can create or edit `settings.json` without leaving the app:

- **settings.json** on the folder toolbar — root of the watch
- Click a **file** or **subfolder** row → **Render settings**
- **Save settings.json** / **Write settings.json to this folder**
- **Remove settings.json** — that location falls back to a parent or the watch defaults

Example `settings.json` (every field is optional; missing keys inherit):

```json
{
  "blender_version": "4.5",
  "engine": "cycles",
  "samples": 128,
  "resolution_x": 1920,
  "resolution_y": 1080,
  "output_format": "exr",
  "use_scene_frames": true,
  "make_movie": false,
  "keep_asset": false
}
```

To render **only frames 1–10**, turn off “Use the frame range from the .blend” (or set `"use_scene_frames": false`) and set:

```json
{
  "use_scene_frames": false,
  "frame_start": 1,
  "frame_end": 10
}
```

Default in Watch is **use the scene’s frame range**. The CLI is the opposite: it defaults to frame 1 only unless you pass `--frame-start` / `--frame-end`.

### Format: EXR vs PNG

| Format | Farm `kind` | Typical use |
|---|---|---|
| `exr` | `exr` | Linear master (original) |
| `png` / `jpeg` / `tiff` / `webp` | `converted` | Preview / delivery stills |

Watch’s default format in the UI is PNG. Choose **EXR** in the folder spec or `settings.json` if you want originals.

Downloads from Watch skip the source `.blend`, logs, and zip archives. You get stills (and a movie if you asked for one).

### File row actions

| Status | What you can do |
|---|---|
| waiting / listed | **Render now**, **Render settings**, skip |
| uploading / inspecting / rendering / downloading | Wait; live status on the row |
| done | **Open** (output folder), **Retry** |
| failed / skipped | **Retry**, **Render settings** |

**Spec** column: `json` means that shot used a `settings.json`; `defaults` means the watch folder spec.

**Remove folder** stops watching. It does not delete files on disk.

---

## 7. What a Watch render does

For each new `.blend` / `.zip`:

1. Wait until the file size is stable (avoids uploading a half-copied file).
2. Upload (ephemeral unless **Keep the .blend in the library** is on).
3. Wait for inspect (scene frame range, resolution, warnings).
4. Create a job with the merged spec.
5. Wait until the job is `completed`.
6. Download render outputs into **Save results here**.

Activity at the bottom of the folder is a short log of those steps.

GPU time is billed on your [fxrender.com](https://fxrender.com) account. Check **Account** in the app, or `fxr whoami`.

---

## 8. Shared login with the CLI

```
fxr login          # also signs the desktop app in
# or sign in in the window — fxr whoami will work
```

One token, one config. You can Watch in the GUI and download a one-off job with `fxr jobs files` in a terminal.

---

## 9. Troubleshooting

**Sign in fails immediately**  
Token typo, revoked token, or empty paste. Create a new token on [fxrender.com](https://fxrender.com) → **API Tokens**.

**Watch lists files but never uploads**  
Auto-process is paused, the folder is paused, or the schedule is **Manual only** / outside the time window. Use **Render now**, or turn auto-process on.

**Output is PNG, I wanted EXR**  
Folder format is PNG (the Watch default). Set format to EXR in Watch settings or `settings.json` (`"output_format": "exr"`). That does not convert a job that already finished.

**`.blend` appeared in the output folder**  
Older builds downloaded source files too. Current Watch download skips `source` / `log` / `archive`. Update the app.

**`npm run tauri dev` — blank window / port in use**  
Another Vite process may already own port 1420. Stop it, or check the terminal for the URL Tauri expected.

**Linux: cannot save login**  
The token goes in Secret Service. Install and unlock GNOME Keyring or KWallet.

---

## 10. MCP (agents)

The desktop app is **not** an MCP server (MCP is a stdio process, not a window). Use the CLI:

```bash
fxr mcp
```

Same login as this app. See [`../cli/README.md`](../cli/README.md) §9 for Cursor / Claude Desktop config.

---

## 11. Related

- CLI setup, compile, and command reference: [`../cli/README.md`](../cli/README.md)
- Site and billing: [https://fxrender.com](https://fxrender.com)
- Public API: `https://api.fxrender.com/v1` (`/docs` when the public API is running)
