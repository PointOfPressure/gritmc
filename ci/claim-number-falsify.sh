#!/usr/bin/env bash
# ci/claim-number-falsify.sh — prove ci/claim-number.py can refuse.
#
# ===== AGENT INTERFACE =====
#     ci/claim-number-falsify.sh          run every case, exit 0 iff all pass
#     ci/claim-number-falsify.sh -v       show each command's output
# ===== end interface =====
#
# # WHY THIS IS NOT JUST `--selftest`
#
# `ci/claim-number.py --selftest` runs on fixtures, and this project has
# repeatedly found that a check first demonstrated on a fixture has only been
# shown to work on the fixture. `docs/AGENT-BRIEF.md`'s hard rule is that a
# falsifier is shown **failing on the unrepaired state before it is shown
# passing**, and the unrepaired state here is not something that has to be
# constructed: as of 2026-08-18 the live ref store carries four cross-branch
# section-number collisions that nothing in the tree detected — §45, §51, §54
# and §57.
#
# So case 1 poisons the REAL worktree against the REAL ref store, using the
# §54 collision that is live right now, and requires the refusal to name the
# other claimant. Case 7 does the same to `ci/check-doc-claims.py` with a real
# landed claim. Everything in between needs to *write* reservation refs, and
# those run against a scratch repository — see `CLAIM_NUMBER_ROOT` in
# `ci/claim-number.py` for why writing them into the live store would be
# unsafe.
#
# Case 8 is the one that guards the split: it asserts this script created no
# reservation refs in the live store. A falsifier that burns a real section
# number every time it runs would be worse than no falsifier.

set -uo pipefail

VERBOSE=0
[ "${1:-}" = "-v" ] && VERBOSE=1

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLAIM="$ROOT/ci/claim-number.py"
DOCCHECK="$ROOT/ci/check-doc-claims.py"
SCRATCH="$(mktemp -d)"

# THIS SCRIPT POISONS THE REAL WORKTREE, so two of it must not run at once.
# Learned on the first run: a foreground copy killed by `timeout` and a
# background copy overlapped, and between them left `docs/claims/` modified
# with neither run's cleanup having fired for the other's edits. The lock is
# per-worktree because the poisoning is per-worktree.
LOCK="$ROOT/.git/claim-number-falsify.lock"
exec 9>"$LOCK" 2>/dev/null || LOCK=""
if [ -n "$LOCK" ] && ! flock -n 9; then
  echo "claim-number-falsify: another run holds $LOCK — refusing to poison the same tree twice" >&2
  exit 2
fi

PASS=0
FAIL=0
DIRTY_BEFORE="$(git -C "$ROOT" status --porcelain -- docs/claims ci | sort)"
# Same before/after discipline as DIRTY_BEFORE, and for the same reason it was
# needed there: this started life as "assert zero reservation refs exist",
# which held only until the first REAL reservation was taken. §59 was claimed
# with `--claim` an hour later and the case went red for the allocator working
# exactly as intended. What this script can honestly assert is that IT created
# none, not that none exist.
REFS_BEFORE="$(git -C "$ROOT" for-each-ref --format='%(refname)' refs/claims | sort)"

# Every temporary file this script writes into the real worktree is removed
# here, on every exit path including a signal. The two poisoned cases below
# edit `docs/claims/` in place, and leaving one behind would red the gate for
# whoever ran this.
POISON_FILES=()      # untracked files this script creates in the real tree
POISONED_TRACKED=()  # tracked files it edits in place, restored from the index
cleanup() {
  local f
  for f in "${POISON_FILES[@]:-}"; do [ -n "$f" ] && rm -f "$f"; done
  for f in "${POISONED_TRACKED[@]:-}"; do
    [ -n "$f" ] && git -C "$ROOT" checkout -- "$f" 2>/dev/null
  done
  rm -rf "$SCRATCH"
}
trap cleanup EXIT INT TERM

say() { printf '\n--- %s\n' "$*"; }
ok()   { PASS=$((PASS+1)); printf 'FALSIFY ok   [%s]\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf 'FALSIFY FAIL [%s]\n' "$1"; }
check(){ if [ "$2" = 1 ]; then ok "$1"; else bad "$1"; fi; }
show() { [ "$VERBOSE" = 1 ] && sed 's/^/    | /' <<<"$1"; return 0; }

# ---------------------------------------------------------------------------
say "1. the live §54 collision, on the real ref store — UNREPAIRED FIRST"

# THE UNPOISONED TREE IS NOT NECESSARILY GREEN, AND ASSERTING THAT IT IS MADE
# THIS CASE FAIL FOR BEING RIGHT — 2026-08-19. The last assertion below read
# `--check` exits 0 once the poison is removed. That held on the day it was
# written and stopped holding the moment a real cross-branch collision existed
# that nobody had repaired: on `main` at e2fc07600 there are two, DETERMINISM
# §46 and PARITY-ROADMAP §57, neither of them anything to do with this script.
# So the case went red while `ci/claim-number.py` was working exactly as
# designed, which is the failure mode where a falsifier trains people to
# ignore it.
#
# The fix is the rule this repo already applies to every residual: when the
# floor is not zero and is not yours, assert the DELTA, not the level. The
# collision set is snapshotted here, before the poison exists, and the
# assertion after removal is that the set is back to exactly this — which
# still goes red if removing the poison fails to remove its collision, and no
# longer goes red because somebody else's branch collides with main.
collisions() { (cd "$ROOT" && python3 "$CLAIM" --check 2>&1) \
    | grep -oE 'CROSS-BRANCH-COLLISION [A-Z-]+ §[0-9.]+' | sort; }
COLLISIONS_BEFORE="$(collisions)"
say "   pre-existing collisions in this tree, not this script's doing:"
[ -n "$COLLISIONS_BEFORE" ] && sed 's/^/     /' <<<"$COLLISIONS_BEFORE"

# `gameplay-corpus-gaps` and `corpus-coverage-gaps` both hold §54 today under
# different slugs. Neither branch can see the other. Dropping one branch's
# claim file into a tree that carries the other reproduces, exactly, the
# situation the brief describes as "a recovered draft claimed §54, which a
# live lane already holds" — and which nothing detected at the time.
POISON="$ROOT/docs/claims/PARITY-ROADMAP-54-what-the-corpus-never-asks.toml"
POISON_FILES+=("$POISON")
cat >"$POISON" <<'TOML'
doc    = "PARITY-ROADMAP"
number = 54
slug   = "what-the-corpus-never-asks"
title  = "poisoned by ci/claim-number-falsify.sh"
branch = "corpus-coverage-gaps"
status = "reserved"
TOML

OUT="$(cd "$ROOT" && python3 "$CLAIM" --check 2>&1)"; RC=$?
show "$OUT"
check "an unrepaired §54 is REFUSED against the live ref store" \
      "$([ $RC -ne 0 ] && echo 1 || echo 0)"
check "the refusal names §54 specifically" \
      "$(grep -qE 'CROSS-BRANCH-COLLISION PARITY-ROADMAP §54' <<<"$OUT" && echo 1 || echo 0)"
check "and names the branch that already holds it" \
      "$(grep -q 'note-block-is-three-defects' <<<"$OUT" && echo 1 || echo 0)"

rm -f "$POISON"
OUT="$(cd "$ROOT" && python3 "$CLAIM" --check 2>&1)"; RC=$?
show "$OUT"
check "the same tree with the poison removed no longer collides on §54" \
      "$(grep -qE 'CROSS-BRANCH-COLLISION PARITY-ROADMAP §54' <<<"$OUT" && echo 0 || echo 1)"
check "and the collision set is back to exactly what it was before the poison" \
      "$([ "$(collisions)" = "$COLLISIONS_BEFORE" ] && echo 1 || echo 0)"

# ---------------------------------------------------------------------------
say "2. the compare-and-swap: a second claimant must lose"

# A scratch repository, because these cases write reservation refs.
git init -q "$SCRATCH/repo"
mkdir -p "$SCRATCH/repo/ci" "$SCRATCH/repo/docs/claims"
cp "$CLAIM" "$SCRATCH/repo/ci/claim-number.py"
export CLAIM_NUMBER_ROOT="$SCRATCH/repo"
SC="python3 $SCRATCH/repo/ci/claim-number.py"

A="$($SC --claim -d PARITY-ROADMAP -s alpha -t A -b lane-a 2>/dev/null)"
B="$($SC --claim -d PARITY-ROADMAP -s beta  -t B -b lane-b 2>/dev/null)"
check "two claims on an empty registry get two different numbers" \
      "$([ -n "$A" ] && [ -n "$B" ] && [ "$A" != "$B" ] && echo 1 || echo 0)"
check "and they are consecutive from 1 (1, then 2)" \
      "$([ "$A" = 1 ] && [ "$B" = 2 ] && echo 1 || echo 0)"
check "the loser did not overwrite the winner's reservation" \
      "$(git -C "$SCRATCH/repo" cat-file -p refs/claims/PARITY-ROADMAP/1 2>/dev/null \
         | grep -c 'lane-a' >/dev/null && echo 1 || echo 0)"

# The race, run for real rather than argued: N claimants started together,
# each of which computes the same `next_free` before any of them has written.
# Without the CAS they would all take the same number.
for i in 1 2 3 4 5 6; do $SC --claim -d DETERMINISM -s "race$i" -t R -b "lane$i" \
    >"$SCRATCH/race.$i" 2>/dev/null & done
wait
RACE="$(cat "$SCRATCH"/race.* | sort -n | tr '\n' ' ')"
UNIQ="$(cat "$SCRATCH"/race.* | sort -u | wc -l)"
show "six concurrent claimants got: $RACE"
check "six concurrent claimants got six DISTINCT numbers" \
      "$([ "$UNIQ" = 6 ] && echo 1 || echo 0)"
check "and no number was skipped (1..6 exactly)" \
      "$([ "$RACE" = "1 2 3 4 5 6 " ] && echo 1 || echo 0)"

# ---------------------------------------------------------------------------
say "3. a reservation blocks the number, and --release gives it back"

N_BEFORE="$($SC --next -d PARITY-ROADMAP)"
check "--next skips the two numbers already reserved" \
      "$([ "$N_BEFORE" = 3 ] && echo 1 || echo 0)"
$SC --release -d PARITY-ROADMAP -n 2 >/dev/null 2>&1
rm -f "$SCRATCH/repo/docs/claims/PARITY-ROADMAP-2-beta.toml"
check "a released number is free again" \
      "$([ "$($SC --next -d PARITY-ROADMAP)" = 2 ] && echo 1 || echo 0)"
OUT="$($SC --release -d PARITY-ROADMAP -n 999 2>&1)"; RC=$?
check "releasing a reservation that does not exist is refused, not silent" \
      "$([ $RC -ne 0 ] && echo 1 || echo 0)"

# ---------------------------------------------------------------------------
say "4. a number reserved by ANOTHER branch is refused to this one"

$SC --claim -d PARITY-ROADMAP -s held -t H -b some-other-lane >/dev/null 2>&1
# Now rewrite the claim file as if this tree had invented the same number for
# a different purpose — the exact §45/§51/§57 shape, but caught before the
# merge instead of at it.
cat >"$SCRATCH/repo/docs/claims/PARITY-ROADMAP-2-mine.toml" <<'TOML'
doc    = "PARITY-ROADMAP"
number = 2
slug   = "mine"
title  = "invented locally"
branch = "this-lane"
TOML
rm -f "$SCRATCH/repo/docs/claims/PARITY-ROADMAP-2-held.toml"
OUT="$(cd "$SCRATCH/repo" && $SC --check 2>&1)"; RC=$?
show "$OUT"
check "a number reserved by another branch is REFUSED here" \
      "$([ $RC -ne 0 ] && echo 1 || echo 0)"
check "and the refusal names the branch holding the reservation" \
      "$(grep -q 'RESERVED-ELSEWHERE.*some-other-lane' <<<"$OUT" && echo 1 || echo 0)"
unset CLAIM_NUMBER_ROOT

# ---------------------------------------------------------------------------
say "5. the allocator's own selftest"

OUT="$(python3 "$CLAIM" --selftest 2>&1)"; RC=$?
show "$OUT"
check "--selftest passes" "$([ $RC -eq 0 ] && echo 1 || echo 0)"

# ---------------------------------------------------------------------------
say "6. --show names every live collision, and none of the landed rows"

OUT="$(cd "$ROOT" && python3 "$CLAIM" --show -d PARITY-ROADMAP 2>&1)"
show "$OUT"
NCOLL="$(grep -c 'COLLISION' <<<"$OUT")"
check "--show reports at least one live collision (found $NCOLL)" \
      "$([ "$NCOLL" -ge 1 ] && echo 1 || echo 0)"
check "--show does not print the 130 branches that merely carry a landed claim" \
      "$(grep -q 'worldgen-sulfur-r18' <<<"$OUT" && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
say "7. RESERVED-BUT-WRITTEN, poisoned on a REAL landed claim"

# §52 is landed: the claim file and the `## 52.` heading both exist on main.
# Mislabelling it `reserved` is the failure mode that would let a reservation
# sit forever over a section that was written months ago.
REAL="$ROOT/docs/claims/PARITY-ROADMAP-52-open-row-audit.toml"
if [ -f "$REAL" ]; then
  POISONED_TRACKED+=("docs/claims/PARITY-ROADMAP-52-open-row-audit.toml")
  printf 'status = "reserved"\n' >>"$REAL"
  OUT="$(cd "$ROOT" && python3 "$DOCCHECK" 2>&1)"; RC=$?
  show "$OUT"
  check 'a landed section mislabelled reserved is REFUSED' \
        "$([ $RC -ne 0 ] && echo 1 || echo 0)"
  check "the red is RESERVED-BUT-WRITTEN and names §52" \
        "$(grep -qE 'RESERVED-BUT-WRITTEN PARITY-ROADMAP §52' <<<"$OUT" && echo 1 || echo 0)"
  git -C "$ROOT" checkout -- docs/claims/PARITY-ROADMAP-52-open-row-audit.toml
  OUT="$(cd "$ROOT" && python3 "$DOCCHECK" 2>&1)"; RC=$?
  check "restored, check-doc-claims is GREEN again" \
        "$([ $RC -eq 0 ] && echo 1 || echo 0)"
else
  bad "fixture §52 claim file is missing — case 7 could not run"
fi

# ---------------------------------------------------------------------------
say "8. this script wrote nothing to the live ref store"

# Run cleanup now rather than trusting the EXIT trap, because case 8 is the
# case that judges whether cleanup worked. A check that runs before the thing
# it checks is the unfalsifiable shape this repo keeps finding.
cleanup
REFS_AFTER="$(git -C "$ROOT" for-each-ref --format='%(refname)' refs/claims | sort)"
APPEARED="$(comm -13 <(printf '%s\n' "$REFS_BEFORE") <(printf '%s\n' "$REFS_AFTER") | grep -c . || true)"

# A REF THAT APPEARED IS NOT NECESSARILY A REF THIS SCRIPT LEFT — 2026-08-19.
# `refs/claims/` is the box-wide allocator's store, shared by every lane on
# this repository, and the whole point of it is that other lanes are taking
# numbers while you work. Measured during this very run: two reservations,
# PARITY-ROADMAP §104 `parallel-serial-stages` and §105 `extractability`,
# appeared inside the window and were reported as this script's leak. Both
# were another lane doing exactly what the allocator is for.
#
# So the before/after set difference answers the wrong question. What this
# script can honestly assert is that no ref it could have written is present,
# and it knows precisely which those are: every reservation it takes goes into
# `$SCRATCH/repo`, so in the LIVE store the answer is none at all, and the
# way to tell is the blob, not the refname. Anything else that appeared is
# reported — visibly, because a concurrent allocation is worth seeing — and
# is not a failure.
LEAKED=0
LEAKED_LIST=""
CONCURRENT=""
while IFS= read -r ref; do
    [ -n "$ref" ] || continue
    body="$(git -C "$ROOT" cat-file -p "$ref" 2>/dev/null || true)"
    if grep -qE 'claim-number-falsify|what-the-corpus-never-asks' <<<"$body"; then
        LEAKED=$((LEAKED+1)); LEAKED_LIST="$LEAKED_LIST$ref"$'\n'
    else
        CONCURRENT="$CONCURRENT$ref ($(grep -oE 'slug *= *"[^"]*"' <<<"$body" | head -1))"$'\n'
    fi
done < <(comm -13 <(printf '%s\n' "$REFS_BEFORE") <(printf '%s\n' "$REFS_AFTER"))

check "this script created no reservation refs in the live store (leaked $LEAKED)" \
      "$([ "$LEAKED" = 0 ] && echo 1 || echo 0)"
[ -n "$LEAKED_LIST" ] && printf '%s' "$LEAKED_LIST" | sed 's/^/    leaked: /'
if [ -n "$CONCURRENT" ]; then
  echo "     ($APPEARED ref(s) appeared while this ran, none of them this script's:"
  printf '%s' "$CONCURRENT" | sed 's/^/       /'
  echo "      that is other lanes allocating, which is what the store is for.)"
fi
# Compared against the state this script STARTED in, not against zero: an
# uncommitted edit that was already there is not something this script did,
# and asserting a pristine tree would make the falsifier unrunnable during
# the very work that changes these files.
DIRTY_AFTER="$(git -C "$ROOT" status --porcelain -- docs/claims ci | sort)"
check "the real worktree is left exactly as this script found it" \
      "$([ "$DIRTY_AFTER" = "$DIRTY_BEFORE" ] && echo 1 || echo 0)"
if [ "$DIRTY_AFTER" != "$DIRTY_BEFORE" ]; then
  printf '    before:\n%s\n    after:\n%s\n' "$DIRTY_BEFORE" "$DIRTY_AFTER"
fi

printf '\n=== claim-number falsifier: %d passed, %d failed ===\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ] || exit 1
