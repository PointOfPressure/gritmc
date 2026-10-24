#!/usr/bin/env bash
# t2-vanilla-opens-our-world.sh — hand a WORLD DIRECTORY our writer produced to
# a real vanilla server and make vanilla say what is in it.
#
# WHY THIS IS NOT ci/t2-vanilla-loads-ours.sh
#
# That gate proves vanilla can read a *region file* we wrote. It works by
# poking a block into a world vanilla itself generated, so everything around
# the region file — `level.dat`, `data/minecraft/*.dat`, `session.lock` — is
# vanilla's own. Until 2026-09-05 this server wrote none of those: `--world`
# produced `dimensions/minecraft/overworld/region/*.mca` inside a directory
# with no `level.dat` at all, which `LevelStorageSource` treats as a world that
# does not exist. So the claim "what lands on disk is a world vanilla can open"
# was true of the file and false of the world, and no gate could see the
# difference because no gate ever asked vanilla to open the *directory*.
#
# THE TWO ARMS, AND WHY EACH ONE CAN FAIL
#
#   A  THE SEED. `server.properties` is written with `level-seed=$DECOY`, a
#      different seed from the one `level-probe` put in
#      `data/minecraft/world_gen_settings.dat`. Vanilla ignores the property
#      for a world that already exists and uses the saved one. So asking the
#      running server `seed` and getting $SEED back is proof that vanilla read
#      a file we wrote — and getting $DECOY back is proof it did not, which is
#      exactly what happens if `level.dat` is missing or unreadable and vanilla
#      creates the world afresh.
#
#      **This arm is the whole gate and it is falsifiable by construction**:
#      run it with the seed decoyed and no `world_gen_settings.dat`, and it
#      must report $DECOY. `--negative-control` does that and requires the
#      failure, so a green here is a green that has been shown it can be red.
#
#   B  THE BRAND LIST. `PrimaryLevelData` keeps `ServerBrands` as the set of
#      brands that have ever run this world and adds its own on load. We write
#      `["rustmc"]`; after vanilla has opened and saved the world the list must
#      contain BOTH. A `level.dat` vanilla rewrote from scratch carries only
#      `vanilla`, and a `level.dat` vanilla never read carries only `rustmc`.
#      One assertion separates all three states.
#
# There is deliberately no arm asserting "vanilla started without an error".
# Vanilla starts perfectly happily on an empty directory — that is what makes
# this whole class of defect invisible — so "it booted" is the assertion that
# cannot fail.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SEED="${T2VO_SEED:-20260722}"
DECOY="${T2VO_DECOY_SEED:-1234567890}"
PORT="${T2VO_PORT:-25939}"
NEGATIVE=0
[ "${1:-}" = "--negative-control" ] && NEGATIVE=1

WORK="$ROOT/oracle-work/t2-vanilla-opens-our-world"
RUN="$WORK/run"
WORLD="$RUN/world"
PROBE="$ROOT/target/debug/level-probe"

fail() { echo "T2-VANILLA-OPENS-OUR-WORLD RED — $*"; exit 1; }
cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$RUN" >/dev/null 2>&1 || true; }
trap cleanup EXIT

command -v cargo >/dev/null 2>&1 || {
    echo "no cargo on PATH; over ssh prefix with: export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >&2
    exit 2
}
cargo build -q -p level-probe || fail "could not build level-probe"

rm -rf "$RUN"
mkdir -p "$WORLD"
"$PROBE" write "$WORLD" "$SEED" world || fail "level-probe could not write the world"

for f in level.dat data/minecraft/world_gen_settings.dat data/minecraft/weather.dat; do
    [ -f "$WORLD/$f" ] || fail "level-probe did not write $f"
done

if [ "$NEGATIVE" = 1 ]; then
    # The control: take away the file arm A depends on. Vanilla then has no
    # saved seed and falls back to `level-seed`, so the arm must report the
    # decoy. `level.dat` goes too — vanilla refuses to read a world whose
    # dimension settings it cannot find and would otherwise error rather than
    # regenerate, which is a third outcome and not the one being controlled
    # for.
    echo "== negative control: removing level.dat and world_gen_settings.dat"
    rm -f "$WORLD/level.dat" "$WORLD/data/minecraft/world_gen_settings.dat"
fi

echo "== booting vanilla on our world directory (level-seed=$DECOY, saved seed=$SEED)"
"$ROOT/ci/vanilla-server.sh" start "$RUN" "$PORT" "minecraft:normal" "$DECOY" \
    || fail "vanilla did not start (see $RUN/server.log)"

# --- arm A: whose seed is the running world on? -----------------------------
#
# `SeedCommand` answers `commands.seed.success` with the seed in square
# brackets, which is why the pattern is anchored on them rather than on the
# bare number: the decoy and the real seed are both plain integers and a loose
# grep would match one inside a timestamp.
"$ROOT/ci/vanilla-server.sh" console "$RUN" "seed" || fail "could not send the seed command"
# The wait is registered in `ci/checks/liveness-proofs.toml`: its predicate has
# to reject a boot log that has not answered yet, and a vanilla log that never
# answers is exactly what a dead console fifo produces. Twenty seconds, then a
# loud refusal — never a fall-through to a reading taken from an empty grep.
SEEN=0
for _ in $(seq 1 40); do
    grep -q -P 'Seed: \[-?[0-9]+\]' "$RUN/server.log" 2>/dev/null && { SEEN=1; break; }
    sleep 0.5
done
[ "$SEEN" = 1 ] || fail "vanilla never answered the seed command in 20s (see $RUN/server.log)"
got=$(grep -oP 'Seed: \[\K-?[0-9]+' "$RUN/server.log" | tail -1)
echo "   vanilla reports seed $got"

if [ "$NEGATIVE" = 1 ]; then
    if [ "$got" = "$SEED" ]; then
        fail "NEGATIVE CONTROL FAILED — vanilla reported our seed $SEED with \
world_gen_settings.dat deleted, so arm A is not reading the file it claims to"
    fi
    echo "T2-VANILLA-OPENS-OUR-WORLD CONTROL GREEN — without our file vanilla \
used the decoy $got, so arm A can go red"
    exit 0
fi

[ "$got" = "$SEED" ] || fail "arm A: vanilla is on seed $got, not the $SEED we \
wrote into data/minecraft/world_gen_settings.dat — it did not read our world"

# --- arm B: did vanilla load our level.dat, or write a new one? -------------
"$ROOT/ci/vanilla-server.sh" stop "$RUN" || true
brands=$("$PROBE" dump "$WORLD" | grep -oP '^ServerBrands=\K.*' || true)
[ -n "$brands" ] || fail "arm B: could not read ServerBrands back out of level.dat"
echo "   ServerBrands after vanilla ran: $brands"
case "$brands" in
    *rustmc*) ;;
    *) fail "arm B: ServerBrands is '$brands' — our brand is gone, so vanilla \
rewrote level.dat from scratch rather than loading ours" ;;
esac
case "$brands" in
    *vanilla*) ;;
    *) fail "arm B: ServerBrands is '$brands' — vanilla's brand is absent, so \
vanilla never saved this level.dat and arm A may be reading a stale file" ;;
esac

echo "T2-VANILLA-OPENS-OUR-WORLD GREEN — seed $got read from our \
world_gen_settings.dat; ServerBrands '$brands' carries both"
