mod mcp;

use std::io::Write;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use fxrender_core::models::{EstimateRequest, JobCreate, JobFile, MovieRequest};
use fxrender_core::{store, Client, LogEvent};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "fxr", version, about = "FXRender from the command line")]
struct Cli {
    /// Print raw JSON instead of a human-readable summary (for scripting).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Save an API token (from the FXRender dashboard's API Tokens page).
    Login {
        /// Paste the token directly; omit to be prompted (not echoed).
        #[arg(long)]
        token: Option<String>,
        /// Override the API base URL (defaults to https://api.fxrender.com/v1).
        #[arg(long)]
        api_url: Option<String>,
    },
    /// Remove the saved API token.
    Logout,
    /// Show the signed-in account: name, email, plan, GPU-time balance.
    Whoami,
    /// GPU-second ledger — grants and spend.
    Usage {
        #[arg(long, default_value_t = 1)]
        page: i64,
        #[arg(long, default_value_t = 20)]
        page_size: i64,
    },
    /// Upload a .blend/.zip file and register it (does not render it).
    Upload {
        file: PathBuf,
        /// Keep it on the shelf like a web upload — by default, API
        /// uploads are ephemeral (not listed, deleted after a grace
        /// period once any job using them finishes).
        #[arg(long)]
        keep: bool,
    },
    /// Cost/time estimate before spending GPU time.
    Estimate {
        /// An already-uploaded asset id (see `fxr assets list`).
        #[arg(long)]
        asset: Uuid,
        #[arg(long, default_value_t = 1)]
        frame_start: i64,
        #[arg(long, default_value_t = 1)]
        frame_end: i64,
        #[arg(long, default_value_t = 1920)]
        resolution_x: i64,
        #[arg(long, default_value_t = 1080)]
        resolution_y: i64,
        #[arg(long, default_value_t = 128)]
        samples: i64,
    },
    /// Upload (if given a file) and render it.
    Render {
        /// A local file to upload first, or an existing asset id.
        source: String,
        #[arg(long, default_value_t = 1)]
        frame_start: i64,
        #[arg(long, default_value_t = 1)]
        frame_end: i64,
        #[arg(long, default_value_t = 1)]
        frame_step: i64,
        #[arg(long, default_value_t = 1920)]
        resolution_x: i64,
        #[arg(long, default_value_t = 1080)]
        resolution_y: i64,
        #[arg(long, default_value_t = 128)]
        samples: i64,
        #[arg(long, default_value = "cycles")]
        engine: String,
        #[arg(long, default_value = "exr")]
        output_format: String,
        #[arg(long, default_value_t = 24)]
        fps: i64,
        /// Encode a movie from the stills once rendering finishes (free).
        #[arg(long)]
        movie: bool,
        /// Stream logs until the job reaches a terminal status.
        #[arg(long)]
        follow: bool,
        /// Keep the uploaded source on the shelf instead of the default
        /// ephemeral (not listed, deleted after a grace period) behavior.
        /// No effect when `source` is already an asset id.
        #[arg(long)]
        keep: bool,
    },
    /// Manage uploaded source files.
    #[command(subcommand)]
    Assets(AssetsCmd),
    /// Manage render jobs.
    #[command(subcommand)]
    Jobs(JobsCmd),
    /// View or change local settings (API URL).
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Run as a local MCP server on stdio (for Cursor, Claude Desktop, etc.).
    /// Uses the same login as `fxr login`. Do not type into this — a host process starts it.
    Mcp,
}

#[derive(Subcommand)]
enum AssetsCmd {
    List {
        #[arg(long, default_value_t = 1)]
        page: i64,
        #[arg(long, default_value_t = 20)]
        page_size: i64,
    },
    Show {
        id: Uuid,
    },
    Rm {
        id: Uuid,
    },
}

#[derive(Subcommand)]
enum JobsCmd {
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value_t = 1)]
        page: i64,
        #[arg(long, default_value_t = 20)]
        page_size: i64,
    },
    Show {
        id: Uuid,
    },
    Cancel {
        id: Uuid,
    },
    /// Print recent log lines, or stream live with --follow.
    Logs {
        id: Uuid,
        #[arg(long)]
        follow: bool,
        #[arg(long, default_value_t = 400)]
        n: u32,
    },
    /// List output files (each with a short-lived signed download URL).
    Files {
        id: Uuid,
        /// Download matching files into this directory instead of just listing them.
        #[arg(long)]
        download: Option<PathBuf>,
        /// `original` = EXR masters (falls back to converted stills if this
        /// job never wrote EXR). Never includes the source .blend or zip.
        /// `all` includes those sidecars.
        #[arg(long, default_value = "original", value_parser = ["original", "exr", "converted", "movie", "all"])]
        kind: String,
    },
    /// Encode a movie from a completed job's stills (free, no GPU time billed).
    Movie {
        id: Uuid,
        #[arg(long, default_value = "mp4")]
        format: String,
        #[arg(long)]
        fps: Option<i64>,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    Show,
    SetUrl { url: String },
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    // `login`/`logout`/`config`/`mcp` don't need a client up front
    // (MCP builds one per tool call so `tools/list` works before auth).
    match &cli.command {
        Command::Login { token, api_url } => return cmd_login(token.clone(), api_url.clone()),
        Command::Logout => return cmd_logout(),
        Command::Config(sub) => return cmd_config(sub),
        Command::Mcp => return mcp::run().await,
        _ => {}
    }

    let cfg = store::load_config()?;
    let token = store::get_token()?
        .ok_or_else(|| anyhow!("not logged in — run `fxr login` first"))?;
    let client = Client::new(cfg.api_url, token)?;

    match cli.command {
        Command::Whoami => cmd_whoami(&client, cli.json).await,
        Command::Usage { page, page_size } => cmd_usage(&client, page, page_size, cli.json).await,
        Command::Upload { file, keep } => cmd_upload(&client, &file, keep, cli.json).await,
        Command::Estimate {
            asset,
            frame_start,
            frame_end,
            resolution_x,
            resolution_y,
            samples,
        } => {
            let req = EstimateRequest {
                asset_id: Some(asset),
                frame_start,
                frame_end,
                frame_step: 1,
                resolution_x,
                resolution_y,
                samples,
                engine: "cycles".to_string(),
            };
            cmd_estimate(&client, req, cli.json).await
        }
        Command::Render {
            source,
            frame_start,
            frame_end,
            frame_step,
            resolution_x,
            resolution_y,
            samples,
            engine,
            output_format,
            fps,
            movie,
            follow,
            keep,
        } => {
            cmd_render(
                &client,
                source,
                frame_start,
                frame_end,
                frame_step,
                resolution_x,
                resolution_y,
                samples,
                engine,
                output_format,
                fps,
                movie,
                follow,
                keep,
                cli.json,
            )
            .await
        }
        Command::Assets(sub) => cmd_assets(&client, sub, cli.json).await,
        Command::Jobs(sub) => cmd_jobs(&client, sub, cli.json).await,
        Command::Login { .. } | Command::Logout | Command::Config(_) | Command::Mcp => {
            unreachable!()
        }
    }
}

// ---- login / logout / config -------------------------------------------

fn cmd_login(token: Option<String>, api_url: Option<String>) -> Result<()> {
    let token = match token {
        Some(t) => t,
        None => {
            print!("Paste your FXRender API token (fxr_live_...): ");
            std::io::stdout().flush()?;
            rpassword_read()?
        }
    };
    let token = token.trim().to_string();
    if token.is_empty() {
        bail!("no token provided");
    }
    store::set_token(&token)?;
    if let Some(api_url) = api_url {
        let mut cfg = store::load_config()?;
        cfg.api_url = api_url;
        store::save_config(&cfg)?;
    }
    println!("Saved. Try `fxr whoami` to confirm it works.");
    Ok(())
}

/// Minimal no-echo stdin read so we don't need to pull in a whole crate just
/// for this — good enough for a one-time paste at login.
fn rpassword_read() -> Result<String> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

fn cmd_logout() -> Result<()> {
    store::delete_token()?;
    println!("Logged out.");
    Ok(())
}

fn cmd_config(sub: &ConfigCmd) -> Result<()> {
    match sub {
        ConfigCmd::Show => {
            let cfg = store::load_config()?;
            println!("api_url = {}", cfg.api_url);
        }
        ConfigCmd::SetUrl { url } => {
            let mut cfg = store::load_config()?;
            cfg.api_url = url.clone();
            store::save_config(&cfg)?;
            println!("api_url = {url}");
        }
    }
    Ok(())
}

// ---- account -------------------------------------------------------------

async fn cmd_whoami(client: &Client, json: bool) -> Result<()> {
    let me = client.me().await?;
    if json {
        print_json(&me)?;
    } else {
        println!("{}  <{}>", me.name, me.email);
        println!(
            "render time remaining: {}s ({:.1}% used)",
            me.time.remaining_seconds, me.time.percent_used
        );
        if let Some(storage) = &me.storage {
            println!(
                "storage: {} / {} GB used ({:.1}%)",
                storage.used_bytes / 1_000_000_000,
                storage.quota_gb,
                storage.percent
            );
        }
    }
    Ok(())
}

async fn cmd_usage(client: &Client, page: i64, page_size: i64, json: bool) -> Result<()> {
    let ledger = client.usage(page, page_size).await?;
    if json {
        return print_json(&ledger);
    }
    for item in &ledger.items {
        let note = item.note.as_deref().unwrap_or("");
        println!(
            "{}  {:>+6}s  remaining={}s  {}",
            item.created_at.format("%Y-%m-%d %H:%M"),
            -item.seconds,
            item.remaining_after,
            note
        );
    }
    println!("({} of {} total)", ledger.items.len(), ledger.total);
    Ok(())
}

// ---- assets ----------------------------------------------------------------

async fn cmd_upload(client: &Client, file: &PathBuf, keep: bool, json: bool) -> Result<()> {
    let asset = upload_file(client, file, keep).await?;
    if json {
        return print_json(&asset);
    }
    println!("uploaded: {} ({})", asset.filename, asset.id);
    println!("status: {}", asset.status);
    Ok(())
}

async fn upload_file(client: &Client, file: &PathBuf, keep: bool) -> Result<fxrender_core::models::Asset> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("not a valid file path: {}", file.display()))?
        .to_string();
    let content_type = if filename.to_lowercase().ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let bytes = tokio::fs::read(file)
        .await
        .with_context(|| format!("reading {}", file.display()))?;
    let uploaded = client.upload_asset(&filename, content_type, bytes, keep).await?;
    if uploaded.reused {
        eprintln!("already uploaded — reusing asset {} (no bytes sent)", uploaded.asset.id);
    }
    Ok(uploaded.asset)
}

async fn cmd_assets(client: &Client, sub: AssetsCmd, json: bool) -> Result<()> {
    match sub {
        AssetsCmd::List { page, page_size } => {
            let list = client.list_assets(page, page_size).await?;
            if json {
                return print_json(&list);
            }
            for a in &list.items {
                println!("{}  {:<10}  {}", a.id, a.status, a.filename);
            }
            println!("({} of {} total)", list.items.len(), list.total);
        }
        AssetsCmd::Show { id } => {
            let a = client.get_asset(id).await?;
            if json {
                return print_json(&a);
            }
            println!("{}  {}", a.id, a.filename);
            println!("status: {}", a.status);
            if let (Some(fs), Some(fe)) = (a.frame_start, a.frame_end) {
                println!("frames: {fs}-{fe}");
            }
            if let (Some(rx), Some(ry)) = (a.resolution_x, a.resolution_y) {
                println!("resolution: {rx}x{ry}");
            }
        }
        AssetsCmd::Rm { id } => {
            client.delete_asset(id).await?;
            println!("removed {id}");
        }
    }
    Ok(())
}

// ---- estimate / render ---------------------------------------------------

async fn cmd_estimate(client: &Client, req: EstimateRequest, json: bool) -> Result<()> {
    let est = client.estimate(&req).await?;
    if json {
        return print_json(&est);
    }
    println!(
        "{} frame(s), ~{}s ({}-{}s), ${:.2} (${:.2}-${:.2})",
        est.frames, est.seconds, est.seconds_low, est.seconds_high, est.cost_usd, est.cost_low_usd, est.cost_high_usd
    );
    println!("{}", est.note);
    if let Some(sufficient) = est.sufficient {
        if !sufficient {
            println!(
                "insufficient balance — short by {}s",
                est.shortfall_seconds.unwrap_or(0)
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_render(
    client: &Client,
    source: String,
    frame_start: i64,
    frame_end: i64,
    frame_step: i64,
    resolution_x: i64,
    resolution_y: i64,
    samples: i64,
    engine: String,
    output_format: String,
    fps: i64,
    movie: bool,
    follow: bool,
    keep: bool,
    json: bool,
) -> Result<()> {
    let asset_id = match Uuid::parse_str(&source) {
        Ok(id) => id,
        Err(_) => {
            let path = PathBuf::from(&source);
            if !path.is_file() {
                bail!("`{source}` is neither an asset id nor an existing file");
            }
            println!("uploading {}…", path.display());
            let asset = upload_file(client, &path, keep).await?;
            println!("uploaded as {}", asset.id);
            asset.id
        }
    };

    let req = JobCreate {
        asset_id: Some(asset_id),
        source_key: None,
        blender_version: "4.5".to_string(),
        engine,
        device: "gpu".to_string(),
        output_format,
        samples,
        resolution_x,
        resolution_y,
        frame_start,
        frame_end,
        frame_step,
        fps,
        camera: None,
        make_movie: movie,
    };
    let job = client.create_job(&req).await?;
    if json {
        print_json(&job)?;
    } else {
        println!("job {} created (status: {})", job.id, job.status);
    }

    if follow {
        follow_logs(client, job.id).await?;
    }
    Ok(())
}

// ---- jobs ------------------------------------------------------------------

async fn cmd_jobs(client: &Client, sub: JobsCmd, json: bool) -> Result<()> {
    match sub {
        JobsCmd::List { status, page, page_size } => {
            let list = client.list_jobs(page, page_size, status.as_deref()).await?;
            if json {
                return print_json(&list);
            }
            for j in &list.items {
                println!("{}  {:<16}  {}", j.id, j.status, j.filename);
            }
            println!("({} of {} total)", list.items.len(), list.total);
        }
        JobsCmd::Show { id } => {
            let detail = client.get_job(id).await?;
            if json {
                return print_json(&detail);
            }
            let j = &detail.job;
            println!("{}  {}", j.id, j.filename);
            println!("status: {}", j.status);
            if let Some(p) = &detail.progress {
                println!("frames: {}/{}", p.frames_done, p.frames_total);
                if let Some(eta) = p.eta_seconds {
                    println!("eta: {eta}s");
                }
            }
            println!("billed: {}s", j.billed_seconds);
        }
        JobsCmd::Cancel { id } => {
            let job = client.cancel_job(id).await?;
            println!("job {} status: {}", job.id, job.status);
        }
        JobsCmd::Logs { id, follow, n } => {
            if follow {
                follow_logs(client, id).await?;
            } else {
                let lines = client.job_logs(id, n, None).await?;
                for line in lines {
                    print_log_line(&line);
                }
            }
        }
        JobsCmd::Files { id, download, kind } => {
            let files = client.job_files(id).await?;
            let (chosen, note) = select_files(&files, &kind)?;
            if let Some(note) = note {
                eprintln!("{note}");
            }
            if json && download.is_none() {
                return print_json(&chosen);
            }
            if chosen.is_empty() {
                if kind == "original" || kind == "exr" {
                    eprintln!(
                        "no EXR originals on this job — re-render with `--output-format exr`, or pass `--kind converted`"
                    );
                } else {
                    eprintln!("no files matching --kind {kind}");
                }
            }
            for f in &chosen {
                println!(
                    "{:<8} {:<24} {}",
                    f.kind,
                    f.filename,
                    f.size_bytes.map(|b| format!("{b} bytes")).unwrap_or_default()
                );
                if let Some(dir) = &download {
                    if let Some(url) = &f.download_url {
                        download_to(client, dir, &f.filename, url).await?;
                    }
                }
            }
        }
        JobsCmd::Movie { id, format, fps } => {
            let conv = client.job_movie(id, &MovieRequest { format, fps }).await?;
            println!("queued {} conversion (status: {})", conv.format, conv.status);
        }
    }
    Ok(())
}

async fn follow_logs(client: &Client, job_id: Uuid) -> Result<()> {
    let mut stream = Box::pin(client.stream_job_logs(job_id).await?);
    while let Some(event) = stream.next().await {
        match event? {
            LogEvent::Backlog(lines) => {
                for line in lines {
                    print_log_line(&line);
                }
            }
            LogEvent::Line(line) => print_log_line(&line),
            LogEvent::Other => {}
        }
    }
    Ok(())
}

fn print_log_line(line: &fxrender_core::models::LogLine) {
    println!("[{}] {} {}", line.ts, line.stream, line.line);
}

/// Pick which job files to list/download.
///
/// `original` prefers `kind=exr`. If this job has none (CLI used to default
/// `--output-format png`, which the farm stores as `converted`), it falls
/// back to converted stills rather than returning nothing. Source .blend,
/// logs, crash dumps, and zip archives are never included unless `all`.
fn select_files<'a>(files: &'a [JobFile], kind: &str) -> Result<(Vec<&'a JobFile>, Option<&'static str>)> {
    let kind = kind.trim().to_ascii_lowercase();
    match kind.as_str() {
        "all" => Ok((files.iter().collect(), None)),
        "converted" => Ok((
            files
                .iter()
                .filter(|f| f.kind.eq_ignore_ascii_case("converted"))
                .collect(),
            None,
        )),
        "movie" => Ok((
            files
                .iter()
                .filter(|f| f.kind.eq_ignore_ascii_case("movie"))
                .collect(),
            None,
        )),
        "original" | "exr" => {
            let exr: Vec<_> = files
                .iter()
                .filter(|f| f.kind.eq_ignore_ascii_case("exr"))
                .collect();
            if !exr.is_empty() {
                return Ok((exr, None));
            }
            let converted: Vec<_> = files
                .iter()
                .filter(|f| f.kind.eq_ignore_ascii_case("converted"))
                .collect();
            let note = if converted.is_empty() {
                None
            } else {
                Some(
                    "no EXR originals on this job (it was rendered as PNG/JPEG/etc.); showing converted stills. Re-render with --output-format exr for linear masters.",
                )
            };
            Ok((converted, note))
        }
        other => bail!("unknown --kind {other} (original, converted, movie, all)"),
    }
}

async fn download_to(client: &Client, dir: &PathBuf, filename: &str, url: &str) -> Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let bytes = client.download_url(url).await?;
    let path = dir.join(filename);
    tokio::fs::write(&path, &bytes).await?;
    println!("  -> {}", path.display());
    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
