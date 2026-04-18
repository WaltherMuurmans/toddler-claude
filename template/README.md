# Toddler-safe workspace scaffold

Copy these files into any repo you want a toddler to work on with Toddler Claude.

## What's here

- `.claude/settings.json` — Claude Code permission deny list (blocks `rm -rf`, `sudo`, `curl`, secret reads, force-push, etc.)
- `.github/CODEOWNERS` — forces your approval on sensitive paths
- `.github/workflows/secret-scan.yml` — gitleaks on every push
- `.github/workflows/pr-guard.yml` — flags sensitive-path edits on PRs
- `.github/workflows/ios-build.yml` — triggers Codemagic iOS build on PR

## One-time setup in the target repo

1. Copy these files into the repo root.
2. Replace `@OWNER` in `CODEOWNERS` with your GitHub username.
3. In GitHub → Settings → Branches, protect `main`:
   - Require pull request before merging
   - Require review from Code Owners
   - Require status checks: `gitleaks`, `sensitive-paths`
   - Require linear history
   - Do not allow bypassing the above
4. In GitHub → Settings → Code security:
   - Enable secret scanning + push protection
   - Enable Dependabot alerts
5. (Optional) Set repo secrets `CODEMAGIC_TOKEN`, `CODEMAGIC_APP_ID` for iOS builds.
