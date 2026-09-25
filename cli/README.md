# fxr — FXRender CLI

Command-line client for [FXRender](https://fxrender.com). Upload a `.blend` (or `.zip`), render it on GPUs, stream logs, and download frames from a terminal.

Source: [https://github.com/technoblobs/fxrender-client](https://github.com/technoblobs/fxrender-client)

```bash
git clone https://github.com/technoblobs/fxrender-client.git
cd fxrender-client
```

The CLI and the [desktop app](../gui/README.md) share the same login. Sign in with either one and the other picks it up.

**API:** `https://api.fxrender.com/v1`

---

## 1. Create an access token

Tokens are not created by this CLI. Create them on the website:

1. Open [https://fxrender.com](https://fxrender.com) and sign in.
2. Go to your account’s **API Tokens** page.
3. Create a token. Copy it immediately — it is shown **once**.
4. It looks like `fxr_live_…`. Treat it like a password.

If you lose it, revoke it on the site and create a new one. Then run `fxr login` again.

---

## 2. Requirements

- [Rust](https://rustup.rs/) 1.77 or newer (`rustc` and `cargo` on your `PATH`)
- A C linker, which the Rust installer sets up (see below)
- A working network path to `https://api.fxrender.com`
- macOS, Windows, or Linux (desktop Linux with a secret service for login)

No Blender install is required on your machine. Rendering happens on the farm. You do not need to learn Rust. `cargo` is only the command that compiles `fxr`.

### Install Rust

Use [rustup](https://rustup.rs/). Skip this if `cargo --version` already prints 1.77 or newer.

macOS or Linux, in a terminal:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Accept the defaults. Then open a new terminal so `cargo` is on your `PATH`.

Windows: download and run `rustup-init.exe` from [https://rustup.rs](https://rustup.rs). When it asks for a C++ build tool, accept the Visual Studio Build Tools install. Close the terminal and open a new one.

Check:

```bash
rustc --version
cargo --version
```

Both commands must be found. If the shell says `cargo: command not found`, the terminal was opened before rustup finished. Open a new one.

A linker is required to finish the build:

| OS | What provides it |
|---|---|
| macOS | Xcode Command Line Tools. If the build says `linker cc not found`, run `xcode-select --install`. |
| Windows | The Visual Studio Build Tools that rustup offered. |
| Linux | A C compiler. On Ubuntu or Debian: `sudo apt install build-essential pkg-config`. |

---

## 3. Compile

From the **workspace root** (`fxrender-client/`), not from `cli/`:

```bash
cargo build -p fxr --release
```

The binary is always written to the workspace `target/` directory:

| OS | Path |
|---|---|
| macOS / Linux | `fxrender-client/target/release/fxr` |
| Windows | `fxrender-client\target\release\fxr.exe` |

There is **no** `cli/target/`. If you are inside `cli/` and just built, run:

```bash
../target/release/fxr --help
```

Optional — put it on your `PATH`:

```bash
# macOS / Linux
sudo cp target/release/fxr /usr/local/bin/fxr

# Windows (PowerShell, run from the workspace)
Copy-Item .\target\release\fxr.exe $env:USERPROFILE\bin\fxr.exe
```

Confirm:

```bash
fxr --help
fxr --version
```

---

## 4. Sign in

```bash
fxr login
# paste: fxr_live_...
```

Non-interactive:

```bash
fxr login --token 'fxr_live_...'
fxr login --token 'fxr_live_...' --api-url https://api.fxrender.com/v1
```

Check the account:

```bash
fxr whoami
fxr usage
```

Sign out:

```bash
fxr logout
```

### Where login is stored

| | Location |
|---|---|
| Token | OS credential store (not a file) |
| API URL | config file |

| OS | Config file | Token store |
|---|---|---|
| macOS | `~/Library/Application Support/fxrender-client/config.json` | Keychain, service `fxrender-client` |
| Windows | `%APPDATA%\fxrender-client\config.json` | Credential Manager |
| Linux | `~/.config/fxrender-client/config.json` | Secret Service (GNOME Keyring / KWallet) |

`fxr` never prints the token. `whoami` only shows name, email, and quota.

The desktop app uses this same store. Signing in with either one signs in the other, on that computer.

### Why a keyring, not an encrypted file

The token is a secret. The operating system keyring is the right place on a machine you use by hand:

| OS | What locks the token |
|---|---|
| macOS | Keychain. Service `fxrender-client`, account `default`. |
| Windows | Credential Manager, same service and account. |
| Linux desktop | Secret Service. GNOME Keyring or KWallet provide it. |

`fxr` does not encrypt the token with a key built into the program. That key would ship inside every copy of the binary, so anyone with the file and the binary could read the token. A private file that "only `fxr` can decrypt" is not protection.

`fxr logout` deletes the keyring entry. It does not revoke the token on the website. Revoke a leaked token at [fxrender.com](https://fxrender.com) → API Tokens.

### Run on a server

A server has no unlocked keyring, so `fxr login` cannot save a token there. Do not copy the macOS keychain. Pass the token in the environment for that process. It is not written to disk.

```bash
export FXRENDER_TOKEN='fxr_live_...'
# optional, if you are not using the public API
export FXRENDER_API_URL='https://api.fxrender.com/v1'

fxr whoami
fxr render /path/shot.blend --frame-start 1 --frame-end 1 --output-format png --follow
fxr jobs files <job-id> --download /var/fxrender/out
```

`FXRENDER_TOKEN` is used instead of the keyring when it is set. Unset it, or leave it empty, to use the keyring again. Put the export in the service unit, CI secret, or secret manager that starts the job. Do not commit it, and do not pass it as a command-line argument (`fxr login --token` lands in shell history).

A systemd service can take the token from a root-only file without `fxr` storing anything:

```ini
[Service]
EnvironmentFile=/etc/fxrender/token.env
ExecStart=/usr/local/bin/fxr render /data/shot.blend --frame-start 1 --frame-end 1 --follow
```

`/etc/fxrender/token.env` contains one line, `FXRENDER_TOKEN=fxr_live_...`, and is mode `0600`. That file is the server's secret store. `fxr` only reads the variable.

---

## 5. Typical workflow

```bash
fxr whoami
fxr render shot.blend --frame-start 1 --frame-end 10 --follow
fxr jobs list
fxr jobs files <job-id> --download ./out
```

`render` uploads the file (unless you pass an existing asset UUID), creates the job, and with `--follow` streams logs until it finishes.

Split steps (reuse one upload for several jobs):

```bash
fxr upload shot.blend --keep
fxr estimate --asset <asset-id> --frame-start 1 --frame-end 48
fxr render <asset-id> --frame-start 1 --frame-end 48 --follow
fxr jobs files <job-id> --download ./out
```

---

## 6. Commands

Global flag: `--json` on any command for machine-readable output.

### Account

| Command | What it does |
|---|---|
| `fxr login` | Save a token from [fxrender.com](https://fxrender.com) |
| `fxr logout` | Remove the saved token |
| `fxr whoami` | Name, email, remaining GPU time, storage |
| `fxr usage` | GPU-second ledger (`--page`, `--page-size`) |
| `fxr config show` | Print the API base URL |
| `fxr config set-url URL` | Change the API base URL |

### Upload

```bash
fxr upload shot.blend
fxr upload shot.blend --keep
fxr upload packed.zip --keep
```

Without `--keep`, the upload is **ephemeral**: not listed for long, deleted after a grace period once jobs using it finish. `--keep` stores it on the shelf like a web upload.

Identical bytes (SHA-256) are reused — the CLI will say it skipped the upload.

### Estimate

```bash
fxr estimate --asset <uuid> --frame-start 1 --frame-end 48
fxr estimate --asset <uuid> --frame-end 48 --samples 256 --resolution-x 1920 --resolution-y 1080
```

### Render

`source` is a local `.blend`/`.zip` **or** an existing asset UUID.

```bash
fxr render shot.blend --frame-start 1 --frame-end 10 --follow
fxr render shot.blend --frame-end 48 --output-format exr --movie --follow
fxr render <asset-id> --frame-start 1 --frame-end 10 --keep
```

| Flag | Default | Meaning |
|---|---|---|
| `--frame-start` | `1` | First frame |
| `--frame-end` | `1` | Last frame (a bare `fxr render file.blend` is **one frame**) |
| `--frame-step` | `1` | Step |
| `--resolution-x` / `--resolution-y` | `1920` / `1080` | Resolution |
| `--samples` | `128` | Cycles samples |
| `--engine` | `cycles` | `cycles` or `eevee` |
| `--output-format` | `exr` | `exr`, `png`, `jpeg`, `tiff`, `webp` |
| `--fps` | `24` | Used if you encode a movie |
| `--movie` | off | Encode a movie when stills finish (no extra GPU bill) |
| `--follow` | off | Stream logs until the job ends |
| `--keep` | off | Keep the uploaded source on the shelf |

Blender version defaults to **auto**: `fxr` reads the file header and sends `4.2`, `4.5`, `5.0`, or `5.3-alpha`. A 5.3 file needs `5.3-alpha` (Blender 4.5 cannot open it). Override with `--blender 4.2`, `--blender 4.5`, `--blender 5.0`, or `--blender 5.3`.

Unlike the desktop Watch tab, the CLI does **not** default to the scene’s frame range. Always pass `--frame-start` and `--frame-end` for a sequence.

Frames 1–10 as linear EXR:

```bash
fxr render hummer.blend --frame-start 1 --frame-end 10 --output-format exr --follow
```

PNG instead of EXR:

```bash
fxr render shot.blend --frame-end 48 --output-format png --follow
```

### Assets

```bash
fxr assets list
fxr assets show <asset-id>
fxr assets rm <asset-id>
```

Ephemeral uploads do not stay in `assets list` unless you used `--keep`.

### Jobs

```bash
fxr jobs list
fxr jobs list --status running
fxr jobs show <job-id>
fxr jobs cancel <job-id>
fxr jobs logs <job-id>
fxr jobs logs <job-id> --follow
fxr jobs logs <job-id> --n 800
fxr jobs movie <job-id> --format mp4 --fps 24
```

Job statuses include `queued`, `provisioning`, `downloading`, `rendering`, `converting`, `uploading`, `completed`, `failed`, `cancelled`, `insufficient_quota`.

### Download results

```bash
fxr jobs files <job-id>
fxr jobs files <job-id> --download ./out
```

`--download` creates the directory if needed. Default `--kind original`:

| `--kind` | What is downloaded |
|---|---|
| `original` (default) | EXR masters. If the job has no EXR (it was rendered as PNG/JPEG/etc.), converted stills. **Never** the source `.blend` or zip archive |
| `exr` | Same as `original` |
| `converted` | PNG / JPEG / TIFF / WebP stills only |
| `movie` | Encoded movie only |
| `all` | Everything, including the `.blend` and zip |

The farm labels PNG/JPEG output as `converted`. Only `--output-format exr` produces `kind=exr` originals.

Wait until `fxr jobs show <id>` says `completed` before downloading.

If you did not pass `--movie` at create time:

```bash
fxr jobs movie <job-id> --format mp4
# wait until the conversion finishes, then
fxr jobs files <job-id> --download ./out --kind movie
```

---

## 7. Scripting

```bash
fxr whoami --json
fxr jobs list --json
fxr jobs files <job-id> --json
```

Exit status is `0` on success, `1` on error. Errors print to stderr as `error: …`.

---

## 8. Troubleshooting

**`not logged in — run fxr login first`**  
On a desktop, run `fxr login` with a token from [fxrender.com](https://fxrender.com). On a server, set `FXRENDER_TOKEN` instead. There is no keyring to save into.

**`404: Not Found` on `whoami` / `usage`**  
The token is probably fine; the path is wrong or the public API is not serving `/v1/me`. Confirm with:

```bash
curl -s https://api.fxrender.com/v1/me -H "Authorization: Bearer fxr_live_..."
```

A JSON account object means the token works. Rebuild this CLI if your binary is older than the `/me` (no trailing slash) fix.

**`missing field id` on create-job**  
nginx 301’d `POST /v1/jobs` to GET `/v1/jobs/`. Current `fxr` posts to `/jobs/` and does not follow 301/302/303 as GET. Update the binary if you still see this.

**Download is all PNG, no EXR**  
That job was submitted as PNG (older CLI default). Re-render with `--output-format exr`. For that job, `--kind converted` is the stills; there are no EXR files to fetch.

**Linux: cannot save token**  
A desktop needs GNOME Keyring or KWallet unlocked. A server has neither. Set `FXRENDER_TOKEN` for that process. See [Run on a server](#run-on-a-server).

**Binary not found after build**  
You ran `./target/release/fxr` from `cli/`. Use `../target/release/fxr` or build from the workspace root.

---

## 9. MCP (agents / Cursor / Claude Desktop)

`fxr mcp` is a **local** Model Context Protocol server on stdio. It is the right way to let an AI agent drive FXRender. It is **not** `mcp.fxrender.com` — the agent must be able to read a `.blend` on disk and write frames to a folder.

Claude Desktop and Claude Code setup, including the config files and a first prompt: **[claude-mcp.md](claude-mcp.md)**.

Same login as the CLI and desktop app (`fxr login`, or env `FXRENDER_TOKEN`). Do not pass the token as a tool argument.

Rebuild so you have this binary:

```bash
cargo build -p fxr --release
```

### What the agent can do

| Tool | Role |
|---|---|
| `whoami` | Remaining GPU time, plan |
| `estimate` | Cost/time before spending |
| `render_file` | Upload local path (or `asset_id`) and **submit** a job — returns `job_id`, does not wait |
| `job_status` | Poll until `completed` / `failed` (wait ≥ 5s between calls) |
| `job_logs` | Recent log lines |
| `download_job` | Write originals into a local folder (`kind=original` default; no `.blend`) |
| `cancel_job` | Stop after the current frame |
| `make_movie` | Free encode from finished stills |
| `list_jobs` / `list_assets` | Browse |

Typical agent flow: `whoami` → `estimate` → `render_file` → poll `job_status` → `download_job`.

If `frame_start` / `frame_end` are omitted, `render_file` uses the scene range from inspect (unlike the CLI, which defaults to one frame). Default format is EXR.

### Cursor

Add to Cursor MCP settings (absolute path to **your** `fxr` binary):

```json
{
  "mcpServers": {
    "fxrender": {
      "command": "/absolute/path/to/fxrender-client/target/release/fxr",
      "args": ["mcp"]
    }
  }
}
```

If you prefer not to use the keychain, set `"env": { "FXRENDER_TOKEN": "fxr_live_..." }`. Create that token on [https://fxrender.com](https://fxrender.com) → API Tokens.

### Claude Desktop

`~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or the equivalent on Windows:

```json
{
  "mcpServers": {
    "fxrender": {
      "command": "/absolute/path/to/fxr",
      "args": ["mcp"]
    }
  }
}
```

Restart the host after saving. You should see tools named `render_file`, `job_status`, `download_job`.

### Env overrides

| Variable | Meaning |
|---|---|
| `FXRENDER_TOKEN` | Bearer token (skips keychain) |
| `FXRENDER_API_URL` | Default `https://api.fxrender.com/v1` |

Do not type into `fxr mcp` yourself — it speaks JSON-RPC on stdin/stdout. A host process starts it.

---

## 10. Publish a CLI release

This builds four archives and attaches them to a GitHub Release. You do not build Windows or Linux on your Mac. Pushing a version tag starts `.github/workflows/release-cli.yml`.

The tag builds whatever is already on GitHub. Commit and push the CLI changes first, then:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow produces:

| Archive | Runs on |
|---|---|
| `fxr-macos-aarch64.tar.gz` | Apple Silicon Macs |
| `fxr-macos-x86_64.tar.gz` | Intel Macs |
| `fxr-windows-x86_64.tar.gz` | Windows 10 and 11 (`fxr.exe`) |
| `fxr-linux-x86_64.tar.gz` | Ubuntu and Fedora on 64-bit PCs |

Each archive includes `LICENSE`, this manual, and a `.sha256` file. The Linux file is one binary for both Ubuntu and Fedora. There is no separate `.deb` or `.rpm`.

Watch the run under the repository **Actions** tab. When it is green, the files are on the Releases page: `https://github.com/technoblobs/fxrender-client/releases/tag/v0.1.0`.

To ship a fix, bump `version` in the workspace `Cargo.toml`, commit, tag `v0.1.1`, and push that tag. Do not move an existing tag.

---

## 11. Related

- Desktop app (Watch folders, GUI): [`../gui/README.md`](../gui/README.md)
- Public API reference: `public_api/docs/API.md` in the FXRender server repo
- Site: [https://fxrender.com](https://fxrender.com)
