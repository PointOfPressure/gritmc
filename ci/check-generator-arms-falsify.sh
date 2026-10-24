#!/usr/bin/env bash
# check-generator-arms-falsify.sh — show ci/check-generator-arms.sh going red.
#
#     ./ci/check-generator-arms-falsify.sh    # ~1 s, no build, no jar, no box
#
# # WHY THIS EXISTS
#
# `ci/check-generator-arms.sh` shipped on 2026-08-2x with no falsifier at all,
# and it is a guard whose whole content is "these two derived sets agree". Two
# things it can be wrong about are opposite and both quiet: it can miss an arm
# nobody wired (the case it was written for, which it caught on
# `gate-instruments` on 2026-09-05 — `tags_block.json`), and it can go on
# exempting an entry in `DORMANT_GENERATORS` long after something started
# reading that table, at which point the exemption is standing ready to hide
# the next arm somebody forgets.
#
# The fixture is a temp `$ROOT` holding a copy of the two scripts and one
# synthetic crate, because the real repository has exactly one dormant arm and
# no unreached one — so every case below would be unreachable against it, which
# is the shape this whole directory refuses.
#
# Exit 0 all cases behaved, 1 a case did not, 2 the harness could not run.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHK="$ROOT/ci/check-generator-arms.sh"
GEN="$ROOT/ci/gen-gameplay-tables.sh"
[ -x "$CHK" ] && [ -x "$GEN" ] || { echo "check-generator-arms-falsify: a script is missing" >&2; exit 2; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$1"; printf '        %s\n' "$2"; }

# Every basename `generator_for` has an arm for, read the same way the checker
# reads it — out of the PARSED function, so a comment holding `foo.json)`
# cannot be miscounted. Deriving it here rather than listing it is the whole
# reason the cases below stay true when somebody adds a generator.
mapfile -t ALL_ARMS < <(bash -c 'source <(sed -n "/^generator_for()/,/^}/p" "$0"); declare -f generator_for' "$GEN" \
                        | grep -oE '^[[:space:]]+[A-Za-z0-9_]+\.(json|bin)\)' | tr -d ' )' | sort -u)
[ "${#ALL_ARMS[@]}" -gt 2 ] || { echo "check-generator-arms-falsify: read ${#ALL_ARMS[@]} arms" >&2; exit 2; }

# build_root <dormant entries> <basenames the crate must NOT name>
#
# The synthetic crate names EVERY arm's table except the omitted ones, because
# the guard compares two whole sets: a fixture naming two tables would report
# the other twenty-seven as unreached and every case would pass for the wrong
# reason. That is how the first draft of this file "passed" four arms.
build_root() {
    local R="$TMP/root"; rm -rf "$R"
    mkdir -p "$R/ci" "$R/crates/fake/src" "$R/tools" "$R/plugins"
    cp "$CHK" "$GEN" "$ROOT/ci/provenance.sh" "$R/ci/"
    chmod +x "$R/ci/check-generator-arms.sh" "$R/ci/gen-gameplay-tables.sh"
    # The dormant list, rewritten wholesale. `sed` over the real file rather
    # than a hand-written copy, so a case cannot pass against a checker the
    # tree does not have.
    python3 - "$R/ci/check-generator-arms.sh" "$1" <<'EOP'
import sys
# LINE-BASED, from the opening line to the first line that is exactly `)`.
# A regex on `\(([^)]*)\)` was tried and was wrong: the comments inside that
# array carry their own parentheses — `state.is(BlockTags.CAMPFIRES ...)` and
# `(ACCEPT_TELEPORTATION, decoded into a field read by nothing)` — so it cut at
# the first of those and left a file that still declared the old entry. The
# case that caught it was "an undeclared unreached arm is refused", which came
# back GREEN saying "1 declared dormant": the fixture builder had silently
# failed to build the fixture.
p, entries = sys.argv[1], sys.argv[2].split()
lines = open(p).read().split("\n")
start = next(i for i, l in enumerate(lines) if l.startswith("DORMANT_GENERATORS=("))
end = next(i for i in range(start, len(lines)) if lines[i].strip() == ")")
body = ["DORMANT_GENERATORS=("] + ["    %s" % e for e in entries] + [")"]
open(p, "w").write("\n".join(lines[:start] + body + lines[end + 1:]))
EOP
    local omit=" $2 "
    { echo 'pub fn tables() {'
      local a
      for a in "${ALL_ARMS[@]}"; do
          case "$omit" in *" $a "*) continue ;; esac
          echo "    let _ = \"data/vanilla/$a\";"
      done
      echo '}'
    } > "$R/crates/fake/src/lib.rs"
    printf '%s\n' "$R"
}

run() { OUT="$(cd "$1" && ./ci/check-generator-arms.sh 2>&1)"; RC=$?; }

echo "check-generator-arms-falsify: $ROOT"
echo

# CONTROL 1 — the real repository. Everything below is meaningless if the
# guard is not green on the tree it actually runs against.
OUT="$(cd "$ROOT" && ./ci/check-generator-arms.sh 2>&1)"; RC=$?
if [ "$RC" -eq 0 ]; then ok "CONTROL: the real tree is green"
else bad "CONTROL: the real tree is green" "rc=$RC: $OUT"; fi

# CONTROL 2 — a fixture whose every referenced table has an arm, one of them
# declared dormant and genuinely unread. This must be green, or the reds below
# would only show a guard that refuses everything.
R="$(build_root "tags_block.json" "tags_block.json")"
run "$R"
if [ "$RC" -eq 0 ]; then ok "CONTROL: a genuinely dormant entry is green"
else bad "CONTROL: a genuinely dormant entry is green" "rc=$RC: $OUT"; fi

# CASE 1 — THE ONE IT WAS WRITTEN FOR, and the one it caught on this branch:
# an arm exists, nothing names the literal, and it is not declared.
R="$(build_root "" "tags_block.json")"
run "$R"
if [ "$RC" -eq 1 ] && printf '%s' "$OUT" | grep -c 'no source literal reaches' >/dev/null; then
    ok "an undeclared unreached arm is refused"
else bad "an undeclared unreached arm is refused" "rc=$RC: $OUT"; fi

# CASE 2 — THE RULE ADDED 2026-09-05: a declared dormant entry that something
# has since started reading. The table is in the derived set, every tree
# generates it, and the comment beside the entry has become false.
R="$(build_root "tags_block.json" "")"
run "$R"
if [ "$RC" -eq 1 ] && printf '%s' "$OUT" | grep -c 'no longer dormant' >/dev/null; then
    ok "a dormant entry that has come true is refused"
else bad "a dormant entry that has come true is refused" "rc=$RC: $OUT"; fi

# CASE 3 — the two reds are DIFFERENT reds. A guard that printed one message
# for both would be indistinguishable from one that only implemented the first.
R="$(build_root "" "tags_block.json")"; run "$R"; A="$OUT"
R="$(build_root "tags_block.json" "")"; run "$R"; B="$OUT"
if [ "$A" != "$B" ] && printf '%s' "$A" | grep -cv 'no longer dormant' >/dev/null; then
    ok "the two refusals say different things"
else bad "the two refusals say different things" "identical output"; fi

# CASE 4 — the derived set coming back empty is an infrastructure refusal (2)
# and not a green over nothing, which is how this guard would fail silently.
R="$(build_root "" "")"
rm -rf "$R/crates" "$R/tools" "$R/plugins"
run "$R"
if [ "$RC" -eq 2 ] || [ "$RC" -eq 1 ]; then
    if [ "$RC" -eq 2 ]; then ok "an empty derived set is a refusal (2), not a green"
    else ok "an empty derived set is refused (1, every arm unreached)"; fi
else bad "an empty derived set is refused" "rc=$RC: $OUT"; fi

# CASE 5 — a dormant entry naming a table with NO arm is not a way to smuggle
# anything in: the list only ever suppresses, it never adds.
R="$(build_root "no_such_table.json" "")"
run "$R"
if [ "$RC" -eq 0 ]; then ok "a dormant entry for a nonexistent arm changes nothing"
else bad "a dormant entry for a nonexistent arm changes nothing" "rc=$RC: $OUT"; fi

printf '\ncheck-generator-arms-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || { echo "CHECK-GENERATOR-ARMS-FALSIFY RED" >&2; exit 1; }
echo "CHECK-GENERATOR-ARMS-FALSIFY GREEN"
