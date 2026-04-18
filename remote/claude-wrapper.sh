#!/usr/bin/env bash
set -euo pipefail
cd /home/dev/work/repo

cat /opt/welcome.txt
echo ""
echo "Repo: ${REPO:-unknown}"
echo "Branch: $(git branch --show-current)"
echo ""

# Start claude. If it exits, drop to bash so the toddler can still do git push / gh pr create
exec claude
