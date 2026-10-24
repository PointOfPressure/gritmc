#!/usr/bin/env bash
# corpus-integrity-arms-falsify.sh — the two readers that produce this branch's
# headline, driven from synthetic floors and from a real ensemble that already
# exists on the box.
#
#     ./ci/corpus-integrity-arms-falsify.sh
#
# # WHY THESE TWO AND NOT THE RIG
#
# `ci/corpus-integrity-108-falsify.sh` covers the rig that takes the
# recordings. Nothing covered the arithmetic that turns them into a verdict,
# and that is where the result actually is: the concurrent-only scenario set
# and the matched-n placement. Both are cheap to get wrong in ways that produce
# a confident, wrong, publishable number.
#
# The cases that matter:
#
#   5  two arms with EQUAL floor counts over DIFFERENT scenarios must not read
#      as agreement. That is the whole reason the discriminator works by name;
#   4  a floor document with no `floor_names` must be refused, not silently
#      treated as an empty set. The first version of both readers looked for a
#      `scenarios` list that `ci/corpus-floor.py` does not write, found nothing,
#      and printed an empty discriminator under a floor of 5;
#   8  a subsample of n < N must actually measure the SUBSET. A subsampler that
#      re-measures the whole ensemble every time reports a perfectly stable
#      distribution and hides the very effect it exists to expose.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARMS_DRIVER="${ARMS_DRIVER:-$ROOT/ci/corpus-integrity-arms.sh}"
SUB_DRIVER="${SUB_DRIVER:-$ROOT/ci/corpus-floor-subsample.py}"
W="$(mktemp -d /tmp/arms-falsify.XXXXXX)"
trap 'rm -rf "$W"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# A real ensemble with real projections, so the subsample cases measure rather
# than mock. Six full-corpus vanilla reference recordings, one recorder, taken
# serially on 2026-08-13; see docs/CORPUS-LOCK-HOLD.md §19.1.
REAL_ENSEMBLE="${ARMS_FALSIFY_ENSEMBLE:-/home/sayer/grit/work-excl2-floor-items/ensemble.json}"

mkfloor() {  # mkfloor <dir> <floor> <name>...
    local d="$1" f="$2"; shift 2
    mkdir -p "$d"
    python3 - "$d/floor.json" "$f" "$@" <<'PY'
import json, sys
out, floor, names = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
json.dump({"floor": floor, "scenarios_common": 108, "floor_names": names}, open(out, "w"))
PY
}
disc() { "$ARMS_DRIVER" --discriminate "$1" 2>&1; }

say() { printf '%s\n' "$*"; }
say "== corpus-integrity-arms-falsify"

# 1-3, 5: the discriminator, by name.
D="$W/d1"; mkfloor "$D/floor-serial" 2 alpha beta; mkfloor "$D/floor-conc3" 3 alpha beta gamma
O="$(disc "$D")"
echo "$O" | grep -cE 'concurrent-only .*: 1' >/dev/null && echo "$O" | grep -c '^      gamma$' >/dev/null \
    && ok "1 a scenario failing only in the concurrent arm is named" \
    || bad "1 concurrent-only scenario not reported: $(echo "$O" | grep -A2 concurrent-only | tr '\n' ' ')"

D="$W/d2"; mkfloor "$D/floor-serial" 2 alpha beta; mkfloor "$D/floor-conc3" 2 alpha beta
O="$(disc "$D")"
echo "$O" | grep -cE 'concurrent-only .*: 0' >/dev/null \
    && ok "2 identical sets give an empty concurrent-only list" \
    || bad "2 identical sets did not give an empty concurrent-only list"

D="$W/d3"; mkfloor "$D/floor-serial" 3 alpha beta delta; mkfloor "$D/floor-conc3" 2 alpha beta
O="$(disc "$D")"
echo "$O" | grep -cE 'concurrent-only .*: 0' >/dev/null && echo "$O" | grep -cE 'serial-only .*: 1' >/dev/null \
    && ok "3 a serial-only scenario is reported as serial-only, not as interference" \
    || bad "3 a serial-only scenario was misfiled"

D="$W/d5"; mkfloor "$D/floor-serial" 2 alpha beta; mkfloor "$D/floor-conc3" 2 gamma delta
O="$(disc "$D")"
if echo "$O" | grep -cE 'concurrent-only .*: 2' >/dev/null; then
    ok "5 equal floor COUNTS over different scenarios do not read as agreement"
else
    bad "5 two arms with floor 2 over disjoint scenario sets read as agreement"
fi

# 4: a floor document whose shape has moved must refuse.
D="$W/d4"; mkdir -p "$D/floor-serial" "$D/floor-conc3"
printf '{"floor": 2, "scenarios_common": 108}\n' > "$D/floor-serial/floor.json"
printf '{"floor": 2, "scenarios_common": 108}\n' > "$D/floor-conc3/floor.json"
O="$(disc "$D")"; RC=$?
if echo "$O" | grep -c 'floor.json has no floor_names' >/dev/null; then
    ok "4 a floor document with no floor_names is refused, not read as empty"
else
    bad "4 a floor document with no floor_names was accepted: $(echo "$O" | tail -2 | tr '\n' ' ')"
fi

# 6-10: the subsampler, against a real ensemble.
if [ ! -s "$REAL_ENSEMBLE" ]; then
    bad "6 no real ensemble at $REAL_ENSEMBLE — the subsample cases cannot run"
else
    N="$(python3 -c 'import json,sys;print(len(json.load(open(sys.argv[1]))["runs"]))' "$REAL_ENSEMBLE")"
    "$SUB_DRIVER" "$REAL_ENSEMBLE" --n $((N + 1)) --draws 3 >/dev/null 2>&1 \
        && bad "6 asked for subsets larger than the ensemble and it agreed" \
        || ok "6 refuses subsets larger than the ensemble"
    "$SUB_DRIVER" "$REAL_ENSEMBLE" --n 1 --draws 3 >/dev/null 2>&1 \
        && bad "6 accepted subsets of one recording, which measure nothing" \
        || ok "6 refuses subsets of fewer than two recordings"

    FULL="$W/full.json"
    "$SUB_DRIVER" "$REAL_ENSEMBLE" --n "$N" --draws 3 --out "$W/subfull" > "$W/full.log" 2>&1
    FULLMED="$(sed -n 's/.*median \([0-9]*\) .*/\1/p' "$W/full.log" | head -1)"
    REALFLOOR="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["floor"])' \
        "$(dirname "$REAL_ENSEMBLE")/floor.json")"
    if [ "$FULLMED" = "$REALFLOOR" ]; then
        ok "7 a subsample at n=N reproduces the ensemble's own floor ($REALFLOOR)"
    else
        bad "7 a subsample at n=N gave $FULLMED, the ensemble's floor is $REALFLOOR"
    fi

    "$SUB_DRIVER" "$REAL_ENSEMBLE" --n 3 --draws 40 --out "$W/sub3" \
        --reference "$(dirname "$REAL_ENSEMBLE")/floor.json" > "$W/sub3.log" 2>&1
    MED3="$(sed -n 's/.*median \([0-9]*\) .*/\1/p' "$W/sub3.log" | head -1)"
    MAX3="$(sed -n 's/.*max \([0-9]*\)$/\1/p' "$W/sub3.log" | head -1)"
    if [ -n "$MED3" ] && [ "$MED3" -lt "$REALFLOOR" ]; then
        ok "8 halving n lowered the floor ($REALFLOOR at n=$N, median $MED3 at n=3) — the confound §19 exists for"
    else
        bad "8 the floor at n=3 (median ${MED3:-none}) is not below the floor at n=$N ($REALFLOOR); the subsampler is not measuring subsets"
    fi
    if grep -qE 'reference floor [0-9]+ sits at percentile' "$W/sub3.log"; then
        PCT="$(sed -n 's/.*sits at percentile \([0-9]*\).*/\1/p' "$W/sub3.log")"
        if [ "$PCT" -ge 50 ]; then
            ok "9 the n=$N reference sits high in the n=3 distribution (percentile $PCT), as it must"
        else
            bad "9 the n=$N reference sits at percentile $PCT of the n=3 distribution, which is the wrong side"
        fi
    else
        bad "9 no percentile was reported for the reference"
    fi
    grep -q 'scenarios seen not reproducing' "$W/sub3.log" \
        && [ "$(grep -cE '^ *[0-9]+% ' "$W/sub3.log")" -ge 3 ] \
        && ok "10 the per-scenario detection frequencies are reported" \
        || bad "10 no per-scenario frequency table"
fi

MISSED=0
if [ -z "${ARMS_NO_SWEEP:-}" ]; then
    echo
    echo "== injections (each MUST turn this suite red)"
    inj() {   # inj <name> <file> <perl> -> patched copy path
        local n="$1" f="$2" e="$3"
        local out="$W/inj.$n.$(basename "$f")"
        cp "$f" "$out"; perl -0pi -e "$e" "$out"; chmod +x "$out"
        if cmp -s "$f" "$out"; then echo STALE; return; fi
        if diff <(grep -vE '^\s*#' "$f") <(grep -vE '^\s*#' "$out") >/dev/null; then
            echo COMMENT-ONLY; return; fi
        echo "$out"
    }
    chk() {   # chk <name> <var=path> <patched>
        local n="$1" var="$2" d="$3"
        [ -n "$d" ] || { printf '  MISSED  %-30s <-- inject produced nothing\n' "$n"; MISSED=$((MISSED+1)); return; }
        case "$d" in
            STALE|COMMENT-ONLY) printf '  MISSED  %-30s <-- %s\n' "$n" "$d"; MISSED=$((MISSED+1)); return ;;
        esac
        if env "$var=$d" ARMS_NO_SWEEP=1 "$0" >/dev/null 2>&1; then
            printf '  MISSED  %-30s <-- the suite stayed green with this broken\n' "$n"; MISSED=$((MISSED+1))
        else
            printf '  CAUGHT  %-30s\n' "$n"
        fi
    }
    # The discriminator compares counts instead of names.
    chk "discriminator-by-count" ARMS_DRIVER \
        "$(inj bycount "$ARMS_DRIVER" 's/only_c, only_s = sorted\(c0 - s0\), sorted\(s0 - c0\)/only_c, only_s = (sorted(c0 - s0) if len(c0) != len(s0) else []), sorted(s0 - c0)/')"
    # A floor document with no floor_names silently becomes an empty set.
    chk "missing-floor-names-silent" ARMS_DRIVER \
        "$(inj nonames "$ARMS_DRIVER" 's/        raise SystemExit\("floor.json has no floor_names/        doc["floor_names"] = []  # was: raise SystemExit("floor.json has no floor_names/')"
    # The subsampler measures the whole ensemble every time.
    chk "subsample-ignores-the-subset" SUB_DRIVER \
        "$(inj wholeens "$SUB_DRIVER" 's/        m\["runs"\] = \[runs\[k\] for k in sub\]/        m["runs"] = runs/')"
    # The subsampler reads a floor document shape that does not exist.
    chk "subsample-empty-names" SUB_DRIVER \
        "$(inj subnames "$SUB_DRIVER" 's/    if "floor_names" in doc:\n        return set\(doc\["floor_names"\]\)/    if False:\n        pass/')"
fi

echo
echo "=================================================================="
echo "cases: $PASS passed, $FAIL failed"
[ -z "${ARMS_NO_SWEEP:-}" ] && echo "injections: $MISSED not caught"
if [ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]; then echo "CORPUS-INTEGRITY-ARMS-FALSIFY GREEN"
else echo "CORPUS-INTEGRITY-ARMS-FALSIFY RED"; fi
echo "=================================================================="
exit $(( FAIL > 0 || MISSED > 0 ))
