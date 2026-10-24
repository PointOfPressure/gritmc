#!/usr/bin/env bash
# nw-corner-poison-control.sh — prove the seed-12345 r=16 window actually
# routes through the spruce trunk placer before anybody opens a brief on the
# 3,209-block tree-and-snow lump in its NW corner.
#
# WHY THIS EXISTS
#
# `ci/residual-census.py` names chunk (49,48) as the window's largest single
# lump: 1,008 charged blocks against a per-chunk innocent floor of 0, mostly
# `spruce_leaves`/`spruce_log`/`snow[layers=1]` moving a block or two either
# way. That is a number, and a number is not evidence that the code a brief
# would go and edit is the code that produced it.
#
# Three poison controls on 2026-08-09 came back byte-identical because nothing
# in the window they were run on routed the rule through the tested path. A
# control that cannot fire is not a control, and the honest outcome of one is
# to say so. This file exists so the NEXT agent on this lump does not spend a
# turn discovering that.
#
# WHAT IS POISONED, AND WHY IT IS THE BASE AND NOT A DRAW
#
# `TrunkPlacer#getTreeHeight` is `base + nextInt(a+1) + nextInt(b+1)`. The
# iceberg control poisons a *draw* because there the question was whether the
# RNG stream threads through the feature. Here the question is narrower and
# geometric: does a spruce trunk get placed **in chunk (49,48) of this
# window**? So the poison is `base + 1` — no draw moves, the stream is
# untouched, every tree in the world that is placed at all gets one block
# taller, and nothing else in the generator can be blamed for the difference.
#
# A draw poison would also fire, and would fire for a *weaker* reason: it
# would move every feature downstream of the trees in the same chunk too, so a
# red result would not distinguish "trees are here" from "something before the
# trees is here".
#
# WHAT A GREEN AND A RED MEAN
#
#   RED (the charge at (49,48) moves)  the lump really is routed through the
#                                      trunk placer and the brief is sound
#   GREEN (byte-identical)             the window does not place a spruce
#                                      trunk in that chunk, the lump is
#                                      something else wearing a tree's blocks,
#                                      and SAYING SO IS THE CORRECT OUTCOME
#
# The comparison is against vanilla-c at N=1 over the five chunks that hold
# the lump, not against the N=6 admissible charge: a routing control does not
# need an ensemble and an N=1 diff costs one reference read.
#
# The tree is restored from a byte copy taken before the edit, on the way out
# and including on failure, so a killed run does not leave a poisoned
# generator behind. Nothing under `oracle-work` is read except the one
# reference, and nothing there is written.
#
# usage: RUSTMC_REMOTE_DIR=<yours> ci/nw-corner-poison-control.sh
# exit 0 the control fired, 1 it did not, 2 the setup is wrong.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

HOST="${RUSTMC_REMOTE:-threadripper}"
DEST="${RUSTMC_REMOTE_DIR:?set RUSTMC_REMOTE_DIR to your own remote dir — the default is the shared one}"
SEED=12345
BBOX="48 48 50 52"
REF="\$HOME/rustmc/oracle-work/gen-wide-$SEED-r16-ow-c"
OUT="\$HOME/rp-poison-$DEST"
SRC=crates/rustmc-worldgen/src/feature/tree/trunk.rs
NEEDLE='        base + random.next_i32_bound(a + 1) + random.next_i32_bound(b + 1)'
POISON='        1 + base + random.next_i32_bound(a + 1) + random.next_i32_bound(b + 1)'

grep -qF "$NEEDLE" "$SRC" || {
    echo "the line this control poisons is no longer in $SRC — REFUSING" >&2
    echo "  wanted: $NEEDLE" >&2
    exit 2; }

BACKUP="$(mktemp)"
cp "$SRC" "$BACKUP"
# Restoring the LOCAL tree is not enough: the poison reaches the box by rsync,
# so a run that stops after the poisoned build leaves a poisoned generator in
# the remote directory, where the next measurement would pick it up and read
# as a code defect. The restore therefore ships the clean file back too, and
# touches it so cargo does not reuse the poisoned artefact.
restore() {
    cp "$BACKUP" "$SRC"
    rm -f "$BACKUP"
    rsync -az "$SRC" "$HOST:$DEST/$SRC" >/dev/null 2>&1 || true
    ssh -n "$HOST" "touch $DEST/$SRC" >/dev/null 2>&1 || true
}
trap restore EXIT INT TERM

run() {
    local label=$1
    rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
        --exclude 'data/vanilla' "$ROOT/" "$HOST:$DEST/" >/dev/null || return 2
    # rsync -a moves mtimes backwards and cargo trusts mtimes; without this
    # the poisoned run measures the clean binary and reports a control that
    # did not fire. 2026-08-08.
    ssh -n "$HOST" "cd $DEST && find crates tools ci -type f \
        \\( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \\) -print0 | xargs -0 touch" || return 2
    ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; cd $DEST && \
        cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -1 && \
        rm -rf $OUT/$label && \
        RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER=zx ./target/release/worldgen-dump \
            full $SEED 16 $OUT/$label 64 64 2>&1 | grep -E 'decorate-order|wrote' && \
        ./target/release/region-differ diff $REF $OUT/$label/region --bbox $BBOX 2>&1 | head -2"
}

echo "=== CLEAN ==="
CLEAN="$(run clean)" || { echo "clean run failed" >&2; exit 2; }
echo "$CLEAN"

echo "=== POISONED: getTreeHeight base + 1 ==="
python3 - "$SRC" "$NEEDLE" "$POISON" <<'PY'
import sys
p, needle, poison = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(p).read()
assert s.count(needle) == 1, f"{p}: expected exactly one match, found {s.count(needle)}"
open(p, "w").write(s.replace(needle, poison))
PY
[ $? -eq 0 ] || { echo "could not apply the poison" >&2; exit 2; }
POISONED="$(run poisoned)" || { echo "poisoned run failed" >&2; exit 2; }
echo "$POISONED"

restore
trap - EXIT INT TERM

cm=$(printf '%s\n' "$CLEAN"    | grep -oP 'mismatches=\K[0-9]+' | head -1)
pm=$(printf '%s\n' "$POISONED" | grep -oP 'mismatches=\K[0-9]+' | head -1)
# The worst chunk is read GENERICALLY and not as `(49,48)`. When the poison
# fires hard enough the worst chunk MOVES — it went to (50,48) the first time
# this ran — and a pattern pinned to one chunk would print `n/a` there, which
# reads like a parse failure rather than like the control working.
cw=$(printf '%s\n' "$CLEAN"    | grep -oP 'worst chunk \K\([0-9]+,[0-9]+\): [0-9]+' | head -1)
pw=$(printf '%s\n' "$POISONED" | grep -oP 'worst chunk \K\([0-9]+,[0-9]+\): [0-9]+' | head -1)
echo
echo "  clean    mismatches=$cm  worst chunk ${cw:-n/a}"
echo "  poisoned mismatches=$pm  worst chunk ${pw:-n/a}"
[ -n "$cm" ] && [ -n "$pm" ] || { echo "NW-CORNER-POISON INCONCLUSIVE — no mismatch counts parsed" >&2; exit 2; }
if [ "$cm" = "$pm" ]; then
    echo "NW-CORNER-POISON DID NOT FIRE — byte-identical. The window does not"
    echo "  place a spruce trunk in this corner and the tree lump is something"
    echo "  else. That is a finding, not a failure: report it."
    exit 1
fi
echo "NW-CORNER-POISON FIRED — the lump is routed through the trunk placer."
