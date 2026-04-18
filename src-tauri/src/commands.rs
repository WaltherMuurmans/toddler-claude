use crate::credentials::{self, keys};
use crate::fly::FlyClient;
use crate::github_oauth::{self, DeviceStart, Repo};
use crate::session::{self, Session, StartParams};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

fn e<T: std::fmt::Display>(msg: T) -> String {
    msg.to_string()
}

#[tauri::command]
pub async fn store_claude_token(token: String) -> Result<(), String> {
    if !token.starts_with("sk-ant-oat") && !token.starts_with("oat_") {
        return Err("Token does not look like a Claude OAuth token.".into());
    }
    credentials::set(keys::CLAUDE_TOKEN, &token).map_err(e)
}

#[tauri::command]
pub async fn has_claude_token() -> Result<bool, String> {
    Ok(credentials::get(keys::CLAUDE_TOKEN).map_err(e)?.is_some())
}

#[tauri::command]
pub async fn clear_claude_token() -> Result<(), String> {
    credentials::delete(keys::CLAUDE_TOKEN).map_err(e)
}

#[tauri::command]
pub async fn github_device_start() -> Result<DeviceStart, String> {
    github_oauth::device_start().await.map_err(e)
}

#[tauri::command]
pub async fn github_device_poll(device_code: String) -> Result<Option<String>, String> {
    github_oauth::device_poll(&device_code).await.map_err(e)
}

#[tauri::command]
pub async fn store_github_token(token: String) -> Result<(), String> {
    credentials::set(keys::GITHUB_TOKEN, &token).map_err(e)
}

#[tauri::command]
pub async fn has_github_token() -> Result<bool, String> {
    Ok(credentials::get(keys::GITHUB_TOKEN).map_err(e)?.is_some())
}

#[tauri::command]
pub async fn list_github_repos() -> Result<Vec<Repo>, String> {
    let token = credentials::get(keys::GITHUB_TOKEN)
        .map_err(e)?
        .ok_or_else(|| "Not signed into GitHub yet.".to_string())?;
    github_oauth::list_repos(&token).await.map_err(e)
}

#[tauri::command]
pub async fn store_fly_token(token: String) -> Result<String, String> {
    let client = FlyClient::new(&token);
    let email = client.verify().await.map_err(e)?;
    credentials::set(keys::FLY_TOKEN, &token).map_err(e)?;
    Ok(email)
}

#[tauri::command]
pub async fn has_fly_token() -> Result<bool, String> {
    Ok(credentials::get(keys::FLY_TOKEN).map_err(e)?.is_some())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub repo: String,
    pub branch: String,
    pub region: String,
    pub fly_org_slug: String,
    pub remote_image: String,
}

fn config_path(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

#[tauri::command]
pub async fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    let path = config_path(&app);
    let json = serde_json::to_vec_pretty(&config).map_err(e)?;
    std::fs::write(path, json).map_err(e)
}

#[tauri::command]
pub async fn load_config(app: AppHandle) -> Result<AppConfig, String> {
    let path = config_path(&app);
    if !path.exists() {
        return Ok(AppConfig {
            branch: "main".into(),
            region: "fra".into(),
            remote_image: "ghcr.io/walthermuurmans/toddler-claude-remote:latest".into(),
            ..Default::default()
        });
    }
    let bytes = std::fs::read(path).map_err(e)?;
    serde_json::from_slice(&bytes).map_err(e)
}

#[tauri::command]
pub async fn start_session(app: AppHandle, config: AppConfig) -> Result<Session, String> {
    let fly = credentials::get(keys::FLY_TOKEN)
        .map_err(e)?
        .ok_or_else(|| "Fly token missing. Open Setup.".to_string())?;
    let claude = credentials::get(keys::CLAUDE_TOKEN)
        .map_err(e)?
        .ok_or_else(|| "Claude token missing. Open Setup.".to_string())?;
    let gh = credentials::get(keys::GITHUB_TOKEN)
        .map_err(e)?
        .ok_or_else(|| "GitHub token missing. Open Setup.".to_string())?;

    let params = StartParams {
        repo: config.repo,
        branch: if config.branch.is_empty() {
            "main".into()
        } else {
            config.branch
        },
        region: if config.region.is_empty() {
            "fra".into()
        } else {
            config.region
        },
        org_slug: config.fly_org_slug,
        image: config.remote_image,
    };
    session::start(&app, fly, claude, gh, params).await.map_err(e)
}

#[tauri::command]
pub async fn stop_session(app: AppHandle) -> Result<(), String> {
    let fly = credentials::get(keys::FLY_TOKEN)
        .map_err(e)?
        .ok_or_else(|| "Fly token missing.".to_string())?;
    session::stop(&app, fly).await.map_err(e)
}

#[tauri::command]
pub async fn session_status(app: AppHandle) -> Result<Option<Session>, String> {
    let state = app.state::<session::SessionState>();
    let guard = state.inner.lock().unwrap();
    Ok(guard.clone())
}
