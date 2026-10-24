#!/usr/bin/env bash
# Launch ONE K3 worker on ONE task in its OWN worktree (SWARM-PLAYBOOK
# prompt pattern 1: worktree-per-agent, port-per-agent).
#
# Usage: swarm/worker.sh <worker-id 1..3> <task-file>
#   worker-id   → worktree ../rustmc-w<id>, branch w<id>/<task-name>,
#                 port base 25600+<id>*10
#   task-file   → markdown task prompt (swarm/tasks/*.md), built by the
#                 orchestrator from the frozen docs: stable prefix first
#                 (cache hits), task-specific diff last (CI.md token rules)
#
# The orchestrator (Fable, Claude Code session) runs these via background
# Bash and drives merges itself — workers implement and commit; they never
# push main, never wait on background jobs (Minecom lesson).
set -euo pipefail
WID="$1"; TASK_FILE="$2"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
TREE="$REPO/../rustmc-w$WID"
TASK_NAME="$(basename "$TASK_FILE" .md)"
BRANCH="w$WID/$TASK_NAME"
LOG_DIR="$REPO/swarm/logs"; mkdir -p "$LOG_DIR"

[ -d "$TREE" ] || git -C "$REPO" worktree add "$TREE" -b "$BRANCH"
git -C "$TREE" checkout -B "$BRANCH" >/dev/null

export MINECOM_TEST_PORT=$((25600 + WID * 10))   # worker's private port range

# Headless K3 (kimi CLI print mode). --quiet = final message only; the full
# transcript stays in the log, never in the orchestrator's context.
cd "$TREE"
kimi --print -p "$(cat "$TASK_FILE")" \
    >"$LOG_DIR/w$WID-$TASK_NAME.log" 2>&1
echo "worker $WID done: $TASK_NAME (log: swarm/logs/w$WID-$TASK_NAME.log)"
