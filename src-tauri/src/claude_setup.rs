//! Spawns `claude setup-token` as a subprocess, parses its streaming output to
//! auto-open the browser and capture the resulting OAuth token — so the user
//! never has to touch a terminal.

use anyhow::{anyhow, Result};
use std::process::Stdio;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

fn claude_binary() -> String {
    if let Ok(p) = which::which("claude") {
        return p.to_string_lossy().to_string();
    }
    // Common Windows install locations
    if let Some(home) = dirs::home_dir() {
        let p1 = home.join(".local").join("bin").join("claude.exe");
        if p1.exists() {
            return p1.to_string_lossy().to_string();
        }
        let p2 = home.join("AppData\\Roaming\\npm\\claude.cmd");
        if p2.exists() {
            return p2.to_string_lossy().to_string();
        }
    }
    "claude".to_string()
}

/// Extracts an OAuth token from any line. Returns Some(token) on match.
fn extract_token(line: &str) -> Option<String> {
    // sk-ant-oat01-... or oat_... patterns; tokens are long (>30 chars).
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
        if (t.starts_with("sk-ant-oat") || t.starts_with("oat_")) && t.len() > 30 {
            return Some(t.to_string());
        }
    }
    None
}

fn extract_url(line: &str) -> Option<String> {
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c: char| c == '"' || c == '\'' || c == ')' || c == ',');
        if (t.starts_with("https://claude.ai/") || t.starts_with("https://console.anthropic.com/"))
            && t.len() < 400
        {
            return Some(t.to_string());
        }
    }
    None
}

pub async fn run(app: AppHandle) -> Result<String> {
    let bin = claude_binary();
    let mut cmd = Command::new(&bin);
    cmd.arg("setup-token");
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("failed to spawn `{}` (is Claude Code installed?): {}", bin, e))?;

    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    let tx2 = tx.clone();

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(line).await;
        }
    });
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx2.send(line).await;
        }
    });

    let mut opened_browser = false;
    let mut token: Option<String> = None;
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(10 * 60);

    while std::time::Instant::now() < deadline {
        let maybe = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await;
        match maybe {
            Ok(Some(line)) => {
                let _ = app.emit("claude-setup-log", &line);
                if !opened_browser {
                    if let Some(url) = extract_url(&line) {
                        opened_browser = true;
                        let _ = app.emit("claude-setup-log", format!("→ opening browser: {}", url));
                        let _ = webbrowser::open(&url);
                    }
                }
                if let Some(t) = extract_token(&line) {
                    token = Some(t);
                    break;
                }
            }
            Ok(None) => break, // channel closed
            Err(_) => {
                // periodic poll — check if child exited
                if let Ok(Some(_status)) = child.try_wait() {
                    // drain a bit more
                    while let Ok(Some(l)) = tokio::time::timeout(
                        std::time::Duration::from_millis(100),
                        rx.recv(),
                    )
                    .await
                    {
                        let _ = app.emit("claude-setup-log", &l);
                        if let Some(t) = extract_token(&l) {
                            token = Some(t);
                        }
                    }
                    break;
                }
            }
        }
    }

    // Cleanup
    let _ = child.kill().await;

    token.ok_or_else(|| anyhow!("did not detect a Claude OAuth token in `claude setup-token` output within 10 minutes"))
}
