#!/usr/bin/env bash
# gen-tables-falsify.sh — prove `ci/gen-gameplay-tables.sh` cannot report a
# green it has not earned.
#
# WHY THIS EXISTS. On 2026-08-12 a fresh tree's `data/vanilla` regeneration
# died partway — `ci/dump-recipe-unlocks.sh`, the twelfth of nineteen, on
# `cargo: command not found`, because a non-interactive `ssh` gets no login
# profile and the setup snippet in `docs/AGENT-BRIEF.md` did not export
# `$HOME/.cargo/bin`. Seven tables were therefore never regenerated. They were
# not *missing*: `cp -rn` had already put another tree's copies there, so
# `data/vanilla` held nineteen plausible, non-empty tables of which seven
# belonged to a different branch. T0 came back with eight red golden-table
# tests and a red `check-use-probe-falsify`, and every one of them read as a
# defect in the branch under test. An agent lost a T0 cycle to it.
#
# The generator's own exit status was never the whole story — this file's
# first job is to keep it honest, and its second is to catch the case the exit
# status cannot see: **a generator that returns 0 and writes nothing**, which
# leaves the table that was already on disk in place and then stamps it with
# this tree's provenance. That is worse than a red. The stamp is the one
# instrument that can tell a table this tree made from one it did not, and
# laundering a foreign table through it disarms `ci/t2-gameplay.sh`'s refusal
# for that table permanently.
#
# HOW. A throwaway tree with stub generators — no cargo, no JVM, no jar, and
# nothing that touches the real `data/vanilla`. Each case injects exactly one
# fault and asserts the run refuses; the controls assert it does not refuse
# when nothing is wrong, because a checker that always refuses is not a
# checker. Runs in about a second.
#
#     ./ci/gen-tables-falsify.sh          # all cases
#     ./ci/gen-tables-falsify.sh -v       # ...and show each run's output
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERBOSE=0
[ "${1:-}" = "-v" ] && VERBOSE=1

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); printf '  PASS  %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$1"; printf '        %s\n' "$2"; }

# --- the throwaway tree -----------------------------------------------------
#
# The table names are the REAL ones and the stub generators sit at the REAL
# paths, because `generator_for()` in the script under test is a hand-written
# case list and the whole point is to drive that list rather than a copy of it.
# Four tables is enough to have a first, a middle and a last.
TABLES=(attributes.json items.json recipe_unlocks.json signal.json)
GENS=(ci/attributes-golden.sh ci/dump-items.sh ci/dump-recipe-unlocks.sh ci/signal-golden.sh)

build_tree() { # build_tree -> prints the path
    local R="$WORK/tree"
    rm -rf "$R"
    mkdir -p "$R/ci" "$R/crates/fake/src" "$R/data/vanilla"
    cp "$ROOT/ci/gen-gameplay-tables.sh" "$R/ci/"
    cp "$ROOT/ci/provenance.sh"          "$R/ci/"
    chmod +x "$R/ci/gen-gameplay-tables.sh"

    # The derived list comes from quoted literals in the sources. Writing them
    # here rather than passing a list is the point: this exercises the same
    # grep the real tree does.
    {
        echo 'pub fn tables() -> &'"'"'static [&'"'"'static str] { &['
        local t; for t in "${TABLES[@]}"; do echo "    \"data/vanilla/$t\","; done
        echo '] }'
    } > "$R/crates/fake/src/lib.rs"

    local i
    for i in "${!TABLES[@]}"; do
        cat > "$R/${GENS[$i]}" <<EOS
#!/usr/bin/env bash
# stub generator for ${TABLES[$i]}
set -euo pipefail
SELF="\$(basename "\$0")"
OUT="\$(cd "\$(dirname "\$0")/.." && pwd)/data/vanilla/${TABLES[$i]}"
if [ "\${STUB_INJECT:-}" = "\$SELF" ]; then
    case "\${STUB_MODE:-}" in
    nonzero) echo "stub: deliberate failure" >&2; exit 3 ;;
    cargo)   cargo build -q -p oracle-runner ;;   # PATH is scrubbed: 127
    silent)  echo "stub: exiting 0 and writing nothing"; exit 0 ;;
    empty)   : > "\$OUT"; exit 0 ;;
    esac
fi
printf '{"stub":"%s","nanos":"%s"}\n' "\$SELF" "\$(date +%s%N)" > "\$OUT"
echo "stub \$SELF -> \$OUT"
EOS
        chmod +x "$R/${GENS[$i]}"
    done
    printf '%s\n' "$R"
}

# A table that is present, non-empty and plausible, carrying no stamp of this
# tree — exactly what `cp -rn` leaves behind.
seed_foreign() { # seed_foreign <tree> <table>
    printf '{"from":"another tree","payload":"plausible"}\n' > "$1/data/vanilla/$2"
}

# Run the script under test in a scrubbed environment. PATH keeps the system
# directories and nothing else, so a stub that shells out to cargo gets 127 —
# the real failure, not a simulation of it.
run_gen() { # run_gen <tree> [args...]; sets OUT_TXT and OUT_RC
    local R="$1"; shift
    OUT_TXT="$(cd "$R" && env -u CARGO_HOME -u RUSTUP_HOME \
        PATH=/usr/local/bin:/usr/bin:/bin STUB_INJECT="${STUB_INJECT:-}" STUB_MODE="${STUB_MODE:-}" \
        ./ci/gen-gameplay-tables.sh "$@" 2>&1)"
    OUT_RC=$?
    [ "$VERBOSE" -eq 1 ] && printf '%s\n' "$OUT_TXT" | sed 's/^/        | /'
    return 0
}

stamped() { [ -s "$1/data/vanilla/.provenance/$2.prov" ]; }
marker()  { printf '%s/data/vanilla/.provenance/INCOMPLETE' "$1"; }

echo "gen-tables-falsify: $ROOT"
echo

# --- CONTROL 1: nothing is wrong -------------------------------------------
# A gate that refuses everything is not a gate. This is the green that gives
# every refusal below its meaning.
echo "control 1 — all four generators succeed"
R="$(build_tree)"; STUB_INJECT=; STUB_MODE=; run_gen "$R"
if [ "$OUT_RC" -ne 0 ]; then
    bad "clean run exits 0" "exit $OUT_RC"
else ok "clean run exits 0"; fi
MISSING=""
for t in "${TABLES[@]}"; do stamped "$R" "$t" || MISSING="$MISSING $t"; done
[ -z "$MISSING" ] && ok "all four tables stamped" || bad "all four tables stamped" "unstamped:$MISSING"
[ -e "$(marker "$R")" ] && bad "no INCOMPLETE marker after a clean run" "marker present" \
                        || ok "no INCOMPLETE marker after a clean run"

# --- CONTROL 2: --check on a complete tree ---------------------------------
run_gen "$R" --check
[ "$OUT_RC" -eq 0 ] && ok "--check exits 0 on a complete tree" \
                    || bad "--check exits 0 on a complete tree" "exit $OUT_RC: $OUT_TXT"
echo

# --- CASE 1: a generator exits non-zero ------------------------------------
echo "case 1 — the third generator exits 3"
R="$(build_tree)"; STUB_INJECT=dump-recipe-unlocks.sh; STUB_MODE=nonzero; run_gen "$R"
[ "$OUT_RC" -ne 0 ] && ok "run exits non-zero" || bad "run exits non-zero" "exit 0"
case "$OUT_TXT" in *dump-recipe-unlocks.sh*FAILED*|*FAILED*dump-recipe-unlocks.sh*)
    ok "says which generator died" ;;
  *) bad "says which generator died" "no naming line in output" ;;
esac
stamped "$R" signal.json && bad "stops at the failure" "signal.json was generated after the failure" \
                         || ok "stops at the failure"
[ -e "$(marker "$R")" ] && ok "leaves an INCOMPLETE marker" \
                        || bad "leaves an INCOMPLETE marker" "no marker at $(marker "$R")"
run_gen "$R" --check
[ "$OUT_RC" -ne 0 ] && ok "--check refuses the half-written tree" \
                    || bad "--check refuses the half-written tree" "exit 0"
echo

# --- CASE 2: the real one — cargo is not on PATH ---------------------------
echo "case 2 — the third generator shells out to cargo, which is not on PATH"
R="$(build_tree)"; STUB_INJECT=dump-recipe-unlocks.sh; STUB_MODE=cargo; run_gen "$R"
[ "$OUT_RC" -ne 0 ] && ok "run exits non-zero" || bad "run exits non-zero" "exit 0"
case "$OUT_TXT" in *"command not found"*) ok "the cause is in the output" ;;
  *) bad "the cause is in the output" "no 'command not found' in output" ;;
esac
case "$OUT_TXT" in *dump-recipe-unlocks.sh*) ok "names the generator" ;;
  *) bad "names the generator" "generator not named" ;;
esac
echo

# --- CASE 3: a generator returns 0 and writes nothing ----------------------
#
# The one the exit status cannot catch. The table on disk is another tree's,
# it is non-empty, and the pre-fix script stamped it as this tree's own.
echo "case 3 — the third generator exits 0 and writes nothing, over a foreign table"
R="$(build_tree)"; seed_foreign "$R" recipe_unlocks.json
STUB_INJECT=dump-recipe-unlocks.sh; STUB_MODE=silent; run_gen "$R"
[ "$OUT_RC" -ne 0 ] && ok "run exits non-zero" || bad "run exits non-zero" "exit 0"
stamped "$R" recipe_unlocks.json \
    && bad "does NOT stamp the table it did not write" "the foreign table was stamped as ours" \
    || ok "does NOT stamp the table it did not write"
if grep -q 'another tree' "$R/data/vanilla/recipe_unlocks.json" 2>/dev/null; then
    ok "the foreign bytes are still on disk (nothing destroyed)"
else
    bad "the foreign bytes are still on disk (nothing destroyed)" "table was modified"
fi
echo

# --- CASE 4: a generator returns 0 and writes an empty table ---------------
echo "case 4 — the third generator exits 0 and truncates its table"
R="$(build_tree)"; STUB_INJECT=dump-recipe-unlocks.sh; STUB_MODE=empty; run_gen "$R"
[ "$OUT_RC" -ne 0 ] && ok "run exits non-zero" || bad "run exits non-zero" "exit 0"
stamped "$R" recipe_unlocks.json && bad "does not stamp an empty table" "stamped" \
                                 || ok "does not stamp an empty table"
echo

# --- CASE 5: resuming a half-written tree finishes it ----------------------
#
# The marker must be a statement about the tree, not a latch: a run that
# completes the work clears it. Otherwise the fix trades a false green for a
# permanent false red, which is the trade that gets a gate switched off.
echo "case 5 — a second run completes the tree and clears the marker"
R="$(build_tree)"; STUB_INJECT=dump-recipe-unlocks.sh; STUB_MODE=nonzero; run_gen "$R"
STUB_INJECT=; STUB_MODE=; run_gen "$R"
[ "$OUT_RC" -eq 0 ] && ok "the repair run exits 0" || bad "the repair run exits 0" "exit $OUT_RC: $OUT_TXT"
[ -e "$(marker "$R")" ] && bad "the marker is cleared" "marker still present" || ok "the marker is cleared"
MISSING=""
for t in "${TABLES[@]}"; do stamped "$R" "$t" || MISSING="$MISSING $t"; done
[ -z "$MISSING" ] && ok "all four tables stamped after the repair" \
                  || bad "all four tables stamped after the repair" "unstamped:$MISSING"
echo

# --- CASE 6: a table with no generator is still a refusal ------------------
#
# Pre-existing behaviour, asserted here so a later edit cannot quietly turn it
# into a skip — a table nothing knows how to make is the shape the derived
# list exists to expose.
echo "case 6 — a required table with no generator_for() case"
R="$(build_tree)"
printf 'pub const X: &str = "data/vanilla/no_such_table.json";\n' >> "$R/crates/fake/src/lib.rs"
STUB_INJECT=; STUB_MODE=; run_gen "$R"
[ "$OUT_RC" -ne 0 ] && ok "run exits non-zero" || bad "run exits non-zero" "exit 0"
case "$OUT_TXT" in *no_such_table.json*) ok "names the table" ;;
  *) bad "names the table" "table not named" ;;
esac
echo

# --- CASE 7: a generator that owns four tables runs ONCE -------------------
#
# `ci/loot-golden.sh` writes `loot.json`, `loot_tables.json`, `item_tags.json`
# and `block_loot_map.json` in one JVM run, and `generator_for()` maps all four
# to it. The regeneration loop was over (table, generator) PAIRS, so it invoked
# that one script four times — ~13 s each on the box, ~52 s for four tables one
# run produces — and stamped only the pair's own table each time, leaving the
# other three carrying no stamp until their turn came round. `prov_check` calls
# an unstamped table "never generated in this tree" and `ci/t2-gameplay.sh`
# exits 4 on it.
#
# The count is the assertion. A timing claim would not travel between machines
# and would not fail on a fast one; the number of invocations is the thing that
# was wrong. On the loop this case was written against it is 4, and the
# `stamped` assertions below pass there too — which is why the count has to be
# checked rather than the outcome.
#
# `ci/dump-recipes.sh` (recipes.json + tags_item.json) has the same shape and
# is covered by the same fix; four tables is the harder case and is the one
# built here.
echo "case 7 — one generator owning four tables is invoked once"
LOOT=(loot.json loot_tables.json item_tags.json block_loot_map.json)
R="$WORK/loot"; rm -rf "$R"; mkdir -p "$R/ci" "$R/crates/fake/src" "$R/data/vanilla"
cp "$ROOT/ci/gen-gameplay-tables.sh" "$R/ci/"
cp "$ROOT/ci/provenance.sh" "$R/ci/"
chmod +x "$R/ci/gen-gameplay-tables.sh"
{
    echo 'pub fn tables() -> &'"'"'static [&'"'"'static str] { &['
    for t in "${LOOT[@]}"; do echo "    \"data/vanilla/$t\","; done
    echo '] }'
} > "$R/crates/fake/src/lib.rs"
cat > "$R/ci/loot-golden.sh" <<'EOS'
#!/usr/bin/env bash
# stub: writes all four loot tables in one run, as the real one does, and
# counts its own invocations.
set -euo pipefail
D="$(cd "$(dirname "$0")/.." && pwd)"
echo x >> "$D/invocations"
for t in loot.json loot_tables.json item_tags.json block_loot_map.json; do
    printf '{"stub":"loot-golden","table":"%s","nanos":"%s"}\n' "$t" "$(date +%s%N)" \
        > "$D/data/vanilla/$t"
done
echo "stub loot-golden.sh -> 4 tables"
EOS
chmod +x "$R/ci/loot-golden.sh"
STUB_INJECT=; STUB_MODE=; run_gen "$R"
[ "$OUT_RC" -eq 0 ] && ok "clean run exits 0" || bad "clean run exits 0" "exit $OUT_RC: $OUT_TXT"
N="$(wc -l < "$R/invocations" 2>/dev/null || echo 0)"
[ "$N" -eq 1 ] && ok "ci/loot-golden.sh invoked once for four tables" \
               || bad "ci/loot-golden.sh invoked once for four tables" "invoked $N time(s)"
MISSING=""
for t in "${LOOT[@]}"; do stamped "$R" "$t" || MISSING="$MISSING $t"; done
[ -z "$MISSING" ] && ok "all four loot tables stamped" \
                  || bad "all four loot tables stamped" "unstamped:$MISSING"
run_gen "$R" --check
[ "$OUT_RC" -eq 0 ] && ok "--check exits 0 after the single run" \
                    || bad "--check exits 0 after the single run" "exit $OUT_RC: $OUT_TXT"
echo

# --- CASE 8: the block-tag arm, driven through the REAL generator_for ------
#
# `data/vanilla/tags_block.json` had no arm at all until 2026-09-05 and the
# table has been on this box since July, unstamped, made by nobody. The arm is
# deliberately DORMANT in the real tree — nothing under `crates`, `tools` or
# `plugins` names the literal, so the derived set does not hold it — which
# means without this case the arm would be code no run reaches, which is the
# defect class this whole directory exists to refuse. So the fixture declares
# the literal itself, exactly as it does for the other four, and the case
# proves three things: `generator_for` maps the table, the loop runs the
# script it names, and the table is stamped afterwards.
echo "case 8 — the tags_block.json arm is reachable and stamps its table"
R="$WORK/blocktags"; rm -rf "$R"; mkdir -p "$R/ci" "$R/crates/fake/src" "$R/data/vanilla"
cp "$ROOT/ci/gen-gameplay-tables.sh" "$R/ci/"
cp "$ROOT/ci/provenance.sh" "$R/ci/"
chmod +x "$R/ci/gen-gameplay-tables.sh"
{
    echo 'pub const A: &'"'"'static str = "data/vanilla/tags_block.json";'
    echo 'pub const B: &'"'"'static str = "data/vanilla/signal.json";'
} > "$R/crates/fake/src/lib.rs"
# A stub at the REAL path the real case list names. If the arm named a
# different script this would not exist and the run would refuse.
cat > "$R/ci/dump-block-tags.sh" <<'EOS'
#!/usr/bin/env bash
set -euo pipefail
D="$(cd "$(dirname "$0")/.." && pwd)"
printf '{"campfires":{},"candles":{},"candle_cakes":{},"nanos":"%s"}\n' "$(date +%s%N)" \
    > "$D/data/vanilla/tags_block.json"
echo "stub dump-block-tags.sh"
EOS
cat > "$R/ci/signal-golden.sh" <<'EOS'
#!/usr/bin/env bash
set -euo pipefail
D="$(cd "$(dirname "$0")/.." && pwd)"
printf '{"stub":"signal","nanos":"%s"}\n' "$(date +%s%N)" > "$D/data/vanilla/signal.json"
echo "stub signal-golden.sh"
EOS
chmod +x "$R/ci/dump-block-tags.sh" "$R/ci/signal-golden.sh"
STUB_INJECT=; STUB_MODE=; run_gen "$R"
[ "$OUT_RC" -eq 0 ] && ok "clean run exits 0" || bad "clean run exits 0" "exit $OUT_RC: $OUT_TXT"
case "$OUT_TXT" in *ci/dump-block-tags.sh*) ok "generator_for maps tags_block.json to ci/dump-block-tags.sh" ;;
  *) bad "generator_for maps tags_block.json to ci/dump-block-tags.sh" "not named in the output"; ;;
esac
stamped "$R" tags_block.json && ok "tags_block.json is stamped" \
                             || bad "tags_block.json is stamped" "no .prov"
run_gen "$R" --check
[ "$OUT_RC" -eq 0 ] && ok "--check exits 0 afterwards" \
                    || bad "--check exits 0 afterwards" "exit $OUT_RC: $OUT_TXT"

# ...and the arm really is what makes the difference: with the case removed
# from generator_for, the same tree refuses instead of guessing.
sed -i '/tags_block.json)        echo ci\/dump-block-tags.sh ;;/d' "$R/ci/gen-gameplay-tables.sh"
rm -f "$R/data/vanilla/.provenance/tags_block.json.prov"
run_gen "$R"
if [ "$OUT_RC" -ne 0 ] && case "$OUT_TXT" in *tags_block.json*) true ;; *) false ;; esac; then
    ok "without the arm the same tree refuses and names the table"
else bad "without the arm the same tree refuses and names the table" "exit $OUT_RC"; fi
echo

printf 'gen-tables-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || { echo "GEN-TABLES-FALSIFY RED" >&2; exit 1; }
echo "GEN-TABLES-FALSIFY GREEN"
