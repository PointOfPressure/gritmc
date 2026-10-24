#!/usr/bin/env bash
# gameplay-difficulty.sh — the ONE place the gameplay corpus chooses a
# difficulty, and the proof that both servers were actually told it.
#
# Sourced by `ci/t2-gameplay.sh` and `ci/t2-gameplay-subset.sh`. Sets:
#
#   GAMEPLAY_DIFFICULTY        peaceful | easy | normal | hard  (default peaceful)
#   DIFFICULTY_ARGS            the argv to hand rustmc-server, possibly empty
#
# and prints one line naming what each side received. Call `difficulty_or_die`
# after sourcing; it refuses, non-zero, when the two sides cannot be matched.
#
# # WHY THIS FILE EXISTS — the two servers were on different difficulties
#
# Measured 2026-09-05. `ci/t2-gameplay.sh` booted vanilla with
# `difficulty=peaceful` in `server.properties` and booted this server with no
# difficulty argument at all, so the subject ran at whatever its own default
# was — `easy`, vanilla's `server.properties` default, on every branch that has
# a difficulty at all. The launch line said nothing about it, and nothing in
# the harness read it back.
#
# **What that costs is the whole monster half of the corpus, and it cost it
# silently.** From the bytecode, at `PEACEFUL`:
#
#   * `SpawnEggItem.useOn` offsets 21-33 — `getType(stack)` then
#     `EntityType.canSpawn(level)`, and on `false` it returns
#     `InteractionResult.FAIL` and `areturn`s **before it has read the clicked
#     position, the clicked face or the block state**. `EntityType.canSpawn`
#     (offsets 13-31) is `isAllowedInPeaceful() || getDifficulty() != PEACEFUL`.
#     So a creeper spawn egg used on grass creates nothing on vanilla: no
#     entity, no `add_entity`, no `set_entity_data`, no sound.
#   * `Mob.checkDespawn` offsets 0-27 — `getDifficulty() == PEACEFUL &&
#     !getType().isAllowedInPeaceful()` → `discard()`. It is reached from
#     `ServerLevel.tick`'s per-entity lambda (`lambda$tick$0`, offsets 17-30),
#     which calls `Entity.checkDespawn()` on **every** entity every tick behind
#     nothing but `isRemoved()` and `TickRateManager.isEntityFrozen`. So a
#     monster that got into the world some other way leaves it on the next tick.
#
# Two independent gates, and the first one alone means a monster scenario run
# against a peaceful vanilla is graded against silence. Recorded against a
# subject at `easy`, which spawns the creeper, the divergence that scenario
# reports is 100% the harness's own configuration. That is this project's
# central defect class with the sign flipped: not a check that cannot go red,
# but a check that cannot go anything else.
#
# Three lanes met it and all three did the right thing — `mob-in-world`,
# `mob-combat` and `mobs-in-world-2` between them add ten mob scenarios and
# **not one uses a monster**: pig, bat, cow, sheep, chicken, iron golem, snow
# golem, all of them either `allowedInPeaceful` or a golem. The corpus has no
# monster coverage because the harness could not give it any.
#
# # WHY IT IS NOT SIMPLY `--difficulty peaceful` ON THE LAUNCH LINE
#
# `crates/rustmc-server/src/main.rs` says it in its own comment — "anything
# else is ignored for now rather than rejected, so adding flags later does not
# break a caller". A tree without the flag takes `--difficulty peaceful`,
# ignores it, and boots at its own default. The launch line would then *say*
# the two sides agree while they did not, which is worse than the bug it
# replaces: today the mismatch is at least visible in the diff between the
# vanilla props and the rustmc argv.
#
# So support is derived from the tree, the way `ci/t2-gameplay-subset.sh`
# derives its required tables and `ci/gen-gameplay-tables.sh` derives its
# generator arms — from the server source that would have to change for the
# answer to change — and the one state that cannot be matched is a refusal:
#
#   **a subject that HAS a difficulty and no way to be told it.**
#
# That is exactly the state the three mob lanes were in. On `main`, which has
# no difficulty at all, both greps are empty, nothing is passed, no refusal
# fires, and the run is byte-for-byte what it was before this file existed.
#
# `ci/gameplay-difficulty-falsify.sh` is its gate: it builds fixture trees for
# each of the four states and asserts the refusal fires on the one that cannot
# be matched and on nothing else.

# `set -e` is the caller's; this file only defines and assigns.

GAMEPLAY_DIFFICULTY="${GAMEPLAY_DIFFICULTY:-peaceful}"
DIFFICULTY_ARGS=()

# Derived, not assumed. `_subject_has_difficulty` asks whether the SUBJECT has
# a difficulty to get wrong — the setter, not the flag, so renaming the flag
# cannot quietly answer "no". `_subject_takes_difficulty` asks whether the
# launch line can tell it one.
_gd_subject_has_difficulty() {
    grep -rq 'pub fn set_difficulty' "$1/crates/rustmc-server/src/" 2>/dev/null
}
_gd_subject_takes_difficulty() {
    grep -q '"--difficulty"' "$1/crates/rustmc-server/src/main.rs" 2>/dev/null
}

# `$1` — the tree root. Prints one line and sets `DIFFICULTY_ARGS`.
# Exit 4, the harness's REFUSED status, when the sides cannot be matched.
difficulty_or_die() {
    local root="$1" has=no takes=no
    case "$GAMEPLAY_DIFFICULTY" in
        peaceful|easy|normal|hard) ;;
        *)  echo >&2
            echo "GAMEPLAY DIFFICULTY REFUSED — GAMEPLAY_DIFFICULTY=$GAMEPLAY_DIFFICULTY is not one of" >&2
            echo "         peaceful, easy, normal, hard. Vanilla's server.properties would take it" >&2
            echo "         and fall back to its own default, so the two sides would differ." >&2
            return 4 ;;
    esac
    _gd_subject_has_difficulty "$root" && has=yes
    _gd_subject_takes_difficulty "$root" && takes=yes

    if [ "$has" = yes ] && [ "$takes" = no ]; then
        echo >&2
        echo "GAMEPLAY DIFFICULTY REFUSED — this server has a difficulty and no way to be told one." >&2
        echo "         crates/rustmc-server/src/ declares set_difficulty(), so the subject runs at" >&2
        echo "         some difficulty; crates/rustmc-server/src/main.rs declares no \"--difficulty\"" >&2
        echo "         flag, so this harness cannot set it, and vanilla is booted at" >&2
        echo "         difficulty=$GAMEPLAY_DIFFICULTY regardless." >&2
        echo "         At PEACEFUL, SpawnEggItem.useOn returns FAIL before creating anything" >&2
        echo "         (EntityType.canSpawn, offsets 13-31), so every monster scenario would be" >&2
        echo "         graded against vanilla silence while the subject spawned the mob." >&2
        echo "         Fix: add the flag to main.rs beside --gamemode." >&2
        return 4
    fi

    if [ "$takes" = yes ]; then
        DIFFICULTY_ARGS=(--difficulty "$GAMEPLAY_DIFFICULTY")
        echo "difficulty: vanilla=$GAMEPLAY_DIFFICULTY  subject=$GAMEPLAY_DIFFICULTY (--difficulty)"
    else
        DIFFICULTY_ARGS=()
        echo "difficulty: vanilla=$GAMEPLAY_DIFFICULTY  subject=none (this tree has no difficulty)"
    fi
    return 0
}
