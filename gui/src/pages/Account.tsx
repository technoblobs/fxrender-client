import { useEffect, useState } from "react";
import { api, commandError } from "../lib/api";
import { formatSeconds } from "../lib/ui";
import type { Me } from "../lib/types";

export default function Account({ me: initial }: { me: Me | null }) {
  const [me, setMe] = useState<Me | null>(initial);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    setMe(initial);
  }, [initial]);

  useEffect(() => {
    if (initial) return;
    api.whoami().then(setMe).catch((e) => setErr(commandError(e)));
  }, [initial]);

  if (err) return <p className="error">{err}</p>;
  if (!me) return <p className="mute">Loading…</p>;

  const remainingPct = Math.max(0, Math.min(100, me.time.percent_remaining));
  const barClass = me.time.exhausted
    ? "exhausted"
    : me.time.critical
      ? "critical"
      : me.time.warning
        ? "warn"
        : "";

  return (
    <div>
      <h2 className="page-title">Account</h2>
      <div className="card" style={{ maxWidth: 480, marginTop: 16 }}>
        <h2 style={{ marginTop: 0, fontSize: 22 }}>{me.name}</h2>
        <p className="meta">{me.email}</p>
        <p className="meta" style={{ marginBottom: 4 }}>
          Render time remaining
        </p>
        <div className="meter" aria-hidden>
          <div className={`meter-bar ${barClass}`} style={{ width: `${remainingPct}%` }} />
        </div>
        <div className="stat-row">
          <strong>{formatSeconds(me.time.remaining_seconds)}</strong>
          <span className="mute">{me.time.percent_used.toFixed(1)}% used</span>
        </div>
        {me.storage ? (
          <p className="meta" style={{ marginTop: 16, marginBottom: 0 }}>
            Storage: {(me.storage.used_bytes / 1_000_000_000).toFixed(2)} / {me.storage.quota_gb} GB
            ({me.storage.percent.toFixed(1)}%) · {me.storage.plan_label}
          </p>
        ) : null}
      </div>
    </div>
  );
}
