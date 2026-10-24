#!/usr/bin/env bash
# aq-sync.sh — the inner-loop rsync for this branch, with the mtime touch that
# AGENT-BRIEF makes mandatory. Not a gate; a convenience so the touch is never
# the step that gets skipped.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
: "${RUSTMC_REMOTE_DIR:=rustmc-aquifer}"
rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
    --exclude 'data/vanilla' "$ROOT/" "threadripper:$RUSTMC_REMOTE_DIR/"
ssh -n threadripper "cd \$HOME/$RUSTMC_REMOTE_DIR && find crates tools ci -type f \
    \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' -o -name '*.java' \) -print0 | xargs -0 touch"
echo "synced $ROOT -> threadripper:$RUSTMC_REMOTE_DIR (mtimes touched)"
