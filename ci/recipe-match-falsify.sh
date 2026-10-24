#!/usr/bin/env bash
# recipe-match-falsify.sh — can the recipe golden catch the defects it was
# written to catch?
#
# `docs/AGENT-BRIEF.md`'s hard rule: **a guard that cannot be shown catching a
# real past instance is itself a check that cannot fail.** Five of the six
# mutations below are defects this repository actually shipped, reconstructed
# in the source they shipped in; the sixth is the one the golden was built for
# and which no unit test could reach. Each is applied to
# `crates/rustmc-server/src/recipes.rs`, the recipe gates are run, and the run
# is required to go RED. "The recipe gates" is three targets, not one, and the
# breadth is not padding: `transmute: material_count` survives
# `recipe_match_golden` alone, because that file's grids come from
# `PlacementInfo` and a `material_count` of `exactly(1)` produces a two-slot
# grid — the golden never presents two materials, so it cannot see a bound read
# as `1..8`. `recipes_table::a_transmute_refuses_a_no_op_and_any_foreign_item`
# does. A suite run against the golden alone would have reported that mutation
# as SURVIVED and been right about it. Then the file is restored and the test is required to go
# GREEN, so a suite that reds because the tree is broken cannot pass either.
#
# Run it after touching `recipes.rs`, `RecipeMatchGolden.java` or the tests. It
# needs `data/vanilla/recipes.json`, `tags_item.json` and `recipe_match.json`,
# takes about six minutes, and needs neither the box nor any lock.
#
# It is deliberately NOT in `ci/t0.sh`: it edits a tracked source file and
# restores it, which is not a thing a gate should do to a tree an agent may be
# working in. The `--dry-run` half is what belongs in a habit.
#
# Result on 2026-09-05, sayer-desktop: caught 6, survived 0, unmatched 0.
#
#   RECIPE_FALSIFY_DRYRUN=1   apply and revert every patch without running the
#                             test, in seconds. Run this after editing
#                             `recipes.rs` at all: a patch that has stopped
#                             MATCHING is a hole in the sweep whether or not you
#                             have four minutes, and it is silent — sed reports
#                             success for a pattern that matched nothing.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/crates/rustmc-server/src/recipes.rs"
BAK="$(mktemp)"
cp "$SRC" "$BAK"
restore() { cp "$BAK" "$SRC"; rm -f "$BAK"; }
trap restore EXIT

export PATH="$HOME/.cargo/bin:$PATH"
DRY="${RECIPE_FALSIFY_DRYRUN:-0}"
CAUGHT=0; SURVIVED=0; UNMATCHED=0

# run_test -> 0 green, non-zero red
run_test() {
    # NO NAME FILTER. A trailing `recipes` here reads as a test-name filter
    # applied to EVERY target, and `our_matcher_agrees_with_vanillas_own_answers`
    # does not contain that string — so the golden silently did not run and four
    # mutations reported SURVIVED. A filter that quietly empties a target is the
    # same defect this suite exists to catch, found inside the suite itself.
    ( cd "$ROOT" && cargo test -p rustmc-server --lib \
        --test recipe_match_golden --test recipes_table ) >/dev/null 2>&1
}

# case <name> <python-mutation>
#
# The mutation is a python snippet with `s` bound to the file text; it must
# CHANGE it. A snippet that leaves the text alone is reported as UNMATCHED and
# fails the suite: that is the failure mode a patch set decays into, and it
# looks exactly like a passing case.
case_() {
    local name="$1" mutation="$2"
    cp "$BAK" "$SRC"
    python3 - "$SRC" <<PY
import sys
p = sys.argv[1]
s = open(p).read()
before = s
$mutation
if s == before:
    sys.exit(3)
open(p, 'w').write(s)
PY
    local rc=$?
    if [ "$rc" -eq 3 ]; then
        echo "UNMATCHED  $name — the patch no longer matches the source; the sweep has a hole"
        UNMATCHED=$((UNMATCHED + 1))
        return
    fi
    [ "$rc" -eq 0 ] || { echo "ERROR      $name — the patch script failed ($rc)"; UNMATCHED=$((UNMATCHED+1)); return; }
    if [ "$DRY" = "1" ]; then
        echo "patched    $name"
        return
    fi
    if run_test; then
        echo "SURVIVED   $name — the golden did not notice"
        SURVIVED=$((SURVIVED + 1))
    else
        echo "caught     $name"
        CAUGHT=$((CAUGHT + 1))
    fi
}

echo "recipe-match-falsify: six mutations against $SRC"

# 1. SHIPPED ON MAIN. `ShapedRecipePattern.shrink` missing, so a pattern written
#    with blank border columns never matched: seven recipes uncraftable.
case_ "shrink: patterns are not trimmed to their bounding box" \
  "s = s.replace('let rows = shrink(&raw);', 'let rows: Vec<String> = raw.iter().map(|r| (*r).to_string()).collect();', 1)"

# 2. SHIPPED ON MAIN. A list-form ingredient dropped from a shapeless recipe's
#    ingredient list rather than dropping the recipe — `dye_black_bed` crafted a
#    bed from one dye.
case_ "ingredients: an unreadable entry is filtered out, not refused" \
  "s = s.replace('.map(Ingredient::parse_value)\n                        .collect::<Option<Vec<_>>>()', '.filter_map(|v| v.as_str().map(Ingredient::parse))\n                        .collect::<Vec<_>>()\n                        .into()', 1)"

# 3. SHIPPED ON MAIN. Nested `#tag` references not followed, so
#    `#minecraft:trimmable_armor` resolved to the empty set.
case_ "tags: nested references are not flattened" \
  "s = s.replace('Ok(Self::flatten(&out))', 'Ok(Self(out))', 1)"

# 4. SHIPPED ON MAIN. `material_count` read as an open range instead of
#    `exactly(1)`, so a bundle could be dyed with up to eight dyes.
case_ "transmute: material_count defaults to an open range" \
  "s = s.replace('(Some(1), Some(1)),', '(None, None),', 1)"

# 5. THE ONE NO UNIT TEST CAN REACH. `getRecipeFor`'s \`lastRecipe\` argument
#    ignored — reads as a dropped cache, changes 256 answers.
#    Both entry points, because most of the 256 vectors are stonecutting: one
#    input cuts to many results, so it is the one-slot lookup that carries the
#    argument most of the time. Stubbing only the crafting one SURVIVES, which
#    is how the shape of this suite was settled.
case_ "getRecipeFor: the lastRecipe branch is skipped" \
  "s = s.replace('        if let Some(last) = last {', '        let last: Option<&str> = if true { None } else { last };\n        if let Some(last) = last {')"

# 6. THE TIE-BREAK ITSELF. The table loads in the right ORDER by construction
#    (a BTreeMap), so this reverses the walk instead: every recipe is still
#    present and correct, and only which of two matching ones wins moves. That
#    is the failure with no local symptom, and the reason this file exists.
#    The table still holds every recipe and every one of them is still right;
#    only which of two matching ones wins moves. That is the failure with no
#    local symptom, and the reason the golden exists.
case_ "tie-break: the whole table loads in reverse id order" \
  "s = s.replace('        Ok(Self { recipes: out, tags })', '        out.reverse();\n        Ok(Self { recipes: out, tags })', 1)"

restore
trap - EXIT

if [ "$DRY" = "1" ]; then
    echo "recipe-match-falsify: DRY RUN — $UNMATCHED patches no longer match"
    [ "$UNMATCHED" -eq 0 ] || exit 1
    exit 0
fi

# THE REPAIR. Without this a suite that reds because the tree is broken passes
# every case and reports six catches.
if run_test; then
    echo "repaired   the restored source is GREEN"
else
    echo "BROKEN     the restored source is RED — every 'caught' above is meaningless"
    exit 1
fi

echo "recipe-match-falsify: caught $CAUGHT, survived $SURVIVED, unmatched $UNMATCHED"
[ "$SURVIVED" -eq 0 ] && [ "$UNMATCHED" -eq 0 ] || exit 1
echo "RECIPE-MATCH-FALSIFY GREEN"
