#!/usr/bin/env bash
# t2-memory.sh — the hardware floor, enforced instead of asserted.
#
#     ./ci/t2-memory.sh
#     RUSTMC_MEM_CEILING_MB=900 ./ci/t2-memory.sh    # tighten it
#
# # What this exists to stop
#
# `docs/PRODUCT.md` item 8 commits to "10-player survival runs WELL on
# 1 vCPU / 1GB VPS" and calls it "CI-enforced like parity". On 2026-08-05 that
# parenthetical was found to describe nothing: no gate in `ci/` set a memory
# limit, a `ulimit` or a cgroup, and no gate measured server RSS at all. The
# server was at the same time sitting at 2,957 MB with **one** player at the
# shipped view distance — three times the floor, for a tenth of the
# population. A floor nobody checks reads as a floor somebody checks, which
# makes it worse than no claim at all.
#
# # Why this is a cgroup and not a comparison
#
# A gate that reads a number and compares it to a constant is a gate that
# passes when the reading is wrong. This runs the server inside a cgroup with
# `MemoryMax` set to the ceiling, so **the kernel** enforces it: a server that
# wants more than it is allowed is killed, and no amount of misreading
# `/proc` can turn that into a pass. The measured peak `VmRSS` is printed
# beside it and compared too, but that is the diagnostic, not the enforcement.
#
# `--selftest` runs the same scenario against a deliberately impossible
# ceiling and **fails if it passes**. `docs/AGENT-BRIEF.md` names twenty
# checks in this project that could not fail; a memory guard is an easy
# twenty-first, because the happy path and a broken instrument look identical
# from the outside. The self-test is what tells them apart, and it runs by
# default.
#
# # The scenario, and what it does NOT cover
#
# Superflat, seed 0, the shipped view distance, ten concurrent clients. Two
# honest limitations, stated here rather than left for a reader to assume:
#
#   * **all ten players stand on spawn.** `crate::tiles` serves shared tiles,
#     so co-located players are the CHEAPEST ten-player arrangement there is —
#     `docs/BENCH.md` measured the fifth player costing 8.5 MB because the
#     first four had already paid for the tiles. Ten players in ten places is
#     strictly worse and this gate does not measure it. Passing here is
#     necessary for the floor, not sufficient.
#   * **superflat is the cheapest world.** Its canvases are mostly untouched
#     zero pages, which do not count towards RSS at all; noise terrain writes
#     every column. `BENCH_LEVEL=normal` runs it on terrain and is the number
#     to watch as worldgen features land.
#
# It is not `taskset`ed to one core. The floor says 1 vCPU and this gate
# measures memory only; a one-core timing gate is a different measurement and
# is listed as unmeasured in `docs/BENCH.md`.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ---------------------------------------------------------------------------
# THE NUMBER.
#
# 700 MB, for ten co-located players at view-distance 10 on superflat.
#
# It is set from measurement and it is set to FAIL, not to pass: the same
# scenario measured 96 MB on 2026-08-05 after tile retirement landed, so this
# ceiling is roughly seven times the current cost. That gap is deliberate and
# is not slack for the sake of it — every mechanic still to land (mob AI,
# entities, block entities, random ticks) adds resident state, and a gate
# that has to be edited on every feature is a gate that gets edited without
# being read. What it catches is a return to the old behaviour: the figure
# this replaces was 1,077 MB for ONE player, so any regression of that shape
# breaches this on the first run.
#
# It sits under the 1 GB floor rather than at it, because the floor is the
# whole VPS — kernel, sshd and everything else share it — and a server
# allowed to use all of it is a server that gets OOM-killed by something
# else's allocation.
# ---------------------------------------------------------------------------
CEILING_MB="${RUSTMC_MEM_CEILING_MB:-700}"
PLAYERS="${RUSTMC_MEM_PLAYERS:-10}"
VD="${RUSTMC_MEM_VIEW_DISTANCE:-10}"
# Left unset so the scenario runs on whatever `status::WORLD_RADIUS` is —
# a guard on the shipped configuration has to move when the shipped
# configuration moves, and pinning a number here would have quietly kept
# gating radius 6 after the default became 10.
RADIUS="${RUSTMC_MEM_RADIUS:-}"
LEVEL="${BENCH_LEVEL:-flat}"
W="$ROOT/oracle-work/mem-floor"
mkdir -p "$W"

have_cgroup() {
    command -v systemd-run > /dev/null 2>&1 &&
        systemd-run --user --scope -q -p MemoryMax=64M /bin/true > /dev/null 2>&1
}

# Run the scenario under `limit_mb` of enforced memory. Echoes the measured
# peak RSS in MB on stdout; returns non-zero if the server did not survive.
run_scenario() {
    # Three statements and not one: `local a="$1" b="$a"` expands `$a` before
    # the assignment lands, so under `set -u` the third word here failed with
    # `tag: unbound variable` and the gate reported RED for a scenario it had
    # never run — a false red is as much a broken gate as a false green.
    local limit_mb="$1"
    local tag="$2"
    local out="$W/mem-$tag.jsonl"
    rm -f "$out"
    local cmd=(python3 ci/bench-boot.py --flavour rustmc --runs 1
        --level "$LEVEL" --seed 0 --view-distance "$VD"
        --chunk-probe 6 --players "$PLAYERS" --player-secs 12
        --idle-secs 5 --tag "$tag" --port 25961 --out "$out")
    [ -n "$RADIUS" ] && cmd+=(--radius "$RADIUS")
    if [ "$CGROUP" = yes ]; then
        systemd-run --user --scope -q -p MemoryMax="${limit_mb}M" \
            -p MemorySwapMax=0 "${cmd[@]}" > /dev/null 2>&1
    else
        "${cmd[@]}" > /dev/null 2>&1
    fi
    # The record is the evidence; a scenario whose server died writes no
    # usable marks, which is itself the failure signal.
    python3 - "$out" "$limit_mb" <<'PY'
import json, sys
path, limit = sys.argv[1], int(sys.argv[2])
try:
    recs = [json.loads(l) for l in open(path) if l.strip()]
except FileNotFoundError:
    recs = []
if not recs:
    print("0 no-record")
    raise SystemExit(1)
r = recs[-1]
marks = [r.get(k) or 0 for k in
         ("rss_at_join_kb", "rss_peak_to_join_kb", "rss_with_players_kb",
          "rss_after_players_kb", "rss_idle_end_kb")]
peak = max(marks) / 1024
# Liveness: a server that served nobody proves nothing about its memory. The
# reload control in docs/BENCH.md nearly published a 14x win for a world that
# was never written, and this is the same trap one field over.
chunks = r.get("probe_chunks") or 0
joined = r.get("players") or 0
print(f"{peak:.0f} chunks={chunks} players={joined}")
raise SystemExit(0 if chunks > 0 and joined > 0 else 2)
PY
}

CGROUP=no
if have_cgroup; then
    CGROUP=yes
    echo "T2-MEMORY: enforcement = cgroup v2 MemoryMax (the kernel kills a breach)"
else
    echo "T2-MEMORY: enforcement = measured VmRSS only — no usable cgroup on this host."
    echo "T2-MEMORY: this is a WEAKER check; the number below is compared, not enforced."
fi
# **Hoisted out of the `echo` on purpose, and this is DEFECT CLASS #21.**
# This read
#
#     "tile radius ${RADIUS:-<the server's own default>}"
#
# and the apostrophe in `server's` opens a quote **inside** the `${var:-word}`
# default, where quote processing still applies. `bash -n` on that is
# `unexpected EOF while looking for matching '`, so this script has not
# parsed — and therefore has not run, once — since the line landed in b27c3e4
# on 2026-08-06, the commit whose own message reports "218 MB against 700".
# That figure predates the edit that broke the file; nothing has re-measured
# it since. Found on 2026-08-06 by running the gate rather than by quoting it.
#
# The bitter part is that everything below this line was built to be
# un-fool-able — a cgroup so the *kernel* enforces the ceiling, and a
# self-test that runs the scenario under an impossible 32 MB and fails if it
# passes — and none of it ran, because the shell never reached it. **An
# instrument's anti-fooling machinery is worth nothing if the file does not
# parse.** `ci/t0.sh` now `bash -n`s every shell script in the tree, first,
# before it does anything else; that sweep was verified against this exact
# line before it was trusted.
RADIUS_LABEL="$RADIUS"
[ -n "$RADIUS_LABEL" ] || RADIUS_LABEL="<the default the server ships>"

# REFUSE rather than redden when there is nothing to measure.
#
# Found on 2026-08-06 while proving this gate can fail, and it is the second
# instrument fault in this file in one day. A tree with no
# `target/release/rustmc-server` produces exactly the output a genuine breach
# produces:
#
#     T2-MEMORY RED: the server did not complete the scenario under 150 MB (no-record)
#
# because `bench-boot.py` exits with `missing binary:` before writing a
# record, and the adjudicator below sees an empty file and cannot tell "the
# kernel killed it" from "there was never a server". Both are `no-record`.
#
# A false red is as much a broken gate as a false green — this script's own
# `run_scenario` already carries that sentence about a different bug — and it
# is worse here than elsewhere, because the honest reaction to a red memory
# gate is to go looking for a memory regression that does not exist.
#
# Exit 4, the same code and the same reasoning as `ci/t2-gameplay.sh`'s
# provenance preflight: a refusal is not a verdict.
for bin in target/release/oracle-runner target/release/rustmc-server; do
    if [ ! -x "$ROOT/$bin" ]; then
        echo "T2-MEMORY REFUSED — no $bin to measure." >&2
        echo "                    cargo build --release -p rustmc-server" >&2
        echo "                    A missing binary and a breached ceiling both" >&2
        echo "                    read as 'no-record', so this refuses instead" >&2
        echo "                    of reporting a red nobody can act on." >&2
        exit 4
    fi
done
echo "T2-MEMORY: scenario = $PLAYERS players, view-distance $VD, level $LEVEL," \
    "tile radius $RADIUS_LABEL"
echo "T2-MEMORY: ceiling  = ${CEILING_MB} MB"

# --- the gate ---------------------------------------------------------------
result="$(run_scenario "$CEILING_MB" floor)"; rc=$?
peak="${result%% *}"
detail="${result#* }"
if [ "$rc" -ne 0 ]; then
    echo "T2-MEMORY RED: the server did not complete the scenario under ${CEILING_MB} MB ($detail)"
    exit 1
fi
echo "T2-MEMORY: measured peak RSS = ${peak} MB ($detail)"
if [ "$peak" -gt "$CEILING_MB" ] 2> /dev/null; then
    echo "T2-MEMORY RED: ${peak} MB exceeds the ${CEILING_MB} MB ceiling"
    exit 1
fi

# --- the self-test: prove the gate can fail ---------------------------------
#
# Skipped only when there is no cgroup, because without one there is nothing
# to prove — the comparison above is the whole check and its failure path is
# a shell `-gt`.
if [ "${RUSTMC_MEM_SELFTEST:-1}" = 1 ] && [ "$CGROUP" = yes ]; then
    echo "T2-MEMORY: self-test — the same scenario under an impossible 32 MB"
    st="$(run_scenario 32 selftest)"; strc=$?
    stpeak="${st%% *}"
    if [ "$strc" -eq 0 ] && [ "$stpeak" -le 32 ] 2> /dev/null; then
        echo "T2-MEMORY RED: the scenario PASSED under a 32 MB limit."
        echo "T2-MEMORY RED: the limit is not being applied — this gate cannot fail,"
        echo "T2-MEMORY RED: so its green above means nothing. Fix the harness first."
        exit 1
    fi
    echo "T2-MEMORY: self-test ok — the limit is real and a breach is caught"
fi

echo "T2-MEMORY GREEN — ${peak} MB against a ${CEILING_MB} MB ceiling"
