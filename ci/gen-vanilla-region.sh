#!/usr/bin/env bash
# gen-vanilla-region.sh <seed> <radius> <outdir> [port] [cx] [cz] [dimension]
# Drive the REAL vanilla server to generate the chunk square
# (cx-radius..cx+radius)² (chunk coords, default center 64,64) with the
# given seed, then harvest its region files for <dimension> (overworld |
# nether | end, default overworld) into <outdir>. This produces the
# reference side of every G3 comparison (ORACLE.md §3).
#
# The dimension argument was added 2026-07-29 — it did NOT exist before, and
# WORKLIST claimed it did. `forceload add` acts on `source.getLevel()`
# (checked in the bytecode: `CommandSourceStack.getLevel().dimension()`), and
# the console's source level is always the overworld, so a non-overworld
# square has to be forceloaded through `execute in <dim> run`. Without that
# prefix the script silently generates the OVERWORLD and then polls an empty
# nether region directory until it times out.
#
# Determinism hardening (found live 2026-07-22, first gate run was 99.9594%
# not 100%): forceloaded chunks TICK — random ticks, fluid flow, weather —
# so harvest-time state depends on how long chunks stayed loaded. Fixes:
# mutation gamerules off immediately after boot, generate AWAY from spawn
# (spawn chunks tick from boot before gamerules land), and a fixed settle
# window so scheduled ticks (fluids) reach the same fixed point both runs.
#
# 26.x layout note: vanilla saves the overworld under
# world/dimensions/minecraft/overworld/region (NOT the classic world/region).
#
# Determinism: fixed forceload command order.
#
# **THE CLAIM THAT USED TO END THIS PARAGRAPH WAS FALSE — corrected 2026-08-16.**
# It read "the t2-regiondiff gate proves two identical invocations produce
# bit-identical inner chunks", and the comment further down said the same of the
# square path: "that order is deterministic for a given extent — two runs at one
# centre are bit-identical". Eight harvests of that gate's own window produce
# **five distinct worlds**, 0..536 blocks apart of 884,736; only 4 of the 28
# pairs are zero. The gate passed because its fixture had stopped being
# regenerated, not because the property held. `docs/DETERMINISM.md` §36.
#
# **It is true under one condition, and this script does not set it for you.**
# The cause is concurrency, not decoration order: `ChunkMap` dispatches every
# chunk status upgrade onto `Util.backgroundExecutor()`, sized
# `availableProcessors() - 1`, so which chunk reaches FEATURES first is a race
# and features write one chunk out. Set
#
#     VANILLA_JVM_OPTS=-Dmax.bg.threads=1
#
# and MOST of the variance goes. That is a system property the unmodified
# server reads for itself; nothing is patched. `GEN_ORDER=perchunk` is NOT a
# substitute: it still leaves 0..79 at N=4. `docs/DETERMINISM.md` §37.
#
# **AND IT IS TRUE ONLY AT A SMALL EXTENT — corrected 2026-08-17.** The
# paragraph used to end "and the harvest is bit-reproducible", full stop. The
# clamp is exact over the INTERIOR OF A SMALL HARVEST: four clamped r=2 runs at
# each of two seeds agree on all six pairs over the inner 9 chunks. Widen the
# box to 25 chunks and a second world appears in the boundary ring (187 blocks
# at 20260722, 190 at 12345); harvest 1,089 chunks instead and nothing agrees
# anywhere, interior included — 15 of 15 pairs non-zero, 6,829..9,738 of
# 107,053,056, which is 45% of the unclamped floor and not zero.
# `docs/DETERMINISM.md` §38 and §39.
#
# So: **take new references under the clamp** — at a small extent it really is
# reproducible, and at a large one it still buys 45% of the floor. But a clamped
# WIDE reference is not reproducible, no stamp on it says so, and the extent is
# as much a condition of that property as the seed is.
# Deltas against a fixed reference set are unaffected either way.
# Forceload cap is 256 chunks **per command** → radius ≤ 7 in one shot. The row
# modes below issue one command per row, `2R+1` chunks each, so they clear that
# cap for any radius up to 127 and the total forceloaded count is bounded only
# by the server's memory. Hence: the square path keeps the r<=7 assertion, the
# row paths do not.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SEED="$1"; R="$2"; OUT="$3"; PORT="${4:-25701}"; CX="${5:-64}"; CZ="${6:-64}"
DIM="${7:-overworld}"
case "${GEN_ORDER:-square}" in
    rows | rows_rev | perchunk) : ;;
    *) [ "$R" -le 7 ] || {
        echo "radius >7 exceeds one forceload: use GEN_ORDER=rows or rows_rev"
        exit 2
    } ;;
esac

# The save directory name and the dimension key are NOT the same string, and
# the overworld is the odd one out in both: it saves under `overworld` and its
# key is `minecraft:overworld`, while the other two save under `the_nether` /
# `the_end`.
case "$DIM" in
    overworld) DIM_DIR="overworld";  DIM_KEY="minecraft:overworld" ;;
    nether)    DIM_DIR="the_nether"; DIM_KEY="minecraft:the_nether" ;;
    end)       DIM_DIR="the_end";    DIM_KEY="minecraft:the_end" ;;
    *) echo "unknown dimension '$DIM' (want overworld|nether|end)"; exit 2 ;;
esac
# `execute in` for every dimension including the overworld: it is a no-op
# there and it means the command that is exercised in the gate is the command
# that runs for the nether.
EXEC_IN="execute in $DIM_KEY run "

DIR="$ROOT/oracle-work/gen-$SEED-$PORT"
DIFFER="$ROOT/target/debug/region-differ"
# `cargo` is not on the PATH of a non-login SSH shell, and this script is
# meant to run on the Threadripper. Without the guard the build fails with
# "cargo: command not found", the script carries on, and the *only* symptom is
# that the progress poll below always reads zero full chunks — so it looks
# like a slow generation rather than a broken one, and you wait ten minutes to
# find out. That happened twice.
if command -v cargo >/dev/null 2>&1; then
    cargo build -q -p region-differ
elif [ ! -x "$DIFFER" ]; then
    echo "no cargo on PATH and no prebuilt $DIFFER" >&2
    echo "over ssh, prefix with: export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >&2
    exit 2
fi

cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$DIR" >/dev/null 2>&1 || true; }
trap cleanup EXIT

# # THE SAFETY HERE IS ONE CHARACTER DEEP. 2026-08-18.
#
# `$OUT` is a reference directory, and in a gate directory those are SYMLINKS
# into the one canonical `~/rustmc/oracle-work` — that is how the setup ritual
# in `docs/AGENT-BRIEF.md` wires every tree, so 2.3 GB of vanilla references
# exists once rather than 146 times.
#
# The line below is SAFE, and it is worth writing down why, because the reason
# is not visible in it. Measured 2026-08-18:
#
#     rm -rf link      -> removes the LINK. The target survives.
#     rm -rf link/     -> removes the TARGET'S CONTENTS.
#     rm -rf link/*    -> the same.
#
# There is no trailing slash here, so a caller that passes a symlink loses its
# own link and nothing else. An earlier draft of this comment claimed the
# opposite — that one run would delete the shared references for every agent on
# the box — and `ci/shared-refs-falsify.sh` case 0 refuted it before it was
# committed. That is recorded rather than quietly corrected because the wrong
# version is the intuitive one.
#
# **This is the same one-character asymmetry that HAS fired here, in rsync.**
# `ci/t0-remote.sh`'s header is written around an exclude pattern that ended in
# a slash: it replaced the remote's real `oracle-work` with a self-pointing
# symlink and cost 200 MB over the wire plus a re-extract of the structure
# templates. Same links, same slash, different tool.
#
# So the guard is a tripwire on a class that has already cost this project a
# day, not a fix for a bug in this line. It also closes a real if milder thing:
# without it a caller pointed at a link silently converts that gate directory
# from *shares the canonical reference* to *has its own private harvest*, and
# nothing anywhere would say so. `ci/t2-regiondiff.sh` passes
# `oracle-work/t2-regiondiff/{a,b}`, which is a symlink to the shared copy in
# all 146 gate directories on this box — one harvest taken 2026-07-22 at 19:55,
# byte-identical from every direction, so what looks like 146 independent
# vanilla-vs-vanilla pairs is one measurement counted 146 times.
#
# Regenerating a reference you meant to regenerate still works: remove the link
# first, deliberately. That is the shape `ci/handoff.sh` stage 1 already uses
# when it takes a gate directory's `oracle-work` private ("removing the LINK
# only").
for _p in "$DIR" "$OUT"; do
    if [ -L "$_p" ]; then
        echo "gen-vanilla-region REFUSED — $_p is a symlink to $(readlink "$_p")" >&2
        echo "    Removing it would un-share this tree's reference silently, and one" >&2
        echo "    trailing slash away is the rsync failure that cost 200 MB in July." >&2
        echo "    If you meant to regenerate it, remove the link first:  rm '$_p'" >&2
        exit 2
    fi
done
rm -rf "$DIR" "$OUT"
"$ROOT/ci/vanilla-server.sh" start "$DIR" "$PORT" "minecraft:normal" "$SEED"

# Freeze every block-mutating live system before our chunks ever load.
# `tick freeze` is the load-bearing one: it stops the game loop entirely
# (fluids, random ticks — everything except chunk gen, which runs off-tick),
# so the harvest is the PURE generator output. Round 2 evidence: without it,
# 27 blocks of mid-flow fluid drift survived a 30s settle. The gamerules
# stay as belt-and-braces for any future script mode that unfreezes.
#
# VERIFIED, and refined, 2026-08-02 — read this before blaming the harness for
# any fluid. `tick freeze` really does work here: `oracle-work/vanilla-r7` was
# generated with it (its log says "The game is frozen" at 02:32:46, thirty
# seconds before the forceload), and the saved chunks still hold **13,852
# `cave_air`, 426 `water` and 27 `lava` ticks queued at `t=0`** — ticks that
# were DUE and that a single turn of the loop would have consumed. The loop
# never turned.
#
# But "the game did not tick" is NOT "nothing but generation ran". The last
# step of generation is `LevelChunk.postProcessGeneration`, called from
# `ChunkMap` on promotion to FULL, and for every position worldgen marked with
# `markPosForPostProcessing` it runs **`FluidState.tick` synchronously**, then
# `BlockState.tick` if the block is a `LiquidBlock` and
# `Block.updateFromNeighbourShapes` + `setBlock(276)` if it is not. That is off
# the game loop, so `tick freeze` cannot and should not stop it. Proof it ran
# in the reference: the saved `PostProcessing` lists are present but **empty**,
# and clearing them is the last thing that method does.
#
# So flowing water in a harvested reference is generation output, not
# simulation drift, and it must be counted. `ci/chunk-ticks-probe.py` is the
# instrument that tells the two apart — use it before arguing the reference is
# contaminated. Full evidence chain in `docs/ORACLE.md`.
# CORRECTED 2026-07-28: every one of these gamerules was a no-op. 26.2's rule
# ids are snake_case, and several are not renamings at all — `doMobSpawning`
# is `spawn_mobs`, `doWeatherCycle` is `advance_weather`, and `doFireTick` has
# become the integer `fire_spread_radius_around_player`. The console answered
# "Incorrect argument for command" five times per run and nothing read it, so
# `tick freeze` has been carrying this alone, exactly as the comment above
# claims it does. Ids read out of the constant pool of
# `net/minecraft/world/level/gamerules/GameRules.class`.
for rule in "tick freeze" "gamerule random_tick_speed 0" \
            "gamerule fire_spread_radius_around_player 0" "gamerule spawn_mobs false" \
            "gamerule mob_griefing false" "gamerule advance_weather false" \
            "gamerule water_source_conversion false" "gamerule lava_source_conversion false" \
            "weather clear 1000000"; do
    "$ROOT/ci/vanilla-server.sh" console "$DIR" "$rule"
done

B1X=$(( (CX - R) * 16 )); B1Z=$(( (CZ - R) * 16 ))
B2X=$(( (CX + R) * 16 + 15 )); B2Z=$(( (CZ + R) * 16 + 15 ))
# One `forceload add` over the whole square leaves the order chunks reach
# FEATURES to the server's scheduler. **That order is NOT deterministic, and
# this comment claimed it was until 2026-08-16** — "two runs at one centre are
# bit-identical" is refuted by eight runs at one centre producing five distinct
# worlds (`docs/DETERMINISM.md` §36). It is deterministic only under
# `VANILLA_JVM_OPTS=-Dmax.bg.threads=1`, per the header. It is also not the same
# between extents, and features write one chunk out, so wherever two chunks'
# features touch the same block the answer moves. Measured: two vanilla worlds
# of seed 20260722, centres (64,64) and (62,62), differ by 4,885 blocks over
# the window they share. See docs/BENCH.md.
#
# `GEN_ORDER=rows` forceloads one chunk row at a time instead, to test whether
# a driven order makes the oracle reproducible across extents rather than only
# within one.
#
# It is also the only **positive control** this project has for an order
# measurement, and it has been used as one. Measured 2026-07-30, seed 20260722,
# centre (64,64), r=7, window (57,57)..(71,71), y 0..127: rows against the
# square moves vanilla's **nether by 3,621 blocks** — basalt/blackstone against
# netherrack, in the `basalt_deltas` corner, where a `count_on_every_layer`
# placement finds its ground y by reading a neighbour that may not have
# decorated yet — and moves vanilla's **End by 0**. So a zero from this knob is
# a real zero rather than a knob that never turned, and any claim that a
# dimension is order-invariant should quote a rows-vs-square figure beside it.
#
# `GEN_ORDER=rows_rev` is the same knob walked **z descending**, and it exists
# because `rows` is not a general order control: it only ever varies *which*
# ascending row goes first. Both perturbations above walk z ascending, so
# neither can reach a coupling whose only trigger is a chunk's **z+1**
# neighbour decorating first — which is exactly the shared property of the two
# decoration orders that move our End (`reverse` and `zx_zrev`) and of no
# other. A zero from `rows` in a dimension whose only order-sensitive feature
# couples along +z is a knob that turned in the wrong plane.
REGION="$DIR/world/dimensions/minecraft/$DIM_DIR/region"

# `GEN_ORDER=perchunk` is the strongest ordering control that is still pure
# configuration: one `forceload add` per chunk, in a fixed row-major order,
# and the next command is not sent until the previous chunk has actually
# reached FULL on disk. Nothing about the jar changes; the server is simply
# never asked for two chunks at once.
#
# It is the last configuration candidate for making the reference
# reproducible, and it is slow on purpose — one `save-all flush` and one scan
# per chunk. Measured cost and measured result are in docs/DETERMINISM.md.
if [ "${GEN_ORDER:-square}" = "perchunk" ]; then
    for cz in $(seq $((CZ - R)) $((CZ + R))); do
        for cx in $(seq $((CX - R)) $((CX + R))); do
            "$ROOT/ci/vanilla-server.sh" console "$DIR" \
                "${EXEC_IN}forceload add $(( cx * 16 )) $(( cz * 16 )) $(( cx * 16 + 15 )) $(( cz * 16 + 15 ))"
            for _ in $(seq 1 60); do
                "$ROOT/ci/vanilla-server.sh" console "$DIR" "save-all flush"
                sleep 0.5
                g=$("$DIFFER" scan "$REGION" --bbox "$cx" "$cz" "$cx" "$cz" 2>/dev/null \
                    | grep -oP 'full=\K[0-9]+' || echo 0)
                [ "$g" -ge 1 ] && break
            done
        done
    done
elif [ "${GEN_ORDER:-square}" = "rows" ] || [ "${GEN_ORDER:-square}" = "rows_rev" ]; then
    ROWS=$(seq $((CZ - R)) $((CZ + R)))
    [ "${GEN_ORDER}" = "rows_rev" ] && ROWS=$(seq $((CZ + R)) -1 $((CZ - R)))
    for cz in $ROWS; do
        "$ROOT/ci/vanilla-server.sh" console "$DIR" \
            "${EXEC_IN}forceload add $B1X $(( cz * 16 )) $B2X $(( cz * 16 + 15 ))"
        sleep 1
    done
else
    "$ROOT/ci/vanilla-server.sh" console "$DIR" "${EXEC_IN}forceload add $B1X $B1Z $B2X $B2Z"
fi
WANT=$(( (2 * R + 1) * (2 * R + 1) ))
# The completion poll's iteration count is configurable, and the default is
# unchanged at 150 (~5 minutes of `save-all flush` + `sleep 2`) so no existing
# caller's timeout moves. It has to be configurable because
# `VANILLA_JVM_OPTS=-Dmax.bg.threads=1` — the clamp the header now tells you to
# take references under — puts all 1,089 chunks of an r=16 window through ONE
# background worker. Unclamped that harvest is ~57 s wall on a 24-thread box;
# clamped it is minutes, and the old fixed cap turned a slow-but-healthy
# harvest into `generation timed out`, which reads as a broken window.
#
# RAISING THIS DOES NOT WEAKEN THE FAILURE. The loop still exits non-zero with
# the `got/WANT` count it reached, so a genuinely wedged server is still caught
# and still says how far it got; only the patience changes. Do not reach for it
# to paper over a harvest that is not progressing — compare two successive
# `got` readings first.
for i in $(seq 1 "${GEN_POLL_ITERS:-150}"); do
    "$ROOT/ci/vanilla-server.sh" console "$DIR" "save-all flush"
    sleep 2
    got=$("$DIFFER" scan "$REGION" --bbox $((CX - R)) $((CZ - R)) $((CX + R)) $((CZ + R)) 2>/dev/null | grep -oP 'full=\K[0-9]+' || echo 0)
    [ "$got" -ge "$WANT" ] && break
    [ "$i" = "${GEN_POLL_ITERS:-150}" ] && { echo "generation timed out: $got/$WANT full"; exit 1; }
done

"$ROOT/ci/vanilla-server.sh" console "$DIR" "${EXEC_IN}forceload remove all"
"$ROOT/ci/vanilla-server.sh" console "$DIR" "save-all flush"
sleep 2
"$ROOT/ci/vanilla-server.sh" stop "$DIR"

mkdir -p "$OUT"
cp "$REGION"/*.mca "$OUT/"
# The entity storage beside it — `entities/r.X.Z.mca`, separate from the block
# regions since 1.17. Worldgen writes entities too: the End's crystals
# (`EndSpikeFeature`), an end city ship's shulkers and item frame. Kept in a
# subdirectory so every `region-differ` call on `$OUT` reads exactly what it
# read before. Added 2026-10-04 on `end-parity`.
ENTITIES="$(dirname "$REGION")/entities"
if ls "$ENTITIES"/*.mca >/dev/null 2>&1; then
    mkdir -p "$OUT/entities"
    cp "$ENTITIES"/*.mca "$OUT/entities/"
fi
# STAMP THE HARVEST ORDER. 2026-08-10.
#
# The order chunks are requested in changes the world vanilla writes — §11
# measured 3,345 blocks between `rows` and `perchunk` over 121 chunks — so it
# is a condition of the reference in the same way the seed is, and until now it
# was recorded nowhere. Our own dump has the matching knob
# (`RUSTMC_DECORATE_ORDER`) and a comparison that does not match the two is
# measuring the mismatch: on the r=16 window that was 11,820 of a 14,270
# admissible charge (§15).
#
# `region-differ` filters on the `.mca` extension, so a plain file here is
# invisible to every existing reader.
#
# STAMP THE CLAMP TOO. 2026-08-16, §37.
#
# The harvest order is not the only condition of a reference that used to be
# recorded nowhere. `-Dmax.bg.threads=1` decides **how wide the distribution is
# that this artefact is one draw from** — eight unclamped harvests of one window
# produced five distinct worlds, while a clamped SMALL harvest is a point and a
# clamped WIDE one is not (§39) — and that is a bigger fact about a reference
# than which row went first. Read it together with the radius this script was
# called with: neither the stamp nor the clamp means anything without it. A reader who
# cannot tell a clamped reference from an unclamped one cannot tell a floor
# that means "our defects" from one that means "vanilla's scheduler".
#
# It is derived from `VANILLA_JVM_OPTS` as this run actually saw it, not from a
# flag the caller passes separately, because a separate flag is a thing that can
# disagree with the JVM that ran. `unset` is written out in full rather than
# left blank: an absent line and an empty value are indistinguishable to a
# `grep -oP`, and "no clamp" is a positive fact about this artefact.
{
    echo "gen_order=${GEN_ORDER:-square}"
    echo "seed=$SEED radius=$R centre=$CX,$CZ dimension=$DIM"
    echo "generated=$(date -Iseconds)"
    echo "jvm_opts=${VANILLA_JVM_OPTS:-unset}"
    echo "bg_threads=$(printf '%s' "${VANILLA_JVM_OPTS:-}" \
        | grep -oP '(?<=-Dmax\.bg\.threads=)[0-9]+' || echo unset)"
    # The dump order that reproduces this harvest's chunk sequence. `rows`
    # walks z ascending with x ascending inside a row, which is `zx`;
    # `rows_rev` walks z descending, which is `zx_zrev`. `square` and
    # `perchunk` have no single-name equivalent and say so rather than
    # guessing.
    case "${GEN_ORDER:-square}" in
        rows)     echo "dump_order=zx" ;;
        rows_rev) echo "dump_order=zx_zrev" ;;
        *)        echo "dump_order=unknown" ;;
    esac
    # STAMP THE JVM CONFIGURATION TOO. 2026-08-18.
    #
    # `gen_order` was stamped because the chunk request order is a condition of
    # the reference. `-Dmax.bg.threads` is a condition of it in exactly the
    # same sense and a larger one: it sets the width of the pool `ChunkMap`
    # dispatches status upgrades onto, and that width is what makes two
    # harvests of one seed and window disagree at all (§1). A reference
    # harvested under `-Dmax.bg.threads=1` and one harvested under the default
    # 23 are not two samples of one distribution — they are samples of two —
    # and until this line existed nothing on disk could tell them apart.
    #
    # Empty by default, so every reference generated before today reads
    # `jvm_opts=` and means "the default pool", which is what they all were.
    echo "jvm_opts=${VANILLA_JVM_OPTS:-}"
} > "$OUT/HARVEST-ORDER"
echo "GEN OK seed=$SEED r=$R dim=$DIM -> $OUT ($(ls "$OUT"/*.mca | wc -l) region files, $(head -1 "$OUT/HARVEST-ORDER"))"
