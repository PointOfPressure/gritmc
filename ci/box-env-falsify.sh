#!/usr/bin/env bash
# box-env-falsify.sh — the gate on the environment repairs.
#
#     ./ci/box-env-falsify.sh
#
# `ci/t0-remote.sh` ships the tree with `--exclude '.git'`, so the tree almost
# every gate runs in is not a git repository. Three checks were written against
# one and could not succeed where they run. `ci/lib/box_env.py` is the repair;
# this is the thing that can say the repair is real.
#
# # WHY THIS FILE IS NOT "RUN THE CHECKS AND SEE IF THEY PASS"
#
# A check that has been taught to stand down when there is no repository passes
# in a box copy trivially, and so does a check that has been taught to stand
# down *always*. The two are indistinguishable from the exit code, and the
# second is strictly worse than the red it replaced, because a red gets looked
# at. So every case here pairs a green with the red that green has to be able
# to become:
#
#   1  a store present, a citation that holds        -> GREEN
#   2  a store present, the witness removed          -> RED       (positive control for 1)
#   3  no store at all                               -> loud skip, exit 0
#   4  no store at all                               -> and NOT the pruned-branch red
#   5  a store present but broken                    -> RED, never a skip
#   6  a skip that nothing declared                  -> the audit reds
#   7  a declared skip                               -> the audit passes
#   8  provenance: a store present, sha resolvable   -> GREEN
#   9  provenance: a store present, sha absent       -> RED       (positive control for 8)
#  10  provenance: no store                          -> loud skip, not 56-of-56 NO WITNESS
#
# Cases 2, 5 and 9 are the ones that make the file worth having. They force the
# **real** red inside a box-shaped copy, which is the only evidence that the
# check still has teeth there rather than having been quietened.
#
# It builds its own repository with `git init` rather than borrowing the
# checkout's, for the same reason `ci/check-figures-falsify.py` does: this file
# has to be runnable in the environment it is about, and that environment has
# no repository to borrow.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PASS=0; FAIL=0
ok()  { printf '  ok    %-38s %s\n' "$1" "${2:-}"; PASS=$((PASS+1)); }
no()  { printf '  FAIL  %-38s %s\n' "$1" "${2:-}"; FAIL=$((FAIL+1)); }

WORK="$(mktemp -d -t box-env-falsify.XXXXXX)" || exit 2
trap 'rm -rf "$WORK"' EXIT

# # THIS FILE MAKES CHECKS SKIP ON PURPOSE, AND THE LEDGER MUST NOT BELIEVE IT.
#
# Cases 3, 4 and 10 run the real checks in a fixture tree with no object store,
# because proving they stand down there is the point. `ci/t0.sh` exports
# `RUSTMC_BOX_SKIP_LOG` and every child appends to it, so those deliberate
# skips landed in the gate's real ledger and `ci/box-skip-audit.py` reported
#
#     FORBIDDEN SKIP  check-cited-audits — this environment can run it and it
#     stood down anyway (no git object store in /tmp/box-env-falsify.xEoHh9/tree)
#
# turning a green T0 red over a fixture path. The audit was right and its input
# was wrong: a falsifier's injected behaviour is not evidence about the tree.
# So this run keeps its own ledger, and the gate's is untouched by it.
export RUSTMC_BOX_SKIP_LOG="$WORK/skips.log"

# --- the fixture repository ------------------------------------------------
#
# One commit, one file, one witness string. `--allow-empty` is not enough here:
# `check-cited-audits.py` asks for `<sha>:<path>`, so the commit has to carry a
# path.
FIX="$WORK/fixture-repo"
mkdir -p "$FIX"
git init -q "$FIX" 2>/dev/null || { echo "box-env-falsify: git init failed" >&2; exit 2; }
git -C "$FIX" config user.email f@f
git -C "$FIX" config user.name fixture
mkdir -p "$FIX/docs"
printf 'the audit found 47 unreached call sites\n' > "$FIX/docs/audit.md"
git -C "$FIX" add -A
git -C "$FIX" commit -q -m fixture
SHA="$(git -C "$FIX" rev-parse HEAD)"
[ -n "$SHA" ] || { echo "box-env-falsify: no fixture sha" >&2; exit 2; }

# --- the box-shaped tree ---------------------------------------------------
#
# The same exclusions `ci/t0-remote.sh` uses, so the thing under test is the
# environment the gate really has and not an approximation of it. `.git` is the
# one that matters; the others are here so the copy is honestly box-shaped.
TREE="$WORK/tree"
mkdir -p "$TREE/ci/lib" "$TREE/ci/checks"
cp ci/check-cited-audits.py "$TREE/ci/" 2>/dev/null
cp ci/figure-provenance.py  "$TREE/ci/" 2>/dev/null
cp ci/figures.toml          "$TREE/ci/" 2>/dev/null
cp ci/lib/box_env.py        "$TREE/ci/lib/" 2>/dev/null
[ -f "$TREE/ci/lib/box_env.py" ] || { echo "box-env-falsify: ci/lib/box_env.py is missing — the repair is not in this tree" >&2; exit 2; }

cited() {
    # $1 destination, $2 witness text
    cat > "$1" <<TOML
[[cited]]
id = "fixture-audit"
branch = "fixture-branch"
sha = "$SHA"
path = "docs/audit.md"
value = "47"
witness = "$2"
what = "the fixture audit's unreached count"
TOML
}
cited "$WORK/good.toml" "47 unreached call sites"
cited "$WORK/bad.toml"  "9999 unreached call sites"

run_cited() {  # $1 toml, rest: env assignments already exported by caller
    ( cd "$TREE" && python3 ci/check-cited-audits.py "$1" 2>&1 )
}

# 1. A store present and a citation that holds. This is the case the box copy
#    could never reach before, so it is the whole point of the repair.
OUT="$(RUSTMC_GIT_DIR="$FIX/.git" run_cited "$WORK/good.toml")"; RC=$?
if [ "$RC" = 0 ] && printf '%s' "$OUT" | grep -c '0 failing' >/dev/null; then
    ok "1 store present, citation holds -> GREEN"
else
    no "1 store present, citation holds -> GREEN" "rc=$RC"
    printf '        %s\n' "$OUT"
fi

# 2. THE POSITIVE CONTROL FOR 1. Same store, a witness the blob does not carry.
#    If this does not go red the green above means nothing.
OUT="$(RUSTMC_GIT_DIR="$FIX/.git" run_cited "$WORK/bad.toml")"; RC=$?
if [ "$RC" = 1 ] && printf '%s' "$OUT" | grep -c 'CITATION DRIFTED' >/dev/null; then
    ok "2 store present, witness gone -> RED" "the check still has teeth in a box copy"
else
    no "2 store present, witness gone -> RED" "rc=$RC — the check has been quietened, not repaired"
    printf '        %s\n' "$OUT"
fi

# 3/4. No store at all. The environment cannot answer, so the stage may stand
#    down — but it must SAY so, and it must not invent a parity failure.
OUT="$(env -u RUSTMC_GIT_DIR bash -c "cd '$TREE' && python3 ci/check-cited-audits.py '$WORK/good.toml' 2>&1")"; RC=$?
if [ "$RC" = 0 ] && printf '%s' "$OUT" | grep -c '^BOX-SKIP' >/dev/null; then
    ok "3 no store -> loud skip, exit 0"
else
    no "3 no store -> loud skip, exit 0" "rc=$RC"
    printf '        %s\n' "$OUT"
fi
if printf '%s' "$OUT" | grep -c 'CITATION UNREACHABLE' >/dev/null; then
    no "4 no store -> not the pruned-branch red" "it still blames a force-push that never happened"
    printf '        %s\n' "$(printf '%s' "$OUT" | head -1)"
else
    ok "4 no store -> not the pruned-branch red"
fi

# 5. A store that is present and broken. This is the case a bare `if not
#    exists: skip` gets wrong, and getting it wrong is how an environment
#    defect becomes a permanent silent pass.
BROKEN="$WORK/broken-store"
mkdir -p "$BROKEN"
printf 'not a repository\n' > "$BROKEN/HEAD"
OUT="$(RUSTMC_GIT_DIR="$BROKEN" run_cited "$WORK/good.toml")"; RC=$?
if [ "$RC" != 0 ] && ! printf '%s' "$OUT" | grep -c '^BOX-SKIP' >/dev/null; then
    ok "5 broken store -> RED, never a skip"
else
    no "5 broken store -> RED, never a skip" "rc=$RC"
    printf '        %s\n' "$OUT"
fi

# 6/7. The skip has to be counted, or "loud" only means "loud into a log nobody
#    diffs". An undeclared skip reds; a declared one does not.
if [ -f ci/box-skip-audit.py ]; then
    printf 'BOX-SKIP  invented-check  no store in this tree\n' > "$WORK/skips.log"
    OUT="$(python3 ci/box-skip-audit.py --log "$WORK/skips.log" --declared /dev/null 2>&1)"; RC=$?
    if [ "$RC" != 0 ]; then ok "6 an undeclared skip -> the audit reds"
    else no "6 an undeclared skip -> the audit reds" "rc=$RC $OUT"; fi

    printf '[[skip]]\ncheck = "invented-check"\nwhy = "fixture"\n' > "$WORK/declared.toml"
    OUT="$(python3 ci/box-skip-audit.py --log "$WORK/skips.log" --declared "$WORK/declared.toml" 2>&1)"; RC=$?
    if [ "$RC" = 0 ]; then ok "7 a declared skip -> the audit passes"
    else no "7 a declared skip -> the audit passes" "rc=$RC $OUT"; fi
else
    no "6 an undeclared skip -> the audit reds" "ci/box-skip-audit.py is missing"
    no "7 a declared skip -> the audit passes" "ci/box-skip-audit.py is missing"
fi

# 8/9/10. `ci/figure-provenance.py --check` is the second instance and needs its
#    own controls: it re-derives from full history, so a store that has the
#    history answers and one that does not must say so rather than reporting
#    every figure as unwitnessed.
if [ -f "$TREE/ci/figure-provenance.py" ]; then
    OUT="$(env -u RUSTMC_GIT_DIR bash -c "cd '$TREE' && python3 ci/figure-provenance.py --check 2>&1")"; RC=$?
    if printf '%s' "$OUT" | grep -c '^BOX-SKIP' >/dev/null; then
        ok "10 provenance, no store -> loud skip"
    else
        no "10 provenance, no store -> loud skip" "rc=$RC"
        printf '        %s\n' "$(printf '%s' "$OUT" | tail -1)"
    fi
    if printf '%s' "$OUT" | grep -cE 'whose derived commit is not in their provenance' >/dev/null; then
        no "10b provenance, no store -> not a blanket NO WITNESS" "$(printf '%s' "$OUT" | tail -1)"
    else
        ok "10b provenance, no store -> not a blanket NO WITNESS"
    fi
fi

# 8/9. THE POSITIVE CONTROL FOR THE PROVENANCE REPAIR. Case 10 shows the
#    blanket red is gone; on its own that is also what deleting the check would
#    show. These two build a repository whose history really does publish a
#    claim, and assert the derivation finds it and reds when the ledger names
#    the wrong commit.
PFIX="$WORK/prov-repo"
mkdir -p "$PFIX/docs"
git init -q "$PFIX"
git -C "$PFIX" config user.email f@f
git -C "$PFIX" config user.name fixture
printf '# fixture\n\nnothing measured yet.\n' > "$PFIX/docs/FIX.md"
git -C "$PFIX" add -A && git -C "$PFIX" commit -q -m "before the measurement"
SHA_BEFORE="$(git -C "$PFIX" rev-parse --short HEAD)"
printf '# fixture\n\nthe residual is 99.500000%% of the window.\n' > "$PFIX/docs/FIX.md"
git -C "$PFIX" add -A && git -C "$PFIX" commit -q -m "publish the claim"
SHA_AFTER="$(git -C "$PFIX" rev-parse --short HEAD)"

PTREE="$WORK/ptree"
mkdir -p "$PTREE/ci/lib" "$PTREE/docs"
cp ci/figure-provenance.py "$PTREE/ci/"
cp ci/lib/box_env.py "$PTREE/ci/lib/"
cp "$PFIX/docs/FIX.md" "$PTREE/docs/FIX.md"
prov_ledger() {  # $1 dest, $2 sha to record
    cat > "$1" <<TOML
[[figure]]
id = "fixture-prov"
kind = "worldgen"
doc = "docs/FIX.md"
claim = "99.500000%"
claim_count = 1
provenance = "recovered by ci/figure-provenance.py from the tree, $2"
TOML
}

prov_ledger "$PTREE/ci/figures.toml" "$SHA_AFTER"
OUT="$(RUSTMC_GIT_DIR="$PFIX/.git" bash -c "cd '$PTREE' && python3 ci/figure-provenance.py --check 2>&1")"; RC=$?
if [ "$RC" = 0 ] && ! printf '%s' "$OUT" | grep -c 'NO WITNESS' >/dev/null; then
    ok "8 store present, claim in history -> GREEN" "derived $SHA_AFTER"
else
    no "8 store present, claim in history -> GREEN" "rc=$RC"
    printf '        %s\n' "$OUT"
fi

prov_ledger "$PTREE/ci/figures.toml" "$SHA_BEFORE"
OUT="$(RUSTMC_GIT_DIR="$PFIX/.git" bash -c "cd '$PTREE' && python3 ci/figure-provenance.py --check 2>&1")"; RC=$?
if [ "$RC" = 1 ]; then
    ok "9 store present, ledger names the wrong commit -> RED" "the derivation still has teeth"
else
    no "9 store present, ledger names the wrong commit -> RED" "rc=$RC"
    printf '        %s\n' "$OUT"
fi

# 11. THE DEGRADED PATH'S DATE TEST. `ci/check-figures.py` rule 18 cannot
#     resolve a sha without a store, so it falls back to shape — and shape
#     alone cannot separate a sha from a date. Excluding every all-digit run
#     rejected `27914806`, a real commit that happens to carry no letter, and
#     reddened main on every branch. Excluding only runs that are a VALID
#     calendar date decides it. Both directions are pinned, because a test that
#     called nothing a date would pass the interesting case and lose the rule.
if OUT="$(python3 ci/lib/date-rule-check.py 2>&1)"; then
    ok "11 the degraded path tells a sha from a date" "$OUT"
else
    no "11 the degraded path tells a sha from a date" "$OUT"
fi

printf '\nbox-env-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ] || exit 1
echo "BOX-ENV-FALSIFY GREEN"
