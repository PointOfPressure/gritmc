#!/usr/bin/env bash
# merge-appendonly-falsify.sh — prove BOTH custom merge drivers by using them.
#
#     ./ci/merge-appendonly-falsify.sh
#
# Everything happens in throwaway `git init` repos under `mktemp -d`; nothing
# here touches this repo's history or its real docs. The drivers under test are
# the real `ci/merge-appendonly.py` and `ci/merge-docregions.py` in THIS
# checkout, invoked by absolute path — a copy would prove nothing about what
# `ci/t0.sh` actually installs. Every case is a REAL three-way `git merge`, not
# a direct call to the driver, so what is proved is what git will actually do.
#
# PART 1 — `ci/merge-appendonly.py`, on `docs/DETERMINISM.md`'s shape:
#
#   1. THE POSITIVE — two branches that each purely append. Merges clean,
#      both tails survive in order, no conflict markers. This is the case
#      `.gitattributes` exists to make cheap.
#   2. ONE BRANCH EDITS EXISTING TEXT WHILE THE OTHER APPENDS — merges, and
#      both survive. This case asserted the opposite until 2026-08-17 and the
#      driver refused; see the note above the case for what that cost and why
#      it moved.
#   2b. THE NEGATIVE — both branches rewrite the SAME existing line. The
#      driver must refuse rather than guess: non-zero exit, standard conflict
#      markers, and the two versions left visible to a human. A driver that
#      cannot fail here is not proof of anything, it is a hope. It also
#      asserts that the refusal is a HUNK and not the whole file, which is
#      the 2026-08-17 regression guard.
#   2c. The shape that actually blocked the branches: a body correction on one
#      side, an append on both.
#   2d. The discriminator — with `merge.appendonly.driver` unset, case 1's
#      shape conflicts. Without this, every case in PART 1 would also pass
#      under git's built-in driver and none of them would be measuring the
#      thing they name.
#
# PART 2 — `ci/merge-docregions.py`, on `docs/WORKLIST.md`'s,
# `docs/PARITY-ROADMAP.md`'s, `ci/figures.toml`'s,
# `ci/checks/gameplay-falsifiable.toml`'s, `ci/checks/gameplay-baseline.toml`'s
# and `ci/t0.sh`'s shapes. The positives are the throughput; **the refusals are
# the point**, because a driver that silently kept both sides of a rewritten
# section would be strictly worse than the conflict it replaced — the conflict
# is visible and the stale figure is not. Cases 3-8, 15, 20, 23-24, 26, 27b are
# positives; 9-14, 16, 21-22, 25, 27, 28 are refusals, and every refusal
# asserts a non-zero exit AND standard conflict markers in the tree.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL=0

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

git -C "$TMP" init -q -b main
git -C "$TMP" config user.email "falsify@example.invalid"
git -C "$TMP" config user.name "merge-appendonly-falsify"
git -C "$TMP" config merge.appendonly.driver \
    "python3 $ROOT/ci/merge-appendonly.py %O %A %B"

mkdir -p "$TMP/docs"
printf 'docs/DETERMINISM.md merge=appendonly\n' > "$TMP/.gitattributes"

BASE_TEXT='## 1. First
base body

## 2. Second
base body
'
printf '%s' "$BASE_TEXT" > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" add .gitattributes docs/DETERMINISM.md
git -C "$TMP" commit -q -m base

# The counts this suite reports about itself are DERIVED here rather than
# written at the bottom. The closing line said "29 cases, 89 assertions" while
# the suite had grown to 43 and 141 — a hand-maintained count outliving its
# evidence, in the file whose whole job is to stop exactly that.
CASES="$(grep -oE "case [0-9]+[a-z]?:" "$0" | sort -u -V | wc -l)"
OKS=0
check() {
    local label="$1" cond="$2"
    if [ "$cond" = "0" ]; then
        echo "ok   $label"
        OKS=$((OKS+1))
    else
        echo "FAIL $label"
        FAIL=1
    fi
}

# --- case 1: the positive — two clean appends ------------------------------
git -C "$TMP" checkout -q -b ours-append
OURS_TAIL='
## 3. Ours — appended by branch A
ours body
'
printf '%s%s' "$BASE_TEXT" "$OURS_TAIL" > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch A appends §3"

git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b theirs-append
THEIRS_TAIL='
## 3. Theirs — appended by branch B
theirs body
'
printf '%s%s' "$BASE_TEXT" "$THEIRS_TAIL" > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch B appends its own §3"

git -C "$TMP" checkout -q ours-append
git -C "$TMP" merge -q theirs-append
MERGE_RC=$?
check "case 1: merge exits clean" "$([ "$MERGE_RC" -eq 0 ] && echo 0 || echo 1)"

MERGED="$(cat "$TMP/docs/DETERMINISM.md")"
check "case 1: no conflict markers" "$(printf '%s' "$MERGED" | grep -cE '^(<<<<<<<|\|\|\|\|\|\|\||=======|>>>>>>>)' >/dev/null && echo 1 || echo 0)"
check "case 1: base survives" "$(printf '%s' "$MERGED" | grep -cF '## 1. First' >/dev/null && echo 0 || echo 1)"
check "case 1: ours' tail survives" "$(printf '%s' "$MERGED" | grep -cF 'Ours — appended by branch A' >/dev/null && echo 0 || echo 1)"
check "case 1: theirs' tail survives" "$(printf '%s' "$MERGED" | grep -cF 'Theirs — appended by branch B' >/dev/null && echo 0 || echo 1)"
# "in order": ours' tail (the side git names `%A`, checked out here) precedes
# theirs' — assert the byte offset, not just presence.
OURS_OFF=$(printf '%s' "$MERGED" | grep -bo 'Ours — appended by branch A' | head -1 | cut -d: -f1)
THEIRS_OFF=$(printf '%s' "$MERGED" | grep -bo 'Theirs — appended by branch B' | head -1 | cut -d: -f1)
check "case 1: tails are in order (ours then theirs)" "$([ "$OURS_OFF" -lt "$THEIRS_OFF" ] && echo 0 || echo 1)"

# --- case 2: one side edits existing text, the other appends ---------------
#
# CHANGED 2026-08-17, AND THIS IS THE ONE ASSERTION IN THIS FILE THAT GOT
# WEAKER. It used to read "case 2: merge refuses (non-zero exit)" and it
# passed, because the driver refused the whole file the moment either side
# stopped being a pure append. That refusal is what blocked five of the six
# conflicted worldgen branches on 2026-08-17 — none of which disagreed with
# `main` about a single existing line — so the driver now falls back to git's
# own line-level 3-way instead of to a conflict spanning the file.
#
# So this case now asserts the OPPOSITE outcome, and the honest thing is to
# say which way it moved rather than to quietly restate it. What stops that
# being a hole is case 2b: the negative did not disappear, it moved to where
# the disagreement actually is — two sides changing the SAME text.
git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b ours-edit
# The edit goes in the FIRST section, not the last line: an edit to the very
# line the other side appends after is adjacent to that append, and git's own
# default driver conflicts on it too. This driver's promise is that it is
# never worse than git's default, not that it is better than a 3-way merge.
printf '%s' "$BASE_TEXT" | sed '0,/^base body$/s//OURS EDITED THIS LINE/' > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch C edits an existing line, not just appends"

git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b theirs-append2
printf '%s%s' "$BASE_TEXT" "$THEIRS_TAIL" > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch D appends cleanly"

git -C "$TMP" checkout -q ours-edit
git -C "$TMP" merge -q theirs-append2
MERGE2_RC=$?
check "case 2: an edit and an unrelated append MERGE (exit 0)" \
    "$([ "$MERGE2_RC" -eq 0 ] && echo 0 || echo 1)"
EDITED="$(cat "$TMP/docs/DETERMINISM.md")"
check "case 2: no conflict markers" \
    "$(printf '%s' "$EDITED" | grep -cE '^(<<<<<<<|\|\|\|\|\|\|\||>>>>>>>)' >/dev/null && echo 1 || echo 0)"
check "case 2: our edit survives" \
    "$(printf '%s' "$EDITED" | grep -cF 'OURS EDITED THIS LINE' >/dev/null && echo 0 || echo 1)"
check "case 2: their append survives" \
    "$(printf '%s' "$EDITED" | grep -cF 'Theirs — appended by branch B' >/dev/null && echo 0 || echo 1)"
# The defect this one exists for: a resolution that keeps both sides WHOLE
# rather than merging them duplicates the body. It is not hypothetical —
# regiondiff-live-fixture reached origin with a docs/DETERMINISM.md of 34,711
# lines against main's 7,191, the body repeated five times, from exactly this
# shape being "resolved" by concatenation.
check "case 2: the base body is not duplicated" \
    "$([ "$(printf '%s' "$EDITED" | grep -cF '## 1. First')" = 1 ] && echo 0 || echo 1)"
git -C "$TMP" merge --abort 2>/dev/null || true

# --- case 2b: the negative that replaces it — both sides rewrite one line --
git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b ours-rewrite
printf '%s' "$BASE_TEXT" | sed 's/^## 2. Second$/## 2. Second — OURS RENAMED IT/' > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch E renames an existing heading"

git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b theirs-rewrite
printf '%s' "$BASE_TEXT" | sed 's/^## 2. Second$/## 2. Second — THEIRS RENAMED IT/' > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch F renames the same heading differently"

git -C "$TMP" checkout -q ours-rewrite
git -C "$TMP" merge -q theirs-rewrite
MERGE2B_RC=$?
check "case 2b: two rewrites of the SAME line still refuse (non-zero exit)" \
    "$([ "$MERGE2B_RC" -ne 0 ] && echo 0 || echo 1)"
REWRITTEN="$(cat "$TMP/docs/DETERMINISM.md")"
check "case 2b: conflict markers ARE present" \
    "$(printf '%s' "$REWRITTEN" | grep -cE '^<<<<<<<' >/dev/null && echo 0 || echo 1)"
# The union rule must not reach a hunk whose base is non-empty. If it ever
# does, this is what it looks like: both headings present, no markers, and a
# stale figure living beside its own correction.
check "case 2b: the two rewrites are NOT silently unioned" \
    "$(printf '%s' "$REWRITTEN" | grep -cE '^<<<<<<<' >/dev/null && echo 0 || echo 1)"
# THE REGRESSION GUARD FOR THE 2026-08-17 DEFECT. A refusal must be a hunk,
# not the file. Under the driver as it stood, the first line of a refused
# merge was the conflict marker itself and every line of both sides sat
# inside it — 51,298 lines in one hunk on worldgen-sulfur-r18. Here §1 is
# untouched by both branches, so it must survive OUTSIDE any marker.
check "case 2b: the refusal is a hunk, not the whole file" \
    "$([ "$(printf '%s' "$REWRITTEN" | head -1)" = '## 1. First' ] && echo 0 || echo 1)"
check "case 2b: text neither side touched is not inside the conflict" \
    "$(printf '%s' "$REWRITTEN" | sed -n '/^<<<<<<< /,/^>>>>>>> /p' | grep -cF '## 1. First' >/dev/null && echo 1 || echo 0)"
git -C "$TMP" merge --abort 2>/dev/null || true

# --- case 2c: an edit ABOVE the anchor, both sides also appending ----------
#
# The shape that actually occurred: `main` corrected a line in the body and
# appended a section; the branch appended its own. Every one of the five
# blocked branches was this. Both tails must survive, in order, with the
# correction intact and the body appearing once.
git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b ours-both
printf '%s%s' "$(printf '%s' "$BASE_TEXT" | sed 's/^## 1. First$/## 1. First — CORRECTED IN THE BODY/')" "$OURS_TAIL" \
    > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch G corrects the body and appends"

git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b theirs-both
printf '%s%s' "$BASE_TEXT" "$THEIRS_TAIL" > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch H appends only"

git -C "$TMP" checkout -q ours-both
git -C "$TMP" merge -q theirs-both
MERGE2C_RC=$?
check "case 2c: body correction + two appends merges (exit 0)" \
    "$([ "$MERGE2C_RC" -eq 0 ] && echo 0 || echo 1)"
BOTH="$(cat "$TMP/docs/DETERMINISM.md")"
check "case 2c: the correction survives" \
    "$(printf '%s' "$BOTH" | grep -cF 'CORRECTED IN THE BODY' >/dev/null && echo 0 || echo 1)"
check "case 2c: ours' tail survives" \
    "$(printf '%s' "$BOTH" | grep -cF 'Ours — appended by branch A' >/dev/null && echo 0 || echo 1)"
check "case 2c: theirs' tail survives" \
    "$(printf '%s' "$BOTH" | grep -cF 'Theirs — appended by branch B' >/dev/null && echo 0 || echo 1)"
C_OURS_OFF=$(printf '%s' "$BOTH" | grep -bo 'Ours — appended by branch A' | head -1 | cut -d: -f1)
C_THEIRS_OFF=$(printf '%s' "$BOTH" | grep -bo 'Theirs — appended by branch B' | head -1 | cut -d: -f1)
check "case 2c: tails are in order (ours then theirs)" \
    "$([ "$C_OURS_OFF" -lt "$C_THEIRS_OFF" ] && echo 0 || echo 1)"
check "case 2c: the body appears once" \
    "$([ "$(printf '%s' "$BOTH" | grep -cF '## 2. Second')" = 1 ] && echo 0 || echo 1)"
git -C "$TMP" merge --abort 2>/dev/null || true

# --- case 2d: the driver is what produced the result, not git's default ----
#
# Every case above would also pass under git's built-in driver, so on its own
# this part of the file cannot tell whether `merge.appendonly.driver` is even
# wired up — the class of unfalsifiable check this repo keeps finding. Case 1
# is the discriminator: two tails appended at the same anchor conflict under
# git's default and merge under this driver. Assert that directly by running
# the same merge with the driver unset.
git -C "$TMP" config --unset merge.appendonly.driver
git -C "$TMP" checkout -q ours-append
git -C "$TMP" merge -q theirs-append2 >/dev/null 2>&1
NODRIVER_RC=$?
check "case 2d: without the driver, case 1's shape CONFLICTS (so case 1 measures the driver)" \
    "$([ "$NODRIVER_RC" -ne 0 ] && echo 0 || echo 1)"
git -C "$TMP" merge --abort 2>/dev/null || true
git -C "$TMP" config merge.appendonly.driver \
    "python3 $ROOT/ci/merge-appendonly.py %O %A %B"

# --- case 2e: adjacency, not disagreement ----------------------------------
#
# One side appends immediately above a heading it left byte-identical; the
# other rewords that heading. git lumps the two into one hunk because they
# touch. This was the whole of what remained of clamped-floor-probe's conflict
# after the 3-way fallback landed — main's new section sat directly above a
# `## 36.` heading the branch had reworded — so it is the case that decides
# whether that branch rebases by itself or waits for a human.
git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b ours-above
printf '%s%s' "$BASE_TEXT" '
## 3. Ours — appended by branch A
ours body
' > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch I appends above the tail"

git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b theirs-reword
printf '%s' "$BASE_TEXT" | sed 's/^## 2. Second$/## 2. Second — REWORDED BY THEIRS/' > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch J rewords a heading nobody else touched"

git -C "$TMP" checkout -q ours-above
git -C "$TMP" merge -q theirs-reword
MERGE2E_RC=$?
check "case 2e: an append beside an untouched-by-them reword MERGES (exit 0)" \
    "$([ "$MERGE2E_RC" -eq 0 ] && echo 0 || echo 1)"
ADJ="$(cat "$TMP/docs/DETERMINISM.md")"
check "case 2e: the reword survives" \
    "$(printf '%s' "$ADJ" | grep -cF 'REWORDED BY THEIRS' >/dev/null && echo 0 || echo 1)"
check "case 2e: the appended section survives" \
    "$(printf '%s' "$ADJ" | grep -cF 'Ours — appended by branch A' >/dev/null && echo 0 || echo 1)"
check "case 2e: the old heading is gone, not kept beside the new one" \
    "$([ "$(printf '%s' "$ADJ" | grep -cE '^## 2\. Second$')" = 0 ] && echo 0 || echo 1)"
check "case 2e: no conflict markers" \
    "$(printf '%s' "$ADJ" | grep -cE '^(<<<<<<<|\|\|\|\|\|\|\||>>>>>>>)' >/dev/null && echo 1 || echo 0)"
git -C "$TMP" merge --abort 2>/dev/null || true

# --- case 2f: a DELETION is never resolved automatically -------------------
#
# The rule case 2e exercises reads "one side left the base region identical
# and added next to it — keep the addition, take the other side's version of
# the region". If "the other side's version" is nothing at all, taking it
# deletes text, and this file's whole contract is that text is appended and
# not removed. The driver must refuse, however unambiguous the diff looks.
git -C "$TMP" checkout -q main
git -C "$TMP" checkout -q -b theirs-delete
printf '%s' "$BASE_TEXT" | sed '/^## 2. Second$/,+1d' > "$TMP/docs/DETERMINISM.md"
git -C "$TMP" commit -q -am "branch K deletes an existing section"

git -C "$TMP" checkout -q ours-above
git -C "$TMP" merge -q theirs-delete
MERGE2F_RC=$?
check "case 2f: an append beside a DELETION refuses (non-zero exit)" \
    "$([ "$MERGE2F_RC" -ne 0 ] && echo 0 || echo 1)"
DEL="$(cat "$TMP/docs/DETERMINISM.md")"
check "case 2f: conflict markers ARE present" \
    "$(printf '%s' "$DEL" | grep -cE '^<<<<<<<' >/dev/null && echo 0 || echo 1)"
check "case 2f: the deleted text is still visible to the human" \
    "$(printf '%s' "$DEL" | grep -cF '## 2. Second' >/dev/null && echo 0 || echo 1)"
git -C "$TMP" merge --abort 2>/dev/null || true


# ===========================================================================
# PART 2 — ci/merge-docregions.py
# ===========================================================================
#
# Each case builds a fresh throwaway repo: a base commit, one branch per side,
# then a real `git merge`. `ours` is the checked-out side, which is what
# upstream is during a rebase, so these scenarios have main's shape on the
# `ours` side exactly as the merge train sees it.

RTMP="$(mktemp -d)"
trap 'rm -rf "$TMP" "$RTMP"' EXIT

MERGE_RC=0
MERGED_FILE=""

# run_merge <relpath> <base-text> <ours-text> <theirs-text>
run_merge() {
    local rel="$1" btxt="$2" otxt="$3" ttxt="$4" R
    R="$RTMP/r$RANDOM$RANDOM"
    mkdir -p "$R/$(dirname "$rel")"
    git -C "$R" init -q -b main
    git -C "$R" config user.email "falsify@example.invalid"
    git -C "$R" config user.name "merge-docregions-falsify"
    git -C "$R" config merge.docregions.driver \
        "python3 $ROOT/ci/merge-docregions.py %O %A %B %P"
    printf '%s merge=docregions\n' "$rel" > "$R/.gitattributes"
    printf '%s' "$btxt" > "$R/$rel"
    git -C "$R" add .gitattributes "$rel"
    git -C "$R" commit -q -m base
    git -C "$R" checkout -q -b side-ours
    printf '%s' "$otxt" > "$R/$rel"
    git -C "$R" commit -q -am ours
    git -C "$R" checkout -q main
    git -C "$R" checkout -q -b side-theirs
    printf '%s' "$ttxt" > "$R/$rel"
    git -C "$R" commit -q -am theirs
    git -C "$R" checkout -q side-ours
    git -C "$R" merge -q side-theirs >/dev/null 2>&1
    MERGE_RC=$?
    MERGED_FILE="$R/$rel"
}

has()     { grep -qF -- "$2" "$1" && echo 0 || echo 1; }
hasnt()   { grep -qF -- "$2" "$1" && echo 1 || echo 0; }
countf()  { grep -cF -- "$2" "$1"; }
conflicted() { grep -qE '^<<<<<<< ' "$1" && echo 0 || echo 1; }
clean_rc()   { [ "$MERGE_RC" -eq 0 ] && echo 0 || echo 1; }
refused_rc() { [ "$MERGE_RC" -ne 0 ] && echo 0 || echo 1; }

# --- the WORKLIST shape: a bounded, keyed region in a rewritten document ----

WL_HEAD='# WORKLIST

## NOW — worldgen features

STATUS: item 3 in flight, 42 blocks moved.

<!-- MERGE-REGION BEGIN cold-start-index key=first-cell check=unique-first-cell -->
| branch | date | summary | file |
|---|---|---|---|
'
WL_ROWS='| `alpha` | 2026-08-01 | alphas first summary | [a](a.md) |
| `beta` | 2026-08-02 | betas summary | [b](b.md) |
'
WL_TAIL='<!-- MERGE-REGION END cold-start-index -->

## THEN, in order
tail prose
'
wl() { printf '%s%s%s' "$WL_HEAD" "$1" "$WL_TAIL"; }

ROW_G='| `gamma` | 2026-08-16 | gammas summary | [g](g.md) |
'
ROW_D='| `delta` | 2026-08-16 | deltas summary | [d](d.md) |
'
ROW_A2='| `alpha` | 2026-08-16 | ALPHA REPLACED ITS OWN ROW | [a](a.md) |
'

# case 3 — the item itself: two branches each append one row.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" "$(wl "$ROW_G$WL_ROWS")" "$(wl "$ROW_D$WL_ROWS")"
check "case 3: two appended rows merge clean" "$(clean_rc)"
check "case 3: no conflict markers" "$([ "$(conflicted "$MERGED_FILE")" = 0 ] && echo 1 || echo 0)"
check "case 3: ours' row survives"   "$(has "$MERGED_FILE" '`gamma`')"
check "case 3: theirs' row survives" "$(has "$MERGED_FILE" '`delta`')"
check "case 3: base rows survive"    "$(has "$MERGED_FILE" '`beta`')"
check "case 3: ## NOW is untouched"  "$(has "$MERGED_FILE" 'STATUS: item 3 in flight, 42 blocks moved.')"
check "case 3: exactly one row per key" \
    "$([ "$(countf "$MERGED_FILE" '| `alpha` |')" = 1 ] && echo 0 || echo 1)"

# case 4 — AGENT-BRIEF's documented workflow: replace your own row while
# another branch appends above it. This is the case that made keyed rows
# necessary; under `merge=union` it is what put two contradictory
# `ci-unfalsifiable-root` rows in the real file.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" "$(wl "$ROW_G$WL_ROWS")" \
          "$(wl "$ROW_A2| \`beta\` | 2026-08-02 | betas summary | [b](b.md) |
")"
check "case 4: replace-own-row beside an append merges clean" "$(clean_rc)"
check "case 4: the REPLACEMENT text is present" "$(has "$MERGED_FILE" 'ALPHA REPLACED ITS OWN ROW')"
check "case 4: the REPLACED text is gone"       "$(hasnt "$MERGED_FILE" 'alphas first summary')"
check "case 4: alpha appears exactly once"      "$([ "$(countf "$MERGED_FILE" '| `alpha` |')" = 1 ] && echo 0 || echo 1)"
check "case 4: the other branch's new row survives" "$(has "$MERGED_FILE" '`gamma`')"

# case 5 — each side edits a DIFFERENT existing row. Per-key, both land.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" \
    "$(wl "| \`alpha\` | 2026-08-16 | OURS EDITED ALPHA | [a](a.md) |
| \`beta\` | 2026-08-02 | betas summary | [b](b.md) |
")" \
    "$(wl "| \`alpha\` | 2026-08-01 | alphas first summary | [a](a.md) |
| \`beta\` | 2026-08-16 | THEIRS EDITED BETA | [b](b.md) |
")"
check "case 5: edits to different rows merge clean" "$(clean_rc)"
check "case 5: ours' edit survives"   "$(has "$MERGED_FILE" 'OURS EDITED ALPHA')"
check "case 5: theirs' edit survives" "$(has "$MERGED_FILE" 'THEIRS EDITED BETA')"

# case 6 — the transition: `theirs` predates the markers entirely and appends
# its row at the BOTTOM of the table, where `ours` put the END marker. The row
# must land INSIDE the region, not below its closing marker.
WL_NOMARK_HEAD='# WORKLIST

## NOW — worldgen features

STATUS: item 3 in flight, 42 blocks moved.

| branch | date | summary | file |
|---|---|---|---|
'
WL_NOMARK_TAIL='
## THEN, in order
tail prose
'
run_merge docs/WORKLIST.md \
    "$(printf '%s%s%s' "$WL_NOMARK_HEAD" "$WL_ROWS" "$WL_NOMARK_TAIL")" \
    "$(wl "$WL_ROWS")" \
    "$(printf '%s%s%s%s' "$WL_NOMARK_HEAD" "$WL_ROWS" "$ROW_D" "$WL_NOMARK_TAIL")"
check "case 6: a bottom append across the marker-introducing commit merges clean" "$(clean_rc)"
check "case 6: the appended row is INSIDE the region, above the END marker" \
    "$(awk '/MERGE-REGION END cold-start-index/{end=NR} /`delta`/{row=NR} END{exit !(row && end && row < end)}' \
        "$MERGED_FILE" && echo 0 || echo 1)"

# --- the PARITY-ROADMAP shape: a to-eof region -----------------------------

PR_HEAD='# ROADMAP

intro prose that gets rewritten in place

<!-- MERGE-REGION BEGIN sections to-eof check=unique-heading-number -->

## 1. First
body one

'
pr() { printf '%s%s' "$PR_HEAD" "$1"; }

# case 7 — two branches each append a new section at EOF, different numbers.
run_merge docs/PARITY-ROADMAP.md "$(pr '## 2. Second
body two
')" "$(pr '## 2. Second
body two

## 3. Ours new section
ours body
')" "$(pr '## 2. Second
body two

## 4. Theirs new section
theirs body
')"
check "case 7: two appended sections merge clean" "$(clean_rc)"
check "case 7: ours' section survives"   "$(has "$MERGED_FILE" '## 3. Ours new section')"
check "case 7: theirs' section survives" "$(has "$MERGED_FILE" '## 4. Theirs new section')"

# case 8 — the section-renumber case, measured on `gameplay-block-schemas`:
# ours appends below an untouched last heading while theirs RENUMBERS it.
run_merge docs/PARITY-ROADMAP.md "$(pr '## 2. Second
')" "$(pr '## 2. Second

## 3. Ours new section
ours body
')" "$(pr '## 5. Second
')"
check "case 8: append-below beside a renumber merges clean" "$(clean_rc)"
check "case 8: the renumbered heading survives" "$(has "$MERGED_FILE" '## 5. Second')"
check "case 8: the old number is gone"          "$(hasnt "$MERGED_FILE" '## 2. Second')"
check "case 8: ours' appended section survives" "$(has "$MERGED_FILE" '## 3. Ours new section')"

# --- the refusals ----------------------------------------------------------

# case 9 — both sides rewrite the SAME row, differently. A genuine
# disagreement about a row, and the whole reason this is not a union.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" \
    "$(wl "| \`alpha\` | 2026-08-16 | OURS SAYS 10 OF 84 | [a](a.md) |
| \`beta\` | 2026-08-02 | betas summary | [b](b.md) |
")" \
    "$(wl "| \`alpha\` | 2026-08-16 | THEIRS SAYS 4 OF 84 | [a](a.md) |
| \`beta\` | 2026-08-02 | betas summary | [b](b.md) |
")"
check "case 9: two rewrites of one row REFUSE (non-zero)" "$(refused_rc)"
check "case 9: conflict markers are present"              "$(conflicted "$MERGED_FILE")"
check "case 9: neither side was silently kept alone" \
    "$([ "$(countf "$MERGED_FILE" 'OURS SAYS 10 OF 84')" = 1 ] && \
       [ "$(countf "$MERGED_FILE" 'THEIRS SAYS 4 OF 84')" = 1 ] && echo 0 || echo 1)"

# case 10 — THE `## NOW` GUARANTEE, in its rewritten form. Both sides rewrite
# the same status line. `## NOW` is outside every declared region, so this
# never even reaches the append rule.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" \
    "$(printf '%s' "$(wl "$WL_ROWS")" | sed 's/42 blocks moved./OURS: 7,940 blocks moved./')" \
    "$(printf '%s' "$(wl "$WL_ROWS")" | sed 's/42 blocks moved./THEIRS: 69,288 blocks moved./')"
check "case 10: two rewrites of a ## NOW line REFUSE"    "$(refused_rc)"
check "case 10: conflict markers are present"            "$(conflicted "$MERGED_FILE")"
check "case 10: both figures are visible to the human"   \
    "$([ "$(countf "$MERGED_FILE" '7,940 blocks moved')" -ge 1 ] && \
       [ "$(countf "$MERGED_FILE" '69,288 blocks moved')" -ge 1 ] && echo 0 || echo 1)"

# case 11 — THE `## NOW` GUARANTEE, in its append form, and this refusal is
# deliberate rather than incidental: two branches adding DIFFERENT prose at the
# same `## NOW` anchor is an add/add, which inside a region would be resolved.
# Outside every region it conflicts, because the file declares no region there.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" \
    "$(printf '%s' "$(wl "$WL_ROWS")" | sed 's/^STATUS: /OURS ADDED A LINE\nSTATUS: /')" \
    "$(printf '%s' "$(wl "$WL_ROWS")" | sed 's/^STATUS: /THEIRS ADDED A LINE\nSTATUS: /')"
check "case 11: an add/add in ## NOW REFUSES (no region is declared there)" "$(refused_rc)"
check "case 11: conflict markers are present" "$(conflicted "$MERGED_FILE")"

# case 12 — one side DELETES a row the other side edits.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" \
    "$(wl '| `alpha` | 2026-08-01 | alphas first summary | [a](a.md) |
')" \
    "$(wl "| \`alpha\` | 2026-08-01 | alphas first summary | [a](a.md) |
| \`beta\` | 2026-08-16 | THEIRS EDITED BETA | [b](b.md) |
")"
check "case 12: delete-vs-edit of one row REFUSES" "$(refused_rc)"
check "case 12: conflict markers are present"      "$(conflicted "$MERGED_FILE")"

# case 13 — both sides ADD a row under the same key, with different content.
# The append rule would keep both; the `unique-first-cell` post-condition is
# what refuses it, and the refusal is the same shape as any other conflict.
run_merge docs/WORKLIST.md "$(wl "$WL_ROWS")" \
    "$(wl "| \`gamma\` | 2026-08-16 | OURS VERSION OF GAMMA | [g](g.md) |
$WL_ROWS")" \
    "$(wl "| \`gamma\` | 2026-08-16 | THEIRS VERSION OF GAMMA | [g](g.md) |
$WL_ROWS")"
check "case 13: two rows claiming one key REFUSE (post-condition)" "$(refused_rc)"
check "case 13: conflict markers are present"                      "$(conflicted "$MERGED_FILE")"

# case 14 — two branches appending a section under the SAME `## N.`.
# `ci/check-doc-claims.py` refuses this at gate time; refusing it here costs a
# whole gate less.
run_merge docs/PARITY-ROADMAP.md "$(pr '## 2. Second
body two
')" "$(pr '## 2. Second
body two

## 3. Ours claims three
ours body
')" "$(pr '## 2. Second
body two

## 3. Theirs also claims three
theirs body
')"
check "case 14: a duplicated ## N. REFUSES (post-condition)" "$(refused_rc)"
check "case 14: conflict markers are present"                "$(conflicted "$MERGED_FILE")"

# --- the ci/figures.toml shape ---------------------------------------------

FIG_HEAD='[meta]
min_figures = 1

# MERGE-REGION BEGIN figures to-eof check=toml-figures

[[figure]]
id = "base-one"
claim = "1%"
'

# case 15 — two branches each register a new figure at EOF.
run_merge ci/figures.toml "$FIG_HEAD" \
    "$FIG_HEAD"'
[[figure]]
id = "ours-fig"
claim = "2%"
' \
    "$FIG_HEAD"'
[[figure]]
id = "theirs-fig"
claim = "3%"
'
check "case 15: two appended [[figure]] blocks merge clean" "$(clean_rc)"
check "case 15: the merged ledger still parses as TOML, 3 unique ids" \
    "$(python3 -c "
import tomllib,collections,sys
d=tomllib.load(open(sys.argv[1],'rb'))
ids=[f['id'] for f in d['figure']]
sys.exit(0 if sorted(ids)==['base-one','ours-fig','theirs-fig'] else 1)
" "$MERGED_FILE" && echo 0 || echo 1)"

# case 16 — both register a figure under the same id. `check=toml-figures`
# re-parses the merged ledger and refuses, so a bad resolution cannot reach
# the file `ci/check-figures.py` adjudicates.
run_merge ci/figures.toml "$FIG_HEAD" \
    "$FIG_HEAD"'
[[figure]]
id = "collide"
claim = "2%"
' \
    "$FIG_HEAD"'
[[figure]]
id = "collide"
claim = "3%"
'
check "case 16: a duplicated [[figure]] id REFUSES (post-condition)" "$(refused_rc)"
check "case 16: conflict markers are present"                        "$(conflicted "$MERGED_FILE")"

# --- the ci/checks/gameplay-falsifiable.toml shape: to-eof, NO key ---------
#
# Rows are `[[scenario]]` stanzas spanning several lines, so — unlike
# figures.toml, whose only field measured here is `id` — a real re-sweep
# edits `killed_by` on an EXISTING stanza without touching its `name` line at
# all. There is no key that would let two such edits merge per-row the way
# `key=toml-key` does for baseline.toml below; this region relies on the
# plain append rule plus `check=toml-scenarios`, and case 22 is the point:
# proving an edited stanza still conflicts, adjacent insert or not.

FALS_HEAD='[meta]
floor = 1

# MERGE-REGION BEGIN scenarios to-eof check=toml-scenarios

[[scenario]]
name = "base-one"
killed_by = ["drop-measured"]
tier = 2
'

# case 20 — two branches each append a new [[scenario]] stanza at EOF.
run_merge ci/checks/gameplay-falsifiable.toml "$FALS_HEAD" \
    "$FALS_HEAD"'
[[scenario]]
name = "ours-scenario"
killed_by = ["drop-measured"]
tier = 2
' \
    "$FALS_HEAD"'
[[scenario]]
name = "theirs-scenario"
killed_by = ["drop-measured"]
tier = 2
'
check "case 20: two appended [[scenario]] stanzas merge clean" "$(clean_rc)"
check "case 20: the merged ledger still parses as TOML, 3 unique names" \
    "$(python3 -c "
import tomllib,sys
d=tomllib.load(open(sys.argv[1],'rb'))
names=[s['name'] for s in d['scenario']]
sys.exit(0 if sorted(names)==['base-one','ours-scenario','theirs-scenario'] else 1)
" "$MERGED_FILE" && echo 0 || echo 1)"

# case 21 — both sides append a stanza under the same NEW name, differently.
# `check=toml-scenarios` catches what the append rule alone would union.
run_merge ci/checks/gameplay-falsifiable.toml "$FALS_HEAD" \
    "$FALS_HEAD"'
[[scenario]]
name = "collide"
killed_by = ["drop-measured"]
tier = 2
' \
    "$FALS_HEAD"'
[[scenario]]
name = "collide"
killed_by = ["retype"]
tier = 2
'
check "case 21: a duplicated [[scenario]] name REFUSES (post-condition)" "$(refused_rc)"
check "case 21: conflict markers are present"                            "$(conflicted "$MERGED_FILE")"

# case 22 — THE REAL SHAPE, measured 2026-08-17 on the gameplay corpus: one
# side re-sweeps `base-one`'s own `killed_by` (an edit to existing text)
# immediately above where the other side appends a brand new stanza. Adjacent
# in the file, so git hands them to the driver as ONE hunk. The append rule
# must not paper over the edit: this has to conflict, exactly as it would
# with no driver installed, and the new stanza is not silently dropped either.
run_merge ci/checks/gameplay-falsifiable.toml "$FALS_HEAD" \
    "$(printf '%s' "$FALS_HEAD" | sed 's/killed_by = \["drop-measured"\]/killed_by = ["drop-measured", "perturb:item"]/')" \
    "$FALS_HEAD"'
[[scenario]]
name = "theirs-new"
killed_by = ["drop-measured"]
tier = 2
'
check "case 22: an edited existing stanza beside a new one REFUSES" "$(refused_rc)"
check "case 22: conflict markers are present"                       "$(conflicted "$MERGED_FILE")"
check "case 22: both readings of the edited field are visible" \
    "$([ "$(countf "$MERGED_FILE" 'killed_by = ["drop-measured"]')" -ge 1 ] && \
       [ "$(countf "$MERGED_FILE" 'killed_by = ["drop-measured", "perturb:item"]')" -ge 1 ] && echo 0 || echo 1)"

# --- the ci/checks/gameplay-baseline.toml shape: to-eof, key=toml-key ------

BASE_HEAD='[expect]
# MERGE-REGION BEGIN rows to-eof key=toml-key check=unique-toml-key
alpha = "match"
beta = "diverge"
'

# case 23 — two branches each append a new row.
run_merge ci/checks/gameplay-baseline.toml "$BASE_HEAD" \
    "$BASE_HEAD"'gamma = "match"
' \
    "$BASE_HEAD"'delta = "match"
'
check "case 23: two appended baseline rows merge clean" "$(clean_rc)"
check "case 23: ours' row survives"   "$(has "$MERGED_FILE" 'gamma = "match"')"
check "case 23: theirs' row survives" "$(has "$MERGED_FILE" 'delta = "match"')"

# case 24 — each side edits a DIFFERENT existing row. Per-key, both land —
# the same guarantee `key=first-cell` gives WORKLIST's table, case 5 above.
run_merge ci/checks/gameplay-baseline.toml "$BASE_HEAD" \
    "$(printf '%s' "$BASE_HEAD" | sed 's/alpha = "match"/alpha = "diverge"/')" \
    "$(printf '%s' "$BASE_HEAD" | sed 's/beta = "diverge"/beta = "match"/')"
check "case 24: edits to different baseline rows merge clean" "$(clean_rc)"
check "case 24: ours' edit survives"   "$(has "$MERGED_FILE" 'alpha = "diverge"')"
check "case 24: theirs' edit survives" "$(has "$MERGED_FILE" 'beta = "match"')"

# case 25 — both sides rewrite the SAME existing row, differently. A genuine
# regression disagreement and must conflict, key or no key.
run_merge ci/checks/gameplay-baseline.toml "$BASE_HEAD" \
    "$(printf '%s' "$BASE_HEAD" | sed 's/alpha = "match"/alpha = "diverge"/')" \
    "$(printf '%s' "$BASE_HEAD" | sed 's/alpha = "match"/alpha = "absent"/')"
check "case 25: two rewrites of one baseline row REFUSE" "$(refused_rc)"
check "case 25: conflict markers are present"             "$(conflicted "$MERGED_FILE")"

# --- the ci/t0.sh shape: a bounded region spanning the whole executable body,
# no key, check=bash-syntax ---------------------------------------------------
#
# Measured 2026-08-17: six branches touched ci/t0.sh that day with zero
# deletions between them, each new check block landing beside the existing one
# it is topically closest to rather than at a shared tail — so the region here
# is `checks`, BEGIN right after `cd`, END right before the closing sentinel,
# not a table or a `to-eof` tail. There is no key: a check block is not named
# anywhere the merge can read. `bash -n` on the whole merged file is what is
# left to verify unconditionally, and case 28 is the point of it — a merge
# this driver would otherwise wave through.

T0_HEAD='#!/usr/bin/env bash
# T0 — fmt + clippy + check
set -euo pipefail
cd "$(dirname "$0")/.."

# MERGE-REGION BEGIN checks check=bash-syntax
# base check
echo "base check"
'
T0_TAIL='# MERGE-REGION END checks

echo "T0 GREEN"
'
t0() { printf '%s%s%s' "$T0_HEAD" "$1" "$T0_TAIL"; }

# case 26 — two branches each append a new check block, next to the one they
# are topically closest to rather than at a shared tail. Merges clean and the
# result still parses.
run_merge ci/t0.sh "$(t0 '')" \
    "$(t0 '
# ours new check
echo "ours check"
')" \
    "$(t0 '
# theirs new check
echo "theirs check"
')"
check "case 26: two appended check blocks merge clean"  "$(clean_rc)"
check "case 26: ours' check survives"                    "$(has "$MERGED_FILE" 'echo "ours check"')"
check "case 26: theirs' check survives"                  "$(has "$MERGED_FILE" 'echo "theirs check"')"
check "case 26: the merged file still parses as bash" \
    "$(bash -n "$MERGED_FILE" >/dev/null 2>&1 && echo 0 || echo 1)"

# case 27 — both sides edit the SAME existing check's line, differently. A
# genuine disagreement about what that check does, and it must conflict
# exactly as it would with no driver installed — no key exists here to give
# it a finer-grained answer the way `key=toml-key` would for baseline.toml.
run_merge ci/t0.sh "$(t0 '')" \
    "$(t0 '' | sed 's/echo "base check"/echo "OURS changed the base check"/')" \
    "$(t0 '' | sed 's/echo "base check"/echo "THEIRS changed the base check"/')"
check "case 27: two rewrites of one existing check REFUSE" "$(refused_rc)"
check "case 27: conflict markers are present"               "$(conflicted "$MERGED_FILE")"
check "case 27: neither side was silently kept alone" \
    "$([ "$(countf "$MERGED_FILE" 'OURS changed the base check')" = 1 ] && \
       [ "$(countf "$MERGED_FILE" 'THEIRS changed the base check')" = 1 ] && echo 0 || echo 1)"

# case 27b — the shape rule (c) exists for: one side purely inserts a new
# check elsewhere while the OTHER independently edits an existing one, base
# left contiguous on the inserting side. This is not two sides disagreeing
# about the same text — theirs never touched that line at all — and it is the
# same rule `gameplay-block-schemas`' §44 -> §51 renumber relies on beside
# main's own appended sections (`ci/merge-docregions.py`'s own docstring).
# Documented here as a POSITIVE so nobody mistakes case 27's refusal for "any
# edit inside this region conflicts" and reports this one as a regression.
run_merge ci/t0.sh "$(t0 '')" \
    "$(t0 '' | sed 's/echo "base check"/echo "base check, comment fixed"/')" \
    "$(t0 '
# theirs new check
echo "theirs check"
')"
check "case 27b: an untouched-elsewhere edit beside an unrelated append merges clean" "$(clean_rc)"
check "case 27b: the edit survives"   "$(has "$MERGED_FILE" 'echo "base check, comment fixed"')"
check "case 27b: theirs' new check survives" "$(has "$MERGED_FILE" 'echo "theirs check"')"
check "case 27b: the merged file still parses as bash" \
    "$(bash -n "$MERGED_FILE" >/dev/null 2>&1 && echo 0 || echo 1)"

# case 28 — a pure append (base empty, so the driver auto-resolves it) whose
# appended text is individually broken: an unterminated quote. The append
# rule alone would wave this through — nothing about "neither side touched
# existing text" says the new text is well-formed. `check=bash-syntax` is what
# catches it, over the WHOLE merged file, same scope `toml-figures` uses.
run_merge ci/t0.sh "$(t0 '')" \
    "$(t0 '
# ours new check
echo "ours check"
')" \
    "$(t0 '
# theirs new check, broken
echo "theirs unterminated
')"
check "case 28: an append that breaks bash syntax REFUSES (post-condition)" "$(refused_rc)"
check "case 28: conflict markers are present"                               "$(conflicted "$MERGED_FILE")"

# --- the structural checker, which is what T0 actually runs ----------------

CHK="$RTMP/chk"; mkdir -p "$CHK"
printf '%s' "$(wl "$WL_ROWS")" > "$CHK/good.md"
check "case 17: --check accepts a well-formed region" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/good.md" >/dev/null 2>&1 && echo 0 || echo 1)"

grep -v 'MERGE-REGION END' "$CHK/good.md" > "$CHK/lost-end.md"
check "case 17: --check REFUSES a region whose END marker was lost" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/lost-end.md" >/dev/null 2>&1 && echo 1 || echo 0)"

grep -v 'MERGE-REGION' "$CHK/good.md" > "$CHK/no-region.md"
check "case 17: --check REFUSES a file that declares no region at all" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/no-region.md" >/dev/null 2>&1 && echo 1 || echo 0)"

sed 's/key=first-cell/key=no-such-key/' "$CHK/good.md" > "$CHK/bad-attr.md"
check "case 17: --check REFUSES an unknown key" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/bad-attr.md" >/dev/null 2>&1 && echo 1 || echo 0)"

# --- case 31: a check over ZERO files is not a check -----------------------
#
# `--check` used to `for p in paths` and return 0, so with no file arguments it
# read nothing and exited GREEN. `ci/t0.sh` treats that zero as "no MERGE-REGION
# marker was lost". The list reaches it as an UNQUOTED `$REGION_FILES` from
# ci/setup-merge-drivers.sh — unquoted because it must word-split — and an
# unquoted variable that goes empty contributes no arguments at all.
#
# Found by running `--check` by hand with no arguments against a file whose END
# marker had just been deleted, and getting a green.
check "case 31: --check with NO files REFUSES instead of passing over nothing" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check >/dev/null 2>&1 && echo 1 || echo 0)"
check "case 31: and the refusal is distinct from a content RED (exit 2, not 1)" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check >/dev/null 2>&1; [ $? = 2 ] && echo 0 || echo 1)"
check "case 31: a real file still passes, so 29 is not just 'always refuse'" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/good.md" >/dev/null 2>&1 && echo 0 || echo 1)"

# --- case 32: .gitattributes and $REGION_FILES are one fact, kept twice -----
#
# `.gitattributes` decides which files the driver RUNS on; $REGION_FILES in
# ci/setup-merge-drivers.sh decides which get `--check`ed for a lost marker.
# Nothing compared them. Add a seventh `merge=docregions` line and forget the
# variable and the new file is driven but never checked, so a lost marker in it
# is silent — the exact state `--check` exists to make loud.
#
# Both directions are asserted because they fail differently: a name only in
# .gitattributes is unchecked, a name only in REGION_FILES makes this script
# assert a registration that is not in force.
# The drift copy is built from the WORKING TREE, not from `git archive HEAD`.
# It was the archive until 2026-09-04, and that failed the case in the one
# place it matters: the merge gate runs T0 in an rsynced directory, where
# `git archive` had no repository and the copy came up empty, so "the tree as
# shipped agrees with itself" reddened on a tree that did
# (gate-ci-docregions-check-vacuous-20260904-204737.log:209). The working
# tree is also the more honest subject — it is what `ci/t0-remote.sh` ships
# and what `setup-merge-drivers.sh` will read, uncommitted edits included.
DRIFT="$RTMP/drift"; rm -rf "$DRIFT"; mkdir -p "$DRIFT/ci"
eval "$(grep -E '^REGION_FILES=' "$ROOT/ci/setup-merge-drivers.sh")"
for f in .gitattributes ci/setup-merge-drivers.sh ci/merge-docregions.py \
         ci/merge-appendonly.py $REGION_FILES; do
    mkdir -p "$DRIFT/$(dirname "$f")"
    cp "$ROOT/$f" "$DRIFT/$f"
done
drift_rc() { ( cd "$DRIFT" && ./ci/setup-merge-drivers.sh >/dev/null 2>&1; echo $? ); }

check "case 32: the tree as shipped agrees with itself" \
    "$([ "$(drift_rc)" = 0 ] && echo 0 || echo 1)"

echo 'docs/CI.md merge=docregions' >> "$DRIFT/.gitattributes"
check "case 32: a file driven but NOT in \$REGION_FILES is refused" \
    "$([ "$(drift_rc)" != 0 ] && echo 0 || echo 1)"
sed -i '/^docs\/CI.md merge=docregions$/d' "$DRIFT/.gitattributes"

check "case 32: and removing it again is green (the guard is not stuck red)" \
    "$([ "$(drift_rc)" = 0 ] && echo 0 || echo 1)"

sed -i '/^ci\/t0.sh merge=docregions$/d' "$DRIFT/.gitattributes"
check "case 32: a file in \$REGION_FILES but no longer driven is refused too" \
    "$([ "$(drift_rc)" != 0 ] && echo 0 || echo 1)"

# case 19 — the post-condition is scoped to the REGION, not the file. The real
# `docs/WORKLIST.md` holds other tables whose first cell legitimately repeats
# (`chat-commands`, `chest-opens-a-screen`, the scenario tables), so a key
# counted file-wide refuses an innocent merge. A post-condition that fires on
# innocent input is the same defect class as one that cannot fire at all.
#
# BOTH SIDES MUST APPEND AN INDEX ROW or the post-condition never runs: two
# edits far enough apart merge with no hunk, the driver returns early, and the
# case passes whatever the scoping does. The first draft of this case did
# exactly that and was green with the fix injected out — an unfalsifiable
# check, caught by injecting the fix out and re-running, which is the only
# thing that establishes a case has teeth.
WL_OTHER='
## Some other table

| scenario | verdict |
|---|---|
| `zeta` | match |
'
wl2() { printf '%s%s%s%s' "$WL_HEAD" "$1" "$WL_TAIL" "$2"; }
run_merge docs/WORKLIST.md "$(wl2 "$WL_ROWS" "$WL_OTHER")" \
    "$(wl2 "$ROW_G$WL_ROWS" "$WL_OTHER")" \
    "$(wl2 "$ROW_D$WL_ROWS" "$WL_OTHER"'| `gamma` | diverge |
')"
check "case 19: a key repeated OUTSIDE the region does not trip the post-condition" "$(clean_rc)"
check "case 19: no conflict markers" "$([ "$(conflicted "$MERGED_FILE")" = 0 ] && echo 1 || echo 0)"
check "case 19: ours' index row survives"       "$(has "$MERGED_FILE" '| `gamma` | 2026-08-16 |')"
check "case 19: theirs' index row survives"     "$(has "$MERGED_FILE" '| `delta` | 2026-08-16 |')"
check "case 19: the other table's row survives" "$(has "$MERGED_FILE" '| `gamma` | diverge |')"

# case 18 — a malformed marker must DEGRADE TO A CONFLICT, never to a union.
# The driver resolves nothing when it cannot read the ours side's structure,
# so a two-append merge that case 3 proved clean now refuses instead.
WL_BAD_HEAD="${WL_HEAD//key=first-cell/key=no-such-key}"
wlbad() { printf '%s%s%s' "$WL_BAD_HEAD" "$1" "$WL_TAIL"; }
run_merge docs/WORKLIST.md "$(wlbad "$WL_ROWS")" "$(wlbad "$ROW_G$WL_ROWS")" "$(wlbad "$ROW_D$WL_ROWS")"
check "case 18: a malformed region degrades to a CONFLICT, not a union" "$(refused_rc)"
check "case 18: conflict markers are present"                            "$(conflicted "$MERGED_FILE")"

# --- case 29: docs/FINDINGS.md, the file every lane appends to --------------
#
# Added 2026-09-05, and the measurement is the argument: 659 of the 666 pairs
# of the 37 branches ahead of main that touch this file conflict on it. The
# file was created 2026-08-19 — the day AFTER `.gitattributes`' coverage sweep
# concluded the remaining undriven surface was 19 pairs of 1035 — so it is not
# a seventh driver added on a hunch, it is the sweep that note asked for.
#
# Its region is `to-eof` with no key: a finding is a Markdown bullet, not a
# keyed row, and the post-condition counts the bullets' own first lines.

FD_HEAD='# FINDINGS

Append-only. One line per finding.

---

<!-- MERGE-REGION BEGIN findings to-eof check=unique-bullet-line -->

## 2026-09-01

* 2026-09-01 the base finding, present on every side.
'
FD_A='* 2026-09-05 lane A found the first thing.
'
FD_B='* 2026-09-05 lane B found the second thing.
'

run_merge docs/FINDINGS.md "$FD_HEAD" "$FD_HEAD$FD_A" "$FD_HEAD$FD_B"
check "case 29: two lanes appending a finding MERGE (exit 0)" "$(clean_rc)"
check "case 29: no conflict markers"        "$([ "$(conflicted "$MERGED_FILE")" = 0 ] && echo 1 || echo 0)"
check "case 29: the base finding survives"  "$(has "$MERGED_FILE" 'the base finding')"
check "case 29: lane A's finding survives"  "$(has "$MERGED_FILE" 'lane A found the first thing')"
check "case 29: lane B's finding survives"  "$(has "$MERGED_FILE" 'lane B found the second thing')"
check "case 29: neither is duplicated"      "$([ "$(countf "$MERGED_FILE" 'lane A found the first thing')" = 1 ] && echo 0 || echo 1)"

# --- case 29b: the negative — a rewritten finding still conflicts -----------
#
# Without this, case 20 is satisfied by `merge=union`, which is the defect this
# whole driver exists to refuse and whose damage is still legible in WORKLIST.
FD_REWRITE_A="${FD_HEAD/the base finding, present on every side./the base finding, corrected by lane A.}"
FD_REWRITE_B="${FD_HEAD/the base finding, present on every side./the base finding, corrected by lane B.}"
run_merge docs/FINDINGS.md "$FD_HEAD" "$FD_REWRITE_A" "$FD_REWRITE_B"
check "case 29b: two rewrites of the SAME finding still refuse" "$(refused_rc)"
check "case 29b: conflict markers ARE present"                  "$(conflicted "$MERGED_FILE")"
check "case 29b: the two rewrites are NOT unioned" \
    "$([ "$(countf "$MERGED_FILE" 'corrected by lane')" -le 2 ] && echo 0 || echo 1)"

# --- case 29c: the post-condition has teeth --------------------------------
#
# One side REPLACES its own earlier finding — the shape that produced two
# contradictory `ci-unfalsifiable-root` rows in WORKLIST under `merge=union` —
# while the other appends. The replacement must not come back beside its
# replacement. Both sides append inside the region or no hunk is formed and the
# driver returns early, which is how case 19 was green with its fix injected
# out.
FD_OLD='* 2026-09-02 lane C measured 4 of 84.
'
FD_NEW='* 2026-09-05 lane C measured 10 of 84, superseding the line above.
'
run_merge docs/FINDINGS.md "$FD_HEAD$FD_OLD" "$FD_HEAD$FD_NEW" "$FD_HEAD$FD_OLD$FD_B"
check "case 29c: a replace-plus-append MERGES (exit 0)" "$(clean_rc)"
check "case 29c: no conflict markers" "$([ "$(conflicted "$MERGED_FILE")" = 0 ] && echo 1 || echo 0)"
check "case 29c: the replaced finding does not come back beside its replacement" \
    "$([ "$(countf "$MERGED_FILE" 'measured 4 of 84')" -le 1 ] && echo 0 || echo 1)"
check "case 29c: the replacement survives" "$(has "$MERGED_FILE" 'measured 10 of 84')"
check "case 29c: the unrelated append survives" "$(has "$MERGED_FILE" 'lane B found the second thing')"

# --- case 29d: --check sees the region ------------------------------------
printf '%s' "$FD_HEAD" > "$CHK/findings.md"
check "case 29d: --check accepts docs/FINDINGS.md's region" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/findings.md" >/dev/null 2>&1 && echo 0 || echo 1)"
grep -v 'MERGE-REGION' "$CHK/findings.md" > "$CHK/findings-noregion.md"
check "case 29d: --check REFUSES it once the marker is gone" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/findings-noregion.md" >/dev/null 2>&1 && echo 1 || echo 0)"

# --- case 30: ci/checks/test-targets.toml, the second-largest collision -----
#
# 193 of the 630 pairs of the 36 branches ahead of main that touch it, measured
# 2026-09-05. Its shape is `ci/t0.sh`'s in TOML: a new `[[bucket]]` lands beside
# the one it is topically closest to, not at a tail — the six hunks on
# `light-engine-2` start at lines 182, 250, 274, 287, 628 and 709 — so the
# region is the whole body and `check=toml-buckets` is what stops it unioning.

TT_HEAD='# test targets

[meta]
min_targets = 1

# MERGE-REGION BEGIN buckets to-eof check=toml-buckets

[[bucket]]
id = "t0-unit"
runs_in = "t0"
members = [
    "rustmc-server::lib",
]
'
TT_MID='
[[bucket]]
id = "t1-heavy"
runs_in = "t1"
members = [
    "rustmc-worldgen::heavy",
]
'
TT_A='
[[bucket]]
id = "t0-light"
runs_in = "t0"
members = [
    "rustmc-server::light",
]
'
TT_B='
[[bucket]]
id = "t1-advancements"
runs_in = "t1"
members = [
    "rustmc-server::advancements",
]
'
# Both sides insert BEFORE the trailing bucket, which is where real branches
# put theirs — an append at EOF would be resolved by the tail rule alone and
# would not exercise the region's width.
run_merge ci/checks/test-targets.toml "$TT_HEAD$TT_MID" \
    "$TT_HEAD$TT_A$TT_MID" "$TT_HEAD$TT_B$TT_MID"
check "case 30: two buckets added at the same mid-file anchor MERGE" "$(clean_rc)"
check "case 30: no conflict markers" "$([ "$(conflicted "$MERGED_FILE")" = 0 ] && echo 1 || echo 0)"
check "case 30: ours' bucket survives"   "$(has "$MERGED_FILE" 't0-light')"
check "case 30: theirs' bucket survives" "$(has "$MERGED_FILE" 't1-advancements')"
check "case 30: the trailing bucket is not duplicated" \
    "$([ "$(countf "$MERGED_FILE" 't1-heavy')" = 1 ] && echo 0 || echo 1)"
check "case 30: the merged ledger still parses as TOML" \
    "$(python3 -c 'import tomllib,sys; tomllib.load(open(sys.argv[1],"rb"))' "$MERGED_FILE" >/dev/null 2>&1 && echo 0 || echo 1)"

# --- case 30b: the post-condition — a duplicated bucket id REFUSES ----------
TT_DUP='
[[bucket]]
id = "t0-light"
runs_in = "t1"
members = [
    "rustmc-server::light_other",
]
'
run_merge ci/checks/test-targets.toml "$TT_HEAD$TT_MID" \
    "$TT_HEAD$TT_A$TT_MID" "$TT_HEAD$TT_DUP$TT_MID"
check "case 30b: two buckets claiming one id REFUSE" "$(refused_rc)"
check "case 30b: conflict markers are present"       "$(conflicted "$MERGED_FILE")"

# --- case 30c: a target listed in two buckets REFUSES ----------------------
#
# The sharper one, and the reason `toml-buckets` counts members as well as ids:
# two lanes can legitimately add differently-named buckets and still put the
# same cargo target in both, which makes the ledger say a target runs in two
# gates. `ci/check-test-targets.py` is entitled to assume that cannot happen.
TT_SAME='
[[bucket]]
id = "t1-light"
runs_in = "t1"
members = [
    "rustmc-server::light",
]
'
run_merge ci/checks/test-targets.toml "$TT_HEAD$TT_MID" \
    "$TT_HEAD$TT_A$TT_MID" "$TT_HEAD$TT_SAME$TT_MID"
check "case 30c: one target in two buckets REFUSES" "$(refused_rc)"
check "case 30c: conflict markers are present"      "$(conflicted "$MERGED_FILE")"

# --- case 30d: a real edit both sides make still conflicts -----------------
TT_E1="${TT_HEAD/min_targets = 1/min_targets = 2}"
TT_E2="${TT_HEAD/min_targets = 1/min_targets = 3}"
run_merge ci/checks/test-targets.toml "$TT_HEAD$TT_MID" "$TT_E1$TT_MID" "$TT_E2$TT_MID"
check "case 30d: two rewrites of one existing line still refuse" "$(refused_rc)"

# --- case 30e: --check sees the region -------------------------------------
printf '%s' "$TT_HEAD$TT_MID" > "$CHK/tt.toml"
check "case 30e: --check accepts test-targets.toml's region" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/tt.toml" >/dev/null 2>&1 && echo 0 || echo 1)"
grep -v 'MERGE-REGION' "$CHK/tt.toml" > "$CHK/tt-noregion.toml"
check "case 30e: --check REFUSES it once the marker is gone" \
    "$(python3 "$ROOT/ci/merge-docregions.py" --check "$CHK/tt-noregion.toml" >/dev/null 2>&1 && echo 1 || echo 0)"

if [ "$FAIL" -eq 0 ]; then
    # The counts are DERIVED, not written. This line said "29 cases, 89
    # assertions" for three weeks while the suite grew to 43 and 141 — the
    # hand-maintained-count defect `docs/AGENT-BRIEF.md` warns about, in the
    # file whose whole job is to stop a claim outliving its evidence.
    echo "MERGE-APPENDONLY-FALSIFY GREEN — $CASES cases, $OKS assertions: clean appends merge"
    echo "  silently on all three drivers, a rewritten row/section/figure/scenario/"
    echo "  baseline-row/check still conflicts, a duplicated key or section number is"
    echo "  refused by post-condition, ## NOW conflicts in both its rewritten and its"
    echo "  appended form, an edited [[scenario]]/check still conflicts beside an"
    echo "  unrelated append, and an append that breaks bash syntax is refused"
else
    echo "MERGE-APPENDONLY-FALSIFY RED"
fi
exit "$FAIL"
