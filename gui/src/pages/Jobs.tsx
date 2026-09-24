import { useEffect, useRef, useState } from "react";
import { api, commandError, onJobLog } from "../lib/api";
import { formatSeconds, statusTone } from "../lib/ui";
import type { Job, JobDetail, LogLine } from "../lib/types";

export default function Jobs() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<JobDetail | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const logPaneRef = useRef<HTMLDivElement>(null);

  function loadJobs() {
    setLoading(true);
    setErr(null);
    api
      .listJobs(1, 50)
      .then((list) => setJobs(list.items))
      .catch((e) => setErr(commandError(e)))
      .finally(() => setLoading(false));
  }

  useEffect(loadJobs, []);

  useEffect(() => {
    if (!selected) return;
    setLogs([]);
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onJobLog(selected.id, (line) => {
      if (cancelled) return;
      setLogs((prev) => [...prev, line]);
    }).then((fn) => {
      unlisten = fn;
    });
    api.tailJobLogs(selected.id).catch(() => {
      // Stream ended (job finished, or a network hiccup) — the log pane
      // just stops updating; re-selecting the job reconnects it.
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [selected?.id]);

  useEffect(() => {
    logPaneRef.current?.scrollTo({ top: logPaneRef.current.scrollHeight });
  }, [logs]);

  async function select(job: Job) {
    setErr(null);
    try {
      const detail = await api.getJob(job.id);
      setSelected(detail);
    } catch (e) {
      setErr(commandError(e));
    }
  }

  async function cancel(id: string) {
    try {
      await api.cancelJob(id);
      loadJobs();
    } catch (e) {
      setErr(commandError(e));
    }
  }

  return (
    <div className="split">
      <div>
        <div className="toolbar">
          <h2 className="page-title">Jobs</h2>
        </div>
        {err ? <p className="error">{err}</p> : null}
        <table>
          <thead>
            <tr>
              <th>File</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {jobs.map((j) => (
              <tr
                key={j.id}
                className={`clickable${selected?.id === j.id ? " selected" : ""}`}
                onClick={() => select(j)}
              >
                <td>{j.filename}</td>
                <td>
                  <span className={`pill ${statusTone(j.status)}`}>{j.status}</span>
                </td>
              </tr>
            ))}
            {!loading && !err && jobs.length === 0 ? (
              <tr>
                <td colSpan={2} className="empty">
                  No jobs yet — upload and render something from the Assets tab.
                </td>
              </tr>
            ) : null}
            {loading ? (
              <tr>
                <td colSpan={2} className="mute">
                  Loading…
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>

      <div>
        {selected ? (
          <div className="card">
            <h3 style={{ marginTop: 0 }}>{selected.filename}</h3>
            <p className="meta">
              <span className={`pill ${statusTone(selected.status)}`}>{selected.status}</span>
              {" · "}
              frames {selected.frame_start}–{selected.frame_end}
              {" · "}
              billed {formatSeconds(selected.billed_seconds)}
            </p>
            {selected.progress ? (
              <p className="meta">
                {selected.progress.frames_done}/{selected.progress.frames_total} frames done
                {selected.progress.eta_seconds != null
                  ? ` · eta ${formatSeconds(selected.progress.eta_seconds)}`
                  : ""}
              </p>
            ) : null}
            <button className="danger" onClick={() => cancel(selected.id)}>
              Cancel job
            </button>
            <div className="log-pane" ref={logPaneRef} style={{ marginTop: 16 }}>
              {logs.map((l, i) => (
                <div key={i} className={`log-line-${l.stream}`}>
                  [{l.ts}] {l.stream} {l.line}
                </div>
              ))}
              {logs.length === 0 ? <span className="mute">Waiting for log lines…</span> : null}
            </div>
          </div>
        ) : (
          <p className="mute">Select a job to see its progress and live logs.</p>
        )}
      </div>
    </div>
  );
}
