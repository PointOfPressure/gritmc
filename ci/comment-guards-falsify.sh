#!/usr/bin/env bash
# The positive control for `ci/check-comment-guards.py`.
#
# A checker for claims nothing verifies, that nothing verifies, would be the
# joke writing itself. Each arm below mutates a scratch COPY of the tree into
# one instance of the class and requires the checker to refuse it by name.
#
# **Arm `grep-shell-emitter` is the discriminating one and it expects GREEN.**
# The other arms only show the rules can fire; a rule that fired on everything
# would pass all of them and be worthless. That arm adds the same sentinel with
# a *reachable* producer — a shell `echo` in a script something calls — and
# requires silence. It is the half that says the rule measures reachability and
# not merely the presence of a string.
#
# Usage:  ./ci/comment-guards-falsify.sh
# Exit:   0 every arm behaved as required | 1 an arm did not
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/tree"
tar -C "$ROOT" -cf - ci crates tools plugins swarm 2>/dev/null | tar -C "$WORK/tree" -xf -
CHK="$WORK/tree/ci/check-comment-guards.py"
PASS=0
FAIL=0

restore() {
    rm -rf "$WORK/tree"
    mkdir -p "$WORK/tree"
    tar -C "$ROOT" -cf - ci crates tools plugins swarm 2>/dev/null | tar -C "$WORK/tree" -xf -
}

# `check <arm> <red|green> <pattern>` — run the checker over the mutated copy.
#
# NOT `"$CHK" | grep -q`: this script runs under `set -o pipefail` and the
# checker exits non-zero precisely when it has something to say, so a pipeline
# would report failure on the arm working. That read as "STAYED GREEN" on the
# first run of this file — a control reporting the opposite of what happened.
check() {
    local name="$1" want="$2" pat="${3:-}" out
    out="$("$CHK" --emit 2>&1)"
    case "$want" in
        red)
            if printf '%s' "$out" | grep -cF "$pat" >/dev/null; then
                echo "ARM $name RED as required — $pat"
                PASS=$((PASS + 1))
            else
                echo "ARM $name STAYED GREEN — expected $pat"
                printf '%s\n' "$out" | sed 's/^/      /' | head -8
                FAIL=$((FAIL + 1))
            fi
            ;;
        green)
            if printf '%s' "$out" | grep -cF "$pat" >/dev/null; then
                echo "ARM $name WENT RED — the rule fired on a reachable producer: $pat"
                FAIL=$((FAIL + 1))
            else
                echo "ARM $name GREEN as required — $pat not reported"
                PASS=$((PASS + 1))
            fi
            ;;
    esac
}

# --- arm 1: a `# Panics` section over a body that returns ------------------
# §73.8's first instance, in the form it had before `t1-audit` repaired it.
restore
python3 - "$WORK/tree" <<'PY'
import sys
from pathlib import Path
p = Path(sys.argv[1]) / "crates/rustmc-worldgen/src/feature/state_spec.rs"
s = p.read_text()
i = s.index("pub fn extract_or_skip() -> Option<DefaultStateTable> {")
j = s.index("\n}\n", i)
s = s[:i] + "pub fn extract_or_skip() -> Option<DefaultStateTable> {\n    return None;\n}\n" + s[j + 3:]
p.write_text(s)
PY
check doc-panic-without-panic red "DOCUMENTED-PANIC-WITHOUT-PANIC"

# --- arm 2: `# Panics / Never.` over a body that plainly can ---------------
# The mirror image, and the more dangerous direction: a caller relies on it.
restore
python3 - "$WORK/tree" <<'PY'
import sys
from pathlib import Path
p = Path(sys.argv[1]) / "crates/rustmc-server/src/broadcast.rs"
s = p.read_text()
s = s.replace(
    "/// # Panics\n/// If a previous holder panicked while holding it.\npub fn tick_lock()",
    "/// # Panics\n/// Never.\npub fn tick_lock()", 1)
s = s.replace(
    "    tick_mutex()\n        .lock()\n        .unwrap_or_else(std::sync::PoisonError::into_inner)",
    "    tick_mutex().lock().unwrap()", 1)
p.write_text(s)
PY
check never-panics-but-can red "DOCUMENTED-NEVER-PANICS-BUT-CAN"

# --- arm 3: a sentinel only a captured Rust print writes -------------------
# §73.2's shape, built from nothing: a gate greps a log for a string that only
# a passing test prints, so the bytes never reach the log.
restore
cat > "$WORK/tree/ci/zz-falsify-gate.sh" <<'INNER'
#!/usr/bin/env bash
# A stand-in gate for ci/comment-guards-falsify.sh arm 3.
LOG=/dev/null
N=$(grep -c "zzfalsify sentinel absent" "$LOG")
[ "$N" -eq 0 ] || exit 1
INNER
chmod +x "$WORK/tree/ci/zz-falsify-gate.sh"
python3 - "$WORK/tree" <<'PY'
import sys
from pathlib import Path
p = Path(sys.argv[1]) / "crates/rustmc-worldgen/tests/probe_prelim.rs"
s = p.read_text()
s += '\n#[allow(dead_code)]\nfn zz_announce() { println!("zzfalsify sentinel absent"); }\n'
p.write_text(s)
PY
check grep-only-captured red "GREP-PATTERN-ONLY-CAPTURED-PRODUCER: 'zzfalsify sentinel absent'"

# --- arm 4: THE DISCRIMINATING ARM — the same sentinel, reachably produced --
# Identical to arm 3 plus a shell `echo` in a script something calls. If the
# rule still fires, it is measuring the presence of a string and not whether a
# grepped log could ever carry it, and arms 1-3 prove nothing.
cat > "$WORK/tree/ci/zz-falsify-emit.sh" <<'INNER'
#!/usr/bin/env bash
echo "zzfalsify sentinel absent"
INNER
chmod +x "$WORK/tree/ci/zz-falsify-emit.sh"
printf '\n./ci/zz-falsify-emit.sh\n' >> "$WORK/tree/ci/zz-falsify-gate.sh"
check grep-shell-emitter green "GREP-PATTERN-ONLY-CAPTURED-PRODUCER: 'zzfalsify sentinel absent'"

# --- arm 5: a test that names itself a control and asserts nothing ---------
# Live in the tree and declared, so the arm is the DECLARATION: red without it.
#
# **MATCHED BY FILENAME, NOT BY LINE — repaired 2026-08-19.** This grep read
# `probe_placement_vs_servers.rs:62`, the line the instance sat on when this
# arm was written on `t1-positive-controls`. On `main` the same instance is at
# :63, so the grep removed NOTHING, the declaration survived, the checker
# stayed green and this arm reported the checker as toothless — which is the
# arm testing itself rather than the checker. A falsifier that hardcodes a line
# number goes silently vacuous the first time the file it names is edited, and
# it is the one kind of check that cannot afford to.
#
# `total` is lowered to match, because `ci/shell-footguns.py`'s sibling rule
# would otherwise red on the count disagreeing with the entries.
restore
grep -v 'probe_placement_vs_servers\.rs:' "$ROOT/ci/checks/comment-guards.toml" \
    | sed 's/^total = 2$/total = 1/' \
    > "$WORK/tree/ci/checks/comment-guards.toml"
ARM5="$("$CHK" 2>&1)"
if printf '%s' "$ARM5" | grep -c "CLAIM-WITHOUT-ASSERTION" >/dev/null; then
    echo "ARM toothless-control RED as required — CLAIM-WITHOUT-ASSERTION"
    PASS=$((PASS + 1))
else
    echo "ARM toothless-control STAYED GREEN — the undeclared instance was not reported"
    printf '%s\n' "$ARM5" | sed 's/^/      /' | head -8
    FAIL=$((FAIL + 1))
fi

# --- arm 6: and green with everything restored ----------------------------
restore
if CLEAN="$("$CHK" 2>&1)"; then
    echo "ARM restored GREEN as required"
    PASS=$((PASS + 1))
else
    echo "ARM restored STAYED RED — the checker does not pass on an unmutated tree"
    printf '%s\n' "$CLEAN" | sed 's/^/      /' | head -10
    FAIL=$((FAIL + 1))
fi

# --- arm 7: a printed name is not a called script -------------------------
#
# `script_is_invoked` decides whether a shell script can write into a log
# anybody greps, and the whole GREP-PATTERN rule rests on it. THE SHAPE THAT
# BROKE IT WAS AN ADVICE MESSAGE: `print("./ci/comment-guards-falsify.sh")`
# inside a "run this next" string made the function report this very falsifier
# as live, so the rule depending on it went quiet — a checker switched off by
# its own advice.
#
# The backtick rule that predates this catches a citation in prose and does not
# catch that. Live in the tree when this arm was written: `ci/t0.sh` calls this
# script on one line and prints `echo "T0 RED — ci/comment-guards-falsify.sh:
# ..."` on the next, and both counted as calls.
#
# Asserted three ways in a hermetic tree, because the middle case is the only
# one that separates the repair from a rule against the word `echo`:
#   only an echo naming it -> NOT invoked
#   a real call            -> invoked
#   echo, then a call      -> invoked   (scoping is per command segment)
ARM7_TMP="$(mktemp -d)"
mkdir -p "$ARM7_TMP/ci"
cp "$CHK" "$ARM7_TMP/ci/check-comment-guards.py"
cp "$ROOT/ci/shellstrip.py" "$ARM7_TMP/ci/shellstrip.py"
printf 'echo hi\n' > "$ARM7_TMP/ci/target.sh"
arm7() {
    printf '%s' "$2" > "$ARM7_TMP/ci/caller.sh"
    python3 - "$ARM7_TMP" <<'PYEOF7'
import importlib.util, sys
from pathlib import Path
root = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("cg", root / "ci" / "check-comment-guards.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
print("yes" if m.script_is_invoked(root / "ci" / "target.sh") else "no")
PYEOF7
}
arm7_check() {
    local label="$1" body="$2" want="$3" got
    got="$(arm7 "$label" "$body")"
    if [ "$got" = "$want" ]; then
        echo "ARM printed-is-not-called: $label -> $got as required"
        PASS=$((PASS + 1))
    else
        echo "ARM printed-is-not-called: $label -> $got, wanted $want"
        FAIL=$((FAIL + 1))
    fi
}
arm7_check "only an echo naming it" 'echo "T0 RED — ci/target.sh: it went green" >&2
' no
arm7_check "a real call"            './ci/target.sh > /tmp/log 2>&1
' yes
arm7_check "echo THEN a call"       'echo running; ./ci/target.sh
' yes
rm -rf "$ARM7_TMP"

echo "COMMENT-GUARDS-FALSIFY $PASS as required, $FAIL not"
[ "$FAIL" -eq 0 ] || exit 1
