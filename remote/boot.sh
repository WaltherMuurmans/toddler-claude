#!/usr/bin/env bash
set -euo pipefail

log() { echo "[boot] $*" >&2; }

: "${REPO:?REPO env var required}"
: "${BRANCH:=main}"
: "${GH_TOKEN:?GH_TOKEN env var required}"
: "${CLAUDE_CODE_OAUTH_TOKEN:?CLAUDE_CODE_OAUTH_TOKEN env var required}"
: "${SESSION_PASS:?SESSION_PASS env var required}"
: "${HARD_LIMIT_SECONDS:=7200}"

log "Applying egress firewall..."
/opt/firewall.sh || log "firewall apply failed (continuing)"

log "Bringing up local Postgres (tmpfs)..."
/opt/pg-init.sh &
PG_PID=$!

log "Cloning ${REPO} (branch ${BRANCH})..."
mkdir -p /home/dev/work
chown -R dev:dev /home/dev/work
cd /home/dev/work

sudo -u dev git config --global user.name "Toddler via Claude"
sudo -u dev git config --global user.email "toddler@local"
sudo -u dev git config --global credential.helper store
echo "https://x-access-token:${GH_TOKEN}@github.com" > /home/dev/.git-credentials
chown dev:dev /home/dev/.git-credentials
chmod 600 /home/dev/.git-credentials

if ! sudo -u dev -E git clone --depth 50 --branch "$BRANCH" \
      "https://x-access-token:${GH_TOKEN}@github.com/${REPO}.git" repo 2>/tmp/clone.err; then
  log "clone failed:"
  cat /tmp/clone.err >&2
  exit 1
fi

cd repo

# Create a fresh feature branch so toddler can never push to default branch
FEATURE_BRANCH="claude/${SESSION_ID:-$(date +%Y%m%d-%H%M%S)}"
sudo -u dev git checkout -b "$FEATURE_BRANCH"
log "Working on feature branch: $FEATURE_BRANCH"

# Optional project bootstrap
if [ -f pubspec.yaml ]; then
  log "flutter pub get..."
  sudo -u dev -E bash -lc 'cd /home/dev/work/repo && flutter pub get || true'
fi
if [ -f package.json ]; then
  log "npm install (root)..."
  sudo -u dev -E bash -lc 'cd /home/dev/work/repo && npm install --no-audit --no-fund || true'
fi
if [ -f backend/package.json ]; then
  log "npm install (backend)..."
  sudo -u dev -E bash -lc 'cd /home/dev/work/repo/backend && npm install --no-audit --no-fund || true'
fi

# Hard-limit watchdog: destroys the session after N seconds
(
  sleep "$HARD_LIMIT_SECONDS"
  log "HARD LIMIT reached. Terminating session."
  pkill -f ttyd || true
) &

# Idle-limit watchdog: if ttyd has no client for 15 min, exit
(
  IDLE_LIMIT=900
  last_active=$(date +%s)
  while true; do
    sleep 60
    # If no ttyd process, exit loop
    if ! pgrep -x ttyd >/dev/null; then break; fi
    # ttyd exposes /token endpoint; check active connections via netstat
    active=$(ss -tn 'sport = :7681' state established 2>/dev/null | tail -n +2 | wc -l)
    if [ "$active" -gt 0 ]; then
      last_active=$(date +%s)
    else
      now=$(date +%s)
      if [ $((now - last_active)) -ge "$IDLE_LIMIT" ]; then
        log "IDLE LIMIT reached. Terminating session."
        pkill -f ttyd || true
        break
      fi
    fi
  done
) &

log "Starting ttyd on :7681..."
exec sudo -u dev -E env \
  CLAUDE_CODE_OAUTH_TOKEN="$CLAUDE_CODE_OAUTH_TOKEN" \
  PGHOST=127.0.0.1 PGUSER=dev PGDATABASE=app_dev PGPASSWORD=dev \
  HOME=/home/dev USER=dev \
  ttyd \
    --writable \
    --port 7681 \
    --interface 0.0.0.0 \
    --credential "toddler:${SESSION_PASS}" \
    --max-clients 1 \
    --check-origin=false \
    /opt/claude-wrapper.sh
