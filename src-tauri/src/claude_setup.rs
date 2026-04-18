//! Spawns `claude setup-token` inside a ConPTY so the bun CLI sees a real
//! terminal, then captures the token it prints after browser approval.

use anyhow::{anyhow, Result};
use portable_pty::{CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Remove ANSI escape sequences (CSI, OSC, simple ESC) and other control
/// bytes (keep \n \r \t) from a string.
fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            if i + 1 < bytes.len() {
                let n = bytes[i + 1];
                if n == b'[' {
                    i += 2;
                    while i < bytes.len() && !((0x40..=0x7e).contains(&bytes[i])) {
                        i += 1;
                    }
                    i += 1;
                    continue;
                } else if n == b']' {
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
                    i += 2;
                    continue;
                }
            } else {
                break;
            }
        }
        if b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t' {
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn extract_token(line: &str) -> Option<String> {
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

fn extract_url(text: &str) -> Option<String> {
    // Find the first https:// substring that contains anthropic or claude domain.
    let lower = text.to_lowercase();
    let mut search_from = 0usize;
    while let Some(rel_pos) = lower[search_from..].find("https://") {
        let pos = search_from + rel_pos;
        let rest_chars: Vec<char> = text[pos..].chars().collect();
        let mut end = 0usize;
        for c in rest_chars.iter() {
            if c.is_whitespace()
                || *c == '"'
                || *c == '\''
                || *c == '<'
                || *c == '>'
                || *c == '`'
                || *c == '('
                || *c == ')'
                || *c == '[' || *c == ']'
                || *c == ','
                || *c == ';'
            {
                break;
            }
            end += c.len_utf8();
        }
        if end > 15 {
            let url = text[pos..pos + end].to_string();
            let url_lower = url.to_lowercase();
            if url_lower.contains("anthropic.com")
                || url_lower.contains("claude.ai")
                || url_lower.contains("claude.com")
            {
                return Some(url);
            }
        }
        search_from = pos + 8;
    }
    None
}

/// Open a URL on Windows in a way that doesn't block behind ShellExecute
/// weirdness: use `cmd /c start "" <url>`.
fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()?;
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
        Ok(())
    }
}

pub async fn run(app: AppHandle) -> Result<String> {
    let bin = crate::tool_discovery::find_claude().ok_or_else(|| {
        anyhow!(
            "Claude Code not found. Install via `winget install Anthropic.ClaudeCode` or from https://claude.ai/install."
        )
    })?;
    let app_clone = app.clone();
    let bin_clone = bin.clone();
    let token = tokio::task::spawn_blocking(move || run_blocking(app_clone, bin_clone))
        .await
        .map_err(|e| anyhow!("claude-setup task join: {}", e))??;
    Ok(token)
}

fn log(app: &AppHandle, start: Instant, msg: impl std::fmt::Display) {
    let ms = start.elapsed().as_millis();
    let _ = app.emit("claude-setup-log", format!("[+{ms}ms] {msg}"));
}

fn run_blocking(app: AppHandle, bin: std::path::PathBuf) -> Result<String> {
    let start = Instant::now();
    log(&app, start, format!("spawning: {}", bin.display()));

    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 140,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow!("openpty: {}", e))?;

    let mut cmd = CommandBuilder::new(&bin);
    cmd.arg("setup-token");
    if let Some(cwd) = dirs::home_dir() {
        cmd.cwd(cwd);
    }
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    // Tell Claude to behave non-interactively with no color
    cmd.env("NO_COLOR", "1");
    cmd.env("CLICOLOR", "0");
    cmd.env("TERM", "xterm-256color");
    // Some CLIs respect BROWSER=""—but Claude uses its own opener; we open it ourselves.

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow!("spawn: {}", e))?;
    drop(pair.slave);
    log(&app, start, "spawned, waiting for output…");

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow!("clone reader: {}", e))?;
    let _writer = pair
        .master
        .take_writer()
        .map_err(|e| anyhow!("take writer: {}", e))?;

    let token_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let url_opened: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    let token_slot_r = token_slot.clone();
    let url_opened_r = url_opened.clone();
    let app_r = app.clone();

    let reader_thread = std::thread::spawn(move || -> Result<()> {
        let mut buf = [0u8; 4096];
        let mut accumulated = String::new();
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            let chunk = String::from_utf8_lossy(&buf[..n]);
            accumulated.push_str(&chunk);
            let clean = strip_ansi(&accumulated);

            // Always emit whatever new lines came through so user sees progress
            while let Some(pos) = accumulated.find('\n') {
                let raw_line = accumulated[..pos].to_string();
                accumulated.drain(..=pos);
                let line = strip_ansi(&raw_line);
                let trimmed = line.trim_end_matches('\r').trim().to_string();
                if !trimmed.is_empty() {
                    log(&app_r, start, &trimmed);
                }
            }

            // Check URL in the full cleaned buffer (cover partial lines)
            if !*url_opened_r.lock().unwrap() {
                if let Some(url) = extract_url(&clean) {
                    *url_opened_r.lock().unwrap() = true;
                    log(&app_r, start, format!("URL captured → opening browser: {}", url));
                    match open_browser(&url) {
                        Ok(_) => log(&app_r, start, "browser opener spawned"),
                        Err(e) => log(&app_r, start, format!("browser opener error: {}", e)),
                    }
                }
            }

            // Check token in the cleaned buffer
            if token_slot_r.lock().unwrap().is_none() {
                if let Some(t) = extract_token(&clean) {
                    log(&app_r, start, format!("token captured ({} chars)", t.len()));
                    *token_slot_r.lock().unwrap() = Some(t);
                }
            }
        }
        Ok(())
    });

    let deadline = Instant::now() + Duration::from_secs(10 * 60);
    loop {
        if token_slot.lock().unwrap().is_some() {
            std::thread::sleep(Duration::from_millis(200));
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            log(&app, start, "claude process exited; draining output");
            std::thread::sleep(Duration::from_millis(400));
            break;
        }
        if Instant::now() > deadline {
            log(&app, start, "10-minute deadline reached, giving up");
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let _ = child.kill();
    let _ = reader_thread.join();

    let token = token_slot.lock().unwrap().clone();
    token.ok_or_else(|| {
        anyhow!(
            "no token captured — paste manually using the option below. Output log is visible in the app."
        )
    })
}
