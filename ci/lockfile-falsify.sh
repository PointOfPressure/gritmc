#!/usr/bin/env bash
# lockfile-falsify.sh — a tracked file that a routine command rewrites.
#
#     ./ci/lockfile-falsify.sh
#
# # THE HAZARD, AND WHY IT WAS DIAGNOSED THREE TIMES AS THREE PROBLEMS
#
# `Cargo.lock` is tracked. It records `grit-plugin-api`, which is a **path
# dependency into the `grit-plugins` repo checked out beside this one** — see
# the note in the workspace `Cargo.toml` for why it is a path and not a copy.
# That sibling's version moved 0.2.1 -> 0.3.0 in its own commit `da26a62`.
#
# So every `cargo` command that RESOLVES the dependency graph rewrote a tracked
# file and left the tree dirty. It does not take a build. Measured 2026-08-18
# against a worktree whose lockfile was stale:
#
#     cargo tree --depth 1                          rewrote Cargo.lock
#     cargo fetch                                   rewrote Cargo.lock
#     cargo metadata --format-version 1             rewrote Cargo.lock
#     cargo metadata --no-deps --format-version 1   did NOT
#     cargo metadata --format-version 1 --locked    rc 101, tree clean
#
# The `--no-deps` row is worth keeping in view. `ci/check-test-targets.py` is
# the one T0 stage that runs cargo without building, and it passes `--no-deps`,
# so it was NOT a culprit — an earlier reading of this hazard said it was, and
# the distinction between "runs cargo" and "resolves the graph" is exactly what
# that reading missed.
#
# git refuses to rebase a dirty tree, so this is how `ci/lanes.sh` dropped 37
# queued branches in 33 seconds, and why abandoned worktrees keep turning up
# dirty with nothing in them but a one-line `Cargo.lock` change. Seven of the
# 248 worktrees on this box were dirty for exactly this reason when it was
# measured, all of them lanes that had run cargo since the sibling bumped.
#
# **Remote gate directories are immune and that is what hid it.** They are
# rsync copies with no `.git` (`ci/t0-remote.sh --exclude '.git'`), so nothing
# there can be "dirty". The damage lands only in `~/grit/work-*` and
# `~/grit/gw-*`, which is where the lanes live and where nobody runs the gate.
#
# It is the environment class seen from the other side. A box copy does not
# merely LACK things — a tool also WRITES things, and a tool that writes a
# tracked file as a side effect of being asked a question is a state
# manufacturer, not a reader.
#
# # THE REPAIR THIS FILE GATES
#
# `--locked` does not ask cargo to be careful; it removes cargo's permission to
# write. A lockfile that genuinely needs updating then produces a loud refusal
# and one commit fixes it for every worktree at once, instead of silent dirt in
# each of them separately.
#
#   1  the hazard reproduces at all                  -> without --locked, dirty
#   2  --locked refuses instead of rewriting         -> rc != 0, tree clean
#   3  --locked with an up-to-date lockfile          -> rc 0, tree clean
#   4  no gate-path cargo call may omit --locked     -> the ratchet
#
# The skip goes through `box_skip` and not `echo`. Printing the marker is not
# the same as recording it: `ci/t0.sh` audits a LEDGER FILE, thirteen of its
# stages filter their own stdout, and this file's skip was printed in a gate log
# and counted by nothing — `box-skip-audit: 1 skip(s)` under two BOX-SKIP lines.
# The channel, again, in the file written to close the channel defect.
#
# Case 1 is the positive control. If the hazard stops reproducing, cases 2 and
# 3 are green over nothing and this file must be re-derived rather than
# trusted.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PASS=0; FAIL=0
ok()  { printf '  ok    %-40s %s\n' "$1" "${2:-}"; PASS=$((PASS+1)); }
no()  { printf '  FAIL  %-40s %s\n' "$1" "${2:-}"; FAIL=$((FAIL+1)); }

command -v cargo >/dev/null 2>&1 || {
    echo "lockfile-falsify REFUSED — cargo is not on PATH; cases 1-3 measure cargo's behaviour and cannot be inferred" >&2
    echo "    export PATH=\$HOME/.cargo/bin:\$PATH" >&2
    exit 2
}

WORK="$(mktemp -d -t lockfile-falsify.XXXXXX)" || exit 2
trap 'rm -rf "$WORK"' EXIT

# --- a miniature of this workspace's exact shape ----------------------------
#
# A tracked lockfile, and a path dependency that lives OUTSIDE the repository
# so nothing in the repository's history can pin it. Registry dependencies are
# deliberately absent: they cannot drift this way, and including one would make
# the fixture need the network.
mkdir -p "$WORK/sibling/src" "$WORK/repo/src"
sib_version() {
    cat > "$WORK/sibling/Cargo.toml" <<TOML
[package]
name = "fixture-sibling"
version = "$1"
edition = "2021"
TOML
}
sib_version 0.2.1
: > "$WORK/sibling/src/lib.rs"

cat > "$WORK/repo/Cargo.toml" <<'TOML'
[package]
name = "fixture-app"
version = "0.1.0"
edition = "2021"

[dependencies]
fixture-sibling = { path = "../sibling" }
TOML
: > "$WORK/repo/src/lib.rs"

git init -q "$WORK/repo"
git -C "$WORK/repo" config user.email f@f
git -C "$WORK/repo" config user.name fixture

# Generate the lockfile against the sibling at 0.2.1 and COMMIT it, which is
# the state every worktree in this project is in.
( cd "$WORK/repo" && cargo metadata --format-version 1 --offline >/dev/null 2>&1 )
[ -f "$WORK/repo/Cargo.lock" ] || { echo "lockfile-falsify REFUSED — the fixture produced no Cargo.lock" >&2; exit 2; }
git -C "$WORK/repo" add -A
git -C "$WORK/repo" commit -q -m "the lockfile, tracked, against the sibling at 0.2.1"

dirty() { [ -n "$(git -C "$WORK/repo" status --porcelain -- Cargo.lock)" ]; }
restore() { git -C "$WORK/repo" checkout -q -- Cargo.lock 2>/dev/null; }

# The sibling moves, in its own repo, with no commit here.
sib_version 0.3.0

# 1. THE POSITIVE CONTROL. Without --locked the question rewrites the answer.
( cd "$WORK/repo" && cargo metadata --format-version 1 --offline >/dev/null 2>&1 ); RC=$?
if [ "$RC" = 0 ] && dirty; then
    ok "1 no --locked -> tracked file rewritten" "rc=$RC, and git now sees a dirty tree"
else
    no "1 no --locked -> tracked file rewritten" "rc=$RC dirty=$(dirty && echo yes || echo no) — the hazard did not reproduce, so cases 2-3 prove nothing"
fi
restore

# 2. THE REPAIR. --locked removes the permission rather than asking for care.
( cd "$WORK/repo" && cargo metadata --format-version 1 --offline --locked >/dev/null 2>&1 ); RC=$?
if [ "$RC" != 0 ] && ! dirty; then
    ok "2 --locked -> refuses, leaves the tree clean" "rc=$RC"
else
    no "2 --locked -> refuses, leaves the tree clean" "rc=$RC dirty=$(dirty && echo yes || echo no)"
fi
restore

# 3. AND IT IS NOT MERELY ALWAYS-RED. With the lockfile current, --locked is
#    silent — otherwise the repair would be a gate that cannot go green, which
#    is the same defect as a gate that cannot go red.
( cd "$WORK/repo" && cargo metadata --format-version 1 --offline >/dev/null 2>&1 )
git -C "$WORK/repo" add -A && git -C "$WORK/repo" commit -q -m "carry the sibling's 0.3.0"
( cd "$WORK/repo" && cargo metadata --format-version 1 --offline --locked >/dev/null 2>&1 ); RC=$?
if [ "$RC" = 0 ] && ! dirty; then
    ok "3 --locked, lockfile current -> rc 0, clean"
else
    no "3 --locked, lockfile current -> rc 0, clean" "rc=$RC dirty=$(dirty && echo yes || echo no)"
fi

# 4. THE RATCHET. A gate stage that runs cargo in a worktree and omits --locked
#    reintroduces the hazard, and would do it silently. The sweep is over the
#    stages that run on EVERY gate — the ones an agent's inner loop reaches —
#    rather than over every script in ci/, because a one-off measurement script
#    an operator runs by hand is not what dropped 37 branches.
# 4. THE PROPERTY ITSELF, MEASURED IN THIS TREE RATHER THAN GREPPED FOR.
#
# The first draft of this case grepped `ci/check-test-targets.py` for
# `["cargo", "metadata"]` without `--locked`. That file carries a dozen
# `["cargo", "test", ...]` literals in its own selftest fixtures and in its
# argv comparisons, so the grep reported six defects in the test data and none
# in the code — a check firing on its own inputs, which is the class this tree
# exists to avoid. The grep was the wrong instrument, not a wrong pattern.
#
# So the real property is measured: run the gate stage that enumerates targets,
# and ask git whether the tree it just audited is still clean. That is the
# control the hazard actually calls for — *a cargo run in a clean worktree
# leaves it clean* — and it cannot be fooled by a string in a fixture.
if ! git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
    # A box copy has no `.git`, so nothing in it can be dirty and this case
    # would pass over nothing. Loud, per ci/checks/box-skips.toml.
    . "$ROOT/ci/lib/box-env.sh"
    box_skip "lockfile-falsify:tree-stays-clean" \
        "$ROOT is not a git repository — dirtiness is not observable here"
elif [ -n "$(git -C "$ROOT" status --porcelain -- Cargo.lock)" ]; then
    . "$ROOT/ci/lib/box-env.sh"
    box_skip "lockfile-falsify:tree-stays-clean" \
        "Cargo.lock is already dirty in $ROOT — refusing to touch a tree with" \
        "uncommitted work in the file under test"
else
    # 4a. THE POSITIVE CONTROL FOR 4b. If this tree cannot show dirt at all,
    #     4b is green over nothing. Without `--locked` the lockfile is
    #     rewritten whenever it disagrees with the sibling checkout; when it
    #     agrees, nothing moves and the control is honestly inconclusive,
    #     which is reported rather than counted as a pass.
    ( cd "$ROOT" && cargo metadata --no-deps --format-version 1 >/dev/null 2>&1 )
    if [ -n "$(git -C "$ROOT" status --porcelain -- Cargo.lock)" ]; then
        ok "4a control: an unlocked cargo call CAN dirty this tree"
        git -C "$ROOT" checkout -q -- Cargo.lock
    else
        printf '  --    %-40s %s\n' "4a control: unlocked cargo dirties this tree" \
            "inconclusive — the lockfile already agrees with ../plugins, so there is nothing to rewrite"
    fi

    # 4b. THE GATE STAGE THAT RUNS CARGO WITHOUT BUILDING. It passes
    #     `--no-deps`, so it should leave the tree clean whether or not the
    #     lockfile is stale — and this is what says so, rather than the reading
    #     of the flag. An edit that drops `--no-deps` makes this red.
    ( cd "$ROOT" && python3 ci/check-test-targets.py >/dev/null 2>&1 )
    if [ -z "$(git -C "$ROOT" status --porcelain -- Cargo.lock)" ]; then
        ok "4b ci/check-test-targets.py leaves the tree clean"
    else
        no "4b ci/check-test-targets.py leaves the tree clean" \
           "a T0 stage rewrote a tracked file; git will not rebase this tree now"
        git -C "$ROOT" checkout -q -- Cargo.lock
    fi
fi

# 5. THE STATIC RATCHET, where grep IS the right instrument. `ci/t0.sh` is
#    shell, its cargo lines are at command position, and it carries no fixture
#    strings — so a new stage added without `--locked` is caught by reading
#    rather than by running a build.
BAD=""
while IFS= read -r line; do
    # `cargo fmt` is rustfmt behind a cargo shim and rejects `--locked`
    # outright (`error: unexpected argument '--locked' found`, measured). It
    # resolves nothing and cannot rewrite the lockfile. Excluded on the
    # measurement, not on the assumption.
    case "$line" in *"cargo fmt"*) continue;; esac
    [ -n "$line" ] && BAD="$BAD
        ci/t0.sh:$line"
done < <(grep -nE '^[[:space:]]*(\([[:space:]]*cd [^)]*&&[[:space:]]*)?cargo[[:space:]]+(build|check|test|clippy|metadata|run)\b' ci/t0.sh 2>/dev/null | grep -v -- '--locked')
if [ -z "$BAD" ]; then
    ok "5 every cargo stage in ci/t0.sh passes --locked"
else
    no "5 every cargo stage in ci/t0.sh passes --locked" "these may rewrite a tracked file:$BAD"
fi

printf '\nlockfile-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ] || exit 1
echo "LOCKFILE-FALSIFY GREEN"
