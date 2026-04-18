//! Runs `claude setup-token` in a **hidden** real Windows console so Ink's
//! raw-mode TTY check passes (portable-pty's ConPTY wasn't enough for
//! bun-bundled Ink). Redirects stdout+stderr to a temp file; we poll the
//! file for the URL (to open the browser) and the final OAuth token.

use anyhow::{anyhow, Result};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b && i + 1 < bytes.len() {
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
    let lower = text.to_lowercase();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("https://") {
        let pos = search_from + rel;
        let rest = &text[pos..];
        let end = rest
            .find(|c: char| {
                c.is_whitespace()
                    || matches!(c, '"' | '\'' | '<' | '>' | '`' | '(' | ')' | '[' | ']' | ',' | ';')
            })
            .unwrap_or(rest.len());
        let url = &rest[..end];
        let ul = url.to_lowercase();
        if url.len() > 15
            && (ul.contains("anthropic.com") || ul.contains("claude.ai") || ul.contains("claude.com"))
        {
            return Some(url.to_string());
        }
        search_from = pos + 8;
    }
    None
}

fn open_browser_detached(url: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

fn log(app: &AppHandle, start: Instant, msg: impl std::fmt::Display) {
    let ms = start.elapsed().as_millis();
    let _ = app.emit("claude-setup-log", format!("[+{ms}ms] {msg}"));
}

/// Spawn `cmd.exe /c "<claude> setup-token > <temp> 2>&1"` with a hidden
/// new console. Returns the Windows PROCESS_INFORMATION so caller can wait/kill.
#[cfg(windows)]
fn spawn_hidden_console(command_line: &str) -> Result<HiddenChild> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{
        CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
        CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_CREATION_FLAGS,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let mut cmdline_w: Vec<u16> = OsStr::new(command_line).encode_wide().chain([0]).collect();

    let mut si = STARTUPINFOW::default();
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    si.dwFlags = STARTF_USESHOWWINDOW;
    si.wShowWindow = SW_HIDE.0 as u16;

    let mut pi = PROCESS_INFORMATION::default();

    unsafe {
        CreateProcessW(
            None,
            PWSTR::from_raw(cmdline_w.as_mut_ptr()),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(CREATE_NEW_CONSOLE.0 | CREATE_UNICODE_ENVIRONMENT.0),
            None,
            None,
            &si,
            &mut pi,
        )
        .map_err(|e| anyhow!("CreateProcessW: {e}"))?;
    }

    Ok(HiddenChild {
        process: pi.hProcess,
        thread: pi.hThread,
        pid: pi.dwProcessId,
    })
}

#[cfg(windows)]
struct HiddenChild {
    process: windows::Win32::Foundation::HANDLE,
    thread: windows::Win32::Foundation::HANDLE,
    pid: u32,
}

#[cfg(windows)]
impl HiddenChild {
    fn try_wait(&self) -> Option<u32> {
        use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        unsafe {
            let w = WaitForSingleObject(self.process, 0);
            if w == WAIT_OBJECT_0 {
                let mut code = 0u32;
                let _ = GetExitCodeProcess(self.process, &mut code);
                Some(code)
            } else if w == WAIT_TIMEOUT {
                None
            } else {
                Some(u32::MAX)
            }
        }
    }

    fn kill(&self) {
        use windows::Win32::System::Threading::TerminateProcess;
        unsafe {
            let _ = TerminateProcess(self.process, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for HiddenChild {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        unsafe {
            let _ = CloseHandle(self.process);
            let _ = CloseHandle(self.thread);
        }
    }
}

fn temp_output_file() -> PathBuf {
    let dir = std::env::temp_dir();
    let name = format!(
        "toddler-claude-setup-{}.log",
        uuid::Uuid::new_v4().as_simple()
    );
    dir.join(name)
}

pub async fn run(app: AppHandle) -> Result<String> {
    let bin = crate::tool_discovery::find_claude().ok_or_else(|| {
        anyhow!(
            "Claude Code not found. Install via `winget install Anthropic.ClaudeCode`."
        )
    })?;
    let app_clone = app.clone();
    let bin_clone = bin.clone();
    let token = tokio::task::spawn_blocking(move || run_blocking(app_clone, bin_clone))
        .await
        .map_err(|e| anyhow!("claude-setup task join: {}", e))??;
    Ok(token)
}

#[cfg(windows)]
fn run_blocking(app: AppHandle, bin: PathBuf) -> Result<String> {
    let start = Instant::now();
    log(&app, start, format!("spawning: {}", bin.display()));

    let tempfile = temp_output_file();
    // Pre-create the file so our polling never trips on "file not found".
    std::fs::write(&tempfile, b"").ok();

    // cmd.exe handles the `>` redirect. Output (stdout + stderr) goes to tempfile.
    // Stdin inherits from the new hidden console, which IS a TTY — so Ink is happy.
    let cmdline = format!(
        "cmd.exe /c \"\"{}\" setup-token > \"{}\" 2>&1\"",
        bin.display(),
        tempfile.display()
    );
    log(&app, start, format!("cmdline: {cmdline}"));

    let child = spawn_hidden_console(&cmdline)?;
    log(
        &app,
        start,
        format!("spawned PID {} (hidden console); polling {}…", child.pid, tempfile.display()),
    );

    let mut opened_browser = false;
    let mut found_token: Option<String> = None;
    let mut seen_bytes: u64 = 0;
    let deadline = Instant::now() + Duration::from_secs(10 * 60);

    loop {
        std::thread::sleep(Duration::from_millis(300));

        if let Ok(bytes) = std::fs::read(&tempfile) {
            if bytes.len() as u64 > seen_bytes {
                seen_bytes = bytes.len() as u64;
                let text = String::from_utf8_lossy(&bytes).to_string();
                let clean = strip_ansi(&text);

                // Emit last ~400 chars to frontend so user sees progress
                let tail: String = clean.chars().rev().take(400).collect::<String>().chars().rev().collect();
                let last_line = tail.lines().last().unwrap_or(&tail).trim();
                if !last_line.is_empty() {
                    log(&app, start, format!("output: {}", last_line));
                }

                if !opened_browser {
                    if let Some(url) = extract_url(&clean) {
                        opened_browser = true;
                        log(&app, start, format!("URL → {}", url));
                        match open_browser_detached(&url) {
                            Ok(_) => log(&app, start, "browser spawn ok"),
                            Err(e) => log(&app, start, format!("browser spawn error: {}", e)),
                        }
                    }
                }

                if let Some(t) = extract_token(&clean) {
                    log(&app, start, format!("token captured ({} chars)", t.len()));
                    found_token = Some(t);
                    break;
                }
            }
        }

        if child.try_wait().is_some() {
            log(&app, start, "claude process exited; draining");
            std::thread::sleep(Duration::from_millis(300));
            // Final drain
            if let Ok(bytes) = std::fs::read(&tempfile) {
                let text = String::from_utf8_lossy(&bytes).to_string();
                let clean = strip_ansi(&text);
                if found_token.is_none() {
                    if let Some(t) = extract_token(&clean) {
                        found_token = Some(t);
                    }
                }
                if !opened_browser {
                    if let Some(url) = extract_url(&clean) {
                        let _ = open_browser_detached(&url);
                    }
                }
            }
            break;
        }

        if Instant::now() > deadline {
            log(&app, start, "10-minute deadline reached");
            break;
        }
    }

    child.kill();
    let _ = std::fs::remove_file(&tempfile);

    found_token.ok_or_else(|| {
        anyhow!(
            "no token captured — if the browser opened but the page said \"can't connect\", claude setup-token crashed before its callback listener started. Use the manual option: open a terminal, run `claude setup-token`, paste the token."
        )
    })
}

#[cfg(not(windows))]
fn run_blocking(_app: AppHandle, _bin: PathBuf) -> Result<String> {
    Err(anyhow!("claude auto setup is Windows-only"))
}
