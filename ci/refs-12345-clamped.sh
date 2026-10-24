#!/usr/bin/env bash
# refs-12345-clamped.sh — the seed-12345 r=16 overworld ensemble, harvested
# under `-Dmax.bg.threads=1`.
#
# WHY THIS EXISTS
#
# `docs/DETERMINISM.md` §36 and §37: eight harvests of one r=2 window produced
# **five distinct worlds**, 0..536 blocks of 884,736, because `ChunkMap`
# dispatches every chunk status upgrade onto `Util.backgroundExecutor()` and
# features write one chunk out — so which chunk reaches FEATURES first is a
# race. Clamped to one background worker, four runs and six pairs came back
# **all zero**.
#
# That was measured at r=2. **Every reference ensemble in `oracle-work/`
# predates it**, so every floor this project has published — overworld
# 3,006/4,105, nether 48, End 252, and the 2,577..4,835 at N=5 that
# `ci/ow-r16-window-12345.sh` prints — is the floor of an ensemble whose
# members were free to disagree with each other for reasons that have nothing
# to do with us. This script builds the ensemble that lets that be checked at
# **r=16**, which is 272x the chunks and far more feature interaction than the
# window the clamp was established on.
#
# It does NOT touch `~/rustmc/oracle-work`. The existing 44 unclamped
# references of this exact window are the control arm and they are read-only:
# same seed, same centre, same radius, same bbox, same `GEN_ORDER=rows`, so the
# clamped and unclamped floors are directly comparable and neither is a
# reference the other overwrote. `REFS_DEST` defaults somewhere else entirely
# for that reason. The old gate fossilised its fixture by `rm -rf`ing shared
# state; this one cannot reach shared state at all.
#
# GEOMETRY, identical to `ci/refs-12345-ensemble.sh` on purpose: seed 12345,
# r=16, centre chunk (64,64), 33x33 = 1,089 chunks, bbox 48 48 80 80.
#
#   rows      (dump_order=zx)       k1 k2 k3 k4 k5 k6   ports 25961..25964, 25967..25968
#   rows_rev  (dump_order=zx_zrev)  kr-a kr-b           ports 25965..25966
#
# Four same-order references is the minimum that gives six pairs, which is what
# §37 used at r=2 and is the smallest N at which "all pairs zero" is a claim
# rather than a coincidence. The two `rows_rev` ones exist because §15 measured
# two order-varied references as worth more than a ninth same-order one, and
# because a clamp that only holds within one request order is a much weaker
# finding than one that holds across two.
#
# **k5 and k6 were added on the second pass and they are not padding.** k1..k4
# were harvested back to back in one eight-minute window on an otherwise quiet
# box. The mechanism the clamp is supposed to defeat is a **scheduling race**,
# and four runs taken under one set of scheduling conditions is the weakest
# possible sample of a scheduling property: it is exactly the shape of evidence
# that would look clean and generalise to nothing. k5 and k6 were taken hours
# later with the box under a load average near 15, so the rows ensemble spans
# two very different scheduler states and the pair matrix grows from 6 pairs to
# 15. A clamp that holds across both is a claim about the clamp; a clamp that
# holds only on a quiet box is a claim about the box.
#
# ONE PORT IS USED ONCE, EVER: `ci/gen-vanilla-region.sh` derives its scratch
# server directory from the port, so two runs sharing one collide. 25961..25966
# are disjoint from the 25931..25942 the unclamped ensemble used.
#
# THE CLAMP IS NOT ASSERTED, IT IS READ BACK OFF THE ARTEFACT. ORACLE rule 30:
# an unrecognised knob is not a turned one, and this project has already paid
# for a `RUSTMC_DECORATE_ORDER` that printed the order it had not used. So
# after each harvest this script greps `bg_threads=1` out of the reference's
# own `HARVEST-ORDER` — written by `ci/gen-vanilla-region.sh` from
# `VANILLA_JVM_OPTS` as the JVM actually saw it — and REFUSES the whole run if
# it is missing. A reference that cannot prove it was clamped is worse than no
# reference: it is an unclamped one wearing a clamped name.
#
# It also records **wall time per harvest**, which is the behavioural control.
# An unclamped r=16 harvest of this window is ~57 s on this box (23 workers);
# a clamped one puts all 1,089 chunks through one worker and cannot be. A
# clamped run that finishes in unclamped time is a flag that did not take.
#
# `GEN_POLL_ITERS` is why `ci/gen-vanilla-region.sh` grew that knob: its old
# fixed 150-iteration cap is ~5 minutes, which is ample unclamped and is not
# clamped, and a harvest killed by that cap exits 1 saying `generation timed
# out`, which reads as a broken window rather than a slow one.
#
# IDEMPOTENT BY REFUSAL, like the ensemble script: an existing reference
# directory is skipped and never regenerated, so a resume costs only what it
# has not done and nothing here can destroy a reference.
#
# usage: ci/refs-12345-clamped.sh
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

DEST=${REFS_DEST:-/home/sayer/grit/clamp-refs}
LOG=${REFS_LOG:-$HOME/rp-logs/refs-12345-clamped.log}
mkdir -p "$(dirname "$LOG")" "$DEST"

# label:port:GEN_ORDER
SPEC="k1:25961:rows
k2:25962:rows
k3:25963:rows
k4:25964:rows
kr-a:25965:rows_rev
kr-b:25966:rows_rev
k5:25967:rows
k6:25968:rows"

rc_all=0
{
  echo "=== seed-12345 r=16 overworld CLAMPED ensemble @ $(date -Is) ==="
  echo "    clamp: VANILLA_JVM_OPTS=-Dmax.bg.threads=1"
  echo "    dest:  $DEST   (NOT ~/rustmc/oracle-work — the unclamped control is there)"
  echo "    box:   $(hostname), $(nproc) threads"
  for line in $SPEC; do
    lab=${line%%:*}; rest=${line#*:}; port=${rest%%:*}; ord=${rest#*:}
    out="$DEST/gen-wide-12345-r16-ow-$lab"
    if [ -d "$out" ]; then
      echo "SKIP  $lab  exists at $out ($(head -1 "$out/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED))"
      continue
    fi
    echo "--- $lab port=$port GEN_ORDER=$ord -> $out"
    t0=$(date +%s)
    GEN_POLL_ITERS=1200 VANILLA_JVM_OPTS=-Dmax.bg.threads=1 GEN_ORDER="$ord" \
        "$ROOT/ci/gen-vanilla-region.sh" 12345 16 "$out" "$port" 64 64 overworld
    rc=$?
    secs=$(( $(date +%s) - t0 ))
    echo "    rc=$rc  ${secs}s  $(ls "$out"/*.mca 2>/dev/null | wc -l) region files"
    if [ "$rc" = 0 ]; then
      bg=$(grep -m1 -oP '^bg_threads=\K.*' "$out/HARVEST-ORDER" 2>/dev/null || echo missing)
      echo "    stamp: $(grep -m1 '^gen_order=' "$out/HARVEST-ORDER") bg_threads=$bg"
      if [ "$bg" != 1 ]; then
        echo "    REFUSING: $lab stamps bg_threads='$bg', not 1 — the clamp did not reach"
        echo "    the JVM, or the stamp does not record it. An unclamped reference under"
        echo "    a clamped name is the worst artefact this probe could produce."
        rc_all=2
        break
      fi
    else
      rc_all=1
    fi
  done
  echo "=== CLAMPED ENSEMBLE DONE rc=$rc_all @ $(date -Is) ==="
} > "$LOG" 2>&1
echo "log: $LOG rc=$rc_all"
exit $rc_all
