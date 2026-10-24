#!/usr/bin/env bash
# ow-new-windows.sh — the three overworld squares that measure biomes no
# published figure has ever been taken in, and what they found.
#
# `ci/ow-biome-census.sh` establishes that all 41 published overworld figures
# are centre chunk (64,64) at r=16 and hold EIGHTEEN of the dimension's
# fifty-five biomes between them, and that exactly one of the eighteen is above
# its own floor — `lush_caves` at 1.02x — so the published squares are
# exhausted per biome and not merely on average.
# `ci/ow-biome-scout.sh` then found squares for the other thirty-seven
# without harvesting anything, and `ci/ow-biome-coverage.py` ranked them.
#
# These are the three that were harvested, seed 20260722, r=7, GEN_ORDER=rows:
#
#   A  centre (-1056,-672)   12 biomes   deep_dark, pale_garden, lush_caves,
#                                        dripstone_caves, dark_forest, ...
#   E  centre  (-960, 672)    7 biomes   mangrove_swamp, swamp, dripstone_caves
#   B  centre  (-480,-1440)   9 biomes   forest, jungle, sparse_jungle, plains,
#                                        deep_dark, stony_peaks
#
# TWENTY-THREE DISTINCT BIOMES, against eighteen for the entire published corpus,
# in three squares that are each ONE FIFTH the area of the r=16 window.
#
# AND THIRTEEN OF THEM RESOLVE. Twin-masked, vanilla first, N=2, dump order
# read off each reference's own stamp. `x` is residual over floor; above 1.0 is
# a measurement and below it is not (ORACLE rule 6). The published r=16
# squares have ZERO biomes above 1.0 between them:
#
#   window  biome                     ours     floor      x    exactness
#   A       river                     2396       239  10.03    99.824140%
#   A       old_growth_birch_forest  16252      1849   8.79    99.450163%
#   A       dripstone_caves          11105      1482   7.49    98.924657%
#   E       windswept_hills            141        25   5.64    99.981378%
#   A       meadow                     546       155   3.52    99.925554%
#   E       dripstone_caves          24970      6874   3.63    99.462110%
#   A       deep_dark                47290     17695   2.67    97.499346%
#   B       sparse_jungle             7547      3108   2.43    99.768900%
#   E       sunflower_plains          1435       668   2.15    99.933333%
#   A       birch_forest              9164      4555   2.01    99.768735%
#   B       dripstone_caves          21761     11616   1.87    99.518504%
#   B       deep_dark                34408     21900   1.57    98.683737%
#   B       plains                     382       336   1.14    99.979962%
#   A       grove                     3160      2604   1.21    99.909724%
#   E       swamp                     4506      4228   1.07    99.917763%
#   A       lush_caves                4392      4229   1.04    99.659783%
#
# The expected values in the form ci/figures.toml registers them, so a drift is
# visible in this header as well as in the gate:
#
#   deep_dark 47290 of 1891105, 97.499346%                 A  (-1056,-672)
#   dripstone_caves 11105 of 1032694, 98.924657%           A
#   old_growth_birch_forest 16252 of 2955783, 99.450163%   A
#   birch_forest 9164 of 3962549, 99.768735%               A
#   lush_caves 4392 of 1290939, 99.659783%                 A
#   pale_garden 6263 of 1010863, 99.380430%                A  UNDER FLOOR
#   deep_dark 34408 of 2614068, 98.683737%                 B  (-480,-1440)
#   dripstone_caves 21761 of 4519456, 99.518504%           B
#   sparse_jungle 7547 of 3265692, 99.768900%              B  after cocoa
#   sparse_jungle 7976 of 3265692, 99.755764%              B  before cocoa
#   plains 382 of 1906416, 99.979962%                      B
#   jungle 41024 of 3424530, 98.802055%                    J  after cocoa
#   jungle 39874 of 3424530, 98.835636%                    J  before cocoa
#   sparse_jungle 11793 of 6923397, 99.829665%             J  after cocoa
#   deep_ocean 555 of 3440414, 99.983868%                  O
#   cold_ocean 0 of 1253376, 100.000000%                   O  its floor is 0 too
#   dripstone_caves 24970 of 4642214, 99.462110%           E  (-960,672)
#
# RE-MEASURED 2026-08-18, branch `sculk-deep-dark`. Same windows, same twin
# pairs, same N=2 floors. The sculk vein spread was switched on (it was written
# on 2026-08-09 and defaulted off under a comment four later commits had
# overturned) and `SculkVeinBlock.onDischarged` corrected to write AIR rather
# than cave_air; `mega_jungle_trunk_placer`, `jungle_foliage_placer` and
# `minecraft:bamboo` were ported. Two of the three moved toward their floor and
# one away from it, and all three are registered:
#
#   deep_dark 44180 of 1891105, 97.663800%                 A  was 47290, 2.50x
#   deep_dark 33499 of 2614068, 98.718511%                 B  was 34408, 1.53x
#   jungle 44460 of 3424530, 98.701720%                    J  was 41024, 4.27x
#
# Nine more rows moved at the same tip, for the same reason and with the same
# pairs and floors — a sculk patch does not respect a biome boundary, so caves
# neighbouring the deep dark move too. Window E is byte-identical and is not
# re-registered.
#
#   dripstone_caves 11122 of 1032694, 98.923011%           A  was 11105
#   birch_forest 9385 of 3962549, 99.763158%               A  was 9164
#   lush_caves 4669 of 1290939, 99.638325%                 A  was 4392
#   grove 3362 of 3500372, 99.903953%                      A  was 3160
#   meadow 616 of 733413, 99.916009%                       A  was 546
#   dripstone_caves 22106 of 4519456, 99.510870%           B  was 21761
#   sparse_jungle 7589 of 3265692, 99.767614%              B  was 7547
#   plains 271 of 1906416, 99.985785%                      B  was 382, IMPROVED
#   sparse_jungle 11995 of 6923397, 99.826747%             J  was 11793
#
# The jungle row is the wrong way for the third consecutive jungle port and is
# published as such; `docs/PARITY-ROADMAP.md` §67 has the mechanism and the
# padding sweep that localises it to about two draws per mega jungle tree.
#
# RE-MEASURED 2026-08-18, branch `feat-worldgen-refused`, off `sculk-deep-dark`.
# Same windows, same twin pairs, same N=2 floors, same dump order `zx`.
# `minecraft:large_dripstone` is ported. It was this port's largest feature
# refusal by COUNT anywhere — 4,569 declines across these squares, 543 in A,
# 2,049 in B, 1,977 in E — and only FIFTH by blocks at 1,593. Both halves
# belong in any sentence quoting either: a tally ranked by count ranks how
# often something is ATTEMPTED, and this one is attempted 10..48 times per
# chunk over the whole vertical column. THE 4,569 DECLINES ARE NOW 0.
#
#   dripstone_caves 9421 of 1032694, 99.087726%            A  was 11122, 6.36x
#   dripstone_caves 8027 of 4519456, 99.822390%            B  was 22106, UNDER FLOOR
#   dripstone_caves 14912 of 4642214, 99.678774%           E  was 24970, 2.17x
#   deep_dark 44178 of 1891105, 97.663905%                 A  was 44180
#   deep_dark 33350 of 2614068, 98.724211%                 B  was 33499
#   birch_forest 9078 of 3962549, 99.770905%               A  was 9385
#   old_growth_birch_forest 16218 of 2955783, 99.451313%   A  was 16252
#   river 2398 of 1362449, 99.823993%                      A  was 2396, WORSE
#   meadow 576 of 733413, 99.921463%                       A  was 616
#   sparse_jungle 7352 of 3265692, 99.774872%              B  was 7589
#   sunflower_plains 841 of 2152484, 99.960929%            E  was 1435
#   windswept_hills 33 of 757159, 99.995642%               E  was 141
#   swamp 4488 of 5479292, 99.918092%                      E  was 4506
#
# WINDOWS J AND O DID NOT MOVE BY A SINGLE BLOCK. Neither square holds a
# dripstone cave, and a control that moves by exactly zero is what makes the
# thirteen rows above attributable to this port rather than to drift. It is
# the same argument window E made for the sculk switch, made by two squares.
#
# TWO OF THE THIRTEEN ARE NOT GOOD NEWS AND ARE PUBLISHED ANYWAY. `river` in A
# went 2,396 -> 2,398: a port that improves twelve rows and worsens one has
# worsened one. And `dripstone_caves` in B fell from 1.90x its floor to 0.69x,
# which means window B has STOPPED ADJUDICATING this biome — under ORACLE
# rule 6 that row is no longer a measurement at all. An improvement that
# removes a window is still an improvement, and the next reader needs to know
# the window is gone rather than rediscover it.
#
# `jungle` in J is unchanged at 44,460 and remains the open item.
#
# AND AGAIN on the same branch, after `dark_oak_trunk_placer` and
# `dark_oak_foliage_placer` were ported. One pair of placers, five trees —
# `minecraft:dark_oak`, `dark_oak_leaf_litter`, `pale_oak`,
# `pale_oak_bonemeal`, `pale_oak_creaking` — of which the first two are now
# placeable and the pale oaks are still refused on `pale_moss`.
#
#   birch_forest 8614 of 3962549, 99.782615%               A  was 9078
#   meadow 390 of 733413, 99.946824%                       A  was 576
#   grove 2563 of 3500372, 99.926779%                      A  was 3362, UNDER FLOOR
#
# WINDOWS B, E, J AND O DID NOT MOVE BY A SINGLE BLOCK. None of them holds a
# `dark_forest` or a `pale_garden`, so four squares of five are the control.
#
# NEITHER `dark_forest` NOR `pale_garden` CAN ADJUDICATE THIS PORT. They sit at
# 0.67x and 0.52x their floors in A, so their rows are not measurements under
# ORACLE rule 6 — `dark_forest` 7,846 -> 7,277 and `pale_garden` 6,435 -> 6,523
# are movements without a verdict. THE BLOCK CENSUS IS THE INSTRUMENT HERE:
#
#   minecraft:dark_oak_leaves     0 -> 6784   vanilla 6017   OVERSHOOT +767
#   minecraft:dark_oak_log        0 -> 2651   vanilla 2185   OVERSHOOT +466
#   minecraft:pale_oak_leaves     0 ->    0   vanilla 6732   still refused
#   minecraft:pale_oak_log        0 ->    0   vanilla 2397   still refused
#
# An overshoot, not a shortfall: the trees place, and there are more of them or
# they are bigger than vanilla's. That is the next thing to decompose, and it
# is why `grove` improving into unmeasurability is not the whole story.
#
# RE-MEASURED 2026-08-19, branch `pale-census`, off `feat-undoworld`. Same
# window, same twin pair, same N=2 floor, same dump order `zx`. `pale_moss`
# (§85), `pale_moss_carpet`'s survival rule, `MossyCarpetBlock.placeAt`,
# `leaf_litter`'s survival rule and `WorldGenRegion.getRandom()` are all
# ported. WINDOW A'S DECLINE COUNT IS 441 -> 23 ACROSS ALL FEATURES.
#
#   birch_forest 7902 of 3962549, 99.800583%               A  was 8614, 1.74x
#   meadow 353 of 733413, 99.951869%                       A  was 390
#   grove 2596 of 3500372, 99.925836%                      A  was 2563, UNDER FLOOR
#   pale_garden 8309 of 1010863, 99.178029%                A  was 6435, UNDER FLOOR
#   dark_forest 5332 of 1426961, 99.626339%                A  was 7846, UNDER FLOOR
#
# WINDOWS B, E, J AND O ARE UNTOUCHED BY THIS WORK — none holds a `dark_forest`
# or a `pale_garden` — and inside A itself SEVEN OF TWELVE BIOMES DID NOT MOVE
# BY A SINGLE BLOCK across six runs: deep_dark, old_growth_birch_forest,
# dripstone_caves, lush_caves, river, jagged_peaks, forest.
#
# ONLY TWO OF THE FIVE MOVED ROWS ARE MEASUREMENTS. `birch_forest` at 1.74x its
# floor and `meadow` at 2.28x both improve, by 944 and 37. `pale_garden` (0.67x)
# and `dark_forest` (0.49x) are under their floors before and after, so their
# rows carry no verdict in either direction — `pale_garden`'s went UP by 1,767
# while its contents became almost exactly right, which is the `cocoa` shape.
# `grove` briefly crossed its floor mid-port and went back under it.
#
# THE BLOCK CENSUS IS THE INSTRUMENT, and this is what it says. Vanilla first,
# floors carried:
#
#   block               vanilla   before  after   floor
#   pale_oak_leaves        6732        0   7191     356   OVERSHOOT +459
#   pale_oak_log           2397        0   2744      28   OVERSHOOT +347
#   pale_moss_block        2836      723   2902     213   inside its floor
#   pale_hanging_moss       964        0   1022      58   at its floor
#   pale_moss_carpet        512        0    492      30   inside its floor
#   dark_oak_leaves        6017     6255   5775     107   UNDERSHOOT -242
#   dark_oak_log           2185     2367   2157      13
#   leaf_litter            3045     3154   3025      23   inside its floor
#   ------------------------------------------------------------------------
#   absolute error                 13255   1248     828
#
# `pale_garden`'s 9,129 — which is `pale_oak_leaves` 6732 plus `pale_oak_log`
# 2397, both at zero — IS 806, and both remaining rows are overshoots rather
# than shortfalls. See `docs/PARITY-ROADMAP.md` §89.
#
# THREE ROWS MOVED THE WRONG WAY MID-PORT AND ALL THREE CAME BACK.
# `dark_oak_log` reached 2415, `leaf_litter` 3153, and `grove` 2988 while the
# family was partly ported. §58.6's mechanism: a refused feature spends no
# randomness and vanilla spends the whole feature's, so each still-refusing
# pale oak desynchronised everything decorated after it in that chunk. A
# wrong-way number from a partly-ported family is evidence about the
# incompleteness, not about the port.
#
# THE `large_dripstone` BLOCK CENSUS DISAGREES WITH ITS OWN RESIDUAL IN A AND
# AGREES IN E, which is the `cocoa` shape and has to be said both ways:
#
#   window A   dripstone_block   10448 -> 10305   vanilla 12033   AWAY
#   window A   pointed_dripstone  2539 ->  2213   vanilla  2799   AWAY
#   window E   dripstone_block   36076 -> 39655   vanilla 46215   TOWARD, +3579
#   window E   pointed_dripstone  8967 ->  8839   vanilla 10200   away, -128
#
# Window E holds 4.5x the dripstone-cave volume of A, and there the port closes
# 35% of the `dripstone_block` gap. In A the few large dripstones it adds are
# outweighed by the pointed-dripstone clusters its draws displace. Both
# residuals improved; only one census did.
#   swamp 4506 of 5479292, 99.917763%                      E  after leave_vine
#   swamp 21030 of 5479292, 99.616191%                     E  before leave_vine
#   sunflower_plains 1435 of 2152484, 99.933333%           E
#   mangrove_swamp 82596 of 8691249, 99.049665%            E  UNDER FLOOR
#   river 2396 of 1362449, 99.824140%                      A
#   meadow 546 of 733413, 99.925554%                       A
#   grove 3160 of 3500372, 99.909724%                      A
#   lush_caves 4392 of 1290939, 99.659783%                 A
#   birch_forest 9164 of 3962549, 99.768735%               A
#   windswept_hills 141 of 757159, 99.981378%              E
#   sunflower_plains 1435 of 2152484, 99.933333%           E
#   plains 382 of 1906416, 99.979962%                      B
#
# WHAT J AND O ADD TO THE VERDICT TABLE:
#
#   window  biome                     ours     floor      x    exactness
#   J       jungle                   41024     10414   3.94    98.802055%
#   O       deep_ocean                 555       226   2.46    99.983868%
#   J       sparse_jungle            11793     10363   1.14    99.829665%
#
# `cold_ocean` is 0 of 1,253,376 — byte-identical to vanilla — and its floor
# over the same pair is ALSO 0, so it is registered UNQUOTABLE. Two runs
# agreeing is not the same as there being nothing to disagree about; that is
# `docs/PARITY.md`'s own argument about the End, made there after the End's
# "floor is exactly zero" claim was withdrawn, and it applies unchanged here.
#
# `ocean` 0.15x and `deep_cold_ocean` 0.77x are under their floors and are not
# measured by this square either.
#
# CORROBORATED ACROSS INDEPENDENT WINDOWS, which is the part that makes it a
# result rather than a reading. `deep_dark` resolves in A and in B — squares
# 900 chunks apart — at 2.67x and 1.57x. `dripstone_caves` resolves in all
# THREE, at 7.49x, 3.63x and 1.87x. Neither biome appears in any published
# window at any radius on any seed.
#
# UNDER THEIR FLOORS, AND THEREFORE NOT MEASURED HERE EITHER: `mangrove_swamp`
# 0.38 (vanilla disagrees with itself over 214,991 blocks of it — 2.4% of the
# biome), `pale_garden` 0.50, `jungle` 0.53, `stony_peaks` 0.51, `dark_forest`
# 0.72, `forest` 0.89 in B and 0.02 in A, `jagged_peaks` 0.60, `savanna` 0.16.
# They are named because a square that fails to adjudicate a biome is a result
# about the square, and the next reader should not spend a harvest rediscovering
# it.
#
# WHAT IS IN THE RESIDUAL, BY NAME. A block-family census of window A against
# its reference, which shares no mechanism with the positional diff:
#
#   minecraft:sculk           30184 -> 10       deep_dark, not generated
#   minecraft:sculk_vein       9053 -> 412
#   minecraft:pale_oak_leaves  6732 -> 0        pale_garden, no tree placed
#   minecraft:pale_oak_log     2397 -> 0
#   minecraft:pale_moss_block  2836 -> 723
#   minecraft:dark_oak_leaves  6017 -> 0        dark_forest, no tree placed
#   minecraft:dark_oak_log     2185 -> 0
#   minecraft:dripstone_block 12033 -> 10440    large_dripstone, 543 declines
#
# `minecraft:large_dripstone` is refused 543, 1977 and 2049 times in these three
# squares and is this port's single largest feature refusal by count. The
# dark oak and pale oak trees are one cause with two names:
# `minecraft:dark_oak_trunk_placer` is not a supported trunk placer, and
# `TreeConfig::is_supported` is an AND, so the whole tree is refused.
#
# The one repair made on this branch is `leave_vine`, and its effect is in the
# `swamp` row above: 21,030 -> 4,506, from 4.97x its floor to 1.07x. The same
# shape is still open in jungle — `["minecraft:cocoa", "trunk_vine",
# "leave_vine"]`, 186 declines, now waiting only on `cocoa` — but `jungle` sits
# at 0.53 of its floor in window B, so porting `cocoa` CANNOT BE VALIDATED on
# any square this project owns. That is a reason to harvest a jungle window
# before porting it, not a reason to skip it.
set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)" || exit 2
REFS=${OWNW_REFS:-$HOME/owcensus-refs}
OUT=${OWNW_OUT:-$HOME/owcensus-out}
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
one() {  # <tag> <cx> <cz> <bx1> <bz1> <bx2> <bz2>
    local tag=$1 cx=$2 cz=$3; shift 3
    local a="$REFS/van-ow-20260722-c${cx}_${cz}-open-a"
    local b="$REFS/van-ow-20260722-c${cx}_${cz}-open-b"
    local ord
    ord=$("$HO" "$a") || { rc=2; return; }
    [ -d "$b" ] || { echo "  no twin at $b — REFUSING." >&2; rc=2; return; }
    if [ ! -s "$OUT/$tag/region/r.-1.-1.mca" ] && [ ! -s "$OUT/$tag/region/r.-1.0.mca" ]; then
        rm -rf "$OUT/$tag"
        RUSTMC_DECORATE_ORDER="$ord" "$D" full 20260722 7 "$OUT/$tag" "$cx" "$cz" \
            > "$OUT/$tag.log" 2>&1
    fi
    echo "=== $tag  seed=20260722 r=7 centre=$cx,$cz  dump_order=$ord"
    echo "--- FLOOR per biome (vanilla vs vanilla, N=2, same pair)"
    "$G" diff "$a" "$b" --bbox "$@" --by-biome 2>&1 | sed -n '1,14p'
    echo "--- OURS per biome, twin-masked, VANILLA FIRST"
    "$G" diff "$a" "$OUT/$tag/region" --twin "$b" --bbox "$@" --by-biome 2>&1 | sed -n '1,15p'
    echo "--- what the port declined here"
    sed -n '/REFUSED/,$p' "$OUT/$tag.log" 2>/dev/null | head -6
    echo
}
one owA -1056 -672  -1063 -679 -1049 -665
one owE  -960  672   -967  665  -953  679
one owB  -480 -1440  -487 -1447 -473 -1433
one owJ    960  -192   953  -199  967  -185
one owO  -1248  -672 -1255  -679 -1241  -665
exit "$rc"
