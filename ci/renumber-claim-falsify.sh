#!/usr/bin/env bash
# ci/renumber-claim-falsify.sh — prove ci/renumber-claim.sh moves what it must
# and leaves alone what it must not.
#
# ===== AGENT INTERFACE =====
#     ci/renumber-claim-falsify.sh        run every case, exit 0 iff all pass
#     ci/renumber-claim-falsify.sh -v     show each case's output
# ===== end interface =====
#
# # THE TWO CASES THIS EXISTS FOR, BOTH FROM REAL DAMAGE OR NEAR-DAMAGE
#
# **CASE 3 — THE SIBLING DOCUMENT.** An unqualified `§7` means one thing in
# `docs/PARITY-ROADMAP.md` and a different thing in `docs/DETERMINISM.md`. The
# first version of `renumber-claim.sh` did not take a `<doc>` argument and
# rewrote `§N` across every tracked file. It performed three real renumbers
# that way and got away with all three ONLY because no sibling document
# happened to carry the contended number — checked afterwards, and no
# DETERMINISM.md was touched. That is luck, and luck is not a property you can
# rely on twice. This case makes it a property: the fixture puts `## 7.` in
# BOTH documents and requires the sibling to come out byte-identical.
#
# **CASE 4 — THE CITATION LEFT BEHIND.** `ci/t0.sh` caught this script citing
# `PARITY-ROADMAP §45` in its own comments — a section this script had itself
# just renumbered to §60 — and DANGLING-SECTION-REF went 8 to 9. That was the
# system working exactly as designed, and an incident nobody wrote down is an
# incident that recurs. So it is a case: renumber, leave a citation behind by
# hand, and require `check-doc-claims` to refuse.
#
# # WHY IT RUNS THE REAL SCRIPT AND NOT A MODEL OF IT
#
# The failure being guarded against is a *side effect* — a file that should
# not have changed changing. That cannot be asserted about a reimplementation;
# it has to be asserted about the thing that will actually run. So the script
# gets `RENUMBER_REPO` / `RENUMBER_WT_BASE` and this builds a throwaway
# repository for it. Nothing here touches the live tree, which case 6 checks.

set -uo pipefail

VERBOSE=0
[ "${1:-}" = "-v" ] && VERBOSE=1

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH="$(mktemp -d)"
PASS=0; FAIL=0
REFS_BEFORE="$(git -C "$ROOT" for-each-ref --format='%(refname)' refs/claims | sort)"
DIRTY_BEFORE="$(git -C "$ROOT" status --porcelain | sort)"

cleanup() { rm -rf "$SCRATCH"; }
trap cleanup EXIT INT TERM

say()  { printf '\n--- %s\n' "$*"; }
ok()   { PASS=$((PASS+1)); printf 'FALSIFY ok   [%s]\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf 'FALSIFY FAIL [%s]\n' "$1"; }
check(){ if [ "$2" = 1 ]; then ok "$1"; else bad "$1"; fi; }
show() { [ "$VERBOSE" = 1 ] && sed 's/^/    | /' <<<"$1"; return 0; }

# --------------------------------------------------------------------------
# A fixture repository with §7 live in BOTH registered documents.
#
# A FUNCTION, not a one-off, because case 4 deliberately vandalises its
# fixture — it has to breach the DANGLING-SECTION-REF ratchet to prove the
# refusal — and a later case running against that wreckage fails for reasons
# that have nothing to do with what it is testing. Each case that mutates the
# tree gets its own.
make_fixture() {
  local FIX="$1"
  mkdir -p "$FIX/docs/claims" "$FIX/ci"
  git init -q "$FIX"
  git -C "$FIX" config user.email t@t; git -C "$FIX" config user.name t
  cp "$ROOT/ci/check-doc-claims.py" "$FIX/ci/check-doc-claims.py"

cat >"$FIX/docs/PARITY-ROADMAP.md" <<'MD'
## 7. The roadmap's seven

Body, citing itself: see `docs/PARITY-ROADMAP.md` §7 and §7.1 below.

### 7.1 A subsection

More body.
MD

cat >"$FIX/docs/DETERMINISM.md" <<'MD'
## 7. Determinism's seven, which is a different section entirely

Body, citing itself: see `docs/DETERMINISM.md` §7. This number must not move.
MD

cat >"$FIX/docs/claims/PARITY-ROADMAP-7-roadmap-seven.toml" <<'TOML'
doc    = "PARITY-ROADMAP"
number = 7
slug   = "roadmap-seven"
title  = "The roadmap's seven"
branch = "fixture"
TOML

cat >"$FIX/docs/claims/DETERMINISM-7-determinism-seven.toml" <<'TOML'
doc    = "DETERMINISM"
number = 7
slug   = "determinism-seven"
title  = "Determinism's seven"
branch = "fixture"
TOML

  git -C "$FIX" add -A >/dev/null
  git -C "$FIX" commit -q -m fixture
  git -C "$FIX" branch -M fixture
}

FIX="$SCRATCH/repo"
make_fixture "$FIX"
SIB_BEFORE="$(sha1sum <"$FIX/docs/DETERMINISM.md")"

export RENUMBER_REPO="$FIX"
export RENUMBER_WT_BASE="$SCRATCH/wt"

# --------------------------------------------------------------------------
say "1. a bad <doc> is refused before anything is touched"
OUT="$("$ROOT/ci/renumber-claim.sh" WORKLIST fixture roadmap-seven 7 9 "r" 2>&1)"; RC=$?
show "$OUT"
check "an unregistered doc name is refused" "$([ $RC -ne 0 ] && echo 1 || echo 0)"
check "and the refusal names what it will accept" \
      "$(grep -q 'DETERMINISM or PARITY-ROADMAP' <<<"$OUT" && echo 1 || echo 0)"

say "2. the renumber moves the claim, the heading and the subsection"
OUT="$("$ROOT/ci/renumber-claim.sh" PARITY-ROADMAP fixture roadmap-seven 7 9 "falsifier" 2>&1)"; RC=$?
show "$OUT"
WT="$SCRATCH/wt/fixture"
check "the renumber succeeds" "$([ $RC -eq 0 ] && echo 1 || echo 0)"
check "the claim file is renamed" \
      "$([ -f "$WT/docs/claims/PARITY-ROADMAP-9-roadmap-seven.toml" ] && echo 1 || echo 0)"
check "the old claim file is gone" \
      "$([ ! -f "$WT/docs/claims/PARITY-ROADMAP-7-roadmap-seven.toml" ] && echo 1 || echo 0)"
check "the claim's own \`number\` field moved with its name" \
      "$(grep -qE '^number[[:space:]]*=[[:space:]]*9$' "$WT/docs/claims/PARITY-ROADMAP-9-roadmap-seven.toml" && echo 1 || echo 0)"
check "the \`## N.\` heading moved" \
      "$(grep -q '^## 9\. The roadmap' "$WT/docs/PARITY-ROADMAP.md" && echo 1 || echo 0)"
check "the \`### N.M\` subsection moved with it" \
      "$(grep -q '^### 9\.1 ' "$WT/docs/PARITY-ROADMAP.md" && echo 1 || echo 0)"
check "the citation in that document moved" \
      "$(grep -qF 'PARITY-ROADMAP.md` §9' "$WT/docs/PARITY-ROADMAP.md" && echo 1 || echo 0)"
check "and no \`§7\` survives in it" \
      "$(grep -qE '§7(\b|\.)' "$WT/docs/PARITY-ROADMAP.md" && echo 0 || echo 1)"

say "3. THE SIBLING DOCUMENT IS UNTOUCHED — byte for byte"
SIB_AFTER="$(sha1sum <"$WT/docs/DETERMINISM.md")"
show "before=$SIB_BEFORE after=$SIB_AFTER"
check "docs/DETERMINISM.md is byte-identical after a PARITY-ROADMAP renumber" \
      "$([ "$SIB_BEFORE" = "$SIB_AFTER" ] && echo 1 || echo 0)"
check "its own \`## 7.\` heading still exists" \
      "$(grep -q '^## 7\. Determinism' "$WT/docs/DETERMINISM.md" && echo 1 || echo 0)"
check "its own \`§7\` citation still says 7" \
      "$(grep -qF 'DETERMINISM.md` §7.' "$WT/docs/DETERMINISM.md" && echo 1 || echo 0)"
check "the sibling's claim file was not renamed" \
      "$([ -f "$WT/docs/claims/DETERMINISM-7-determinism-seven.toml" ] && echo 1 || echo 0)"

say "4. A CITATION LEFT BEHIND BY A RENUMBER IS REFUSED"
OUT="$(cd "$WT" && python3 ci/check-doc-claims.py 2>&1)"; RC=$?
show "$OUT"
check "the renumbered tree is green to begin with" "$([ $RC -eq 0 ] && echo 1 || echo 0)"
N_BASE="$(grep -oE 'section refs: [0-9]+ dangling' <<<"$OUT" | grep -oE '[0-9]+' | head -1)"
N_BASE="${N_BASE:-0}"
# The exact shape ci/t0.sh caught on 2026-08-18: prose still naming the number
# the section used to have.
printf '\nA leftover: `docs/PARITY-ROADMAP.md` §7 still cited here.\n' >>"$WT/docs/PARITY-ROADMAP.md"
OUT="$(cd "$WT" && python3 ci/check-doc-claims.py 2>&1)"; RC=$?
show "$OUT"
# Asserted as a DELTA and by name, not as a literal count: the checker copied
# into the fixture carries two example citations in its own comments, so the
# baseline is 2 and hardcoding "1 dangling" made this case fail for a reason
# that had nothing to do with renumbering.
N_AFTER="$(grep -oE 'section refs: [0-9]+ dangling' <<<"$OUT" | grep -oE '[0-9]+' | head -1)"
check "the leftover is DETECTED — the dangling count rose by exactly one" \
      "$([ "${N_AFTER:-0}" = "$((N_BASE + 1))" ] && echo 1 || echo 0)"
check "and the green line names the orphan it found" \
      "$(grep -qE 'section refs:.*docs/PARITY-ROADMAP\.md:[0-9]+ -> PARITY-ROADMAP §7' <<<"$OUT" && echo 1 || echo 0)"

# DANGLING-SECTION-REF is a RATCHET, not an absolute — `MAX_DANGLING_SECTION_REFS`
# is 8, so one orphan is reported and tolerated. That is deliberate: forward
# references to sections that have not merged yet are normal here. What must
# not be tolerated is the count GROWING, which is exactly how ci/t0.sh caught
# this script on 2026-08-18 — it went 8 to 9 and refused. So the case has to
# push past the ceiling to assert the refusal, and asserting the detection
# separately above is what keeps the two properties distinguishable.
for i in 1 2 3 4 5 6 7 8; do
  printf 'And another leftover: `docs/PARITY-ROADMAP.md` §7 here too, %d.\n' "$i" >>"$WT/docs/PARITY-ROADMAP.md"
done
OUT="$(cd "$WT" && python3 ci/check-doc-claims.py 2>&1)"; RC=$?
show "$OUT"
check "leftovers past the ratchet's ceiling are REFUSED" \
      "$([ $RC -ne 0 ] && echo 1 || echo 0)"
check "and the red is DANGLING-SECTION-REF naming §7" \
      "$(grep -qE 'DANGLING-SECTION-REF.*PARITY-ROADMAP §7' <<<"$OUT" && echo 1 || echo 0)"

say "5. the move is one commit, on top, rewriting nothing"
N_NEW="$(git -C "$WT" rev-list --count fixture)"
check "exactly one commit was added" "$([ "$N_NEW" = 2 ] && echo 1 || echo 0)"
check "the fixture commit is still an ancestor — no rewrite" \
      "$(git -C "$WT" merge-base --is-ancestor "$(git -C "$FIX" rev-list --max-parents=0 fixture)" fixture && echo 1 || echo 0)"
check "the commit records WHICH clause fired, not just the rule" \
      "$(git -C "$WT" log -1 --format=%B fixture | grep -c 'WHY THIS SIDE AND NOT THE OTHER' >/dev/null && echo 1 || echo 0)"

say "6. A CO-CHECKOUT OF THE SAME BRANCH IS RE-SYNCED, NOT LEFT STAGED TO REVERT"

# The 2026-08-18 damage, as a case. `git worktree add --force` gives a second
# checkout of a branch already checked out; committing moves the ref, and the
# other worktree keeps the OLD tree in its index against the NEW HEAD — so its
# status shows the renumber staged as a revert, and a lane committing from
# there undoes it silently. That is what happened to `~/grit/work-end`.
FIX2="$SCRATCH/repo2"
make_fixture "$FIX2"
export RENUMBER_REPO="$FIX2"
export RENUMBER_WT_BASE="$SCRATCH/wt2"
CO="$SCRATCH/co"
git -C "$FIX2" worktree add -q --force "$CO" fixture
CO_HEAD_BEFORE="$(git -C "$CO" rev-parse HEAD)"
OUT="$("$ROOT/ci/renumber-claim.sh" PARITY-ROADMAP fixture roadmap-seven 7 11 "falsifier co-checkout" 2>&1)"; RC=$?
show "$OUT"
check "the second renumber succeeds with a co-checkout present" \
      "$([ $RC -eq 0 ] && echo 1 || echo 0)"
check "the clean co-checkout is re-synced to the new commit" \
      "$([ "$(git -C "$CO" rev-parse HEAD)" != "$CO_HEAD_BEFORE" ] && echo 1 || echo 0)"
check "and it is CLEAN — not showing the renumber as a staged revert" \
      "$([ -z "$(git -C "$CO" status --porcelain)" ] && echo 1 || echo 0)"
check "its files are at the new number" \
      "$([ -f "$CO/docs/claims/PARITY-ROADMAP-11-roadmap-seven.toml" ] && echo 1 || echo 0)"

# And the other half: a co-checkout with work of its own is NOT touched.
git -C "$FIX2" worktree add -q --force "$SCRATCH/co2" fixture
echo "a lane's uncommitted work" >>"$SCRATCH/co2/docs/PARITY-ROADMAP.md"
OUT="$("$ROOT/ci/renumber-claim.sh" PARITY-ROADMAP fixture roadmap-seven 11 13 "falsifier dirty co-checkout" 2>&1)"; RC=$?
show "$OUT"
check "a co-checkout with uncommitted work is LEFT ALONE" \
      "$(grep -q "a lane's uncommitted work" "$SCRATCH/co2/docs/PARITY-ROADMAP.md" && echo 1 || echo 0)"
check "and the operator is told, by path" \
      "$(grep -qF "$SCRATCH/co2" <<<"$OUT" && echo 1 || echo 0)"

say "7. nothing in the live tree moved"
REFS_AFTER="$(git -C "$ROOT" for-each-ref --format='%(refname)' refs/claims | sort)"
DIRTY_AFTER="$(git -C "$ROOT" status --porcelain | sort)"
check "no reservation refs were created in the live store" \
      "$([ "$REFS_BEFORE" = "$REFS_AFTER" ] && echo 1 || echo 0)"
check "the live worktree is exactly as this script found it" \
      "$([ "$DIRTY_BEFORE" = "$DIRTY_AFTER" ] && echo 1 || echo 0)"

printf '\n=== renumber-claim falsifier: %d passed, %d failed ===\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ] || exit 1
