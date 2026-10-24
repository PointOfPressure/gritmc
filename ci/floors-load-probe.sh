#!/usr/bin/env bash
# floors-loadprobe.sh — is a floor conditional on the box being quiet?
#
# Every published floor in this project was harvested on an idle Threadripper
# at ~70 s a reference. This campaign is harvesting at ~175 s because three
# other lanes are recording gameplay corpora, load average ~95. A floor is a
# measurement of vanilla's OWN scheduler jitter, so the obvious worry is that
# it is really a measurement of the box's, and that every floor is conditional
# on a condition nobody recorded.
#
# The control: three more references of the seed-12345 r=16 window — the one
# window with forty quiet references already on disk — harvested NOW, under
# load. Same seed, same radius, same centre, same GEN_ORDER, same jar. The only
# thing that differs is the machine they were taken on. Their pairwise width
# against each other and against the quiet set is the whole answer.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT=/home/sayer/grit/work-floors
cd "$ROOT" || exit 2
DEST=/home/sayer/rustmc-floors/oracle-work
LOG=/home/sayer/grit/logs/floors-loadprobe.log
{
echo "=== LOAD PROBE, seed 12345 r=16 overworld @ $(date -Is) ==="
for lab in la lb lc; do
  port=$(( 26170 + $(printf '%d' "'${lab:1:1}") - 96 ))
  out="$DEST/floors-ow12345r16-load-$lab"
  [ -d "$out" ] && { echo "SKIP $lab"; continue; }
  la_before=$(cut -d' ' -f1 /proc/loadavg); t0=$(date +%s)
  echo "--- $lab port=$port load=$la_before"
  GEN_ORDER=rows "$ROOT/ci/gen-vanilla-region.sh" 12345 16 "$out" "$port" 64 64 overworld \
      > "$DEST/floors-ow12345r16-load-$lab.log" 2>&1
  echo "    rc=$? $(( $(date +%s) - t0 ))s regions=$(ls "$out"/*.mca 2>/dev/null | wc -l) load_after=$(cut -d' ' -f1 /proc/loadavg)"
done
echo "=== LOAD PROBE DONE @ $(date -Is) ==="
} >> "$LOG" 2>&1
