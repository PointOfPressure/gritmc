#!/usr/bin/env bash
# PreToolUse gate on the Agent tool for the rustmc run.
#
# WHY
#
# Throughput collapsed 6x (173 commits/day on 10 Aug -> 31 on 16 Aug) because
# the orchestrator spawned one agent per DEFECT instead of one per AREA. Every
# rustmc agent reads a fixed prelude before editing anything, and that tax is
# FIXED PER AGENT — six agents on one area pay it six times for identical
# reading.
#
# The owner's ruling, 2026-08-16: "one agent, one big task, not spawning them
# in on individual bugs... if this doesn't change gritmc will NEVER finish."
#
# WHAT FIXED IT WAS THE SUBSYSTEM BOUNDARY, NOT THE NUMBER — 2026-08-19.
#
# This gate originally BLOCKED any brief declaring fewer than ten expected
# commits. Thirty instrumented hours say that arm was the wrong half of the
# rule. The lanes that produced the run's best work were area-shaped, and the
# count they hit was irrelevant to it: three finished honestly UNDER their
# target (10 not 20, 13 not 22, 8 not 20) and one correctly refused to port
# anything at all, because nothing it could have written would have been
# verifiable. That refusal was the single most valuable output of the night,
# and under the old arm a lane that foresaw it could not have been briefed.
#
# A floor on the count does not distinguish an area from a bug. It only
# distinguishes an honest estimate from a flattering one, and it selects for
# the flattering one. So the count is now a SIZING CHECK: it must be stated,
# because stating it forces the judgement, and a low one is answered with a
# question rather than a refusal.
#
# The rule that does distinguish an area from a bug is enforced instead, and
# mechanically: a brief whose AREA names one claim number, one figure id, one
# file or one bug is a bug-shaped brief however many commits it promises.
#
# WHAT IT ENFORCES
#
#   1. Every brief declares an AREA, and that AREA is not a single claim,
#      figure, file or bug.                                          (BLOCKS)
#   2. Every brief states EXPECTED COMMITS, so the size is judged.   (BLOCKS)
#   3. A count below the sizing floor is queried, not refused.       (WARNS)
#   4. No more than MAX_AGENTS agent worktrees have live processes.  (BLOCKS)
#
# It denies with a reason rather than silently mangling the call. Set
# RUSTMC_BRIEF_GATE=off in the environment to bypass for a genuine one-off.
set -uo pipefail

# --- lane count, derived from the gate's drain rate ---------------------------
#
# THE DERIVATION IS WRITTEN OUT WITH ITS INPUTS SO IT CAN BE RE-RUN, and that
# is the point of this block rather than the value at the bottom of it. This
# number has now been wrong twice for the same reason: 8 and the 4 before it
# were budget decisions, and the 4 that replaced them on 2026-08-19 INHERITED a
# drain rate instead of measuring one. A constant with no derivation beside it
# cannot be checked when the thing it was derived from moves, and the gate cost
# moves constantly.
#
#   INPUT 1 — the gate's drain rate, in branches landed per day.
#             9.4 to 11.6, measured 2026-08-19 from the gate's own logs:
#             GREEN 107.9 min (n = 4), RED 26.1 min (n = 7).
#             SUPERSEDES 22/day, which came from
#             docs/ASSESSMENT-2026-08-19.md section 4.1 pricing a gate at 66.1
#             expected minutes. That figure was inherited here and not checked.
#
#   INPUT 2 — branch production, in branches per lane per day.
#             6.3, measured 2026-08-19: 63 remote branch tips moved in the 30
#             hours to 18:34 BST with MAX_AGENTS=8.
#
#   9.4 / 6.3 = 1.5        11.6 / 6.3 = 1.8        ->  MAX_AGENTS = 2
#
# Not every one of those 63 branches came from a lane — some are orchestrator
# pushes — and the eight lanes were not all live for all thirty hours. Both
# errors push INPUT 2 down, so 6.3 is a lower bound on per-lane production and
# the quotient is an UPPER bound on lane count. Hence "at most 2", not "2".
#
# TWO LANES MEASURED INPUT 1 ON THE SAME DAY AND GOT FIGURES 2x APART. This
# file uses the second — 9.4 to 11.6, taken 2026-08-19 against an `origin/main`
# that did not move at all during the measuring lane's run. It is NAMED rather
# than averaged with the other, because a reader has to be able to tell which
# measurement this constant rests on, and a silently merged pair of
# disagreeing measurements is a number with no provenance. docs/FINDINGS.md
# carries both.
#
# THERE IS NO BATCHED ALTERNATIVE, and an earlier draft of this file offered
# one. Batching was REFUSED on 2026-08-19: at today's costs no width beats
# serial, and the `p = 0.8217 at N = 157` that justified it is NOT
# REPRODUCIBLE — those gate logs are gone, 28 survive, giving 11 verdicts and
# p = 0.3636. Do not set RUSTMC_MAX_AGENTS to 6 on the strength of batching.
#
# TO CHANGE THIS NUMBER: re-measure both inputs, divide, and rewrite the block
# above with the new inputs and their dates. Do not adjust the constant.
#
# Anything above this line is inventory. In the thirty hours to 2026-08-19 main
# moved 198 commits while the unlanded backlog GREW, 1,342 to 1,377 distinct
# changes, which is what producing above the drain rate looks like from the
# outside.
MAX_AGENTS=${RUSTMC_MAX_AGENTS:-2}

# The sizing floor. Below this the gate asks whether the work is an area; it
# does not refuse. See the header.
SIZING_FLOOR=${RUSTMC_MIN_COMMITS:-10}

# The registry, consulted only to tell a figure id from a hyphenated phrase.
# Injectable so the falsifier can be hermetic; absent, arm 2b's figure-id test
# simply never fires, which is the safe direction.
FIGURES=${RUSTMC_FIGURES:-/home/sayer/grit/repo/ci/figures.toml}

deny() {
    # PreToolUse: permissionDecision "deny" blocks the call and shows the reason.
    python3 -c '
import json,sys
print(json.dumps({"hookSpecificOutput":{
    "hookEventName":"PreToolUse",
    "permissionDecision":"deny",
    "permissionDecisionReason":sys.argv[1]}}))' "$1"
    exit 0
}

warn() {
    # Not a decision. The call proceeds; the text is surfaced to the operator.
    python3 -c '
import json,sys
print(json.dumps({"systemMessage": sys.argv[1]}))' "$1"
    exit 0
}

payload=$(cat)
[ "${RUSTMC_BRIEF_GATE:-on}" = off ] && exit 0

prompt=$(printf '%s' "$payload" | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("tool_input",{}).get("prompt",""))
except Exception: print("")')

# Only govern rustmc work. Other projects' agents pass straight through.
printf '%s' "$prompt" | grep -qiE 'rustmc|gritmc|/home/sayer/grit' || exit 0

# --- 1. concurrency -----------------------------------------------------------
# Count by process CWD, not by command line. `pgrep -af <path>` matches the
# shell that carries the path in its own argv, so every worktree read as live
# and the gate would have blocked every agent forever. /proc/<pid>/cwd cannot
# self-match: this script's cwd is not a worktree.
#
# But CWD ALONE COUNTS COMPUTE, NOT AGENTS — and this cap is on agents.
# 2026-08-17 01:12: five detached evidence sweeps were running t2-gameplay in
# five worktrees, all reparented to pid 1 because the agents that launched them
# had finished. Exactly the behaviour the brief asks for — "script the whole
# sequence and run it detached" — and it read as five live agents, so the gate
# denied every new agent with ZERO agents running. A cap meant to stop twenty
# agents was instead enforcing none, which is the same failure it was built to
# prevent.
#
# So: a worktree is agent-occupied only when a `claude` process is in the
# ancestry of whatever holds that CWD. Orphaned detached work is compute; it
# pays no reading tax, which is the entire thing this cap rations.
live=$(python3 - <<'PYEOF'
import os, glob
roots = {os.path.realpath(d) for d in glob.glob('/home/sayer/grit/work-*') if os.path.isdir(d)}

# pid -> (ppid, comm), read once. /proc races with exiting processes, so every
# read is guarded and a vanished pid simply does not count.
proc = {}
for p in glob.glob('/proc/[0-9]*'):
    pid = os.path.basename(p)
    try:
        with open(os.path.join(p, 'stat')) as f:
            stat = f.read()
        # comm is parenthesised and may itself contain spaces or ')', so split
        # on the LAST ')' — the field after it is state, then ppid.
        rp = stat.rindex(')')
        proc[pid] = (stat[rp + 2:].split()[1], stat[stat.index('(') + 1:rp])
    except (OSError, ValueError):
        continue

def has_claude_ancestor(pid):
    seen = set()
    while pid in proc and pid not in seen and pid != '1':
        seen.add(pid)
        ppid, comm = proc[pid]
        if comm == 'claude':
            return True
        pid = ppid
    return False

busy = set()
for pid in proc:
    try:
        cwd = os.path.realpath(os.path.join('/proc', pid, 'cwd'))
    except OSError:
        continue
    for r in roots:
        if cwd == r or cwd.startswith(r + os.sep):
            if has_claude_ancestor(pid):
                busy.add(r)
            break
print(len(busy))
PYEOF
)
if [ "$live" -ge "$MAX_AGENTS" ]; then
    deny "BLOCKED: $live rustmc agent worktrees already have live processes (max $MAX_AGENTS).

This cap is the GATE'S DRAIN RATE, not a budget, and it is derived rather than
decided. Measured 2026-08-19: the gate lands 9.4 to 11.6 branches a day (GREEN
107.9 min, n=4; RED 26.1 min, n=7), and lanes produced 6.3 branches each per
day over the preceding 30 hours. 9.4/6.3 = 1.5, 11.6/6.3 = 1.8.

Every branch produced above that line is inventory that depreciates as main
moves underneath it: in those same 30 hours main took 198 commits while the
unlanded backlog GREW, 1,342 to 1,377.

BATCHING DOES NOT RAISE THIS. It was refused on 2026-08-19 — at today's costs
no width beats serial — so do not set RUSTMC_MAX_AGENTS to 6.

Wait for a lane to finish, or fold this work into the brief of the agent that
already owns the area. To change the cap, re-measure both inputs and rewrite
the derivation at the top of this hook; do not adjust the constant.

See /home/sayer/grit/RESUME-NOW.md and docs/FINDINGS.md."
fi

# --- 2. the brief must be area-shaped ----------------------------------------
if ! printf '%s' "$prompt" | grep -qiE '^[[:space:]]*(#+[[:space:]]*)?\**AREA\**[[:space:]]*:'; then
    deny "BLOCKED: this brief declares no AREA.

One agent owns one AREA end to end. A brief whose whole scope is one file or
one bug belongs folded into the agent that owns that area.

Add two lines:

    AREA: <the subsystem this agent owns, e.g. the gameplay corpus>
    EXPECTED COMMITS: <N — a sizing check, not a target; see below>

Natural areas: the gameplay corpus; worldgen parity; worldgen performance; the
gate/CI machinery; the docs-and-figures ledger; the working method itself.
See /home/sayer/grit/RESUME-NOW.md."
fi

# --- 2b. ...and the AREA must not be one claim, one figure, one file, one bug -
#
# This is the arm that actually stops per-bug spawning. It replaces the commit
# floor, which never distinguished an area from a bug and only ever selected
# for a flattering estimate.
area=$(printf '%s' "$prompt" | grep -iE '^[[:space:]]*(#+[[:space:]]*)?\**AREA\**[[:space:]]*:' \
    | head -1 | sed -E 's/^[[:space:]]*(#+[[:space:]]*)?\**AREA\**[[:space:]]*:[[:space:]]*//' \
    | sed -E 's/[[:space:]]+$//')

bug_shaped=""
# one bare number, or "claim 7" / "figure 7" / "bug #7" / "issue 7"
printf '%s' "$area" | grep -qiE '^(claim|figure|bug|issue|defect|red)?[[:space:]]*#?[0-9]+$' \
    && bug_shaped="it names a single claim or bug number"
# a single path with an extension and nothing else
printf '%s' "$area" | grep -qE '^[A-Za-z0-9_./-]+\.[A-Za-z0-9]+$' \
    && bug_shaped="it names a single file"
# a single registry figure id and nothing else
printf '%s' "$area" | grep -qE '^[a-z0-9]+(-[a-z0-9]+){2,}$' \
    && grep -q "^id = \"$area\"$" "$FIGURES" 2>/dev/null \
    && bug_shaped="it names a single registered figure"

if [ -n "$bug_shaped" ]; then
    deny "BLOCKED: \"$area\" is not an area — $bug_shaped.

A brief naming one claim number is a bug, not an area. That distinction is what
fixed the 6x throughput collapse of 2026-08-16, and it is the only part of this
gate that ever did: the commit floor it replaced never told an area from a bug.

Name the subsystem that owns this defect and give that whole subsystem to one
agent. If the subsystem already has a lane, fold this into that lane's brief
instead of spawning for it.

See /home/sayer/grit/RESUME-NOW.md."
fi

# --- 3. the size must be stated -----------------------------------------------
n=$(printf '%s' "$prompt" | grep -ioE 'EXPECTED[[:space:]]+COMMITS[[:space:]]*:[[:space:]]*[0-9]+' \
    | grep -oE '[0-9]+' | head -1)
if [ -z "$n" ]; then
    deny "BLOCKED: the brief declares an AREA but no EXPECTED COMMITS.

Add:  EXPECTED COMMITS: <N>

N IS A SIZING CHECK, NOT A TARGET, and it is required because stating it forces
the judgement, not because the agent must hit it. If you cannot see roughly $SIZING_FLOOR
commits of work in this subsystem, that is the signal it is not an area — fold
it into the lane that owns the subsystem rather than spawning for it.

The agent is not scored on reaching N. Finishing early and honestly is a
success, and so is refusing to port on verifiability grounds: three lanes
finished under target last run and one correctly refused to port anything at
all, which was the most valuable single output of the night. Say so in the
brief, or the number trains that behaviour out.

See /home/sayer/grit/RESUME-NOW.md."
fi

# --- 4. a low count is a question, not a refusal ------------------------------
if [ "$n" -lt "$SIZING_FLOOR" ]; then
    warn "SIZING CHECK: EXPECTED COMMITS is $n, under the usual $SIZING_FLOOR. NOT BLOCKED.

If \"$area\" really is a subsystem, carry on — a small honest estimate is fine
and finishing under it is a success. If $n commits is all the work there is,
that is the signal this is a bug rather than an area, and it belongs in the
brief of the lane that owns the subsystem.

Until 2026-08-19 this arm refused the call. It was wrong: the count never told
an area from a bug, it only selected for a flattering estimate, and it would
have blocked the lane that correctly refused to port anything."
fi

exit 0
