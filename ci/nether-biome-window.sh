#!/usr/bin/env bash
# nether-biome-window.sh — every published nether figure, per biome, re-taken.
#
# docs/PARITY.md §5 stopped publishing a single nether exactness on 2026-08-18
# and started publishing one per biome, because an aggregate over a nether
# window is a weighted average whose weights are that window's biome areas and
# nobody reading it knows those weights. docs/DETERMINISM.md §47 measures the
# weights: 15.6% of the r=16 window is `basalt_deltas` and carries 98.9% of
# vanilla's own floor.
#
# This is the artefact `ci/figures.toml` names for all eight of those figures,
# and it prints each one so a reader can compare rather than trust. The
# expected values, so a drift is visible in the output rather than only in the
# gate:
#
#   soul_sand_valley 104 of 7323536        seed 20260722 c(64,64)   r=7
#   soul_sand_valley 253 of 7372749        seed 20260722 c(100,100) r=7
#   basalt_deltas 33112 of 2052634         seed 31415926 c(64,64)   r=7
#   soul_sand_valley 1312 of 2090453       seed 31415926 c(64,64)   r=7
#   nether_wastes 1165 of 3105666          seed 31415926 c(64,64)   r=7
#   soul_sand_valley 70130 of 30089264, 99.766927%   seed 20260722 c(64,64) r=16
#   basalt_deltas 109518 of 5188505        seed 20260722 c(64,64)   r=16
#   nether_wastes 5 of 28634               seed 20260722 c(64,64)   r=16
#
# Added 2026-08-18 with the two windows that hold the biomes nobody had ever
# measured. `crimson_forest` and `warped_forest` appear in none of the four
# windows above, so they were unmeasured for a window-finding reason and not a
# measurement one:
#
#   crimson_forest 79911 of 6674283, 98.802703%    seed 31415926 c(0,0)      r=7
#   crimson_forest 58229 of 4819195, 98.791728%    seed 31415926 c(-200,200) r=7
#   warped_forest 13795 of 1795823, 99.231829%     seed 31415926 c(-200,200) r=7
#   nether_wastes 5053 of 722927, 99.301036%       seed 31415926 c(-200,200) r=7
#
# The two crimson readings are independent windows a thousand blocks apart and
# agree to 0.011 percentage points. All four residuals are the same three
# unported feature families, which `worldgen-dump` names in its own REFUSED
# block: `huge_fungus`, `nether_forest_vegetation`, `weeping_vines`.
#
# THE DUMP ORDER IS READ OFF THE REFERENCE, NEVER ASSERTED (ORACLE rule 29).
# Every reference this script uses stamps `gen_order=rows`, whose matching
# `RUSTMC_DECORATE_ORDER` is `zx`. It is worth 335 against 7,638 on the r=7
# window, so a nether figure taken in the wrong order is not a worse figure,
# it is a different measurement.
#
# `--ybox 0 127` on every call. `DimensionType` gives the nether 256 blocks
# and its noise settings generate 128, so the top eight sections are
# guaranteed air on both sides and half of an unbounded nether denominator is
# free.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)" || exit 2
REFS=${NBW_REFS:-$HOME/rustmc/oracle-work}
OUT=${NBW_OUT:-$HOME/nether-biome-window}
D=./target/release/worldgen-dump
G=./target/release/region-differ
for b in "$D" "$G"; do
    [ -x "$b" ] || { echo "  $b is missing — cargo build --release -p worldgen-dump -p region-differ. REFUSING." >&2; exit 2; }
    # A binary older than the tree produces self-consistent dumps and a delta
    # of exactly zero, which reads as "the change did nothing".
    newer=$(find crates tools -type f \( -name '*.rs' -o -name '*.toml' \) -newer "$b" -print -quit 2>/dev/null)
    [ -z "$newer" ] || { echo "  $b is OLDER than $newer — it does not contain this tree. REFUSING." >&2; exit 2; }
done
mkdir -p "$OUT"
rc=0
# `<refprefix>` is the whole name up to the trailing `a`/`b`, INCLUDING any
# infix. The r=16 ensemble is `gen-wide-...-nether-a`; the r=7 references
# harvested for §5 are `van-nether-...-open-a`, and a hard-coded `-a` suffix
# silently refused five of the six windows on this script's first run — which
# is the refusal working, but it is also why the prefix is passed whole rather
# than assembled here.
one() {  # <tag> <seed> <r> <cx> <cz> <refprefix-through-the-dash> <bx1> <bz1> <bx2> <bz2>
    local tag=$1 seed=$2 r=$3 cx=$4 cz=$5 pfx=$6; shift 6
    local a="$REFS/${pfx}a" b="$REFS/${pfx}b"
    for d in "$a" "$b"; do
        [ -d "$d" ] || { echo "  no reference at $d — REFUSING rather than reporting a figure it did not measure." >&2; rc=2; return; }
    done
    # Read it, do not assert it.
    local ord
    ord=$(grep -m1 -oP '^dump_order=\K.*' "$a/HARVEST-ORDER" 2>/dev/null || echo missing)
    case "$ord" in
        zx|zx_zrev) ;;
        *) echo "  $a stamps dump_order='$ord', which no RUSTMC_DECORATE_ORDER reproduces — REFUSING." >&2; rc=2; return ;;
    esac
    if [ ! -s "$OUT/$tag/region/r.1.1.mca" ]; then
        rm -rf "$OUT/$tag"
        RUSTMC_DECORATE_ORDER="$ord" "$D" full-nether "$seed" "$r" "$OUT/$tag" "$cx" "$cz" \
            > "$OUT/$tag.log" 2>&1
    fi
    echo "=== $tag  seed=$seed r=$r centre=$cx,$cz  dump_order=$ord (read off ${pfx}a)"
    "$G" diff "$a" "$OUT/$tag/region" --twin "$b" --bbox "$@" --ybox 0 127 --by-biome \
        2>&1 | sed -n '1,9p'
}
one r7-c6464    20260722  7  64  64 van-nether-20260722-c6464-open-      57 57 71 71
one r7-c100100  20260722  7 100 100 van-nether-20260722-c100100-open-    93 93 107 107
one r7-s2       31415926  7  64  64 van-nether-31415926-c6464-open-      57 57 71 71
one r16-c6464   20260722 16  64  64 gen-wide-20260722-r16-nether-        48 48 80 80
one r7-crimson  31415926  7   0   0 van-nether-31415926-c0_0-open-       -7 -7 7 7
one r7-warped   31415926  7 -200 200 van-nether-31415926-c-200_200-open- -207 193 -193 207
exit "$rc"
