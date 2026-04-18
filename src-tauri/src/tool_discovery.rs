//! Finds installed CLI tools (gh, flyctl, claude) even when the Tauri app
//! was launched from a shortcut with a stripped PATH. Checks common Windows
//! install locations in addition to PATH.

use std::path::PathBuf;

fn env_path() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

fn candidates(exe_names: &[&str], extra_dirs: &[&str]) -> Option<PathBuf> {
    // First: try PATH directly (fast path for properly configured systems)
    for name in exe_names {
        if let Ok(p) = which::which(name) {
            return Some(p);
        }
    }
    // Collect search dirs: PATH + well-known install locations
    let mut search: Vec<PathBuf> = env_path();
    for d in extra_dirs {
        let expanded = expand(d);
        if !expanded.as_os_str().is_empty() {
            search.push(expanded);
        }
    }
    for dir in search {
        if !dir.exists() {
            continue;
        }
        for name in exe_names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn expand(p: &str) -> PathBuf {
    let mut s = p.to_string();
    for (k, v) in std::env::vars() {
        s = s.replace(&format!("%{k}%"), &v);
    }
    PathBuf::from(s)
}

pub fn find_gh() -> Option<PathBuf> {
    candidates(
        &["gh.exe", "gh.cmd", "gh"],
        &[
            r"C:\Program Files\GitHub CLI",
            r"C:\Program Files (x86)\GitHub CLI",
            r"%LOCALAPPDATA%\GitHubCLI",
            r"%LOCALAPPDATA%\Microsoft\WinGet\Links",
            r"%USERPROFILE%\scoop\shims",
            r"%ChocolateyInstall%\bin",
        ],
    )
}

pub fn find_flyctl() -> Option<PathBuf> {
    candidates(
        &["flyctl.exe", "flyctl.cmd", "flyctl", "fly.exe", "fly"],
        &[
            r"%USERPROFILE%\.fly\bin",
            r"%LOCALAPPDATA%\Fly",
            r"%LOCALAPPDATA%\Microsoft\WinGet\Links",
            r"%USERPROFILE%\scoop\shims",
            r"C:\Program Files\Fly",
        ],
    )
}

pub fn find_claude() -> Option<PathBuf> {
    candidates(
        &["claude.exe", "claude.cmd", "claude"],
        &[
            r"%LOCALAPPDATA%\Microsoft\WinGet\Links",
            r"%LOCALAPPDATA%\Microsoft\WinGet\Packages\Anthropic.ClaudeCode_Microsoft.Winget.Source_8wekyb3d8bbwe",
            r"%USERPROFILE%\.local\bin",
            r"%USERPROFILE%\AppData\Roaming\npm",
            r"%USERPROFILE%\scoop\shims",
            r"%ChocolateyInstall%\bin",
        ],
    )
}
