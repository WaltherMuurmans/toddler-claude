//! Headless CLI self-test mode. Invoked when the binary is launched with
//! `--test <name>`; prints JSON to stdout, exits 0 on success / nonzero on
//! failure, skipping the Tauri GUI entirely.
//!
//!   toddler-claude.exe --test diagnose          # prints discovered tool paths
//!   toddler-claude.exe --test github-cli        # github_cli_signin + list_repos
//!   toddler-claude.exe --test fly-cli           # flyctl_token + list_orgs
//!   toddler-claude.exe --test claude-auto       # runs claude setup-token (interactive)
//!   toddler-claude.exe --test fly-api           # uses stored token to hit Fly API
//!   toddler-claude.exe --test stored-tokens     # reports presence of each token

use crate::{credentials, credentials::keys, fly, fly_setup, github_oauth, tool_discovery};
use serde_json::json;

pub fn maybe_run_and_exit() {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    let mut test: Option<String> = None;
    while i < args.len() {
        if args[i] == "--test" && i + 1 < args.len() {
            test = Some(args[i + 1].clone());
            break;
        }
        i += 1;
    }
    let Some(name) = test else { return };

    // Release builds use windows_subsystem=windows (no console). Attach to
    // parent console so stdout/stderr from --test show up in the shell that
    // launched us.
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(pid: u32) -> i32;
            fn AllocConsole() -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            AllocConsole();
        }
    }

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = rt.block_on(run_test(&name));
    match result {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&json!({ "error": format!("{e}") }))
                    .unwrap_or_default()
            );
            std::process::exit(1);
        }
    }
}

async fn run_test(name: &str) -> anyhow::Result<serde_json::Value> {
    match name {
        "diagnose" => {
            let gh = tool_discovery::find_gh().map(|p| p.display().to_string());
            let flyctl = tool_discovery::find_flyctl().map(|p| p.display().to_string());
            let claude = tool_discovery::find_claude().map(|p| p.display().to_string());
            Ok(json!({
                "gh": gh,
                "flyctl": flyctl,
                "claude": claude,
                "path_len": std::env::var("PATH").map(|p| p.len()).unwrap_or(0),
            }))
        }
        "github-cli" => {
            let token = tokio::task::spawn_blocking(github_oauth::gh_cli_token).await??;
            // Verify via /user
            let client = reqwest::Client::new();
            let me: serde_json::Value = client
                .get("https://api.github.com/user")
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "ToddlerClaude/0.1")
                .header("Accept", "application/vnd.github+json")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let repos = github_oauth::list_repos(&token).await?;
            credentials::set(keys::GITHUB_TOKEN, &token)?;
            Ok(json!({
                "login": me["login"].as_str(),
                "repo_count": repos.len(),
                "first_5": repos.iter().take(5).map(|r| &r.full_name).collect::<Vec<_>>(),
            }))
        }
        "fly-cli" => {
            let token = tokio::task::spawn_blocking(fly_setup::flyctl_token).await??;
            let client = fly::FlyClient::new(&token);
            let email = client.verify().await?;
            let orgs = fly_setup::list_orgs(&token).await?;
            credentials::set(keys::FLY_TOKEN, &token)?;
            Ok(json!({ "email": email, "orgs": orgs }))
        }
        "fly-api" => {
            let token = credentials::get(keys::FLY_TOKEN)?
                .ok_or_else(|| anyhow::anyhow!("no stored fly token"))?;
            let client = fly::FlyClient::new(&token);
            let email = client.verify().await?;
            Ok(json!({ "email": email }))
        }
        "stored-tokens" => Ok(json!({
            "claude": credentials::get(keys::CLAUDE_TOKEN)?.is_some(),
            "github": credentials::get(keys::GITHUB_TOKEN)?.is_some(),
            "fly":    credentials::get(keys::FLY_TOKEN)?.is_some(),
        })),
        "claude-probe-piped" => {
            // Alternative spawn: Stdio::piped() for stdin, no new console.
            // Check whether Ink raw-mode error appears.
            use std::io::Write;
            use std::process::{Command, Stdio};
            let bin = tool_discovery::find_claude()
                .ok_or_else(|| anyhow::anyhow!("claude not found"))?;
            let mut cmd = Command::new(&bin);
            cmd.arg("setup-token");
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let mut child = cmd.spawn()?;
            // Close stdin immediately
            drop(child.stdin.take());
            std::thread::sleep(std::time::Duration::from_secs(8));
            let _ = child.kill();
            let out = child.wait_with_output()?;
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("=STDOUT=\n{stdout}\n=STDERR=\n{stderr}");
            let clean = crate::claude_setup::strip_ansi(&combined);
            let has_raw = clean.contains("Raw mode is not supported");
            let url = crate::claude_setup::extract_url(&clean);
            Ok(json!({
                "ink_raw_mode_error": has_raw,
                "url_found": url,
                "stdout_len": stdout.len(),
                "stderr_len": stderr.len(),
                "combined_head_600": clean.chars().take(600).collect::<String>(),
            }))
        }
        "claude-probe" => {
            // Spawn claude setup-token exactly like the app does, watch ~12s,
            // report: did it start, does the output contain the raw-mode error,
            // did a URL appear, did a token appear.
            use std::time::Instant;
            let bin = tool_discovery::find_claude()
                .ok_or_else(|| anyhow::anyhow!("claude not found"))?;
            let tempfile = std::env::temp_dir().join(format!(
                "tc-claude-probe-{}.log",
                uuid::Uuid::new_v4().as_simple()
            ));
            std::fs::write(&tempfile, b"")?;
            let cmdline = format!(
                "cmd.exe /c \"\"{}\" setup-token > \"{}\" 2>&1\"",
                bin.display(),
                tempfile.display()
            );
            let child = crate::claude_setup::spawn_hidden_console(&cmdline)?;
            std::thread::sleep(std::time::Duration::from_secs(12));
            child.kill();
            let bytes = std::fs::read(&tempfile).unwrap_or_default();
            let content = String::from_utf8_lossy(&bytes).into_owned();
            let clean = crate::claude_setup::strip_ansi(&content);
            let has_raw = clean.contains("Raw mode is not supported");
            let has_welcome = clean.contains("Welcome to Claude Code");
            let url = crate::claude_setup::extract_url(&clean);
            let token = crate::claude_setup::extract_token(&clean);
            let _ = std::fs::remove_file(&tempfile);
            Ok(json!({
                "size_bytes": bytes.len(),
                "ink_raw_mode_error": has_raw,
                "printed_welcome": has_welcome,
                "url_found": url,
                "token_found": token.map(|t| format!("{}…({} chars)", &t[..20.min(t.len())], t.len())),
                "head": clean.chars().take(400).collect::<String>(),
                "tail": clean.chars().rev().take(400).collect::<String>().chars().rev().collect::<String>(),
            }))
        }
        "keyring-roundtrip" => {
            let marker = format!("test-{}", uuid::Uuid::new_v4());
            let key = "test_roundtrip";
            let set_res = credentials::set(key, &marker);
            let got = credentials::get(key);
            let del = credentials::delete(key);
            Ok(json!({
                "set_ok": set_res.is_ok(),
                "set_err": set_res.err().map(|e| e.to_string()),
                "get_ok": got.as_ref().map(|v| v.is_some()).unwrap_or(false),
                "get_matches": got.as_ref().ok().and_then(|o| o.as_ref()).map(|v| v == &marker).unwrap_or(false),
                "get_value_len": got.as_ref().ok().and_then(|o| o.as_ref()).map(|v| v.len()).unwrap_or(0),
                "get_err": got.err().map(|e| e.to_string()),
                "delete_ok": del.is_ok(),
            }))
        }
        other => Err(anyhow::anyhow!(
            "unknown test `{}`. Valid: diagnose, github-cli, fly-cli, fly-api, stored-tokens",
            other
        )),
    }
}
