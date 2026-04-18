import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AppConfig } from "../App";

const openExternal = (url: string) => openUrl(url);

interface Repo {
  full_name: string;
  private: boolean;
  default_branch: string;
}

interface DeviceStart {
  device_code: string;
  user_code: string;
  verification_uri: string;
  interval: number;
}

interface FlySigninResult {
  email: string;
  orgs: string[];
}

export default function Setup({
  initialConfig,
  onDone,
}: {
  initialConfig: AppConfig | null;
  onDone: () => void;
}) {
  const [step, setStep] = useState<1 | 2 | 3 | 4>(1);
  const [claudeOk, setClaudeOk] = useState(false);
  const [ghOk, setGhOk] = useState(false);
  const [flyOk, setFlyOk] = useState(false);

  // Claude
  const [claudeBusy, setClaudeBusy] = useState(false);
  const [claudeLog, setClaudeLog] = useState<string[]>([]);
  const [claudeErr, setClaudeErr] = useState<string | null>(null);
  const [showClaudeManual, setShowClaudeManual] = useState(false);
  const [claudePaste, setClaudePaste] = useState("");
  const unlistenRef = useRef<UnlistenFn | null>(null);

  // Fly
  const [flyToken, setFlyToken] = useState("");
  const [flyOrg, setFlyOrg] = useState(initialConfig?.fly_org_slug ?? "");
  const [flyOrgs, setFlyOrgs] = useState<string[]>([]);
  const [flyEmail, setFlyEmail] = useState<string | null>(null);
  const [flyErr, setFlyErr] = useState<string | null>(null);
  const [showFlyManual, setShowFlyManual] = useState(false);

  // GitHub
  const [device, setDevice] = useState<DeviceStart | null>(null);
  const [ghErr, setGhErr] = useState<string | null>(null);
  const [ghLogin, setGhLogin] = useState<string | null>(null);
  const [showGhManual, setShowGhManual] = useState(false);
  const [ghPat, setGhPat] = useState("");
  const pollStop = useRef(false);

  // Repo
  const [repos, setRepos] = useState<Repo[]>([]);
  const [repo, setRepo] = useState(initialConfig?.repo ?? "");
  const [branch, setBranch] = useState(initialConfig?.branch ?? "main");
  const [region, setRegion] = useState(initialConfig?.region ?? "fra");
  const [image, setImage] = useState(
    initialConfig?.remote_image ??
      "ghcr.io/walthermuurmans/toddler-claude-remote:latest",
  );

  useEffect(() => {
    (async () => {
      setClaudeOk(await invoke<boolean>("has_claude_token"));
      setGhOk(await invoke<boolean>("has_github_token"));
      setFlyOk(await invoke<boolean>("has_fly_token"));
    })();
    return () => {
      if (unlistenRef.current) unlistenRef.current();
      pollStop.current = true;
    };
  }, []);

  // ───── Claude ─────
  async function claudeAuto() {
    setClaudeErr(null);
    setClaudeLog([]);
    setClaudeBusy(true);
    try {
      unlistenRef.current = await listen<string>(
        "claude-setup-log",
        (evt) => setClaudeLog((prev) => [...prev.slice(-50), evt.payload]),
      );
      await invoke("claude_auto_setup");
      setClaudeOk(true);
      setStep(2);
    } catch (err: any) {
      setClaudeErr(
        String(err) +
          " — try the manual paste option, or install Claude Code first.",
      );
    } finally {
      setClaudeBusy(false);
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    }
  }

  async function claudeManual() {
    setClaudeErr(null);
    try {
      await invoke("store_claude_token", { token: claudePaste.trim() });
      setClaudeOk(true);
      setClaudePaste("");
      setStep(2);
    } catch (err: any) {
      setClaudeErr(String(err));
    }
  }

  // ───── GitHub ─────
  async function githubCliFastPath() {
    setGhErr(null);
    try {
      const login = await invoke<string>("github_cli_signin");
      setGhLogin(login);
      setGhOk(true);
      const r = await invoke<Repo[]>("list_github_repos");
      setRepos(r);
      setStep(3);
    } catch (err: any) {
      setGhErr(
        "gh CLI not available or not logged in: " +
          String(err) +
          " — try the Browser or PAT option.",
      );
    }
  }

  async function githubDevice() {
    setGhErr(null);
    pollStop.current = false;
    try {
      const d = await invoke<DeviceStart>("github_device_start");
      setDevice(d);
      await openExternal(d.verification_uri);
      const deadline = Date.now() + 900 * 1000;
      const poll = async () => {
        if (pollStop.current) return;
        if (Date.now() > deadline) {
          setGhErr("GitHub code expired. Try again.");
          setDevice(null);
          return;
        }
        try {
          const token = await invoke<string | null>("github_device_poll", {
            deviceCode: d.device_code,
          });
          if (token) {
            await invoke("store_github_token", { token });
            setGhOk(true);
            setDevice(null);
            const r = await invoke<Repo[]>("list_github_repos");
            setRepos(r);
            setStep(3);
            return;
          }
        } catch (err) {
          setGhErr(String(err));
          setDevice(null);
          return;
        }
        setTimeout(poll, (d.interval + 1) * 1000);
      };
      setTimeout(poll, d.interval * 1000);
    } catch (err: any) {
      setGhErr(String(err));
    }
  }

  async function githubPat() {
    setGhErr(null);
    try {
      await invoke("store_github_token", { token: ghPat.trim() });
      const r = await invoke<Repo[]>("list_github_repos");
      setRepos(r);
      setGhOk(true);
      setGhPat("");
      setStep(3);
    } catch (err: any) {
      setGhErr(String(err));
    }
  }

  // ───── Fly ─────
  async function flyCliFastPath() {
    setFlyErr(null);
    try {
      const r = await invoke<FlySigninResult>("fly_cli_signin");
      setFlyEmail(r.email);
      setFlyOrgs(r.orgs);
      setFlyOrg((prev) => prev || r.orgs[0] || "personal");
      setFlyOk(true);
      setStep(4);
    } catch (err: any) {
      setFlyErr(
        "flyctl not available or not logged in: " +
          String(err) +
          " — use the manual option.",
      );
    }
  }

  async function flyManual() {
    setFlyErr(null);
    try {
      const email = await invoke<string>("store_fly_token", {
        token: flyToken.trim(),
      });
      setFlyEmail(email);
      setFlyOk(true);
      setFlyToken("");
      setStep(4);
    } catch (err: any) {
      setFlyErr(String(err));
    }
  }

  async function save() {
    const cfg: AppConfig = {
      repo,
      branch: branch || "main",
      region: region || "fra",
      fly_org_slug: flyOrg || "personal",
      remote_image: image,
    };
    await invoke("save_config", { config: cfg });
    onDone();
  }

  return (
    <div className="container">
      <h1>Set up Toddler Claude</h1>
      <p className="muted">
        Three one-click steps (or a manual fallback for each). Stored encrypted
        on this computer.
      </p>

      {/* ───────── Step 1: Claude ───────── */}
      <section className={step === 1 ? "card active" : "card"}>
        <h2>
          1. Sign in to Claude {claudeOk && <span className="ok">✓</span>}
        </h2>
        {!showClaudeManual ? (
          <>
            <p>
              Click below. A browser window will open, you approve there, then
              come back here.
            </p>
            <button
              className="primary"
              onClick={claudeAuto}
              disabled={claudeBusy}
            >
              {claudeBusy ? "Waiting for browser approval…" : "Sign in with Claude"}
            </button>
            {claudeLog.length > 0 && (
              <pre className="log">
                {claudeLog.slice(-15).join("\n")}
              </pre>
            )}
            {claudeErr && <p className="err">{claudeErr}</p>}
            <p className="muted small">
              Works if Claude Code is installed. No terminal needed.
            </p>
            <button className="link" onClick={() => setShowClaudeManual(true)}>
              Or paste a token manually →
            </button>
          </>
        ) : (
          <>
            <p>
              In a terminal, run <code>claude setup-token</code>. Copy the long
              token it prints, paste below.
            </p>
            <input
              type="password"
              placeholder="sk-ant-oat… or oat_…"
              value={claudePaste}
              onChange={(e) => setClaudePaste(e.target.value)}
            />
            {claudeErr && <p className="err">{claudeErr}</p>}
            <button onClick={claudeManual} disabled={!claudePaste}>
              Save token
            </button>{" "}
            <button className="link" onClick={() => setShowClaudeManual(false)}>
              ← Back to one-click
            </button>
          </>
        )}
        {claudeOk && (
          <button className="link" onClick={() => setStep(2)}>
            Continue →
          </button>
        )}
      </section>

      {/* ───────── Step 2: GitHub ───────── */}
      <section className={step === 2 ? "card active" : "card"}>
        <h2>
          2. Connect GitHub {ghOk && <span className="ok">✓</span>}
          {ghLogin && <span className="muted"> — {ghLogin}</span>}
        </h2>
        {!showGhManual ? (
          <>
            <p>
              <strong>Fastest:</strong> if you already use the GitHub CLI (
              <code>gh</code>) on this computer, re-use that sign-in.
            </p>
            <button className="primary" onClick={githubCliFastPath}>
              Use my GitHub CLI sign-in
            </button>
            <p className="muted small" style={{ marginTop: 16 }}>
              Or sign in fresh via the browser:
            </p>
            {!device ? (
              <button onClick={githubDevice}>Sign in via browser</button>
            ) : (
              <div className="device-prompt">
                <p>Enter this code on GitHub:</p>
                <p className="code-big">{device.user_code}</p>
                <button
                  onClick={() => openExternal(device.verification_uri)}
                >
                  Open the page
                </button>
                <p className="muted">Waiting for you to approve…</p>
              </div>
            )}
            {ghErr && <p className="err">{ghErr}</p>}
            <button className="link" onClick={() => setShowGhManual(true)}>
              Or paste a Personal Access Token →
            </button>
          </>
        ) : (
          <>
            <p>
              Create a fine-grained token at{" "}
              <button
                className="link"
                onClick={() =>
                  openExternal(
                    "https://github.com/settings/personal-access-tokens/new",
                  )
                }
              >
                github.com/settings/personal-access-tokens/new
              </button>{" "}
              with <strong>Repository access: selected repos</strong> and
              permissions <strong>Contents (R/W)</strong>,{" "}
              <strong>Pull requests (R/W)</strong>, <strong>Metadata (R)</strong>.
            </p>
            <input
              type="password"
              placeholder="github_pat_…"
              value={ghPat}
              onChange={(e) => setGhPat(e.target.value)}
            />
            {ghErr && <p className="err">{ghErr}</p>}
            <button onClick={githubPat} disabled={!ghPat}>
              Save PAT
            </button>{" "}
            <button className="link" onClick={() => setShowGhManual(false)}>
              ← Back
            </button>
          </>
        )}
        {ghOk && (
          <button className="link" onClick={() => setStep(3)}>
            Continue →
          </button>
        )}
      </section>

      {/* ───────── Step 3: Fly.io ───────── */}
      <section className={step === 3 ? "card active" : "card"}>
        <h2>
          3. Connect Fly.io {flyOk && <span className="ok">✓</span>}
          {flyEmail && <span className="muted"> — {flyEmail}</span>}
        </h2>
        {!showFlyManual ? (
          <>
            <p>
              <strong>Fastest:</strong> if you have <code>flyctl</code> on this
              computer, re-use that sign-in.
            </p>
            <button className="primary" onClick={flyCliFastPath}>
              Use my flyctl sign-in
            </button>
            {flyErr && <p className="err">{flyErr}</p>}
            <button className="link" onClick={() => setShowFlyManual(true)}>
              Or paste a Fly token manually →
            </button>
          </>
        ) : (
          <>
            <p>
              1. Sign up free at{" "}
              <button
                className="link"
                onClick={() => openExternal("https://fly.io/app/sign-up")}
              >
                fly.io
              </button>{" "}
              (a card is required, even on the free tier).
              <br />
              2. Create a token at{" "}
              <button
                className="link"
                onClick={() =>
                  openExternal("https://fly.io/user/personal_access_tokens")
                }
              >
                fly.io/user/personal_access_tokens
              </button>
              .<br />
              3. Paste the token below.
            </p>
            <input
              type="password"
              placeholder="Paste Fly personal access token"
              value={flyToken}
              onChange={(e) => setFlyToken(e.target.value)}
            />
            <label>Fly org slug (usually "personal")</label>
            <input
              type="text"
              value={flyOrg}
              onChange={(e) => setFlyOrg(e.target.value)}
            />
            {flyErr && <p className="err">{flyErr}</p>}
            <button onClick={flyManual} disabled={!flyToken || !flyOrg}>
              Save Fly token
            </button>{" "}
            <button className="link" onClick={() => setShowFlyManual(false)}>
              ← Back
            </button>
          </>
        )}
        {flyOk && (
          <button className="link" onClick={() => setStep(4)}>
            Continue →
          </button>
        )}
      </section>

      {/* ───────── Step 4: Pick repo ───────── */}
      <section className={step === 4 ? "card active" : "card"}>
        <h2>4. Pick a project</h2>
        {repos.length === 0 ? (
          <p className="muted">
            No repos loaded yet. Complete the GitHub step first.
          </p>
        ) : (
          <>
            <label>Repository</label>
            <select value={repo} onChange={(e) => setRepo(e.target.value)}>
              <option value="">— choose a repo —</option>
              {repos.map((r) => (
                <option key={r.full_name} value={r.full_name}>
                  {r.full_name}
                  {r.private ? " (private)" : ""}
                </option>
              ))}
            </select>
            <label>Branch to work from</label>
            <input
              type="text"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
            />
            {flyOrgs.length > 0 && (
              <>
                <label>Fly org</label>
                <select value={flyOrg} onChange={(e) => setFlyOrg(e.target.value)}>
                  {flyOrgs.map((o) => (
                    <option key={o} value={o}>
                      {o}
                    </option>
                  ))}
                </select>
              </>
            )}
            <label>Fly region</label>
            <select value={region} onChange={(e) => setRegion(e.target.value)}>
              <option value="fra">Frankfurt (fra)</option>
              <option value="ams">Amsterdam (ams)</option>
              <option value="lhr">London (lhr)</option>
              <option value="iad">Ashburn US-East (iad)</option>
              <option value="ord">Chicago (ord)</option>
              <option value="sjc">San Jose (sjc)</option>
              <option value="sin">Singapore (sin)</option>
              <option value="syd">Sydney (syd)</option>
            </select>
            <details>
              <summary className="muted">Advanced</summary>
              <label>Remote image</label>
              <input
                type="text"
                value={image}
                onChange={(e) => setImage(e.target.value)}
              />
            </details>
            <button
              className="primary"
              onClick={save}
              disabled={!repo || !flyOrg}
            >
              Finish setup
            </button>
          </>
        )}
      </section>
    </div>
  );
}
