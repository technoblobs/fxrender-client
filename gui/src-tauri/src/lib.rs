mod watch_engine;

use fxrender_core::models::{EstimateRequest, EstimateResponse, Job, JobCreate, JobDetail, JobList};
use fxrender_core::store;
use fxrender_core::watch::{
    LocationSettings, RenderSpec, WatchFolder, WatchFolderInput, WatchSnapshot,
};
use fxrender_core::{Client, LogEvent, UploadedAsset};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;
use watch_engine::WatchEngine;

fn to_err(e: fxrender_core::Error) -> String {
    e.to_string()
}

fn client() -> Result<Client, String> {
    store::load_client().map_err(to_err)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub api_url: String,
    pub logged_in: bool,
}

fn app_config() -> Result<AppConfig, String> {
    let cfg = store::load_config().map_err(to_err)?;
    let logged_in = store::get_token().map_err(to_err)?.is_some();
    Ok(AppConfig { api_url: cfg.api_url, logged_in })
}

#[tauri::command]
fn get_config() -> Result<AppConfig, String> {
    app_config()
}

#[tauri::command]
fn set_api_url(url: String) -> Result<AppConfig, String> {
    let mut cfg = store::load_config().map_err(to_err)?;
    cfg.api_url = url;
    store::save_config(&cfg).map_err(to_err)?;
    app_config()
}

#[tauri::command]
fn login(token: String) -> Result<AppConfig, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("token is empty".into());
    }
    store::set_token(&token).map_err(to_err)?;
    app_config()
}

#[tauri::command]
fn logout() -> Result<AppConfig, String> {
    store::delete_token().map_err(to_err)?;
    app_config()
}

#[tauri::command]
async fn whoami() -> Result<fxrender_core::models::Me, String> {
    client()?.me().await.map_err(to_err)
}

#[tauri::command]
async fn list_jobs(page: i64, page_size: i64, status: Option<String>) -> Result<JobList, String> {
    client()?
        .list_jobs(page, page_size, status.as_deref())
        .await
        .map_err(to_err)
}

#[tauri::command]
async fn get_job(id: Uuid) -> Result<JobDetail, String> {
    client()?.get_job(id).await.map_err(to_err)
}

#[tauri::command]
async fn cancel_job(id: Uuid) -> Result<Job, String> {
    client()?.cancel_job(id).await.map_err(to_err)
}

#[tauri::command]
async fn list_assets(page: i64, page_size: i64) -> Result<fxrender_core::models::AssetList, String> {
    client()?.list_assets(page, page_size).await.map_err(to_err)
}

/// Reads `path` off disk and runs the full upload handshake (signed URL,
/// PUT bytes, register). `path` comes from the frontend's native file
/// picker (`@tauri-apps/plugin-dialog`), so it's a real filesystem path,
/// not a browser `File` object.
#[tauri::command]
async fn upload_file(path: String, keep: bool) -> Result<UploadedAsset, String> {
    let client = client()?;
    let path = std::path::PathBuf::from(path);
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("not a valid file path")?
        .to_string();
    let content_type = if filename.to_lowercase().ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
    client
        .upload_asset(&filename, content_type, bytes, keep)
        .await
        .map_err(to_err)
}

#[tauri::command]
async fn create_job(req: JobCreate) -> Result<Job, String> {
    client()?.create_job(&req).await.map_err(to_err)
}

#[tauri::command]
async fn estimate(req: EstimateRequest) -> Result<EstimateResponse, String> {
    client()?.estimate(&req).await.map_err(to_err)
}

/// Streams a job's logs to the frontend as `job-log:{job_id}` events (one
/// event per backlog line, then one per new line) until the connection ends.
/// Fire-and-forget from the frontend's point of view — call again if it
/// needs to resubscribe (e.g. after reopening the Jobs page).
#[tauri::command]
async fn tail_job_logs(app: AppHandle, id: Uuid) -> Result<(), String> {
    let client = client()?;
    let mut stream = Box::pin(client.stream_job_logs(id).await.map_err(to_err)?);
    let event_name = format!("job-log:{id}");
    while let Some(event) = stream.next().await {
        match event.map_err(to_err)? {
            LogEvent::Backlog(lines) => {
                for line in lines {
                    let _ = app.emit(&event_name, line);
                }
            }
            LogEvent::Line(line) => {
                let _ = app.emit(&event_name, line);
            }
            LogEvent::Other => {}
        }
    }
    Ok(())
}

#[tauri::command]
fn watch_snapshot(engine: State<WatchEngine>) -> WatchSnapshot {
    engine.snapshot()
}

#[tauri::command]
fn watch_set_enabled(engine: State<WatchEngine>, enabled: bool) -> WatchSnapshot {
    engine.set_global(enabled)
}

#[tauri::command]
fn watch_add(engine: State<WatchEngine>, input: WatchFolderInput) -> Result<WatchSnapshot, String> {
    engine.add_folder(input)
}

#[tauri::command]
fn watch_update(engine: State<WatchEngine>, folder: WatchFolder) -> Result<WatchSnapshot, String> {
    engine.update_folder(folder)
}

#[tauri::command]
fn watch_remove(engine: State<WatchEngine>, id: Uuid) -> Result<WatchSnapshot, String> {
    engine.remove_folder(id)
}

#[tauri::command]
fn watch_scan(engine: State<WatchEngine>, id: Uuid) -> Result<WatchSnapshot, String> {
    engine.scan_now(id)
}

#[tauri::command]
fn watch_process_now(engine: State<WatchEngine>, item_id: Uuid) -> Result<WatchSnapshot, String> {
    engine.process_now(item_id)
}

#[tauri::command]
fn watch_skip(engine: State<WatchEngine>, item_id: Uuid) -> Result<WatchSnapshot, String> {
    engine.skip_item(item_id)
}

#[tauri::command]
fn watch_retry(engine: State<WatchEngine>, item_id: Uuid) -> Result<WatchSnapshot, String> {
    engine.retry_item(item_id)
}

#[tauri::command]
fn watch_write_settings(engine: State<WatchEngine>, id: Uuid, overwrite: bool) -> Result<String, String> {
    engine.write_settings(id, overwrite)
}

#[tauri::command]
fn watch_location_settings(
    engine: State<WatchEngine>,
    id: Uuid,
    relative: String,
) -> Result<LocationSettings, String> {
    engine.location_settings(id, relative)
}

#[tauri::command]
fn watch_save_location_settings(
    engine: State<WatchEngine>,
    id: Uuid,
    relative: String,
    spec: RenderSpec,
) -> Result<LocationSettings, String> {
    engine.save_location_settings(id, relative, spec)
}

#[tauri::command]
fn watch_delete_location_settings(
    engine: State<WatchEngine>,
    id: Uuid,
    relative: String,
) -> Result<LocationSettings, String> {
    engine.delete_location_settings(id, relative)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let engine = WatchEngine::start(app.handle().clone());
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_api_url,
            login,
            logout,
            whoami,
            list_jobs,
            get_job,
            cancel_job,
            list_assets,
            upload_file,
            create_job,
            estimate,
            tail_job_logs,
            watch_snapshot,
            watch_set_enabled,
            watch_add,
            watch_update,
            watch_remove,
            watch_scan,
            watch_process_now,
            watch_skip,
            watch_retry,
            watch_write_settings,
            watch_location_settings,
            watch_save_location_settings,
            watch_delete_location_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the FXRender desktop app");
}
