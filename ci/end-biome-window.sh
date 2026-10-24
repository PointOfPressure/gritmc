#!/usr/bin/env bash
# end-biome-window.sh — every published End figure, per biome, re-taken.
#
# docs/PARITY.md §6 published two aggregate numbers for the End at r=16 and
# decomposed neither. An aggregate over a dimension is a weighted average whose
# weights are that window's biome areas, and the End's published window turns
# out to have a weight of **zero** on the biome that defines the dimension:
#
#   r=16, centre (64,64), seed 20260722 — 1,089 chunks, 71,368,704 blocks
#     end_midlands        25,821,184   36.18%
#     small_end_islands   19,464,192   27.27%
#     end_highlands       18,415,616   25.80%
#     end_barrens          7,667,712   10.74%
#     the_end                      0    0.00%   <-- the central island
#
# The centre of that window is block (1,024, 1,024), 1,448 blocks from the
# origin, and `TheEndBiomeSource` returns `the_end` only inside 1,000. So the
# island with the ten obsidian spikes, the exit portal and the arrival platform
# — the thing a player calls "the End" — was outside every window this project
# had ever measured, in a dimension it published 100.000000% for.
#
# The windows below fix that. Expected values, so a drift is visible in the
# output rather than only in the gate:
#
#   the_end            0 of 14745600, 100.000000%   seed 20260722 c(0,0)    r=7
#   end_midlands       0 of 25821184, 100.000000%   seed 20260722 c(64,64)  r=16
#   small_end_islands  0 of 19464192, 100.000000%   seed 20260722 c(64,64)  r=16
#   end_highlands      0 of 18415616, 100.000000%   seed 20260722 c(64,64)  r=16
#   end_barrens        0 of  7667712, 100.000000%   seed 20260722 c(64,64)  r=16
#
# All five are zero, so the number this script defends is a **coverage** claim
# rather than a fidelity one, and the three windows are chosen to make each
# ported feature reachable by at least one of them. Vanilla's own feature lists
# (`generated-data/.../worldgen/biome/*.json`), which is what "reachable" means:
#
#   the_end            end_spike, end_platform
#   end_highlands      end_gateway_return, chorus_plant
#   end_midlands       (none)
#   small_end_islands  end_island_decorated
#   end_barrens        (none)
#
# NONE OF THESE WINDOWS IS VACUOUS AND THE CENSUS IS HERE SO THAT CAN BE
# CHECKED RATHER THAN TRUSTED. A green over a void window would prove nothing,
# and 91% of an End window is air:
#
#   c(0,0)   r=7   1,257,395 end_stone, 40,524 obsidian, 146 iron_bars,
#                  10 bedrock, 10 fire  — the ten spikes, their cages, their
#                  bedrock tops and their crystal fires, plus the arrival
#                  platform, which `end_platform` fixes at exactly (100,49,0)
#                  and which is therefore inside this window and no other.
#   c(83,62) r=7   3,183 purpur_block, 1,226 purpur_stairs, 1,164 purpur_pillar,
#                  918 end_stone_bricks, 202 magenta_stained_glass, 114 end_rod,
#                  16 magenta_wall_banner, 9 ladder, 3 chest, 1 ender_chest
#                  — a whole end city, the dimension's only structure set.
#   c(64,64) r=16  12,497 chorus_plant, 1,571 chorus_flower and NOTHING ELSE
#                  but end_stone and air: `starts=0 pieces=0`. The widest End
#                  window this project has exercises exactly two block
#                  families, which is the fact the aggregate hid.
#
# THE ONE UNPORTED FEATURE IS IN NONE OF THEM, AND THAT IS ARITHMETIC RATHER
# THAN BAD LUCK. `end_gateway_return` is `rarity_filter chance=700` then a
# biome filter to `end_highlands`. Over all three windows — 1,539 chunks, of
# which end_highlands' share is ~398 chunk-equivalents — the expected number of
# return gateways is **0.57**, and the observed number is 0: no `end_gateway`
# block and no bedrock anywhere outside the ten spike tops. So the End's
# 100.000000% has never been asked about the one thing the port does not do,
# and could not have been. Measuring it needs a window with ~1,750
# end_highlands chunks, which none of these is.
#
# THE DUMP ORDER IS READ OFF THE REFERENCE, NEVER ASSERTED (ORACLE rule 29).
# Every reference here stamps `gen_order=rows`, whose matching
# `RUSTMC_DECORATE_ORDER` is `zx`. On this dimension that knob is worth
# **0 against 944**: our dump is byte-identical to a `rows` vanilla and differs
# from a `rows_rev` vanilla by 944 blocks — which is exactly what two vanilla
# runs in those two orders differ by, to the block. An End figure taken in the
# wrong order is not a worse figure, it is a different measurement.
#
# `--ybox 0 255` on every call, which for the End is full height:
# `noise_settings_end` generates 0..255 and the published denominators
# (71,368,704 = 1,089 x 65,536) are already that. It is written out rather than
# omitted because the nether's mandatory `0 127` sits three lines away in the
# sister script and a copied invocation would silently halve the End.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)" || exit 2
REFS=${EBW_REFS:-$HOME/rustmc/oracle-work}
OUT=${EBW_OUT:-$HOME/end-biome-window}
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
one() {  # <tag> <seed> <r> <cx> <cz> <refprefix-through-the-dash> <bx1> <bz1> <bx2> <bz2>
    local tag=$1 seed=$2 r=$3 cx=$4 cz=$5 pfx=$6; shift 6
    local a="$REFS/${pfx}a" b="$REFS/${pfx}b"
    for d in "$a" "$b"; do
        [ -d "$d" ] || { echo "  no reference at $d — REFUSING rather than reporting a figure it did not measure." >&2; rc=2; return; }
    done
    # Read it, do not assert it. Every End reference this project had before
    # 2026-08-18 predates HARVEST-ORDER and would land in the refusal arm.
    local ord
    ord=$(grep -m1 -oP '^dump_order=\K.*' "$a/HARVEST-ORDER" 2>/dev/null || echo missing)
    case "$ord" in
        zx|zx_zrev) ;;
        *) echo "  $a stamps dump_order='$ord', which no RUSTMC_DECORATE_ORDER reproduces — REFUSING." >&2; rc=2; return ;;
    esac
    if [ ! -s "$OUT/$tag/region/r.1.1.mca" ]; then
        rm -rf "$OUT/$tag"
        RUSTMC_DECORATE_ORDER="$ord" "$D" full-end "$seed" "$r" "$OUT/$tag" "$cx" "$cz" \
            > "$OUT/$tag.log" 2>&1
    fi
    echo "=== $tag  seed=$seed r=$r centre=$cx,$cz  dump_order=$ord (read off ${pfx}a)"
    "$G" diff "$a" "$OUT/$tag/region" --twin "$b" --bbox "$@" --ybox 0 255 --by-biome \
        2>&1 | sed -n '1,10p'
}
one r7-c0_0   20260722  7  0  0 van-end-20260722-c0_0-open-    -7 -7 7 7
one r7-c8362  20260722  7 83 62 van-end-20260722-c8362-open-   76 55 90 69
one r16-c6464 20260722 16 64 64 gen-wide-20260722-r16-end-     48 48 80 80

# THE FLOOR, DECOMPOSED THE SAME WAY, because a residual per biome and a floor
# per dimension cannot be read against each other. The N=6 union floor over
# `gen-wide-20260722-r16-end-{a..f}` is 127 and `a` against `c` realises all of
# it, so this one pair is the whole floor rather than a sample of it — which is
# why it can be decomposed by a single diff instead of a union over fifteen.
# `ci/floor-ensemble.sh` is what establishes that; this reproduces the split.
#
#   end_highlands  123 of 18415616  99.999332%   25.80% of the window, 96.85% of the floor
#   end_midlands     4 of 25821184  99.999985%   36.18% of the window,  3.15% of the floor
#
# And the order control, which is the same measurement with the knob turned:
# `rows` against `rows_rev`, vanilla against vanilla, is 944 (99.998678%) and
# ours against a `rows_rev` reference is the same 944 to the block, because our
# dump is a forward-order vanilla bit for bit.
floor() {  # <tag> <a> <b> <bx1> <bz1> <bx2> <bz2>
    local tag=$1 a=$2 b=$3; shift 3
    for d in "$REFS/$a" "$REFS/$b"; do
        [ -d "$d" ] || { echo "  no reference at $d — REFUSING." >&2; rc=2; return; }
    done
    echo "=== $tag  $a vs $b"
    "$G" diff "$REFS/$a" "$REFS/$b" --bbox "$@" --ybox 0 255 --by-biome 2>&1 | sed -n '1,8p'
}
floor r16-floor-ac  gen-wide-20260722-r16-end-a gen-wide-20260722-r16-end-c    48 48 80 80
floor r16-order-rev gen-wide-20260722-r16-end-a gen-wide-20260722-r16-end-rr-a 48 48 80 80
exit "$rc"
