#!/usr/bin/env bash
# gameplay-difficulty-falsify.sh — the gate on `ci/gameplay-difficulty.sh`.
#
#     ./ci/gameplay-difficulty-falsify.sh
#
# No box, no corpus lock, no server, no cargo. Under two seconds.
#
# # WHAT IT HAS TO PROVE
#
# `ci/gameplay-difficulty.sh` exists because the harness ran vanilla at
# `peaceful` and this server at its own default, and **nothing read either
# value back**. A repair for that which is itself unreadable would be the same
# defect one layer up, so this suite is written the way
# `ci/unfalsifiable-falsify.py` is: it reconstructs the shipped defect and
# asserts the repair refuses it.
#
# Two halves:
#
#   * **the predicate** — `difficulty_or_die` against fixture trees for each of
#     the four states a subject can be in. The one that matters is
#     `has-difficulty, no-flag`: that is exactly where `mob-in-world`,
#     `mob-combat` and `mobs-in-world-2` were, and it must REFUSE. The other
#     three must not, because a refusal on `main` — which has no difficulty at
#     all — would stop every gate in the queue for a hazard that cannot exist
#     there.
#
#   * **the symmetry, textually** — the shipped defect was not a wrong value,
#     it was **two independent literals**, one in vanilla's `server.properties`
#     and one absent from the rustmc argv. So the suite injects that shape back
#     into a copy of each harness script and asserts it is caught: a hardcoded
#     `difficulty=<name>` on the vanilla side, and a launch line that does not
#     carry `DIFFICULTY_ARGS`. A test that only checked today's text would go
#     green forever on the day somebody re-hardcodes it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); printf '  ok    %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$1"; }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1 — expected [$3], got [$2]"; fi; }

# --- fixture trees -----------------------------------------------------------
# Only the two files the predicate reads. Anything else would be a fixture that
# has to be maintained beside the real tree, which is how a fixture rots.
mk() { # mk <name> <has-setter 0|1> <has-flag 0|1>
    local d="$T/$1"
    mkdir -p "$d/crates/rustmc-server/src"
    : > "$d/crates/rustmc-server/src/main.rs"
    : > "$d/crates/rustmc-server/src/mob.rs"
    [ "$3" = 1 ] && printf 'if let Some(i) = args.iter().position(|a| a == "--difficulty") {}\n' \
        >> "$d/crates/rustmc-server/src/main.rs"
    [ "$2" = 1 ] && printf 'pub fn set_difficulty(name: &str) -> Result<(), String> { Ok(()) }\n' \
        >> "$d/crates/rustmc-server/src/mob.rs"
    echo "$d"
}

# Run the predicate in a subshell so each case gets clean globals.
run() { # run <tree> <GAMEPLAY_DIFFICULTY or ""> ; prints "<rc>|<args>|<line>"
    # NOT a command substitution around `difficulty_or_die`: that would run it
    # in a subshell of its own and `DIFFICULTY_ARGS` — the thing under test —
    # would die with it, leaving every case reading an empty array and passing
    # for the wrong reason. Caught by this suite on its first run.
    (
        set +e
        unset GAMEPLAY_DIFFICULTY
        [ -n "${2:-}" ] && export GAMEPLAY_DIFFICULTY="$2"
        . "$ROOT/ci/gameplay-difficulty.sh"
        difficulty_or_die "$1" > "$T/line" 2>/dev/null; rc=$?
        printf '%s|%s|%s\n' "$rc" "${DIFFICULTY_ARGS[*]:-}" "$(cat "$T/line")"
    )
}

echo "== the predicate, one case per subject state"

r="$(run "$(mk main 0 0)" "")"
check "main (no difficulty, no flag) is accepted"        "${r%%|*}" "0"
check "main passes no difficulty argument"               "$(echo "$r" | cut -d'|' -f2)" ""

r="$(run "$(mk mobs 1 1)" "")"
check "a mob tree with the flag is accepted"             "${r%%|*}" "0"
check "a mob tree is told peaceful"                      "$(echo "$r" | cut -d'|' -f2)" "--difficulty peaceful"

r="$(run "$(mk mobs 1 1)" "easy")"
check "GAMEPLAY_DIFFICULTY=easy reaches the argv"        "$(echo "$r" | cut -d'|' -f2)" "--difficulty easy"

# THE SHIPPED DEFECT. `mob-in-world`, `mob-combat` and `mobs-in-world-2` all
# sat here: a difficulty on the subject, no way for the harness to set it, and
# vanilla booted at peaceful regardless.
r="$(run "$(mk stranded 1 0)" "")"
check "has a difficulty and no flag REFUSES"             "${r%%|*}" "4"

r="$(run "$(mk stranded 1 0)" "easy")"
check "…refuses at easy too, not only at peaceful"       "${r%%|*}" "4"

# A flag with no setter is harmless — main.rs ignores an unknown token — and
# must not refuse, or a tree mid-port stops the queue.
r="$(run "$(mk flagonly 0 1)" "")"
check "a flag with no setter is accepted"                "${r%%|*}" "0"

r="$(run "$(mk mobs 1 1)" "PEACEFUL")"
check "a difficulty name vanilla would not parse REFUSES" "${r%%|*}" "4"
r="$(run "$(mk mobs 1 1)" "creative")"
check "a gamemode where a difficulty belongs REFUSES"     "${r%%|*}" "4"

echo "== the symmetry, and each injection that breaks it"

# `sym <file>` — the two sides of one script, as text. A launch line that does
# not carry DIFFICULTY_ARGS, or a props block naming a literal difficulty, is
# the shipped defect.
# Comments are excluded, and that is not a loophole: both files argue at length
# about `difficulty=peaceful` in prose, and a check that read prose would be
# unfixable rather than strict.
sym() {
    local f="$1" bad=0 code
    code="$(grep -vE '^[[:space:]]*#' "$f")"
    grep -qE '^difficulty=\$GAMEPLAY_DIFFICULTY$' <<<"$code" || bad=1
    grep -qE '^[[:space:]]*difficulty=(peaceful|easy|normal|hard)[[:space:]]*$' <<<"$code" && bad=1
    grep -q 'DIFFICULTY_ARGS\[@\]' <<<"$code" || bad=1
    return "$bad"
}

for f in ci/t2-gameplay.sh ci/t2-gameplay-subset.sh; do
    if sym "$f"; then ok "$f: both sides read GAMEPLAY_DIFFICULTY"
    else bad "$f: the two sides are not the same variable"; fi

    # injection 1 — re-hardcode the vanilla side
    cp "$f" "$T/inj1.sh"
    sed -i 's/^difficulty=\$GAMEPLAY_DIFFICULTY$/difficulty=peaceful/' "$T/inj1.sh"
    if sym "$T/inj1.sh"; then bad "$f: a hardcoded vanilla difficulty is NOT caught"
    else ok "$f: injection 1 (vanilla hardcoded back to peaceful) is caught"; fi

    # injection 2 — drop the subject side off the launch line
    cp "$f" "$T/inj2.sh"
    sed -i 's/ \${DIFFICULTY_ARGS\[@\]+"\${DIFFICULTY_ARGS\[@\]}"}//' "$T/inj2.sh"
    if sym "$T/inj2.sh"; then bad "$f: a launch line with no difficulty is NOT caught"
    else ok "$f: injection 2 (subject told nothing) is caught"; fi
done

# The helper must be sourced by both, or the variable is unset and every
# `difficulty=$GAMEPLAY_DIFFICULTY` expands to the empty string — which
# `server.properties` accepts and silently defaults.
for f in ci/t2-gameplay.sh ci/t2-gameplay-subset.sh; do
    if grep -q 'difficulty_or_die "\$ROOT"' "$f"; then ok "$f calls difficulty_or_die"
    else bad "$f does not call difficulty_or_die — GAMEPLAY_DIFFICULTY would be empty"; fi
done

echo
echo "gameplay-difficulty-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
