import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AppConfig, Session } from "../App";

export default function Home({
  config,
  onOpenSetup,
  onStarted,
}: {
  config: AppConfig;
  onOpenSetup: () => void;
  onStarted: (s: Session) => void;
}) {
  const [status, setStatus] = useState("Idle");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function start() {
    setErr(null);
    setBusy(true);
    setStatus("Starting machine… (10-30 seconds)");
    try {
      const s = await invoke<Session>("start_session", { config });
      onStarted(s);
    } catch (e: any) {
      setErr(String(e));
      setStatus("Idle");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="container center">
      <h1>Toddler Claude</h1>
      <p className="muted">
        Working on <strong>{config.repo}</strong> (branch{" "}
        <strong>{config.branch}</strong>) in <strong>{config.region}</strong>
      </p>

      <button className="big-green" onClick={start} disabled={busy}>
        {busy ? "Please wait…" : "Start working"}
      </button>

      <p className="status">{status}</p>
      {err && <p className="err">{err}</p>}

      <p className="muted small">
        Each session auto-stops after 15 minutes of no typing, and always within
        2 hours.
      </p>

      <button className="link" onClick={onOpenSetup}>
        Change settings
      </button>
    </div>
  );
}
