//! Desktop watch loop: notify + periodic scan, schedule gate, run_item.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use fxrender_core::store;
use fxrender_core::watch::{
    apply_scan, folder_can_process, item_is_stable, push_activity, resolve_spec, run_item,
    delete_location_settings, location_settings, save_folders, save_items, save_location_settings,
    snapshot, write_settings_json, ItemStatus, LocationSettings, RenderSpec, RunProgress,
    WatchFolder, WatchFolderInput, WatchItem, WatchSnapshot,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;
use uuid::Uuid;

pub struct WatchEngine {
    inner: Arc<Mutex<Inner>>,
    wake: Arc<Notify>,
}

struct Inner {
    app: Option<AppHandle>,
    global_enabled: bool,
    folders: Vec<WatchFolder>,
    items: Vec<WatchItem>,
    activity: Vec<fxrender_core::watch::WatchActivity>,
    watchers: HashMap<Uuid, RecommendedWatcher>,
    busy_item: Option<Uuid>,
}

impl WatchEngine {
    pub fn start(app: AppHandle) -> Self {
        let (global_enabled, folders, mut items) = fxrender_core::watch::load_state()
            .unwrap_or((true, Vec::new(), Vec::new()));
        // In-flight work from a previous session: resume if we have a job id,
        // otherwise re-queue.
        for it in &mut items {
            if it.status.is_active() {
                if it.job_id.is_some() && matches!(it.status, ItemStatus::Rendering | ItemStatus::Downloading | ItemStatus::Inspecting) {
                    it.status = ItemStatus::Queued;
                } else {
                    it.status = ItemStatus::Queued;
                    it.job_id = None;
                }
                it.updated_at = Utc::now();
            }
        }
        let engine = Self {
            inner: Arc::new(Mutex::new(Inner {
                app: Some(app.clone()),
                global_enabled,
                folders: folders.clone(),
                items,
                activity: Vec::new(),
                watchers: HashMap::new(),
                busy_item: None,
            })),
            wake: Arc::new(Notify::new()),
        };

        {
            let g = engine.lock();
            let ids: Vec<Uuid> = g.folders.iter().map(|f| f.id).collect();
            drop(g);
            for id in ids {
                engine.rewatch(id);
                engine.scan_one(id);
            }
        }

        let loop_engine = engine.clone_engine();
        tauri::async_runtime::spawn(async move {
            loop_engine.process_loop().await;
        });

        let scan_engine = engine.clone_engine();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(8)).await;
                let ids = {
                    let g = scan_engine.lock();
                    g.folders.iter().map(|f| f.id).collect::<Vec<_>>()
                };
                for id in ids {
                    scan_engine.scan_one(id);
                }
                scan_engine.wake.notify_waiters();
            }
        });

        engine.emit();
        engine
    }

    fn clone_engine(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            wake: Arc::clone(&self.wake),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> WatchSnapshot {
        let g = self.lock();
        snapshot(g.global_enabled, &g.folders, &g.items, &g.activity)
    }

    fn emit(&self) {
        let (app, snap) = {
            let g = self.lock();
            (g.app.clone(), snapshot(g.global_enabled, &g.folders, &g.items, &g.activity))
        };
        if let Some(app) = app {
            let _ = app.emit("watch:state", snap);
        }
    }

    fn persist_folders(&self) {
        let g = self.lock();
        let _ = save_folders(g.global_enabled, &g.folders);
    }

    fn persist_items(&self) {
        let g = self.lock();
        let _ = save_items(&g.items);
    }

    fn note(&self, watch_id: Option<Uuid>, item_id: Option<Uuid>, kind: &str, message: impl Into<String>) {
        {
            let mut g = self.lock();
            push_activity(&mut g.activity, watch_id, item_id, kind, message);
        }
        self.emit();
    }

    pub fn set_global(&self, on: bool) -> WatchSnapshot {
        {
            let mut g = self.lock();
            g.global_enabled = on;
        }
        self.persist_folders();
        self.note(
            None,
            None,
            "info",
            if on {
                "Auto-process on"
            } else {
                "Auto-process paused — files are listed, nothing is sent"
            },
        );
        self.wake.notify_waiters();
        self.snapshot()
    }

    pub fn add_folder(&self, input: WatchFolderInput) -> Result<WatchSnapshot, String> {
        let folder = input.into_folder().map_err(|e| e.to_string())?;
        let id = folder.id;
        let name = folder.name.clone();
        {
            let mut g = self.lock();
            if g.folders.iter().any(|f| f.source_dir == folder.source_dir) {
                return Err("that folder is already being watched".into());
            }
            g.folders.push(folder);
        }
        self.persist_folders();
        self.rewatch(id);
        let added = self.scan_one(id);
        self.note(
            Some(id),
            None,
            "ok",
            format!("Watching {name} — found {added} file(s)"),
        );
        self.wake.notify_waiters();
        Ok(self.snapshot())
    }

    pub fn update_folder(&self, folder: WatchFolder) -> Result<WatchSnapshot, String> {
        if !folder.source_dir.is_dir() {
            return Err(format!(
                "watch folder does not exist: {}",
                folder.source_dir.display()
            ));
        }
        std::fs::create_dir_all(&folder.output_dir).map_err(|e| e.to_string())?;
        let id = folder.id;
        {
            let mut g = self.lock();
            let Some(slot) = g.folders.iter_mut().find(|f| f.id == id) else {
                return Err("watch folder not found".into());
            };
            *slot = folder;
        }
        self.persist_folders();
        self.rewatch(id);
        self.scan_one(id);
        self.note(Some(id), None, "info", "Watch folder updated");
        self.wake.notify_waiters();
        Ok(self.snapshot())
    }

    pub fn remove_folder(&self, id: Uuid) -> Result<WatchSnapshot, String> {
        {
            let mut g = self.lock();
            g.watchers.remove(&id);
            g.folders.retain(|f| f.id != id);
            g.items.retain(|i| i.watch_id != id);
        }
        self.persist_folders();
        self.persist_items();
        self.note(Some(id), None, "info", "Stopped watching folder");
        Ok(self.snapshot())
    }

    pub fn scan_now(&self, id: Uuid) -> Result<WatchSnapshot, String> {
        let added = self.scan_one(id);
        self.note(Some(id), None, "info", format!("Scan complete — {added} new file(s)"));
        self.wake.notify_waiters();
        Ok(self.snapshot())
    }

    pub fn process_now(&self, item_id: Uuid) -> Result<WatchSnapshot, String> {
        {
            let mut g = self.lock();
            let Some(it) = g.items.iter_mut().find(|i| i.id == item_id) else {
                return Err("file not found".into());
            };
            if it.status.is_active() {
                return Err("that file is already being processed".into());
            }
            it.status = ItemStatus::Queued;
            it.error = None;
            it.updated_at = Utc::now();
            // Make debounce a no-op for an explicit click.
            it.first_seen_at = Utc::now() - chrono::Duration::seconds(10);
        }
        self.persist_items();
        self.note(None, Some(item_id), "info", "Queued for render now");
        self.wake.notify_waiters();
        Ok(self.snapshot())
    }

    pub fn skip_item(&self, item_id: Uuid) -> Result<WatchSnapshot, String> {
        {
            let mut g = self.lock();
            let Some(it) = g.items.iter_mut().find(|i| i.id == item_id) else {
                return Err("file not found".into());
            };
            if it.status.is_active() {
                return Err("can't skip a file that's already rendering".into());
            }
            it.status = ItemStatus::Skipped;
            it.updated_at = Utc::now();
        }
        self.persist_items();
        self.emit();
        Ok(self.snapshot())
    }

    pub fn retry_item(&self, item_id: Uuid) -> Result<WatchSnapshot, String> {
        self.process_now(item_id)
    }

    pub fn write_settings(&self, id: Uuid, overwrite: bool) -> Result<String, String> {
        let (dir, spec) = {
            let g = self.lock();
            let f = g
                .folders
                .iter()
                .find(|f| f.id == id)
                .ok_or_else(|| "watch folder not found".to_string())?;
            (f.source_dir.clone(), f.spec.clone())
        };
        let path = write_settings_json(&dir, &spec, overwrite).map_err(|e| e.to_string())?;
        self.note(
            Some(id),
            None,
            "ok",
            format!("Wrote {}", path.display()),
        );
        Ok(path.display().to_string())
    }

    fn folder(&self, id: Uuid) -> Result<WatchFolder, String> {
        let g = self.lock();
        g.folders
            .iter()
            .find(|f| f.id == id)
            .cloned()
            .ok_or_else(|| "watch folder not found".to_string())
    }

    pub fn location_settings(&self, id: Uuid, relative: String) -> Result<LocationSettings, String> {
        let folder = self.folder(id)?;
        location_settings(&folder, &relative).map_err(|e| e.to_string())
    }

    pub fn save_location_settings(
        &self,
        id: Uuid,
        relative: String,
        spec: RenderSpec,
    ) -> Result<LocationSettings, String> {
        let folder = self.folder(id)?;
        let path = save_location_settings(&folder, &relative, &spec).map_err(|e| e.to_string())?;
        let loc = location_settings(&folder, &relative).map_err(|e| e.to_string())?;
        self.mark_spec_source(id, &loc.relative_dir, "settings.json");
        self.note(
            Some(id),
            None,
            "ok",
            format!("Saved render settings for {} ({})", loc.label, path.display()),
        );
        Ok(loc)
    }

    pub fn delete_location_settings(
        &self,
        id: Uuid,
        relative: String,
    ) -> Result<LocationSettings, String> {
        let folder = self.folder(id)?;
        let _ = delete_location_settings(&folder, &relative).map_err(|e| e.to_string())?;
        let loc = location_settings(&folder, &relative).map_err(|e| e.to_string())?;
        self.mark_spec_source(id, &loc.relative_dir, "ui");
        self.note(
            Some(id),
            None,
            "info",
            format!("Removed settings.json from {}", loc.label),
        );
        Ok(loc)
    }

    fn mark_spec_source(&self, watch_id: Uuid, relative_dir: &str, source: &str) {
        {
            let mut g = self.lock();
            for it in g.items.iter_mut().filter(|i| i.watch_id == watch_id) {
                let parent = match it.relative.rfind('/') {
                    Some(i) => &it.relative[..i],
                    None => ".",
                };
                if parent == relative_dir || (relative_dir == "." && !it.relative.contains('/')) {
                    it.spec_source = source.to_string();
                }
            }
        }
        self.persist_items();
        self.emit();
    }

    fn scan_one(&self, id: Uuid) -> usize {
        let mut g = self.lock();
        let Some(folder) = g.folders.iter().find(|f| f.id == id).cloned() else {
            return 0;
        };
        let added = apply_scan(&folder, &mut g.items);
        drop(g);
        if added > 0 {
            self.persist_items();
            self.emit();
        }
        added
    }

    fn rewatch(&self, id: Uuid) {
        let folder = {
            let g = self.lock();
            g.folders.iter().find(|f| f.id == id).cloned()
        };
        let Some(folder) = folder else { return };

        let engine = self.clone_engine();
        let watch_id = folder.id;
        let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                engine.scan_one(watch_id);
                engine.wake.notify_waiters();
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                self.note(Some(id), None, "err", format!("Could not watch folder: {e}"));
                return;
            }
        };
        let mode = if folder.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(e) = watcher.watch(&folder.source_dir, mode) {
            self.note(Some(id), None, "err", format!("Could not watch {}: {e}", folder.source_dir.display()));
            return;
        }
        let mut g = self.lock();
        g.watchers.insert(id, watcher);
    }

    fn pick_next(&self) -> Option<(WatchFolder, WatchItem, fxrender_core::watch::RenderSpec, String)> {
        let mut g = self.lock();
        if g.busy_item.is_some() {
            return None;
        }
        let global = g.global_enabled;
        // Explicit Queued always runs (Render now), even if paused/scheduled,
        // so a single click still works while auto-process is off.
        let mut chosen: Option<Uuid> = None;
        for it in &g.items {
            if it.status == ItemStatus::Queued {
                chosen = Some(it.id);
                break;
            }
        }
        if chosen.is_none() {
            for it in &g.items {
                if !it.status.is_open() {
                    continue;
                }
                if !item_is_stable(it) {
                    continue;
                }
                let Some(folder) = g.folders.iter().find(|f| f.id == it.watch_id) else {
                    continue;
                };
                if !folder_can_process(global, folder) {
                    continue;
                }
                chosen = Some(it.id);
                break;
            }
        }
        let id = chosen?;
        let item = g.items.iter().find(|i| i.id == id)?.clone();
        let folder = g.folders.iter().find(|f| f.id == item.watch_id)?.clone();
        let (spec, spec_source) = resolve_spec(&folder, &item.path);
        if let Some(it) = g.items.iter_mut().find(|i| i.id == id) {
            it.spec_source = spec_source.clone();
            it.status = ItemStatus::Uploading;
            it.updated_at = Utc::now();
            it.error = None;
        }
        g.busy_item = Some(id);
        Some((folder, item, spec, spec_source))
    }

    fn finish_busy(&self, item_id: Uuid, result: Result<fxrender_core::watch::RunOutcome, String>) {
        {
            let mut g = self.lock();
            if let Some(it) = g.items.iter_mut().find(|i| i.id == item_id) {
                match result {
                    Ok(out) => {
                        it.status = ItemStatus::Done;
                        it.job_id = Some(out.job_id);
                        it.downloaded_to = Some(out.dest.clone());
                        it.files_downloaded = out.files_downloaded;
                        it.error = None;
                    }
                    Err(e) => {
                        it.status = ItemStatus::Failed;
                        it.error = Some(e);
                    }
                }
                it.updated_at = Utc::now();
            }
            if g.busy_item == Some(item_id) {
                g.busy_item = None;
            }
        }
        self.persist_items();
        self.emit();
        self.wake.notify_waiters();
    }

    fn set_item_status(&self, item_id: Uuid, status: ItemStatus, job_id: Option<Uuid>) {
        {
            let mut g = self.lock();
            if let Some(it) = g.items.iter_mut().find(|i| i.id == item_id) {
                it.status = status;
                if job_id.is_some() {
                    it.job_id = job_id;
                }
                it.updated_at = Utc::now();
            }
        }
        self.emit();
    }

    async fn process_loop(&self) {
        loop {
            tokio::select! {
                _ = self.wake.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
            let Some((folder, item, spec, spec_source)) = self.pick_next() else {
                continue;
            };
            self.emit();
            self.note(
                Some(folder.id),
                Some(item.id),
                "info",
                format!("Starting {} ({spec_source})", item.relative),
            );

            let client = match store::load_client() {
                Ok(c) => c,
                Err(e) => {
                    {
                        let mut g = self.lock();
                        if let Some(it) = g.items.iter_mut().find(|i| i.id == item.id) {
                            it.status = ItemStatus::Queued;
                            it.updated_at = Utc::now();
                        }
                        g.busy_item = None;
                    }
                    self.emit();
                    if matches!(e, fxrender_core::Error::MissingToken) {
                        self.note(
                            Some(folder.id),
                            Some(item.id),
                            "info",
                            "Waiting for sign-in before sending to the farm",
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    } else {
                        self.note(Some(folder.id), Some(item.id), "err", e.to_string());
                    }
                    continue;
                }
            };

            let item_id = item.id;
            let relative = item.relative.clone();
            let path = item.path.clone();
            let output = folder.output_dir.clone();
            let engine = self.clone_engine();

            let outcome = run_item(
                &client,
                &path,
                &output,
                &relative,
                &spec,
                |p| match p {
                    RunProgress::Uploading => engine.set_item_status(item_id, ItemStatus::Uploading, None),
                    RunProgress::Inspecting { .. } => {
                        engine.set_item_status(item_id, ItemStatus::Inspecting, None)
                    }
                    RunProgress::Rendering { job_id } => {
                        engine.set_item_status(item_id, ItemStatus::Rendering, Some(job_id))
                    }
                    RunProgress::Downloading { job_id } => {
                        engine.set_item_status(item_id, ItemStatus::Downloading, Some(job_id))
                    }
                },
            )
            .await;

            match outcome {
                Ok(out) => {
                    let dest = out.dest.display().to_string();
                    let n = out.files_downloaded;
                    self.finish_busy(item_id, Ok(out));
                    self.note(
                        Some(folder.id),
                        Some(item_id),
                        "ok",
                        format!("Saved {relative} → {dest} ({n} file(s))"),
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.finish_busy(item_id, Err(msg.clone()));
                    self.note(Some(folder.id), Some(item_id), "err", format!("{relative}: {msg}"));
                }
            }
        }
    }
}
