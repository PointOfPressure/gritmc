#!/usr/bin/env bash
# t2-gameplay-survival.sh — the gameplay oracle on a SURVIVAL substrate.
#
#     ./ci/t2-gameplay-survival.sh
#
# Same instrument as `ci/t2-gameplay.sh` — real vanilla as the reference, this
# server as the subject, the scenarios' own projections — over
# `ci/checks/survival` instead of `ci/checks/gameplay`, with both servers in
# survival instead of creative.
#
# # WHY THIS IS A SECOND PASS AND NOT A FLAG ON THE FIRST
#
# The corpus substrate is `gamemode=creative`, and that is not a preference:
# it is the only way a harness player can hold a block, because `Step::Hold`
# sends `set_creative_mode_slot` and
# `ServerGamePacketListenerImpl.handleSetCreativeModeSlot` refuses the whole
# packet when `!player.isCreative()` — silently, no error, nothing in the log.
# So a survival scenario cannot `hold`, cannot `place`, and cannot build a rig
# the way all 115 existing checks do. Flipping the corpus would not measure
# more; it would break every scenario in it.
#
# What creative costs is that **four mechanics cannot be graded at all**, and
# that is a different thing from being unported. `ci/substrate-gate-audit.py`
# reads the gates out of the pinned jar:
#
#   Player.hurtServer                            15 -> 32    damage, health
#   Player.causeFoodExhaustion                    4 -> 10    hunger
#   ServerPlayerGameMode.handleBlockBreakAction 178 -> 194    mining progress
#   ServerPlayerGameMode.destroyBlock           193 -> 200    block drops
#
# Every one of those is an early return ABOVE the send. No work on this server
# could ever have produced the packets below them, which is why `damage_event`
# and `set_health` appear zero times in 87 recordings and why they sat in the
# roadmap as unported work items until 2026-08-18.
#
# So this pass exists to open those four gates, over scenarios written not to
# need a `hold`. It is a sibling of the creative corpus, not a replacement:
# `ci/checks/gameplay` keeps its substrate and its baseline untouched.
#
# # THE SUBSTRATE, AND THE ONE RULE THAT IS NOT COSMETIC
#
# Identical to `ci/t2-gameplay.sh`'s except for two things:
#
#   * `gamemode=survival` instead of creative — the whole point;
#   * `natural_health_regeneration false`, added to the four global rules.
#
# **That rule is load-bearing and difficulty stays peaceful because of it.**
# `ServerPlayer.tickRegeneration` offsets 7-32 are
# `difficulty == PEACEFUL && gamerule NATURAL_HEALTH_REGENERATION` -> heal on
# a `tickCount % 20` timer. A survival player on peaceful therefore heals on a
# clock, which would make `set_health` fire on a timer nobody drove and every
# damage measurement in this pass a coin toss. Turning the rule off shuts that
# heal AND the food-based regen in `FoodData.tick`, which is why the substrate
# needs only ONE property change and not a difficulty change as well.
#
# Keeping `difficulty=peaceful` matters: it is what keeps `spawn-monsters` and
# the rest of the creative substrate's invariants true, so the only variable
# between this pass and the corpus is the game mode. The peaceful lever is
# still real — `Player.hurtServer` offsets 62-115 zero the damage for any
# source whose `DamageScaling` is `always`, which is 4 of the jar's 51 damage
# types (explosion, player_explosion, bad_respawn_point, sonic_boom) — but
# none of those four is reachable from these scenarios, and `player_attack` is
# `when_caused_by_living_non_player`, whose causer here IS a player, so
# `scalesWithDifficulty()` returns false and the peaceful branch is skipped
# entirely. That is the reading; the recording is what checks it.
#
# # IT PRODUCES NO CORPUS FIGURE
#
# Its denominator is `ci/checks/survival`, which is not the corpus. Nothing
# here may be quoted as a parity percentage — `ci/t2-gameplay.sh` remains the
# only thing that may produce one. What this produces is verdicts on scenarios
# that had no verdict at all before, which is a different and smaller claim.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VAN_PORT="${T2_SURVIVAL_VANILLA_PORT:-}"
OUR_PORT="${T2_SURVIVAL_RUSTMC_PORT:-}"
WORK="$ROOT/oracle-work/gameplay/survival"
VAN_DIR="$WORK/vanilla"
CHECKS="${RUSTMC_SURVIVAL_CHECKS:-$ROOT/ci/checks/survival}"
SEED="${GAMEPLAY_SEED:-20260803}"

RUSTMC_PID=""
cleanup() {
    [ -n "$RUSTMC_PID" ] && kill "$RUSTMC_PID" 2>/dev/null || true
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

refuse() { echo; echo "SURVIVAL PASS REFUSED — $*" >&2; exit 4; }

# The same work-directory refusal `ci/t2-gameplay-subset.sh` carries, for the
# same reason: a run that records into a tree it is not measuring produces a
# verdict about the wrong tree.
for d in oracle-work oracle-work/gameplay; do
    if [ -L "$ROOT/$d" ]; then
        tgt="$(readlink -f "$ROOT/$d" 2>/dev/null || true)"
        case "$tgt/" in
            "$ROOT"/*) ;;
            *) refuse "$d is a symlink to $tgt, outside $ROOT.
         Fix:  rm $ROOT/$d   (the symlink only — never the target)
               mkdir -p $ROOT/oracle-work/gameplay" ;;
        esac
    fi
done

# `|| true` on the find, and it is not noise. Without it `set -euo pipefail`
# kills the script on a missing directory BEFORE `refuse` can say which
# directory — an exit with no message at all, which is the one failure mode a
# refusal is supposed to prevent.
[ -d "$CHECKS" ] || refuse "no such scenario directory: $CHECKS"
N_CHECKS="$( { find "$CHECKS" -name '*.toml' 2>/dev/null || true; } | wc -l)"
[ "$N_CHECKS" -gt 0 ] || refuse "no scenarios in $CHECKS"
echo "survival pass: $N_CHECKS scenario(s) from $CHECKS"

# **THE HOLD REFUSAL, and it is a refusal rather than a warning.**
#
# A `hold` in a survival scenario does not fail — `handleSetCreativeModeSlot`
# drops the packet in silence — so the scenario runs with an empty hand and
# reports a divergence that has nothing to do with the server. That is the
# single most expensive mistake available in this directory, and it is
# invisible in the output, so it is caught here rather than discovered.
if grep -rlE '^\s*do\s*=\s*"hold"' "$CHECKS" 2>/dev/null | grep -c . >/dev/null; then
    echo "  offending file(s):" >&2
    grep -rlE '^\s*do\s*=\s*"hold"' "$CHECKS" >&2
    refuse "a scenario in $CHECKS uses Step::Hold.
         handleSetCreativeModeSlot refuses the packet outright when the player
         is not creative — silently, with nothing in the log — so the step
         does not fail, it does NOTHING, and every verdict downstream of it
         describes a rig that was never built."
fi

# **THE TABLE PREFLIGHT, and it is here because its absence cost a run.**
#
# `ci/t2-gameplay-subset.sh` has one and this script did not. The first
# detached attempt reached the recorder and died on
# `data/vanilla/entity_types.json: No such file or directory` — AFTER queueing
# behind three other runs for the corpus lock. A missing table is cheap to
# detect and expensive to discover, and the expense is entirely in the
# waiting, so the detection belongs above `corpus_lock_acquire` rather than
# below it.
#
# Derived rather than listed, the same way the subset script derives it: every
# `data/vanilla/...` path the server crate actually opens. A hand-kept list
# goes stale, and this one would go stale silently.
mapfile -t REQUIRED < <(grep -rhoE '"data/vanilla/[a-z_]+\.(json|bin)"' \
                        "$ROOT/crates/rustmc-server/src" | tr -d '"' | sort -u)
[ "${#REQUIRED[@]}" -gt 0 ] || refuse "derived no required tables — this script has gone stale"
MISSING=()
for t in "${REQUIRED[@]}"; do
    [ -f "$ROOT/$t" ] || MISSING+=("$t")
done
if [ "${#MISSING[@]}" -gt 0 ]; then
    printf '  missing: %s\n' "${MISSING[@]}" >&2
    refuse "${#MISSING[@]} of ${#REQUIRED[@]} required table(s) are absent from this tree.
         Run: ./ci/gen-gameplay-tables.sh
         (and if that stops early, cargo must be on PATH:
          export PATH=\$HOME/.cargo/bin:\$PATH)
         Checked BEFORE the corpus lock on purpose — the first run of this
         script queued behind three other runs and then died on a missing
         table, which is the whole cost of finding out late."
fi

. "$ROOT/ci/corpus-lock.sh"
corpus_lock_acquire
# Ports are chosen AFTER the corpus lock, from the tree path, and probed:
# the lock is a pool since 2026-09-24 and a fixed pair shared by every tree is
# the 2026-08-06 collision under a pool. See ci/corpus-ports.sh.
. "$ROOT/ci/corpus-ports.sh"
if [ -z "$VAN_PORT" ] || [ -z "$OUR_PORT" ]; then
    mapfile -t _PP < <(corpus_free_ports 2 survival) || true
    [ "${#_PP[@]}" -eq 2 ] || { echo "SURVIVAL REFUSED — no two free ports in 28000-28999" >&2; exit 4; }
    VAN_PORT="${VAN_PORT:-${_PP[0]}}"; OUR_PORT="${OUR_PORT:-${_PP[1]}}"
fi

cargo build -q -p oracle-runner --release
cargo build -q -p rustmc-server --release
BIN="$ROOT/target/release/oracle-runner"

rm -rf "$WORK"
mkdir -p "$WORK"

echo "== vanilla (superflat, SURVIVAL, no structures) on :$VAN_PORT"
VANILLA_EXTRA_PROPS=$'gamemode=survival\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
    "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$VAN_PORT" "minecraft:flat" "$SEED"
# The corpus's four, plus the one this pass adds. See the header: without
# `natural_health_regeneration false` a survival player on peaceful heals on a
# 20-tick timer and every set_health reading becomes a coin toss.
for r in advance_time spawn_mobs random_tick_speed advance_weather natural_health_regeneration; do
    case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
done
sleep 2
"$BIN" record "127.0.0.1:$VAN_PORT" "$CHECKS" --out "$WORK/vanilla.json" \
    --console "$VAN_DIR/console.fifo"
"$ROOT/ci/vanilla-server.sh" alive "$VAN_DIR" || refuse \
    "vanilla was NOT RUNNING when its recording finished — it died mid-pass.
     $WORK/vanilla.json describes a server that stopped answering."
"$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR"

echo "== rustmc (SURVIVAL) on :$OUR_PORT"
. "$ROOT/ci/server-ready.sh"
# **The subject gets a console too, and the same five rules through it.**
#
# `ci/t2-gameplay.sh` has passed `--console` on both record passes since the
# console step landed; this pass did not, so a survival scenario with a
# `do = "console"` step — every `/damage` check — refused on the subject
# before it measured anything. And the five game rules above reached vanilla
# alone: this server's `natural_health_regeneration` stayed at its default
# `true`, so `FoodData.tick`'s heal arm ran on the subject and not on the
# reference, which is exactly the coin toss the header says the rule exists
# to remove.
OUR_DIR="$WORK/rustmc"
mkdir -p "$OUR_DIR"
mkfifo "$OUR_DIR/console.fifo" 2>/dev/null || true
./target/release/rustmc-server run --port "$OUR_PORT" --level-type flat --radius 2 \
    --view-distance 4 --gamemode survival --console "$OUR_DIR/console.fifo" \
    >"$WORK/rustmc.log" 2>&1 &
RUSTMC_PID=$!
echo "$RUSTMC_PID" > "$OUR_DIR/server.pid"
RUSTMC_READY=0
for _ in $(seq 1 120); do
    grep -q "listening on 0.0.0.0:$OUR_PORT[ ,]" "$WORK/rustmc.log" 2>/dev/null \
        && ready_socket_owner "$OUR_PORT" "$RUSTMC_PID" \
        && ready_speaks_status "$BIN" "127.0.0.1:$OUR_PORT" && { RUSTMC_READY=1; break; }
    kill -0 "$RUSTMC_PID" 2>/dev/null || { echo "rustmc died — tail:"; tail -20 "$WORK/rustmc.log"; exit 1; }
    sleep 1
done
[ "$RUSTMC_READY" = 1 ] || refuse \
    "rustmc-server never proved it was listening on :$OUR_PORT within 120s.
     tail: $(tail -20 "$WORK/rustmc.log")"
sleep 1
for r in advance_time spawn_mobs random_tick_speed advance_weather natural_health_regeneration; do
    case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
    "$ROOT/ci/vanilla-server.sh" console "$OUR_DIR" "gamerule $r $v" || refuse \
        "could not deliver 'gamerule $r $v' to rustmc through $OUR_DIR/console.fifo"
done
sleep 1
if grep -qE "console: no such game rule|console: .* takes (true or false|an integer)" "$WORK/rustmc.log"; then
    refuse "rustmc refused a substrate game rule — see $WORK/rustmc.log"
fi
"$BIN" record "127.0.0.1:$OUR_PORT" "$CHECKS" --out "$WORK/rustmc.json" \
    --console "$OUR_DIR/console.fifo"
kill "$RUSTMC_PID" 2>/dev/null || true
RUSTMC_PID=""

# No --baseline on the first run of this pass, and that is deliberate rather
# than pending. `ci/checks/gameplay-baseline.toml` is a ratchet, and a row
# written from a run that has happened once locks in a state measured at N=1 —
# which is how a future false regression gets manufactured. The first run
# reports; the baseline is written from what it measured, with its N beside
# it, and only for verdicts that a second run reproduced.
"$BIN" compare "$CHECKS" --reference "$WORK/vanilla.json" --subject "$WORK/rustmc.json" \
    --json "$WORK/compare.json" || true

echo
echo "== substrate check: did the gates actually open? =="
# The whole pass is worthless if vanilla behaved as it does in creative, and
# that failure looks exactly like a green everywhere else in the pipeline —
# every scenario runs, every creative-substrate packet still arrives, and the
# comparison is well-formed. Nothing downstream would catch it.
#
# It lives in ci/survival-substrate-assert.py, with its own falsifier, rather
# than inline here: an assertion whose absence is invisible is precisely the
# one that has to be runnable against a synthetic input.
"$ROOT/ci/survival-substrate-assert.py" "$WORK/vanilla.json"

echo
echo "SURVIVAL PASS COMPLETE — $N_CHECKS scenario(s)."
echo "  NOT a corpus figure. Its denominator is $CHECKS, which is not the corpus."
echo "  reference: $WORK/vanilla.json"
echo "  subject:   $WORK/rustmc.json"
