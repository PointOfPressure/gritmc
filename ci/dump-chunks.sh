#!/usr/bin/env bash
# dump-chunks.sh — capture real `play/level_chunk_with_light` bodies.
#
# The chunk packet is the last thing between a completed login and a client
# standing in a rustmc world, and it is the wrong shape to infer: its layout
# is a section buffer whose length is implied, palettes with three regimes and
# two different threshold tables, and light as bitsets plus nibble arrays.
# Two fields were only settled by having the bytes:
#
#   - `LevelChunkSection.write` writes TWO shorts, nonEmptyBlockCount then
#     fluidCount. A field list shows one obvious count and hides the other.
#   - the packed long arrays carry no length; it is implied by bitsPerEntry
#     (`writeFixedSizeLongArray`).
#
# Both desync everything after them while still parsing for a while, which is
# exactly the failure the join packet hit. So the bytes come from a real
# server, like the registries next door.
#
# The seed is fixed so the capture can be compared against our own worldgen
# for the same chunks. Output is Mojang-derived, so it is gitignored and lives
# outside the repo by default.
#
#   ci/dump-chunks.sh [out] [port] [seed] [settle_secs]
#
# Point RUSTMC_CHUNK_CAPTURE at the result to run
# crates/rustmc-server/tests/golden_chunk_packet.rs.
#
# SETTLE_SECS EXISTS BECAUSE VANILLA'S PACKET LIGHT LAGS ITS OWN BLOCKS.
# A chunk is serialised with whatever light the engine has finished, and
# `LevelChunk.setBlockState`'s repair runs on the light thread — so a chunk
# whose lava is still spreading goes out with the new blocks and the old
# light, and the correction follows as a `ClientboundLightUpdatePacket` that
# this dumper does not record. Measured 2026-09-05 by
# `crates/rustmc-server/tests/golden_chunk_light.rs` on two dumps from ONE
# server 120 s apart, same chunks: the block-light disagreement with our own
# engine falls from **5,261 nibbles to 55**, and the sky half does not move.
#
# So: a capture taken the instant the server boots grades the fluid ticks. It
# is fine for `golden_chunk_packet`, which only round-trips bytes, and it is
# not fine for anything that reads the light. Pass a settle time for that; the
# first connection is made and thrown away so the chunks are generated,
# ticked, and repaired before the dump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-/tmp/rustmc-chunks.bin}"
PORT="${2:-25591}"
SEED="${3:-31415926}"
SETTLE="${4:-0}"
DIR="$ROOT/oracle-work/gen-chunks-$PORT"

cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$DIR" >/dev/null 2>&1 || true; }
trap cleanup EXIT

rm -rf "$DIR"
"$ROOT/ci/vanilla-server.sh" start "$DIR" "$PORT" minecraft:normal "$SEED"
if [ "$SETTLE" -gt 0 ]; then
    CHUNK_DUMP="$(mktemp)" python3 "$ROOT/ci/regdump.py" 127.0.0.1 "$PORT" chunkwarm
    echo "settling ${SETTLE}s before the graded dump"
    sleep "$SETTLE"
fi
CHUNK_DUMP="$OUT" python3 "$ROOT/ci/regdump.py" 127.0.0.1 "$PORT" chunkbot
echo "chunks -> $OUT (seed $SEED, settle ${SETTLE}s)"
