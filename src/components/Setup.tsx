import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

const openExternal = (url: string) => openUrl(url);
import { AppConfig } from "../App";

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

  const [claudePaste, setClaudePaste] = useState("");
  const [claudeErr, setClaudeErr] = useState<string | null>(null);

  const [flyToken, setFlyToken] = useState("");
  const [flyOrg, setFlyOrg] = useState(initialConfig?.fly_org_slug ?? "");
  const [flyEmail, setFlyEmail] = useState<string | null>(null);
  const [flyErr, setFlyErr] = useState<string | null>(null);

  const [device, setDevice] = useState<DeviceStart | null>(null);
  const [ghErr, setGhErr] = useState<string | null>(null);

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
  }, []);

  async function submitClaude() {
    setClaudeErr(null);
    try {
      await invoke("store_claude_token", { token: claudePaste.trim() });
      setClaudeOk(true);
      setClaudePaste("");
      setStep(2);
    } catch (e: any) {
      setClaudeErr(String(e));
    }
  }

  async function startGithub() {
    setGhErr(null);
    try {
      const d = await invoke<DeviceStart>("github_device_start");
      setDevice(d);
      await openExternal(d.verification_uri);
      const deadline = Date.now() + 900 * 1000;
      const poll = async () => {
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
    } catch (e: any) {
      setGhErr(String(e));
    }
  }

  async function submitFly() {
    setFlyErr(null);
    try {
      const email = await invoke<string>("store_fly_token", {
        token: flyToken.trim(),
      });
      setFlyEmail(email);
      setFlyOk(true);
      setFlyToken("");
      setStep(4);
    } catch (e: any) {
      setFlyErr(String(e));
    }
  }

  async function save() {
    const cfg: AppConfig = {
      repo,
      branch: branch || "main",
      region: region || "fra",
      fly_org_slug: flyOrg,
      remote_image: image,
    };
    await invoke("save_config", { config: cfg });
    onDone();
  }

  return (
    <div className="container">
      <h1>Set up Toddler Claude</h1>
      <p className="muted">
        Four one-time steps. This info is stored encrypted on your computer.
      </p>

      <section className={step === 1 ? "card active" : "card"}>
        <h2>
          1. Sign in to Claude {claudeOk && <span className="ok">✓</span>}
        </h2>
        <p>
          On your keyboard, open a terminal and run:
          <br />
          <code>claude setup-token</code>
        </p>
        <p>
          After you approve in the browser, copy the long token that appears,
          then paste it here.
        </p>
        <input
          type="password"
          placeholder="Paste Claude token (starts with sk-ant-oat… or oat_…)"
          value={claudePaste}
          onChange={(e) => setClaudePaste(e.target.value)}
        />
        {claudeErr && <p className="err">{claudeErr}</p>}
        <button onClick={submitClaude} disabled={!claudePaste}>
          Save Claude token
        </button>
        {claudeOk && (
          <button className="link" onClick={() => setStep(2)}>
            Continue →
          </button>
        )}
      </section>

      <section className={step === 2 ? "card active" : "card"}>
        <h2>
          2. Connect GitHub {ghOk && <span className="ok">✓</span>}
        </h2>
        {!device ? (
          <>
            <p>We’ll open GitHub in your browser so you can approve access.</p>
            <button onClick={startGithub}>Connect GitHub</button>
            {ghErr && <p className="err">{ghErr}</p>}
          </>
        ) : (
          <>
            <p>Enter this code on GitHub:</p>
            <p className="code-big">{device.user_code}</p>
            <button
              className="link"
              onClick={() => openExternal(device.verification_uri)}
            >
              Open github.com/login/device
            </button>
            <p className="muted">Waiting for you to approve…</p>
          </>
        )}
        {ghOk && (
          <button className="link" onClick={() => setStep(3)}>
            Continue →
          </button>
        )}
      </section>

      <section className={step === 3 ? "card active" : "card"}>
        <h2>
          3. Connect Fly.io {flyOk && <span className="ok">✓</span>}
        </h2>
        <p>
          Fly.io runs the remote coding machines. Sign up free at{" "}
          <button className="link" onClick={() => openExternal("https://fly.io/app/sign-up")}>
            fly.io
          </button>{" "}
          then generate a personal access token at{" "}
          <button
            className="link"
            onClick={() =>
              openExternal("https://fly.io/user/personal_access_tokens")
            }
          >
            Personal access tokens
          </button>
          .
        </p>
        <input
          type="password"
          placeholder="Paste Fly.io personal access token"
          value={flyToken}
          onChange={(e) => setFlyToken(e.target.value)}
        />
        <input
          type="text"
          placeholder="Fly org slug (usually 'personal')"
          value={flyOrg}
          onChange={(e) => setFlyOrg(e.target.value)}
        />
        {flyErr && <p className="err">{flyErr}</p>}
        {flyEmail && <p className="ok">Connected as {flyEmail}</p>}
        <button onClick={submitFly} disabled={!flyToken || !flyOrg}>
          Save Fly token
        </button>
        {flyOk && (
          <button className="link" onClick={() => setStep(4)}>
            Continue →
          </button>
        )}
      </section>

      <section className={step === 4 ? "card active" : "card"}>
        <h2>4. Pick a project</h2>
        {repos.length === 0 ? (
          <p className="muted">Loading repos… (finish step 2 first)</p>
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
