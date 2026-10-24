#!/usr/bin/env bash
# dump-registries.sh — capture the 29 synchronised registries from vanilla.
#
# A client will not render a world until it has all of them, and their
# contents are datapack data rather than code — the same category as
# everything else under data/vanilla/, which this project extracts rather
# than reimplements.
#
# Building them from the datapack JSON would mean re-deriving NBT types that
# JSON does not carry (is `ambient_light` a float or a double?) for hundreds
# of fields, every one a guess unless read from a codec. So the bytes come
# from a real server: this starts one, completes login and configuration as a
# client, and writes every registry_data body verbatim.
#
# Regenerate on a version bump. The output is gitignored.
#
# **THIS OUTPUT IS NOT REPRODUCIBLE, AND THAT IS VANILLA'S, NOT OURS.**
# Measured 2026-08-03 on the Threadripper: four dumps of the same jar, same
# world, same everything, produced four distinct md5s and all four were
# **exactly 121,847 bytes**. `cmp -l` between two of them is 1,235 bytes in one
# region from offset 2092, and the region reads as text — two entries of a
# biome's `attributes`, `minecraft:visual/water_fog_color` and
# `minecraft:visual/sky_color`, swapping places. An unordered collection being
# serialised inside vanilla's own registry encoding. This script writes each
# body verbatim, so there is nothing here to pin.
#
# Three things follow, and the first one is why this comment exists:
#
#   * **a `registries.bin` that differs from another copy is not stale.** The
#     canonical seed's copy was read as stale on 2026-08-03 until it was dumped
#     four times. Do not spend a turn on it.
#   * the provenance stamp is still self-consistent — `md5=` pins a file to its
#     own stamp, taken at generation — but what it certifies is "this file has
#     not been swapped", not "this is what the generator produces".
#   * **armed**: we replay these bytes to a client while a live vanilla server
#     re-derives the order on every boot, so any future check that compares
#     configuration-phase bytes is unmatchable for a reason with nothing to do
#     with this server. Nothing observes the configuration phase today.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/registries.bin}"
PORT="${2:-25590}"
DIR="$ROOT/oracle-work/gen-registries-$PORT"

# **ONE DUMP PER PORT ON THE BOX AT A TIME — 2026-09-24.** Every tree's
# `ci/gen-gameplay-tables.sh` calls this with the default port, and the gate
# does too, so two table generations at once — any two lanes, or a lane and
# the merge gate — met at :25590 and the second died with `Address already in
# use`, which voided a world-lane T0 that morning for a reason with nothing to
# do with its branch. The directory is per tree; the port is not. So the whole
# start -> dump -> stop sequence runs under a box-wide `flock` keyed on the
# port, and a caller that finds it held says so and waits, bounded by
# RUSTMC_GEN_REGISTRIES_WAIT seconds (default 900; a dump is one vanilla
# boot). The lock is released when this process exits, after the EXIT trap
# has stopped the server, so the next caller never sees the port still bound.
LOCK="${RUSTMC_GEN_REGISTRIES_LOCK:-$HOME/.rustmc-gen-registries-$PORT.lock}"
WAIT="${RUSTMC_GEN_REGISTRIES_WAIT:-900}"
exec 9>>"$LOCK"
if ! flock -n 9; then
    echo "dump-registries: :$PORT is held by another table generation — waiting up to ${WAIT}s" >&2
    echo "                 holder: $(cat "$LOCK.holder" 2>/dev/null || echo unknown)" >&2
    if ! flock -w "$WAIT" 9; then
        echo "dump-registries: REFUSED — $LOCK still held after ${WAIT}s; nothing was started." >&2
        exit 2
    fi
    echo "dump-registries: got $LOCK" >&2
fi
echo "pid=$$ tree=$ROOT since=$(date -Is)" > "$LOCK.holder"

cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$DIR" >/dev/null 2>&1 || true; }
trap cleanup EXIT

rm -rf "$DIR"
"$ROOT/ci/vanilla-server.sh" start "$DIR" "$PORT" 12345 default
REGDUMP_OUT="$OUT" python3 "$ROOT/ci/regdump.py" 127.0.0.1 "$PORT" Probe
echo "registries -> $OUT"
