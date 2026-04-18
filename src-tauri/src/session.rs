use crate::fly::{FlyClient, Machine, MachineSpec};
use anyhow::{anyhow, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const HARD_LIMIT_SECONDS: u64 = 2 * 60 * 60;
const IDLE_POLL_INTERVAL: u64 = 30;

#[derive(Debug, Default)]
pub struct SessionState {
    pub inner: Mutex<Option<Session>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub app_name: String,
    pub machine_id: String,
    pub hostname: String,
    pub password: String,
    pub started_at: u64,
    pub region: String,
    pub repo: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartParams {
    pub repo: String,
    pub branch: String,
    pub region: String,
    pub org_slug: String,
    pub image: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn gen_password() -> String {
    let mut buf = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut buf);
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(buf)
}

pub async fn start(
    app: &AppHandle,
    fly_token: String,
    claude_token: String,
    github_token: String,
    params: StartParams,
) -> Result<Session> {
    let state = app.state::<SessionState>();
    {
        let guard = state.inner.lock().unwrap();
        if guard.is_some() {
            return Err(anyhow!(
                "A session is already running. Stop it before starting another."
            ));
        }
    }

    let client = FlyClient::new(fly_token);
    let uuid = uuid::Uuid::new_v4().to_string();
    let short = uuid.split('-').next().unwrap_or("sess").to_string();
    let app_name = format!("toddler-{}", short);

    client.ensure_app(&app_name, &params.org_slug).await?;
    let _ = client.allocate_ipv4(&app_name).await;

    let password = gen_password();
    let repo_sanitized = params.repo.replace('/', "__");

    let mut env: HashMap<String, String> = HashMap::new();
    env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), claude_token);
    env.insert("GH_TOKEN".into(), github_token);
    env.insert("REPO".into(), params.repo.clone());
    env.insert("BRANCH".into(), params.branch.clone());
    env.insert("SESSION_PASS".into(), password.clone());
    env.insert("SESSION_ID".into(), short.clone());
    env.insert("HARD_LIMIT_SECONDS".into(), HARD_LIMIT_SECONDS.to_string());
    env.insert("REPO_DIR".into(), repo_sanitized);

    let spec = MachineSpec {
        app_name: app_name.clone(),
        region: params.region.clone(),
        image: params.image,
        cpu_kind: "shared".into(),
        cpus: 2,
        memory_mb: 2048,
        env,
    };

    let machine: Machine = client.create_machine(&spec).await?;
    client.wait_started(&app_name, &machine.id, 180).await?;

    let hostname = format!("{}.fly.dev", app_name);

    let sess = Session {
        id: short,
        app_name,
        machine_id: machine.id,
        hostname,
        password,
        started_at: now_secs(),
        region: params.region,
        repo: params.repo,
        branch: params.branch,
    };

    {
        let mut guard = state.inner.lock().unwrap();
        *guard = Some(sess.clone());
    }
    Ok(sess)
}

pub async fn stop(app: &AppHandle, fly_token: String) -> Result<()> {
    let sess = {
        let state = app.state::<SessionState>();
        let mut guard = state.inner.lock().unwrap();
        guard.take()
    };
    if let Some(s) = sess {
        let client = FlyClient::new(fly_token);
        let _ = client.destroy_machine(&s.app_name, &s.machine_id).await;
        let _ = client.destroy_app(&s.app_name).await;
    }
    Ok(())
}

pub async fn idle_reaper(app: AppHandle) {
    loop {
        tokio::time::sleep(Duration::from_secs(IDLE_POLL_INTERVAL)).await;
        let (needs_stop, fly_token) = {
            let state = app.state::<SessionState>();
            let guard = state.inner.lock().unwrap();
            if let Some(s) = guard.as_ref() {
                let age = now_secs().saturating_sub(s.started_at);
                let expired = age >= HARD_LIMIT_SECONDS;
                let token = crate::credentials::get(crate::credentials::keys::FLY_TOKEN)
                    .ok()
                    .flatten();
                (expired, token)
            } else {
                (false, None)
            }
        };
        if needs_stop {
            if let Some(token) = fly_token {
                log::warn!("Hard session limit reached; destroying machine.");
                let _ = stop(&app, token).await;
                let _ = app.emit("session-hard-stopped", ());
            }
        }
    }
}
