export function statusTone(status: string): "ok" | "err" | "run" | "warn" | "mute" {
  const s = status.toLowerCase();
  if (["completed", "ready", "ok", "inspected", "done", "watching"].includes(s)) return "ok";
  if (
    ["failed", "cancelled", "canceled", "insufficient_quota", "timeout", "timed_out", "error"].includes(s)
  ) {
    return "err";
  }
  if (["queued", "pending", "inspecting", "waiting", "discovered", "scheduled", "paused", "manual"].includes(s)) {
    return "warn";
  }
  if (
    ["rendering", "downloading", "converting", "uploading", "provisioning", "queued"].includes(s)
  ) {
    return "run";
  }
  if (["skipped", "paused", "manual"].includes(s)) return "mute";
  return "mute";
}

export function itemTone(status: string): "ok" | "err" | "run" | "warn" | "mute" {
  const s = status.toLowerCase();
  if (s === "done") return "ok";
  if (s === "failed") return "err";
  if (["uploading", "inspecting", "rendering", "downloading", "queued"].includes(s)) return "run";
  if (["waiting", "discovered"].includes(s)) return "warn";
  return "mute";
}

export function formatSeconds(total: number | null | undefined): string {
  const s = Math.max(0, Math.floor(total ?? 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${sec}s`;
  return `${sec}s`;
}
