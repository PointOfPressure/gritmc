#!/usr/bin/env bash
# chunk-batch-probe.sh — how many chunks does vanilla actually send us?
#
# The gameplay oracle's broadcast channel was intermittent: whether a block
# change reached a given client varied run to run with no change to either
# server. This probe is the measurement that explains it.
#
# `PlayerChunkSender` is constructed with `maxUnacknowledgedBatches = 1`.
# `sendNextChunks` returns immediately while
# `unacknowledgedBatches >= maxUnacknowledgedBatches`, and only
# `onChunkBatchReceivedByClient` — the handler for the serverbound
# `chunk_batch_received` packet — decrements that counter and raises the cap
# to 10. So a client that never acknowledges a batch is sent **one batch and
# then nothing**, for the rest of the session.
#
# That matters far beyond chunk loading, because
# `ChunkMap.isChunkTracked(player, x, z)` is
#
#     player.getChunkTrackingView().contains(x, z)
#         && !player.connection.chunkSender.isPending(ChunkPos.pack(x, z))
#
# and `ChunkHolder.broadcastChanges` sends to `getPlayers(pos, false)`, which
# filters on exactly that. A chunk still queued for delivery is a chunk whose
# block changes are silently withheld — and the same predicate gates entity
# pairing in `ChunkMap.TrackedEntity.updatePlayer`.
#
#     ./ci/chunk-batch-probe.sh
#
# Prints the chunk and batch counts a client sees with the acknowledgement
# and without it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${CHUNK_PROBE_PORT:-25698}"
WORK="$ROOT/oracle-work/chunk-batch-probe"
STAY="${CHUNK_PROBE_STAY:-8}"

cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$WORK" >/dev/null 2>&1 || true; }
trap cleanup EXIT

cargo build -q -p oracle-runner --release
BIN="$ROOT/target/release/oracle-runner"

rm -rf "$WORK"
VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful' \
    "$ROOT/ci/vanilla-server.sh" start "$WORK" "$PORT" "minecraft:flat"
sleep 1

echo "== view-distance the server admits to:"
grep -E '^(view-distance|simulation-distance)' "$WORK/server.properties" || true

echo "== without the acknowledgement (every trace before 2026-08-03)"
ORACLE_NO_CHUNK_ACK=1 "$BIN" join "127.0.0.1:$PORT" NoAck --stay-secs "$STAY"

echo "== with the acknowledgement"
"$BIN" join "127.0.0.1:$PORT" WithAck --stay-secs "$STAY"

"$ROOT/ci/vanilla-server.sh" stop "$WORK"
