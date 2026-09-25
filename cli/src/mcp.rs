//! Local stdio MCP server: `fxr mcp`.
//!
//! Same public API and login as the rest of `fxr` (keychain / `FXRENDER_TOKEN`).
//! Tools submit jobs and return immediately — poll `job_status`, then
//! `download_job`. Do not block the MCP session on a GPU render.

use std::path::PathBuf;
use std::time::Duration;

use fxrender_core::models::{EstimateRequest, JobCreate, JobFile, MovieRequest};
use fxrender_core::store;
use fxrender_core::Client;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

const INSTRUCTIONS: &str = "\
FXRender GPU farm for Blender. Create tokens at https://fxrender.com (API Tokens); \
the user signs in with `fxr login` (or FXRENDER_TOKEN). \
Workflow: whoami → estimate (optional) → render_file (returns a job_id, does not wait) → \
job_status every 5+ seconds until status is completed → download_job to a local folder. \
render_file reads a local .blend/.zip path (or an existing asset_id). \
blender_version may be 4.2, 4.5, 5.0, or 5.3-alpha. Omit it to read the file header \
(5.3 files need 5.3-alpha; Blender 4.5 cannot open them). \
Default output is EXR (linear original). PNG/JPEG/etc. are converted stills, not masters. \
download_job kind=original never includes the source .blend. \
Renders spend GPU time on the user's account — check whoami remaining_seconds first. \
Do not pass the API token as a tool argument.";

const MAX_FRAME_SPAN: i64 = 10_000;
const INSPECT_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone)]
pub struct FxrenderMcp {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl FxrenderMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Show the signed-in FXRender account: name, email, remaining GPU seconds, storage. Call this first."
    )]
    async fn whoami(&self) -> Result<String, String> {
        let client = api_client()?;
        let me = client.me().await.map_err(err)?;
        ok(json!({
            "name": me.name,
            "email": me.email,
            "remaining_seconds": me.time.remaining_seconds,
            "used_seconds": me.time.used_seconds,
            "granted_seconds": me.time.granted_seconds,
            "percent_used": me.time.percent_used,
            "exhausted": me.time.exhausted,
            "plan": me.storage.as_ref().map(|s| &s.plan_label),
            "storage_gb_used": me.storage.as_ref().map(|s| s.used_bytes as f64 / 1e9),
            "storage_quota_gb": me.storage.as_ref().map(|s| s.quota_gb),
        }))
    }

    #[tool(
        description = "Estimate GPU seconds and USD cost before submitting a render. Prefer this before render_file. Pass asset_id of an already-uploaded file, or a local path to upload+inspect first."
    )]
    async fn estimate(&self, Parameters(p): Parameters<EstimateArgs>) -> Result<String, String> {
        let client = api_client()?;
        let asset_id = resolve_asset(&client, p.path.as_deref(), p.asset_id.as_deref(), false).await?;
        let mut frame_start = p.frame_start.unwrap_or(1);
        let mut frame_end = p.frame_end.unwrap_or(frame_start);
        if p.frame_start.is_none() || p.frame_end.is_none() {
            if let Ok(asset) = wait_for_asset(&client, asset_id).await {
                if p.frame_start.is_none() {
                    frame_start = asset.frame_start.unwrap_or(1);
                }
                if p.frame_end.is_none() {
                    frame_end = asset.frame_end.unwrap_or(frame_start);
                }
            }
        }
        check_frame_span(frame_start, frame_end)?;
        let req = EstimateRequest {
            asset_id: Some(asset_id),
            frame_start,
            frame_end,
            frame_step: p.frame_step.unwrap_or(1).max(1),
            resolution_x: p.resolution_x.unwrap_or(1920),
            resolution_y: p.resolution_y.unwrap_or(1080),
            samples: p.samples.unwrap_or(128),
            engine: p.engine.unwrap_or_else(|| "cycles".into()),
        };
        let est = client.estimate(&req).await.map_err(err)?;
        ok(json!({
            "asset_id": asset_id,
            "frames": est.frames,
            "seconds": est.seconds,
            "seconds_low": est.seconds_low,
            "seconds_high": est.seconds_high,
            "cost_usd": est.cost_usd,
            "cost_low_usd": est.cost_low_usd,
            "cost_high_usd": est.cost_high_usd,
            "sufficient": est.sufficient,
            "shortfall_seconds": est.shortfall_seconds,
            "remaining_seconds": est.remaining_seconds,
            "note": est.note,
        }))
    }

    #[tool(
        description = "Upload a local .blend/.zip (or reuse asset_id) and submit a render job. Returns job_id immediately — does not wait for the farm. Poll job_status until completed, then download_job. If frame_start/frame_end are omitted, uses the scene range from inspect. blender_version: 4.2, 4.5, 5.0, or 5.3-alpha; omit to use the file header. Default format is exr (originals). Engine defaults to cycles."
    )]
    async fn render_file(&self, Parameters(p): Parameters<RenderArgs>) -> Result<String, String> {
        let client = api_client()?;
        let (asset_id, reused, filename) =
            resolve_asset_full(&client, p.path.as_deref(), p.asset_id.as_deref(), p.keep.unwrap_or(false))
                .await?;
        let asset = wait_for_asset(&client, asset_id).await?;
        let frame_start = p
            .frame_start
            .or(asset.frame_start)
            .unwrap_or(1);
        let frame_end = p
            .frame_end
            .or(asset.frame_end)
            .unwrap_or(frame_start)
            .max(frame_start);
        check_frame_span(frame_start, frame_end)?;
        let output_format = p
            .output_format
            .unwrap_or_else(|| "exr".into())
            .to_ascii_lowercase();
        let mut req = JobCreate::for_asset(asset_id);
        req.blender_version = resolve_blender_version(
            p.blender_version.as_deref(),
            p.path.as_deref(),
            asset.blender_saved.as_deref(),
        )?;
        req.engine = p.engine.unwrap_or_else(|| "cycles".into());
        req.output_format = output_format.clone();
        req.samples = p.samples.unwrap_or(128);
        req.resolution_x = p.resolution_x.unwrap_or(asset.resolution_x.unwrap_or(1920));
        req.resolution_y = p.resolution_y.unwrap_or(asset.resolution_y.unwrap_or(1080));
        req.frame_start = frame_start;
        req.frame_end = frame_end;
        req.frame_step = p.frame_step.unwrap_or(1).max(1);
        req.fps = p.fps.unwrap_or(24);
        req.make_movie = p.movie.unwrap_or(false);
        let job = client.create_job(&req).await.map_err(err)?;
        ok(json!({
            "job_id": job.id,
            "status": job.status,
            "asset_id": asset_id,
            "filename": filename.unwrap_or(job.filename),
            "frame_start": job.frame_start,
            "frame_end": job.frame_end,
            "output_format": output_format,
            "blender_version": req.blender_version,
            "engine": req.engine,
            "reused_upload": reused,
            "note": "Job submitted. Poll job_status at most once every 5 seconds until status is completed, failed, or cancelled. Then call download_job. Do not wait inside this tool.",
        }))
    }

    #[tool(description = "List recent render jobs (id, status, filename, frames, billed seconds).")]
    async fn list_jobs(&self, Parameters(p): Parameters<ListJobsArgs>) -> Result<String, String> {
        let client = api_client()?;
        let list = client
            .list_jobs(p.page.unwrap_or(1), p.page_size.unwrap_or(20), p.status.as_deref())
            .await
            .map_err(err)?;
        let items: Vec<_> = list
            .items
            .iter()
            .map(|j| {
                json!({
                    "job_id": j.id,
                    "status": j.status,
                    "filename": j.filename,
                    "frame_start": j.frame_start,
                    "frame_end": j.frame_end,
                    "output_format": j.output_format,
                    "billed_seconds": j.billed_seconds,
                })
            })
            .collect();
        ok(json!({ "total": list.total, "items": items }))
    }

    #[tool(
        description = "Job status, progress, billed seconds, and a short message. Poll this after render_file. Terminal statuses: completed, failed, cancelled, insufficient_quota."
    )]
    async fn job_status(&self, Parameters(p): Parameters<JobIdArgs>) -> Result<String, String> {
        let client = api_client()?;
        let id = parse_uuid(&p.job_id, "job_id")?;
        let detail = client.get_job(id).await.map_err(err)?;
        let j = &detail.job;
        let mut body = json!({
            "job_id": j.id,
            "filename": j.filename,
            "status": j.status,
            "frame_start": j.frame_start,
            "frame_end": j.frame_end,
            "output_format": j.output_format,
            "billed_seconds": j.billed_seconds,
            "message": j.message,
        });
        if let Some(p) = &detail.progress {
            body["frames_done"] = json!(p.frames_done);
            body["frames_total"] = json!(p.frames_total);
            body["eta_seconds"] = json!(p.eta_seconds);
        }
        let terminal = matches!(
            j.status.to_ascii_lowercase().as_str(),
            "completed" | "failed" | "cancelled" | "canceled" | "insufficient_quota"
        );
        body["done"] = json!(terminal);
        if j.status.eq_ignore_ascii_case("completed") {
            body["next"] = json!("Call download_job with this job_id and a local destination folder.");
        }
        ok(body)
    }

    #[tool(description = "Recent log lines for a job (not a live stream). Use after job_status if you need to see why it failed.")]
    async fn job_logs(&self, Parameters(p): Parameters<LogsArgs>) -> Result<String, String> {
        let client = api_client()?;
        let id = parse_uuid(&p.job_id, "job_id")?;
        let n = p.n.unwrap_or(80).min(400);
        let lines = client.job_logs(id, n, None).await.map_err(err)?;
        let items: Vec<_> = lines
            .iter()
            .map(|l| json!({"ts": l.ts, "stream": l.stream, "line": l.line, "frame": l.frame}))
            .collect();
        ok(json!({ "items": items }))
    }

    #[tool(
        description = "Download render outputs to a local folder. Default kind=original (EXR if present, else converted stills). Never downloads the source .blend or zip unless kind=all. Job should be completed."
    )]
    async fn download_job(&self, Parameters(p): Parameters<DownloadArgs>) -> Result<String, String> {
        let client = api_client()?;
        let id = parse_uuid(&p.job_id, "job_id")?;
        let dest = PathBuf::from(p.dest.trim());
        if dest.as_os_str().is_empty() {
            return Err("dest is required — a local directory path for the frames".into());
        }
        let kind = p.kind.unwrap_or_else(|| "original".into());
        let files = client.job_files(id).await.map_err(err)?;
        let (chosen, note) = select_files(&files, &kind)?;
        if chosen.is_empty() {
            return Err(format!(
                "no files matching kind={kind}. If this job was PNG, use kind=converted. For EXR originals, re-render with output_format=exr."
            ));
        }
        tokio::fs::create_dir_all(&dest)
            .await
            .map_err(|e| format!("creating {}: {e}", dest.display()))?;
        let mut downloaded = Vec::new();
        for f in chosen {
            let Some(url) = f.download_url.as_deref() else {
                continue;
            };
            let name = safe_filename(&f.filename);
            let bytes = client.download_url(url).await.map_err(err)?;
            let path = dest.join(&name);
            tokio::fs::write(&path, &bytes)
                .await
                .map_err(|e| format!("writing {}: {e}", path.display()))?;
            downloaded.push(path.display().to_string());
        }
        ok(json!({
            "job_id": id,
            "dest": dest.display().to_string(),
            "kind": kind,
            "downloaded": downloaded,
            "count": downloaded.len(),
            "note": note,
        }))
    }

    #[tool(description = "Cancel a live render after the current frame. Already-finished frames stay billed.")]
    async fn cancel_job(&self, Parameters(p): Parameters<JobIdArgs>) -> Result<String, String> {
        let client = api_client()?;
        let id = parse_uuid(&p.job_id, "job_id")?;
        let job = client.cancel_job(id).await.map_err(err)?;
        ok(json!({ "job_id": job.id, "status": job.status }))
    }

    #[tool(description = "Queue a free movie encode from a completed job's stills (mp4/webm/mov). Poll job_status, then download_job with kind=movie.")]
    async fn make_movie(&self, Parameters(p): Parameters<MovieArgs>) -> Result<String, String> {
        let client = api_client()?;
        let id = parse_uuid(&p.job_id, "job_id")?;
        let conv = client
            .job_movie(
                id,
                &MovieRequest {
                    format: p.format.unwrap_or_else(|| "mp4".into()),
                    fps: p.fps,
                },
            )
            .await
            .map_err(err)?;
        ok(json!({
            "job_id": id,
            "conversion_id": conv.id,
            "format": conv.format,
            "status": conv.status,
            "note": "Movie encode is free. Poll job_status / download_job kind=movie when ready.",
        }))
    }

    #[tool(description = "List kept source assets on the shelf (ephemeral uploads may not appear).")]
    async fn list_assets(&self, Parameters(p): Parameters<PageArgs>) -> Result<String, String> {
        let client = api_client()?;
        let list = client
            .list_assets(p.page.unwrap_or(1), p.page_size.unwrap_or(20))
            .await
            .map_err(err)?;
        let items: Vec<_> = list
            .items
            .iter()
            .map(|a| {
                json!({
                    "asset_id": a.id,
                    "filename": a.filename,
                    "status": a.status,
                    "frame_start": a.frame_start,
                    "frame_end": a.frame_end,
                    "resolution": a.resolution_x.zip(a.resolution_y).map(|(x, y)| format!("{x}x{y}")),
                    "ephemeral": a.ephemeral,
                })
            })
            .collect();
        ok(json!({ "total": list.total, "items": items }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FxrenderMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("fxr", env!("CARGO_PKG_VERSION"))
                    .with_title("FXRender")
                    .with_website_url("https://fxrender.com"),
            )
            .with_instructions(INSTRUCTIONS)
    }
}

// ---- params ----------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EstimateArgs {
    /// Local .blend or .zip to upload first (optional if asset_id is set).
    path: Option<String>,
    /// Already-uploaded asset UUID.
    asset_id: Option<String>,
    frame_start: Option<i64>,
    frame_end: Option<i64>,
    frame_step: Option<i64>,
    resolution_x: Option<i64>,
    resolution_y: Option<i64>,
    samples: Option<i64>,
    engine: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RenderArgs {
    /// Absolute or cwd-relative path to a .blend or .zip.
    path: Option<String>,
    /// Existing asset UUID (skip upload).
    asset_id: Option<String>,
    /// First frame. Omit to use the scene start from inspect.
    frame_start: Option<i64>,
    /// Last frame. Omit to use the scene end from inspect.
    frame_end: Option<i64>,
    frame_step: Option<i64>,
    resolution_x: Option<i64>,
    resolution_y: Option<i64>,
    samples: Option<i64>,
    /// cycles (default) or eevee.
    engine: Option<String>,
    /// 4.2, 4.5, 5.0, or 5.3-alpha. Omit to read the .blend header.
    blender_version: Option<String>,
    /// exr (original, default), png, jpeg, tiff, webp.
    output_format: Option<String>,
    fps: Option<i64>,
    /// Encode a movie when stills finish (free).
    movie: Option<bool>,
    /// Keep the upload on the shelf. Default false (ephemeral).
    keep: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListJobsArgs {
    status: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct JobIdArgs {
    job_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LogsArgs {
    job_id: String,
    /// Max lines (default 80, max 400).
    n: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DownloadArgs {
    job_id: String,
    /// Local directory to write frames into.
    dest: String,
    /// original (default) | exr | converted | movie | all
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MovieArgs {
    job_id: String,
    format: Option<String>,
    fps: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PageArgs {
    page: Option<i64>,
    page_size: Option<i64>,
}

// ---- helpers ---------------------------------------------------------------

fn resolve_blender_version(
    requested: Option<&str>,
    path: Option<&str>,
    saved: Option<&str>,
) -> Result<String, String> {
    use fxrender_core::blender::{
        blender_version_from_saved, normalize_blender_version, sniff_blend_file,
    };
    if let Some(raw) = requested {
        let choice = normalize_blender_version(raw)?;
        if choice != "auto" {
            return Ok(choice);
        }
    }
    if let Some(path) = path {
        if let Some(found) = sniff_blend_file(std::path::Path::new(path)) {
            return Ok(found);
        }
    }
    Ok(blender_version_from_saved(saved))
}

fn api_client() -> Result<Client, String> {
    let token = store::resolve_token()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "not logged in — run `fxr login` with a token from https://fxrender.com (API Tokens), or set FXRENDER_TOKEN".to_string()
        })?;
    let api_url = store::resolve_api_url().map_err(|e| e.to_string())?;
    Client::new(api_url, token).map_err(err)
}

async fn resolve_asset(
    client: &Client,
    path: Option<&str>,
    asset_id: Option<&str>,
    keep: bool,
) -> Result<Uuid, String> {
    Ok(resolve_asset_full(client, path, asset_id, keep).await?.0)
}

async fn resolve_asset_full(
    client: &Client,
    path: Option<&str>,
    asset_id: Option<&str>,
    keep: bool,
) -> Result<(Uuid, bool, Option<String>), String> {
    if let Some(id) = asset_id.filter(|s| !s.trim().is_empty()) {
        let id = parse_uuid(id, "asset_id")?;
        let asset = client.get_asset(id).await.map_err(err)?;
        return Ok((asset.id, true, Some(asset.filename)));
    }
    let path = path.filter(|s| !s.trim().is_empty()).ok_or_else(|| {
        "pass path (local .blend/.zip) or asset_id".to_string()
    })?;
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(format!("not a file: {}", path.display()));
    }
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "invalid file name".to_string())?
        .to_string();
    let content_type = if filename.to_ascii_lowercase().ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    let uploaded = client
        .upload_asset(&filename, content_type, bytes, keep)
        .await
        .map_err(err)?;
    Ok((uploaded.asset.id, uploaded.reused, Some(filename)))
}

async fn wait_for_asset(client: &Client, asset_id: Uuid) -> Result<fxrender_core::models::Asset, String> {
    let deadline = tokio::time::Instant::now() + INSPECT_TIMEOUT;
    loop {
        let asset = client.get_asset(asset_id).await.map_err(err)?;
        match asset.status.as_str() {
            "ready" | "warning" => return Ok(asset),
            "unreadable" | "failed" => {
                return Err(asset
                    .inspect_message
                    .unwrap_or_else(|| format!("asset {}", asset.status)));
            }
            _ => {
                if tokio::time::Instant::now() > deadline {
                    return Err(format!(
                        "timed out waiting for inspect on {asset_id} (status={})",
                        asset.status
                    ));
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

fn check_frame_span(start: i64, end: i64) -> Result<(), String> {
    if end < start {
        return Err(format!("frame_end ({end}) is before frame_start ({start})"));
    }
    let span = end.saturating_sub(start) + 1;
    if span > MAX_FRAME_SPAN {
        return Err(format!(
            "frame range {start}–{end} is {span} frames; cap is {MAX_FRAME_SPAN}. Split the job."
        ));
    }
    Ok(())
}

fn parse_uuid(s: &str, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(s.trim()).map_err(|_| format!("{name} is not a UUID: {s}"))
}

fn select_files<'a>(files: &'a [JobFile], kind: &str) -> Result<(Vec<&'a JobFile>, Option<&'static str>), String> {
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
                Some("no EXR on this job; downloaded converted stills. Re-render with output_format=exr for originals.")
            };
            Ok((converted, note))
        }
        other => Err(format!("unknown kind {other} (original, converted, movie, all)")),
    }
}

fn safe_filename(name: &str) -> String {
    name.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("file")
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c })
        .collect()
}

fn ok(v: impl Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub async fn run() -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    use rmcp::transport::stdio;

    let server = FxrenderMcp::new();
    let running = server.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}
