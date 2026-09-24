import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  AssetList,
  JobCreate,
  JobDetail,
  JobList,
  LogLine,
  Me,
  UploadedAsset,
  LocationSettings,
  RenderSpec,
  WatchFolder,
  WatchFolderInput,
  WatchSnapshot,
} from "./types";

/** Tauri command errors reject with the `String` our Rust commands return. */
export function commandError(ex: unknown): string {
  if (typeof ex === "string" && ex.trim()) return ex;
  if (ex instanceof Error && ex.message) return ex.message;
  return "Something went wrong";
}

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  setApiUrl: (url: string) => invoke<AppConfig>("set_api_url", { url }),
  login: (token: string) => invoke<AppConfig>("login", { token }),
  logout: () => invoke<AppConfig>("logout"),

  whoami: () => invoke<Me>("whoami"),

  listJobs: (page: number, pageSize: number, status?: string) =>
    invoke<JobList>("list_jobs", { page, pageSize, status: status ?? null }),
  getJob: (id: string) => invoke<JobDetail>("get_job", { id }),
  cancelJob: (id: string) => invoke("cancel_job", { id }),

  listAssets: (page: number, pageSize: number) =>
    invoke<AssetList>("list_assets", { page, pageSize }),
  uploadFile: (path: string, keep: boolean) => invoke<UploadedAsset>("upload_file", { path, keep }),

  createJob: (req: JobCreate) => invoke<JobDetail>("create_job", { req }),

  /** Starts the backlog+live tail; resolves once the SSE connection ends
   * (job reached a terminal status, or an error). Subscribe with
   * `onJobLog` first, then call this. */
  tailJobLogs: (id: string) => invoke<void>("tail_job_logs", { id }),

  watchSnapshot: () => invoke<WatchSnapshot>("watch_snapshot"),
  watchSetEnabled: (enabled: boolean) => invoke<WatchSnapshot>("watch_set_enabled", { enabled }),
  watchAdd: (input: WatchFolderInput) => invoke<WatchSnapshot>("watch_add", { input }),
  watchUpdate: (folder: WatchFolder) => invoke<WatchSnapshot>("watch_update", { folder }),
  watchRemove: (id: string) => invoke<WatchSnapshot>("watch_remove", { id }),
  watchScan: (id: string) => invoke<WatchSnapshot>("watch_scan", { id }),
  watchProcessNow: (itemId: string) => invoke<WatchSnapshot>("watch_process_now", { itemId }),
  watchSkip: (itemId: string) => invoke<WatchSnapshot>("watch_skip", { itemId }),
  watchRetry: (itemId: string) => invoke<WatchSnapshot>("watch_retry", { itemId }),
  watchWriteSettings: (id: string, overwrite: boolean) =>
    invoke<string>("watch_write_settings", { id, overwrite }),
  watchLocationSettings: (id: string, relative: string) =>
    invoke<LocationSettings>("watch_location_settings", { id, relative }),
  watchSaveLocationSettings: (id: string, relative: string, spec: RenderSpec) =>
    invoke<LocationSettings>("watch_save_location_settings", { id, relative, spec }),
  watchDeleteLocationSettings: (id: string, relative: string) =>
    invoke<LocationSettings>("watch_delete_location_settings", { id, relative }),
};

export function onJobLog(jobId: string, handler: (line: LogLine) => void): Promise<UnlistenFn> {
  return listen<LogLine>(`job-log:${jobId}`, (event) => handler(event.payload));
}

export function onWatchState(handler: (snap: WatchSnapshot) => void): Promise<UnlistenFn> {
  return listen<WatchSnapshot>("watch:state", (event) => handler(event.payload));
}
