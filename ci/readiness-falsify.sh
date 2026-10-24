#!/usr/bin/env bash
# readiness-falsify.sh — drive the real server into every not-ready state and
# run the harness's own readiness lines against it.
#
#     ./ci/readiness-falsify.sh            # about 20 s after the build
#     READINESS_KEEP=1 ./ci/readiness-falsify.sh    # keep the captured logs
#
# # What this is evidence for
#
# `ci/liveness-proofs.py` checks a readiness proof against a *fixture* — text
# in `ci/checks/liveness-proofs.toml` claimed to be what a not-ready subject
# prints. That is the right shape for a T0 gate and it has one hole: the
# fixture is a claim about the server, and nothing in that gate boots one. A
# fixture that drifts into fiction turns the gate green for the same reason
# the defect it replaced was green — it stopped being able to tell.
#
# So this script produces the logs instead of asserting them. It boots the
# real binary into four states, captures what it actually printed, and then
# runs **the readiness line lifted verbatim out of each harness script**
# against each capture, with `$OUR_PORT`, `$RUSTMC_PID`, `$WORK` and `$BIN`
# bound the way the script binds them. Nothing here is a paraphrase of the
# check; it is the check.
#
# # The four states, and which arm refuses each
#
#   ready            bound, ours, answering            accepted by all three
#   starting         built the world, has not bound     arm 1 (no line yet)
#   bind-refused     `listen: Address already in use`   arm 1 (no line at all)
#   wrong-port       bound, ours, answering, elsewhere  arm 1 (port mismatch)
#   foreign          the port is another server's       arm 2 (pid mismatch)
#
# The last two are the ones that could not exist before this branch. `ready`'s
# line carries `listener.local_addr()`, so there is a port in it that was
# bound rather than requested, and arm 1 can compare it against the port the
# harness asked for. And `foreign` is the failure the whole item is named
# after: our bind is refused, somebody else's server answers, and every
# instrument downstream agrees with every other while all of them are
# measuring the neighbour.
#
# # The red-when-broken evidence, both directions
#
# The last arm reconstructs the **pre-repair** log — the bind-refused capture
# with the announcement put back in front of the error, which is byte for byte
# what this server printed until 2026-08-13 — and shows that
#
#   * the pre-repair predicate `grep -q 'listening on'` ACCEPTS it. That is
#     the defect, reproduced: there is no log containing that string which
#     that predicate refuses, so its green could not be turned red;
#   * the shipped readiness line REFUSES it, and refuses it on arm 2 rather
#     than arm 1 — the log names the right port, so what rejects it is the
#     kernel being asked whose socket that is.
#
# That second point is why arm 2 exists. Arm 1 alone would be sound against
# the repaired server and would go unsound again the moment anything printed
# that line early, which is a property of a file somebody can edit.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/ci/server-ready.sh"

FAIL=0
W="$(mktemp -d -t readiness-falsify-XXXXXX)"
SQUATTERS=()
SERVERS=()

cleanup() {
    for p in "${SERVERS[@]:-}" "${SQUATTERS[@]:-}"; do
        [ -n "$p" ] && kill "$p" 2>/dev/null || true
    done
    if [ -n "${READINESS_KEEP:-}" ]; then
        echo "captures kept in $W"
    else
        rm -rf "$W"
    fi
}
trap cleanup EXIT

ok()  { echo "  ok    $*"; }
bad() { echo "  RED   $*" >&2; FAIL=1; }
want() { if [ "$1" = 0 ]; then ok "$2"; else bad "$2"; fi; }

# A port nothing is on. Asked for and released, so this is a probe and not a
# reservation — the same thing `ci/t2-gameplay.sh` does, and it carries the
# same small race, which is one of the things being defended against here.
free_port() {
    python3 -c 'import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

# The readiness line, lifted out of the registry, which `ci/liveness-proofs.py`
# separately holds to be verbatim what is in the script (its STALE arm). So
# reading it from here cannot drift from the shipped text without T0 saying so.
source_line_of() {
    python3 - "$1" <<'PY'
import sys, tomllib, pathlib
reg = tomllib.loads(pathlib.Path("ci/checks/liveness-proofs.toml").read_text())
for p in reg["proof"]:
    if p["script"] == sys.argv[1]:
        print(p["source_line"])
        break
else:
    sys.exit(f"no proof registered for {sys.argv[1]}")
PY
}

# Run one script's readiness line against a captured log, in a shell where the
# names it uses mean what the script means by them. `for _ in 1` so that the
# line's own `break` is legal outside its loop.
#
# `eval` on tracked text this script also gates is not the hazard it looks
# like: the alternative is a transcription of the line, and a transcription is
# exactly what would go stale and pass while the shipped line failed.
run_readiness_line() {
    local script="$1" logdir="$2" port="$3" pid="$4" line
    line="$(source_line_of "$script")"
    # A lookup that failed would produce an empty line, `eval ""`, and a
    # reading of "refused" — which is the answer four of the five arms below
    # want to see. Refuse instead.
    [ -n "$line" ] || { echo "no readiness line found for $script" >&2; exit 2; }
    # **An unparseable line is a hard failure, not a refusal. 2026-09-05.**
    #
    # `eval` on a fragment prints `syntax error: unexpected end of file` and
    # leaves RUSTMC_READY at 0 — which is indistinguishable, to every caller
    # below, from a predicate that read the log and said "not ready". Four of
    # the five arms here duly printed `ok` for a line that had never run. That
    # is a refusal arm passing because nothing happened, in the script whose
    # subject is proofs that cannot go red.
    if ! bash -n -c "for _ in 1; do
$line
done" 2>/dev/null; then
        echo "the registered readiness line for $script does not parse — a syntax" >&2
        echo "error would read as \"refused\" in every arm below:" >&2
        echo "    $line" >&2
        exit 2
    fi
    (
        WORK="$logdir"
        OUR_PORT="$port"
        RUSTMC_PID="$pid"
        RUSTMC_READY=0
        for _ in 1; do eval "$line"; done
        [ "$RUSTMC_READY" = 1 ]
    )
}

# Every script whose readiness proof implements the harness contract. Read
# from the registry rather than listed, so a fifth site is covered the day it
# is registered instead of the day somebody remembers this file.
#
# **Selected on `contract`, not on `"rustmc.log" in source_line`. 2026-09-05.**
# That substring was a proxy for the real condition — that the line reads
# `$WORK/rustmc.log`, matches `$OUR_PORT`, attributes to `$RUSTMC_PID` and
# signals with `RUSTMC_READY=1`, which is the shell `run_readiness_line` builds
# for it. `ci/readiness-falsify.sh`'s own boot wait mentions the log and does
# none of those, so registering it swept it in here and every arm below ran an
# `eval` that could not parse. The proxy is now the declaration, and
# `ci/liveness-proofs.py --check` refuses a proof naming the log that declares
# neither `contract` nor a written `why_no_contract` — so a proof cannot leave
# this population by omission, only by saying why.
mapfile -t SCRIPTS < <(python3 - <<'PY'
import tomllib, pathlib
reg = tomllib.loads(pathlib.Path("ci/checks/liveness-proofs.toml").read_text())
for p in reg["proof"]:
    if p.get("contract") == "rustmc-ready":
        print(p["script"])
PY
)
[ "${#SCRIPTS[@]}" -ge 3 ] || { echo "expected at least three rustmc readiness proofs, found ${#SCRIPTS[@]}" >&2; exit 2; }

echo "== building"
cargo build -q -p rustmc-server -p oracle-runner
SERVER="$ROOT/target/debug/rustmc-server"
BIN="$ROOT/target/debug/oracle-runner"
[ -x "$SERVER" ] && [ -x "$BIN" ] || { echo "build produced no binaries" >&2; exit 2; }

# --------------------------------------------------------------------------
# STATE 1 — ready. And STATE 2 falls out of the same boot: the log as it stood
# at the last moment before the announcement, which is a capture rather than a
# construction. A server that has built its world and not yet bound is the
# state the old sixty-second loop timed out in and recorded anyway.
# --------------------------------------------------------------------------
echo "== state: ready (and, from the same boot, still-starting)"
READY_PORT="$(free_port)"
mkdir -p "$W/ready" "$W/starting"
"$SERVER" run --port "$READY_PORT" --level-type flat --radius 2 --view-distance 4 \
    >"$W/ready/rustmc.log" 2>&1 &
READY_PID=$!
SERVERS+=("$READY_PID")
for _ in $(seq 1 600); do
    if grep -q "listening on 0.0.0.0:$READY_PORT[ ,]" "$W/ready/rustmc.log" 2>/dev/null; then
        break
    fi
    # The pre-announcement snapshot, overwritten until it stops being one. The
    # last one written is the closest this server ever gets to ready without
    # being ready.
    #
    # **COPY FIRST, THEN CHECK THE COPY — NEVER TRUST THE GREP ABOVE TO SPEAK
    # FOR IT.** That grep tests the file the server is still writing to, so the
    # announcement can land in the window between it and this copy and be
    # copied along with it. State 2 is then not a not-ready log at all, the
    # three predicates correctly accept it, and the script scores three REDs
    # naming a defect that is not there. That window is scheduling and not
    # logic: measured 2026-08-18 on ONE tree, RED at 1-min load 11.8 and GREEN
    # at 3.5, with identical binaries. Keeping only a snapshot that has been
    # PROVEN clean closes it whatever the copy raced, and it is still a capture
    # rather than a construction — nothing is edited, a raced sample is simply
    # not kept.
    cp "$W/ready/rustmc.log" "$W/starting/rustmc.log.tmp" 2>/dev/null || true
    if ! grep -q 'listening on' "$W/starting/rustmc.log.tmp" 2>/dev/null; then
        mv "$W/starting/rustmc.log.tmp" "$W/starting/rustmc.log" 2>/dev/null || true
    fi
    kill -0 "$READY_PID" 2>/dev/null || break
    sleep 0.1
done
rm -f "$W/starting/rustmc.log.tmp"
grep -q "listening on 0.0.0.0:$READY_PORT[ ,]" "$W/ready/rustmc.log" 2>/dev/null \
    || { echo "the server never announced on :$READY_PORT — nothing below can be measured" >&2
         tail -5 "$W/ready/rustmc.log" >&2; exit 2; }
ok "the ready capture carries a post-bind line naming :$READY_PORT"
if grep -q 'listening on' "$W/starting/rustmc.log" 2>/dev/null; then
    bad "the still-starting capture already contains the announcement — the snapshot
        raced the bind, so state 2 is not measured this run"
else
    ok "the still-starting capture has no announcement in it"
fi
if grep -q 'spawn tile:' "$W/starting/rustmc.log" 2>/dev/null; then
    ok "and it is a server that got as far as building its world"
else
    echo "  note  the still-starting capture is empty — the boot outran a 100 ms" >&2
    echo "        sampler. Still a valid not-ready log, weaker as a demonstration." >&2
fi

echo "== the three arms, each on its own"
want $(ready_socket_owner "$READY_PORT" "$READY_PID" && echo 0 || echo 1) \
     "arm 2 accepts: the LISTEN socket on :$READY_PORT is pid $READY_PID's"
want $(ready_socket_owner "$READY_PORT" 1 && echo 1 || echo 0) \
     "arm 2 refuses a pid that does not own it"
want $(ready_speaks_status "$BIN" "127.0.0.1:$READY_PORT" && echo 0 || echo 1) \
     "arm 3 accepts: the server completes a status exchange"
DUMB_PORT="$(free_port)"
python3 -c 'import socket,sys,time
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(8); time.sleep(120)' "$DUMB_PORT" &
DUMB_PID=$!
SQUATTERS+=("$DUMB_PID")
sleep 0.5
want $(ready_speaks_status "$BIN" "127.0.0.1:$DUMB_PORT" && echo 1 || echo 0) \
     "arm 3 refuses a socket that accepts and does not speak the protocol"

echo "== every readiness line against the ready capture"
for s in "${SCRIPTS[@]}"; do
    want $(run_readiness_line "$s" "$W/ready" "$READY_PORT" "$READY_PID" && echo 0 || echo 1) \
         "$s accepts a server that is bound, ours and answering"
done

echo "== every readiness line against the still-starting capture"
for s in "${SCRIPTS[@]}"; do
    want $(run_readiness_line "$s" "$W/starting" "$READY_PORT" "$READY_PID" && echo 1 || echo 0) \
         "$s refuses a server that has not bound yet"
done

echo "== every readiness line against the ready capture, asked about another port"
OTHER_PORT="$(free_port)"
for s in "${SCRIPTS[@]}"; do
    want $(run_readiness_line "$s" "$W/ready" "$OTHER_PORT" "$READY_PID" && echo 1 || echo 0) \
         "$s refuses a server that bound somewhere else (:$READY_PORT, asked :$OTHER_PORT)"
done

# --------------------------------------------------------------------------
# STATE 3 — the bind is refused, by a second rustmc-server. Not by a python
# socket: the point of using the real thing is that this is the situation the
# harness meets in the field, where the neighbour on the port is a working
# server that will answer anything asked of it.
# --------------------------------------------------------------------------
echo "== state: bind refused, with a foreign rustmc-server holding the port"
mkdir -p "$W/inuse" "$W/foreign"
FOREIGN_PORT="$(free_port)"
"$SERVER" run --port "$FOREIGN_PORT" --level-type flat --radius 2 --view-distance 4 \
    >"$W/foreign/rustmc.log" 2>&1 &
FOREIGN_PID=$!
SERVERS+=("$FOREIGN_PID")
# Waited on by observation rather than by grep, and deliberately: a file whose
# subject is "a readiness proof must not be satisfiable by a string" should not
# contain one. `ci/liveness-proofs.py --discover` found the log-grep version of
# this loop and refused it as an unregistered proof, which was the gate working
# on its own author. The two arms below are strictly stronger than the grep
# anyway — they are pid-attributed and they need no log.
for _ in $(seq 1 600); do
    ready_socket_owner "$FOREIGN_PORT" "$FOREIGN_PID" \
        && ready_speaks_status "$BIN" "127.0.0.1:$FOREIGN_PORT" && break
    kill -0 "$FOREIGN_PID" 2>/dev/null || { echo "the foreign server died" >&2; exit 2; }
    sleep 0.1
done
"$SERVER" run --port "$FOREIGN_PORT" --level-type flat --radius 2 --view-distance 4 \
    >"$W/inuse/rustmc.log" 2>&1 &
INUSE_PID=$!
set +e
wait "$INUSE_PID"
INUSE_STATUS=$?
set -e

want $([ "$INUSE_STATUS" -ne 0 ] && echo 0 || echo 1) \
     "a refused bind exits non-zero (got $INUSE_STATUS)"
want $(grep -q '^listen: ' "$W/inuse/rustmc.log" && echo 0 || echo 1) \
     "and says so: $(grep -m1 '^listen: ' "$W/inuse/rustmc.log" || echo '(nothing)')"
want $(grep -q 'listening on' "$W/inuse/rustmc.log" && echo 1 || echo 0) \
     "and prints NO readiness line at all — the fact every fixture in
        ci/checks/liveness-proofs.toml rests on"

for s in "${SCRIPTS[@]}"; do
    want $(run_readiness_line "$s" "$W/inuse" "$FOREIGN_PORT" "$INUSE_PID" && echo 1 || echo 0) \
         "$s refuses a server whose bind was refused, while a live one answers on :$FOREIGN_PORT"
done

# --------------------------------------------------------------------------
# The historical defect, reconstructed, both directions. The capture above
# with the announcement put back where `main.rs` used to print it: before the
# call that binds, and so before the error.
# --------------------------------------------------------------------------
echo "== the pre-repair log, rebuilt from this run's own capture"
mkdir -p "$W/historical"
{
    grep -v '^listen: ' "$W/inuse/rustmc.log" || true
    echo "rustmc 26.2 listening on 0.0.0.0:$FOREIGN_PORT (not saving: pass --world <dir> to persist)"
    grep '^listen: ' "$W/inuse/rustmc.log" || true
} > "$W/historical/rustmc.log"

want $(LOG="$W/historical/rustmc.log" bash -c "grep -q 'listening on' \"\$LOG\"" && echo 0 || echo 1) \
     "the PRE-REPAIR predicate accepts it — the defect, reproduced: its green
        could not be turned red by any log containing that string"
for s in "${SCRIPTS[@]}"; do
    want $(run_readiness_line "$s" "$W/historical" "$FOREIGN_PORT" "$INUSE_PID" && echo 1 || echo 0) \
         "$s refuses it anyway — on arm 2, because the log names the right port
        and the socket on it belongs to pid $FOREIGN_PID"
done

# --------------------------------------------------------------------------
# The fixtures in the registry are transcripts. Check they still are.
# --------------------------------------------------------------------------
echo "== the registry's positive fixtures against this run's real announcement"
REAL_SHAPE="$(grep -m1 'listening on' "$W/ready/rustmc.log" | sed "s/:$READY_PORT/:PORT/")"
for s in "${SCRIPTS[@]}"; do
    FIX_SHAPE="$(python3 - "$s" <<'PY'
import sys, tomllib, pathlib, re
reg = tomllib.loads(pathlib.Path("ci/checks/liveness-proofs.toml").read_text())
for p in reg["proof"]:
    if p["script"] == sys.argv[1]:
        for line in p.get("positive", "").splitlines():
            if "listening on" in line:
                print(re.sub(r":\d+", ":PORT", line, count=1))
                break
        break
PY
)"
    if [ "$FIX_SHAPE" = "$REAL_SHAPE" ]; then
        ok "$s's positive fixture is still what the server prints"
    else
        bad "$s's positive fixture has drifted from the server
        fixture: $FIX_SHAPE
        server:  $REAL_SHAPE"
    fi
done

echo
if [ "$FAIL" = 0 ]; then
    echo "readiness-falsify: GREEN — every readiness line accepted the one ready state and"
    echo "refused all four not-ready ones, and the pre-repair predicate refused none of them."
else
    echo "readiness-falsify: RED" >&2
fi
exit "$FAIL"
