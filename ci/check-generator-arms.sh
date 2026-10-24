#!/usr/bin/env bash
# check-generator-arms.sh — a generator nothing asks for is a table nobody makes.
#
#     ./ci/check-generator-arms.sh        # T0; ~0.2 s, no jar, no build, no box
#
# # THE DEFECT, AND IT COST A GATE ON 2026-09-05
#
# `ci/gen-gameplay-tables.sh` derives the tables a tree must generate by
# grepping `crates tools plugins` for the quoted literal
# `"data/vanilla/<name>.(json|bin)"`, then refuses any derived table with no
# `generator_for()` arm. It never asked the other direction, and the other
# direction is the one that fails silently.
#
# `block-entities-2` added a correct arm —
# `block_entities.json) echo ci/block-entity-golden.sh` — and wrote its load
# site as `.join("../../data/vanilla/block_entities.json")`, whose literal
# begins `"../../` and so matches nothing. The arm was reachable from no
# table, `gen-gameplay-tables.sh` printed `21 tables current` and exit 0,
# `ci/block-entity-golden.sh` never ran in any gate directory, and three tests
# panicked `missing fixture` — T0 exit 101 — on a branch whose own tree had
# generated the table by hand and was green.
#
# The forward check cannot see this: it iterates the derived set, and the
# table is not in it. Nothing else can either, because the arm is live bash
# that no test calls.
#
# # WHY THIS IS ITS OWN SCRIPT AND NOT A BLOCK INSIDE THAT ONE
#
# It was written inside `gen-gameplay-tables.sh` first, and `ci/gen-tables-
# falsify.sh` went red on ten of its twenty-two cases. That falsifier builds
# hermetic fixture trees holding a handful of synthetic table references, so
# in a fixture *every* arm is unreached and the guard aborted the run the
# falsifier was measuring. The property is about the real repository, not
# about any tree this script happens to be pointed at, so it belongs in T0
# where the real repository is what runs.
#
# # NO SECOND COPY OF THE GREP
#
# The required set comes from `./ci/gen-gameplay-tables.sh --list-tables`,
# which is that script's own derivation. Re-implementing the grep here would
# be two derivations of one fact, and the second would go stale the first time
# the pattern was widened — which has happened twice already (scan root
# 2026-08-07, character class the same day).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GEN="$ROOT/ci/gen-gameplay-tables.sh"

# Basenames whose arm is deliberately reached by nothing yet. `use_item_on.json`
# is why the list exists rather than the check being a bare error: its arm was
# written dormant on 2026-08-06, before `crate::useitemon` named the path, so
# that no tree paid ~95 s for a table nothing read. That was a reviewed
# decision and this check would have refused it. An arm nobody declared dormant
# is a mistake; this list is the difference between the two.
#
# ONE LINE PER ENTRY, each with the reason on the line above it. The array was
# a single `()` and two lanes appending to one line is a conflict for no
# reason; more to the point, an entry with no stated reason is indistinguishable
# from an entry somebody added to get green.
DORMANT_GENERATORS=(
    # (`tags_block.json` sat here from 2026-09-05, `gate-instruments`, until
    # `crate::blocktags` gave it its first reader on `diverge-burndown` —
    # `WallBlock.connectsTo`'s `state.is(BlockTags.WALLS)` — and came off in
    # that commit, as this entry said it would.)
    # `ci/shape-three.sh`, ~20 minutes. Dormant BY DESIGN and argued at length
    # in `ci/gen-gameplay-tables.sh` under "WHO REQUIRED THE EXPENSIVE ONES":
    # the deriving grep reads a quoted path and does not care whether the
    # quotes are around a `File::open` or around a sentence in a comment, so
    # an accidental mention would put a twenty-minute generator in every
    # tree's setup with none of the intent. `ShapeGolden.java` already names
    # this table in backticks, one character from the quoted form. The arm is
    # here so the first real consumer is a one-line change; until there is
    # one, an unreached arm is the correct state and not a mistake.
    #
    # This entry is the whole reason this check has a list rather than being a
    # bare error, and `use_item_on.json` — dormant for six days in 2026-08 for
    # the same reason, now required by `crate::useitemon` — is the precedent.
    update_shape3.json
)

mapfile -t TABLES < <("$GEN" --list-tables)
[ "${#TABLES[@]}" -gt 0 ] || {
    echo "GENERATOR-ARMS RED — ci/gen-gameplay-tables.sh --list-tables printed nothing" >&2
    exit 2
}

# `declare -f` reads the arms out of the PARSED function, so a comment or a
# doc line holding the string `foo.json)` cannot be miscounted as an arm.
mapfile -t ARMS < <(bash -c 'source <(sed -n "/^generator_for()/,/^}/p" "$0"); declare -f generator_for' "$GEN" \
                    | grep -oE '^[[:space:]]+[A-Za-z0-9_]+\.(json|bin)\)' | tr -d ' )' | sort -u)
[ "${#ARMS[@]}" -gt 0 ] || {
    echo "GENERATOR-ARMS RED — read no generator_for arms; the extraction has gone stale" >&2
    exit 2
}

UNREACHED=()
for a in "${ARMS[@]}"; do
    for t in "${TABLES[@]}"; do [ "$(basename "$t")" = "$a" ] && continue 2; done
    for d in ${DORMANT_GENERATORS[@]+"${DORMANT_GENERATORS[@]}"}; do [ "$d" = "$a" ] && continue 2; done
    UNREACHED+=("$a")
done

# ===== A DORMANT DECLARATION THAT HAS COME TRUE IS ALSO A RED — 2026-09-05 ===
#
# The list above is an exemption, and an exemption nobody re-checks is how a
# comment ends up describing a state the tree left months ago. Every entry
# says "nothing reads this yet". The moment something does, that sentence is
# false, the table is in the derived set and generated by every tree anyway,
# and the entry is doing nothing but standing ready to hide the NEXT arm
# somebody forgets to wire. So the same run that refuses an undeclared dormant
# arm refuses a declared one that has stopped being dormant.
#
# This is not hypothetical for `use_item_on.json`: its arm was dormant from
# 2026-08-06 until `crate::useitemon` named the path on 2026-08-12, and had
# this list existed then, nothing would have asked for it back.
STALE_DORMANT=()
for d in ${DORMANT_GENERATORS[@]+"${DORMANT_GENERATORS[@]}"}; do
    for t in "${TABLES[@]}"; do
        if [ "$(basename "$t")" = "$d" ]; then STALE_DORMANT+=("$d"); break; fi
    done
done
if [ "${#STALE_DORMANT[@]}" -gt 0 ]; then
    echo "GENERATOR-ARMS RED — ${#STALE_DORMANT[@]} DORMANT_GENERATORS entr(ies) are no longer dormant:" >&2
    for d in "${STALE_DORMANT[@]}"; do
        echo "  !! $d — a source literal under crates/ tools/ plugins/ now names \"data/vanilla/$d\"," >&2
        echo "     so this table IS in the derived set and every tree generates it." >&2
    done
    echo "    The exemption has come true and the comment beside it is now false." >&2
    echo "    Delete the entry. An exemption kept past its reason is one that will" >&2
    echo "    silently cover the next arm nobody wired." >&2
    exit 1
fi

if [ "${#UNREACHED[@]}" -gt 0 ]; then
    echo "GENERATOR-ARMS RED — ${#UNREACHED[@]} generator_for arm(s) no source literal reaches:" >&2
    for a in "${UNREACHED[@]}"; do
        echo "  !! $a — nothing under crates/ tools/ plugins/ names \"data/vanilla/$a\"" >&2
    done
    cat >&2 <<'MSG'
    A table nobody asks for is never generated, and the tests that read it
    panic on a missing fixture in a gate directory while the tree that wrote
    it by hand stays green. Spell the path as one quoted "data/vanilla/x.json"
    literal — a `concat!` or a `.join("../../data/vanilla/x.json")` is
    invisible to the deriver — or add the basename to DORMANT_GENERATORS in
    this file with a comment saying why.
MSG
    exit 1
fi

echo "GENERATOR-ARMS GREEN — ${#ARMS[@]} generator_for arms, ${#TABLES[@]} derived tables," \
     "${#DORMANT_GENERATORS[@]} declared dormant and still dormant, every other arm reached"
