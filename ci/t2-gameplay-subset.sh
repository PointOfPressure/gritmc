#!/usr/bin/env bash
# t2-gameplay-subset.sh — the gameplay oracle over a named handful of checks.
#
#     ./ci/t2-gameplay-subset.sh 238-digging-the-top-of-a-tall-flower-takes-the-bottom.toml
#     ./ci/t2-gameplay-subset.sh 130-*.toml 132-*.toml
#
# Same instrument as `ci/t2-gameplay.sh` — real vanilla as the reference, this
# server as the subject, the scenarios' own projections — over a copy of the
# corpus containing only the files named. Roughly one minute instead of forty.
#
# # What this is for, and what it must never be used for
#
# It exists for one discipline: **write the check, watch it go red, fix the
# server, watch it go green.** A check written after its fix has never been
# observed failing, and this project has now found fourteen checks that could
# not fail. Making the red observation cost forty minutes is what tempts
# people to skip it, so it costs one minute here.
#
# It is **not a gate and produces no score.** It does not read
# `gameplay-baseline.toml`, does not ratchet, and its denominator is whatever
# you passed on the command line. Never quote a number from it as a corpus
# figure — `ci/t2-gameplay.sh` is the only thing that may produce one, because
# a denominator that changes per invocation is exactly how two withdrawn
# figures got published. Anything landing in a tracked doc comes from the full
# gate.
#
# The one thing it does keep is the provenance preflight, because a subset run
# against another tree's tables measures that tree just as thoroughly as a
# full one does.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

[ "$#" -gt 0 ] || { echo "usage: $0 <check.toml> [check.toml ...]" >&2; exit 2; }

VAN_PORT="${T2_GAMEPLAY_VANILLA_PORT:-}"
OUR_PORT="${T2_GAMEPLAY_RUSTMC_PORT:-}"
WORK="$ROOT/oracle-work/gameplay/subset"
VAN_DIR="$WORK/vanilla"
SUBSET="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"

RUSTMC_PID=""
cleanup() {
    [ -n "$RUSTMC_PID" ] && kill "$RUSTMC_PID" 2>/dev/null || true
    # Tidies the corpus lock's holder record. NOT what releases the lock — the
    # kernel kills the anchor when this process dies. Guarded because the trap
    # is installed before ci/corpus-lock.sh is sourced.
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# **The same work-directory refusal the full runner has, and it belongs here
# more than it belongs there.**
#
# `ci/t2-gameplay.sh` refuses to run in a tree whose `oracle-work` is a
# whole-directory symlink out of `$ROOT`, because "the gate would record into,
# and read from, a tree it is not gating". This script did not, and this is the
# instrument an agent reaches for *precisely* when it is in a chain directory,
# which is where that hazard lives. Every subset run in the chain that added
# the attack and sneak verbs therefore recorded into `~/rustmc/oracle-work/
# gameplay/subset` and booted its vanilla there — in a world whose workspaces
# earlier runs had already generated — while the gate booted a fresh world and
# streamed 67 workspaces' chunks as it went. The two are not the same
# environment, and one check's verdict differed between them.
#
# Refusing here does not make the subset a gate; it makes the one-minute
# instrument measure the same tree the forty-minute one does, which is the only
# reason a red observed here is evidence about a red observed there.
refuse() { echo; echo "SUBSET REFUSED — $*" >&2; exit 4; }
for d in oracle-work oracle-work/gameplay; do
    if [ -L "$ROOT/$d" ]; then
        tgt="$(readlink -f "$ROOT/$d" 2>/dev/null || true)"
        case "$tgt/" in
            "$ROOT"/*) ;;
            *) refuse "$d is a symlink to $tgt, which is outside $ROOT.
         This run would record into, and boot vanilla from, a tree it is not
         measuring — and then a red seen here would not be a red seen by
         ci/t2-gameplay.sh.
         Fix:  rm $ROOT/$d   (the symlink only — never the target)
               mkdir -p $ROOT/oracle-work/gameplay
         and symlink the individual vanilla references in, not the directory." ;;
        esac
    fi
done

# The two servers must be on the SAME difficulty. Sourced here for the same
# reason the work-directory refusal above is here: this is the instrument an
# agent reaches for when writing a new check, so a red observed here has to be
# a red observable in `ci/t2-gameplay.sh`, and a subset run at a different
# difficulty from the gate is a different environment. `ci/gameplay-difficulty.sh`.
. "$ROOT/ci/gameplay-difficulty.sh"
difficulty_or_die "$ROOT" || exit 4

# And the game rules, from the same one place the gate reads them. This script
# quieted VANILLA with its own second copy of the list and sent nothing at all
# to the subject — it passed no `--console`, so it could not have. See
# `ci/gameplay-substrate.sh`.
. "$ROOT/ci/gameplay-substrate.sh"

. "$ROOT/ci/provenance.sh"
. "$ROOT/ci/server-ready.sh"
mapfile -t REQUIRED < <(grep -rhoE '"data/vanilla/[a-z_]+\.(json|bin)"' \
                        "$ROOT/crates/rustmc-server/src" | tr -d '"' | sort -u)
[ "${#REQUIRED[@]}" -gt 0 ] || { echo "derived no required tables — this script has gone stale" >&2; exit 4; }
for t in "${REQUIRED[@]}"; do
    if why="$(prov_check "$ROOT" "$ROOT/$t")"; then continue; fi
    echo "SUBSET REFUSED — $t: $why" >&2
    echo "         Run: ./ci/gen-gameplay-tables.sh" >&2
    exit 4
done

rm -rf "$WORK"
mkdir -p "$SUBSET"
for f in "$@"; do
    src="$ROOT/ci/checks/gameplay/$(basename "$f")"
    [ -f "$src" ] || { echo "no such check: $src" >&2; exit 2; }
    cp "$src" "$SUBSET/"
done
echo "subset: $(find "$SUBSET" -name '*.toml' | wc -l) check(s)"

# One corpus run on this box at a time. A subset is a corpus run — it boots a
# vanilla and a rustmc-server and drives scenarios at them — so it collides with
# a gate exactly as the full runner does, and being short does not make it
# harmless: it is the *boots* and the settle waits that collide, not the
# scenario count. Waiting behind a gate to debug one check is a real cost, and
# it is still cheaper than the alternative, which is what happened on
# 2026-08-06. See ci/corpus-lock.sh.
. "$ROOT/ci/corpus-lock.sh"
corpus_lock_acquire
# Ports are chosen AFTER the corpus lock, from the tree path, and probed:
# the lock is a pool since 2026-09-24 and a fixed pair shared by every tree is
# the 2026-08-06 collision under a pool. See ci/corpus-ports.sh.
. "$ROOT/ci/corpus-ports.sh"
if [ -z "$VAN_PORT" ] || [ -z "$OUR_PORT" ]; then
    mapfile -t _PP < <(corpus_free_ports 2 subset) || true
    [ "${#_PP[@]}" -eq 2 ] || refuse "no two free ports in 28000-28999"
    VAN_PORT="${VAN_PORT:-${_PP[0]}}"; OUR_PORT="${OUR_PORT:-${_PP[1]}}"
fi

cargo build -q -p oracle-runner --release
cargo build -q -p rustmc-server --release
BIN="$ROOT/target/release/oracle-runner"

echo "== vanilla on :$VAN_PORT"
VANILLA_EXTRA_PROPS="gamemode=creative
generate-structures=false
spawn-monsters=false
difficulty=$GAMEPLAY_DIFFICULTY
spawn-radius=0" \
    "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$VAN_PORT" "minecraft:flat" "$SEED"
# **The same substrate as `ci/t2-gameplay.sh`, including the pinned day time.**
# A subset run on a different substrate is not the cheap version of the gate,
# it is a different experiment — and this script exists to make the red
# observation that the gate later reproduces. `SUBSTRATE_DAY_TIME` is 6000
# there and 6000 here; see that file for why noon and why pinning it at all.
SUBSTRATE_DAY_TIME=6000
for gr in "${SUBSTRATE_GAMERULES[@]}"; do
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule ${gr%%=*} ${gr#*=}"
done
sleep 2
"$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "time set $SUBSTRATE_DAY_TIME"
sleep 1
# **THIS GREP SAID `Set the time to $SUBSTRATE_DAY_TIME` UNTIL 2026-09-24, AND
# 26.2 NEVER PRINTS IT.** `ci/t2-gameplay.sh`'s `time_set_or_die` records the
# same defect costing `stack-worldgen` a T2 slot on 2026-09-05 and fixed it
# there by deriving the confirmation from the pinned jar; this copy was not
# fixed, so every subset run on 26.2 exited 1 here, after its whole wait for
# the corpus lock and a vanilla boot. Measured on `corpus-parallel`: vanilla's
# log reads `Set minecraft:overworld to 6000 tick(s)`. Derived now, the same
# way, from the same key (`commands.time.set.absolute`).
_VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
_TIME_RE="$(unzip -p "$ROOT/oracle-work/vanilla-$_VER/versions/$_VER/server-$_VER.jar" \
    assets/minecraft/lang/en_us.json | python3 "$ROOT/ci/lang-message-re.py" \
    commands.time.set.absolute "$SUBSTRATE_DAY_TIME")" \
    || refuse "could not derive the 'time set' confirmation from the pinned jar"
grep -qE "$_TIME_RE" "$VAN_DIR/logs/latest.log" 2>/dev/null || {
    echo "FAIL: vanilla never confirmed 'time set $SUBSTRATE_DAY_TIME'" >&2
    tail -5 "$VAN_DIR/logs/latest.log" >&2
    exit 1
}
# **`--console` on the REFERENCE pass as well — 2026-10-01, `commands-exec`.**
# The subject pass below has carried it since 2026-09-24; this one never did,
# so every scenario with a `do = "console"` step (and therefore a
# `console_restore`) came back `error — this pass has no console` from the
# subset while the full gate, which passes it on both (`ci/t2-gameplay.sh`
# line ~912), measured it fine. Eight of nine command checks errored that way
# before this line existed.
"$BIN" record "127.0.0.1:$VAN_PORT" "$SUBSET" --out "$WORK/vanilla.json" \
    --console "$VAN_DIR/console.fifo"
"$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR"

echo "== rustmc on :$OUR_PORT"
# `$OUR_DIR` and the fifo mirror `ci/t2-gameplay.sh`'s, so `ci/vanilla-server.sh
# console` is reused unmodified for the subject exactly as it is there.
OUR_DIR="$WORK/rustmc"
mkdir -p "$OUR_DIR"
mkfifo "$OUR_DIR/console.fifo" 2>/dev/null || true
# `--view-distance 4` matches `ci/vanilla-server.sh`'s `view-distance=4`, and
# it is substrate in the same sense `difficulty=peaceful` is: the two servers
# have to be configured alike or every distance-dependent comparison measures
# the configuration. It went unset until 2026-08-04, when
# `530-walking-away-drops-a-standing-player-from-view` measured the
# difference — this server was using the generated spawn tile's radius, 2, as
# a view distance, so it tracked entities to 32 blocks where vanilla tracked
# to 64. `--radius` is a separate quantity and stays: it is how big a tile is.
# `--level-seed`: the seed vanilla is booted with (`level-seed=` above),
# which every loot table naming a `random_sequence` draws from.
./target/release/rustmc-server run --port "$OUR_PORT" --level-type flat --radius 2 \
    --level-seed "$SEED" \
    --day-time "$SUBSTRATE_DAY_TIME" --advance-time false \
    --view-distance 4 ${DIFFICULTY_ARGS[@]+"${DIFFICULTY_ARGS[@]}"} \
    --console "$OUR_DIR/console.fifo" \
    --seed "$SEED" \
    >"$WORK/rustmc.log" 2>&1 &
RUSTMC_PID=$!
# **The pid file is what makes `--console` usable at all** — added 2026-09-24,
# found by the world lane recording 798. `ci/vanilla-server.sh console` refuses
# (exit 2, DROPPED) unless `alive "$OUR_DIR"` can read `$OUR_DIR/server.pid`,
# because writing to a fifo whose reader is gone blocks forever. The full gate
# has always written it (`ci/t2-gameplay.sh`, `record_subject`); this copy
# booted the same server with the same fifo and never did, so every substrate
# game rule sent to our server was dropped. `ci/harness-startup-agree.py` (T0)
# now refuses any harness that gives a server a console fifo and no pid file.
echo "$RUSTMC_PID" > "$OUR_DIR/server.pid"
# All three arms, and the reason this script takes the one the full gate does
# not is that **nothing downstream of here checks the recording.**
# `ci/t2-gameplay.sh` has `assert_recording`, which refuses a trace that does
# not cover the corpus; this script records and compares whatever it gets. So
# the readiness proof is the only thing standing between it and a silently
# thin result, and it takes every arm available: the post-bind line naming
# **this** port, the kernel's word that the socket is ours, and a status
# exchange through the recorder's own client stack — which is the one arm that
# distinguishes a bound socket from a server that answers on it.
#
# It is also the script most likely to meet the failure by construction: the
# port is a fixed default, 25897, so two of these at once, or one left over,
# is the `Address already in use` case rather than a hypothetical.
RUSTMC_READY=0
for _ in $(seq 1 120); do
    grep -q "listening on 0.0.0.0:$OUR_PORT[ ,]" "$WORK/rustmc.log" 2>/dev/null && ready_socket_owner "$OUR_PORT" "$RUSTMC_PID" && ready_speaks_status "$BIN" "127.0.0.1:$OUR_PORT" && { RUSTMC_READY=1; break; }
    kill -0 "$RUSTMC_PID" 2>/dev/null || { echo "rustmc died — tail:"; tail -20 "$WORK/rustmc.log"; exit 1; }
    sleep 1
done
[ "$RUSTMC_READY" = 1 ] || {
    echo "rustmc-server never proved it was listening on :$OUR_PORT within 120s" >&2
    echo "(alive, but no post-bind line for that port, or the socket there is not ours," >&2
    echo " or it does not answer a status request) — tail:" >&2
    tail -20 "$WORK/rustmc.log" >&2
    exit 1
}
sleep 1
# THE SUBJECT WAS NEVER QUIETED HERE, AND THE GATE HAS QUIETED IT SINCE
# 2026-08. `ci/t2-gameplay.sh`'s own comment says why: this server's
# `random_tick_speed` default is vanilla's 3, so an un-quieted subject
# random-ticks every rig's grass through every scenario. This script sent the
# four rules to vanilla and nothing to rustmc, which is the asymmetry its own
# header forbids — "a red observed here is evidence about a red observed there"
# only if the two instruments run the same environment.
for gr in "${SUBSTRATE_GAMERULES[@]}"; do
    "$ROOT/ci/vanilla-server.sh" console "$OUR_DIR" "gamerule ${gr%%=*} ${gr#*=}" || {
        echo "FAIL: could not deliver 'gamerule ${gr%%=*} ${gr#*=}' to rustmc in $OUR_DIR" >&2
        exit 1
    }
done
sleep 1
if grep -qE "console: no such game rule|console: .* takes (true or false|an integer)" "$WORK/rustmc.log" 2>/dev/null; then
    echo "FAIL: rustmc refused a substrate game rule — see $WORK/rustmc.log" >&2
    grep -E "console: no such game rule|console: .* takes" "$WORK/rustmc.log" | tail -4 >&2
    exit 1
fi
# `--console` on the recorder too, or a scenario declaring a `[[gamerule]]`
# window cannot run at all in the subset — the one instrument an agent uses to
# watch a new check go red.
"$BIN" record "127.0.0.1:$OUR_PORT" "$SUBSET" --out "$WORK/rustmc.json" \
    --console "$OUR_DIR/console.fifo"
kill "$RUSTMC_PID" 2>/dev/null || true
rm -f "$OUR_DIR/server.pid"
RUSTMC_PID=""

# No --baseline: this is an observation, not a ratchet.
"$BIN" compare "$SUBSET" --reference "$WORK/vanilla.json" --subject "$WORK/rustmc.json"
