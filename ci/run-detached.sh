#!/usr/bin/env bash
# ===== AGENT INTERFACE — this is all you need to read =====
#
# ci/run-detached.sh <name> — run a whole sequence detached, and wake once.
#
#     ./ci/run-detached.sh mygate <<'EOF'
#     ./ci/handoff.sh branch-one
#     ./ci/handoff.sh branch-two
#     EOF
#
# It prints ONE line: the `run_in_background: true` waiter to arm. Arm it, do
# other work, and you are woken once when the whole sequence is over — pass or
# fail, sentinel or sudden death.
#
#     ./ci/run-detached.sh --status <name>      what happened, one line
#     ./ci/run-detached.sh --log <name>         path to the log
#
# WHAT BREAKS IF YOU IGNORE THIS: ending your turn between steps re-reads your
# entire context on resume. A nine-stop agent costs about twice a single-shot
# one for identical work.
# ===== end interface; everything below is why =====
#
# # WHY THIS EXISTS
#
# The rule "never wait in the foreground" was followed and the cost came back
# anyway, by a different door. Agents stopped writing `until … sleep` in the
# foreground and started **ending the turn** between steps instead: gate a
# branch, stop; be resumed, read the verdict, stop; be resumed, gate the next
# one. Every one of those resumes re-caches the agent's whole context as
# `cache_creation`. Nine stops is nine full context writes for one sequence a
# script could have run start to finish.
#
# So the blessed shape is: **put the whole sequence in a script, run it
# detached, arm exactly one waiter, and do other work.** Not "run step one,
# wait, run step two".
#
# # THE SENTINEL MUST FIRE ON EVERY OUTCOME, INCLUDING THE ONES NOBODY PRINTS
#
# This is the same defect as `ci/t2-gameplay.sh`'s missing RED line, and it is
# the reason this file exists as a script rather than a paragraph. A waiter
# keyed on strings the payload chooses to print can only see the outcomes that
# payload has a string for. `ci/t2-gameplay.sh` printed GREEN on success and
# REFUSED when it declined to measure, and on a genuine corpus regression it
# exited non-zero saying neither — so `docs/AGENT-BRIEF.md`'s own recommended
# waiter, in the same block that warns a green-only waiter is silent through a
# crash, hung forever through exactly that.
#
# The sentinel here is written by the WRAPPER, not by the payload, and it
# carries the exit status rather than a verdict:
#
#     DETACHED-DONE <name> rc=<n> <iso8601>
#
# It cannot be absent on a failure, because nothing about the payload's own
# output is consulted to produce it.
#
# There is one outcome the sentinel still cannot cover: the wrapper itself
# being killed, `kill -9` or a reboot. So the waiter this script prints exits
# on **either** the sentinel or the pidfile's process being gone. A waiter that
# can only see a clean finish is a waiter that hangs on a dirty one, and a hung
# waiter is indistinguishable from a long job.
#
# # WHY IT DOES NOT PARSE THE PAYLOAD'S OUTPUT
#
# It could grep the log for GREEN/RED and report a verdict. It deliberately does
# not: that would put this script back in the business of knowing which strings
# which stage prints, which is the thing that broke. The exit status is the
# payload's own verdict and is the only thing forwarded.
#
# `ci/run-detached-falsify.sh` is its gate — it asserts the sentinel appears on
# a zero exit, a non-zero exit, a syntax error, a payload killed with -9, and a
# payload that prints nothing at all, and that the printed waiter pattern
# matches each. No box, no corpus lock, a few seconds.

set -uo pipefail

STATE="${RUSTMC_DETACHED_DIR:-$HOME/.rustmc-detached}"
mkdir -p "$STATE"

usage() { sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; }

case "${1:-}" in
    ""|-h|--help) usage; exit 0 ;;
    --status)
        name="${2:?--status needs a name}"
        log="$STATE/$name.log"; pidf="$STATE/$name.pid"
        if [ ! -f "$log" ]; then echo "DETACHED-UNKNOWN $name (no log at $log)"; exit 2; fi
        if line=$(grep -a '^DETACHED-DONE ' "$log" | tail -1) && [ -n "$line" ]; then
            echo "$line"; exit 0
        fi
        if [ -f "$pidf" ] && kill -0 "$(cat "$pidf")" 2>/dev/null; then
            echo "DETACHED-RUNNING $name pid=$(cat "$pidf") log=$log"; exit 0
        fi
        # No sentinel and no process. Fail closed: this is a death, not a pass.
        echo "DETACHED-DIED $name — no sentinel and no live process. log=$log"; exit 3
        ;;
    --log) echo "$STATE/${2:?--log needs a name}.log"; exit 0 ;;
esac

NAME="$1"
case "$NAME" in
    *[!A-Za-z0-9._-]*) echo "run-detached: name must be [A-Za-z0-9._-]" >&2; exit 2 ;;
esac

# **EXTRA ARGUMENTS ARE A REFUSAL, NOT A SHRUG. 2026-09-05.**
#
# `./ci/run-detached.sh mygate bash ~/setup.sh` looks exactly like every other
# runner an agent has ever used, and it USED to be accepted: `$2` onward were
# dropped on the floor, the body was read from an empty stdin (`ssh -n` gives
# /dev/null, which is not a tty, so the `-t 0` guard below does not fire), and
# the wrapper wrote **`DETACHED-DONE <name> rc=0`** for a payload that had
# never existed. An agent then reads a green sentinel and an empty log and
# concludes its sequence ran and printed nothing.
#
# That is this project's defect class inside the one instrument every brief
# points agents at: a verdict whose green cannot be turned red, because the
# empty program cannot fail. Cost one wasted setup cycle on 2026-09-05 and
# would cost a whole gate the first time somebody detached one this way.
if [ "$#" -gt 1 ]; then
    shift
    echo "run-detached: REFUSED — the sequence is read from STDIN, not from arguments." >&2
    echo "  Ignored: $*" >&2
    echo "  A run started this way would have an EMPTY body and would report" >&2
    echo "  DETACHED-DONE rc=0 for a sequence that never ran. Write:" >&2
    echo "      ./ci/run-detached.sh $NAME <<'EOF'" >&2
    echo "      $*" >&2
    echo "      EOF" >&2
    exit 2
fi

BODY="$STATE/$NAME.body.sh"
LOG="$STATE/$NAME.log"
PIDF="$STATE/$NAME.pid"

if [ -t 0 ]; then
    echo "run-detached: the sequence is read from stdin. See ./ci/run-detached.sh --help" >&2
    exit 2
fi
cat > "$BODY"

# **AN EMPTY BODY IS A REFUSAL, for the same reason and by the other door.**
# `bash -n` passes an empty program and `bash ""` exits 0, so a body lost to a
# closed stdin, a mistyped heredoc terminator, or a pipeline that produced
# nothing becomes a green sentinel. Comments and blank lines do not count as a
# sequence: `#!/usr/bin/env bash` alone is the shape a truncated heredoc leaves.
if ! grep -qv '^[[:space:]]*\(#\|$\)' "$BODY"; then
    echo "run-detached: REFUSED — the sequence read from stdin has no command in it." >&2
    echo "  $(wc -c < "$BODY") bytes, no non-comment line. A run with an empty body" >&2
    echo "  reports DETACHED-DONE rc=0 for work that never happened." >&2
    echo "  Check the heredoc terminator, and that stdin is not /dev/null (ssh -n)." >&2
    rm -f "$BODY"
    exit 2
fi

# Refuse a body that does not parse, HERE, while an agent is looking at the
# output. Detached, it would exit 2 in a log nobody opens and the sentinel would
# say rc=2 with no clue why.
if ! bash -n "$BODY" 2>/tmp/run-detached-syntax.$$; then
    echo "run-detached: REFUSED — the sequence does not parse:" >&2
    cat /tmp/run-detached-syntax.$$ >&2; rm -f /tmp/run-detached-syntax.$$
    exit 2
fi
rm -f /tmp/run-detached-syntax.$$

if [ -f "$PIDF" ] && kill -0 "$(cat "$PIDF")" 2>/dev/null; then
    echo "run-detached: REFUSED — '$NAME' is already running as pid $(cat "$PIDF")." >&2
    echo "  Two runs under one name share a log and a sentinel, and the waiter" >&2
    echo "  then fires on the first one to finish. Pick another name." >&2
    exit 2
fi

: > "$LOG"
setsid nohup bash -c '
    body="$1"; log="$2"; pidf="$3"; name="$4"
    echo "$$" > "$pidf"
    bash "$body" >> "$log" 2>&1
    rc=$?
    printf "DETACHED-DONE %s rc=%s %s\n" "$name" "$rc" "$(date -Is)" >> "$log"
    rm -f "$pidf"
' _ "$BODY" "$LOG" "$PIDF" "$NAME" >/dev/null 2>&1 &

# Give the wrapper a moment to write its pidfile, so the waiter below cannot
# read "no pidfile" as "already dead" on the very first iteration.
for _ in 1 2 3 4 5 6 7 8 9 10; do [ -s "$PIDF" ] && break; sleep 0.2; done

cat <<EOF
DETACHED-ARMED $NAME log=$LOG pid=$(cat "$PIDF" 2>/dev/null || echo '?')
Arm exactly this, as ONE Bash call with run_in_background: true:

  until grep -qa '^DETACHED-DONE $NAME ' $LOG || ! kill -0 \$(cat $PIDF 2>/dev/null || echo 0) 2>/dev/null; do sleep 60; done; ./ci/run-detached.sh --status $NAME

Then do other work. Do not poll it: a poll costs a full context write to learn
what the notification delivers free.
EOF
