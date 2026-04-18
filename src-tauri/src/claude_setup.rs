//! Spawns `claude setup-token` as a subprocess in a pseudo-terminal (ConPTY
//! on Windows) so the bun-based Claude Code CLI thinks it has a real TTY,
//! captures the token it prints after browser approval, and auto-opens the
//! browser URL it emits — so the user never touches a terminal.

use anyhow::{anyhow, Result};
use portable_pty::{CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// Remove ANSI escape sequences (CSI, OSC, simple ESC) from a string.
fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            // ESC. Skip the whole sequence.
            if i + 1 < bytes.len() {
                let n = bytes[i + 1];
                if n == b'[' {
                    // CSI: ESC [ params (0x30-0x3F) intermediate (0x20-0x2F) final (0x40-0x7E)
                    i += 2;
                    while i < bytes.len()
                        && !((0x40..=0x7e).contains(&bytes[i]))
                    {
                        i += 1;
                    }
                    i += 1;
                    continue;
                } else if n == b']' {
                    // OSC: ESC ] ... BEL or ESC \
                    i += 2;
                    while i < bytes.len() && bytes[i] != 0x07 && bytes[i] != 0x1b {
                        i += 1;
                    }
                    if i < bytes.len() && bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                } else {
                    // Two-char escape
                    i += 2;
                    continue;
                }
            } else {
                break;
            }
        }
        // also drop other control chars except \n, \r, \t
        if b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t' {
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Find a Claude OAuth token in a line. Matches `sk-ant-oat…`, `oat_…`, or any
/// long base64url-like string that is plausibly a token.
fn extract_token(line: &str) -> Option<String> {
    // Try to match a token on its own whitespace-separated token first
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c: char| {
            !c.is_ascii_alphanumeric() && c != '-' && c != '_'
        });
        if (t.starts_with("sk-ant-oat") || t.starts_with("oat_")) && t.len() > 40 {
            return Some(t.to_string());
        }
    }
    // Fallback: look inside the raw line for the known prefixes (in case of
    // surrounding characters the trim didn't remove)
    for needle in ["sk-ant-oat", "oat_"] {
        if let Some(pos) = line.find(needle) {
            let rest = &line[pos..];
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                .unwrap_or(rest.len());
            let candidate = &rest[..end];
            if candidate.len() > 40 {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

fn extract_url(line: &str) -> Option<String> {
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c: char| {
            c == '"' || c == '\'' || c == ')' || c == ',' || c == '.' || c == '<' || c == '>'
        });
        if (t.starts_with("https://claude.ai/")
            || t.starts_with("https://console.anthropic.com/")
            || t.starts_with("https://accounts.anthropic.com/"))
            && t.len() < 500
        {
            return Some(t.to_string());
        }
    }
    None
}

pub async fn run(app: AppHandle) -> Result<String> {
    let bin = crate::tool_discovery::find_claude().ok_or_else(|| {
        anyhow!(
            "Claude Code not found. Install from https://claude.ai/install (or run `winget install Anthropic.ClaudeCode`)."
        )
    })?;

    let app_clone = app.clone();
    let token = tokio::task::spawn_blocking(move || run_blocking(app_clone, bin))
        .await
        .map_err(|e| anyhow!("claude-setup task panic: {}", e))??;
    Ok(token)
}

fn run_blocking(app: AppHandle, bin: std::path::PathBuf) -> Result<String> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow!("openpty: {}", e))?;

    let mut cmd = CommandBuilder::new(&bin);
    cmd.arg("setup-token");
    // Inherit PATH & USERPROFILE so claude can locate its config
    if let Some(cwd) = dirs::home_dir() {
        cmd.cwd(cwd);
    }
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    // Steer claude toward plain output
    cmd.env("NO_COLOR", "1");
    cmd.env("CLICOLOR", "0");
    cmd.env("TERM", "xterm-256color");

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow!("spawn: {}", e))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow!("clone reader: {}", e))?;
    let writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|e| anyhow!("take writer: {}", e))?,
    ));

    let token_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let token_slot_read = token_slot.clone();
    let app_read = app.clone();
    let writer_for_read = writer.clone();

    let reader_thread = std::thread::spawn(move || -> Result<()> {
        let mut buf = [0u8; 4096];
        let mut accumulated = String::new();
        let mut opened_browser = false;
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            let chunk = String::from_utf8_lossy(&buf[..n]);
            accumulated.push_str(&chunk);

            // Process complete lines
            while let Some(pos) = accumulated.find('\n') {
                let raw = accumulated[..pos].to_string();
                accumulated.drain(..=pos);
                let line = strip_ansi(&raw);
                let trimmed = line.trim_end_matches('\r').to_string();
                if !trimmed.is_empty() {
                    let _ = app_read.emit("claude-setup-log", &trimmed);
                }
                if !opened_browser {
                    if let Some(url) = extract_url(&trimmed) {
                        opened_browser = true;
                        let _ = app_read
                            .emit("claude-setup-log", format!("→ opening: {}", url));
                        let _ = webbrowser::open(&url);
                    }
                }
                if let Some(t) = extract_token(&trimmed) {
                    *token_slot_read.lock().unwrap() = Some(t);
                }
            }

            // Also check the tail for token or URL before newline
            let tail_line = strip_ansi(&accumulated);
            if !opened_browser {
                if let Some(url) = extract_url(&tail_line) {
                    opened_browser = true;
                    let _ = app_read
                        .emit("claude-setup-log", format!("→ opening: {}", url));
                    let _ = webbrowser::open(&url);
                }
            }
            if token_slot_read.lock().unwrap().is_none() {
                if let Some(t) = extract_token(&tail_line) {
                    *token_slot_read.lock().unwrap() = Some(t);
                }
            }

            // If we have a token, try to send enter/exit politely
            if token_slot_read.lock().unwrap().is_some() {
                if let Ok(mut w) = writer_for_read.lock() {
                    let _ = w.write_all(b"\r\n");
                    let _ = w.flush();
                }
                // keep draining a bit more, but we can stop soon
            }
        }
        Ok(())
    });

    // Wait up to 10 minutes for the process; check token periodically
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10 * 60);
    loop {
        if token_slot.lock().unwrap().is_some() {
            // Give the process a moment to settle, then kill
            std::thread::sleep(std::time::Duration::from_millis(300));
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            // Process exited; give reader a moment to drain
            std::thread::sleep(std::time::Duration::from_millis(500));
            break;
        }
        if std::time::Instant::now() > deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    let _ = child.kill();
    let _ = reader_thread.join();

    let token = token_slot.lock().unwrap().clone();
    token.ok_or_else(|| anyhow!(
        "did not detect a Claude token in the output within 10 minutes. Approve in the browser, then if nothing happens use the manual option."
    ))
}
