#!/usr/bin/env bash
# ci/sweep-tip.sh — one recording, one sweep, for THIS tree's new scenarios.
#
# ===========================================================================
# THE PROBLEM THIS SOLVES, WHICH IS NOT THE GATE
# ===========================================================================
#
# `ci/corpus-falsifiable.py`'s own header states the split:
#
#     --sweep    needs recordings; produces the ledger; seconds; no lock
#     --check    needs nothing; refuses on the ledger; this is what T0 runs
#
# `--check` refuses `UNREGISTERED` — a scenario file with no ledger entry — so
# **a new gameplay scenario cannot pass the tree checks until it appears in some
# recording.** The sweep is free. The RECORDING is not: it comes from
# `ci/t2-gameplay.sh`, which takes the box-wide corpus lock and runs for ~35-40
# minutes, and the box runs one at a time.
#
# So the corpus lock is on the critical path for **adding** a scenario, not only
# for gating one, and that is by design and correct. `MAX_UNSWEPT` exists as the
# bounded escape hatch precisely because "the recordings on the box lag the
# corpus by a scenario or two at any moment, and a gate that cannot land is not
# a gate" — and that file says outright that growing the allowance is a reviewed
# code change and not something an agent does while adding the check the
# allowance would hide. **Do not raise `MAX_UNSWEPT`.** This script is the other
# way out.
#
# THE LEVER IS FEWER, BIGGER RECORDINGS. A recording covers the scenarios
# present in the tree it runs against. Today each agent queues its own run for
# its own branch's scenarios, so four agents pay four lock acquisitions of ~40
# minutes. **On one tree containing all of their scenarios, one recording covers
# all of them at once.** That is what this script does for a single tree, and
# what `ci/batch.sh --sweep` does with it over an integration tip carrying N
# members' scenarios.
#
# ===========================================================================
# TWO REFUSALS THAT ARE THE WHOLE POINT
# ===========================================================================
#
# **1. It will not take the lock if there is nothing to sweep.** It asks
# `ci/corpus-falsifiable.py --check` first. A tree whose scenarios are all
# registered needs no recording, and a 40-minute lock acquisition to discover
# that is 40 minutes taken from someone who did need it.
#
# **2. It will not queue against a production merge gate.** `ci/handoff.sh:772`
# runs `ci/t2-gameplay.sh` with `RUSTMC_CORPUS_LOCK_ROLE=gate`, which is a
# priority and not a bypass: a gate jumps the whole queue because it is the only
# thing on the box that can move `main`. A measurement that declares itself
# `gate` competes with production merges **as an equal**, and one did exactly
# that today — a production gate sat behind an agent's measurement for 25
# minutes.
#
# So this script declares `RUSTMC_CORPUS_LOCK_ROLE=run`, which is what
# `docs/AGENT-BRIEF.md` prescribes ("take the corpus lock — as role `run`, so a
# real merge gate still jumps ahead of you"), **and additionally refuses to
# start at all while a merge gate holds or waits the lock in another tree.**
# Role `run` alone would already yield the lock; refusing outright also keeps
# this off the box while the gate is recording, where it would compete for CPU
# and for the reference cache. `--allow-gate-contention` overrides, and says so.
#
# ===========================================================================
# USAGE
# ===========================================================================
#
#   RUSTMC_REMOTE_DIR=<your gate dir> ci/sweep-tip.sh [options]
#
#   --dry-run                  say what would be swept; take no lock, record
#                              nothing
#   --allow-gate-contention    proceed even while a merge gate holds/waits
#   --ledger PATH              default ci/checks/gameplay-falsifiable.toml
#
#   RUSTMC_REMOTE            host, default threadripper
#   RUSTMC_REMOTE_DIR        gate dir on that host. REQUIRED, and may not be
#                            the canonical checkout
#   RUSTMC_CORPUS_LOCK_DIR   default ~/.rustmc-corpus-lock
#   RUSTMC_SWEEP_RECORD_CMD  replaces the whole remote record+sweep phase.
#                            Exists for ci/batch-falsify.sh; a real run leaves
#                            it unset
#
# Exit: 0 swept (or nothing needed sweeping) | 1 the record or sweep failed
#       | 2 refused (state, or a merge gate has the box)
#
# **This does not gate and it does not merge.** It produces evidence.
# ===========================================================================
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

CANONICAL="${RUSTMC_CANONICAL:-/home/sayer/grit/repo}"
HOST="${RUSTMC_REMOTE:-threadripper}"
REMOTE_DIR="${RUSTMC_REMOTE_DIR:-}"
LOCK_DIR="${RUSTMC_CORPUS_LOCK_DIR:-$HOME/.rustmc-corpus-lock}"
LEDGER="ci/checks/gameplay-falsifiable.toml"
DRY=""
ALLOW_CONTENTION=""

while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run)               DRY=1; shift ;;
        --allow-gate-contention) ALLOW_CONTENTION=1; shift ;;
        --ledger)                LEDGER="${2:-}"; shift 2 ;;
        -h|--help)               sed -n '/^# USAGE/,/^# ====/p' "$0"; exit 0 ;;
        *)                       echo "REFUSED  unknown option: $1"; exit 2 ;;
    esac
done

say() { printf '%s  sweep-tip: %s\n' "$(date '+%H:%M:%S')" "$*"; }

if [ -d "$CANONICAL" ] && [ "$ROOT" -ef "$CANONICAL" ]; then
    echo "REFUSED  \$ROOT is the canonical checkout $CANONICAL — a live gate owns it"
    exit 2
fi
case "$REMOTE_DIR" in
    "")            echo "REFUSED  set RUSTMC_REMOTE_DIR to your own gate directory"; exit 2 ;;
    rustmc|rustmc/) echo "REFUSED  RUSTMC_REMOTE_DIR=$REMOTE_DIR is the canonical checkout"; exit 2 ;;
esac
[ -r "$LEDGER" ] || { echo "REFUSED  no ledger at $LEDGER"; exit 2; }

# ---------------------------------------------------------------------------
# REFUSAL 1: is there anything to sweep at all?
# ---------------------------------------------------------------------------
# `--check` needs no recordings and no lock, so this costs a second and can save
# forty minutes of somebody else's queue. Its own words for the two states that
# a recording would fix are UNREGISTERED (no ledger entry at all) and STALE (the
# scenario changed since its evidence was taken).
CHECK_OUT="$(python3 ci/corpus-falsifiable.py --check 2>&1)"
CHECK_RC=$?
NEED="$(printf '%s\n' "$CHECK_OUT" | grep -cE 'UNREGISTERED|STALE|UNSWEPT' || true)"
if [ "$CHECK_RC" -eq 0 ] && [ "${NEED:-0}" -eq 0 ]; then
    say "every scenario in this tree already carries evidence — nothing to record"
    say "  (ci/corpus-falsifiable.py --check is green; taking the corpus lock to"
    say "   discover that would be 40 minutes out of somebody else's queue)"
    exit 0
fi
say "$NEED scenario(s) need a recording:"
printf '%s\n' "$CHECK_OUT" | grep -E 'UNREGISTERED|STALE|UNSWEPT' | head -12 | sed 's/^/           /'

# ---------------------------------------------------------------------------
# REFUSAL 2: a production merge gate must not queue behind a measurement
# ---------------------------------------------------------------------------
# Read from the lock's own state files rather than by parsing `status` prose:
# `holder` and `waiters/<pid>` carry `role=` and `tree=` as key=value lines, and
# a format this script parses is a format that will not change under it silently
# the way a human-readable listing would.
gate_owner() {
    local f role tree pid
    for f in "$LOCK_DIR/holder" "$LOCK_DIR"/waiters/*; do
        [ -r "$f" ] || continue
        role="$(sed -n 's/^role=//p' "$f" | head -1)"
        [ "$role" = gate ] || continue
        tree="$(sed -n 's/^tree=//p' "$f" | head -1)"
        pid="$(sed -n 's/^pid=//p' "$f" | head -1)"
        # A record whose process is gone is not a holder; the lock says so
        # itself, and treating it as one would refuse forever.
        case "$pid" in ''|*[!0-9]*) continue ;; esac
        kill -0 "$pid" 2>/dev/null || continue
        [ "$tree" -ef "$ROOT" ] 2>/dev/null && continue    # that would be us
        printf '%s\t%s\t%s\n' "$pid" "$tree" "$(basename "$(dirname "$f")")"
        return 0
    done
    return 1
}

if owner="$(gate_owner)"; then
    say "a merge gate has the box: pid ${owner%%	*} in $(printf '%s' "$owner" | cut -f2)"
    if [ -z "$ALLOW_CONTENTION" ]; then
        echo "REFUSED  a production merge gate holds or waits the corpus lock."
        echo "         ci/handoff.sh declares role=gate, which jumps the whole queue"
        echo "         because it is the only thing that can move main. A measurement"
        echo "         that starts now competes with it for the box even as role=run."
        echo "         Re-run when it is done, or --allow-gate-contention."
        exit 2
    fi
    say "--allow-gate-contention: proceeding anyway"
fi

BEFORE_ENTRIES="$(grep -c '^\[\[scenario\]\]' "$LEDGER" || true)"
say "ledger has ${BEFORE_ENTRIES:-0} scenario entries before this run"

if [ -n "$DRY" ]; then
    say "--dry-run: would record and sweep in ~/$REMOTE_DIR on $HOST as role=run"
    say "--dry-run: no lock taken, nothing recorded"
    exit 0
fi

# ---------------------------------------------------------------------------
# THE RECORD-AND-SWEEP PHASE
# ---------------------------------------------------------------------------
# Order is not negotiable and is the same order `ci/handoff.sh` uses for the
# same reason: the generators live in this tree so they cannot run before the
# rsync, and the recorder reads the tables they produce so it cannot run before
# them. `docs/AGENT-BRIEF.md` records this being got wrong three times.
#
# ROLE=run, NOT gate. See the header. This is a measurement; it yields.
#
# The gate directory's own oracle-work and data/vanilla are a PRECONDITION here,
# not something this script sets up: getting that wrong is the 2026-08-03
# failure where two trees shared one `oracle-work` and a gate measured whichever
# branch had last regenerated it, and `ci/handoff.sh` stage 1 is the reviewed
# implementation. If the remote dir is not prepared, `ci/t2-gameplay.sh` exits 4
# on a foreign or missing table and says which — which is the honest refusal.
RECORD_CMD="${RUSTMC_SWEEP_RECORD_CMD:-}"
LOG="${RUSTMC_SWEEP_LOG:-$HOME/rustmc-private/gates/sweep-$(date +%Y%m%d-%H%M%S).log}"
mkdir -p "$(dirname "$LOG")" 2>/dev/null

remote() {
    ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; \
        cd \$HOME/$REMOTE_DIR && $*"
}

if [ -n "$RECORD_CMD" ]; then
    say "record phase overridden by RUSTMC_SWEEP_RECORD_CMD"
    bash -c "$RECORD_CMD" >>"$LOG" 2>&1
    RC=$?
else
    say "shipping this tree to ~/$REMOTE_DIR on $HOST -> $LOG"
    ./ci/t0-remote.sh --sync >>"$LOG" 2>&1; RC=$?
    if [ "$RC" -eq 0 ]; then
        say "generating this tree's own tables"
        remote "./ci/gen-gameplay-tables.sh" >>"$LOG" 2>&1; RC=$?
    fi
    if [ "$RC" -eq 0 ]; then
        say "recording as role=run — a merge gate will still jump ahead of this"
        remote "RUSTMC_CORPUS_LOCK_ROLE=run ./ci/t2-gameplay.sh" >>"$LOG" 2>&1; RC=$?
    fi
    if [ "$RC" -eq 0 ]; then
        say "sweeping (seconds, no lock) and writing the ledger"
        remote "python3 ci/corpus-falsifiable.py --sweep \
                  --bin target/release/oracle-runner \
                  oracle-work/gameplay/vanilla.json \
                  oracle-work/gameplay/rustmc.json" >>"$LOG" 2>&1; RC=$?
    fi
    if [ "$RC" -eq 0 ]; then
        say "bringing the ledger back"
        scp -q "$HOST:\$HOME/$REMOTE_DIR/$LEDGER" "$LEDGER" >>"$LOG" 2>&1 || \
        ssh -n "$HOST" "cat \$HOME/$REMOTE_DIR/$LEDGER" > "$LEDGER.new" 2>>"$LOG" && \
            mv "$LEDGER.new" "$LEDGER"
        RC=$?
    fi
fi

if [ "$RC" -ne 0 ]; then
    echo "FAILED   the record-and-sweep phase exited $RC — $LOG"
    echo "         Nothing has been committed. The ledger is unchanged unless the"
    echo "         copy-back stage was reached, which the log will say."
    exit 1
fi

AFTER_ENTRIES="$(grep -c '^\[\[scenario\]\]' "$LEDGER" || true)"
say "ledger now has ${AFTER_ENTRIES:-0} scenario entries (was ${BEFORE_ENTRIES:-0})"

# The point of the whole exercise, asked the only way that can answer it: does
# `--check` pass now? A ledger that grew but still fails is a sweep that did not
# cover what was missing, and reporting "swept N scenarios" without asking would
# be reporting the instrument.
if python3 ci/corpus-falsifiable.py --check >/dev/null 2>&1; then
    say "ci/corpus-falsifiable.py --check is GREEN — this tree's scenarios carry evidence"
    say "commit $LEDGER; it is the evidence, and T0 reads it"
    exit 0
fi
say "ci/corpus-falsifiable.py --check is STILL RED after the sweep:"
python3 ci/corpus-falsifiable.py --check 2>&1 | grep -E 'UNREGISTERED|STALE|UNSWEPT|TIER|BATTERY' | head -8 | sed 's/^/           /'
say "the recording did not cover what was missing — $LOG"
exit 1
