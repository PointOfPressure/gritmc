#!/usr/bin/env bash
# Install this repo's Claude Code hooks to the path settings.json points at.
#
#     ./ci/install-hooks.sh          # copy repo -> live
#     ./ci/install-hooks.sh --check  # exit 1 if the live copy has drifted
#
# WHY A COPY AND NOT A SYMLINK
#
# The live path is read by a PreToolUse hook on every Agent call, including
# calls made while this repo is mid-rebase or checked out at another branch. A
# symlink into a worktree would make the gate's behaviour a function of
# whatever tree happened to be checked out, which is the opposite of what a
# gate is for. So it is a copy, and drift between the two is a checked fault
# rather than an impossible one.
set -uo pipefail
cd "$(dirname "$0")/.."
LIVE=${RUSTMC_HOOK_DIR:-/home/sayer/grit/hooks}

fail=0
for f in hooks/*.sh; do
    b=$(basename "$f")
    if [ "${1:-}" = --check ]; then
        if [ ! -f "$LIVE/$b" ]; then
            echo "HOOKS: $b is NOT INSTALLED at $LIVE — the gate in this repo is not the gate that runs"
            fail=1
        elif ! cmp -s "$f" "$LIVE/$b"; then
            echo "HOOKS: $LIVE/$b has DRIFTED from hooks/$b"
            diff -u "$f" "$LIVE/$b" | head -40
            fail=1
        else
            echo "HOOKS: $b installed and identical"
        fi
    else
        mkdir -p "$LIVE" && cp "$f" "$LIVE/$b" && chmod +x "$LIVE/$b" && echo "HOOKS: installed $b -> $LIVE/$b"
    fi
done
exit $fail
