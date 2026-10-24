#!/usr/bin/env bash
# bastion-nether-window.sh — the bastion figures, re-taken from artefacts.
#
# `docs/PARITY-ROADMAP.md` §98 publishes six numbers about one window. This
# takes all six, so a reader compares rather than trusts, and so that a lane
# that changes the nether cannot leave the prose behind:
#
#   soul_sand_valley 2455 of 30089264, 99.991841%
#   bastion_remnant (55,63)  23     margin 0, 105 pieces
#   bastion_remnant (70,49)   4     margin 0,  89 pieces
#   basalt_deltas        109518     unchanged by this port, and NOT quotable
#   the N=6 unclamped floor  1709 .. 6133, median 4048
#
# THE DUMP ORDER IS READ OFF THE REFERENCE, NEVER ASSERTED (ORACLE rule 29).
# All six r=16 nether references stamp `gen_order=rows`, whose matching
# `RUSTMC_DECORATE_ORDER` is `zx`. `nether-biome-census` measured order as
# worth 335 against 7,638 on the r=7 window, so a nether figure taken in the
# wrong order is not a worse figure, it is a different measurement.
#
# THE FLOOR IS THE FULL ENSEMBLE, NOT A PAIR. Every published floor for this
# window until now was `b` against `c` — one pair of fifteen — and the pair it
# happened to be is the fourth-lowest. `--floor` takes all fifteen. It is
# UNCLAMPED (`jvm_opts=` on every reference) and it carries its N, because
# `-Dmax.bg.threads=1` does NOT make a nether harvest reproducible: three
# clamped runs spread 34/438/460 against 75/134/151 for three default-pool
# ones, which is WIDER. The whole finding is on the `nether-biome-census`
# branch, which numbers it in DETERMINISM; it is cited by branch rather than
# by section because that section does not exist on this one.
#
# `--ybox 0 127` on every call: `DimensionType` gives the nether 256 blocks
# and its noise settings generate 128, so the top eight sections are
# guaranteed air on both sides and half of an unbounded denominator is free.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)" || exit 2
REFS=${BNW_REFS:-$HOME/rustmc/oracle-work}
OUT=${BNW_OUT:-$HOME/bastion-nether-window}
D=./target/release/worldgen-dump
G=./target/release/region-differ
for b in "$D" "$G"; do
    [ -x "$b" ] || { echo "  $b is missing — cargo build --release -p worldgen-dump -p region-differ. REFUSING." >&2; exit 2; }
    # A binary older than the tree produces a self-consistent dump and a delta
    # of exactly zero, which reads as "the change did nothing".
    newer=$(find crates tools -type f \( -name '*.rs' -o -name '*.toml' \) -newer "$b" -print -quit 2>/dev/null)
    [ -z "$newer" ] || { echo "  $b is OLDER than $newer — it does not contain this tree. REFUSING." >&2; exit 2; }
done
# `region-differ diff --by-biome` LANDS WITH `nether-biome-census`, NOT WITH
# THIS BRANCH. An aggregate nether exactness is a weighted average whose
# weights are the window's biome areas — 15.6% of this window is
# `basalt_deltas` and carries 98.9% of vanilla's own floor — so every figure
# here is per-biome and there is no honest way to take them without the flag.
# Refused by name rather than skipped: a skip here reports nothing and exits
# 0, which is how a figure stops being re-derived without anyone noticing.
if ! "$G" diff --help 2>&1 | grep -c -- --by-biome >/dev/null; then
    echo "  $G has no --by-biome. That flag lands with the nether-biome-census" >&2
    echo "  lane; until it merges, build region-differ from a tree that has it." >&2
    echo "  REFUSING rather than reporting an aggregate nobody can weight." >&2
    exit 2
fi

A="$REFS/gen-wide-20260722-r16-nether-a"
[ -d "$A" ] || { echo "  no reference at $A — REFUSING rather than reporting a figure it did not measure." >&2; exit 2; }
ORD=$(grep -m1 -oP '^dump_order=\K.*' "$A/HARVEST-ORDER" 2>/dev/null || echo missing)
case "$ORD" in
    zx|zx_zrev) ;;
    *) echo "  $A stamps dump_order='$ORD', which no RUSTMC_DECORATE_ORDER reproduces — REFUSING." >&2; exit 2 ;;
esac

if [ "${1:-}" = "--floor" ]; then
    # Vanilla against vanilla, no twin mask: masking a floor with one of its
    # own members is circular.
    echo "== soul_sand_valley floor, N=6, UNCLAMPED, 15 pairs, no twin mask =="
    for a in a b c d e f; do for b in a b c d e f; do
        [[ "$a" < "$b" ]] || continue
        v=$("$G" diff "$REFS/gen-wide-20260722-r16-nether-$a" "$REFS/gen-wide-20260722-r16-nether-$b" \
            --bbox 48 48 80 80 --ybox 0 127 --by-biome 2>/dev/null \
            | awk '/soul_sand_valley/ {print $3}')
        echo "  $a-$b $v"
    done; done | sort -k2 -n
    exit 0
fi

mkdir -p "$OUT"
if [ ! -s "$OUT/r16/region/r.1.1.mca" ]; then
    rm -rf "$OUT/r16"
    RUSTMC_TRACE_BEARD=1 RUSTMC_DECORATE_ORDER="$ORD" \
        "$D" full-nether 20260722 16 "$OUT/r16" 64 64 > "$OUT/r16.log" 2>&1
fi
echo "== the starts this port lays out (the third is OUTSIDE the window) =="
grep -E 'bastion_remnant' "$OUT/r16.log"
echo
echo "== seed=20260722 r=16 centre=64,64 nether  dump_order=$ORD (read off the reference) =="
"$G" diff "$A" "$OUT/r16/region" --twin "$REFS/gen-wide-20260722-r16-nether-b" \
    --bbox 48 48 80 80 --ybox 0 127 --by-biome 2>&1 | sed -n '1,9p'
echo
echo "== soul_sand_valley charged to VANILLA's saved starts, margin 0 =="
# Margin 0 and not the tool's default 16: `bastion_remnant` declares no
# `terrain_adaptation`, so it beards nothing and there is no ring to widen for.
"$D" jigsaw-starts "$A" > "$OUT/vanilla-starts.txt" 2>&1
"$G" diff "$A" "$OUT/r16/region" --twin "$REFS/gen-wide-20260722-r16-nether-b" \
    --bbox 48 48 80 80 --ybox 0 127 --by-biome --positions 2>/dev/null \
    | awk '$6=="minecraft:soul_sand_valley"' > "$OUT/ssv.txt"
python3 ci/attribute-residual.py "$OUT/ssv.txt" "$OUT/vanilla-starts.txt" 0 | head -8
