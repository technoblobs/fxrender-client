# fxrender-client

Source: [https://github.com/technoblobs/fxrender-client](https://github.com/technoblobs/fxrender-client)

CLI and desktop app for [FXRender](https://fxrender.com) — upload a `.blend`, render it on GPUs, and pull down the results from a terminal or a native window.

```bash
git clone https://github.com/technoblobs/fxrender-client.git
cd fxrender-client
```

Create an account and an **API token** on [https://fxrender.com](https://fxrender.com) (API Tokens page). The token is shown once. Both tools talk to `https://api.fxrender.com/v1` and share the same local login.

## Layout

| Path | What it is | Manual |
|---|---|---|
| [`core/`](core) | Shared API client (auth, assets, jobs, estimate, usage, watch engine) | — |
| [`cli/`](cli) | `fxr` command-line tool | **[CLI documentation](cli/README.md)** — setup, compile, every command. **[Claude via MCP](cli/claude-mcp.md)** |
| [`gui/`](gui) | Tauri 2 + React desktop app | **[Desktop user manual](gui/README.md)** — sign-in, Watch folders, build installers |

## Quick build

```bash
# CLI
cargo build -p fxr --release
./target/release/fxr login          # paste token from https://fxrender.com
./target/release/fxr whoami

# Desktop (dev)
cd gui && npm install && npm run tauri dev

# Desktop installers (.dmg / .msi / .deb)
cd gui && npm run tauri build
```

Requires [Rust](https://rustup.rs/) 1.77+. The desktop app also needs [Node.js](https://nodejs.org/) 20+ and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Quick use

```bash
fxr render shot.blend --frame-start 1 --frame-end 10 --follow
fxr jobs files <job-id> --download ./out
```

In the desktop app: **Watch** → **Add folder** → drop a `.blend` → frames land in the output directory you chose.

For AI agents (Cursor, Claude Desktop): `fxr mcp` — local MCP, same login. See [cli/README.md](cli/README.md) §9. Not a hosted `mcp.fxrender.com`.

On a server, do not use `fxr login`. Export `FXRENDER_TOKEN` for the process. The keyring is only for a desktop. See [cli/README.md](cli/README.md) §4.

## License

MIT — see [LICENSE](LICENSE).
