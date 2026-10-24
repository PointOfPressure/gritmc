#!/usr/bin/env bash
# One corpus recording against a deliberately wrong server, and the four-way
# reading of what each check noticed.
#
# # Why this is not `ci/t2-gameplay.sh` with a flag
#
# `ci/t2-gameplay.sh` is the gate. It boots two servers, records both, and
# refuses on the diff. This boots **one** — ours, carrying
# `crates/rustmc-server/src/sabotage.rs`'s controls — and compares it against a
# recording that already exists. The reference half is not re-derived because
# nothing about vanilla is under test here: the subject is our own corpus, and
# the falsification half of a demonstration never needs the reference to be
# taken again. That is `ci/mute-falsify.py`'s technique and
# `ci/corpus-falsifiable.py` inherits it.
#
# The saving is the point. A gate's corpus run holds the box for forty minutes
# with two servers in it; this holds it for one server and no vanilla boot, and
# recordings on this box queue several deep.
#
# # The clean half comes from a tree, not from a second run
#
# The reading is "did the check move when the implementation got worse", so it
# needs a clean recording of *the same code*. That is one more corpus run, and
# the budget is one. It is not needed: every sabotage site compiles to a
# constant `false` without `--features corpus-sabotage`, so a clean recording
# of any tree whose gameplay crates match this one modulo those insertions is
# this tree's clean recording. `--clean` names it and `--assert-clean-tree`
# checks the claim rather than trusting it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CLEAN=""
REF=""
CLEAN_TREE=""
PORT="${CORPUS_PC_PORT:-}"
CHECKS="$ROOT/ci/checks/gameplay"
OUT="$ROOT/oracle-work/positive-control"

while [ $# -gt 0 ]; do
    case "$1" in
        --clean)             CLEAN="$2"; shift 2 ;;
        --reference)         REF="$2"; shift 2 ;;
        --assert-clean-tree) CLEAN_TREE="$2"; shift 2 ;;
        --checks)            CHECKS="$2"; shift 2 ;;
        --out)               OUT="$2"; shift 2 ;;
        --port)              PORT="$2"; shift 2 ;;
        *) echo "unknown argument $1" >&2; exit 2 ;;
    esac
done

refuse() { echo "CORPUS-POSITIVE-CONTROL REFUSED — $*" >&2; exit 2; }

# **`--out` must not be inside a remote gate directory, and this is a refusal
# rather than a note.** `ci/t0-remote.sh` rsyncs the working tree into
# `~/$RUSTMC_REMOTE_DIR` with `--delete`; a run whose artefacts live there
# loses its recording, its clean half and its server log the moment anybody in
# any worktree runs the gate. That happened on 2026-08-18 to a run that had
# already spent 25 minutes in the corpus queue, and the symptom arrives an hour
# later as a missing file rather than as anything pointing at the cause.
case "$(cd "$(dirname "$OUT")" 2>/dev/null && pwd || dirname "$OUT")" in
    "$HOME"/rustmc*|"$HOME"/conf*)
        refuse "--out is $OUT, inside a directory ci/t0-remote.sh rsyncs with --delete.
     Any gate run from any worktree would erase this run's artefacts.
     Put it somewhere that is not a remote gate directory." ;;
esac

[ -n "$CLEAN" ] || refuse "--clean <rustmc.json> is required"
[ -n "$REF" ]   || refuse "--reference <vanilla.json> is required"
[ -f "$CLEAN" ] || refuse "no clean recording at $CLEAN"
[ -f "$REF" ]   || refuse "no reference recording at $REF"

# **The clean recording's tree is checked, not assumed.** A clean half taken
# against different gameplay code turns every difference between the two runs
# into a difference this instrument attributes to its own controls — which is
# the same "measures its own input" defect the whole exercise is about.
if [ -n "$CLEAN_TREE" ]; then
    [ -d "$CLEAN_TREE/crates" ] || refuse "$CLEAN_TREE has no crates/"
    stray="$(diff -rq "$CLEAN_TREE/crates" "$ROOT/crates" 2>/dev/null \
        | grep -v 'sabotage\.rs' \
        | grep -v 'rustmc-server/src/lib.rs' \
        | grep -v 'rustmc-server/Cargo.toml' \
        | grep -v 'rustmc-server/src/\(broadcast\|placement\|power\|shapes\|ticks\|useblock\|world_send\|main\)\.rs' \
        || true)"
    [ -z "$stray" ] || refuse "the clean recording's tree differs outside the sabotage sites:
$stray"
    for f in broadcast placement power shapes ticks useblock world_send main; do
        removed="$(diff "$CLEAN_TREE/crates/rustmc-server/src/$f.rs" \
                        "$ROOT/crates/rustmc-server/src/$f.rs" \
                   | grep -c '^<' || true)"
        # `world_send.rs` turns `if placed {` into `if placed && !structural {`,
        # which is one removed line and is inert with the feature off.
        [ "$removed" -le 1 ] || refuse "$f.rs removes $removed line(s) from the clean tree — \
the controls must be additions, or the clean half is not this tree's clean half"
    done
    echo "clean-tree: $CLEAN_TREE differs from this tree only by the sabotage insertions"
fi

export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p "$OUT"

echo "== building"
cargo build -q -p oracle-runner --release
cargo build -q -p rustmc-server --release --features corpus-sabotage
BIN="$ROOT/target/release/oracle-runner"

echo "== projecting the reference, to choose each workspace's controls"
"$BIN" project "$CHECKS" --trace "$REF" --json "$OUT/reference-projection.json" >/dev/null
python3 "$ROOT/ci/corpus-controls.py" "$CHECKS" "$OUT/reference-projection.json" \
    > "$OUT/controls.json"
SPEC="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["spec"])' "$OUT/controls.json")"
echo "   RUSTMC_SABOTAGE=$SPEC"

# The lock. One acquisition, one recording, one server — see the header.
. "$ROOT/ci/corpus-lock.sh"
cleanup() {
    [ -n "${SRV_PID:-}" ] && kill "$SRV_PID" 2>/dev/null || true
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
}
trap cleanup EXIT
corpus_lock_acquire
# Ports are chosen AFTER the corpus lock, from the tree path, and probed:
# the lock is a pool since 2026-09-24 and a fixed pair shared by every tree is
# the 2026-08-06 collision under a pool. See ci/corpus-ports.sh.
. "$ROOT/ci/corpus-ports.sh"
[ -n "$PORT" ] || PORT="$(corpus_free_ports 1 posctl)" || refuse "no free port in 28000-28999"

echo "== booting the sabotaged server on :$PORT"
RUSTMC_SABOTAGE="$SPEC" ./target/release/rustmc-server run --port "$PORT" \
    --level-type flat --radius 2 --view-distance 4 \
    > "$OUT/server.log" 2>&1 &
SRV_PID=$!
# Two arms, the same two `ci/t2-gameplay.sh` uses, and for the reason
# `ci/server-ready.sh` documents: the server prints its readiness line past the
# bind now, but our bind failing and another tree holding the port are
# indistinguishable from the log alone.
. "$ROOT/ci/server-ready.sh"
ready=0
for _ in $(seq 1 120); do
    # **One line, deliberately.** `ci/liveness-proofs.py` discovers a readiness
    # proof by finding the probe and the loop exit on the SAME line; splitting
    # this across a continuation for readability made the gate stop seeing it
    # altogether, which is a wait loop that has quietly opted out of the one
    # check that asks whether it can say "not ready".
    grep -q "listening on 0.0.0.0:$PORT[ ,]" "$OUT/server.log" 2>/dev/null && ready_socket_owner "$PORT" "$SRV_PID" && { ready=1; break; }
    kill -0 "$SRV_PID" 2>/dev/null || { echo "server died — tail:"; tail -20 "$OUT/server.log"; exit 1; }
    sleep 1
done
[ "$ready" = 1 ] || refuse "the sabotaged server never bound :$PORT within 120s
$(tail -20 "$OUT/server.log")"
# **Armed is asserted, not hoped for.** A control that fails to arm fires zero
# times and zero firings is this instrument's spelling of UNVISITED, so a run
# whose server quietly armed nothing would report the whole corpus as testing
# dead code.
grep -q "sabotage control(s) armed" "$OUT/server.log" \
    || refuse "the server did not announce any armed control
$(head -5 "$OUT/server.log")"
sleep 1

echo "== recording"
# **A non-zero exit here is ordinary and must not end the run.** The subject is
# a server with eight subsystems removed; scenarios whose setup depended on one
# of them cannot complete, and `record` says so and exits non-zero. That is the
# positive control working — a scenario the harness could not finish is a
# scenario the check cannot call `match` — and the recording it wrote is still
# the reading. `set -e` would throw away forty minutes of a contended lock over
# the expected outcome.
rec_rc=0
"$BIN" record "127.0.0.1:$PORT" "$CHECKS" --out "$OUT/rustmc-sabotaged.json" || rec_rc=$?
[ "$rec_rc" = 0 ] || echo "   record exited $rec_rc — expected under sabotage; reading what it wrote"
[ -s "$OUT/rustmc-sabotaged.json" ] || refuse "record wrote no recording at all (rc=$rec_rc)"
kill "$SRV_PID" 2>/dev/null || true
SRV_PID=""

echo "== reading"
python3 "$ROOT/ci/corpus-grades.py" --report \
    --checks "$CHECKS" --reference "$REF" \
    --clean "$CLEAN" --sabotaged "$OUT/rustmc-sabotaged.json" \
    --controls "$OUT/controls.json" --log "$OUT/server.log" \
    --out "$OUT/grades.json" --bin "$BIN"
