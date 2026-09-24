//! Wire types for the FXRender public API (`public_api/docs/API.md`).
//!
//! Field names mirror `api/app/schemas.py` exactly (both sides speak plain
//! snake_case JSON, so no rename is needed except where Python uses a Rust
//! keyword). Kept intentionally close to the Python source of truth —
//! cross-check there before adding a field.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---- account -----------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageQuota {
    pub plan_id: String,
    pub plan_label: String,
    pub plan_expires_at: Option<DateTime<Utc>>,
    pub quota_bytes: i64,
    pub quota_gb: i64,
    pub used_bytes: i64,
    pub remaining_bytes: i64,
    pub percent: f64,
    pub inputs_bytes: i64,
    pub outputs_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeQuota {
    pub remaining_seconds: i64,
    pub used_seconds: i64,
    pub granted_seconds: i64,
    pub percent_used: f64,
    pub percent_remaining: f64,
    pub exhausted: bool,
    pub warning: bool,
    pub critical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub is_admin: bool,
    pub is_verified: bool,
    pub is_active: bool,
    pub remaining_seconds: i64,
    #[serde(default)]
    pub seconds_granted: i64,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub storage: Option<StorageQuota>,
    /// Computed server-side from `remaining_seconds`/`seconds_granted`.
    pub time: TimeQuota,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

// ---- usage ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeCharge {
    pub id: Uuid,
    pub kind: String,
    pub seconds: i64,
    pub remaining_after: i64,
    pub job_id: Option<Uuid>,
    pub job_filename: Option<String>,
    pub frame: Option<i64>,
    pub payment_id: Option<Uuid>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLedger {
    pub time: TimeQuota,
    pub items: Vec<TimeCharge>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

// ---- assets ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct UploadUrlRequest {
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadUrlResponse {
    pub upload_url: String,
    pub key: String,
    pub expires_in: i64,
    pub content_type: Option<String>,
    pub storage: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetCompleteRequest {
    pub key: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
    /// API uploads are ephemeral by default (not listed, deleted after a
    /// grace period) — set true to keep this one like a normal upload.
    #[serde(default)]
    pub keep_in_library: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: Uuid,
    pub filename: String,
    pub storage_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    #[serde(default = "default_asset_kind")]
    pub kind: String,
    /// "queued" | "inspecting" | "ready" | "warning" | "unreadable" | "failed"
    pub status: String,
    pub inspect_message: Option<String>,
    pub report: Option<serde_json::Value>,
    pub verdict: Option<String>,
    pub blender_saved: Option<String>,
    pub frame_start: Option<i64>,
    pub frame_end: Option<i64>,
    pub fps: Option<f64>,
    pub resolution_x: Option<i64>,
    pub resolution_y: Option<i64>,
    pub engine: Option<String>,
    pub samples: Option<i64>,
    pub scene_name: Option<String>,
    #[serde(default)]
    pub ephemeral: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub frame_count: Option<i64>,
}

fn default_asset_kind() -> String {
    "blend".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetList {
    pub items: Vec<Asset>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

// ---- jobs ------------------------------------------------------------------

/// Mirrors `JobCreate` in `api/app/schemas.py`. Use `JobCreate::for_asset`
/// for the common case; every field has the same default as the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCreate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_key: Option<String>,
    pub blender_version: String,
    pub engine: String,
    pub device: String,
    pub output_format: String,
    pub samples: i64,
    pub resolution_x: i64,
    pub resolution_y: i64,
    pub frame_start: i64,
    pub frame_end: i64,
    pub frame_step: i64,
    pub fps: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    pub make_movie: bool,
}

impl JobCreate {
    /// Sane defaults matching the server (`api/app/schemas.py::JobCreate`) —
    /// override the fields you care about.
    pub fn for_asset(asset_id: Uuid) -> Self {
        Self {
            asset_id: Some(asset_id),
            source_key: None,
            blender_version: "4.5".to_string(),
            engine: "cycles".to_string(),
            device: "gpu".to_string(),
            output_format: "png".to_string(),
            samples: 128,
            resolution_x: 1920,
            resolution_y: 1080,
            frame_start: 1,
            frame_end: 1,
            frame_step: 1,
            fps: 24,
            camera: None,
            make_movie: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub asset_id: Option<Uuid>,
    pub status: String,
    pub filename: String,
    pub blender_version: String,
    pub engine: String,
    pub device: String,
    pub output_format: String,
    pub samples: i64,
    pub resolution_x: i64,
    pub resolution_y: i64,
    pub frame_start: i64,
    pub frame_end: i64,
    pub frame_step: i64,
    pub fps: i64,
    pub camera: Option<String>,
    pub make_movie: bool,
    pub billed_seconds: i64,
    pub worker_id: Option<String>,
    pub provider: Option<String>,
    pub gpu_name: Option<String>,
    pub message: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub chunks_queued: i64,
    #[serde(default)]
    pub chunks_active: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobList {
    pub items: Vec<Job>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    #[serde(default)]
    pub frames_done: i64,
    #[serde(default)]
    pub frames_total: i64,
    #[serde(default)]
    pub chunks_total: i64,
    #[serde(default)]
    pub chunks_done: i64,
    #[serde(default)]
    pub chunks_active: i64,
    #[serde(default)]
    pub workers: i64,
    pub eta_seconds: Option<i64>,
    pub avg_frame_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobQuota {
    pub remaining_seconds: i64,
    pub billed_seconds: i64,
    pub needed_seconds: i64,
    pub needed_high_seconds: i64,
    pub remaining_after_typical: i64,
    pub warn: bool,
    pub exhausted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobFile {
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub key: String,
    pub filename: String,
    pub content_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub frame: Option<i64>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    /// Short-lived signed URL — re-fetch this endpoint once it expires
    /// rather than caching it.
    #[serde(default, alias = "url")]
    pub download_url: Option<String>,
}

/// `GET /jobs/{id}` — `Job` plus files/progress/a live cost estimate.
///
/// Extra arrays (`frames`, `chunks`, `conversions`) are captured so they
/// don't collide with `#[serde(flatten)]` on `job` (the API's top-level
/// `chunks` is a list of chunk objects; `estimate.chunks` is a count).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDetail {
    #[serde(flatten)]
    pub job: Job,
    #[serde(default)]
    pub files: Vec<JobFile>,
    #[serde(default)]
    pub frames: Vec<serde_json::Value>,
    #[serde(default)]
    pub conversions: Vec<Conversion>,
    #[serde(default)]
    pub chunks: Vec<serde_json::Value>,
    #[serde(default)]
    pub progress: Option<JobProgress>,
    #[serde(default)]
    pub estimate: Option<EstimateResponse>,
    #[serde(default)]
    pub quota: Option<JobQuota>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MovieRequest {
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversion {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub kind: String,
    pub format: String,
    pub fps: Option<i64>,
    pub status: String,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub ts: String,
    pub stream: String,
    pub line: String,
    pub frame: Option<i64>,
    pub seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub items: Vec<LogLine>,
}

// ---- estimate ----------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EstimateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<Uuid>,
    pub frame_start: i64,
    pub frame_end: i64,
    pub frame_step: i64,
    pub resolution_x: i64,
    pub resolution_y: i64,
    pub samples: i64,
    pub engine: String,
}

impl EstimateRequest {
    pub fn for_asset(asset_id: Uuid) -> Self {
        Self {
            asset_id: Some(asset_id),
            frame_start: 1,
            frame_end: 1,
            frame_step: 1,
            resolution_x: 1920,
            resolution_y: 1080,
            samples: 128,
            engine: "cycles".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EstimateResponse {
    pub frames: i64,
    pub seconds: i64,
    pub seconds_low: i64,
    pub seconds_high: i64,
    pub seconds_per_frame: f64,
    pub minimum_seconds: i64,
    pub chunks: i64,
    pub cost_usd: f64,
    pub cost_low_usd: f64,
    pub cost_high_usd: f64,
    pub hourly_usd: f64,
    pub note: String,
    #[serde(default)]
    pub remaining_seconds: Option<i64>,
    #[serde(default)]
    pub sufficient: Option<bool>,
    #[serde(default)]
    pub shortfall_seconds: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_detail_accepts_api_chunks_array_and_estimate_count() {
        let json = r#"{
            "id": "11111111-1111-1111-1111-111111111111",
            "asset_id": null,
            "status": "rendering",
            "filename": "hummer.blend",
            "blender_version": "4.5",
            "engine": "cycles",
            "device": "gpu",
            "output_format": "png",
            "samples": 128,
            "resolution_x": 1920,
            "resolution_y": 1080,
            "frame_start": 1,
            "frame_end": 10,
            "frame_step": 1,
            "fps": 24,
            "camera": null,
            "make_movie": false,
            "billed_seconds": 0,
            "worker_id": null,
            "provider": "runpod",
            "gpu_name": "RTX 4090",
            "message": null,
            "started_at": null,
            "completed_at": null,
            "created_at": "2026-09-17T12:00:00Z",
            "updated_at": "2026-09-17T12:00:00Z",
            "chunks_queued": 1,
            "chunks_active": 1,
            "files": [{"filename": "frame_0001.png", "kind": "png", "download_url": "https://example.com/f"}],
            "frames": [{"id": "22222222-2222-2222-2222-222222222222", "frame": 1, "seconds": 1.2, "billed_seconds": 2, "created_at": "2026-09-17T12:00:00Z"}],
            "conversions": [],
            "chunks": [{"id": "33333333-3333-3333-3333-333333333333", "frame_start": 1, "frame_end": 10, "frame_step": 1, "status": "rendering"}],
            "progress": {"frames_done": 1, "frames_total": 10},
            "estimate": {
                "frames": 10,
                "seconds": 120,
                "seconds_low": 90,
                "seconds_high": 180,
                "seconds_per_frame": 12.0,
                "minimum_seconds": 30,
                "chunks": 1,
                "cost_usd": 0.04,
                "cost_low_usd": 0.03,
                "cost_high_usd": 0.06,
                "hourly_usd": 1.1,
                "note": "ok"
            },
            "quota": {
                "remaining_seconds": 900,
                "billed_seconds": 0,
                "needed_seconds": 120,
                "needed_high_seconds": 180,
                "remaining_after_typical": 780,
                "warn": false,
                "exhausted": false
            }
        }"#;
        let detail: JobDetail = serde_json::from_str(json).expect("JobDetail should parse");
        assert_eq!(detail.job.filename, "hummer.blend");
        assert_eq!(detail.job.status, "rendering");
        assert_eq!(detail.files.len(), 1);
        assert_eq!(detail.files[0].filename, "frame_0001.png");
        assert_eq!(detail.chunks.len(), 1);
        assert_eq!(detail.estimate.as_ref().unwrap().chunks, 1);
    }
}
