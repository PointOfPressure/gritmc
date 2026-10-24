#!/usr/bin/env bash
# ===== AGENT INTERFACE =====
#
#     ./ci/entity-reach-falsify.sh        # ~1s, no network, no server, no lock
#
# Shows `ci/entity-reach.py --check` going RED on each way its ledger can stop
# describing the tree, and green again when the mutation is reverted.
# ===== end interface; everything below is why =====
#
# `docs/AGENT-BRIEF.md`'s hard rule: a check that has only ever been seen green
# is not evidence. `ci/entity-reach.py` makes a claim of its own — that
# `ci/checks/entity-reach.toml` still names the entity types the corpus puts in
# the world — and that claim needs the same treatment as a scenario's.
#
# FIVE MUTATIONS. THE THIRD IS THE ONE THE INVENTORY EXISTS FOR AND THE FIFTH
# IS THE ONE THE `residue` COLUMN EXISTS FOR.
#
#   1. a row names a type the jar does not have — somebody typed it, or the
#      registry moved under a version bump;
#   2. a row names a check that has been retired or renamed — the ledger keeps
#      asserting a spawn nothing performs any more;
#   3. **a check observes `add_entity` and no row names it.** This is the live
#      one. It is not a typo class: it is what happens when the next agent adds
#      an entity check and the inventory silently stops being the inventory.
#      `ci/checks/projection-drops.toml` exists for the identical failure one
#      layer down, where the thing nobody wrote down was a dropped field;
#   4. a row does not say what its check leaves in the world;
#   5. **a row says `walks` and gives no reason.** The class alone is the cheap
#      half. A declaration the next reader cannot check is how "the coupling is
#      bounded, 21 to 26 blocks, five blocks of spare" survived for a week at
#      N = 2 and then turned out to be the width of one run's random walk
#      (`ci/checks/gameplay/README.md`). The reason has to carry a measurement
#      and an N, and this case is what stops it being optional.
#
# Everything happens in a `mktemp -d` copy. Nothing here writes to the tree,
# which is why it is safe beside the check in T0.
set -u
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
ROOT=$PWD
fail=0
pass=0

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# A copy the mutations can be made in. `data/` is symlinked rather than copied:
# it is jar-derived, large, and nothing here mutates it.
mkdir -p "$work/ci" "$work/data"
cp -r "$ROOT/ci/checks" "$work/ci/checks"
cp "$ROOT/ci/entity-reach.py" "$work/ci/entity-reach.py"
if [ -e "$ROOT/data/vanilla" ]; then
    ln -sfn "$(readlink -f "$ROOT/data/vanilla")" "$work/data/vanilla"
else
    # **THIS SKIP IS ALLOWED AND THE SIBLING'S WAS NOT, AND THE DIFFERENCE IS
    # WHETHER A GATE CAN REACH IT.** `ci/floor-stamp-vocabulary-falsify.sh`
    # skipped on recordings that happened to be on the box, which made its
    # coverage a function of leftover working state — §50.5's "conditionally
    # armed, and nothing says so" — and it was rebuilt on synthetic fixtures
    # for that reason.
    #
    # This one skips on `data/vanilla`, and `ci/t0.sh` REDS on that same
    # condition in the default `RUSTMC_FIXTURES=required` mode. So the skip is
    # unreachable from a gate: the only way here is `optional`, which already
    # prints a different final line so that nothing can read the run as a
    # result. The registry cannot be synthesised anyway without synthesising
    # the 158 types the committed ledger names, at which point the suite would
    # be testing its own fixture.
    echo "SKIP: data/vanilla absent — run ./ci/gen-gameplay-tables.sh."
    echo "      Refusing to report a green from a tree that could not read the registry."
    echo "      ci/t0.sh reds on this in RUSTMC_FIXTURES=required, so a gate cannot get here."
    exit 0
fi

expect () {   # name  want_rc  want_substring
    local name=$1 want=$2 needle=$3
    local out rc
    out=$(cd "$work" && python3 ci/entity-reach.py --check 2>&1); rc=$?
    if [ "$rc" != "$want" ]; then
        echo "  FAIL $name: exit $rc, wanted $want"; fail=$((fail+1)); return
    fi
    if ! printf '%s' "$out" | grep -c -- "$needle" >/dev/null; then
        echo "  FAIL $name: output did not carry '$needle'"; fail=$((fail+1)); return
    fi
    echo "  ok   $name"; pass=$((pass+1))
}

L=$work/ci/checks/entity-reach.toml

echo "control, before any mutation"
expect "0 the committed ledger still describes the tree" 0 "still as tagged"

echo "case 1 — a row names a type the jar's registry does not have"
cp "$L" "$L.bak"
sed -i 's|^type = "minecraft:iron_golem"|type = "minecraft:iron_gollem"|' "$L"
expect "1 UNKNOWN TYPE" 1 "UNKNOWN TYPE"
cp "$L.bak" "$L"
expect "1r reverted" 0 "still as tagged"

echo "case 2 — a row names a check that is not in the corpus"
sed -i 's|^check = "a-minecart-sits-on-a-rail"|check = "a-minecart-sits-on-a-monorail"|' "$L"
expect "2 VANISHED CHECK" 1 "VANISHED CHECK"
cp "$L.bak" "$L"
expect "2r reverted" 0 "still as tagged"

echo "case 3 — a check observes add_entity and the ledger does not name it"
# The live case. Written as a NEW check rather than by deleting a row, because
# deleting a row is case 2 wearing a different hat: what has to be caught is
# the corpus growing, not the ledger shrinking.
cat >"$work/ci/checks/gameplay/799-a-fabricated-entity-check.toml" <<'TOML'
name = "a-fabricated-entity-check"
about = "exists for ci/entity-reach-falsify.sh case 3 and is deleted before it returns"
observe = ["add_entity"]

[[step]]
do = "place"
on = [1, -1, 0]
face = "up"
TOML
expect "3 UNDECLARED" 1 "UNDECLARED"
rm -f "$work/ci/checks/gameplay/799-a-fabricated-entity-check.toml"
expect "3r reverted" 0 "still as tagged"

echo "case 4 — a row does not say what the check leaves in the world"
sed -i '0,/^residue = "walks"/s//residue = "vanishes"/' "$L"
expect "4 NO RESIDUE" 1 "NO RESIDUE"
cp "$L.bak" "$L"
expect "4r reverted" 0 "still as tagged"

echo "case 5 — a residue class with no reason behind it"
# The class alone is the cheap half. A `walks` with an empty `residue_why` is
# a declaration that the next reader cannot check and cannot re-measure, which
# is how "the coupling is bounded" survived at N = 2 for a week.
sed -i '0,/^residue_why = /s/^residue_why = .*/residue_why = ""/' "$L"
expect "5 BARE RESIDUE" 1 "BARE RESIDUE"
cp "$L.bak" "$L"
expect "5r reverted" 0 "still as tagged"

echo
echo "entity-reach-falsify: $pass passed, $fail failed"
[ "$fail" = 0 ] || exit 1
