import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, commandError } from "../lib/api";
import { statusTone } from "../lib/ui";
import type { Asset } from "../lib/types";

export default function Assets() {
  const [assets, setAssets] = useState<Asset[]>([]);
  const [uploading, setUploading] = useState(false);
  const [keep, setKeep] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  function load() {
    api
      .listAssets(1, 50)
      .then((list) => setAssets(list.items))
      .catch((e) => setErr(commandError(e)));
  }

  useEffect(load, []);

  async function pickAndUpload() {
    setErr(null);
    setNotice(null);
    const path = await open({
      multiple: false,
      filters: [{ name: "Blender scene", extensions: ["blend", "zip"] }],
    });
    if (!path || Array.isArray(path)) return;
    setUploading(true);
    try {
      const { reused } = await api.uploadFile(path, keep);
      if (reused) setNotice("Already uploaded — reused the existing file, nothing was sent.");
      load();
    } catch (e) {
      setErr(commandError(e));
    } finally {
      setUploading(false);
    }
  }

  return (
    <div>
      <div className="toolbar">
        <h2 className="page-title">Assets</h2>
        <div className="toolbar-actions">
          <label className="check">
            <input type="checkbox" checked={keep} onChange={(e) => setKeep(e.target.checked)} />
            Keep on shelf
          </label>
          <button className="primary" disabled={uploading} onClick={pickAndUpload}>
            {uploading ? "Uploading…" : "Upload .blend / .zip"}
          </button>
        </div>
      </div>
      <p className="hint">
        Uploads are ephemeral by default — not listed here for long, deleted a while after the
        render finishes. Check “Keep on shelf” to keep this one like a normal upload.
      </p>
      {err ? <p className="error">{err}</p> : null}
      {notice ? <p className="hint">{notice}</p> : null}
      <table>
        <thead>
          <tr>
            <th>File</th>
            <th>Status</th>
            <th>Frames</th>
            <th>Resolution</th>
          </tr>
        </thead>
        <tbody>
          {assets.map((a) => (
            <tr key={a.id}>
              <td>{a.filename}</td>
              <td>
                <span className={`pill ${statusTone(a.status)}`}>{a.status}</span>
              </td>
              <td>{a.frame_start != null ? `${a.frame_start}–${a.frame_end}` : "—"}</td>
              <td>{a.resolution_x != null ? `${a.resolution_x}×${a.resolution_y}` : "—"}</td>
            </tr>
          ))}
          {assets.length === 0 ? (
            <tr>
              <td colSpan={4} className="empty">
                Nothing uploaded yet.
              </td>
            </tr>
          ) : null}
        </tbody>
      </table>
    </div>
  );
}
