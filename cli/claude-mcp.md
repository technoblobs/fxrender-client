# Use FXRender from Claude (MCP)

`fxr mcp` is a local [Model Context Protocol](https://modelcontextprotocol.io) server. Claude Desktop and Claude Code start it, then call its tools. It is not a website. The chat on claude.ai cannot see a `.blend` on your disk.

Source: [https://github.com/technoblobs/fxrender-client](https://github.com/technoblobs/fxrender-client)

The server uses the same login as the `fxr` command. Create the token at [https://fxrender.com](https://fxrender.com) → API Tokens. It is shown once.

## 1. Build and sign in

From the repository root:

```bash
git clone https://github.com/technoblobs/fxrender-client.git
cd fxrender-client
cargo build -p fxr --release
./target/release/fxr login
./target/release/fxr whoami
```

The binary you will point Claude at is `fxrender-client/target/release/fxr`. On Windows it is `target\release\fxr.exe`.

`whoami` must print your account before you continue. If the keychain prompt fails, skip `fxr login` and put `FXRENDER_TOKEN` in the Claude config in the next step instead.

## 2. Claude Desktop

Quit Claude Desktop completely, then edit the config file:

| System | File |
|---|---|
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |
| Linux | `~/.config/Claude/claude_desktop_config.json` |

Create the file if it does not exist. Merge this into any `mcpServers` you already have. Use the absolute path of the binary you just built:

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

If Claude cannot read the macOS keychain (common when the app is opened from the Dock), pass the token in the config instead of running `fxr login`:

```json
{
  "mcpServers": {
    "fxrender": {
      "command": "/absolute/path/to/fxrender-client/target/release/fxr",
      "args": ["mcp"],
      "env": {
        "FXRENDER_TOKEN": "fxr_live_..."
      }
    }
  }
}
```

Do not commit that file. The token is a secret.

Open Claude Desktop again. In a new chat, the tools menu should list `fxrender`. A first message that checks the connection:

```text
Use the fxrender tools. Call whoami and tell me the remaining GPU time. Do not render anything.
```

## 3. Claude Code

In a terminal, from any directory:

```bash
claude mcp add --transport stdio --scope user fxrender -- /absolute/path/to/fxrender-client/target/release/fxr mcp
```

`--scope user` makes it available in every project. Omit it to add the server only to the current project (`.mcp.json`).

Check it:

```bash
claude mcp list
```

Start `claude` and ask the same `whoami` question as above.

To pass a token without the keychain, add the server from a project `.mcp.json` instead:

```json
{
  "mcpServers": {
    "fxrender": {
      "command": "/absolute/path/to/fxrender-client/target/release/fxr",
      "args": ["mcp"],
      "env": {
        "FXRENDER_TOKEN": "fxr_live_..."
      }
    }
  }
}
```

Keep that file out of git if it contains the token.

## 4. Ask it to render

Give Claude a path on this machine and a folder for the frames. It cannot render a file that exists only in the chat.

```text
Use fxrender. Render frame 1 of /Users/me/Downloads/shot.blend as a PNG.
Poll job_status until it finishes, then download_job into /Users/me/Downloads/mcp-test.
```

`render_file` only submits the job and returns a `job_id`. Claude must poll `job_status` (about every 15 seconds) and then call `download_job`. A GPU may sit in queue for several minutes before the first pixel.

## Tools

| Tool | What it does |
|---|---|
| `whoami` | Account and remaining GPU time |
| `estimate` | Time and cost before a render |
| `render_file` | Upload a local `.blend` or `.zip`, or reuse an `asset_id`, and submit a job |
| `job_status` | Status of that job |
| `job_logs` | Recent log lines |
| `download_job` | Save finished frames into a local folder |
| `cancel_job` | Stop a live job |
| `make_movie` | Encode a movie from the stills (no extra GPU charge) |
| `list_jobs` | Recent jobs |
| `list_assets` | Uploads kept in the library |

## Limits

- `render_file` accepts `blender_version`: `4.2`, `4.5`, `5.0`, or `5.3-alpha` (`5.3` is the same as `5.3-alpha`). If you omit it, `fxr` reads the file header. A file saved by Blender 5.3 must use `5.3-alpha`.
- Engine defaults to **Cycles**. Pass `engine: eevee` only when you want Eevee.
- If you omit the frame range, `render_file` renders the whole scene range from inspect, not a single frame. Say the frame numbers in the prompt.
- Default output is EXR. Ask for `png`, `jpeg`, `tiff`, or `webp` when you want those. Download TIFF/PNG/JPEG with `kind` `converted`. EXR is `kind` `original`.
- The scene must contain a camera. The farm rejects a file that has none.
- Each render spends GPU time on the account from `whoami`.

## If it does not connect

- The `command` path must be absolute. `fxr` on your shell `PATH` is not enough for Claude Desktop.
- Restart Claude Desktop after every config edit. Reloading the window is not enough.
- `whoami` returning `not logged in` means the keychain was empty and `FXRENDER_TOKEN` was not set.
- Do not run `fxr mcp` in a terminal yourself. It speaks JSON-RPC on stdin and will look stuck.
