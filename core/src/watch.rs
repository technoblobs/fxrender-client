//! Watch-folder model: persist folders, scan for `.blend`/`.zip`, merge
//! `settings.json`, and run one file through upload → inspect → render →
//! download. The desktop app (and later the CLI) owns the loop that calls
//! these; this module has no file-system watcher of its own.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Local, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::models::{JobCreate, JobFile};
use crate::store;

const ITEMS_CAP: usize = 2000;
const ACTIVITY_CAP: usize = 120;
const STABLE_SECS: i64 = 2;

pub const SETTINGS_FILE: &str = "settings.json";

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "__pycache__",
    ".cache",
    ".Trash",
    ".DS_Store",
];

const TERMINAL_OK: &[&str] = &["completed"];
const TERMINAL_FAIL: &[&str] = &[
    "failed",
    "cancelled",
    "canceled",
    "insufficient_quota",
    "timeout",
    "timed_out",
];

// ---- spec / schedule -----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderSpec {
    #[serde(default = "default_blender")]
    pub blender_version: String,
    #[serde(default = "default_engine")]
    pub engine: String,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_format")]
    pub output_format: String,
    #[serde(default = "default_samples")]
    pub samples: i64,
    #[serde(default = "default_res_x")]
    pub resolution_x: i64,
    #[serde(default = "default_res_y")]
    pub resolution_y: i64,
    /// `None` = take the range from the inspected .blend.
    #[serde(default)]
    pub frame_start: Option<i64>,
    #[serde(default)]
    pub frame_end: Option<i64>,
    #[serde(default = "default_step")]
    pub frame_step: i64,
    #[serde(default = "default_fps")]
    pub fps: i64,
    #[serde(default)]
    pub camera: Option<String>,
    #[serde(default)]
    pub make_movie: bool,
    #[serde(default)]
    pub keep_asset: bool,
    /// When true, ignore `frame_start`/`frame_end` and use the scene.
    #[serde(default = "default_true")]
    pub use_scene_frames: bool,
}

impl Default for RenderSpec {
    fn default() -> Self {
        Self {
            blender_version: default_blender(),
            engine: default_engine(),
            device: default_device(),
            output_format: default_format(),
            samples: default_samples(),
            resolution_x: default_res_x(),
            resolution_y: default_res_y(),
            frame_start: None,
            frame_end: None,
            frame_step: default_step(),
            fps: default_fps(),
            camera: None,
            make_movie: false,
            keep_asset: false,
            use_scene_frames: true,
        }
    }
}

fn default_blender() -> String {
    "4.5".into()
}
fn default_engine() -> String {
    "cycles".into()
}
fn default_device() -> String {
    "gpu".into()
}
fn default_format() -> String {
    "png".into()
}
fn default_samples() -> i64 {
    128
}
fn default_res_x() -> i64 {
    1920
}
fn default_res_y() -> i64 {
    1080
}
fn default_step() -> i64 {
    1
}
fn default_fps() -> i64 {
    24
}
fn default_true() -> bool {
    true
}

/// Loose overlay used for `settings.json` (every field optional).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RenderSpecOverride {
    pub blender_version: Option<String>,
    pub engine: Option<String>,
    pub device: Option<String>,
    pub output_format: Option<String>,
    pub samples: Option<i64>,
    pub resolution_x: Option<i64>,
    pub resolution_y: Option<i64>,
    pub frame_start: Option<i64>,
    pub frame_end: Option<i64>,
    pub frame_step: Option<i64>,
    pub fps: Option<i64>,
    pub camera: Option<String>,
    pub make_movie: Option<bool>,
    pub movie: Option<bool>,
    pub keep_asset: Option<bool>,
    pub keep: Option<bool>,
    pub use_scene_frames: Option<bool>,
}

impl RenderSpecOverride {
    pub fn apply(&self, spec: &mut RenderSpec) {
        if let Some(v) = self.blender_version.clone() {
            spec.blender_version = v;
        }
        if let Some(v) = self.engine.clone() {
            spec.engine = v;
        }
        if let Some(v) = self.device.clone() {
            spec.device = v;
        }
        if let Some(v) = self.output_format.clone() {
            spec.output_format = v;
        }
        if let Some(v) = self.samples {
            spec.samples = v;
        }
        if let Some(v) = self.resolution_x {
            spec.resolution_x = v;
        }
        if let Some(v) = self.resolution_y {
            spec.resolution_y = v;
        }
        if self.frame_start.is_some() {
            spec.frame_start = self.frame_start;
        }
        if self.frame_end.is_some() {
            spec.frame_end = self.frame_end;
        }
        if let Some(v) = self.frame_step {
            spec.frame_step = v;
        }
        if let Some(v) = self.fps {
            spec.fps = v;
        }
        if self.camera.is_some() {
            spec.camera = self.camera.clone();
        }
        if let Some(v) = self.make_movie.or(self.movie) {
            spec.make_movie = v;
        }
        if let Some(v) = self.keep_asset.or(self.keep) {
            spec.keep_asset = v;
        }
        if let Some(v) = self.use_scene_frames {
            spec.use_scene_frames = v;
        } else if self.frame_start.is_some() || self.frame_end.is_some() {
            spec.use_scene_frames = false;
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleMode {
    Always,
    Window,
    Manual,
}

impl Default for ScheduleMode {
    fn default() -> Self {
        Self::Always
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Schedule {
    #[serde(default)]
    pub mode: ScheduleMode,
    /// `"HH:MM"` 24h local time. Used when `mode == Window`.
    #[serde(default = "default_window_start")]
    pub window_start: String,
    #[serde(default = "default_window_end")]
    pub window_end: String,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            mode: ScheduleMode::Always,
            window_start: default_window_start(),
            window_end: default_window_end(),
        }
    }
}

fn default_window_start() -> String {
    "22:00".into()
}
fn default_window_end() -> String {
    "08:00".into()
}

impl Schedule {
    pub fn allows_now(&self) -> bool {
        match self.mode {
            ScheduleMode::Always => true,
            ScheduleMode::Manual => false,
            ScheduleMode::Window => {
                let Some(start) = parse_hhmm(&self.window_start) else {
                    return true;
                };
                let Some(end) = parse_hhmm(&self.window_end) else {
                    return true;
                };
                in_window(Local::now().time(), start, end)
            }
        }
    }
}

fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let s = s.trim();
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .ok()
}

fn in_window(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    if start <= end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

// ---- folders / items -----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchFolder {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
    #[serde(default = "default_true")]
    pub recursive: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub spec: RenderSpec,
    /// Closest `settings.json` between the .blend and the watch root wins.
    #[serde(default = "default_true")]
    pub prefer_settings_json: bool,
    #[serde(default)]
    pub schedule: Schedule,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchFolderInput {
    #[serde(default)]
    pub name: String,
    pub source_dir: String,
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub recursive: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub spec: RenderSpec,
    #[serde(default = "default_true")]
    pub prefer_settings_json: bool,
    #[serde(default)]
    pub schedule: Schedule,
}

impl WatchFolderInput {
    pub fn into_folder(self) -> Result<WatchFolder> {
        let source = PathBuf::from(self.source_dir.trim());
        let output = PathBuf::from(self.output_dir.trim());
        if source.as_os_str().is_empty() {
            return Err(Error::Other("pick a folder to watch".into()));
        }
        if output.as_os_str().is_empty() {
            return Err(Error::Other("pick a folder to save results into".into()));
        }
        if !source.is_dir() {
            return Err(Error::Other(format!(
                "watch folder does not exist: {}",
                source.display()
            )));
        }
        fs::create_dir_all(&output).map_err(|e| {
            Error::Other(format!("creating output folder {}: {e}", output.display()))
        })?;
        let name = if self.name.trim().is_empty() {
            source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Watch")
                .to_string()
        } else {
            self.name.trim().to_string()
        };
        Ok(WatchFolder {
            id: Uuid::new_v4(),
            name,
            source_dir: source,
            output_dir: output,
            recursive: self.recursive,
            enabled: self.enabled,
            spec: self.spec,
            prefer_settings_json: self.prefer_settings_json,
            schedule: self.schedule,
            created_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Discovered,
    Waiting,
    Queued,
    Uploading,
    Inspecting,
    Rendering,
    Downloading,
    Done,
    Failed,
    Skipped,
}

impl ItemStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Uploading | Self::Inspecting | Self::Rendering | Self::Downloading
        )
    }

    pub fn is_open(&self) -> bool {
        matches!(
            self,
            Self::Discovered | Self::Waiting | Self::Queued
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchItem {
    pub id: Uuid,
    pub watch_id: Uuid,
    pub path: PathBuf,
    pub relative: String,
    pub size_bytes: u64,
    pub mtime: i64,
    pub status: ItemStatus,
    #[serde(default)]
    pub spec_source: String,
    #[serde(default)]
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub downloaded_to: Option<PathBuf>,
    #[serde(default)]
    pub files_downloaded: u32,
    #[serde(default = "Utc::now")]
    pub first_seen_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchActivity {
    pub ts: DateTime<Utc>,
    pub watch_id: Option<Uuid>,
    pub item_id: Option<Uuid>,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WatchCounts {
    pub total: u32,
    pub waiting: u32,
    pub active: u32,
    pub done: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchFolderView {
    #[serde(flatten)]
    pub folder: WatchFolder,
    pub allows_now: bool,
    pub counts: WatchCounts,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchSnapshot {
    pub global_enabled: bool,
    pub folders: Vec<WatchFolderView>,
    pub items: Vec<WatchItem>,
    pub activity: Vec<WatchActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WatchFile {
    #[serde(default = "default_true")]
    global_enabled: bool,
    #[serde(default)]
    folders: Vec<WatchFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ItemsFile {
    #[serde(default)]
    items: Vec<WatchItem>,
}

// ---- persistence ---------------------------------------------------------

fn watches_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("watches.json"))
}

fn items_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("watch_items.json"))
}

pub fn load_state() -> Result<(bool, Vec<WatchFolder>, Vec<WatchItem>)> {
    let wf = watches_path()?;
    let (global, folders) = if wf.exists() {
        let text = fs::read_to_string(&wf)
            .map_err(|e| Error::Other(format!("reading {}: {e}", wf.display())))?;
        let parsed: WatchFile = serde_json::from_str(&text).unwrap_or(WatchFile {
            global_enabled: true,
            folders: vec![],
        });
        (parsed.global_enabled, parsed.folders)
    } else {
        (true, vec![])
    };
    let ip = items_path()?;
    let items = if ip.exists() {
        let text = fs::read_to_string(&ip)
            .map_err(|e| Error::Other(format!("reading {}: {e}", ip.display())))?;
        serde_json::from_str::<ItemsFile>(&text)
            .unwrap_or_default()
            .items
    } else {
        vec![]
    };
    Ok((global, folders, items))
}

pub fn save_folders(global_enabled: bool, folders: &[WatchFolder]) -> Result<()> {
    let path = watches_path()?;
    let body = WatchFile {
        global_enabled,
        folders: folders.to_vec(),
    };
    fs::write(&path, serde_json::to_string_pretty(&body)?)
        .map_err(|e| Error::Other(format!("writing {}: {e}", path.display())))
}

pub fn save_items(items: &[WatchItem]) -> Result<()> {
    let path = items_path()?;
    let body = ItemsFile {
        items: items.to_vec(),
    };
    fs::write(&path, serde_json::to_string_pretty(&body)?)
        .map_err(|e| Error::Other(format!("writing {}: {e}", path.display())))
}

pub fn example_settings_json(spec: &RenderSpec) -> String {
    let mut v = serde_json::json!({
        "blender_version": spec.blender_version,
        "engine": spec.engine,
        "samples": spec.samples,
        "resolution_x": spec.resolution_x,
        "resolution_y": spec.resolution_y,
        "output_format": spec.output_format,
        "fps": spec.fps,
        "make_movie": spec.make_movie,
        "use_scene_frames": spec.use_scene_frames,
    });
    if !spec.use_scene_frames {
        if let Some(s) = spec.frame_start {
            v["frame_start"] = s.into();
        }
        if let Some(e) = spec.frame_end {
            v["frame_end"] = e.into();
        }
        v["frame_step"] = spec.frame_step.into();
    }
    serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into())
}

pub fn write_settings_json(dir: &Path, spec: &RenderSpec, overwrite: bool) -> Result<PathBuf> {
    let path = dir.join(SETTINGS_FILE);
    if path.exists() && !overwrite {
        return Err(Error::Other(format!(
            "{} already exists — pass overwrite to replace it",
            path.display()
        )));
    }
    fs::write(&path, example_settings_json(spec))
        .map_err(|e| Error::Other(format!("writing {}: {e}", path.display())))?;
    Ok(path)
}

/// Render settings.json for one directory inside a watch folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationSettings {
    /// `.` for the watch root, otherwise a posix-style relative dir.
    pub relative_dir: String,
    pub label: String,
    pub path: String,
    pub exists: bool,
    /// Effective spec (watch defaults + every settings.json from the root
    /// down to this folder).
    pub spec: RenderSpec,
    /// Relative dir of a parent `settings.json`, or `None` if only UI defaults apply.
    pub inherited_from: Option<String>,
}

/// Map a UI relative path (file or folder) onto a directory inside `source_dir`.
pub fn settings_dir_for(source_dir: &Path, relative: &str) -> Result<(PathBuf, String)> {
    let source = source_dir.canonicalize().map_err(|e| {
        Error::Other(format!("watch folder {}: {e}", source_dir.display()))
    })?;
    let rel = relative.replace('\\', "/");
    let rel = rel.trim().trim_start_matches('/').trim_end_matches('/');
    let joined = if rel.is_empty() || rel == "." {
        source.clone()
    } else {
        if rel.split('/').any(|p| p == ".." || p == ".") {
            return Err(Error::Other("invalid settings path".into()));
        }
        source.join(rel)
    };
    let target = if joined.is_file() {
        joined
            .parent()
            .ok_or_else(|| Error::Other("invalid file path".into()))?
            .to_path_buf()
    } else {
        joined
    };
    let canon = target.canonicalize().map_err(|e| {
        Error::Other(format!("{}: {e}", target.display()))
    })?;
    if !canon.starts_with(&source) {
        return Err(Error::Other("settings path is outside the watch folder".into()));
    }
    let rel_dir = relative_to(&canon, &source);
    let rel_dir = if rel_dir.is_empty() {
        ".".to_string()
    } else {
        rel_dir
    };
    Ok((canon, rel_dir))
}

fn apply_settings_file(spec: &mut RenderSpec, path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(over) = serde_json::from_str::<RenderSpecOverride>(&text) else {
        return false;
    };
    over.apply(spec);
    true
}

pub fn location_settings(folder: &WatchFolder, relative: &str) -> Result<LocationSettings> {
    let (dir, rel_dir) = settings_dir_for(&folder.source_dir, relative)?;
    let path = dir.join(SETTINGS_FILE);
    let exists = path.is_file();

    let mut spec = folder.spec.clone();
    let mut inherited_from: Option<String> = None;
    // Root → this folder, same order as resolve_spec.
    let mut chain = Vec::new();
    let mut cur = Some(dir.as_path());
    let source = folder.source_dir.canonicalize().unwrap_or_else(|_| folder.source_dir.clone());
    while let Some(d) = cur {
        chain.push(d.to_path_buf());
        if d == source.as_path() {
            break;
        }
        cur = d.parent();
    }
    chain.reverse();
    for d in &chain {
        let p = d.join(SETTINGS_FILE);
        if apply_settings_file(&mut spec, &p) {
            let r = relative_to(d, &source);
            inherited_from = Some(if r.is_empty() { ".".into() } else { r });
        }
    }
    // If this folder has its own file, it's not "inherited".
    if exists {
        inherited_from = None;
    }

    let label = if rel_dir == "." {
        "watch folder root".to_string()
    } else {
        format!("{rel_dir}/")
    };
    Ok(LocationSettings {
        relative_dir: rel_dir,
        label,
        path: path.display().to_string(),
        exists,
        spec,
        inherited_from,
    })
}

pub fn save_location_settings(
    folder: &WatchFolder,
    relative: &str,
    spec: &RenderSpec,
) -> Result<PathBuf> {
    let (dir, _) = settings_dir_for(&folder.source_dir, relative)?;
    fs::create_dir_all(&dir)
        .map_err(|e| Error::Other(format!("creating {}: {e}", dir.display())))?;
    write_settings_json(&dir, spec, true)
}

pub fn delete_location_settings(folder: &WatchFolder, relative: &str) -> Result<bool> {
    let (dir, _) = settings_dir_for(&folder.source_dir, relative)?;
    let path = dir.join(SETTINGS_FILE);
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path)
        .map_err(|e| Error::Other(format!("removing {}: {e}", path.display())))?;
    Ok(true)
}

// ---- scan / spec resolve -------------------------------------------------

pub fn is_render_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if name.starts_with('.') {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".blend") || lower.ends_with(".zip")
}

fn skip_dir_name(name: &str) -> bool {
    SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s))
}

pub fn scan_sources(folder: &WatchFolder) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(&folder.source_dir, folder, &mut out);
    out
}

fn walk(dir: &Path, folder: &WatchFolder, out: &mut Vec<PathBuf>) {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if skip_dir_name(name) {
                continue;
            }
            if let (Ok(p), Ok(outp)) = (path.canonicalize(), folder.output_dir.canonicalize()) {
                if p == outp || p.starts_with(&outp) {
                    continue;
                }
            }
            if folder.recursive {
                walk(&path, folder, out);
            }
            continue;
        }
        if is_render_source(&path) {
            out.push(path);
        }
    }
}

pub fn relative_to(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

pub fn file_meta(path: &Path) -> Option<(u64, i64)> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some((size, mtime))
}

/// Merge UI defaults with `settings.json` files from the watch root down
/// to the .blend's directory. Closest file wins.
pub fn resolve_spec(folder: &WatchFolder, blend_path: &Path) -> (RenderSpec, String) {
    let mut spec = folder.spec.clone();
    let mut source = "ui".to_string();
    if !folder.prefer_settings_json {
        return (spec, source);
    }
    let mut dirs = Vec::new();
    let mut cur = blend_path.parent();
    while let Some(dir) = cur {
        dirs.push(dir.to_path_buf());
        if dir == folder.source_dir {
            break;
        }
        cur = dir.parent();
    }
    dirs.reverse();
    for dir in dirs {
        let p = dir.join(SETTINGS_FILE);
        let Ok(text) = fs::read_to_string(&p) else {
            continue;
        };
        let Ok(over) = serde_json::from_str::<RenderSpecOverride>(&text) else {
            continue;
        };
        over.apply(&mut spec);
        source = SETTINGS_FILE.to_string();
    }
    (spec, source)
}

pub fn item_is_stable(item: &WatchItem) -> bool {
    if item.size_bytes == 0 {
        return false;
    }
    let age = Utc::now()
        .signed_duration_since(item.first_seen_at)
        .num_seconds();
    age >= STABLE_SECS
}

pub fn folder_can_process(global_enabled: bool, folder: &WatchFolder) -> bool {
    global_enabled && folder.enabled && folder.schedule.allows_now()
}

pub fn counts_for(watch_id: Uuid, items: &[WatchItem]) -> WatchCounts {
    let mut c = WatchCounts::default();
    for it in items.iter().filter(|i| i.watch_id == watch_id) {
        c.total += 1;
        match it.status {
            ItemStatus::Done => c.done += 1,
            ItemStatus::Failed => c.failed += 1,
            s if s.is_active() => c.active += 1,
            ItemStatus::Discovered | ItemStatus::Waiting | ItemStatus::Queued => c.waiting += 1,
            _ => {}
        }
    }
    c
}

pub fn snapshot(
    global_enabled: bool,
    folders: &[WatchFolder],
    items: &[WatchItem],
    activity: &[WatchActivity],
) -> WatchSnapshot {
    let folders = folders
        .iter()
        .map(|f| WatchFolderView {
            allows_now: f.schedule.allows_now(),
            counts: counts_for(f.id, items),
            folder: f.clone(),
        })
        .collect();
    WatchSnapshot {
        global_enabled,
        folders,
        items: items.to_vec(),
        activity: activity.to_vec(),
    }
}

pub fn push_activity(
    activity: &mut Vec<WatchActivity>,
    watch_id: Option<Uuid>,
    item_id: Option<Uuid>,
    kind: &str,
    message: impl Into<String>,
) {
    activity.push(WatchActivity {
        ts: Utc::now(),
        watch_id,
        item_id,
        kind: kind.into(),
        message: message.into(),
    });
    if activity.len() > ACTIVITY_CAP {
        let drop_n = activity.len() - ACTIVITY_CAP;
        activity.drain(0..drop_n);
    }
}

pub fn cap_items(items: &mut Vec<WatchItem>) {
    if items.len() <= ITEMS_CAP {
        return;
    }
    // Drop oldest *done/skipped* first so in-flight work is kept.
    let extra = items.len() - ITEMS_CAP;
    let mut removed = 0;
    items.retain(|it| {
        if removed >= extra {
            return true;
        }
        if matches!(it.status, ItemStatus::Done | ItemStatus::Skipped) {
            removed += 1;
            false
        } else {
            true
        }
    });
}

pub fn new_item(folder: &WatchFolder, path: PathBuf, size: u64, mtime: i64) -> WatchItem {
    let relative = relative_to(&path, &folder.source_dir);
    WatchItem {
        id: Uuid::new_v4(),
        watch_id: folder.id,
        path,
        relative,
        size_bytes: size,
        mtime,
        status: ItemStatus::Discovered,
        spec_source: "ui".into(),
        job_id: None,
        error: None,
        downloaded_to: None,
        files_downloaded: 0,
        first_seen_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// Merge a fresh directory listing into `items`. Returns how many new files
/// were added.
pub fn apply_scan(folder: &WatchFolder, items: &mut Vec<WatchItem>) -> usize {
    let found = scan_sources(folder);
    let mut added = 0;
    let mut seen = Vec::with_capacity(found.len());
    for path in found {
        let Some((size, mtime)) = file_meta(&path) else {
            continue;
        };
        seen.push(path.clone());
        if let Some(existing) = items
            .iter_mut()
            .find(|i| i.watch_id == folder.id && i.path == path)
        {
            if existing.status.is_active() {
                continue;
            }
            if existing.size_bytes != size || existing.mtime != mtime {
                existing.size_bytes = size;
                existing.mtime = mtime;
                existing.first_seen_at = Utc::now();
                existing.updated_at = Utc::now();
                existing.error = None;
                existing.job_id = None;
                existing.downloaded_to = None;
                existing.files_downloaded = 0;
                if existing.status != ItemStatus::Skipped {
                    existing.status = ItemStatus::Discovered;
                } else {
                    // A skipped file that changed is a new shot.
                    existing.status = ItemStatus::Discovered;
                }
            }
            continue;
        }
        items.push(new_item(folder, path, size, mtime));
        added += 1;
    }
    // Files that vanished and aren't in-flight: drop them if they never ran.
    items.retain(|i| {
        if i.watch_id != folder.id {
            return true;
        }
        if i.status.is_active() || matches!(i.status, ItemStatus::Done | ItemStatus::Failed) {
            return true;
        }
        seen.iter().any(|p| p == &i.path)
    });
    cap_items(items);
    added
}

// ---- run one file --------------------------------------------------------

#[derive(Debug, Clone)]
pub enum RunProgress {
    Uploading,
    Inspecting { asset_id: Uuid },
    Rendering { job_id: Uuid },
    Downloading { job_id: Uuid },
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub job_id: Uuid,
    pub dest: PathBuf,
    pub files_downloaded: u32,
}

pub async fn run_item(
    client: &Client,
    path: &Path,
    output_root: &Path,
    relative: &str,
    spec: &RenderSpec,
    mut progress: impl FnMut(RunProgress),
) -> Result<RunOutcome> {
    progress(RunProgress::Uploading);

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Other(format!("not a valid file: {}", path.display())))?
        .to_string();
    let content_type = if filename.to_ascii_lowercase().ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| Error::Other(format!("reading {}: {e}", path.display())))?;
    let uploaded = client
        .upload_asset(&filename, content_type, bytes, spec.keep_asset)
        .await?;
    let asset_id = uploaded.asset.id;

    progress(RunProgress::Inspecting { asset_id });
    let asset = wait_for_asset(client, asset_id).await?;

    let (frame_start, frame_end) = if spec.use_scene_frames {
        let start = asset.frame_start.unwrap_or(1);
        let end = asset.frame_end.unwrap_or(start);
        (start, end)
    } else {
        let start = spec.frame_start.or(asset.frame_start).unwrap_or(1);
        let end = spec.frame_end.or(asset.frame_end).unwrap_or(start);
        (start, end.max(start))
    };

    let mut req = JobCreate::for_asset(asset_id);
    req.blender_version = spec.blender_version.clone();
    req.engine = spec.engine.clone();
    req.device = spec.device.clone();
    req.output_format = spec.output_format.clone();
    req.samples = spec.samples;
    req.resolution_x = spec.resolution_x;
    req.resolution_y = spec.resolution_y;
    req.frame_start = frame_start;
    req.frame_end = frame_end;
    req.frame_step = spec.frame_step.max(1);
    req.fps = spec.fps;
    req.camera = spec.camera.clone();
    req.make_movie = spec.make_movie;

    let job = client.create_job(&req).await?;
    progress(RunProgress::Rendering { job_id: job.id });
    wait_for_job(client, job.id).await?;

    progress(RunProgress::Downloading { job_id: job.id });
    let dest = output_dir_for(output_root, relative);
    tokio::fs::create_dir_all(&dest)
        .await
        .map_err(|e| Error::Other(format!("creating {}: {e}", dest.display())))?;
    let n = download_job_files(client, job.id, &dest).await?;

    Ok(RunOutcome {
        job_id: job.id,
        dest,
        files_downloaded: n,
    })
}

fn output_dir_for(output_root: &Path, relative: &str) -> PathBuf {
    let rel = Path::new(relative);
    let stem = rel.with_extension("");
    output_root.join(stem)
}

async fn wait_for_asset(client: &Client, asset_id: Uuid) -> Result<crate::models::Asset> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    loop {
        let asset = client.get_asset(asset_id).await?;
        match asset.status.as_str() {
            "ready" | "warning" => return Ok(asset),
            "unreadable" | "failed" => {
                let msg = asset
                    .inspect_message
                    .unwrap_or_else(|| format!("asset {}", asset.status));
                return Err(Error::Other(msg));
            }
            _ => {
                if tokio::time::Instant::now() > deadline {
                    return Err(Error::Other(
                        "timed out waiting for scene inspect — is the inspect worker running?"
                            .into(),
                    ));
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn wait_for_job(client: &Client, job_id: Uuid) -> Result<()> {
    loop {
        let detail = client.get_job(job_id).await?;
        let status = detail.job.status.to_lowercase();
        if TERMINAL_OK.contains(&status.as_str()) {
            // Stills can land a moment after `completed`. Source/log rows
            // don't count — wait until a real output exists.
            if !detail.files.iter().any(is_render_output) {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            return Ok(());
        }
        if TERMINAL_FAIL.contains(&status.as_str()) {
            let msg = detail
                .job
                .message
                .unwrap_or_else(|| format!("job {status}"));
            return Err(Error::Other(msg));
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
}

async fn download_job_files(client: &Client, job_id: Uuid, dest: &Path) -> Result<u32> {
    let mut files: Vec<JobFile> = client.job_files(job_id).await?;
    if files.iter().all(|f| f.kind == "log") {
        tokio::time::sleep(Duration::from_secs(4)).await;
        files = client.job_files(job_id).await?;
    }
    let mut n = 0u32;
    for f in files {
        if !is_render_output(&f) {
            continue;
        }
        let Some(url) = f.download_url.as_deref() else {
            continue;
        };
        let bytes = client.download_url(url).await?;
        let name = safe_filename(&f.filename);
        let path = dest.join(name);
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| Error::Other(format!("writing {}: {e}", path.display())))?;
        n += 1;
    }
    Ok(n)
}

fn is_render_output(f: &JobFile) -> bool {
    let kind = f.kind.to_ascii_lowercase();
    if matches!(
        kind.as_str(),
        "source" | "log" | "crash" | "archive" | "blend"
    ) {
        return false;
    }
    let name = f.filename.to_ascii_lowercase();
    if name.ends_with(".blend") || name.ends_with(".zip") || name.ends_with(".json") {
        return false;
    }
    true
}

fn safe_filename(name: &str) -> String {
    let trimmed = name.replace('\\', "/");
    trimmed
        .rsplit('/')
        .next()
        .unwrap_or("file")
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_same_day() {
        let start = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(17, 0, 0).unwrap();
        assert!(in_window(NaiveTime::from_hms_opt(12, 0, 0).unwrap(), start, end));
        assert!(!in_window(NaiveTime::from_hms_opt(8, 0, 0).unwrap(), start, end));
    }

    #[test]
    fn window_overnight() {
        let start = NaiveTime::from_hms_opt(22, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        assert!(in_window(NaiveTime::from_hms_opt(23, 0, 0).unwrap(), start, end));
        assert!(in_window(NaiveTime::from_hms_opt(3, 0, 0).unwrap(), start, end));
        assert!(!in_window(NaiveTime::from_hms_opt(12, 0, 0).unwrap(), start, end));
    }

    #[test]
    fn settings_override_closest_wins() {
        let mut spec = RenderSpec::default();
        let over = RenderSpecOverride {
            samples: Some(64),
            frame_start: Some(1),
            frame_end: Some(48),
            movie: Some(true),
            ..Default::default()
        };
        over.apply(&mut spec);
        assert_eq!(spec.samples, 64);
        assert_eq!(spec.frame_end, Some(48));
        assert!(spec.make_movie);
        assert!(!spec.use_scene_frames);
    }

    #[test]
    fn blend_not_backup() {
        assert!(is_render_source(Path::new("shot.blend")));
        assert!(is_render_source(Path::new("shot.BLEND")));
        assert!(is_render_source(Path::new("pack.zip")));
        assert!(!is_render_source(Path::new("shot.blend1")));
        assert!(!is_render_source(Path::new(".hidden.blend")));
    }

    #[test]
    fn settings_dir_stays_inside_watch() {
        let root = std::env::temp_dir().join(format!("fxr-watch-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("shots")).unwrap();
        let (dir, rel) = settings_dir_for(&root, "shots").unwrap();
        assert_eq!(rel, "shots");
        assert!(dir.starts_with(root.canonicalize().unwrap()));
        assert!(settings_dir_for(&root, "../secret").is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
