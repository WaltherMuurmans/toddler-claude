use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

pub async fn device_start() -> Result<DeviceStart> {
    let client = Client::new();
    let res: Value = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", CLIENT_ID),
            ("scope", "repo read:user read:org"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(DeviceStart {
        device_code: res["device_code"].as_str().unwrap_or_default().to_string(),
        user_code: res["user_code"].as_str().unwrap_or_default().to_string(),
        verification_uri: res["verification_uri"]
            .as_str()
            .unwrap_or("https://github.com/login/device")
            .to_string(),
        expires_in: res["expires_in"].as_i64().unwrap_or(900),
        interval: res["interval"].as_i64().unwrap_or(5),
    })
}

pub async fn device_poll(device_code: &str) -> Result<Option<String>> {
    let client = Client::new();
    let res: Value = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    if let Some(token) = res["access_token"].as_str() {
        return Ok(Some(token.to_string()));
    }
    if let Some(err) = res["error"].as_str() {
        match err {
            "authorization_pending" | "slow_down" => return Ok(None),
            _ => return Err(anyhow!("github device auth error: {}", err)),
        }
    }
    Ok(None)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Repo {
    pub full_name: String,
    pub private: bool,
    pub default_branch: String,
}

pub async fn list_repos(token: &str) -> Result<Vec<Repo>> {
    let client = Client::new();
    let mut repos = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!(
            "https://api.github.com/user/repos?per_page=100&sort=updated&page={}",
            page
        );
        let batch: Vec<Value> = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "ToddlerClaude/0.1")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if batch.is_empty() {
            break;
        }
        for r in &batch {
            repos.push(Repo {
                full_name: r["full_name"].as_str().unwrap_or_default().to_string(),
                private: r["private"].as_bool().unwrap_or(false),
                default_branch: r["default_branch"]
                    .as_str()
                    .unwrap_or("main")
                    .to_string(),
            });
        }
        if batch.len() < 100 {
            break;
        }
        page += 1;
        if page > 10 {
            break;
        }
    }
    Ok(repos)
}
