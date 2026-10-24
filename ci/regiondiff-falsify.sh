#!/usr/bin/env bash
# regiondiff-falsify.sh — the red-when-broken evidence for ci/t2-regiondiff.sh.
#
#     ./ci/regiondiff-falsify.sh                 # arms 1-3, no vanilla boot
#     REGIONDIFF_FALSIFY_LIVE=1 ./ci/regiondiff-falsify.sh   # + arm 4 (2 boots)
#
# The gate this proves replaced one whose green could not be turned red by any
# defect in the thing it was named for (`docs/DETERMINISM.md` §36, §37). Saying
# the replacement is better is worth nothing; this runs it against real injected
# faults and requires each to come back RED, by name.
#
# # The four arms and what each one is aimed at
#
#   1. THE PARSER MIS-READS A CHUNK. `read_container` in
#      `crates/rustmc-world/src/anvil.rs` resolves a palette index per block.
#      The injection shifts every index down by one — silent, in range, no
#      error, exactly the shape of a real bit-unpacking bug. REGIONDIFF-KAT's
#      census must refuse it. **The census is the only arm that can**: chunk
#      counts and denominators are geometry and survive a parser that decodes
#      geometry and gets every block wrong.
#
#   2. THE DIFFER STOPS COUNTING. `block_miss += 1` in
#      `tools/region-differ/src/main.rs` becomes `+= 0`. This is the arm the
#      old gate could never have had: `a` and `b` agree exactly and the clamped
#      live pair agrees exactly, so "0 mismatches" is the *right* answer
#      everywhere else in the gate and an always-zero differ gives it. Only the
#      twinned pair `vanilla-r7` / `vanilla-r7-b`, pinned at 823 blocks of
#      2,654,208, refuses. **An earlier version of this arm used a new
#      reference `c/` that existed only in the tree that made it, and it failed
#      at a real merge gate for exactly that reason — see arm 0.**
#
#   3. THE DIFFER DROPS A CHUNK. `load_dir` skips one full chunk. Both the KAT
#      denominator and REGIONDIFF-CONTROL's arithmetic must refuse it.
#
#   4. THE LIVE HALF WITHOUT ITS KNOB. Two fresh harvests with
#      `-Dmax.bg.threads=1` removed. §36 measured 24 of 28 pairs of this window
#      non-zero without the clamp, so this arm is expected red — and *this is
#      the evidence that REGIONDIFF-LIVE is a live check rather than a
#      restatement of a constant*. It costs two vanilla boots, so it is opt-in;
#      arms 1-3 need none.
#
# Nothing here touches `oracle-work/t2-regiondiff/{a,b}` or `oracle-work/
# vanilla-r7{,-b}` — they are read-only references and the gate never writes
# them either. The source patches are
# applied to this checkout and restored by an EXIT trap, including on ^C.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
FAIL=0

ANVIL="crates/rustmc-world/src/anvil.rs"
DIFFER_SRC="tools/region-differ/src/main.rs"
TMP="$(mktemp -d)"
cp "$ANVIL" "$TMP/anvil.orig"
cp "$DIFFER_SRC" "$TMP/differ.orig"

restore() {
    cp "$TMP/anvil.orig" "$ANVIL"
    cp "$TMP/differ.orig" "$DIFFER_SRC"
    rm -rf "$TMP"
}
trap restore EXIT

# A live pair, made once and reused by every arm, so no arm below boots vanilla.
# `a` and `b` here are two copies of one reference: the arms are about the gate
# refusing injected faults, not about harvest reproducibility, which is arm 4's
# job. The gate is told about this directory explicitly, and its own header says
# that flag is for this script and not for skipping a real harvest.
PAIR="$TMP/pair"
mkdir -p "$PAIR"
cp -r oracle-work/t2-regiondiff/a "$PAIR/a"
cp -r oracle-work/t2-regiondiff/b "$PAIR/b"

# `sub <file> <literal-old> <literal-new>` — a fixed-string replacement that
# REFUSES when the text is not there. A patch that has silently stopped
# matching is a hole in the sweep, not a passing arm, and this project has been
# bitten by exactly that in `ci/corpus-lock-falsify.sh`.
sub() {
    local file="$1" old="$2" new="$3"
    grep -qF -- "$old" "$file" || {
        echo "regiondiff-falsify: PATCH NO LONGER MATCHES in $file:" >&2
        echo "    $old" >&2
        echo "  The injection could not be applied, so this arm proves nothing." >&2
        echo "  Re-target it against the current source rather than deleting it." >&2
        exit 2
    }
    python3 - "$file" "$old" "$new" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
p.write_text(s.replace(sys.argv[2], sys.argv[3], 1))
PY
}

# `arm <n> <name> <expect-substring>` — run the gate, require a non-zero exit
# AND require the named assertion to be the one that failed. Requiring only a
# non-zero exit would let a compile error count as a caught fault, which is the
# same mistake as a gate that cannot fail.
arm() {
    local n="$1"
    local name="$2"
    local expect="$3"
    local log="$TMP/arm$n.log"
    local rc
    T2_REGIONDIFF_LIVE_DIR="$PAIR" ./ci/t2-regiondiff.sh > "$log" 2>&1
    rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "FAIL arm $n ($name): the gate went GREEN with the fault injected"
        FAIL=1
    elif grep -q "FAIL $expect" "$log"; then
        echo "ok   arm $n ($name): RED on '$expect'"
    else
        echo "FAIL arm $n ($name): red (rc=$rc) but not on '$expect' — it may have"
        echo "     failed for an unrelated reason, which proves nothing:"
        sed -n '/FAIL/p' "$log" | sed 's/^/       /' | head -5
        FAIL=1
    fi
    restore_sources
}

restore_sources() {
    cp "$TMP/anvil.orig" "$ANVIL"
    cp "$TMP/differ.orig" "$DIFFER_SRC"
}

echo "== regiondiff-falsify: baseline + 3 injected faults (+1 live arm, opt-in) =="

# --- arm 0: the baseline, and this arm exists because its absence cost a gate -
#
# **Every arm below is a difference measurement, and a difference measurement
# needs a baseline.** Without one, "the gate came back red with the fault
# injected" is indistinguishable from "the gate comes back red here anyway",
# and the second is not evidence of anything.
#
# That is not hypothetical. On 2026-08-16 20:43 this script reported arm 2 as
# `red (rc=2) but not on 'KAT differ mismatches'` at a real merge gate. The
# injection was fine and compiled fine; the gate was exiting 2 in that tree
# before it ever reached the assertion, because the reference the arm depends on
# was not in the shared directory. The named-assertion rule caught it — that is
# why the rule is there — but nothing here could say *why*, and an arm that can
# only report "not the expected failure" sends the next reader to the wrong
# file. This arm answers it in one line, before any source is touched.
BASE_LOG="$TMP/arm0.log"
if T2_REGIONDIFF_LIVE_DIR="$PAIR" ./ci/t2-regiondiff.sh > "$BASE_LOG" 2>&1; then
    echo "ok   arm 0 (baseline): the gate is GREEN before any fault is injected"
else
    echo "FAIL arm 0 (baseline): the gate is ALREADY RED (rc=$?) with nothing injected."
    echo "     Every arm below would be uninterpretable, so they are not run: a red"
    echo "     that was red anyway is not a caught fault. This is an environment or"
    echo "     a gate defect, not a falsification failure. What it said:"
    sed -n '/FAIL\|REFUSED/p' "$BASE_LOG" | sed 's/^/       /' | head -8
    echo "REGIONDIFF-FALSIFY RED"
    exit 1
fi

# --- arm 1: the parser mis-reads a chunk -----------------------------------
sub "$ANVIL" \
    'let idx = ((long >> ((i % per_long) as u32 * bits)) & mask) as usize;' \
    'let idx = (((long >> ((i % per_long) as u32 * bits)) & mask) as usize).saturating_sub(1);'
arm 1 "parser mis-reads a chunk" "KAT census minecraft:stone"

# --- arm 2: the differ stops counting --------------------------------------
sub "$DIFFER_SRC" 'block_miss += 1;' 'block_miss += 0;'
arm 2 "differ stops counting mismatches" "KAT differ mismatches"

# --- arm 3: the differ drops a chunk ---------------------------------------
sub "$DIFFER_SRC" \
    '            if chunk.status == "minecraft:full" {' \
    '            if chunk.status == "minecraft:full" && !(chunk.x == 64 && chunk.z == 64) {'
arm 3 "differ drops a chunk" "KAT census chunks"

# --- arm 4: the live half with its knob removed ----------------------------
#
# Not an injected fault: the real harvest path, run the way it ran before §37.
# Expected RED, and if it ever comes back green that is a finding about vanilla,
# not a passing test — say so rather than deleting the arm.
if [ "${REGIONDIFF_FALSIFY_LIVE:-0}" = "1" ]; then
    echo "-- arm 4: two fresh harvests WITHOUT -Dmax.bg.threads=1 (2 vanilla boots)"
    UNCLAMPED="$TMP/unclamped"
    mkdir -p "$UNCLAMPED"
    VANILLA_JVM_OPTS="" ./ci/gen-vanilla-region.sh 20260722 2 "$UNCLAMPED/a" 25951 64 64 > "$TMP/arm4a.log" 2>&1
    VANILLA_JVM_OPTS="" ./ci/gen-vanilla-region.sh 20260722 2 "$UNCLAMPED/b" 25952 64 64 > "$TMP/arm4b.log" 2>&1
    OUT=$(./target/debug/region-differ diff "$UNCLAMPED/a" "$UNCLAMPED/b" --bbox 63 63 65 65 2>&1)
    MIS=$(printf '%s' "$OUT" | grep -oP 'mismatches=\K[0-9]+' | head -1 || true)
    if [ "${MIS:-0}" -gt 0 ]; then
        echo "ok   arm 4 (live half, clamp removed): $MIS mismatches of 884,736 — RED"
    else
        echo "note arm 4 (live half, clamp removed): this pair agreed anyway."
        echo "     Not a failure of the gate: DETERMINISM.md §36 measured 4 of 28"
        echo "     unclamped pairs at zero, so ~1 run in 7 lands here. Re-run it."
    fi
else
    echo "-- arm 4 skipped (REGIONDIFF_FALSIFY_LIVE=1 to run it; 2 vanilla boots)"
fi

if [ "$FAIL" -ne 0 ]; then echo "REGIONDIFF-FALSIFY RED"; exit 1; fi
echo "REGIONDIFF-FALSIFY GREEN — every injected fault was refused, by name"
