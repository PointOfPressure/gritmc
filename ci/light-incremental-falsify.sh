#!/usr/bin/env bash
# light-incremental-falsify.sh — show that the DECREASE-queue comparison can
# go red, and record which injections the static suite cannot see.
#
# `ci/light-falsify.sh` does this for `tests/light_golden.rs`, the from-scratch
# comparison. This one exists because that suite is structurally blind to the
# half of the light engine this project added last: every case in it reaches
# the engine only through `propagateLightSources`, which enqueues nothing but
# increases, so `propagateDecreases` drains an empty queue and a port with no
# decrease queue at all passes all of it.
#
# That is not a guess and it is not a subset. **Every injection into the
# ENGINE below — cases 2 through 7 — is `static=pass, mutation=red`**,
# measured on the Threadripper 2026-09-05: the static comparison
# (`tests/light_incremental_golden.rs`, 6084/6084 sections against vanilla) is
# green for all of them, and the mutation comparison
# (`tests/light_incremental_mutation.rs`, 2340/2340 over five edited worlds) is
# red for all of them. Each case runs BOTH tests and asserts both verdicts, so
# this script is simultaneously the falsifier for the new checks and the
# standing evidence for the claim that the old one could not have caught any of
# them. (Case 8 injects the GOLDEN rather than the engine and its static
# verdict is `skip`, because emptying the mutation lists cannot move a
# comparison that never reads them.)
#
# Case 1 is the positive control on the unmodified tree. A suite of failures
# with no control cannot tell "the check is strict" from "the check is
# broken".
#
# **A patch that has stopped matching is a hole in the sweep**, so every
# injection is verified to have changed its file before anything is run, and a
# patch that does not apply is a failure of this script rather than a skipped
# case — `ci/corpus-lock-falsify.sh`'s rule, here for the same reason.
#
# `LIGHT_INCREMENTAL_FALSIFY_DRYRUN=1` applies every injection, confirms each
# still changes its file, and runs no cargo — seconds instead of minutes. That
# is the mode `ci/t0.sh` gates on; the full run is what a change to the engine
# earns.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

INC="crates/rustmc-server/src/light_incremental.rs"
GOLDEN="crates/rustmc-server/golden/light_vectors.json"
BACKUP="$(mktemp -d)"
cp "$INC" "$BACKUP/light_incremental.rs"
cp "$GOLDEN" "$BACKUP/light_vectors.json"
restore() {
    cp "$BACKUP/light_incremental.rs" "$INC"
    cp "$BACKUP/light_vectors.json" "$GOLDEN"
}
trap 'restore; rm -rf "$BACKUP"' EXIT

PASS=0
FAIL=0
DRYRUN="${LIGHT_INCREMENTAL_FALSIFY_DRYRUN:-0}"

run_mutation() {
    cargo test --release -p rustmc-server --test light_incremental_mutation \
        >/dev/null 2>&1
}
run_static() {
    cargo test --release -p rustmc-server --test light_incremental_golden \
        >/dev/null 2>&1
}

# $1 name, $2 wanted verdict of the MUTATION check, $3 wanted verdict of the
# STATIC check (pass|red|skip).
verdict() {
    local name="$1" want_mut="$2" want_static="$3" got_mut got_static
    if [ "$DRYRUN" = "1" ]; then
        printf 'ok    %-44s injection applies (dry run)\n' "$name"
        PASS=$((PASS + 1))
        restore
        return
    fi
    if run_mutation; then got_mut=pass; else got_mut=red; fi
    if [ "$want_static" = "skip" ]; then
        got_static=skip
    elif run_static; then
        got_static=pass
    else
        got_static=red
    fi
    if [ "$got_mut" = "$want_mut" ] && [ "$got_static" = "$want_static" ]; then
        printf 'ok    %-44s mutation=%-4s static=%s\n' "$name" "$got_mut" "$got_static"
        PASS=$((PASS + 1))
    else
        printf 'FAIL  %-44s wanted mutation=%s static=%s, got %s / %s\n' \
            "$name" "$want_mut" "$want_static" "$got_mut" "$got_static"
        FAIL=$((FAIL + 1))
    fi
    restore
}

patch_file() {
    local file="$1" expr="$2" name="$3" before after
    before="$(md5sum "$file" | cut -d' ' -f1)"
    sed -i "$expr" "$file"
    after="$(md5sum "$file" | cut -d' ' -f1)"
    if [ "$before" = "$after" ]; then
        printf 'FAIL  %-44s injection did not apply — the sweep has a hole\n' "$name"
        FAIL=$((FAIL + 1))
        return 1
    fi
    return 0
}

echo "== light-incremental-falsify: 8 cases"

# 1. Positive control.
if [ "$DRYRUN" = "1" ]; then
    printf 'ok    %-44s no injection to apply\n' "1 positive control (unmodified tree)"
    PASS=$((PASS + 1))
else
    verdict "1 positive control (unmodified tree)" pass pass
fi

# 2. runLightUpdates runs propagateDecreases BEFORE propagateIncreases. Swap
#    them and an increase places light that a still-queued decrease then
#    erases. Invisible to the static suite, which never queues a decrease.
if patch_file "$INC" \
    's|        n += self.propagate_decreases();|        n += self.propagate_increases();|' \
    "2 decreases after increases"; then
    verdict "2 decreases after increases" red pass
fi

# 3. checkNode's decrease arm zeroes the position and enqueues a decrease
#    carrying the level that WAS there. Enqueue PULL_LIGHT_IN_ENTRY instead
#    and a removed source leaves its light behind.
if patch_file "$INC" \
    's|            self.enqueue_decrease(p, QueueEntry::decrease_all_directions(stored));|            self.enqueue_decrease(p, PULL_LIGHT_IN_ENTRY);|' \
    "3 checkNode never carries the old level"; then
    verdict "3 checkNode never carries the old level" red pass
fi

# 4. propagateDecrease's else arm re-queues a brighter neighbour as an
#    increase in exactly ONE direction. Drop that and the light that survived
#    a removal never flows back into the hole.
if patch_file "$INC" \
    's|QueueEntry::increase_only_one_direction(n_stored, false, d.opposite()),|QueueEntry::increase_only_one_direction(0, false, d.opposite()),|' \
    "4 decrease never refills from a survivor"; then
    verdict "4 decrease never refills from a survivor" red pass
fi

# 5. DataLayer materialisation across edits. Read emptiness off the values
#    instead of off the write history and the `relit` case — a room lit and
#    then darkened — reports an empty section where vanilla sends 2048 zero
#    bytes.
if patch_file "$INC" \
    's|let uniform = !engine.storage.materialised.contains(&(cx, sy, cz));|let uniform = vals.iter().all(\|\&v\| v == first);|' \
    "5 emptiness from values, not history"; then
    verdict "5 emptiness from values, not history" red pass
fi

# 6. SkyLightEngine.removeSourcesBelow is the only thing that takes a column
#    of 15s away when a block seals the sky. Make it return immediately and
#    `sky_sealed` must notice.
if patch_file "$INC" \
    's|        if lowest <= bottom {|        if true \|\| lowest <= bottom {|' \
    "6 removeSourcesBelow does nothing"; then
    verdict "6 removeSourcesBelow does nothing" red pass
fi

# 7. The sky sources table must be RECOMPUTED when the world changes —
#    ChunkSkyLightSources.update runs on every LevelChunk.setBlockState. Keep
#    the pre-edit table and checkNode reads the old lowest source, which is
#    exactly the defect this suite found in the golden's own mutator.
if patch_file "$INC" \
    's|        if self.layer == Layer::Sky {|        if false \&\& self.layer == Layer::Sky {|' \
    "7 stale sky sources after an edit"; then
    verdict "7 stale sky sources after an edit" red pass
fi

# 8. The instrument's own vacuity guard. Empty every mutation list in the
#    golden and the mutation test must REFUSE — a suite that lost its edits
#    would otherwise report a green over zero cases.
if patch_file "$GOLDEN" \
    's|"mutations": \[{[^]]*\]|"mutations": []|g' \
    "8 golden loses its recorded edits"; then
    verdict "8 golden loses its recorded edits" red skip
fi

echo
if [ "$DRYRUN" = "1" ]; then
    echo "light-incremental-falsify (dry run): $PASS injections apply, $FAIL do not"
else
    echo "light-incremental-falsify: $PASS passed, $FAIL failed"
fi
[ "$FAIL" -eq 0 ]
