#!/usr/bin/env bash
# ow-biome-census.sh — every published overworld window, decomposed by biome.
#
# `docs/PARITY.md` publishes 41 overworld figures through `ci/figures.toml`,
# and every one of them is centre chunk (64,64) at r=16 on one of three seeds.
# Section 2's narrower rows — chunks 57..71 and chunks 59..69 — are sub-squares
# of that same centre. Three points on the map. Each was chosen by centre and
# radius; none was chosen by biome, and until this script nothing reported what
# biomes they contain.
#
# They contain EIGHTEEN of the overworld's fifty-five between them:
#
#   20260722 r=16   savanna 42.74%  lukewarm_ocean 39.34%  river 7.44%
#                   beach 6.48%  sulfur_caves 3.49%  windswept_savanna 0.51%
#   12345    r=16   frozen_ocean 78.70%  snowy_plains 8.43%  snowy_beach 8.30%
#                   frozen_river 1.88%  snowy_taiga 1.84%  sulfur_caves 0.84%
#   12345    r=18   as above plus ice_spikes 0.10%
#   31415926 r=16   lukewarm_ocean 61.74%  beach 22.17%  savanna 5.92%
#                   river 3.77%  sulfur_caves 2.20%  jungle 1.72%
#                   lush_caves 0.94%  plains 0.73%  deep_lukewarm_ocean 0.34%
#                   sparse_jungle 0.23%  forest 0.25%
#
# THE THIRD WINDOW WAS ALMOST MISSED AND THE COUNT WAS PUBLISHED WRONG BECAUSE
# OF IT. Seed 31415926's overworld references are not named `gen-wide-*` like
# every other window's; they are `floors-ow31415926r16-*` under
# `~/rustmc-floors/oracle-work`, and two searches keyed on the other naming
# family — one on `HARVEST-ORDER`, one on `gen-wide-` — each returned nothing
# and each looked conclusive. This branch published "twelve of fifty-five" and
# "forty-three with no figure" on that basis. It is EIGHTEEN and THIRTY-SEVEN.
# The correction is recorded rather than edited away because the wrong numbers
# were committed.
#
# It is also the window that holds `jungle`, `plains`, `forest`, `lush_caves`,
# `sparse_jungle` and `deep_lukewarm_ocean` — six biomes this branch called
# unmeasured — and `docs/PARITY.md` section 2 calls that window corroboration
# only and never quotable on its own. So those six appear in a window that
# carries no quotable figure, which is a different statement from unmeasured
# and a weaker one than measured.
#
# So "the overworld at r=16" is, on the flagship seed, a savanna-and-warm-ocean
# reading, and on the only seed carrying a quotable exactness it is 78.7% deep
# frozen water. `docs/DETERMINISM.md` section 47 made this argument for the
# nether; section 48 makes it here, where the weights are more lopsided.
#
# WHAT THE DECOMPOSITION SHOWS, AND IT IS A NULL. Against the N=2 floor taken
# on the same pair, EVERY biome of the 20260722 and 12345 windows is under its
# own floor. On 31415926 exactly one biome clears it, `lush_caves` at 1.02x
# over 0.94% of the window, which is as thin as this project quotes anything:
#
#   20260722   biome              ours    floor(N=2)   ratio
#              lukewarm_ocean     2387        6941     0.34
#              savanna            1714        8105     0.21
#              sulfur_caves        874       12969     0.07
#              river               434         730     0.59
#              beach               135         697     0.19
#              windswept_savanna     0         118     0.00
#
#   12345      frozen_ocean       1834       13896     0.13
#              sulfur_caves       1213        1698     0.71
#              snowy_beach         244        1104     0.22
#              frozen_river        145         360     0.40
#              snowy_plains         79         472     0.17
#              snowy_taiga          38         249     0.15
##   31415926   lush_caves         5765        5672     1.02   <- the one
#              jungle             4388        6644     0.66
#              lukewarm_ocean     3798       21520     0.18
#              sulfur_caves       1400       12310     0.11
#              savanna             800        1314     0.61
#              beach               351        5865     0.06
#              river                 4         372     0.01
#
# The aggregate was already known to be under its floor and therefore
# disqualified by ORACLE rule 6. What was not known is that no SUB-window
# rescues it: there is no biome inside either square where this port and
# vanilla's own two runs are far enough apart to see anything. The r=16
# overworld window is exhausted per biome, not merely on average, and the next
# information has to come from a different square.
#
# AND THE ONE NAME WORTH CARRYING OUT OF IT. `sulfur_caves` is 3.49% of the
# 20260722 window and 15.76% of its residual, and 0.84% of the 12345 window and
# 34.14% of its residual — the only biome in either square that is
# over-represented in the residual, and the only one that gets within 0.71 of
# its floor. It is also THE ONLY CAVE BIOME EITHER WINDOW CONTAINS.
# `dripstone_caves`, `lush_caves` and `deep_dark` appear in neither, and
# `minecraft:large_dripstone` is this port's single largest feature refusal by
# count. See `ci/ow-biome-scout.sh` and `ci/ow-biome-coverage.py`.
#
# The two per-biome figures this script registers, in the form
# `ci/figures.toml` records them. Both are UNDER their floor and neither is
# quotable; they are registered because they are printed, and every printed
# number in this project needs an artefact and a window:
#
#   sulfur_caves 874 of 3723607, 99.976528%    seed 20260722 r=16, floor 12969
#   sulfur_caves 1213 of 899230, 99.865107%    seed 12345    r=16, floor  1698
#   jungle 4388 of 1835916, 99.760991%         seed 31415926 r=16, floor  6644
#
# And one that IS above its floor, barely:
#
#   lush_caves 5765 of 1002840, 99.425133%     seed 31415926 r=16, floor  5672
#
# ORDER IS READ, NEVER ASSERTED — through `ci/harvest-order.sh`, because eight
# of the nine 20260722 references keep their stamp in a different directory
# tree and a plain grep of the reference finds nothing. That resolver's header
# is the whole story.
set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)" || exit 2
REFS=${OWC_REFS:-$HOME/rustmc/oracle-work}
OUT=${OWC_OUT:-$HOME/owcensus-out}
D=./target/release/worldgen-dump
G=./target/release/region-differ
HO=./ci/harvest-order.sh
# Scoped to each binary's OWN dependency graph — see ci/binary-is-current.sh
# for why a workspace-wide `find` false-refuses and cannot be satisfied by
# rebuilding.
for b in "$D" "$G"; do
    ./ci/binary-is-current.sh "$b" || exit 2
done
mkdir -p "$OUT"
rc=0
one() {  # <tag> <seed> <r> <cx> <cz> <ref-a> <ref-b> <bx1> <bz1> <bx2> <bz2>
    local tag=$1 seed=$2 r=$3 cx=$4 cz=$5 a=$REFS/$6 b=$REFS/$7; shift 7
    local ord
    ord=$("$HO" "$a") || { rc=2; return; }
    [ -d "$b" ] || { echo "  no twin at $b — REFUSING." >&2; rc=2; return; }
    if [ ! -s "$OUT/$tag/region/r.1.1.mca" ]; then
        rm -rf "$OUT/$tag"
        RUSTMC_DECORATE_ORDER="$ord" "$D" full "$seed" "$r" "$OUT/$tag" "$cx" "$cz" \
            > "$OUT/$tag.log" 2>&1
    fi
    echo "=== $tag  seed=$seed r=$r centre=$cx,$cz  dump_order=$ord"
    echo "--- composition (self-diff of the reference: zero mismatches, a free census)"
    "$G" diff "$a" "$a" --bbox "$@" --by-biome 2>&1 | sed -n '2,9p'
    echo "--- FLOOR per biome (vanilla vs vanilla, same pair)"
    "$G" diff "$a" "$b" --bbox "$@" --by-biome 2>&1 | sed -n '1,9p'
    echo "--- OURS per biome, twin-masked, VANILLA FIRST"
    "$G" diff "$a" "$OUT/$tag/region" --twin "$b" --bbox "$@" --by-biome 2>&1 | sed -n '1,10p'
    echo
}
one ow-20260722-r16 20260722 16 64 64 \
    gen-wide-20260722-r16-ow-a gen-wide-20260722-r16-ow-b 48 48 80 80
one ow-12345-r16    12345    16 64 64 \
    gen-wide-12345-r16-ow-c   gen-wide-12345-r16-ow-d   48 48 80 80
# Seed 31415926's references live under a DIFFERENT ROOT and a different naming
# family; `OWC_REFS` is overridden for this one rather than the pair renamed,
# because renaming a reference is renaming an artefact.
OWC_REFS=${OWC_REFS_31415926:-$HOME/rustmc-floors/oracle-work} \
REFS=${OWC_REFS_31415926:-$HOME/rustmc-floors/oracle-work} \
one ow-31415926-r16 31415926 16 64 64 \
    floors-ow31415926r16-a    floors-ow31415926r16-b    48 48 80 80
exit "$rc"
