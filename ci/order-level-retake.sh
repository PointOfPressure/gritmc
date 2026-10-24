#!/usr/bin/env bash
# order-level-retake.sh — re-take the published r=16 overworld LEVEL with the
# harvest order controlled, and take the §4 ladder with it.
#
# WHY THIS EXISTS
#
# `ci/figures.toml:ow-r16-admissible-n2` records 29,605 of 107,053,056 —
# 99.972345% — and `docs/DETERMINISM.md` §15 then established that the figure
# was taken with an uncontrolled variable in it: our dump walks chunks x-major
# (`Area::chunks()`), every reference in the ensemble was harvested z-major
# (`ci/gen-vanilla-region.sh GEN_ORDER=rows` forceloads one z row at a time),
# and the same instrument charges an *unmodified vanilla run* 117,628 blocks
# for that mismatch against 3,804 for merely being a different run of the right
# order. So the published level is an accurate `|ours not in V|` under a
# condition nobody set, and it has to be re-taken under one that is set.
#
# There are exactly two ways to set it and they answer different questions:
#
#   A. MATCH the dump order to the references' harvest order (`zx`).  Controls
#      the variable.  Asks: with chunk order held fixed on both sides, what is
#      left?
#   B. WIDEN the ensemble so it holds both orders.  Averages over the variable.
#      Asks: is our block one vanilla writes here under *some* order?
#
# This script measures both, from its own dumps and its own diffs, so the
# choice is made on numbers this tree produced rather than on numbers quoted
# from a doc.  It also re-takes the innocent-vanilla floor at every N it
# reports a charge at, because a charge without the floor beside it is the
# defect this project keeps re-paying for.
#
# WHAT IT DELIBERATELY DOES NOT DO
#
# It does not read or write `~/rp-logs-order`, which belongs to the agent that
# measured §15, and it does not regenerate a reference.  Everything it consumes
# under `oracle-work` is read-only; everything it writes is under `$OUT`.
#
# Vanilla FIRST in every region-differ call.
#
#   LADDER_OUT   scratch root                    (default ~/rp-logs-ladder)
#   LADDER_REFS  read-only reference root        (default ~/rustmc/oracle-work)
#   LADDER_ROWS  the rows-harvested references   (default "a b c d e f g h i")
#   LADDER_RREV  the rows_rev-harvested ones     (default "rr-a rr-b")
#   LADDER_DIM   overworld|nether                (default overworld)
#   LADDER_SEED  the seed and reference prefix   (default 20260722)
#   LADDER_ANCHOR  reference(s) whose `rows` harvest order is not in doubt,
#                comma-separated, used to classify the unstamped ones from the
#                matrix rather than asserting them  (default i)
#
# LADDER_SEED EXISTS BECAUSE A SECOND SEED IS THE POINT, NOT A COPY OF IT.
# `ci/ow-r16-window-12345.sh` is deliberately a copy of `ci/ow-r16-window.sh`
# and says why in its own header: `ci/check-figures.py` reads published figures
# out of an artefact's MEASURED block, and one file holding two seeds' figures
# makes ARTEFACT-DRIFT unable to say which one drifted. That argument is about
# *artefacts*. This file is a **driver**: it publishes no figure, carries no
# MEASURED block, and every lesson encoded in it — the coverage refusal, the
# full ordered pair matrix, the byte-distinctness check on the two dumps —
# would have to be re-earned in a copy. Copying it would be copying the bugs
# it has already had.
#
# The nether needs `--ybox 0 127` on every call and the wrong `worldgen-dump`
# subcommand would silently dump an overworld, so the dimension sets the
# prefix, the ybox and the subcommand together rather than being three knobs
# an agent can set two of.
#
# Stages, so a resume costs only what it has not already done:
#   dumps    our two candidate dumps
#   pairs    every unordered reference pair, and every reference vs each dump
#   report   the charges, the floors and the exactness, computed from the above
#   resolve  the window's RESOLVING POWER IN BLOCKS, from the same .pos files
#
# `resolve` is separate from `report` because it answers a different question
# and a different reader wants it. `report` says what the level is; `resolve`
# says whether a level is a thing this window can have — the floor's spread
# across choices of candidate and reference subset, which is the smallest
# improvement that can be told from having picked a luckier pair of
# references, and the per-kind table that says which `vanilla -> ours` pairs
# are still adjudicable when the aggregate is not. A window whose aggregate
# floor is unusable is not a spent window; on the 20260722 one the aggregate
# was hopeless while `grass_block -> dirt` had a floor of 1.
# `all` runs it, so nobody has to remember to ask.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)" || exit 2

OUT=${LADDER_OUT:-$HOME/rp-logs-ladder}
REFS=${LADDER_REFS:-$HOME/rustmc/oracle-work}
ROWS=${LADDER_ROWS:-a b c d e f g h i}
RREV=${LADDER_RREV:-rr-a rr-b}
DIM=${LADDER_DIM:-overworld}
SEED=${LADDER_SEED:-20260722}
ANCHOR=${LADDER_ANCHOR:-i}
case "$DIM" in
    overworld) PFX=gen-wide-$SEED-r16-ow;     YB="";              SUB=full ;;
    nether)    PFX=gen-wide-$SEED-r16-nether; YB="--ybox 0 127";  SUB=full-nether ;;
    *) echo "unknown LADDER_DIM '$DIM' (want overworld|nether)" >&2; exit 2 ;;
esac
# The scratch root is keyed by seed as well as dimension. Without this a second
# seed's dumps land on top of the first seed's, the `[ -s .../r.1.1.mca ]`
# resume check says "dump present", and the run scores seed A's blocks against
# seed B's references while every log line names seed B.
[ "$SEED" = 20260722 ] || OUT=$OUT/$SEED
OUT=$OUT/$DIM
BBOX="48 48 80 80"
D=./target/release/worldgen-dump
G=./target/release/region-differ
STAGE=${1:-all}

# THE BINARY MUST NOT PREDATE THE TREE. 2026-08-10.
#
# This script runs `./target/release/worldgen-dump`; it does not build it.
# On 2026-08-10 an agent measured a ported feature with the binary from
# before the port and got **byte-identical dumps and a delta of exactly
# zero** — a result that reads as "the change did nothing" and is in fact
# "the change was never compiled".
#
# Every existing guard below sailed past it, and the reason is worth stating:
# coverage was full, both traversal digests were present and *distinct*, and
# the two dumps *did* differ from each other in bytes. All three checks
# compare the run against itself, and a stale binary is self-consistent. The
# only thing that could have caught it is a comparison against the clock,
# which is what this is.
#
# `find -newer` rather than a build, deliberately: building here would hide
# the ordering constraint the header describes and would silently rebuild a
# tree the caller had reason to leave alone. Refusing is the whole job.
if [ "$STAGE" = all ] || [ "$STAGE" = dumps ]; then
    for b in "$D" "$G"; do
        [ -x "$b" ] || { echo "  $b is missing — build it first. REFUSING." >&2; exit 2; }
        newer=$(find crates tools -type f \( -name '*.rs' -o -name '*.toml' \) \
                    -newer "$b" -print -quit 2>/dev/null)
        [ -z "$newer" ] || {
            echo "  $b is OLDER than $newer — it does not contain this tree." >&2
            echo "  A stale binary produces self-consistent dumps and a delta of" >&2
            echo "  zero, which reads as 'the change did nothing'. REFUSING." >&2
            echo "  Fix: cargo build --release -p worldgen-dump -p region-differ" >&2
            exit 2; }
    done
    echo "  binaries are newer than every source file in the tree"
fi

mkdir -p "$OUT/pos"
ALL="$ROWS $RREV"

# Coverage first, always. A reference directory that is present but short reads
# as agreement and is indistinguishable from a good one downstream.
if [ "$STAGE" = all ] || [ "$STAGE" = pairs ]; then
    WANT=""
    for r in $ALL; do
        d="$REFS/$PFX-$r"
        [ -d "$d" ] || { echo "no reference at $d — REFUSING" >&2; exit 2; }
        # shellcheck disable=SC2086
        got=$("$G" scan "$d" --bbox $BBOX $YB 2>/dev/null | grep -oP 'full=\K[0-9]+' || echo 0)
        [ -z "$WANT" ] && WANT="$got"
        ho=$(head -1 "$d/HARVEST-ORDER" 2>/dev/null || echo "gen_order=UNSTAMPED")
        printf '  coverage %-8s full=%-6s %s\n' "$r" "$got" "$ho"
        [ "$got" = "$WANT" ] && [ "$got" != 0 ] || {
            echo "  $r covers $got, expected $WANT — REFUSING" >&2; exit 2; }
    done
fi

# THE MATCHED DUMP ORDER IS READ OFF THE REFERENCES, NEVER ASSERTED.
#
# `zx` was a literal here until 2026-08-10, which was correct for the ensemble
# this script was written against and is exactly the shape of assertion ORACLE
# rule 29 exists to stop: the harvest order is a condition of the comparison,
# and a condition read out of a script rather than out of the artefact is a
# condition nobody set. `ci/gen-vanilla-region.sh` writes the answer into every
# reference it makes — `dump_order=zx` for `rows`, `zx_zrev` for `rows_rev` —
# so read it, and refuse rather than guess when it is absent or inconsistent.
#
# THE ANCHOR MUST BE STAMPED. The rest may be unstamped and are then
# classified from the matrix by `ci/order-level-report.py`, which is the only
# honest way to recover a reference that predates the stamp — and is how the
# 20260722 ensemble's `a`..`h` are handled, since only `i`, `rr-a` and `rr-b`
# were generated after `ci/gen-vanilla-region.sh` began writing the file.
# Refusing every unstamped reference outright would have retired that ensemble,
# which is not a correctness win: an order corroborated by a 4.6x gap in the
# matrix is *measured*, where one asserted in a script header is not.
MATCHED=""
UNSTAMPED=""
for r in $ROWS; do
    ho="$REFS/$PFX-$r/HARVEST-ORDER"
    if [ ! -s "$ho" ]; then UNSTAMPED="$UNSTAMPED $r"; continue; fi
    d=$(grep -m1 -oP '^dump_order=\K.*' "$ho")
    case "$d" in
        zx|zx_zrev) ;;
        *) echo "  $PFX-$r stamps dump_order='$d', which no RUSTMC_DECORATE_ORDER reproduces — REFUSING." >&2
           exit 2 ;;
    esac
    if [ -z "$MATCHED" ]; then MATCHED="$d"
    elif [ "$MATCHED" != "$d" ]; then
        echo "  LADDER_ROWS mixes harvest orders ($MATCHED and $d) — REFUSING." >&2
        echo "  The MATCH arm needs one order to match; put the others in LADDER_RREV." >&2
        exit 2
    fi
done
for a in $(echo "$ANCHOR" | tr ',' ' '); do
    case " $UNSTAMPED " in *" $a "*)
        echo "  the anchor '$a' carries no HARVEST-ORDER stamp — REFUSING." >&2
        echo "  Something in LADDER_ROWS has to state the order, or the matrix" >&2
        echo "  classification below has nothing to classify against and the" >&2
        echo "  MATCH arm is matching a guess (ORACLE rule 29)." >&2
        exit 2 ;;
    esac
done
[ -n "$MATCHED" ] || { echo "  nothing in LADDER_ROWS is stamped — REFUSING." >&2; exit 2; }
echo "  matched dump order read off the references' HARVEST-ORDER: $MATCHED"
[ -z "$UNSTAMPED" ] || echo "  UNSTAMPED (order recovered from the matrix, not asserted):$UNSTAMPED"

if [ "$STAGE" = all ] || [ "$STAGE" = dumps ]; then
    for order in default "$MATCHED"; do
        d=$OUT/dump-$order
        # THE RESUME KEY MUST INCLUDE EVERYTHING THE CHECKS BELOW READ, not
        # just the output. Keyed on the region file alone, a dump taken before
        # this script passed `RUSTMC_DECORATE_ORDER=default` explicitly counts
        # as present while its log carries no `decorate-order:` line at all —
        # so the traversal-digest check has nothing on one side and refuses a
        # run whose only defect is a stale cache. Found 2026-08-10 by that
        # refusal firing on the seed-12345 artefact, which is the guard working
        # and the cache key not.
        if [ ! -s "$d/region/r.1.1.mca" ] || \
           ! grep -q '^decorate-order:' "$OUT/dump-$order.log" 2>/dev/null; then
            rm -rf "$d"
            # `default` is passed EXPLICITLY rather than by leaving the var
            # unset. `worldgen-assembly` only prints its `decorate-order:`
            # line when the var is non-empty, so the unset arm produced no
            # traversal digest at all and the comparison below had nothing on
            # one side of it. Stating the condition is also the rule
            # (ORACLE 29): an order nobody set is an order nobody can check.
            RUSTMC_DECORATE_ORDER="$order" $D "$SUB" "$SEED" 16 "$d" 64 64 \
                > "$OUT/dump-$order.log" 2>&1
            echo "dump $order rc=$? $(grep -m1 '^decorate-order:' "$OUT/dump-$order.log" || true)"
        else
            echo "dump $order present"
        fi
    done
    # COVERAGE BEFORE COMPARISON. `diff -rq` on two directories that do not
    # exist also reports "they differ", so the byte check below would have
    # certified a failed build as proof the knob turned — it did exactly that
    # on the first run of this script, when the binaries had not been built.
    for order in default "$MATCHED"; do
        n=$(ls "$OUT/dump-$order/region"/*.mca 2>/dev/null | wc -l)
        [ "$n" -gt 0 ] || {
            echo "  dump-$order produced no region files — REFUSING." >&2
            tail -3 "$OUT/dump-$order.log" >&2 2>/dev/null
            exit 2; }
        # ANCHOR THE GREP TO THE TRAVERSAL LINE. Unanchored, `-m1 digest=`
        # matched `BIOME_PALETTE order=Forward len=66 digest=...` on line 1 —
        # a palette digest that is identical for every order by construction.
        # So this column printed the same value for two demonstrably different
        # traversals, and would have printed the same value for two identical
        # ones: a check that cannot fail. Found 2026-08-10 on the seed-12345
        # run, where both dumps reported 4462c36f462d1ad9 while the zx dump's
        # own traversal digest was 1afc539f71b91805.
        printf '  dump %-8s %s region files, traversal digest %s\n' "$order" "$n" \
            "$(grep -m1 -oP '^decorate-order: .*digest=\K[0-9a-f]+' "$OUT/dump-$order.log" 2>/dev/null || echo none)"
    done
    DGA=$(grep -m1 -oP '^decorate-order: .*digest=\K[0-9a-f]+' "$OUT/dump-default.log" 2>/dev/null || echo none)
    DGB=$(grep -m1 -oP '^decorate-order: .*digest=\K[0-9a-f]+' "$OUT/dump-$MATCHED.log" 2>/dev/null || echo none)
    if [ "$DGA" = none ] || [ "$DGB" = none ]; then
        echo "  a dump printed no traversal digest — REFUSING rather than assuming the knob turned." >&2
        exit 2
    fi
    if [ "$DGA" = "$DGB" ]; then
        echo "  both dumps walked the SAME traversal ($DGA) — REFUSING." >&2
        exit 2
    fi
    echo "  traversals are distinct: $DGA vs $DGB"
    # Distinct traversal digests, or the two dumps are one dump run twice and
    # every number below is a comparison of a thing with itself. `diag` writing
    # `default`'s bytes under a different digest is the standing proof that the
    # digest alone is not enough, so check the BYTES differ too.
    if diff -rq "$OUT/dump-default/region" "$OUT/dump-$MATCHED/region" >/dev/null 2>&1; then
        echo "  the two dumps are BYTE-IDENTICAL — the order knob did not turn. REFUSING." >&2
        exit 2
    fi
    echo "  the two dumps differ in bytes: the knob turned"
fi

if [ "$STAGE" = all ] || [ "$STAGE" = pairs ]; then
    # The FULL ORDERED matrix, not half of it. The position set of
    # `diff A B` and `diff B A` really is the same, and the first instinct is
    # to generate one and read it both ways — but `ci/window-resolving-power.py`
    # reads the *reference's* block out of field 4 to split a charge into
    # unanimous and split, and read backwards that field is the candidate's
    # block. The saving is half the wall clock and the cost is that one of the
    # three answers it prints becomes silently wrong.
    for r1 in $ALL; do
        for r2 in $ALL; do
            [ "$r1" = "$r2" ] && continue
            p="$OUT/pos/$r1-vs-$r2.pos"
            [ -s "$p" ] || { # shellcheck disable=SC2086
                $G diff "$REFS/$PFX-$r1" "$REFS/$PFX-$r2" --bbox $BBOX $YB --positions > "$p" 2>&1; }
        done
        echo "  pairs from $r1 done"
    done
    # Every reference against each of our two candidate dumps. Vanilla first.
    # `ours` is the x-major default the published level was taken with; `zx` is
    # the dump order that reproduces the references' own harvest order.
    for cand in ours "$MATCHED"; do
        src=$OUT/dump-default/region; [ "$cand" = "$MATCHED" ] && src=$OUT/dump-$MATCHED/region
        for r in $ALL; do
            p="$OUT/pos/$r-vs-$cand.pos"
            [ -s "$p" ] || { # shellcheck disable=SC2086
                $G diff "$REFS/$PFX-$r" "$src" --bbox $BBOX $YB --positions > "$p" 2>&1; }
        done
        echo "  $cand vs all references done"
    done
fi

if [ "$STAGE" = all ] || [ "$STAGE" = report ]; then
    python3 ci/order-level-report.py --pos "$OUT/pos" \
        --rows "$(echo $ROWS | tr ' ' ,)" --rrev "$(echo $RREV | tr ' ' ,)" \
        --denominator "${LADDER_DENOM:-107053056}" --seed "$SEED" \
        --known-rows "$ANCHOR" --matched-label "$MATCHED"
fi

if [ "$STAGE" = all ] || [ "$STAGE" = resolve ]; then
    # The MATCHED candidate, not the default one: the resolving power of an
    # instrument is a property of the instrument as it will actually be used,
    # and it will be used with the order matched.
    echo "=== RESOLVING POWER (matched dump order $MATCHED, harvest rows) ==="
    python3 ci/window-resolving-power.py "$OUT/pos" \
        --refs "$(echo $ROWS | tr ' ' ,)" --cand "$MATCHED" \
        --label "seed $SEED r=16 c(64,64), harvest rows, dump $MATCHED"
fi
echo "LADDER RETAKE DONE ($STAGE)"
