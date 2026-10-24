#!/usr/bin/env bash
# schema-check-falsify.sh — do `960`-`963` go red when the property is pinned
# back to the constant the corpus already accepted? Against THIS SERVER ALONE.
#
#     ./ci/schema-check-falsify.sh
#
# # Why this needs no vanilla and no corpus lock
#
# A discriminator has two halves. *Does our answer agree with vanilla* needs
# the reference server, the lock, and forty minutes. *Can this check come back
# red at all* needs neither: it is a question about *our* pipeline — does the
# property reach the wire, and does the scenario's declared projection carry
# it — and our own server answers it, compared against its own mutated self.
#
# Doing that half first is not a shortcut, it is the cheap half of the same
# discipline `ci/t2-gameplay-subset.sh`'s header describes: write the check,
# watch it go red, and only then spend a corpus slot. Running this before the
# gate already caught two defects in the harness that would otherwise have
# been found after a forty-minute run.
#
# It boots one `rustmc-server` and drives four scenarios at it. That is a
# server with `settle_ms` timers, which is exactly the shape `ci/corpus-lock.sh`
# serialises, so **it refuses to start while the lock is held** — not because
# CPU is scarce (the documented pathology is the opposite: two runs at load
# 0.02 waiting out each other's timers) but because driving a second server is
# the thing that causes it. It takes about a minute, so it waits rather than
# queues.
#
# # How each of the four is broken, and why they are broken two different ways
#
# `data/vanilla/placement.json` is read from disk on every connection
# (`crates/rustmc-server/src/session_tables.rs:120` ->
# `crates/rustmc-server/src/placement.rs:432`, a plain `read_to_string`; there
# is no `build.rs` and nothing is baked into the binary). So a placement
# property can be pinned to a constant by editing data, with no rebuild — and
# a rebuild is not merely inconvenient here, it would be a different
# experiment, because it would also rebuild the code the recording is
# supposed to be exercising.
#
# Each row of `table` carries `states` (the distinct results) and `cells` (144
# indices into it). Pinning a block to one state is therefore one edit:
# rewrite that row's `states` so every entry is the same string. The 144 cells
# keep pointing wherever they pointed and every one of them now resolves to
# the constant.
#
#     960  table[2]    oak_log  [axis=y, axis=z, axis=x] -> all axis=x
#     961  table[232]  anvil    [west, north, east, south] -> all north
#
# `962` and `963` are NOT pinned, because **this server already answers them
# with the constant** — they are the wrong model, live, today:
#
#   * `crates/rustmc-server/src/power.rs:2427` — on the falling edge the lamp
#     calls `lamp_wants_a_tick`, which logs *"this server has no lamp tick —
#     it will not go out"* and pushes no `TickRequest`. `ticks.rs` dispatches
#     only torches, observers and repeaters. **A lit lamp stays lit for ever**,
#     which is precisely the "never un-lights" server `962` was written against.
#   * `crates/rustmc-server/src/placement.rs:971` — `computed_with_neighbours`
#     handles `minecraft:ladder` and `minecraft:redstone_wire` and nothing
#     else. There is no `EndRodBlock` port anywhere in `crates/`. **An end rod
#     always takes the clicked face**, which is exactly the wrong model `963`
#     names.
#
# For those two, pinning would be a no-op dressed as an experiment — you
# cannot pin a constant that is already the answer, and a verdict that does
# not move would read as "the check is dead" when the truth is "the server was
# broken before I touched it". So they are not pinned. What this prints
# instead is the states our server actually put on the wire for all four rigs
# (`ci/schema-falsify-dump.py`), which is the evidence for the two claims
# above, and it is **printed rather than asserted** — a script that asserted
# our server is missing a feature would go red the day someone fixes it.
#
# Their red-when-broken half is therefore owed by the recording-mutation leg,
# `ci/schema-check-falsify.py`, which rewrites one property in a recording and
# requires the verdict to move. That answers the narrower question those two
# still need answered — does the scenario's projection carry the field at all
# — without a vanilla boot, and it is run separately because it takes a
# recording rather than a server.
#
# Nothing here claims our server is right. `ci/t2-gameplay.sh` is the only
# thing that may say that, and for `962` and `963` it is expected to say the
# opposite.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${SCHEMA_FALSIFY_PORT:-25971}"
WORK="$ROOT/oracle-work/schema-falsify"
TABLE="$ROOT/data/vanilla/placement.json"
BACKUP="$WORK/placement.json.orig"
BIN="$ROOT/target/release/oracle-runner"
SERVER="$ROOT/target/release/rustmc-server"
PID=""

cleanup() {
    [ -n "$PID" ] && kill "$PID" 2>/dev/null || true
    # The table is restored here as well as on the happy path: a run killed
    # half way through must never leave a pinned table behind for the next
    # agent's corpus run to measure. That would read as a regression in a
    # branch that never touched placement.
    if [ -f "$BACKUP" ]; then
        cp "$BACKUP" "$TABLE"
        echo "== placement.json restored from $BACKUP"
    fi
}
trap cleanup EXIT INT TERM

if ./ci/corpus-lock.sh status 2>/dev/null | grep -c 'state:.*HELD' >/dev/null; then
    echo "schema-check-falsify: the corpus lock is HELD — this drives a server" >&2
    echo "  with settle_ms timers and would sit in the holder's timers. Wait." >&2
    exit 3
fi

[ -x "$SERVER" ] && [ -x "$BIN" ] || {
    echo "build first: cargo build --release -p rustmc-server -p oracle-runner" >&2; exit 2; }

rm -rf "$WORK"; mkdir -p "$WORK/checks"
cp ci/checks/gameplay/960-*.toml ci/checks/gameplay/961-*.toml \
   ci/checks/gameplay/962-*.toml ci/checks/gameplay/963-*.toml "$WORK/checks/"
cp "$TABLE" "$BACKUP"
echo "== 4 checks, table backed up to $BACKUP"

# `rustmc-server` prints `listening on 0.0.0.0:PORT` BEFORE it has bound, and
# then prints `listen: Address already in use` and keeps running. So grepping
# for `listening on` — which is what `ci/t2-gameplay.sh` does, and what the
# first version of this script did — is NOT proof that the server this
# function started is the one on that port.
#
# That is not hypothetical. The first run of this script recorded all four
# scenarios against a stray `rustmc-server` left on :25971 by an earlier
# killed job. Every boot after it silently failed to bind, the recordings came
# back plausible, and the pinned-table experiment measured a server whose
# table had never been pinned. **A bind failure that still yields a recording
# is a silent wrong measurement**, and it is the exact shape of the
# provenance defects `ci/t2-gameplay.sh:429` records. So:
#
#   * refuse before starting if anything already holds the port;
#   * refuse after starting if the log ever says the bind failed;
#   * and confirm the listener is OUR pid, not merely that a listener exists.
port_holder() { ss -ltnp 2>/dev/null | grep ":$PORT " || true; }

boot() {
    [ -n "$PID" ] && { kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; PID=""; }
    for _ in $(seq 1 20); do
        [ -z "$(port_holder)" ] && break
        sleep 1
    done
    if [ -n "$(port_holder)" ]; then
        echo "schema-check-falsify: :$PORT is already held and did not free:" >&2
        port_holder >&2
        echo "  refusing — recording against someone else's server is how the" >&2
        echo "  first run of this script measured an unpinned table." >&2
        exit 1
    fi
    rm -rf "$ROOT/world" 2>/dev/null || true
    "$SERVER" run --port "$PORT" --level-type flat --radius 2 --view-distance 4 \
        >"$WORK/rustmc.log" 2>&1 &
    PID=$!
    for _ in $(seq 1 60); do
        if grep -q 'Address already in use' "$WORK/rustmc.log" 2>/dev/null; then
            echo "rustmc could not bind :$PORT:"; tail -5 "$WORK/rustmc.log"; exit 1
        fi
        if grep -q "listening on 0.0.0.0:$PORT[ ,]" "$WORK/rustmc.log" 2>/dev/null \
           && port_holder | grep -c "pid=$PID," >/dev/null; then
            return 0
        fi
        kill -0 "$PID" 2>/dev/null || { echo "rustmc died:"; tail -20 "$WORK/rustmc.log"; exit 1; }
        sleep 1
    done
    echo "rustmc never took :$PORT (log says listening, ss disagrees):"
    tail -20 "$WORK/rustmc.log"; port_holder; exit 1
}

record() {  # record <out>
    "$BIN" record "127.0.0.1:$PORT" "$WORK/checks" --out "$1" >/dev/null
}

# `pin <row-index> <state>` — rewrite one table row's `states` so every entry
# is the same block state, which makes all 144 cells resolve to it. Refuses if
# the row is not the block expected, because the indices are positional and a
# regenerated table would silently pin something else.
pin() {
    python3 - "$TABLE" "$1" "$2" "$3" <<'PY'
import json, sys
path, idx, block, state = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
d = json.load(open(path))
row = d["table"][idx]
if row["block"] != block:
    sys.exit(f"table[{idx}] is {row['block']}, not {block} — the table was "
             "regenerated and these indices have moved; re-derive them")
if state not in row["states"]:
    sys.exit(f"{state} is not one of {row['block']}'s states {row['states']} — "
             "pinning to a state the block cannot take would test the loader's "
             "fallback, not the check")
row["states"] = [state] * len(row["states"])
json.dump(d, open(path, "w"))
PY
}

verdicts() {  # verdicts <reference> <subject>
    "$BIN" compare "$WORK/checks" --reference "$1" --subject "$2" 2>/dev/null \
        | sed -nE 's/^  (match|diverge|absent|vacuous|inert|error)[[:space:]]+([^[:space:]]+)[[:space:]]*$/\2 \1/p'
}

FAILED=0

echo "== booting rustmc alone on :$PORT (no vanilla, no lock)"
boot
record "$WORK/intact.json"
echo "== intact recording taken"
echo
verdicts "$WORK/intact.json" "$WORK/intact.json" | while read -r n v; do
    printf "   intact self-compare  %-8s %s\n" "$v" "$n"
done

# Each pinned block: the row, the block, the constant, and the scenario that
# must notice. `962`/`963` are handled by the python leg, not here.
while read -r idx block state scenario; do
    echo
    echo "== pinning $block to $state (table[$idx]) — the model $scenario names"
    pin "$idx" "$block" "$state"
    boot
    record "$WORK/pinned-$idx.json"
    moved=0
    while read -r n v; do
        if [ "$n" = "$scenario" ]; then
            if [ "$v" = "match" ]; then
                echo "   FAIL $scenario stayed match with the property pinned —"
                echo "        the check cannot see the constant it was written against"
                FAILED=1
            else
                echo "   ok   $scenario  match -> $v"
                moved=1
            fi
        elif [ "$v" != "match" ] && [ "$v" != "vacuous" ]; then
            echo "   COLLATERAL $n -> $v (a pin on $block moved another rig)"
            FAILED=1
        fi
    done < <(verdicts "$WORK/intact.json" "$WORK/pinned-$idx.json")
    [ "$moved" = 1 ] || FAILED=1
    cp "$BACKUP" "$TABLE"
    boot
    record "$WORK/restored-$idx.json"
    bad=$(verdicts "$WORK/intact.json" "$WORK/restored-$idx.json" \
          | awk '$2!="match" && $2!="vacuous"' | wc -l)
    if [ "$bad" = 0 ]; then
        echo "   ok   restored -> every scenario back to match"
    else
        echo "   FAIL restored recording still differs from intact in $bad scenario(s)"
        echo "        the pin was not the only thing that changed, so the red above"
        echo "        cannot be attributed to it"
        FAILED=1
    fi
done <<'ROWS'
2 minecraft:oak_log minecraft:oak_log[axis=x] a-log-clicked-on-the-top-and-the-side-lies-two-ways
232 minecraft:anvil minecraft:anvil[facing=north] an-anvil-and-a-pumpkin-from-one-yaw-are-a-quarter-turn-apart
ROWS

kill "$PID" 2>/dev/null || true; PID=""

echo
echo "== what this server actually emitted, per scenario — the intact leg."
echo "   962 and 963 are NOT pinned above: this server is already the wrong"
echo "   model for both, so what it emitted here IS the evidence, printed"
echo "   rather than asserted. The agreement half belongs to t2-gameplay."
python3 ci/schema-falsify-dump.py "$WORK/intact.json"

echo
[ "$FAILED" = 0 ] && echo "SCHEMA-CHECK-FALSIFY GREEN" || echo "SCHEMA-CHECK-FALSIFY RED"
exit "$FAILED"
