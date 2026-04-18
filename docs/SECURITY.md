# Security model

## Trust boundaries

```
Toddler's Windows PC  ─┐
                       │  Tauri app (WebView2)
                       │  ↕ IPC
                       │  Rust backend
                       │  ↕ Windows Credential Manager (encrypted at rest)
                       │  ↕ HTTPS to Fly API / GitHub API
                       │  ↕ WSS to remote VM
                       ▼
              Fly.io ephemeral VM
              (Ubuntu 24 + rootless app user + nftables egress allowlist)
                       ↓ git clone (fine-grained PAT)
              GitHub repo (branch protection + CODEOWNERS)
                       ↓ PR
                    Owner (you) reviews + merges
                       ↓ CI
              Codemagic iOS build (separate, real secrets here)
                       ↓
                Prod deployment (never touched by toddler)
```

## Layer-by-layer

### Desktop app
- Tauri v2 with tight CSP: `connect-src` limited to Fly API, GitHub API, Fly WSS.
- Tauri capabilities allow exactly one shell command (`claude setup-token`); no arbitrary shell.
- Secrets stored in Windows Credential Manager (`ToddlerClaude:*` entries).
- Ephemeral session state held in process memory; cleared on stop.

### Transport
- All outbound: TLS 1.3.
- Remote VM terminal: Fly.io terminates TLS on port 443; backend is plain TCP to `ttyd` on 7681. `ttyd` enforces HTTP Basic auth with a per-session 24-byte random password.
- Single-client mode on `ttyd`: prevents hijack by a second connection.

### Remote VM
- Ubuntu 24.04. All services run as non-root `dev` user.
- `sudo` restricted to `nft` binary (for firewall setup).
- nftables: default OUTPUT drop, allowlist of ~20 FQDNs (resolved to A records at boot).
- Postgres in `tmpfs` (/run/pgdata). No persistent disk at all except the container rootfs, which is ephemeral.
- `auto_destroy = true` on the Fly machine: when the main process exits, Fly deletes the VM.
- Two watchdogs:
  1. Hard limit 7200s → kills `ttyd` → main process exits → VM destroyed.
  2. Idle limit 900s (no active WSS connection) → same.

### Claude Code inside the VM
- `.claude/settings.json` (committed in target repo) denies:
  - Destructive: `rm -rf /`, `rm -rf ~`, `rm -rf .git`
  - Privilege: `sudo`, `su`
  - Exfil: `curl`, `wget`, `nc`, `ssh`, `scp`
  - VCS damage: `git push --force`, `git push origin main`, `git config`, `git remote`, `git reset --hard`
  - GitHub admin: `gh auth`, `gh api`, `gh secret`, `gh workflow`
  - Cloud CLIs: `docker`, `kubectl`, `gcloud`, `aws`
  - Secret reads: `.env*`, `*.pem`, `*.key`, `id_rsa*`, `~/.aws/**`, `~/.ssh/**`
  - Guardrail bypass: editing `.github/workflows/**`, `.claude/**`

### GitHub
- Fine-grained PAT scoped to one repo, `contents:write` + `pull-requests:write` + `metadata:read` only.
- Branch protection on `main`:
  - Require PR
  - Require CODEOWNERS approval
  - Require status checks (`gitleaks`, `sensitive-paths`)
  - Require linear history
  - Block force-push
- CODEOWNERS protects `/.github/`, `/.claude/`, `/.devcontainer/`, `/infra/`, `/backend/migrations/`, `/ios/`, `/android/`
- Secret scanning + Push protection (org-level): blocks commits with detected secrets.
- gitleaks action runs on every push as belt-and-suspenders.

### Claude subscription auth
- User generates long-lived OAuth token via `claude setup-token`.
- Token stored locally in Windows Credential Manager.
- Sent to Fly VM as env var at machine creation (Fly encrypts env at rest).
- Never written to disk on the VM (lives only in the process env of `ttyd` and its children).
- VM destroyed → env gone.
- Revocable at any time from claude.ai settings.

## Threat model — attacks NOT mitigated

- **Compromise of the user's Windows PC.** If the toddler's PC is malware-infected, Credential Manager secrets are readable. Mitigation: normal endpoint hygiene; outside our scope.
- **Compromise of Fly.io.** If Fly.io is breached, env vars leak. Mitigation: rotate Claude token; revoke GitHub PAT. Not our problem to solve.
- **Prompt injection in the repo.** If the target repo contains adversarial markdown/code that Claude reads, it could attempt denied actions. Those attempts fail (settings deny), but Claude might produce misleading PRs. Mitigation: you review every PR.
- **Supply-chain attack on the Fly image.** If `ubuntu:24.04` or the Flutter tarball is compromised, the VM is compromised. Mitigation: pin versions; periodic rebuild; image is ephemeral per session.

## Revocation playbook

- **Revoke Claude access for a toddler:** claude.ai → Settings → Connected apps → revoke.
- **Revoke GitHub access:** GitHub settings → Applications → revoke the OAuth app or the fine-grained PAT.
- **Revoke Fly access:** fly.io → Personal access tokens → revoke.
- **Kill an active session:** open the app, click "Stop and discard". Or from Fly dashboard: destroy the `toddler-*` app.
