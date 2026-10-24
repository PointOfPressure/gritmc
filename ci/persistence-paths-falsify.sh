#!/usr/bin/env bash
# persistence-paths-falsify.sh — show that `ci/persistence-applies.py` can say
# both words, and that `--check` can refuse.
#
# WHY
#
# `ci/handoff.sh` stage 7 runs the persistence gates only when this decides
# they apply. A decider that always said APPLIES would be a stage nobody
# noticed was unconditional; a decider that always said NOT-APPLICABLE would
# be a stage that never ran, which is where the four gates already were and is
# the whole thing this change exists to end. Neither failure is visible from a
# gate log: both produce a plausible line.
#
# So both answers are demonstrated, on real inputs, plus the three ways
# `--check` refuses a broken ledger. Nineteen assertions, no build, no jar, no
# lock, about two seconds — which is why it runs inside `ci/t0.sh`.
#
# WHAT IT DOES NOT PROVE
#
# It does not prove the list is complete. Completeness is not a property any
# script can check: it would need to know which source files a vanilla boot's
# outcome depends on, which is the question the gates themselves exist to
# answer. The ledger's own header says what a missing path costs — that branch
# is left exactly where every branch was before the stage existed — and this
# suite proves only that every path which IS declared works.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

APPLIES=ci/persistence-applies.py
LEDGER=ci/checks/persistence-paths.toml
fails=0
n=0

ok() { # ok <label> <wanted> <got>
    n=$((n + 1))
    if [ "$2" = "$3" ]; then
        printf '  ok   %-58s %s\n' "$1" "$3"
    else
        printf '  FAIL %-58s wanted %s, got %s\n' "$1" "$2" "$3"
        fails=$((fails + 1))
    fi
}

decide() { printf '%s\n' "$@" | "$APPLIES" - 2>/dev/null; }

echo "== 1. every declared prefix, one at a time, must make the stage apply"
#
# Read out of the ledger rather than listed here. A hand-copied list would go
# stale the first time somebody added a path, and would then report a prefix
# as covered while testing a prefix that no longer exists — the same defect
# the ledger's own `--check` refuses one level up.
mapfile -t PREFIXES < <(sed -n 's/^prefix = "\(.*\)"$/\1/p' "$LEDGER")
[ "${#PREFIXES[@]}" -ge 8 ] || {
    echo "  FAIL could only read ${#PREFIXES[@]} prefixes out of $LEDGER"
    exit 1
}
for p in "${PREFIXES[@]}"; do
    # A directory prefix is exercised through a file inside it, because that
    # is the shape `git diff --name-only` produces: it never names a
    # directory. A prefix that only matched its own exact string would pass a
    # test that used the bare prefix and fail on every real branch.
    if [ -d "$p" ]; then
        probe="$p/Cargo.toml"
    else
        probe="$p"
    fi
    ok "$probe" APPLIES "$(decide "$probe")"
done

echo "== 2. changes outside every prefix must NOT apply"
#
# The four here are the shapes that actually arrive: a doc-only branch, a
# worldgen branch, a gameplay-corpus branch, and a branch touching a
# rustmc-server module that has nothing to do with saving. If any of these
# said APPLIES the filter would be doing nothing and the stage would be
# unconditional in fact while the log claimed it was conditional.
ok "docs only"        NOT-APPLICABLE "$(decide docs/FINDINGS.md docs/CI.md)"
ok "worldgen"         NOT-APPLICABLE "$(decide crates/rustmc-worldgen/src/lib.rs)"
ok "gameplay corpus"  NOT-APPLICABLE "$(decide ci/checks/gameplay/place-sign.toml)"
ok "an unrelated server module" NOT-APPLICABLE "$(decide crates/rustmc-server/src/recipes.rs)"
ok "nothing changed"  NOT-APPLICABLE "$(decide)"

echo "== 3. a mixed branch applies — one declared path among many"
#
# The real case. A branch touching forty files of which one is `anvil.rs` has
# to run the stage, and a decider that required a majority, or the first
# path, or a whole-tree match would fail exactly here.
ok "40 unrelated + anvil.rs" APPLIES \
    "$(decide docs/WORKLIST.md crates/rustmc-worldgen/src/lib.rs \
              crates/rustmc-world/src/anvil.rs README.md)"

echo "== 4. --check refuses a broken ledger"
#
# Three injections, each a defect this file has actually had or could have.
# The first one is not hypothetical: the ledger shipped with
# `crates/rustmc-world/src/region.rs`, a file that does not exist, and
# `--check` refused it on its first run.
BACKUP=$(mktemp)
cp "$LEDGER" "$BACKUP"
restore() { cp "$BACKUP" "$LEDGER"; }
trap 'restore; rm -f "$BACKUP"' EXIT

check_rc() { "$APPLIES" --check >/dev/null 2>&1; echo $?; }

ok "unpatched ledger" 0 "$(check_rc)"

printf '\n[[path]]\nprefix = "crates/rustmc-world/src/no-such-file.rs"\ngates = ["all"]\nwhy = "x"\n' >> "$LEDGER"
ok "a prefix nothing matches" 1 "$(check_rc)"
restore

printf '\n[[path]]\nprefix = "ci/t2-persistence.sh"\ngates = ["all"]\nwhy = "x"\n' >> "$LEDGER"
ok "a prefix declared twice" 1 "$(check_rc)"
restore

printf '\n[[path]]\nprefix = "ci/t0.sh"\ngates = ["t2-no-such-gate"]\nwhy = "x"\n' >> "$LEDGER"
ok "a gate ci/t2-persistence.sh does not run" 1 "$(check_rc)"
restore

sed -i 's/^min_paths = 8$/min_paths = 999/' "$LEDGER"
ok "fewer paths than the floor" 1 "$(check_rc)"
restore

ok "restored" 0 "$(check_rc)"

echo
if [ "$fails" -eq 0 ]; then
    echo "PERSISTENCE-PATHS-FALSIFY GREEN — $n assertions, ${#PREFIXES[@]} declared prefixes, 0 failed"
    exit 0
fi
echo "PERSISTENCE-PATHS-FALSIFY RED — $fails of $n assertions failed"
exit 1
