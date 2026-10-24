#!/usr/bin/env bash
# block-entity-subset.sh — iterate on the block-entity checks without a corpus run.
#
#     ./ci/block-entity-subset.sh                 # 76* by default
#     BE_SUBSET_GLOB='77*' ./ci/block-entity-subset.sh
#
# **This is not a gate and must never be quoted as one.** It records a *subset*
# of `ci/checks/gameplay` against vanilla and against this server and runs the
# comparator over it, so the numbers it prints have a denominator of whatever
# matched the glob. `docs/PARITY-ROADMAP.md` §20 exists because a corpus score
# was once compared across two different denominators; the same mistake is
# available here and it is available cheaply, so: **no score printed here is a
# corpus score.** `ci/t2-gameplay.sh` is the gate, it reads no baseline from
# this script, and this script writes none.
#
# What it is for is the inner loop. `ci/t2-gameplay.sh` takes the box-wide
# corpus lock and runs 90-odd scenarios twice at ~9 s each with the box to
# itself, and it is frequently three deep; a branch adding thirteen checks
# cannot afford one iteration per idea. This boots both servers on their own
# ports, records only the matching checks, and prints per-check verdicts in
# about a minute.
#
# It deliberately does **not** take the corpus lock — it is the same shape as
# `ci/finish-dig-probe.sh` and `ci/block-entity-probe.sh`, which do not either.
# Keep the glob small for that reason: a long run outside the lock is
# throughput taken from whoever holds it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GLOB="${BE_SUBSET_GLOB:-76*}"
VAN_PORT="${BE_SUBSET_VAN_PORT:-25892}"
OUR_PORT="${BE_SUBSET_OUR_PORT:-25893}"
WORK="$ROOT/oracle-work/gameplay/be-subset"
VAN_DIR="$WORK/vanilla"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"
RUSTMC_PID=""

cleanup() {
    [ -n "$RUSTMC_PID" ] && kill "$RUSTMC_PID" 2>/dev/null || true
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "$WORK"
mkdir -p "$SCEN"
# shellcheck disable=SC2086
cp $ROOT/ci/checks/gameplay/$GLOB.toml "$SCEN/"
n=$(find "$SCEN" -name '*.toml' | wc -l)
[ "$n" -gt 0 ] || { echo "no checks matched $GLOB" >&2; exit 2; }
echo "== $n check(s) matching '$GLOB' — NOT a corpus score, see this file's header"

cargo build -q -p oracle-runner --release
cargo build -q --release -p rustmc-server
BIN="$ROOT/target/release/oracle-runner"
. "$ROOT/ci/server-ready.sh"

echo "== booting vanilla on :$VAN_PORT"
VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
    "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$VAN_PORT" "minecraft:flat" "$SEED"
for r in advance_time spawn_mobs random_tick_speed advance_weather; do
    case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
done
sleep 2
"$BIN" record "127.0.0.1:$VAN_PORT" "$SCEN" --out "$WORK/vanilla.json"
"$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR"

echo "== booting rustmc on :$OUR_PORT"
./target/release/rustmc-server run --port "$OUR_PORT" --level-type flat --radius 2 \
    --view-distance 4 >"$WORK/rustmc.log" 2>&1 &
RUSTMC_PID=$!
# Same three arms as `ci/t2-gameplay-subset.sh`, for the same reason — nothing
# downstream asserts the recording — and one more that is this script's alone:
# it has just stopped a vanilla server, and a port a process was told to
# release is not a port it has released. The socket-owner arm is what makes
# "the port answers" and "the port is ours" different statements while the
# previous occupant is still shutting down.
RUSTMC_READY=0
for _ in $(seq 1 120); do
    grep -q "listening on 0.0.0.0:$OUR_PORT[ ,]" "$WORK/rustmc.log" 2>/dev/null && ready_socket_owner "$OUR_PORT" "$RUSTMC_PID" && ready_speaks_status "$BIN" "127.0.0.1:$OUR_PORT" && { RUSTMC_READY=1; break; }
    kill -0 "$RUSTMC_PID" 2>/dev/null || { echo "rustmc died — tail:"; tail -20 "$WORK/rustmc.log"; exit 1; }
    sleep 1
done
[ "$RUSTMC_READY" = 1 ] || {
    echo "rustmc-server never proved it was listening on :$OUR_PORT within 120s" >&2
    tail -20 "$WORK/rustmc.log" >&2
    exit 1
}
sleep 1
"$BIN" record "127.0.0.1:$OUR_PORT" "$SCEN" --out "$WORK/rustmc.json"
kill "$RUSTMC_PID" 2>/dev/null || true
RUSTMC_PID=""

# No `--baseline`: a baseline is a ratchet over the whole corpus and handing it
# a subset would compare a partial run against a complete file.
"$BIN" compare "$SCEN" --reference "$WORK/vanilla.json" --subject "$WORK/rustmc.json" \
    --json "$WORK/report.json" || true

python3 - "$WORK/report.json" "$WORK/vanilla.json" "$WORK/rustmc.json" <<'PY'
import json, sys
rep = json.load(open(sys.argv[1]))
van = {s["name"]: s for s in json.load(open(sys.argv[2]))["scenarios"]}
our = {s["name"]: s for s in json.load(open(sys.argv[3]))["scenarios"]}
rows = rep.get("outcomes", [])
print(f"\n{'check':62s} {'declared':10s} {'strict':10s} be_data van/ours")
print("-" * 104)
for r in rows:
    n = r.get("name", "?")
    def bed(d):
        s = d.get(n)
        if not s:
            return "-"
        o = [x for x in s.get("obs", ()) if x["kind"] == "block_entity_data" and x["who"] == "actor"]
        return ",".join(str(x["fields"].get("block_entity_type")) for x in o) or "none"
    print(f"{n:62s} {str(r.get('verdict')):10s} {str(r.get('strict')):10s} {bed(van):>12s} / {bed(our)}")
PY
