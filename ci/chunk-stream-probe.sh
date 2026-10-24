#!/usr/bin/env bash
# chunk-stream-probe.sh — how many chunks does *this* server send, and does it
# stop where vanilla stops?
#
#     ./ci/chunk-stream-probe.sh
#
# `ci/chunk-batch-probe.sh` asks the same question of real vanilla and its
# header records the answer: at `view-distance=4`, a client that never sends
# `chunk_batch_received` is given **9 chunks and then nothing** for the rest
# of the session, and a conforming client is given **117**. This script asks
# it of rustmc-server, and asserts both numbers rather than printing them.
#
# # Why 117 is the sharp one
#
# 117 is not a round number and it is not `(2*4+1)^2 = 81`. It is the size of
# `ChunkTrackingView$Positioned(centre, 4)`: the padded Euclidean disc
# `max(0,|dx|-2)^2 + max(0,|dz|-2)^2 < 16` over a bounding box of `4 + 1`,
# which is the whole 11x11 box less its four corners. This server sent 81
# until 2026-09-05, because `world_send::ChunkView` walked a Chebyshev square.
# So the count alone separates the two implementations, against a vanilla
# measurement taken a month before the port existed.
#
# # Why the no-ack number is 25 + something here and 9 in vanilla
#
# `enter_world` writes the spawn tile — 25 chunks at `--radius 2` — directly
# onto the socket before the play loop exists, as one unbatched blob, and only
# then hands the view to the chunk sender. So a client that never
# acknowledges sees that blob plus the sender's **one** permitted batch, where
# vanilla sends one batch and nothing else. The stall is reproduced; the join
# burst is not. That is the one named gap in
# `crates/rustmc-server/src/world_send.rs`'s `ChunkView::sent`.
#
# That one batch is at most `floor(START_CHUNKS_PER_TICK) = 9` and is usually
# fewer, and the reason is worth stating because it looks like a defect: at
# `--radius 2` only the 25-chunk spawn tile is generated, and the throttled
# branch of `collectChunksToSend` selects the nearest `quota` **positions**
# before it filters out the ones that are not there yet. An ungenerated chunk
# therefore spends a slot and is not backfilled — vanilla's own arithmetic —
# so the batch comes back short and the missing chunks are retried next tick.
# The bound is asserted; the exact figure is a property of how much world this
# probe generated and is not.
#
# # What makes this able to go red
#
# Four independent assertions, each with a stated other-value:
#
#   * no-ack chunks in `26..=34`. 117 means `chunk_batch_received` is ignored
#     entirely; exactly 25 means the sender never ran at all, which is what
#     this probe measured before `keep_alive_loop` called
#     `update_chunk_tracking` on join; above 34 means one batch exceeded
#     `floor(START_CHUNKS_PER_TICK)`.
#   * no-ack batches == 2. Three or more means the cap is not 1.
#   * ack chunks == 117. A Chebyshev square reads 81; a square of the
#     bounding box reads 121.
#   * ack batches > 2. Equal means the acknowledgement changed nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/ci/server-ready.sh"

OUR_PORT="${CHUNK_STREAM_PROBE_PORT:-25699}"
STAY="${CHUNK_STREAM_PROBE_STAY:-8}"
VIEW="${CHUNK_STREAM_PROBE_VIEW:-4}"
WORK="$ROOT/oracle-work/chunk-stream-probe"

export PATH="$HOME/.cargo/bin:$PATH"
cargo build -q -p oracle-runner -p rustmc-server --release
BIN="$ROOT/target/release/oracle-runner"

rm -rf "$WORK"
mkdir -p "$WORK"
./target/release/rustmc-server run --port "$OUR_PORT" --level-type flat --radius 2 \
    --view-distance "$VIEW" >"$WORK/rustmc.log" 2>&1 &
RUSTMC_PID=$!
cleanup() { kill "$RUSTMC_PID" 2>/dev/null || true; }
trap cleanup EXIT

# Two arms, the same pair `ci/t2-gameplay.sh` uses and for the same reasons:
# the post-bind line naming the port we asked for, and the kernel's word that
# the LISTEN socket on it is this pid's. A timeout is a failure, not a
# fall-through — a probe that measured a server that never came up would
# report zero chunks and read as a total regression.
#
# **The variable names are not free.** `ci/readiness-falsify.sh` `eval`s this
# very line against real not-ready servers, supplying `WORK`, `OUR_PORT`,
# `RUSTMC_PID` and `RUSTMC_READY` — so a proof spelled with any other names
# is not transcribed, it is simply not exercised, and `set -u` turns that into
# an unbound-variable error the harness reports as a failure of this script.
RUSTMC_READY=0
for _ in $(seq 1 120); do
    grep -q "listening on 0.0.0.0:$OUR_PORT[ ,]" "$WORK/rustmc.log" 2>/dev/null && ready_socket_owner "$OUR_PORT" "$RUSTMC_PID" && { RUSTMC_READY=1; break; }
    kill -0 "$RUSTMC_PID" 2>/dev/null || { echo "rustmc died — tail:"; tail -20 "$WORK/rustmc.log"; exit 1; }
    sleep 1
done
[ "$RUSTMC_READY" = 1 ] || { echo "rustmc-server never proved it was listening on :$OUR_PORT"; tail -20 "$WORK/rustmc.log"; exit 2; }

counts() { # $1 = label, sets CHUNKS and BATCHES
    local line
    line="$(grep -E '^JOIN OK  chunks=' "$1" || true)"
    [ -n "$line" ] || { echo "no chunk count in $1 — the join never completed"; cat "$1"; exit 1; }
    CHUNKS="$(sed -E 's/.*chunks=([0-9]+).*/\1/' <<<"$line")"
    BATCHES="$(sed -E 's/.*batches=([0-9]+).*/\1/' <<<"$line")"
}

echo "== without the acknowledgement (ORACLE_NO_CHUNK_ACK=1)"
ORACLE_NO_CHUNK_ACK=1 "$BIN" join "127.0.0.1:$OUR_PORT" NoAck --stay-secs "$STAY" | tee "$WORK/noack.txt"
counts "$WORK/noack.txt"; NOACK_CHUNKS=$CHUNKS; NOACK_BATCHES=$BATCHES

echo "== with the acknowledgement"
"$BIN" join "127.0.0.1:$OUR_PORT" WithAck --stay-secs "$STAY" | tee "$WORK/ack.txt"
counts "$WORK/ack.txt"; ACK_CHUNKS=$CHUNKS; ACK_BATCHES=$BATCHES

echo
echo "view-distance=$VIEW  radius=2"
echo "  no ack: chunks=$NOACK_CHUNKS batches=$NOACK_BATCHES"
echo "  ack:    chunks=$ACK_CHUNKS batches=$ACK_BATCHES"

FAIL=0
want() { # $1 name, $2 got, $3 want, $4 what-else-would-mean
    if [ "$2" != "$3" ]; then
        echo "  !! $1 = $2, expected $3 — $4" >&2
        FAIL=1
    fi
}
if [ "$NOACK_CHUNKS" -le 25 ] || [ "$NOACK_CHUNKS" -gt 34 ]; then
    echo "  !! no-ack chunks = $NOACK_CHUNKS, expected 26..34 — 25 means the sender never ran, 117 means the acknowledgement is ignored, above 34 means one batch beat floor(START_CHUNKS_PER_TICK)" >&2
    FAIL=1
fi
want "no-ack batches" "$NOACK_BATCHES" 2 \
    "more than 2 means maxUnacknowledgedBatches did not start at 1"
want "ack chunks" "$ACK_CHUNKS" 117 \
    "81 is a Chebyshev square of radius 4; 121 is its whole bounding box"
if [ "$ACK_BATCHES" -le "$NOACK_BATCHES" ]; then
    echo "  !! ack batches $ACK_BATCHES is not more than no-ack $NOACK_BATCHES — the acknowledgement changed nothing" >&2
    FAIL=1
fi
[ "$FAIL" = 0 ] || { echo "CHUNK-STREAM-PROBE RED"; exit 1; }
echo "CHUNK-STREAM-PROBE GREEN — $NOACK_CHUNKS/$NOACK_BATCHES without the acknowledgement, 117/$ACK_BATCHES with it"
