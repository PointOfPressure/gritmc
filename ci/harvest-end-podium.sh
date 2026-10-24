#!/usr/bin/env bash
# harvest-end-podium.sh <seed> <outdir> [port] — a vanilla End WITH the exit
# portal the dragon fight builds.
#
# `EnderDragonFight.tick` only scans the arena (and so only runs
# `spawnExitPortal(false)` and spawns the dragon) while `dragonEvent` has a
# player, so no chunk harvest holds a podium. This boots vanilla, generates
# the arena (chunks -8..8 of the End), joins one offline client
# (`oracle-runner join`), sends it to the End, waits for the fight to
# initialise, and harvests the End's regions and entities into <outdir>.
#
# KILL_DRAGON=1 also kills the dragon once the fight has scanned, waits out
# `EnderDragon.tickDeath` (200 ticks), and so harvests what
# `EnderDragonFight.setDragonKilled` builds: the ACTIVE exit portal, the
# dragon egg and the first return gateway (`spawnNewGateway`). The fight's
# saved data (the remaining gateway order) is copied from every `data/`
# directory of the world into <outdir>/data/. With GATEWAY set as well
# (see below) the player is walked into that gateway.
#
# Not frozen (`tick freeze` would stop the fight's tick), so the only
# gamerules are the ones that keep the arena still.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SEED="$1"; OUT="$2"; PORT="${3:-25981}"
DIR="$ROOT/oracle-work/podium-$SEED-$PORT"
RUNNER="$ROOT/target/release/oracle-runner"
DIFFER="$ROOT/target/release/region-differ"
for b in "$RUNNER" "$DIFFER"; do
    [ -x "$b" ] || { echo "missing $b: cargo build --release -p oracle-runner -p region-differ" >&2; exit 2; }
done
cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$DIR" >/dev/null 2>&1 || true; }
trap cleanup EXIT
rm -rf "$DIR" "$OUT"
"$ROOT/ci/vanilla-server.sh" start "$DIR" "$PORT" "minecraft:normal" "$SEED"
for rule in "gamerule random_tick_speed 0" "gamerule fire_spread_radius_around_player 0" \
            "gamerule spawn_mobs false" "gamerule mob_griefing false" \
            "gamerule advance_weather false"; do
    "$ROOT/ci/vanilla-server.sh" console "$DIR" "$rule"
done
"$ROOT/ci/vanilla-server.sh" console "$DIR" "execute in minecraft:the_end run forceload add -128 -128 127 127"
REGION="$DIR/world/dimensions/minecraft/the_end/region"
for i in $(seq 1 120); do
    "$ROOT/ci/vanilla-server.sh" console "$DIR" "save-all flush"
    sleep 2
    got=$("$DIFFER" scan "$REGION" --bbox -8 -8 7 7 2>/dev/null | grep -oP 'full=\K[0-9]+' || echo 0)
    [ "$got" -ge 256 ] && break
    [ "$i" = 120 ] && { echo "arena generation timed out: $got/256" >&2; exit 1; }
done
"$RUNNER" join "127.0.0.1:$PORT" podium --stay-secs "${STAY_SECS:-150}" > "$OUT.join.log" 2>&1 &
JOIN=$!
for i in $(seq 1 30); do
    grep -q 'podium joined the game' "$DIR/logs/latest.log" 2>/dev/null && break
    sleep 1
    [ "$i" = 30 ] && { echo "the client never joined" >&2; exit 1; }
done
"$ROOT/ci/vanilla-server.sh" console "$DIR" "execute in minecraft:the_end run tp podium 0 100 0"
# The fight scans players every 20 ticks and then the arena; 20 s is ten
# times what it needs. The proof is in the log, not in the wait.
for i in $(seq 1 40); do
    grep -q 'Found that the dragon has not yet been killed' "$DIR/logs/latest.log" 2>/dev/null && break
    sleep 1
    [ "$i" = 40 ] && { echo "the dragon fight never scanned" >&2; tail -20 "$DIR/logs/latest.log" >&2; exit 1; }
done
sleep 5
if [ "${KILL_DRAGON:-0}" = 1 ]; then
    "$ROOT/ci/vanilla-server.sh" console "$DIR" "kill @e[type=minecraft:ender_dragon]"
    # 200 ticks of death animation; 30 s is three times that at 20 TPS.
    sleep 30
    # GATEWAY="x y z" (the gateway the kill built, from
    # `gateway_ring_position(gateway_spawn_order(seed)[0])`) then walks the
    # player into it, so `TheEndGatewayBlockEntity.getPortalPosition` runs
    # `findOrCreateValidTeleportPos` and builds the exit gateway ~1024
    # blocks out. Wrong coordinates leave no exit: the check sees that.
    if [ -n "${GATEWAY:-}" ]; then
        read -r gx gy gz <<<"$GATEWAY"
        # The block's centre: x + 0.5 (for x = -57 that is -56.5, not -57.5).
        cx=$(awk -v v="$gx" 'BEGIN{printf "%.1f", v + 0.5}')
        cz=$(awk -v v="$gz" 'BEGIN{printf "%.1f", v + 0.5}')
        "$ROOT/ci/vanilla-server.sh" console "$DIR" "execute in minecraft:the_end run tp podium $cx $gy $cz"
        sleep 20
    fi
fi
"$ROOT/ci/vanilla-server.sh" console "$DIR" "save-all flush"
sleep 2
kill "$JOIN" 2>/dev/null || true
"$ROOT/ci/vanilla-server.sh" stop "$DIR"
mkdir -p "$OUT/entities"
cp "$REGION"/*.mca "$OUT/"
cp "$(dirname "$REGION")/entities/"*.mca "$OUT/entities/" 2>/dev/null || true
if [ "${KILL_DRAGON:-0}" = 1 ]; then
    find "$DIR/world" -path '*/data/*' -type f | while read -r f; do
        rel="${f#"$DIR/world/"}"
        mkdir -p "$OUT/data/$(dirname "$rel")"
        cp "$f" "$OUT/data/$rel"
    done
fi
{
    echo "gen_order=forceload-square-then-player"
    echo "seed=$SEED dimension=end arena=-8..7 player=podium tp=(0,100,0) kill_dragon=${KILL_DRAGON:-0} gateway=${GATEWAY:-none}"
    echo "generated=$(date -Is)"
} > "$OUT/HARVEST-ORDER"
echo "PODIUM OK seed=$SEED -> $OUT"
