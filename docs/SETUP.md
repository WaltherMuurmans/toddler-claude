# Setup guide (owner — you)

You do these once. Your toddler(s) never do them.

## 1. Fly.io account

- Sign up at https://fly.io → add a credit card (required even for the free tier).
- Set a **monthly spend limit** under Billing → Spend limit. $10/mo is a safe ceiling for a few toddlers.
- Create a **personal access token** at https://fly.io/user/personal_access_tokens. Give it any name; expiration 90 days.
- Note your **org slug** (usually `personal`). Visible on the dashboard URL: `https://fly.io/dashboard/<slug>`.

## 2. Remote Docker image

The image lives on ghcr.io and is built/published automatically by `.github/workflows/publish-image.yml` whenever `remote/**` changes on `main`.

First push to `main` will trigger it. Confirm the image is public:

- https://github.com/users/WaltherMuurmans/packages/container/package/toddler-claude-remote
- Package settings → Change visibility → Public.

Your toddlers' Fly machines pull this image; public is simplest. If you prefer private, you'll also need to configure GHCR pull credentials on Fly.

## 3. Windows installer

- Push to `main` → `build-windows.yml` produces `toddler-claude-windows-msi` and `toddler-claude-windows-nsis` artifacts.
- To cut a versioned release: `gh release create v0.1.0 --generate-notes` → workflow uploads the MSI/EXE directly to the release.

### Code signing (optional but recommended)

Unsigned installers trigger Windows SmartScreen warnings. Options:

- **Cheap path:** accept SmartScreen; toddler clicks "More info → Run anyway" once.
- **Proper path:** buy a code-signing cert (~$200/yr from SSL.com / SignPath / Certum). Add it as repo secret `WINDOWS_CERT_PFX_BASE64` + `WINDOWS_CERT_PASSWORD`, extend the workflow to sign the MSI after build.

## 4. Per-toddler setup (what they do)

1. Install the MSI.
2. Open the app.
3. **Claude token:**
   - In any terminal (or via the WSL/Git Bash prompt on their machine): `claude setup-token`.
   - Browser opens; they approve; CLI prints a long `sk-ant-oat...` token.
   - Paste it into the app.
4. **GitHub:** click "Connect GitHub" → browser opens → code shown → paste → approve.
5. **Fly.io:** you give them a Fly token from step 1 (either a shared one under your org, or create a sub-org for them). Paste into app.
6. **Pick repo** from dropdown.
7. Done. Click the big green button.

## 5. Per-repo setup

For each repo a toddler will work on:

```bash
cp -r template/.claude        /path/to/target-repo/
cp -r template/.github        /path/to/target-repo/
# Replace @OWNER in CODEOWNERS with your GitHub username
sed -i 's/@OWNER/@WaltherMuurmans/g' /path/to/target-repo/.github/CODEOWNERS
cd /path/to/target-repo
git add .claude .github && git commit -m "Add toddler-claude guardrails"
git push
```

Then on GitHub:

- Settings → Branches → add rule for `main`:
  - Require PR before merging
  - Require approvals (1) from Code Owners
  - Require status checks: `gitleaks`, `sensitive-paths`
  - Require linear history
- Settings → Code security → enable Secret scanning + Push protection.

## 6. Claude subscription

Toddler Claude uses the user's Claude Pro/Max subscription via OAuth. **No API key**. No API billing. If they don't have a subscription, they can't use the app.

One concurrent session per Claude account (subscription constraint). The app enforces a single-session lock locally.
