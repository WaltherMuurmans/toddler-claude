//! Fast paths for Fly.io credential discovery. Fly has no public 3rd-party OAuth;
//! we can at best reuse an existing `flyctl` CLI login on the user's machine.

use anyhow::{anyhow, Result};
use std::process::Command;

pub fn flyctl_token() -> Result<String> {
    let out = Command::new("flyctl")
        .args(["auth", "token"])
        .output()
        .map_err(|e| anyhow!("flyctl not found on PATH: {}", e))?;
    if !out.status.success() {
        return Err(anyhow!(
            "flyctl auth token failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if token.is_empty() {
        return Err(anyhow!(
            "flyctl auth token returned empty; run `flyctl auth login` first"
        ));
    }
    Ok(token)
}

pub async fn list_orgs(token: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "query": "query { viewer { organizations { nodes { slug name } } } }"
    });
    let resp: serde_json::Value = client
        .post("https://api.fly.io/graphql")
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut out = Vec::new();
    if let Some(nodes) = resp["data"]["viewer"]["organizations"]["nodes"].as_array() {
        for n in nodes {
            if let Some(slug) = n["slug"].as_str() {
                out.push(slug.to_string());
            }
        }
    }
    if out.is_empty() {
        out.push("personal".into());
    }
    Ok(out)
}
