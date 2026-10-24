#!/usr/bin/env bash
# Prove `hooks/agent-brief-gate.sh` decides, and that its 2026-08-19 repair
# changed the decision it makes.
#
#     ./ci/brief-gate-falsify.sh          # ~2 s, no build, no jar, no box
#
# WHY THIS EXISTS
#
# The gate is a PreToolUse hook, so nothing in the repo runs it and nothing in
# the repo notices when it stops deciding. It has already failed that way once:
# on 2026-08-17 the concurrency arm counted detached compute as live agents and
# denied every spawn with zero agents running — a cap meant to stop twenty
# agents enforcing none — and it was found by an operator being blocked, not by
# a check.
#
# THE CASE THAT IS THE WHOLE ARGUMENT is `low-count`. Before 2026-08-19 an
# eight-commit brief was DENIED. After it, the same brief WARNS and proceeds.
# So this falsifier reconstructs the pre-repair arm from the live script and
# asserts the old decision on the old code and the new decision on the new,
# which is the only way to show a repair to a rule that now passes everything
# it used to fail. A test that only ran the new script would be green whether
# the arm existed or not.
#
# NEGATIVE CONTROLS. Four cases must stay GREEN, and each of them is a way this
# gate could have been written to refuse everything:
#
#   * `well-formed`      — a real area brief at N=20 is never touched.
#   * `not-rustmc`       — another project's agent passes through with no AREA.
#   * `area-is-a-phrase` — "the gameplay corpus" is not bug-shaped.
#   * `hyphenated-area`  — "worldgen-parity-and-performance" has a figure id's
#                          SHAPE and is not a figure id. Arm 2b consults the
#                          registry, not the punctuation, and this case is what
#                          says so. Without it, 2b would be a rule against
#                          hyphens.
#
# Exit 0 all cases behaved; 1 otherwise.
set -uo pipefail

HOOK=${HOOK:-$(dirname "$0")/../hooks/agent-brief-gate.sh}
[ -x "$HOOK" ] || { echo "BRIEF-GATE-FALSIFY: no hook at $HOOK"; exit 1; }

TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
fail=0

# A hermetic registry, so the figure-id arm is tested against known contents
# rather than against whatever is registered today.
cat > "$TMP/figures.toml" <<'TOMLEOF'
[[figure]]
id = "ow-r16-floor-n2"
TOMLEOF

# The pre-repair script: put the blocking arm back exactly as it read, so the
# old decision is asserted on old code rather than described in a comment. The
# warn() call is a multi-line string, so the span is replaced by locating its
# closing quote rather than by a line-wise sed, which would leave the tail of
# the old string dangling and produce a script that cannot parse.
python3 - "$HOOK" "$TMP/pre-repair.sh" <<'PYEOF'
import sys
src = open(sys.argv[1]).read().split('\n')
starts = [i for i, l in enumerate(src) if l.startswith('    warn "SIZING CHECK')]
if len(starts) != 1:
    sys.exit(f"expected exactly one SIZING CHECK warn block, found {len(starts)}")
i = starts[0]
j = i
while not src[j].rstrip().endswith('"') or j == i:
    j += 1
    if j >= len(src):
        sys.exit("SIZING CHECK warn block never closes")
src[i:j + 1] = ['    deny "BLOCKED: EXPECTED COMMITS is $n, below the floor of $SIZING_FLOOR."']
open(sys.argv[2], 'w').write('\n'.join(src))
PYEOF
chmod +x "$TMP/pre-repair.sh"
if ! bash -n "$TMP/pre-repair.sh" 2>/dev/null; then
    echo "BRIEF-GATE-FALSIFY: the reconstructed pre-repair script does not parse."
    echo "  Fix the reconstruction rather than deleting the case: without it the"
    echo "  repair is untested, because the new script passes whether the arm"
    echo "  was ever there or not."
    exit 1
fi
if ! grep -q 'BLOCKED: EXPECTED COMMITS is \$n, below the floor' "$TMP/pre-repair.sh"; then
    echo "BRIEF-GATE-FALSIFY: could not reconstruct the pre-repair arm."
    exit 1
fi

run() {   # run <script> <prompt> -> prints the hook's JSON
    local script=$1 prompt=$2
    python3 -c 'import json,sys; print(json.dumps({"tool_name":"Agent","tool_input":{"prompt":sys.argv[1]}}))' "$prompt" \
        | RUSTMC_FIGURES="$TMP/figures.toml" RUSTMC_MAX_AGENTS=${MAXA:-99} bash "$script"
}

decision() {  # -> deny | warn | pass
    python3 -c '
import json,sys
raw=sys.stdin.read().strip()
if not raw: print("pass"); sys.exit()
d=json.loads(raw)
if d.get("hookSpecificOutput",{}).get("permissionDecision")=="deny": print("deny")
elif "systemMessage" in d: print("warn")
else: print("pass")'
}

expect() {  # expect <name> <want> <script> <prompt> [substring the reason must carry]
    local name=$1 want=$2 script=$3 prompt=$4 must=${5:-}
    local out got
    out=$(run "$script" "$prompt")
    got=$(printf '%s' "$out" | decision)
    if [ "$got" != "$want" ]; then
        echo "  RED  $name: expected $want, got $got"
        fail=1
        return
    fi
    if [ -n "$must" ] && ! printf '%s' "$out" | grep -cF "$must" >/dev/null; then
        echo "  RED  $name: decision $got is right but the text never says '$must'"
        fail=1
        return
    fi
    echo "  ok   $name -> $got"
}

AREA_OK='AREA: the gameplay corpus
EXPECTED COMMITS: 20
Work in /home/sayer/grit/work-x on rustmc.'

echo "-- negative controls (these must stay green) --"
expect "well-formed"      pass "$HOOK" "$AREA_OK"
expect "not-rustmc"       pass "$HOOK" "Please tidy up the sayer.pages.dev deploy script."
expect "area-is-a-phrase" pass "$HOOK" "$AREA_OK"
expect "hyphenated-area"  pass "$HOOK" 'AREA: worldgen-parity-and-performance
EXPECTED COMMITS: 20
rustmc'

echo "-- the arms that must be able to refuse --"
expect "no-area"          deny "$HOOK" 'Fix the null check in rustmc ci/handoff.sh.' "declares no AREA"
expect "no-count"         deny "$HOOK" 'AREA: worldgen parity
rustmc work.' "SIZING CHECK, NOT A TARGET"
expect "area-is-a-bug"    deny "$HOOK" 'AREA: claim 7
EXPECTED COMMITS: 20
rustmc' "single claim or bug number"
expect "area-is-a-file"   deny "$HOOK" 'AREA: ci/handoff.sh
EXPECTED COMMITS: 20
rustmc' "names a single file"
expect "area-is-a-figure" deny "$HOOK" 'AREA: ow-r16-floor-n2
EXPECTED COMMITS: 20
rustmc' "single registered figure"
MAXA=0 expect "over-cap"  deny "$HOOK" "$AREA_OK" "GATE'S DRAIN RATE"

echo "-- the repair: one input, two decisions --"
LOW='AREA: the gate and CI machinery
EXPECTED COMMITS: 8
rustmc'
expect "low-count-BEFORE" deny "$TMP/pre-repair.sh" "$LOW" "below the floor"
expect "low-count-AFTER"  warn "$HOOK"              "$LOW" "NOT BLOCKED"

echo "-- the bypass still bypasses --"
if [ -n "$(RUSTMC_BRIEF_GATE=off run "$HOOK" 'rustmc with no area at all')" ]; then
    echo "  RED  bypass: RUSTMC_BRIEF_GATE=off still emitted a decision"; fail=1
else
    echo "  ok   bypass -> pass"
fi

if [ "$fail" = 0 ]; then echo "BRIEF-GATE-FALSIFY GREEN"; else echo "BRIEF-GATE-FALSIFY RED"; fi
exit $fail
