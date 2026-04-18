import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Setup from "./components/Setup";
import Home from "./components/Home";
import SessionView from "./components/SessionView";

type Screen = "loading" | "setup" | "home" | "session";

export interface Session {
  id: string;
  app_name: string;
  machine_id: string;
  hostname: string;
  password: string;
  started_at: number;
  region: string;
  repo: string;
  branch: string;
}

export interface AppConfig {
  repo: string;
  branch: string;
  region: string;
  fly_org_slug: string;
  remote_image: string;
}

export default function App() {
  const [screen, setScreen] = useState<Screen>("loading");
  const [session, setSession] = useState<Session | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const [claude, gh, fly, cfg, sess] = await Promise.all([
          invoke<boolean>("has_claude_token"),
          invoke<boolean>("has_github_token"),
          invoke<boolean>("has_fly_token"),
          invoke<AppConfig>("load_config"),
          invoke<Session | null>("session_status"),
        ]);
        setConfig(cfg);
        if (sess) {
          setSession(sess);
          setScreen("session");
          return;
        }
        if (!claude || !gh || !fly || !cfg.repo || !cfg.fly_org_slug) {
          setScreen("setup");
        } else {
          setScreen("home");
        }
      } catch (e) {
        console.error(e);
        setScreen("setup");
      }
    })();
  }, []);

  if (screen === "loading") {
    return (
      <div className="center">
        <p>Loading…</p>
      </div>
    );
  }

  if (screen === "setup") {
    return (
      <Setup
        initialConfig={config}
        onDone={async () => {
          const cfg = await invoke<AppConfig>("load_config");
          setConfig(cfg);
          setScreen("home");
        }}
      />
    );
  }

  if (screen === "home" && config) {
    return (
      <Home
        config={config}
        onOpenSetup={() => setScreen("setup")}
        onStarted={(s) => {
          setSession(s);
          setScreen("session");
        }}
      />
    );
  }

  if (screen === "session" && session) {
    return (
      <SessionView
        session={session}
        onStopped={() => {
          setSession(null);
          setScreen("home");
        }}
      />
    );
  }

  return null;
}
