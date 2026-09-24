import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { api, commandError, onWatchState } from "../lib/api";
import { itemTone } from "../lib/ui";
import type {
  LocationSettings,
  RenderSpec,
  ScheduleMode,
  WatchFolder,
  WatchFolderInput,
  WatchItem,
  WatchSnapshot,
} from "../lib/types";
import { emptyWatchInput } from "../lib/types";

async function pickDir(): Promise<string | null> {
  const p = await open({ directory: true, multiple: false });
  if (!p || Array.isArray(p)) return null;
  return p;
}

function shortPath(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/").filter(Boolean);
  if (parts.length <= 3) return p;
  return "…/" + parts.slice(-3).join("/");
}

function folderState(globalOn: boolean, f: WatchFolder): { label: string; tone: string } {
  if (!globalOn || !f.enabled) return { label: "paused", tone: "mute" };
  if (f.schedule.mode === "manual") return { label: "manual", tone: "mute" };
  if (f.schedule.mode === "window" && !f.allows_now) return { label: "scheduled", tone: "warn" };
  if (f.counts.active) return { label: "rendering", tone: "run" };
  return { label: "watching", tone: "ok" };
}

function itemLabel(status: string): string {
  if (status === "discovered") return "waiting";
  return status.replace(/_/g, " ");
}

export default function Watch() {
  const [snap, setSnap] = useState<WatchSnapshot | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<WatchFolderInput | WatchFolder | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [busy, setBusy] = useState(false);

  function apply(next: WatchSnapshot) {
    setSnap(next);
    setErr(null);
    if (selectedId && !next.folders.some((f) => f.id === selectedId)) {
      setSelectedId(next.folders[0]?.id ?? null);
    }
  }

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    api
      .watchSnapshot()
      .then((s) => {
        apply(s);
        if (!selectedId) setSelectedId(s.folders[0]?.id ?? null);
      })
      .catch((e) => setErr(commandError(e)));
    onWatchState((s) => {
      setSnap(s);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const selected = snap?.folders.find((f) => f.id === selectedId) ?? null;
  const items = useMemo(
    () => (snap && selectedId ? snap.items.filter((i) => i.watch_id === selectedId) : []),
    [snap, selectedId],
  );
  const activity = useMemo(() => {
    if (!snap) return [];
    const list = selectedId
      ? snap.activity.filter((a) => !a.watch_id || a.watch_id === selectedId)
      : snap.activity;
    return [...list].reverse().slice(0, 40);
  }, [snap, selectedId]);

  async function run<T>(fn: () => Promise<T>, after?: (v: T) => void) {
    setBusy(true);
    setErr(null);
    try {
      const v = await fn();
      after?.(v);
    } catch (e) {
      setErr(commandError(e));
    } finally {
      setBusy(false);
    }
  }

  function startAdd() {
    setIsNew(true);
    setDraft(emptyWatchInput());
  }

  function startEdit(f: WatchFolder) {
    setIsNew(false);
    setDraft({ ...f, spec: { ...f.spec }, schedule: { ...f.schedule } });
  }

  async function saveDraft(opts: { writeSettingsJson: boolean }) {
    if (!draft) return;
    if (!draft.source_dir.trim() || !draft.output_dir.trim()) {
      setErr("Pick a folder to watch and a folder to save results into.");
      return;
    }
    await run(
      async () => {
        const s = isNew ? await api.watchAdd(draft) : await api.watchUpdate(draft as WatchFolder);
        const id = isNew
          ? s.folders.find((f) => f.source_dir === draft.source_dir)?.id
          : (draft as WatchFolder).id;
        if (opts.writeSettingsJson && id) {
          await api.watchSaveLocationSettings(id, ".", draft.spec);
          return api.watchSnapshot();
        }
        return s;
      },
      (s) => {
        apply(s);
        const id = isNew
          ? s.folders.find((f) => f.source_dir === draft.source_dir)?.id
          : (draft as WatchFolder).id;
        if (id) setSelectedId(id);
        setDraft(null);
      },
    );
  }

  if (!snap) {
    return err ? <p className="error">{err}</p> : <p className="mute">Loading…</p>;
  }

  const autoOn = snap.global_enabled;

  return (
    <div className="watch-page">
      <div className="toolbar">
        <div>
          <h2 className="page-title">Watch</h2>
          <p className="hint" style={{ margin: "4px 0 0" }}>
            Drop a .blend into a watched folder. FXRender renders it and saves the result where you choose.
          </p>
        </div>
        <div className="toolbar-actions">
          <button
            className={`switch ${autoOn ? "on" : ""}`}
            role="switch"
            aria-checked={autoOn}
            title={autoOn ? "Auto-process is on" : "Auto-process is paused"}
            onClick={() => run(() => api.watchSetEnabled(!autoOn), apply)}
          >
            <span className="switch-knob" />
          </button>
          <span className="switch-label">{autoOn ? "Auto-process on" : "Paused"}</span>
          <button className="primary" onClick={startAdd} disabled={busy}>
            Add folder
          </button>
        </div>
      </div>

      {!autoOn ? (
        <div className="watch-banner">
          Auto-process is paused. New files are listed, but nothing is sent to the farm until you turn it on —
          except a file you click <strong>Render now</strong> on.
        </div>
      ) : null}

      {err ? <p className="error">{err}</p> : null}

      {snap.folders.length === 0 && !draft ? (
        <div className="card empty-hero">
          <h3>Watch a folder</h3>
          <p className="lede">
            Point FXRender at a directory of shots. It finds every .blend (and nested folders), uses the
            render settings you set here — or a <code>settings.json</code> next to the file — and downloads
            the finished frames to an output folder.
          </p>
          <button className="primary" onClick={startAdd}>
            Add folder
          </button>
        </div>
      ) : snap.folders.length === 0 && draft ? (
        <FolderEditor
          draft={draft}
          isNew={isNew}
          busy={busy}
          onChange={setDraft}
          onCancel={() => setDraft(null)}
          onSave={saveDraft}
        />
      ) : (
        <div className="watch-layout">
          <div className="watch-list">
            {snap.folders.map((f) => {
              const st = folderState(autoOn, f);
              return (
                <button
                  key={f.id}
                  className={`watch-card ${selectedId === f.id && !draft ? "selected" : ""}`}
                  onClick={() => {
                    setSelectedId(f.id);
                    setDraft(null);
                  }}
                >
                  <div className="watch-card-top">
                    <span className={`pill ${st.tone}`}>{st.label}</span>
                    <strong>{f.name}</strong>
                  </div>
                  <div className="watch-paths" title={`${f.source_dir} → ${f.output_dir}`}>
                    {shortPath(f.source_dir)}
                    <span className="arrow">→</span>
                    {shortPath(f.output_dir)}
                  </div>
                  <div className="watch-counts">
                    {f.counts.active ? `${f.counts.active} running · ` : null}
                    {f.counts.waiting ? `${f.counts.waiting} waiting · ` : null}
                    {f.counts.done} done
                    {f.counts.failed ? ` · ${f.counts.failed} failed` : null}
                  </div>
                </button>
              );
            })}
          </div>

          <div className="watch-detail">
            {draft ? (
              <FolderEditor
                draft={draft}
                isNew={isNew}
                busy={busy}
                onChange={setDraft}
                onCancel={() => setDraft(null)}
                onSave={saveDraft}
              />
            ) : selected ? (
              <FolderDetail
                folder={selected}
                globalOn={autoOn}
                items={items}
                activity={activity}
                busy={busy}
                onEdit={() => startEdit(selected)}
                onToggle={() =>
                  run(
                    () => api.watchUpdate({ ...selected, enabled: !selected.enabled }),
                    apply,
                  )
                }
                onScan={() => run(() => api.watchScan(selected.id), apply)}
                onRemove={() => {
                  if (!confirm(`Stop watching “${selected.name}”? Existing results on disk are kept.`)) return;
                  run(() => api.watchRemove(selected.id), apply);
                }}
                onProcess={(id) => run(() => api.watchProcessNow(id), apply)}
                onSkip={(id) => run(() => api.watchSkip(id), apply)}
                onRetry={(id) => run(() => api.watchRetry(id), apply)}
              />
            ) : (
              <p className="mute">Select a folder, or add one.</p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function FolderDetail({
  folder,
  globalOn,
  items,
  activity,
  busy,
  onEdit,
  onToggle,
  onScan,
  onRemove,
  onProcess,
  onSkip,
  onRetry,
}: {
  folder: WatchFolder;
  globalOn: boolean;
  items: WatchItem[];
  activity: WatchSnapshot["activity"];
  busy: boolean;
  onEdit: () => void;
  onToggle: () => void;
  onScan: () => void;
  onRemove: () => void;
  onProcess: (id: string) => void;
  onSkip: (id: string) => void;
  onRetry: (id: string) => void;
}) {
  const st = folderState(globalOn, folder);
  const groups = groupItems(items);
  const [loc, setLoc] = useState<LocationSettings | null>(null);
  const [locSpec, setLocSpec] = useState<RenderSpec | null>(null);
  const [locBusy, setLocBusy] = useState(false);
  const [locErr, setLocErr] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLocErr(null);
    api
      .watchLocationSettings(folder.id, ".")
      .then((s) => {
        if (cancelled) return;
        setLoc(s);
        setLocSpec(s.spec);
      })
      .catch((e) => {
        if (!cancelled) setLocErr(commandError(e));
      });
    return () => {
      cancelled = true;
    };
  }, [folder.id]);

  async function openLoc(relative: string) {
    setLocErr(null);
    try {
      const s = await api.watchLocationSettings(folder.id, relative);
      setLoc(s);
      setLocSpec(s.spec);
    } catch (e) {
      setLocErr(commandError(e));
    }
  }

  return (
    <div>
      <div className="toolbar" style={{ marginBottom: 8 }}>
        <div>
          <h3 style={{ margin: 0 }}>{folder.name}</h3>
          <p className="meta" style={{ margin: "6px 0 0" }}>
            <span className={`pill ${st.tone}`}>{st.label}</span>
            {folder.schedule.mode === "window"
              ? ` · ${folder.schedule.window_start}–${folder.schedule.window_end}`
              : folder.schedule.mode === "manual"
                ? " · scan or render yourself"
                : " · as files arrive"}
          </p>
        </div>
        <div className="toolbar-actions">
          <button className="ghost" disabled={busy} onClick={onToggle}>
            {folder.enabled ? "Pause folder" : "Resume folder"}
          </button>
          <button className="ghost" disabled={busy} onClick={onScan}>
            Scan now
          </button>
          <button className="ghost" disabled={busy} onClick={() => openLoc(".")}>
            settings.json
          </button>
          <button className="ghost" disabled={busy} onClick={onEdit}>
            Watch settings
          </button>
        </div>
      </div>

      <p className="watch-io">
        <span title={folder.source_dir}>Watching {shortPath(folder.source_dir)}</span>
        <span className="arrow">→</span>
        <button
          className="linkish"
          onClick={() => openPath(folder.output_dir).catch(() => {})}
          title={folder.output_dir}
        >
          Save to {shortPath(folder.output_dir)}
        </button>
      </p>

      {locErr ? <p className="error">{locErr}</p> : null}

      {loc && locSpec ? (
        <LocationSettingsCard
          loc={loc}
          spec={locSpec}
          fileCount={items.filter((it) => parentDir(it.relative) === loc.relative_dir).length}
          busy={locBusy}
          onChange={setLocSpec}
          onClose={() => {
            setLoc(null);
            setLocSpec(null);
          }}
          onSave={async () => {
            setLocBusy(true);
            setLocErr(null);
            try {
              const s = await api.watchSaveLocationSettings(folder.id, loc.relative_dir, locSpec);
              setLoc(s);
              setLocSpec(s.spec);
            } catch (e) {
              setLocErr(commandError(e));
            } finally {
              setLocBusy(false);
            }
          }}
          onRemove={async () => {
            if (!confirm(`Remove settings.json from ${loc.label}? Files there will use parent or watch defaults.`)) {
              return;
            }
            setLocBusy(true);
            setLocErr(null);
            try {
              const s = await api.watchDeleteLocationSettings(folder.id, loc.relative_dir);
              setLoc(s);
              setLocSpec(s.spec);
            } catch (e) {
              setLocErr(commandError(e));
            } finally {
              setLocBusy(false);
            }
          }}
        />
      ) : null}

      <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>File</th>
            <th>Status</th>
            <th>Spec</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {groups.map((g) => (
            <GroupRows
              key={g.dir}
              group={g}
              busy={busy}
              selectedDir={loc?.relative_dir ?? null}
              onSettings={openLoc}
              onProcess={onProcess}
              onSkip={onSkip}
              onRetry={onRetry}
            />
          ))}
          {items.length === 0 ? (
            <tr>
              <td colSpan={4} className="empty">
                No .blend files yet. Drop one into this folder
                {folder.recursive ? " (subfolders included)" : ""}.
              </td>
            </tr>
          ) : null}
        </tbody>
      </table>
      </div>

      <div className="watch-foot">
        <span className="mute" style={{ fontSize: 12 }}>
          Click a file or subfolder to edit <code>settings.json</code> for that location.
        </span>
        <button className="danger" disabled={busy} onClick={onRemove}>
          Remove folder
        </button>
      </div>

      {activity.length ? (
        <div className="activity-list">
          {activity.map((a, i) => (
            <div key={i} className={`activity-row ${a.kind}`}>
              <span className="activity-time">{formatTime(a.ts)}</span>
              {a.message}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

type Group = { dir: string; items: WatchItem[] };

function groupItems(items: WatchItem[]): Group[] {
  const map = new Map<string, WatchItem[]>();
  for (const it of items) {
    const slash = it.relative.lastIndexOf("/");
    const dir = slash === -1 ? "." : it.relative.slice(0, slash);
    const list = map.get(dir) ?? [];
    list.push(it);
    map.set(dir, list);
  }
  return [...map.entries()].map(([dir, items]) => ({ dir, items }));
}

function parentDir(relative: string): string {
  const slash = relative.lastIndexOf("/");
  return slash === -1 ? "." : relative.slice(0, slash);
}

function GroupRows({
  group,
  busy,
  selectedDir,
  onSettings,
  onProcess,
  onSkip,
  onRetry,
}: {
  group: Group;
  busy: boolean;
  selectedDir: string | null;
  onSettings: (relative: string) => void;
  onProcess: (id: string) => void;
  onSkip: (id: string) => void;
  onRetry: (id: string) => void;
}) {
  return (
    <>
      {group.dir !== "." ? (
        <tr
          className={`group-row clickable ${selectedDir === group.dir ? "selected" : ""}`}
          onClick={() => onSettings(group.dir)}
        >
          <td colSpan={3}>{group.dir}/</td>
          <td>
            <div className="row-actions">
              <button
                className="ghost"
                onClick={(e) => {
                  e.stopPropagation();
                  onSettings(group.dir);
                }}
              >
                Render settings
              </button>
            </div>
          </td>
        </tr>
      ) : null}
      {group.items.map((it) => {
        const name = it.relative.split("/").pop() ?? it.relative;
        const active = ["uploading", "inspecting", "rendering", "downloading"].includes(it.status);
        const selected = selectedDir === parentDir(it.relative);
        return (
          <tr
            key={it.id}
            className={`clickable${selected ? " selected" : ""}`}
            onClick={() => onSettings(it.relative)}
          >
            <td>
              <div>{name}</div>
              {it.error ? <div className="error" style={{ fontSize: 12 }}>{it.error}</div> : null}
            </td>
            <td>
              <span className={`pill ${itemTone(it.status)}`}>{itemLabel(it.status)}</span>
            </td>
            <td className="mute">{it.spec_source === "settings.json" ? "json" : "defaults"}</td>
            <td>
              <div className="row-actions" onClick={(e) => e.stopPropagation()}>
                <button className="ghost" onClick={() => onSettings(it.relative)}>
                  Render settings
                </button>
                {it.status === "done" && it.downloaded_to ? (
                  <button className="ghost" onClick={() => openPath(it.downloaded_to!).catch(() => {})}>
                    Open
                  </button>
                ) : null}
                {it.status === "failed" || it.status === "skipped" || it.status === "done" ? (
                  <button className="ghost" disabled={busy || active} onClick={() => onRetry(it.id)}>
                    Retry
                  </button>
                ) : null}
                {it.status === "discovered" || it.status === "waiting" || it.status === "queued" ? (
                  <>
                    <button className="ghost" disabled={busy || active} onClick={() => onProcess(it.id)}>
                      Render now
                    </button>
                    <button className="ghost" disabled={busy || active} onClick={() => onSkip(it.id)}>
                      Skip
                    </button>
                  </>
                ) : null}
              </div>
            </td>
          </tr>
        );
      })}
    </>
  );
}

function LocationSettingsCard({
  loc,
  spec,
  fileCount,
  busy,
  onChange,
  onClose,
  onSave,
  onRemove,
}: {
  loc: LocationSettings;
  spec: RenderSpec;
  fileCount: number;
  busy: boolean;
  onChange: (s: RenderSpec) => void;
  onClose: () => void;
  onSave: () => void;
  onRemove: () => void;
}) {
  const where = loc.relative_dir === "." ? "the watch folder root" : loc.label;
  let source = "using the watch folder defaults";
  if (loc.exists) {
    source = "this folder has its own settings.json";
  } else if (loc.inherited_from === ".") {
    source = "using the watch root settings.json";
  } else if (loc.inherited_from) {
    source = `using settings.json from ${loc.inherited_from}/`;
  }

  return (
    <div className="card loc-card">
      <div className="toolbar" style={{ marginBottom: 8 }}>
        <div>
          <h3 style={{ margin: 0 }}>settings.json</h3>
          <p className="meta" style={{ margin: "6px 0 0" }}>
            Stored in {where}
            {fileCount ? ` · ${fileCount} file${fileCount === 1 ? "" : "s"} here` : ""}
            {" · "}
            {source}
          </p>
        </div>
        <button className="ghost" onClick={onClose}>
          Close
        </button>
      </div>
      <p className="hint">
        These values are written to <code>settings.json</code> in this directory. Every .blend in the
        folder uses them. A subfolder can have its own file; the closest one to the shot wins.
      </p>
      <SpecFields spec={spec} onChange={(p) => onChange({ ...spec, ...p })} />
      <p className="mute" style={{ fontSize: 12, margin: "8px 0 14px" }} title={loc.path}>
        {loc.exists ? loc.path : `Will create ${loc.path}`}
      </p>
      <div className="toolbar-actions" style={{ justifyContent: "flex-end" }}>
        {loc.exists ? (
          <button className="danger" disabled={busy} onClick={onRemove}>
            Remove settings.json
          </button>
        ) : null}
        <button className="primary" disabled={busy} onClick={onSave}>
          {loc.exists ? "Save settings.json" : "Write settings.json to this folder"}
        </button>
      </div>
    </div>
  );
}

function SpecFields({
  spec,
  onChange,
}: {
  spec: RenderSpec;
  onChange: (p: Partial<RenderSpec>) => void;
}) {
  return (
    <>
      <div className="spec-grid">
        <div className="field">
          <label>Blender</label>
          <select
            value={spec.blender_version}
            onChange={(e) => onChange({ blender_version: e.target.value })}
          >
            <option value="4.2">4.2</option>
            <option value="4.5">4.5</option>
            <option value="5.2">5.2</option>
            <option value="5.3-alpha">blender-5.3-alpha</option>
          </select>
        </div>
        <div className="field">
          <label>Engine</label>
          <select value={spec.engine} onChange={(e) => onChange({ engine: e.target.value })}>
            <option value="cycles">Cycles</option>
            <option value="eevee">Eevee</option>
          </select>
        </div>
        <div className="field">
          <label>Format</label>
          <select
            value={spec.output_format}
            onChange={(e) => onChange({ output_format: e.target.value })}
          >
            <option value="png">PNG</option>
            <option value="exr">EXR</option>
            <option value="jpeg">JPEG</option>
            <option value="tiff">TIFF</option>
            <option value="webp">WebP</option>
          </select>
        </div>
        <div className="field">
          <label>Samples</label>
          <input
            type="number"
            min={1}
            value={spec.samples}
            onChange={(e) => onChange({ samples: Number(e.target.value) || 1 })}
          />
        </div>
        <div className="field">
          <label>Width</label>
          <input
            type="number"
            min={1}
            value={spec.resolution_x}
            onChange={(e) => onChange({ resolution_x: Number(e.target.value) || 1 })}
          />
        </div>
        <div className="field">
          <label>Height</label>
          <input
            type="number"
            min={1}
            value={spec.resolution_y}
            onChange={(e) => onChange({ resolution_y: Number(e.target.value) || 1 })}
          />
        </div>
      </div>

      <label className="check" style={{ margin: "4px 0 12px" }}>
        <input
          type="checkbox"
          checked={spec.use_scene_frames}
          onChange={(e) => onChange({ use_scene_frames: e.target.checked })}
        />
        Use the frame range from the .blend
      </label>
      {!spec.use_scene_frames ? (
        <div className="spec-grid">
          <div className="field">
            <label>Start</label>
            <input
              type="number"
              value={spec.frame_start ?? 1}
              onChange={(e) => onChange({ frame_start: Number(e.target.value) || 1 })}
            />
          </div>
          <div className="field">
            <label>End</label>
            <input
              type="number"
              value={spec.frame_end ?? 1}
              onChange={(e) => onChange({ frame_end: Number(e.target.value) || 1 })}
            />
          </div>
          <div className="field">
            <label>Step</label>
            <input
              type="number"
              min={1}
              value={spec.frame_step}
              onChange={(e) => onChange({ frame_step: Number(e.target.value) || 1 })}
            />
          </div>
        </div>
      ) : null}

      <div className="spec-grid">
        <div className="field">
          <label>FPS (movie)</label>
          <input
            type="number"
            min={1}
            value={spec.fps}
            onChange={(e) => onChange({ fps: Number(e.target.value) || 24 })}
          />
        </div>
      </div>
      <label className="check">
        <input
          type="checkbox"
          checked={spec.make_movie}
          onChange={(e) => onChange({ make_movie: e.target.checked })}
        />
        Encode a movie when the stills finish (free)
      </label>
      <label className="check" style={{ marginTop: 8, marginBottom: 12 }}>
        <input
          type="checkbox"
          checked={spec.keep_asset}
          onChange={(e) => onChange({ keep_asset: e.target.checked })}
        />
        Keep the .blend in the library (otherwise it’s ephemeral)
      </label>
    </>
  );
}

function FolderEditor({
  draft,
  isNew,
  busy,
  onChange,
  onCancel,
  onSave,
}: {
  draft: WatchFolderInput | WatchFolder;
  isNew: boolean;
  busy: boolean;
  onChange: (d: WatchFolderInput | WatchFolder) => void;
  onCancel: () => void;
  onSave: (opts: { writeSettingsJson: boolean }) => void;
}) {
  const spec = draft.spec;
  const [writeSettingsJson, setWriteSettingsJson] = useState(isNew);
  function patch(p: Partial<WatchFolderInput>) {
    onChange({ ...draft, ...p });
  }
  function patchSpec(p: Partial<RenderSpec>) {
    onChange({ ...draft, spec: { ...spec, ...p } });
  }

  return (
    <div className="card">
      <h3 style={{ marginTop: 0 }}>{isNew ? "Add a watch folder" : "Folder settings"}</h3>
      <p className="lede">
        FXRender lists every .blend in this directory. Render settings come from below, unless a
        <code> settings.json </code> sits next to the file (or in a parent folder).
      </p>

      <div className="field">
        <label>Name</label>
        <input
          placeholder="Night shots"
          value={draft.name}
          onChange={(e) => patch({ name: e.target.value })}
        />
      </div>

      <div className="field">
        <label>Watch this folder</label>
        <div className="path-row">
          <input readOnly value={draft.source_dir} placeholder="Folder where .blend files appear" />
          <button
            className="ghost"
            onClick={async () => {
              const p = await pickDir();
              if (p) patch({ source_dir: p, name: draft.name || p.split(/[/\\]/).pop() || "" });
            }}
          >
            Browse
          </button>
        </div>
      </div>

      <div className="field">
        <label>Save results here</label>
        <div className="path-row">
          <input readOnly value={draft.output_dir} placeholder="Folder for finished frames / movies" />
          <button
            className="ghost"
            onClick={async () => {
              const p = await pickDir();
              if (p) patch({ output_dir: p });
            }}
          >
            Browse
          </button>
        </div>
      </div>

      <label className="check" style={{ marginBottom: 16 }}>
        <input
          type="checkbox"
          checked={draft.recursive}
          onChange={(e) => patch({ recursive: e.target.checked })}
        />
        Include subfolders
      </label>

      <p className="field-label">When to render</p>
      <div className="seg" role="radiogroup">
        {(
          [
            ["always", "As files arrive"],
            ["window", "Only in a time window"],
            ["manual", "Manual only"],
          ] as [ScheduleMode, string][]
        ).map(([mode, label]) => (
          <button
            key={mode}
            className={draft.schedule.mode === mode ? "on" : ""}
            onClick={() => patch({ schedule: { ...draft.schedule, mode } })}
          >
            {label}
          </button>
        ))}
      </div>
      {draft.schedule.mode === "window" ? (
        <div className="spec-grid" style={{ marginTop: 12 }}>
          <div className="field">
            <label>From</label>
            <input
              type="time"
              value={draft.schedule.window_start}
              onChange={(e) =>
                patch({ schedule: { ...draft.schedule, window_start: e.target.value } })
              }
            />
          </div>
          <div className="field">
            <label>Until</label>
            <input
              type="time"
              value={draft.schedule.window_end}
              onChange={(e) =>
                patch({ schedule: { ...draft.schedule, window_end: e.target.value } })
              }
            />
          </div>
        </div>
      ) : null}
      {draft.schedule.mode === "manual" ? (
        <p className="hint">Files are listed. Nothing is sent unless you click Render now or Scan now.</p>
      ) : null}
      {draft.schedule.mode === "window" ? (
        <p className="hint">
          Overnight windows that wrap midnight work (e.g. 22:00 → 08:00). Outside the window, files wait.
        </p>
      ) : null}

      <p className="field-label">Render defaults</p>
      <label className="check" style={{ marginBottom: 12 }}>
        <input
          type="checkbox"
          checked={draft.prefer_settings_json}
          onChange={(e) => patch({ prefer_settings_json: e.target.checked })}
        />
        Prefer settings.json in a shot folder when it exists
      </label>
      <SpecFields spec={spec} onChange={patchSpec} />
      <label className="check" style={{ margin: "8px 0 12px" }}>
        <input
          type="checkbox"
          checked={writeSettingsJson}
          onChange={(e) => setWriteSettingsJson(e.target.checked)}
        />
        Write these as <code>settings.json</code> in the watch folder
      </label>
      <p className="hint">
        That file lives next to your .blend files. You can change it later from the Watch page, or per
        subfolder. Closest <code>settings.json</code> to the shot wins.
      </p>

      <div className="toolbar-actions" style={{ justifyContent: "flex-end" }}>
        <button className="ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <button
          className="primary"
          onClick={() => onSave({ writeSettingsJson })}
          disabled={busy}
        >
          {isNew ? "Start watching" : "Save"}
        </button>
      </div>
    </div>
  );
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
