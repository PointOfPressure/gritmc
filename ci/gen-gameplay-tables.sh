#!/usr/bin/env bash
# gen-gameplay-tables.sh — make this tree's `data/vanilla/` tables its own.
#
#     ./ci/gen-gameplay-tables.sh            # generate whatever is missing/stale
#     ./ci/gen-gameplay-tables.sh --force    # regenerate all of them
#     ./ci/gen-gameplay-tables.sh --check    # report only, change nothing
#
# The list is **derived, not written down**: every `data/vanilla/…` path the
# server crate actually opens. A branch that adds a table adds it to this list
# by adding the `open`, and a branch that stops using one drops it the same
# way. A hand-maintained list is a list that goes stale, and the gate this
# replaces went stale in exactly that way — `set_placed_by.json` was the whole
# point of a branch and no gate knew it existed.
#
# Each table is stamped (see `ci/provenance.sh`) so `ci/t2-gameplay.sh` can
# refuse a table that came from another tree or predates its own generator.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
. "$ROOT/ci/provenance.sh"

MODE=""
case "${1:-}" in
    --force) MODE=force ;;
    --check) MODE=check ;;
    # Print the derived required set, one path per line, and stop. Nothing is
    # read, written or stamped. It exists so `ci/check-generator-arms.sh` can
    # ask THIS script what the required set is instead of re-implementing the
    # grep below, which is the drift this project keeps paying for.
    --list-tables) MODE=list ;;
    # Put back, from SEED, every table in data/vanilla that a tree generated
    # and THIS tree does not require. See the block after the derivation.
    --drop-foreign)
        MODE=drop
        SEED="${2:-}"
        [ -n "$SEED" ] || { echo "usage: $0 --drop-foreign <seed data/vanilla dir>" >&2; exit 2; } ;;
    "")      ;;
    *)       echo "usage: $0 [--force|--check|--list-tables|--drop-foreign SEED]" >&2; exit 2 ;;
esac

# --- the incomplete marker --------------------------------------------------
#
# ADDED 2026-08-12, and the incident it comes from is worth the paragraph
# because the exit status was never the problem. On that day a fresh tree's
# regeneration died at the twelfth of nineteen generators — `cargo: command not
# found`, from a non-interactive `ssh` with no login profile — and this script
# did exactly what it should: it printed `ci/dump-recipe-unlocks.sh FAILED` and
# **exited 1**. Measured, not assumed; the log is quoted in
# `docs/cold-start/ci-gentables-exit-status.md`.
#
# What went wrong is what the tree looked like afterwards. `cp -rn` had already
# populated `data/vanilla` from another checkout, so the seven tables that were
# never reached were not *absent* — they were another branch's, present,
# non-empty and entirely plausible. Eleven tables carried this tree's stamp and
# seven carried a foreign one, and **nothing that reads a table reads its
# stamp**: every golden test in T0 just opens the file. T0 came back with eight
# red golden-table tests and a red `check-use-probe-falsify`, all of which read
# as defects in the branch under test, and an agent lost a cycle to it.
#
# So the exit status of the run that failed is not the only thing a later
# caller needs; it needs the state of the tree, and the run that could tell it
# is over. This marker is that state, written where the tables are so it
# travels with them — including through the `cp -rn` that starts every gate
# directory, which is the right direction: a `data/vanilla` copied out of a
# half-generated tree is itself half-generated.
#
# It is a statement, not a latch. A run that completes the work removes it, so
# the repair for a red is to finish the job rather than to clear a flag; a
# check that can only be silenced by hand is a check that gets silenced.
MARKER="$ROOT/data/vanilla/.provenance/INCOMPLETE"

marker_write() { # marker_write <reason>
    mkdir -p "$(dirname "$MARKER")"
    {
        printf 'tree=%s\n' "$ROOT"
        printf 'host=%s\n' "$(prov_host_id)"
        printf 'since=%s\n' "$(date -Is)"
        printf 'reason=%s\n' "$1"
        printf 'pending=%s\n' "${2:-unknown}"
    } > "$MARKER"
}
marker_report() {
    [ -e "$MARKER" ] || return 1
    echo "gen-gameplay-tables: this data/vanilla is INCOMPLETE — a previous run did not finish" >&2
    sed 's/^/    /' "$MARKER" >&2
    echo "    fix it by running: ./ci/gen-gameplay-tables.sh" >&2
    return 0
}

# table basename -> generator. The only hand-written part, and a table with no
# entry here is an error rather than a silent skip.
generator_for() {
    case "$1" in
        advancements.json)      echo ci/advancements-golden.sh ;;
        attributes.json)        echo ci/attributes-golden.sh ;;
        # THREE JAR-ONLY GENERATORS, ~6 s between them. None starts a JVM
        # beyond `javap` and none needs cargo, so they are the cheapest
        # rows in this table; see `crates/rustmc-entity/src/spawn/`.
        biome_spawners.json)    echo ci/dump-biome-spawners.sh ;;
        entity_spawn_facts.json) echo ci/dump-entity-spawn-facts.sh ;;
        block_entities.json)    echo ci/block-entity-golden.sh ;;
        block_explosion.json)   echo ci/block-explosion-golden.sh ;;
        explosion_ray.json)     echo ci/explosion-ray-golden.sh ;;
        explosion_wire.json)    echo ci/explosion-wire-golden.sh ;;
        explosion_entity.json)  echo ci/explosion-entity-golden.sh ;;
        block_props.json)       echo ci/block-props-golden.sh ;;
        block_mining.json)      echo ci/mining-golden.sh ;;
        collision_shapes.json)  echo ci/collision-shapes-golden.sh ;;
        # The OUTLINE shapes (`getShape`), same carrier; `BoatItem.use`'s
        # view ray is the first reader. ~10 s, two runs, compared.
        outline_shapes.json)    echo ci/outline-shapes-golden.sh ;;
        commands_tree.json)     echo ci/commands-golden.sh ;;
        # Vanilla's WHOLE tree, which is a different question from the one
        # above: `commands_tree.json` is vanilla's bytes for OUR command set,
        # this is vanilla's bytes for VANILLA's. ~32 s, two runs, compared.
        command_tree_full.json) echo ci/command-tree-extract.sh ;;
        # And a third question again: the two above are both about the tree a
        # client is SHOWN. This is what an argument PARSE consumes — 770 rows
        # of (parser, input) -> (accepted, cursor) from vanilla's own
        # `ArgumentType.parse`. ~10 s, two runs, compared byte for byte.
        command_parse.json)     echo ci/command-parse-golden.sh ;;
        damage_types.json)      echo ci/damage-types-golden.sh ;;
        mob_effects.json)       echo ci/mob-effects-golden.sh ;;
        movement_props.json)    echo ci/movement-props-golden.sh ;;
        enchantments.json)      echo ci/enchantments-golden.sh ;;
        path_facts.json)        echo ci/path-facts-golden.sh ;;
        mob_variants.json)      echo ci/dump-mob-variants.sh ;;
        entity_types.json)      echo ci/dump-entity-types.sh ;;
        # `FireBlock.igniteOdds` / `burnOdds`, the two maps every fire
        # decision reads. ~10 s, one JVM run, two dumps compared, and its size
        # cross-checked against a count derived from the bytecode rather than
        # written down — see the script header.
        fire_odds.json)         echo ci/fire-odds-golden.sh ;;
        # `canSurvive` over (subject, ground) and a dark pass. Required since
        # 2026-10-03 through `rustmc_behavior::survival::TABLE_PATH`; before
        # that no literal named it, so a gate kept whatever copy it was
        # seeded with and `golden_survival` measured a foreign oracle.
        survival.json)          echo ci/survival-golden.sh ;;
        end_podium.json)        echo ci/end-podium-golden.sh ;;
        end_gateway_ring.json)  echo ci/end-gateway-ring-golden.sh ;;
        flat_preset.json)       echo ci/dump-flat-preset.sh ;;
        fluid_rules.json)       echo ci/fluid-rules-golden.sh ;;
        fuel_values.json)       echo ci/fuel-values-golden.sh ;;
        jukebox_songs.json)     echo ci/jukebox-song-golden.sh ;;
        items.json)             echo ci/dump-items.sh ;;
        # FOUR TABLES, ONE GENERATOR — 2026-09-05.
        #
        # `ci/loot-golden.sh` writes all four in one JVM run, so this loop calls
        # it up to four times on a tree that has none of them: ~13 s each,
        # ~52 s of one-time setup. That is the price of the crate's headline
        # figure being gate-enforced rather than resting on files that lived in
        # one worktree, which is where all four were until this commit.
        loot.json)              echo ci/loot-golden.sh ;;
        loot_tables.json)       echo ci/loot-golden.sh ;;
        item_tags.json)         echo ci/loot-golden.sh ;;
        block_loot_map.json)    echo ci/loot-golden.sh ;;
        # `items.json` is the registry *report* — ids and nothing else. This
        # is the components: `Item.components()` per item, which is where
        # `Item.Properties` puts max stack size, max damage, rarity, food,
        # tool and equippable. About 10 s; one JVM, no world.
        item_props.json)        echo ci/item-props-golden.sh ;;
        # `BlockState.canSurvive` for every block a `BlockItem` can place,
        # over 79 fully specified one-block neighbourhoods at two light
        # levels. `survival.json` cannot answer this: its subjects come from
        # the datapack's `would_survive` modifiers and are 51 vegetation
        # blocks, where a `BlockItem` names 1,115 including every torch,
        # sign, rail, door, ladder and carpet. ~14 s; one JVM, no world.
        item_survive.json)      echo ci/item-survive-golden.sh ;;
        # Who `BoneMealItem.useOn` can grow. The census is the point:
        # `BonemealableBlock` is 45 classes over 63 blocks, and a port that
        # grows wheat and silently does nothing for the other 58 is
        # indistinguishable in every trace from one that was never written.
        # ~13 s; one JVM, no world.
        bonemeal.json)          echo ci/bonemeal-golden.sh ;;
        piston_push.json)       echo ci/piston-golden.sh ;;
        placement.json)         echo ci/placement-golden.sh ;;
        potions.json)           echo ci/potions-golden.sh ;;
        protocol_ids.json)      echo ci/dump-protocol-ids.sh ;;
        recipe_unlocks.json)    echo ci/dump-recipe-unlocks.sh ;;
        # Vanilla's own answer to "what does this grid craft", asked of the
        # jar's `RecipeMap.getRecipesFor(...).findFirst()` over grids built from
        # `Recipe.placementInfo()`. 3,122 vectors, ~15 s, one JVM run. It is a
        # separate generator from `dump-recipes.sh` because that one restates
        # the recipe DOCUMENTS and this one is a measurement of vanilla's
        # behaviour over them — a table that agrees with the documents by
        # construction cannot gate the thing they leave open, which is which of
        # two matching recipes wins.
        recipe_match.json)      echo ci/recipe-match-golden.sh ;;
        # One script for two tables, deliberately: recipes are written almost
        # entirely in terms of item tags, so a tree with one and not the other
        # has a recipe engine that matches nothing. See ci/dump-recipes.sh.
        recipes.json)           echo ci/dump-recipes.sh ;;
        tags_item.json)         echo ci/dump-recipes.sh ;;
        # BLOCK TAGS — 2026-09-05, and the arm is here BEFORE any reader.
        #
        # `data/vanilla/tags_block.json` has been on this box since
        # 2026-07-21 with no generator at all: `generator_for()` had no arm,
        # so no tree could make it and no tree stamped it, and every copy of
        # it arrived through the shared `data/vanilla` copy from whichever
        # branch produced the original. `crates/rustmc-server/src/itemuse.rs`
        # names this exactly and refuses `FlintAndSteelItem`'s lighting branch
        # on it — `state.is(BlockTags.CAMPFIRES / CANDLES / CANDLE_CAKES)` —
        # because reading an unstamped artefact gates one branch against
        # another. That refusal was correct; the missing generator is what
        # made it necessary.
        #
        # **The table does not enter the required set until something names
        # the literal.** The list above is derived from quoted
        # `"data/vanilla/x.json"` literals in `crates`, `tools` and `plugins`,
        # and `itemuse.rs` mentions this one only in prose, in backticks. A
        # literal planted with no reader is a dead reference and this project
        # has paid for one already. So this arm is dormant by construction and
        # fires on the commit that adds the first `state.is(...)` call — at
        # which point every tree regenerates and stamps it, which is the whole
        # point.
        tags_block.json)        echo ci/dump-block-tags.sh ;;
        registries.bin)         echo ci/dump-registries.sh ;;
        set_placed_by.json)     echo ci/placed-golden.sh ;;
        spawn_placements.json)  echo ci/dump-spawn-placements.sh ;;
        # Four tables, one script, for the reason recipes and item tags share
        # one: a tree with the sets but not the definitions has a structure
        # census whose denominator and whose numerator come from different
        # jars. `ci/dump-structure-tables.sh` is an unzip and a merge — no JVM,
        # no cargo, well under a second — so the cost of regenerating all four
        # whenever any one is stale is nothing.
        #
        # `structure_sets.json` is the reason this arm exists. It was on every
        # tree on the box, read by nine source files, produced by no script,
        # and carried no provenance stamp, so the foreign-table refusal that
        # exists to stop a gate measuring another tree's tables could not see
        # it at all.
        structure_sets.json)      echo ci/dump-structure-tables.sh ;;
        structure_defs.json)      echo ci/dump-structure-tables.sh ;;
        template_pools.json)      echo ci/dump-structure-tables.sh ;;
        structure_templates.json) echo ci/dump-structure-tables.sh ;;
        signal.json)            echo ci/signal-golden.sh ;;
        sound_types.json)       echo ci/sound-types-golden.sh ;;
        timelines.json)         echo ci/dump-timelines.sh ;;
        # THREE TABLES, ONE GENERATOR, for the reason the loot four are:
        # all three are a walk over a registry that `Bootstrap.bootStrap()`
        # has to build first, and the boot is the entire cost. A tree missing
        # all three therefore pays the ~10 s boot three times here; that is
        # the price of each table being independently derivable rather than
        # of one file secretly carrying the others.
        sound_events.json)      echo ci/sound-particle-golden.sh ;;
        particle_types.json)    echo ci/sound-particle-golden.sh ;;
        level_events.json)      echo ci/sound-particle-golden.sh ;;
        note_block.json)        echo ci/sound-particle-golden.sh ;;
        update_shape.json)      echo ci/shape-golden.sh ;;
        # No JVM and no golden: 26.2 moved villager trades into the datapack,
        # so this reads `oracle-work/vanilla-*/generated-data` directly. See
        # the script's header for what that changed.
        #
        # It wrote `villager_trades.json` until train-a (2026-09-30), and so
        # did `ci/villager-trade-golden.sh` below, in a different shape: two
        # lanes landed the same registry under one name. This one is the whole
        # 388-entry registry in `crate::villager`'s shape, so it took the name
        # that says so; the golden's pair keeps the old one.
        villager_trade_registry.json) echo ci/dump-villager-trades.sh ;;
        use_without_item.json)  echo ci/use-golden.sh ;;
        # The other half of the right-click, offset 145 rather than 196. Two
        # generators and not one because this one is 92 million probes and ~95
        # seconds a run against the other's ~14, and a tree whose
        # `use_without_item.json` went stale should not pay for that.
        #
        # **THIS ARM NOW FIRES, AS OF 2026-08-12.** It was written dormant —
        # nothing under `crates/` named the table, so the derived required set
        # did not hold it — because adding a two-and-a-half-minute generator to
        # every tree's setup before anything read its output would have been a
        # tax on every other branch on the box. `crate::useitemon` is the first
        # consumer and names the path, so from that commit every tree
        # regenerates this table and every tree pays the ~95 s.
        #
        # It is worth checking rather than assuming that it stays reproducible:
        # measured 2026-08-12 in `~/rustmc-pot`, the table this script
        # generated and a second run of `ci/use-item-on-golden.sh` over it are
        # byte-identical — 1,748,342 bytes, md5
        # `33bf3128688a105248fab730bad4e2bf`, `cmp` clean, and the generator
        # itself dumps twice and refuses if its own two runs disagree. A table
        # that is not reproducible would poison every measurement taken after
        # it, and from this commit that is every measurement on the box.
        use_item_on.json)       echo ci/use-item-on-golden.sh ;;
        # The three-position extract: `updateShape` for the 78,795 pairs that
        # `update_shape.json` refuses because the rule read one position past
        # its own neighbour. Same shape of arm as `use_item_on.json` above and
        # for a stronger version of the same reason — it is about twenty
        # minutes a run against `ci/shape-golden.sh`'s ~84 seconds, and
        # **nothing under `crates/` opens it**, so nothing requires it and this
        # arm does not fire.
        #
        # It is not a census and must not be filed with one. Its level answers
        # three positions and throws for a fourth, so unlike
        # `ci/checks/shape-refusal-census.json` it invents nothing and a port
        # may read it the day one exists. The arm is here so that day is a
        # one-line change.
        update_shape3.json)     echo ci/shape-three.sh ;;
        # TWO TABLES, ONE GENERATOR, and the split is a partition of one
        # registry rather than two subjects: in 26.2 every trade is an entry in
        # `minecraft:villager_trade` and the wandering trader's 97 live in the
        # same registry as the professions' 291. One JVM run writes both, so a
        # tree missing either pays ~4 s once.
        villager_trades.json)   echo ci/villager-trade-golden.sh ;;
        wandering_trader_trades.json) echo ci/villager-trade-golden.sh ;;
        *)                      return 1 ;;
    esac
}

# CLOSED 2026-08-07. It was marked LATENT on 2026-08-06 with the note that
# widening is "a one-word change and a no-op today". Both halves were measured
# before changing it and both held, so it is widened here rather than carried:
#
#   * **the scan root.** It was `crates/rustmc-server/src`, and the property
#     claimed is repo-wide. The branch that exposed the stale-table defect
#     failed in `crates/rustmc-behavior`, which reaches `block_props.json`
#     through a path the server crate also names — a coincidence of the layout,
#     not a property. One file move reinstates "a branch that adds a table can
#     never pass".
#   * **the character class.** `[a-z_]+` does not match a digit or a capital.
#     Demonstrated 2026-08-07 on a constructed tree: a server source opening
#     `"data/vanilla/signal2.json"`, that table absent from `data/vanilla`, and
#     this script printing `1 tables required` / `all current` and **exit 0**.
#     `ci/t2-gameplay.sh`'s preflight runs the same grep, so its refusal —
#     the one that exists to stop a gate measuring another tree's tables —
#     could not fire for that table either.
#
# Measured on this tree the same day, all four combinations of the two axes
# derive the **same 16 tables**, so this widening changes nothing today and is
# a no-op the gate can be re-measured against.
#
# RESIDUAL, STATED SO THE PASS IS NOT READ AS MORE THAN IT IS. A path that is
# not one quoted literal is still invisible — `concat!("data/vanilla/",
# "beds.json")` derives nothing, demonstrated in the same run. A blanket "any
# mention of data/vanilla we did not match is an error" was tried and rejected:
# it fires today on `data/vanilla/hash_order.json` and `data/vanilla/structure/`,
# which are version-pinned jar extracts with no generator, so it would refuse
# every tree. The literal is the convention; a computed one is a code review
# item, not something this grep can see.
mapfile -t TABLES < <(grep -rhoE '"data/vanilla/[A-Za-z0-9_]+\.(json|bin)"' \
                      crates tools plugins 2>/dev/null \
                      | tr -d '"' | sort -u)
[ "${#TABLES[@]}" -gt 0 ] || { echo "gen-gameplay-tables: derived no tables — the grep has gone stale" >&2; exit 2; }

if [ "$MODE" = list ]; then printf '%s\n' "${TABLES[@]}"; exit 0; fi

# --- --drop-foreign: a gate directory holds only seed tables and this tree's ---
#
# A gate directory is reused by every branch. This script regenerates and
# stamps the tables THIS tree requires, and leaves every other file alone — so
# a table another branch required, generated and stamped stayed behind, and
# the next branch's tests read it as if it were the seed's. The ranged gate of
# 2026-10-04 18:31 (gate-ranged-20261004-183154.log, T1 RED, four
# `golden_survival.rs` reds) read nether-parity's `survival.json`: nether-parity
# changed `SurvivalGolden.java` and required the table, ranged required neither
# and inherited its output. Nothing could have caught it, because the only
# check that reads a stamp is asked only about this tree's own tables.
#
# A stamp (`data/vanilla/.provenance/<table>.prov`) is the mark of a generated
# table: the seed copy carries none (stage 1 removes them). So every stamped
# table this tree does not require is another tree's output. It is restored
# from SEED — the state a fresh gate directory starts in — or removed if the
# seed has no such file, and its stamp goes with it. Tables this tree requires
# are left to the run that follows, which checks or regenerates each one.
if [ "$MODE" = drop ]; then
    [ -d "$SEED" ] || { echo "gen-gameplay-tables --drop-foreign: no seed directory at $SEED" >&2; exit 2; }
    dropped=0
    for prov in "$ROOT"/data/vanilla/.provenance/*.prov; do
        [ -e "$prov" ] || continue
        t="data/vanilla/$(basename "$prov" .prov)"
        case " ${TABLES[*]} " in *" $t "*) continue ;; esac
        if [ -e "$SEED/$(basename "$t")" ]; then
            cp -f "$SEED/$(basename "$t")" "$ROOT/$t" || { echo "  !! could not restore $t from $SEED" >&2; exit 2; }
            echo "  <- $(basename "$t")  (another tree's; this tree does not require it — restored from the seed)"
        else
            rm -f "$ROOT/$t"
            echo "  xx $(basename "$t")  (another tree's; this tree does not require it and the seed has none — removed)"
        fi
        rm -f "$prov"
        dropped=$((dropped + 1))
    done
    echo "gen-gameplay-tables --drop-foreign: $dropped foreign table(s) put back"
    exit 0
fi

echo "gen-gameplay-tables: $ROOT"
echo "  ${#TABLES[@]} tables required by crates/, tools/ and plugins/"

# # WHO REQUIRED THE EXPENSIVE ONES, NAMED RATHER THAN INFERRED
#
# The grep above reads a **quoted path**, and it does not care whether the
# quotes are around a `File::open` or around a sentence in a comment. Verified
# by construction on 2026-08-19: a Rust doc comment reading
# `/// see "data/vanilla/update_shape3.json"` puts that table in the required
# set, and every tree on the box then pays twenty minutes at setup for a table
# nothing opens.
#
# That is not hypothetical. `tools/goldengen/vanilla/ShapeGolden.java` already
# names `update_shape3.json` in a comment, inside this scan root, in backticks
# — one character from the quoted form. So does this file. The arms below say
# "the first consumer is a one-line change"; an accidental *mention* is the
# same one-line change with none of the intent, and it arrives as twenty
# unexplained minutes rather than as an error.
#
# So when an expensive table is required, this says which line required it.
# Naming the citing file is the whole fix: a real consumer reads as a source
# line that opens the table, and an accident reads as a comment.
for expensive in update_shape3.json use_item_on.json; do
    case " ${TABLES[*]} " in
        *" $expensive "*) ;;
        *) continue ;;
    esac
    echo "  NOTE: $expensive is REQUIRED by this tree, and it is one of the two"
    echo "        expensive optional tables. Required by:"
    grep -rn "\"data/vanilla/$expensive\"" crates tools plugins 2>/dev/null \
        | sed 's/^/          /' | head -8
    echo "        If those are comments rather than code that opens it, the"
    echo "        requirement is an accident and costs every tree on the box"
    echo "        a full generation at setup. Backticks do not match; quotes do."
done
RC=0
NEED=()
# The DISTINCT generators `NEED` names, in first-seen order. Several scripts
# here write more than one table in one JVM run — `ci/loot-golden.sh` writes
# four, `ci/dump-recipes.sh` two — and the loop below used to be over tables,
# so it invoked such a script once per table it owns. See the block above
# `run_generator`.
GENS=()
for t in "${TABLES[@]}"; do
    base="$(basename "$t")"
    if ! gen="$(generator_for "$base")"; then
        echo "  !! $base — no generator known. Add one to generator_for()." >&2
        RC=2; continue
    fi
    if [ ! -x "$ROOT/$gen" ]; then
        echo "  !! $base — generator $gen is missing from this tree" >&2
        RC=2; continue
    fi
    if [ "$MODE" = force ]; then
        why="forced"
    elif why="$(prov_check "$ROOT" "$ROOT/$t")"; then
        echo "  ok $base"
        continue
    fi
    echo "  -> $base  ($why)  via $gen"
    NEED+=("$t|$gen")
    case " ${GENS[*]-} " in *" $gen "*) ;; *) GENS+=("$gen") ;; esac
done
[ "$RC" -eq 0 ] || exit "$RC"

if [ "$MODE" = check ]; then
    RC=0
    marker_report && RC=1
    if [ "${#NEED[@]}" -eq 0 ]; then
        [ "$RC" -eq 0 ] && echo "gen-gameplay-tables: all current"
        exit "$RC"
    fi
    echo "gen-gameplay-tables: ${#NEED[@]} table(s) need regenerating" >&2
    exit 1
fi

[ "${#NEED[@]}" -eq 0 ] || \
    marker_write "generating ${#NEED[@]} table(s)" "$(printf '%s ' "${NEED[@]%%|*}")"

# ONE GENERATOR, ONE RUN — 2026-09-05.
#
# This loop was over `NEED`, which is (table, generator) pairs, so a generator
# owning n tables ran n times. `ci/loot-golden.sh` owns four — `loot.json`,
# `loot_tables.json`, `item_tags.json`, `block_loot_map.json` — and writes all
# four in a single JVM run, which its own entry in `generator_for` said in
# words while this loop went on paying for it: on a tree that has none of them
# it was invoked 4 times at ~13 s, ~52 s where ~13 s buys the same four tables.
# `ci/dump-recipes.sh` owns two and cost double the same way.
#
# The stamping is what makes this more than an optimisation. `prov_stamp` was
# applied to the *pair's* table only, so run 1 wrote four tables and stamped
# one, and the other three were left carrying no stamp until their own turn
# came round — a window in which `prov_check` calls them "never generated in
# this tree" and `ci/t2-gameplay.sh` exits 4 on them. Stamping every table a
# generator owns, in the run that wrote them, closes that by construction
# rather than by the loop happening to come back.
#
# What is NOT assumed: that a generator writes every table it owns. Each one is
# checked for existence, non-emptiness and a moved mtime individually, exactly
# as a single-table run was — so a generator that quietly stopped writing one
# of its four is a failure here and not a table left holding another tree's
# copy under this tree's stamp.
for gen in "${GENS[@]+"${GENS[@]}"}"; do
    # The tables this generator owns, out of the ones that need regenerating.
    OWNED=()
    for item in "${NEED[@]+"${NEED[@]}"}"; do
        [ "${item##*|}" = "$gen" ] && OWNED+=("${item%%|*}")
    done
    echo "== $gen  (${#OWNED[@]} table(s))"

    # What the table looked like before the generator ran, so that "the
    # generator claims it wrote this" can be tested rather than believed.
    # Contents are the wrong thing to compare — several of these generators are
    # deterministic and say so in their own output ("identical across two
    # runs"), so an unchanged digest is the normal case. A generator that
    # writes its output always moves the mtime; one that returns 0 having
    # written nothing does not.
    BEFORE=()
    for t in "${OWNED[@]}"; do
        if [ -e "$ROOT/$t" ]; then BEFORE+=("$(stat -c '%y %s' "$ROOT/$t" 2>/dev/null)")
        else BEFORE+=(""); fi
    done

    "$ROOT/$gen" || {
        echo "gen-gameplay-tables: $gen FAILED" >&2
        marker_write "$gen failed" "$(printf '%s ' "${OWNED[@]}")"
        exit 1
    }

    for i in "${!OWNED[@]}"; do
    t="${OWNED[$i]}"; before="${BEFORE[$i]}"
    [ -s "$ROOT/$t" ] || {
        echo "gen-gameplay-tables: $gen ran but $t is missing or empty" >&2
        marker_write "$gen produced no table" "$t"
        exit 1
    }

    # THE CASE THE EXIT STATUS CANNOT SEE, and the reason this check is worth
    # more than the one above it. A generator that exits 0 without writing
    # leaves whatever was already at that path — and on every gate directory
    # what is already at that path is **another tree's table**, put there by
    # the `cp -rn` in the setup. The two checks above both pass on it: the
    # status is 0 and the file is not empty. The stamp on the next line would
    # then certify a foreign table as this tree's own, which does not merely
    # miss a defect, it disarms `ci/t2-gameplay.sh`'s refusal for that table
    # permanently — the one instrument that can tell the difference is the one
    # being told the lie. `ci/dump-flat-preset.sh` could reach this exact state
    # until 2026-08-12; see its header.
    after="$(stat -c '%y %s' "$ROOT/$t" 2>/dev/null)"
    [ "$after" != "$before" ] || {
        echo "gen-gameplay-tables: $gen exited 0 but did not write $t —" >&2
        echo "    the file on disk is the one that was already there, and is NOT this tree's." >&2
        marker_write "$gen exited 0 without writing" "$t"
        exit 1
    }

    prov_stamp "$ROOT" "$ROOT/$t" "$gen"
    echo "   stamped $t"
    done
done

rm -f "$MARKER"
echo "gen-gameplay-tables: ${#TABLES[@]} tables current in $ROOT"
