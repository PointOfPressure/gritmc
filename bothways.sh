#!/usr/bin/env bash
# Both-ways reference-cache measurement for the gate-throughput area brief.
# Runs the SAME branch (scratch-check/refcache-bothways, main + root-invariance
# fix a6072c51) through ci/handoff.sh --dry-run twice: once with the cache off
# (baseline) and once with RUSTMC_REFERENCE_CACHE=1 (cached), each in its own
# RUSTMC_REMOTE_DIR so the artefacts do not collide. Samples the corpus lock's
# internal split during each with ci/corpus-hold-sample.sh. Writes a summary
# comparing verdicts, timings and the recorded vanilla.json digests.
set -uo pipefail
cd /home/sayer/grit/bench-refcache-bw || exit 1
BR=refcache-bothways-bench
OUT=/home/sayer/grit/bench-refcache-bw-out/RESULTS.txt
: > "$OUT"

log() { printf '%s  %s\n' "$(date -Is)" "$*" | tee -a "$OUT"; }

log "=== baseline (no cache) starting, RUSTMC_REMOTE_DIR=rustmc-refcache-bw-A ==="
( setsid nohup ./ci/corpus-hold-sample.sh /home/sayer/grit/bench-refcache-bw-out/hold-baseline.tsv 400 15 \
    >/home/sayer/grit/bench-refcache-bw-out/hold-baseline.out 2>&1 & echo $! > /home/sayer/grit/bench-refcache-bw-out/hold-baseline.pid )
T0A=$(date +%s)
RUSTMC_REMOTE_DIR=rustmc-refcache-bw-A ./ci/handoff.sh "$BR" --dry-run \
    > /home/sayer/grit/bench-refcache-bw-out/baseline.log 2>&1
RCA=$?
T1A=$(date +%s)
kill "$(cat /home/sayer/grit/bench-refcache-bw-out/hold-baseline.pid)" 2>/dev/null
log "baseline rc=$RCA wall=$((T1A - T0A))s"
grep -E '^(GREEN|RED|REFUSED|INFRA|MERGED)' /home/sayer/grit/bench-refcache-bw-out/baseline.log | tee -a "$OUT"
grep -F 'stage 6' /home/sayer/grit/bench-refcache-bw-out/baseline.log | tee -a "$OUT"
ssh -n threadripper "sha256sum ~/rustmc-refcache-bw-A/oracle-work/gameplay/vanilla.json 2>/dev/null" | tee -a "$OUT"

log "=== cached (RUSTMC_REFERENCE_CACHE=1) starting, RUSTMC_REMOTE_DIR=rustmc-refcache-bw-B ==="
( setsid nohup ./ci/corpus-hold-sample.sh /home/sayer/grit/bench-refcache-bw-out/hold-cached.tsv 400 15 \
    >/home/sayer/grit/bench-refcache-bw-out/hold-cached.out 2>&1 & echo $! > /home/sayer/grit/bench-refcache-bw-out/hold-cached.pid )
T0B=$(date +%s)
RUSTMC_REMOTE_DIR=rustmc-refcache-bw-B RUSTMC_REFERENCE_CACHE=1 ./ci/handoff.sh "$BR" --dry-run \
    > /home/sayer/grit/bench-refcache-bw-out/cached.log 2>&1
RCB=$?
T1B=$(date +%s)
kill "$(cat /home/sayer/grit/bench-refcache-bw-out/hold-cached.pid)" 2>/dev/null
log "cached   rc=$RCB wall=$((T1B - T0B))s"
grep -E '^(GREEN|RED|REFUSED|INFRA|MERGED)' /home/sayer/grit/bench-refcache-bw-out/cached.log | tee -a "$OUT"
grep -F 'stage 6' /home/sayer/grit/bench-refcache-bw-out/cached.log | tee -a "$OUT"
grep -F 'reference-cache:' /home/sayer/grit/bench-refcache-bw-out/cached.log | tee -a "$OUT"
ssh -n threadripper "sha256sum ~/rustmc-refcache-bw-B/oracle-work/gameplay/vanilla.json 2>/dev/null" | tee -a "$OUT"

log "=== corpus-lock hold decomposition (cached run) ==="
tail -n 20 /home/sayer/grit/bench-refcache-bw-out/hold-cached.tsv | tee -a "$OUT"
log "=== corpus-lock hold decomposition (baseline run) ==="
tail -n 20 /home/sayer/grit/bench-refcache-bw-out/hold-baseline.tsv | tee -a "$OUT"

log "=== DONE rc_baseline=$RCA rc_cached=$RCB wall_baseline=$((T1A-T0A))s wall_cached=$((T1B-T0B))s ==="
