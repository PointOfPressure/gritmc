#!/usr/bin/env bash
# t2-persistence-falsify.sh — show that `ci/t2-persistence.sh` can print RED,
# and that each of the three ways a gate can go wrong reaches that RED.
#
# WHY
#
# The stage driver is a check like any other, and it is the check whose green
# `ci/handoff.sh` stage 7 reads. "It runs four scripts and prints GREEN" is
# not evidence that it prints anything else. Two of the three failure shapes
# below are ones a stage driver plausibly gets wrong and neither is visible
# from a passing run:
#
#   1  A GATE PRINTS NO SENTINEL. The commonest real failure — a script that
#      dies on a missing fixture, or refuses, exits non-zero and never prints
#      its GREEN line.
#   2  A GATE PRINTS ITS SENTINEL AND EXITS NON-ZERO. A script that proved
#      what it was asked to and then failed to clean up. The stage must not
#      wave that through: `grep -qF` alone would.
#   3  **A CONTROL THAT DID NOT FIRE.** The one that matters, and the one a
#      status-based driver cannot see at all. A control arm exits 0 when the
#      control WORKED — it prints `CONTROL GREEN` and returns success — so a
#      driver reading `$?` reports a control that silently stopped controlling
#      as a pass. The stage asserts on the CONTROL GREEN string instead, and
#      this suite is what shows that assertion is live.
#
# HOW, AND WHY IT IS A COPY RATHER THAN A FLAG
#
# The suite copies `ci/t2-persistence.sh` into a scratch tree, rewrites its
# `GATES` array to point at fake scripts, and runs the copy. There is
# deliberately NO environment override on the real script: a variable that
# points the stage at a different gate list is a hole in the stage, and the
# whole file exists to close holes of that shape. `ci/chain-falsify.sh` uses
# the same device (`CHAIN_FALSIFY_SRC`) for the same reason.
#
# No cargo, no jar, no corpus lock, no JVM. About a second, which is why it
# runs inside `ci/t0.sh`.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGE="$ROOT/ci/t2-persistence.sh"
[ -x "$STAGE" ] || { echo "T2-PERSISTENCE-FALSIFY REFUSED — no $STAGE" >&2; exit 2; }

fails=0
n=0
LAB=$(mktemp -d)
trap 'rm -rf "$LAB"' EXIT

mkdir -p "$LAB/ci" "$LAB/oracle-work/vanilla-26.2/versions/26.2"
# The jar's PRESENCE is a precondition of the real stage, so the lab needs a
# file at that path. It is never executed: every gate in the lab is a fake.
: > "$LAB/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
cp "$ROOT/ci/corpus-lock.sh" "$LAB/ci/" 2>/dev/null || true

# A fake gate. `$1` is what it prints, `$2` the status it exits with.
make_gate() { # make_gate <path> <line> <status>
    cat > "$1" <<EOF
#!/usr/bin/env bash
[ "\${1:-}" = "--negative-control" ] && { printf '%s\n' "$4"; exit $5; }
printf '%s\n' "$2"
exit $3
EOF
    chmod +x "$1"
}

# The stage, with its manifest replaced. `--no-lock` is passed by the caller
# below rather than patched in, so the real argument parser is exercised.
build_stage() { # build_stage <gates-array-body>
    python3 - "$STAGE" "$LAB/ci/t2-persistence.sh" "$1" <<'PY'
import re, sys
src, dst, body = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(src).read()
new = "GATES=(\n" + body + "\n)\n"
s = re.sub(r"GATES=\(\n.*?\n\)\n", new, s, count=1, flags=re.S)
assert "GATES=(\n" + body in s, "the GATES array could not be replaced"
open(dst, "w").write(s)
PY
    chmod +x "$LAB/ci/t2-persistence.sh"
}

run_stage() {
    ( cd "$LAB" && ./ci/t2-persistence.sh --no-lock "$@" 2>&1 )
}

ok() { # ok <label> <wanted-substring> <output>
    n=$((n + 1))
    if printf '%s' "$3" | grep -cF "$2" >/dev/null; then
        printf '  ok   %-52s %s\n' "$1" "$2"
    else
        printf '  FAIL %-52s wanted %s\n' "$1" "$2"
        printf '%s\n' "$3" | sed 's/^/         /'
        fails=$((fails + 1))
    fi
}

# --- 0. the control at the top: a lab where everything behaves goes GREEN ---
#
# Without this every case below passes on a stage that prints RED
# unconditionally, which is the mirror image of the defect this file is about.
make_gate "$LAB/ci/g-good.sh" "GOOD GREEN" 0 "GOOD CONTROL GREEN" 0
build_stage '  "ci/g-good.sh|--negative-control|GOOD GREEN|GOOD CONTROL GREEN"'
out=$(run_stage)
ok "0 everything behaves" "T2-PERSISTENCE GREEN" "$out"
ok "0 and it says how many controls ran" "1 control(s)" "$out"

# --- 1. a gate that prints no sentinel --------------------------------------
make_gate "$LAB/ci/g-silent.sh" "SOMETHING ELSE ENTIRELY" 1 "GOOD CONTROL GREEN" 0
build_stage '  "ci/g-silent.sh|--negative-control|GOOD GREEN|GOOD CONTROL GREEN"'
out=$(run_stage)
ok "1 no sentinel" "T2-PERSISTENCE RED" "$out"
ok "1 and it names the missing string" "no 'GOOD GREEN' in" "$out"

# --- 2. a gate that prints its sentinel and exits non-zero ------------------
#
# `grep -qF` alone waves this through. The stage checks the status too, and
# reports the two differently — a sentinel with a bad status is a script that
# proved its point and then broke, which is a different thing to fix from a
# script that proved nothing.
make_gate "$LAB/ci/g-dirty.sh" "GOOD GREEN" 3 "GOOD CONTROL GREEN" 0
build_stage '  "ci/g-dirty.sh|--negative-control|GOOD GREEN|GOOD CONTROL GREEN"'
out=$(run_stage)
ok "2 sentinel then non-zero exit" "T2-PERSISTENCE RED" "$out"
ok "2 and it says which" "printed its sentinel and exited 3" "$out"

# --- 3. THE ONE THAT MATTERS: a control that did not fire -------------------
#
# The control arm exits 0 and prints the gate's ORDINARY green line instead of
# its CONTROL GREEN line — which is exactly what a control that has stopped
# controlling looks like: the injected defect no longer moves the gate, so the
# gate passes, so the control script's "the gate went red as required" branch
# is never taken.
#
# A driver reading only `$?` reports this as a full pass. It is the reason
# `run_gate` asserts on the string.
make_gate "$LAB/ci/g-limp.sh" "GOOD GREEN" 0 "GOOD GREEN" 0
build_stage '  "ci/g-limp.sh|--negative-control|GOOD GREEN|GOOD CONTROL GREEN"'
out=$(run_stage)
ok "3 control did not fire, exit 0" "T2-PERSISTENCE RED" "$out"
ok "3 and it names the control" "g-limp --negative-control" "$out"

# --- 4. a gate with no control arm is still run -----------------------------
#
# `ci/anvil-byte-identity-falsify.sh` has an empty control column because it
# IS a control suite. A driver that skipped a row with an empty column, or
# that ran it with an empty argument, would both look right in a green log.
make_gate "$LAB/ci/g-solo.sh" "SOLO GREEN" 0 "unused" 0
build_stage '  "ci/g-solo.sh||SOLO GREEN|"'
out=$(run_stage)
ok "4 no control column, gate still runs" "T2-PERSISTENCE GREEN" "$out"
ok "4 and no control is counted" "0 control(s)" "$out"

# --- 5. refusals, which are not REDs ----------------------------------------
#
# A refusal means the stage could not measure; a RED means it measured and the
# answer was bad. Conflating them is how an environment fault gets recorded as
# a parity failure, which `ci/handoff.sh` has a whole INFRA verdict about.
build_stage '  "ci/g-good.sh|--negative-control|GOOD GREEN|GOOD CONTROL GREEN"'
out=$(run_stage --wat 2>&1)
ok "5 an unrecognised argument refuses" "T2-PERSISTENCE REFUSED" "$out"

out=$(run_stage --only no-such-gate 2>&1)
ok "5 --only matching nothing refuses" "T2-PERSISTENCE REFUSED" "$out"

rm -f "$LAB/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
out=$(run_stage 2>&1)
ok "5 a missing jar refuses rather than reddens" "no vanilla jar" "$out"
: > "$LAB/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"

build_stage '  "ci/g-does-not-exist.sh|--negative-control|X GREEN|X CONTROL GREEN"'
out=$(run_stage 2>&1)
ok "5 a missing gate script refuses" "T2-PERSISTENCE REFUSED" "$out"

# --- 6. --only selects, and runs the control too ----------------------------
make_gate "$LAB/ci/g-a.sh" "A GREEN" 0 "A CONTROL GREEN" 0
make_gate "$LAB/ci/g-b.sh" "B GREEN" 0 "B CONTROL GREEN" 0
build_stage '  "ci/g-a.sh|--negative-control|A GREEN|A CONTROL GREEN"
  "ci/g-b.sh|--negative-control|B GREEN|B CONTROL GREEN"'
out=$(run_stage --only g-a)
ok "6 --only runs one gate" "1 gate(s), 1 control(s)" "$out"
ok "6 and it is the one asked for" "ok g-a " "$out"
n=$((n + 1))
if printf '%s' "$out" | grep -cF "ok g-b " >/dev/null; then
    printf '  FAIL %-52s g-b ran under --only g-a\n' "6 and only that one"
    fails=$((fails + 1))
else
    printf '  ok   %-52s g-b did not run\n' "6 and only that one"
fi

echo
if [ "$fails" -eq 0 ]; then
    echo "T2-PERSISTENCE-FALSIFY GREEN — $n assertions, 0 failed"
    exit 0
fi
echo "T2-PERSISTENCE-FALSIFY RED — $fails of $n assertions failed"
exit 1
