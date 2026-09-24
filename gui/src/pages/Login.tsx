import { useState } from "react";
import { api, commandError } from "../lib/api";
import type { AppConfig } from "../lib/types";

export default function Login({ onLoggedIn }: { onLoggedIn: (cfg: AppConfig) => void }) {
  const [token, setToken] = useState("");
  const [apiUrl, setApiUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function submit() {
    setBusy(true);
    setErr(null);
    try {
      if (apiUrl.trim()) {
        await api.setApiUrl(apiUrl.trim());
      }
      const cfg = await api.login(token.trim());
      // login() only saves the token locally — confirm it's actually valid
      // against the API before treating this as signed in. A bad token
      // otherwise "succeeds" silently and just fails later on every page.
      try {
        await api.whoami();
      } catch (e) {
        await api.logout();
        setErr(commandError(e));
        return;
      }
      onLoggedIn(cfg);
    } catch (e) {
      setErr(commandError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="centered">
      <div className="card login-card">
        <div className="brand">
          <img src="/logo.png" alt="" />
          <span className="brand-name">FXRender</span>
        </div>
        <h2>Sign in</h2>
        <p className="lede">
          Paste an API token from your account’s API Tokens page on the FXRender dashboard.
        </p>
        <div className="field">
          <label>API token</label>
          <input
            type="password"
            placeholder="fxr_live_..."
            value={token}
            onChange={(e) => setToken(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && token.trim() && !busy) submit();
            }}
          />
        </div>
        <div className="field">
          <label>API URL (optional)</label>
          <input
            placeholder="https://api.fxrender.com/v1"
            value={apiUrl}
            onChange={(e) => setApiUrl(e.target.value)}
          />
        </div>
        {err ? <p className="error">{err}</p> : null}
        <button className="primary" disabled={busy || !token.trim()} onClick={submit} style={{ width: "100%" }}>
          {busy ? "Signing in…" : "Sign in"}
        </button>
      </div>
    </div>
  );
}
