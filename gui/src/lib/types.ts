// Mirrors the subset of fxrender-core::models this UI touches. Keep field
// names identical to the Rust structs (which mirror api/app/schemas.py) —
// serde/Tauri pass them through as plain JSON, no renaming either side.

export type AppConfig = {
  api_url: string;
  logged_in: boolean;
};

export type TimeQuota = {
  remaining_seconds: number;
  used_seconds: number;
  granted_seconds: number;
  percent_used: number;
  percent_remaining: number;
  exhausted: boolean;
  warning: boolean;
  critical: boolean;
};

export type StorageQuota = {
  plan_label: string;
  quota_gb: number;
  used_bytes: number;
  percent: number;
};

export type Me = {
  id: string;
  name: string;
  email: string;
  remaining_seconds: number;
  time: TimeQuota;
  storage?: StorageQuota | null;
};

export type Job = {
  id: string;
  status: string;
  filename: string;
  billed_seconds: number;
  frame_start: number;
  frame_end: number;
  created_at: string;
};

export type JobList = {
  items: Job[];
  total: number;
  page: number;
  page_size: number;
};

export type JobProgress = {
  frames_done: number;
  frames_total: number;
  eta_seconds: number | null;
};

export type JobDetail = Job & {
  progress: JobProgress | null;
};

export type Asset = {
  id: string;
  filename: string;
  status: string;
  frame_start: number | null;
  frame_end: number | null;
  resolution_x: number | null;
  resolution_y: number | null;
  ephemeral: boolean;
};

export type UploadedAsset = {
  asset: Asset;
  /** true if this exact file (by content hash) was already uploaded — no
   * bytes were sent, the existing asset was just reused. */
  reused: boolean;
};

export type AssetList = {
  items: Asset[];
  total: number;
};

export type LogLine = {
  ts: string;
  stream: string;
  line: string;
  frame: number | null;
};

export type JobCreate = {
  asset_id: string;
  blender_version: string;
  engine: string;
  device: string;
  output_format: string;
  samples: number;
  resolution_x: number;
  resolution_y: number;
  frame_start: number;
  frame_end: number;
  frame_step: number;
  fps: number;
  make_movie: boolean;
};

export type RenderSpec = {
  blender_version: string;
  engine: string;
  device: string;
  output_format: string;
  samples: number;
  resolution_x: number;
  resolution_y: number;
  frame_start: number | null;
  frame_end: number | null;
  frame_step: number;
  fps: number;
  camera: string | null;
  make_movie: boolean;
  keep_asset: boolean;
  use_scene_frames: boolean;
};

export type ScheduleMode = "always" | "window" | "manual";

export type Schedule = {
  mode: ScheduleMode;
  window_start: string;
  window_end: string;
};

export type WatchCounts = {
  total: number;
  waiting: number;
  active: number;
  done: number;
  failed: number;
};

export type WatchFolder = {
  id: string;
  name: string;
  source_dir: string;
  output_dir: string;
  recursive: boolean;
  enabled: boolean;
  spec: RenderSpec;
  prefer_settings_json: boolean;
  schedule: Schedule;
  created_at: string;
  allows_now: boolean;
  counts: WatchCounts;
};

export type WatchFolderInput = {
  name: string;
  source_dir: string;
  output_dir: string;
  recursive: boolean;
  enabled: boolean;
  spec: RenderSpec;
  prefer_settings_json: boolean;
  schedule: Schedule;
};

export type ItemStatus =
  | "discovered"
  | "waiting"
  | "queued"
  | "uploading"
  | "inspecting"
  | "rendering"
  | "downloading"
  | "done"
  | "failed"
  | "skipped";

export type WatchItem = {
  id: string;
  watch_id: string;
  path: string;
  relative: string;
  size_bytes: number;
  mtime: number;
  status: ItemStatus;
  spec_source: string;
  job_id: string | null;
  error: string | null;
  downloaded_to: string | null;
  files_downloaded: number;
  first_seen_at: string;
  updated_at: string;
};

export type WatchActivity = {
  ts: string;
  watch_id: string | null;
  item_id: string | null;
  kind: string;
  message: string;
};

export type WatchSnapshot = {
  global_enabled: boolean;
  folders: WatchFolder[];
  items: WatchItem[];
  activity: WatchActivity[];
};

export type LocationSettings = {
  relative_dir: string;
  label: string;
  path: string;
  exists: boolean;
  spec: RenderSpec;
  inherited_from: string | null;
};

export const defaultRenderSpec = (): RenderSpec => ({
  blender_version: "4.5",
  engine: "cycles",
  device: "gpu",
  output_format: "png",
  samples: 128,
  resolution_x: 1920,
  resolution_y: 1080,
  frame_start: null,
  frame_end: null,
  frame_step: 1,
  fps: 24,
  camera: null,
  make_movie: false,
  keep_asset: false,
  use_scene_frames: true,
});

export const defaultSchedule = (): Schedule => ({
  mode: "always",
  window_start: "22:00",
  window_end: "08:00",
});

export const emptyWatchInput = (): WatchFolderInput => ({
  name: "",
  source_dir: "",
  output_dir: "",
  recursive: true,
  enabled: true,
  spec: defaultRenderSpec(),
  prefer_settings_json: true,
  schedule: defaultSchedule(),
});
