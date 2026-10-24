#!/usr/bin/env bash
# Show `ci/corpus-grades.py` reaching each of its three green-and-meaningless
# verdicts on a check built to deserve it.
#
# # Why a selftest is not enough here
#
# `ci/corpus-grades.py --selftest` exercises `classify`, which is a table. That
# table is not the instrument: the instrument is a server whose subsystems have
# been replaced, a recorder that walks a corpus through it, and a comparator
# that reads the difference. A table nobody has run against a real recording is
# the shape this whole exercise exists to refuse.
#
# So this builds three checks whose verdict is known in advance, records them
# twice against a real server on a private port — once clean, once with the
# controls armed — and asserts the reading:
#
#   grades     a real placement rule, which the `placement` control removes
#   vacuous    a check whose only `observe` kind is `block_changed_ack`, the
#              sequence acknowledgement every server sends whatever it did.
#              The control changes the world and the check cannot see it.
#   unvisited  a check whose step is a command, which is on none of the seven
#              sabotaged paths, so no armed control ever reaches live code.
#              (A walk was the first fixture and is the wrong shape: movement
#              packet counts vary between runs, so it moved on its own and
#              read GRADES — a flaky fixture, not a flaky classifier.)
#
# **No corpus lock and no vanilla boot.** Three scenarios on a private port
# take about a minute; the reference half of each pair is the *clean* recording
# rather than vanilla, because the question is whether the check moved when the
# implementation got worse and vanilla is not part of that question.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

PORT="${GRADES_FALSIFY_PORT:-27915}"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/corpus-grades-falsify.XXXXXX")"
trap '[ -n "${GRADES_FALSIFY_KEEP:-}" ] || rm -rf "$TMP"; [ -n "${SRV:-}" ] && kill "$SRV" 2>/dev/null || true' EXIT

fail=0
want() { # want <case> <ok?> <detail>
    if [ "$2" = 1 ]; then echo "  ok    $1"; else echo "  FAIL  $1 — $3"; fail=1; fi
}

mkdir -p "$TMP/checks"

# --- GRADES: a placement rule, which the `placement` control removes --------
cat > "$TMP/checks/010-grades.toml" <<'TOML'
name = "falsify-grades"
about = "a log placed against a vertical face takes that axis, which is a placement rule"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_changed_ack"]

[[setup]]
do = "look"
yaw = 0.0
pitch = 0.0

[[setup]]
do = "hold"
item = "minecraft:stone"

[[setup]]
do = "place"
on = [1, -1, 0]
face = "up"

[[setup]]
do = "hold"
item = "minecraft:oak_log"

[[step]]
do = "place"
on = [1, 0, 0]
face = "east"
TOML

# --- VACUOUS: reads only the acknowledgement --------------------------------
#
# `block_changed_ack` carries the sequence number the client sent back. Every
# server that answers the packet at all sends it, whatever it did with the
# block, so no implementation difference can move it — while the block update
# beside it, which this check does not name, moves plainly.
cat > "$TMP/checks/020-vacuous.toml" <<'TOML'
name = "falsify-vacuous"
about = "reads only the sequence acknowledgement, which every server sends whatever it did"
observe_from = "actor"
observe = ["block_changed_ack"]

[[setup]]
do = "look"
yaw = 0.0
pitch = 0.0

[[setup]]
do = "hold"
item = "minecraft:stone"

[[setup]]
do = "place"
on = [1, -1, 0]
face = "up"

[[setup]]
do = "hold"
item = "minecraft:oak_log"

[[step]]
do = "place"
on = [1, 0, 0]
face = "east"
TOML

# --- UNVISITED: no armed control is on any path this scenario takes ---------
cat > "$TMP/checks/030-unvisited.toml" <<'TOML'
name = "falsify-unvisited"
about = "answers a command, which is on none of the seven sabotaged paths"
observe_from = "actor"
observe = ["chat"]
settle_ms = 1500

[[step]]
do = "command"
text = "list"
TOML

echo "== building"
cargo build -q -p oracle-runner --release
cargo build -q -p rustmc-server --release --features corpus-sabotage
BIN="$ROOT/target/release/oracle-runner"

boot() { # boot <spec> <logfile>
    RUSTMC_SABOTAGE="$1" ./target/release/rustmc-server run --port "$PORT" \
        --level-type flat --radius 2 --view-distance 4 >"$2" 2>&1 &
    SRV=$!
    for _ in $(seq 1 120); do
        grep -q "listening on 0.0.0.0:$PORT[ ,]" "$2" 2>/dev/null && return 0
        kill -0 "$SRV" 2>/dev/null || { echo "server died:"; tail -20 "$2"; exit 1; }
        sleep 1
    done
    echo "server never bound :$PORT"; tail -20 "$2"; exit 1
}

echo "== recording clean"
boot "" "$TMP/clean.log"
"$BIN" record "127.0.0.1:$PORT" "$TMP/checks" --out "$TMP/clean.json" >/dev/null
kill "$SRV" 2>/dev/null || true; SRV=""; sleep 1

echo "== choosing controls"
"$BIN" project "$TMP/checks" --trace "$TMP/clean.json" --json "$TMP/proj.json" >/dev/null
python3 ci/corpus-controls.py "$TMP/checks" "$TMP/proj.json" > "$TMP/controls.json"
SPEC="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["spec"])' "$TMP/controls.json")"
echo "   RUSTMC_SABOTAGE=$SPEC"

echo "== recording sabotaged"
boot "$SPEC" "$TMP/sab.log"
"$BIN" record "127.0.0.1:$PORT" "$TMP/checks" --out "$TMP/sab.json" >/dev/null
kill "$SRV" 2>/dev/null || true; SRV=""

echo "== reading"
python3 ci/corpus-grades.py --report --checks "$TMP/checks" \
    --reference "$TMP/clean.json" --clean "$TMP/clean.json" \
    --sabotaged "$TMP/sab.json" --controls "$TMP/controls.json" \
    --log "$TMP/sab.log" --out "$TMP/grades.json" --bin "$BIN" || true

verdict() { python3 -c '
import json,sys
rows = json.load(open(sys.argv[1]))["rows"]
print(next((r["verdict"] for r in rows if r["name"] == sys.argv[2]), "MISSING"))' "$TMP/grades.json" "$1"; }

echo
echo "== assertions"
g="$(verdict falsify-grades)";    want "a placement rule GRADES"                 "$([ "$g" = GRADES ] && echo 1 || echo 0)"    "got $g"
v="$(verdict falsify-vacuous)";   want "an acknowledgement-only check is VACUOUS" "$([ "$v" = VACUOUS ] && echo 1 || echo 0)"   "got $v"
u="$(verdict falsify-unvisited)"; want "a command-only check is UNVISITED"       "$([ "$u" = UNVISITED ] && echo 1 || echo 0)" "got $u"

# The negative that matters most: the vacuous check must NOT be reported as
# unvisited. Its control fired — the world under it really did change — and the
# only reason it stayed green is that it does not look. Conflating the two
# would send somebody to add a step where the repair is to widen `observe`.
want "the vacuous check's control did fire" \
    "$(python3 -c '
import json,sys
rows = json.load(open(sys.argv[1]))["rows"]
r = next(x for x in rows if x["name"] == "falsify-vacuous")
print(1 if r["fired"] else 0)' "$TMP/grades.json")" \
    "no control fired, so VACUOUS and UNVISITED are indistinguishable here"

echo
if [ "$fail" = 0 ]; then echo "CORPUS-GRADES-FALSIFY GREEN"; else echo "CORPUS-GRADES-FALSIFY RED" >&2; fi
exit "$fail"
