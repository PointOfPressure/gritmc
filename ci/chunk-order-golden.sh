#!/usr/bin/env bash
# chunk-order-golden.sh — the chunk positions a real vanilla server sends a
# joining client, in the order it sends them.
#
#     ./ci/chunk-order-golden.sh
#
# Writes `crates/rustmc-server/golden/chunk_send_order.json`: the spawn chunk,
# the server's `view-distance`, and every `level_chunk_with_light` position in
# arrival order. Chunk *bodies* are not written — this is a fixture about
# geometry and order, and the bodies are the world.
#
# # What it is an oracle for
#
# Two claims that were otherwise readings of bytecode:
#
#   * **the set.** `ChunkTrackingView$Positioned(spawn, viewDistance)` is a
#     padded Euclidean disc, not a Chebyshev square. At `view-distance=4` the
#     disc is 117 chunks and the square is 81. The recorded set decides which
#     of the two this project ported, against a real server rather than
#     against a re-reading of the same `javap` output.
#   * **the order.** `PlayerChunkSender.collectChunksToSend` sorts by
#     `ChunkPos.distanceSquared` from the player's chunk, ascending. The sort
#     is **per batch**, over whatever is pending at that tick, so the recorded
#     sequence is a concatenation of ascending runs and is *not* globally
#     monotone: `ChunkMap.onChunkReadyToSend` queues a chunk when it finishes
#     generating, so a nearer chunk can enter the queue after a farther one
#     has already gone out. Measured on two fresh superflat worlds: 117 chunks
#     in **11 runs** and in **10 runs**, with different centres — vanilla's
#     superflat spawn wanders between worlds, and how many batches the
#     generation takes is a race. The *set* is 117 both times and the runs are
#     ascending both times; those are the durable claims and the run count is
#     recorded rather than asserted exactly.
#
# # What it is deliberately NOT an oracle for
#
# **The exact sequence.** Batch boundaries here depend on when each chunk
# finished generating, which is a race, so the fixture is compared as a *set*
# plus a *monotonicity* property. The order among chunks at equal
# `distanceSquared` — `pendingChunks`' `LongOpenHashSet` spliterator order,
# through Guava's `TopKSelector` when throttled — is ported and gated exactly
# by `ci/chunk-send-ties-golden.sh`, which drives the jar's own
# `collectChunksToSend` with no server and no race.
#
# # Why the capture is safe to commit where a region file is not
#
# It holds chunk coordinates and nothing else: no block states, no NBT, no
# bytes off the wire. It is the same kind of artefact as `data/vanilla/*.json`
# — a measurement derived from vanilla, not a copy of it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${CHUNK_ORDER_PORT:-25697}"
WORK="$ROOT/oracle-work/chunk-order-golden"
OUT="${1:-$ROOT/crates/rustmc-server/golden/chunk_send_order.json}"

cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$WORK" >/dev/null 2>&1 || true; }
trap cleanup EXIT

rm -rf "$WORK"
VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful' \
    "$ROOT/ci/vanilla-server.sh" start "$WORK" "$PORT" "minecraft:flat"

VIEW="$(grep -oP '^view-distance=\K[0-9]+' "$WORK/server.properties")"
[ -n "$VIEW" ] || { echo "no view-distance in $WORK/server.properties" >&2; exit 1; }

CAP="$WORK/capture.bin"
CHUNK_DUMP="$CAP" python3 ci/regdump.py 127.0.0.1 "$PORT" chunkbot > "$WORK/regdump.log" 2>&1 || {
    echo "regdump failed:"; tail -20 "$WORK/regdump.log"; exit 1; }
grep -E "^captured |^  chunk x" "$WORK/regdump.log" || true

python3 - "$CAP" "$OUT" "$VIEW" <<'PYEOF'
import json, struct, sys

cap, out, view = sys.argv[1], sys.argv[2], int(sys.argv[3])
data = open(cap, "rb").read()
assert data[:8] == b"RMCCHK01", "not a chunk capture"
n = int.from_bytes(data[8:12], "big")
i, order = 12, []
for _ in range(n):
    cx, cz, ln = struct.unpack(">iiI", data[i:i + 12])
    i += 12 + ln
    order.append([cx, cz])
assert i == len(data), "capture has trailing bytes"
assert order, "no chunks captured"

# The centre: `set_chunk_cache_center` is not in the capture, so it is taken
# as the chunk of the FIRST packet — which is sound exactly because the order
# is by distance from the player and 0 is the unique minimum.
centre = order[0]
d2 = lambda c: (c[0] - centre[0]) ** 2 + (c[1] - centre[1]) ** 2

fail = []
if len(set(map(tuple, order))) != len(order):
    fail.append("a chunk arrived twice")
keys = [d2(c) for c in order]
# Descents are batch boundaries, not defects: each batch sorts what is pending
# at its own tick. What must hold is that every maximal run is ascending —
# which it is by construction of the run decomposition — and that there are
# few of them. A server ordering by anything but distance would produce a run
# per chunk, or close to it.
descents = [k for k in range(1, len(keys)) if keys[k] < keys[k - 1]]
runs = []
start = 0
for k in descents + [len(keys)]:
    runs.append(k - start)
    start = k
if order[0] != centre:
    fail.append(f"the first chunk is {order[0]}, not the centre {centre} — "
                "distanceSquared 0 is the unique minimum")
if len(runs) > 20:
    fail.append(f"{len(runs)} ascending runs over {len(order)} chunks — a batch sort by "
                "distanceSquared cannot need that many; the comparator is not distance")
if max(runs) < 5:
    fail.append(f"the longest ascending run is {max(runs)} — a sorted batch of "
                "floor(batchQuota) chunks should be much longer than that")
print(f"  {len(runs)} ascending runs (batches), lengths {runs}")

# The set the port predicts, computed here as well as in Rust so that the
# fixture is refused rather than merely recorded when the two disagree.
def contains(c, pad=2):
    dx = max(0, abs(c[0] - centre[0]) - pad)
    dz = max(0, abs(c[1] - centre[1]) - pad)
    return dx * dx + dz * dz < view * view
disc = {(x, z)
        for x in range(centre[0] - view - 1, centre[0] + view + 2)
        for z in range(centre[1] - view - 1, centre[1] + view + 2)
        if contains((x, z))}
square = {(centre[0] + dx, centre[1] + dz)
          for dx in range(-view, view + 1) for dz in range(-view, view + 1)}
got = set(map(tuple, order))
print(f"  captured {len(order)} chunks around {centre} at view-distance={view}")
print(f"  padded disc predicts {len(disc)}, a Chebyshev square predicts {len(square)}")
if got != disc:
    fail.append(f"the captured set is not the padded disc: "
                f"{len(disc - got)} predicted and absent, {len(got - disc)} sent and unpredicted"
                + (f"; it IS the Chebyshev square" if got == square else ""))

if fail:
    for f in fail:
        print(f"  !! {f}", file=sys.stderr)
    sys.exit(1)

# Written by hand rather than `json.dump(indent=...)`: an indented dump puts
# every coordinate of every pair on its own line, and the Rust side has no
# JSON dependency to hide that behind. One pair per line is both valid JSON
# and greppable.
with open(out, "w") as f:
    f.write("{\n")
    f.write(' "note": "level_chunk_with_light positions from a real vanilla 26.2 '
            'superflat server, in arrival order, by ci/chunk-order-golden.sh. '
            'Positions only: no block data.",\n')
    f.write(f' "view_distance": {view},\n')
    f.write(f' "centre": [{centre[0]}, {centre[1]}],\n')
    f.write(f' "count": {len(order)},\n')
    f.write(f' "runs": {json.dumps(runs)},\n')
    f.write(' "order": [\n')
    f.write(",\n".join(f"  [{cx}, {cz}]" for cx, cz in order))
    f.write("\n ]\n}\n")
print(f"chunk send order -> {out} ({len(order)} positions in {len(runs)} ascending runs)")
PYEOF
