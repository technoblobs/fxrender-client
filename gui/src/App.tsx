import { useEffect, useState } from "react";
import { api, commandError, onWatchState } from "./lib/api";
import type { AppConfig, Me, WatchSnapshot } from "./lib/types";
import Login from "./pages/Login";
import Account from "./pages/Account";
import Jobs from "./pages/Jobs";
import Assets from "./pages/Assets";
import Watch from "./pages/Watch";

type Tab = "account" | "assets" | "jobs" | "watch";

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [me, setMe] = useState<Me | null>(null);
  const [tab, setTab] = useState<Tab>("jobs");
  const [err, setErr] = useState<string | null>(null);
  const [watch, setWatch] = useState<WatchSnapshot | null>(null);

  useEffect(() => {
    api.getConfig().then(setConfig).catch((e) => setErr(commandError(e)));
  }, []);

  useEffect(() => {
    if (!config?.logged_in) {
      setMe(null);
      setWatch(null);
      return;
    }
    api.whoami().then(setMe).catch(() => setMe(null));
    let unlisten: (() => void) | undefined;
    api.watchSnapshot().then(setWatch).catch(() => setWatch(null));
    onWatchState(setWatch).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [config?.logged_in]);

  if (err) return <div className="centered error">{err}</div>;
  if (!config) return <div className="centered mute">Loading…</div>;
  if (!config.logged_in) return <Login onLoggedIn={setConfig} />;

  async function logout() {
    const cfg = await api.logout();
    setMe(null);
    setConfig(cfg);
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <img src="/logo.png" alt="" />
          <span className="brand-name">FXRender</span>
        </div>
        {(["jobs", "watch", "assets", "account"] as Tab[]).map((t) => (
          <button
            key={t}
            className={`nav-btn ${tab === t ? "active" : ""}`}
            onClick={() => setTab(t)}
          >
            {t[0].toUpperCase() + t.slice(1)}
            {t === "watch" && watch?.global_enabled && watch.folders.some((f) => f.enabled) ? (
              <span className="nav-dot" />
            ) : null}
          </button>
        ))}
        <div className="sidebar-foot">
          {me ? <div className="sidebar-user">{me.email}</div> : null}
          <button className="nav-btn" onClick={logout}>
            Sign out
          </button>
        </div>
      </aside>
      <div className="main">
        {tab === "jobs" ? <Jobs /> : null}
        {tab === "watch" ? <Watch /> : null}
        {tab === "assets" ? <Assets /> : null}
        {tab === "account" ? <Account me={me} /> : null}
      </div>
    </div>
  );
}
