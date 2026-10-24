#!/usr/bin/env bash
# light-falsify.sh — show that the section-light comparison can go red.
#
# `crates/rustmc-server/tests/light_golden.rs` reports 3276/3276 sections
# agreeing with vanilla. That sentence is worth nothing on its own: this
# project's central defect is a check whose green cannot be turned red, and it
# has shipped that defect at least five times. So every rule the differ is
# supposed to be enforcing gets broken here, one at a time, and the differ has
# to notice.
#
# Case 1 is the positive control — the unmodified tree, which must PASS. A
# suite of failures with no control cannot tell "the check is strict" from
# "the check is broken".
#
# **A patch that has stopped matching is a hole in the sweep**, so every
# injection is verified to have changed the file before the test is run, and a
# patch that does not apply is a failure of this script rather than a skipped
# case. That rule is `ci/corpus-lock-falsify.sh`'s and it is here for the same
# reason.
#
# Runs in the tree it is invoked from, patching and reverting in place. About
# five minutes on the Threadripper, most of it incremental rebuilds.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LIGHT="crates/rustmc-server/src/light.rs"
ENGINE="crates/rustmc-server/src/light_engine.rs"
GOLDEN="crates/rustmc-server/golden/light_vectors.json"
BACKUP="$(mktemp -d)"
cp "$LIGHT" "$BACKUP/light.rs"
cp "$ENGINE" "$BACKUP/light_engine.rs"
cp "$GOLDEN" "$BACKUP/light_vectors.json"
restore() {
    cp "$BACKUP/light.rs" "$LIGHT"
    cp "$BACKUP/light_engine.rs" "$ENGINE"
    cp "$BACKUP/light_vectors.json" "$GOLDEN"
}
trap 'restore; rm -rf "$BACKUP"' EXIT

PASS=0
FAIL=0

# `LIGHT_FALSIFY_DRYRUN=1` applies every injection, confirms each one still
# changes its file, and runs no cargo at all — seconds instead of minutes.
#
# It exists for the same reason `CORPUS_LOCK_FALSIFY_DRYRUN` does. The way a
# suite like this rots is not that a case starts giving the wrong verdict; it
# is that a refactor moves the line a `sed` was matching, the injection
# silently applies to nothing, and the case then reports "red" for a tree that
# was never patched — a green that means the opposite of what it says. That
# failure is invisible to the full run and obvious to this one, so this is the
# mode `ci/t0.sh` gates on and the full suite is what a change to the engine
# earns.
DRYRUN="${LIGHT_FALSIFY_DRYRUN:-0}"

run_test() {
    cargo test --release -p rustmc-server --test light_golden >/dev/null 2>&1
}

# $1 name, $2 expected verdict (pass|red)
verdict() {
    local name="$1" want="$2" got
    if [ "$DRYRUN" = "1" ]; then
        # The injection already proved it applies; in dry run that IS the case.
        printf 'ok    %-46s injection applies (dry run)\n' "$name"
        PASS=$((PASS + 1))
        restore
        return
    fi
    if run_test; then got=pass; else got=red; fi
    if [ "$got" = "$want" ]; then
        printf 'ok    %-46s %s\n' "$name" "$got"
        PASS=$((PASS + 1))
    else
        printf 'FAIL  %-46s wanted %s, got %s\n' "$name" "$want" "$got"
        FAIL=$((FAIL + 1))
    fi
    restore
}

# Apply a sed edit and refuse to continue if it changed nothing.
patch_file() {
    local file="$1" expr="$2" name="$3" before after
    before="$(md5sum "$file" | cut -d' ' -f1)"
    sed -i "$expr" "$file"
    after="$(md5sum "$file" | cut -d' ' -f1)"
    if [ "$before" = "$after" ]; then
        printf 'FAIL  %-46s injection did not apply — the sweep has a hole\n' "$name"
        FAIL=$((FAIL + 1))
        return 1
    fi
    return 0
}

echo "== light-falsify: 8 cases"

# 1. Positive control. The unmodified tree must pass, or every case below is
#    measuring a broken build rather than a broken rule.
if [ "$DRYRUN" = "1" ]; then
    printf 'ok    %-46s no injection to apply\n' "1 positive control (unmodified tree)"
    PASS=$((PASS + 1))
else
    verdict "1 positive control (unmodified tree)" pass
fi

# 2. The mask bit is a dense index; the storage lookup is at
#    getMinLightSection() + i. Conflate them and every bit moves five places
#    in the overworld — and nowhere at all in a dimension whose minSectionY
#    is 0, which is why this needs a test and not a reading.
if patch_file "$LIGHT" \
    's|self.min_light_section() + i|i|' \
    "2 mask bit conflated with section Y"; then
    verdict "2 mask bit conflated with section Y" red
fi

# 3. The stored-section rule is a 3x3x3 cube in SECTION space and therefore
#    crosses chunk borders. Restrict it to the chunk's own column and the
#    `tower` case must notice.
if patch_file "$ENGINE" \
    's|for dx in -1..=1 {|for dx in 0..=0 {|; s|for dz in -1..=1 {|for dz in 0..=0 {|' \
    "3 occupancy per-column instead of 3D"; then
    verdict "3 occupancy per-column instead of 3D" red
fi

# 4. propagateIncrease refuses a neighbour outside the stored set. Let light
#    leak into unstored sections and the values change.
if patch_file "$ENGINE" \
    's|if !stored.contains(&(ncx, ncz, nsy)) {|if false \&\& !stored.contains(\&(ncx, ncz, nsy)) {|' \
    "4 light leaks into unstored sections"; then
    verdict "4 light leaks into unstored sections" red
fi

# 5. LightEngine.getOpacity is max(1, dampening). Drop the max and a
#    transparent block costs nothing, so light stops attenuating through air
#    and every gradient goes flat.
#
#    Written as `.max(0)` rather than by deleting the line, because sed
#    cannot span lines and a deletion that silently matched nothing is the
#    hole this script's own patch_file guard exists to catch.
if patch_file "$ENGINE" \
    's|^            .max(1)$|            .max(0)|' \
    "5 opacity without the max(1)"; then
    verdict "5 opacity without the max(1)" red
fi

# 6. DataLayer.isEmpty() is about the representation. Make it a content scan
#    and a homogeneous-15 sky section — superflat has two per chunk — stops
#    being distinguishable from an absent one.
if patch_file "$LIGHT" \
    's|        self.data.is_none() \&\& self.default_value == 0|        self.data.is_none()|' \
    "6 isEmpty ignores defaultValue"; then
    verdict "6 isEmpty ignores defaultValue" red
fi

# 7. BitSet.toLongArray() trims trailing zero words, so an all-clear mask is a
#    zero-length array. Stop trimming and every mask gains a word.
if patch_file "$LIGHT" \
    's|    while words.last() == Some(\&0) {|    while false \&\& words.last() == Some(\&0) {|' \
    "7 mask keeps its trailing zero words"; then
    verdict "7 mask keeps its trailing zero words" red
fi

# 8. The registered history divergence must refuse to be vacuous. Strip the
#    from_scratch_control link out of the golden and the test has to say the
#    divergence is no longer covered, rather than passing with one case fewer.
if patch_file "$GOLDEN" \
    's|"from_scratch_control": "sealed_dark"|"from_scratch_control": null|' \
    "8 history divergence loses its control"; then
    verdict "8 history divergence loses its control" red
fi

echo
if [ "$DRYRUN" = "1" ]; then
    echo "light-falsify (dry run): $PASS injections apply, $FAIL do not"
else
    echo "light-falsify: $PASS passed, $FAIL failed"
fi
[ "$FAIL" -eq 0 ]
