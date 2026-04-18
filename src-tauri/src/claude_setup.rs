//! End-to-end automation of `claude setup-token`:
//!   1. Spawn claude in a hidden real Windows console (Ink satisfied)
//!   2. Poll its redirected stdout for the OAuth URL → auto-open browser
//!      + emit "claude-url" event for the UI
//!   3. The UI captures the code from the user (who copied it from Anthropic's
//!      code-display page) and calls `claude_submit_code`
//!   4. We AttachConsole(child_pid) + WriteConsoleInputW to inject the code
//!      + Enter into claude's console input buffer — claude reads it as stdin
//!   5. Poll output for the token, capture, emit "claude-token"

use anyhow::{anyhow, Result};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

// ───────────────────────── ANSI + parsing ─────────────────────────

pub fn strip_ansi(s: &str) -> String {
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

pub fn extract_token(line: &str) -> Option<String> {
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

pub fn extract_url(text: &str) -> Option<String> {
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

// ───────────────────────── spawn ─────────────────────────

#[cfg(windows)]
pub struct HiddenChild {
    pub process: windows::Win32::Foundation::HANDLE,
    pub thread: windows::Win32::Foundation::HANDLE,
    pub pid: u32,
}

#[cfg(windows)]
pub fn spawn_hidden_console(command_line: &str) -> Result<HiddenChild> {
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
impl HiddenChild {
    pub fn try_wait(&self) -> Option<u32> {
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
    pub fn kill(&self) {
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

// ───────────────────────── stdin injection ─────────────────────────

/// Inject `text` + Enter into the target process's console input buffer.
/// `pid` must be a child spawned with CREATE_NEW_CONSOLE.
#[cfg(windows)]
pub fn inject_to_console(pid: u32, text: &str) -> Result<()> {
    use windows::Win32::Foundation::{BOOL, HANDLE};
    use windows::Win32::System::Console::{
        AttachConsole, FreeConsole, GetStdHandle, WriteConsoleInputW, INPUT_RECORD,
        INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, STD_INPUT_HANDLE,
    };

    unsafe {
        // Detach any console we may be attached to (harmless no-op for a GUI app).
        let _ = FreeConsole();
        AttachConsole(pid).map_err(|e| anyhow!("AttachConsole({pid}): {e}"))?;

        let in_h: HANDLE = GetStdHandle(STD_INPUT_HANDLE)
            .map_err(|e| anyhow!("GetStdHandle(STD_INPUT): {e}"))?;

        let mut records: Vec<INPUT_RECORD> = Vec::with_capacity(text.chars().count() * 2 + 2);
        let push = |recs: &mut Vec<INPUT_RECORD>, ch: u16, down: BOOL| {
            let ker = KEY_EVENT_RECORD {
                bKeyDown: down,
                wRepeatCount: 1,
                wVirtualKeyCode: 0,
                wVirtualScanCode: 0,
                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: ch },
                dwControlKeyState: 0,
            };
            recs.push(INPUT_RECORD {
                EventType: KEY_EVENT as u16,
                Event: INPUT_RECORD_0 { KeyEvent: ker },
            });
        };
        for c in text.chars() {
            let u = c as u32;
            if u > 0xffff {
                let _ = FreeConsole();
                return Err(anyhow!("non-BMP character not supported"));
            }
            push(&mut records, u as u16, true.into());
            push(&mut records, u as u16, false.into());
        }
        // Enter key = \r
        push(&mut records, 0x0d, true.into());
        push(&mut records, 0x0d, false.into());

        let mut written = 0u32;
        WriteConsoleInputW(in_h, &records, &mut written)
            .map_err(|e| anyhow!("WriteConsoleInputW: {e}"))?;

        let _ = FreeConsole();
        if (written as usize) < records.len() {
            return Err(anyhow!(
                "partial write: {} of {} events",
                written,
                records.len()
            ));
        }
    }
    Ok(())
}

// ───────────────────────── session state ─────────────────────────

#[derive(Default)]
pub struct ClaudeSessionState {
    pub inner: Mutex<Option<RunningSession>>,
}

pub struct RunningSession {
    pub pid: u32,
    pub temp_file: PathBuf,
}

fn temp_output_file() -> PathBuf {
    std::env::temp_dir().join(format!(
        "toddler-claude-setup-{}.log",
        uuid::Uuid::new_v4().as_simple()
    ))
}

// ───────────────────────── main flow ─────────────────────────

pub async fn run(app: AppHandle) -> Result<String> {
    let bin = crate::tool_discovery::find_claude().ok_or_else(|| {
        anyhow!("Claude Code not found. Install via `winget install Anthropic.ClaudeCode`.")
    })?;
    let app_clone = app.clone();
    let bin_clone = bin.clone();
    tokio::task::spawn_blocking(move || run_blocking(app_clone, bin_clone))
        .await
        .map_err(|e| anyhow!("claude-setup task join: {}", e))?
}

#[cfg(windows)]
fn run_blocking(app: AppHandle, bin: PathBuf) -> Result<String> {
    let start = Instant::now();
    log(&app, start, format!("spawning: {}", bin.display()));

    let tempfile = temp_output_file();
    std::fs::write(&tempfile, b"").ok();

    let cmdline = format!(
        "cmd.exe /c \"\"{}\" setup-token > \"{}\" 2>&1\"",
        bin.display(),
        tempfile.display()
    );
    let child = spawn_hidden_console(&cmdline)?;
    let pid = child.pid;
    log(&app, start, format!("spawned PID {}", pid));

    // Publish session state so submit_code can find the pid
    {
        use tauri::Manager;
        let st = app.state::<ClaudeSessionState>();
        *st.inner.lock().unwrap() = Some(RunningSession {
            pid,
            temp_file: tempfile.clone(),
        });
    }

    let mut opened_browser = false;
    let mut url_emitted = false;
    let mut found_token: Option<String> = None;
    let mut seen_bytes: u64 = 0;
    let deadline = Instant::now() + Duration::from_secs(15 * 60);

    loop {
        std::thread::sleep(Duration::from_millis(300));

        if let Ok(bytes) = std::fs::read(&tempfile) {
            if bytes.len() as u64 > seen_bytes {
                seen_bytes = bytes.len() as u64;
                let text = String::from_utf8_lossy(&bytes).to_string();
                let clean = strip_ansi(&text);

                if !url_emitted {
                    if let Some(url) = extract_url(&clean) {
                        url_emitted = true;
                        log(&app, start, format!("URL: {}", url));
                        let _ = app.emit("claude-url", url.clone());
                        if !opened_browser {
                            opened_browser = true;
                            match open_browser_detached(&url) {
                                Ok(_) => log(&app, start, "browser spawn ok"),
                                Err(e) => log(&app, start, format!("browser spawn error: {}", e)),
                            }
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
            log(&app, start, "claude process exited; final drain");
            std::thread::sleep(Duration::from_millis(300));
            if let Ok(bytes) = std::fs::read(&tempfile) {
                let text = String::from_utf8_lossy(&bytes).to_string();
                let clean = strip_ansi(&text);
                if found_token.is_none() {
                    if let Some(t) = extract_token(&clean) {
                        found_token = Some(t);
                    }
                }
            }
            break;
        }

        if Instant::now() > deadline {
            log(&app, start, "15-minute deadline reached");
            break;
        }
    }

    child.kill();
    let _ = std::fs::remove_file(&tempfile);
    {
        use tauri::Manager;
        let st = app.state::<ClaudeSessionState>();
        *st.inner.lock().unwrap() = None;
    }

    found_token.ok_or_else(|| {
        anyhow!("claude auth did not complete. Use the manual paste option.")
    })
}

#[cfg(not(windows))]
fn run_blocking(_app: AppHandle, _bin: PathBuf) -> Result<String> {
    Err(anyhow!("claude auto setup is Windows-only"))
}

pub fn submit_code(app: &AppHandle, code: &str) -> Result<()> {
    use tauri::Manager;
    let pid = {
        let st = app.state::<ClaudeSessionState>();
        let guard = st.inner.lock().unwrap();
        guard.as_ref().map(|s| s.pid)
    };
    let pid = pid.ok_or_else(|| anyhow!("no claude setup session is running"))?;
    inject_to_console(pid, code)
}
