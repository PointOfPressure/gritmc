#!/usr/bin/env bash
# use-item-on-golden.sh — extract `BlockState.useItemOn` per state and item.
#
# The other half of the right-click. `ci/use-golden.sh` extracts offset 196 of
# `ServerPlayerGameMode.useItemOn` — the empty-handed offer — and this extracts
# offset 145, the one that runs first and is handed the stack. See
# `tools/goldengen/vanilla/UseItemOnGolden.java` for what the probe is, which
# blocks it asks, and how the set it does not ask is falsified.
#
# **Why this is a separate script and not a flag on `ci/use-golden.sh`.** The
# two tables are generated from the same harness and stamped separately because
# `ci/gen-gameplay-tables.sh` maps one table to one generator and re-runs a
# generator whose table is stale. One script writing two tables makes either
# one's staleness re-derive both, and the `useItemOn` grid is 84 million probes
# against the other's 1.2 million.
#
# Same signature-stripping as `ci/use-golden.sh`, for the same reason: the
# dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed jar.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# both streams through log4j, which re-emits each line with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so `grep -E "^SOMETHING"`
# matches nothing and exits 1 — indistinguishable from a clean run that found
# nothing. That prefix is also why this dumper writes its JSON to stdout and is
# captured with `>` rather than through a pipe.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/use_item_on.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

# The `instanceof ServerLevel` gates, derived from the bytecode before the
# dumper runs. Same order and same reason as `ci/use-golden.sh`: the fact
# cannot be observed from inside a run, `--emit` refuses if
# `ci/checks/server-level-gates.toml` does not describe what the jar does, and
# a stale ledger has to stop the pipeline before a table is written with a
# wrong gate column rather than after.
"$ROOT/ci/server-level-gates.py" --emit >&2 || exit 1
GATES="$ROOT/data/vanilla/server_level_gates.json"

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
# `UseGolden` too, and not because this needs its `main`: the level, the
# player, the stance sweep, the tick sinks and the component/tag binding all
# live there and are shared deliberately. Two copies of "construct the probe"
# is how the two tables would end up describing two different players while
# both called it `base`.
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/UseGolden.java" \
    "$ROOT/tools/goldengen/vanilla/UseItemOnGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    # stderr kept, not discarded — a refusal recorded in a row and a failure to
    # start both land here and the two look nothing alike.
    java -Xmx6g -Drustmc.data="$VDIR/generated-data" -Drustmc.gates="$GATES" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.UseItemOnGolden > "$1" 2> "$1.log" || true
    if [ "$(head -c 1 "$1" 2>/dev/null)" != "{" ]; then
        echo "UseItemOnGolden did not produce JSON; the last of what it said:" >&2
        head -20 "$1" >&2
        tail -30 "$1.log" >&2
        return 1
    fi
}

# Twice, and they must agree. Same check as `ci/use-golden.sh` and for the same
# reason: `BuiltInRegistries.BLOCK` and `.ITEM` iterate in registration order
# and are stable, but a map iterated anywhere in the probe would not be. This
# dumper groups its cells through a `LinkedHashMap` keyed on the answer, which
# is insertion-ordered by construction — this is how that stays true rather
# than being asserted.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of UseItemOnGolden disagree — something in it iterates a map" >&2
    echo "  sizes: $(stat -c%s "$WORK/out.json") and $(stat -c%s "$WORK/again.json")" >&2
    python3 - "$WORK/out.json" "$WORK/again.json" >&2 <<'PYDIFF'
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
for k in a:
    if k != "table" and a[k] != b[k]:
        print(f"  summary field {k}: {a[k]!r} vs {b[k]!r}")
ra = {r["block"]: r for r in a["table"]}
rb = {r["block"]: r for r in b["table"]}
for k in sorted(set(ra) | set(rb)):
    if ra.get(k) != rb.get(k):
        print(f"  block {k} differs")
PYDIFF
    exit 1
}

# THE ORACLES. Three, and each one is a thing that has silently not worked.
#
#  1. **The flower pot**, which is this table's `minecraft:lever`: the one
#     block whose `useItemOn` is known end to end from its own bytecode.
#     `FlowerPotBlock.useItemOn` looks the held item's block up in
#     `POTTED_BY_CONTENT`, refuses if the pot is already full, and otherwise
#     writes the potted state with flags 3 and consumes one item. If the
#     extraction does not reproduce that, the extraction is wrong.
#  2. **The note block**, which is the *tag* oracle. Its whole rule is
#     `stack.is(ItemTags.NOTE_BLOCK_TOP_INSTRUMENTS)`, and an unbound tag
#     registry does not throw — it answers false for every item in the game.
#     A table generated without `UseGolden.bindItemTags` is uniformly, silently
#     "does nothing" here and nowhere else says so.
#  3. **No `Components not bound yet` anywhere, and no unavailable stance.**
#     Both are the same defect: without bound components there is no
#     `ItemStack` to hold, so this table could not exist and
#     `use_without_item.json` carried 39 refusals. If either comes back, the
#     binding has stopped working and every row is a reading of that.
python3 - "$WORK/out.json" <<'PYEOF'
import json, sys
doc = json.load(open(sys.argv[1]))
rows = {r["block"]: r for r in doc["table"]}
bad = []

def cells(block):
    """(item -> cell) for the block's single state, defaults expanded lazily."""
    row = rows.get(block)
    if row is None or not row["states"]:
        return None, None
    st = row["states"][0]
    by = {}
    for group in st["except"]:
        for item in group["items"]:
            by[item] = group["cell"]
    return st["default"], by

default, by = cells("minecraft:flower_pot")
if by is None:
    bad.append("no row for minecraft:flower_pot, which is the one block we can check")
else:
    if default.get("result") != "TRY_WITH_EMPTY_HAND" or default.get("writes"):
        bad.append("flower_pot's default answer is %r, expected an inert TRY_WITH_EMPTY_HAND"
                   % default)
    dand = by.get("minecraft:dandelion")
    if dand is None:
        bad.append("flower_pot says nothing special to a dandelion; POTTED_BY_CONTENT is "
                   "not being read, or the item axis is not being swept")
    else:
        w = dand.get("writes") or []
        if len(w) != 1 or w[0]["at"] != [0, 0, 0]:
            bad.append("flower_pot + dandelion wrote %r, expected one write at its own position"
                       % w)
        elif "minecraft:potted_dandelion" not in w[0]["state"]:
            bad.append("flower_pot + dandelion -> %s, expected potted_dandelion" % w[0]["state"])
        elif w[0]["flags"] != 3:
            bad.append("flower_pot + dandelion wrote with flags %d, expected 3" % w[0]["flags"])
        if dand.get("result") != "SUCCESS":
            bad.append("flower_pot + dandelion returned %r, expected SUCCESS" % dand.get("result"))
        if dand.get("stackAfter") != "empty":
            bad.append("flower_pot + dandelion left the stack as %r; `stack.consume(1, player)` "
                       "is the last line of that rule and a consumer that skips it duplicates "
                       "the flower" % dand.get("stackAfter"))
    potted_default, potted_by = cells("minecraft:potted_dandelion")
    if potted_by is None:
        bad.append("no row for minecraft:potted_dandelion")
    elif (potted_by.get("minecraft:dandelion") or {}).get("result") != "CONSUME":
        bad.append("potted_dandelion + dandelion answered %r, expected CONSUME — a full pot "
                   "refuses without writing"
                   % (potted_by.get("minecraft:dandelion") or {}).get("result"))

note = rows.get("minecraft:note_block")
if note is None or not note["states"]:
    bad.append("no row for minecraft:note_block — item tags are almost certainly unbound, "
               "in which case `ItemStack.is(TagKey)` is false everywhere and this table is "
               "silently blind wherever a tag decides a rule")
else:
    named = {i for st in note["states"] for g in st["except"] for i in g["items"]}
    if not named:
        bad.append("note_block distinguishes no item at all; #minecraft:note_block_top_instruments "
                   "is empty, so the item tag registry did not bind")

if doc.get("probeStancesUnavailable"):
    bad.append("stances this run could not set up: %s. Every one of them is an axis reported "
               "clean on no evidence." % doc["probeStancesUnavailable"])
for why, n in doc.get("refusedBy", {}).items():
    if "Components not bound" in why:
        bad.append("%d cells refused on %r — UseGolden.bindItemComponents did not take" % (n, why))

if bad:
    print("UseItemOnGolden disagrees with what its own bytecode says:", file=sys.stderr)
    for b in bad[:10]:
        print("  " + b, file=sys.stderr)
    sys.exit(1)
print("  pot oracle:   flower_pot + dandelion -> potted_dandelion, flags 3, stack consumed")
print("  tag oracle:   note_block names %d items out of the instrument tag"
      % len({i for st in rows["minecraft:note_block"]["states"]
             for g in st["except"] for i in g["items"]}))
PYEOF

# THE REFUSAL. Identical in kind to `ci/use-golden.sh`'s, and pointed at this
# table's own allowlist. A row whose answer is a reading of the probe's player
# rather than of the block is this project's signature defect in a golden
# table; `UseItemOnGolden` sweeps the same eleven stances `UseGolden` does and
# this refuses to write the table when the result is something
# `ci/checks/use-item-on-probe-dependent.toml` does not already know.
#
# Deliberately BEFORE the `mv`, and deliberately not a warning.
"$ROOT/ci/check-use-probe.py" "$WORK/out.json" \
    "$ROOT/ci/checks/use-item-on-probe-dependent.toml" || exit 1

mv "$WORK/out.json" "$OUT"

# STAMP WHAT WE WROTE, because nothing else will.
#
# Every other golden table is stamped by `ci/gen-gameplay-tables.sh` after it
# calls the generator. This one has no automated caller **by design** — its
# arm in that script only fires when something under `crates/` names the table,
# and nothing does — so a tree that runs this script by hand ends up with a
# table that is genuinely its own and carries no proof of it.
#
# That is not a cosmetic gap. `ci/check-use-probe-falsify.sh`'s
# `real-table-use-item-on` arm refuses an unstamped table and **T0 goes red**,
# pointing at `ci/gen-gameplay-tables.sh` — which will not generate this table
# and so cannot repair it. Found 2026-08-18 by generating it to measure what
# refusing a block entity costs: the only documented route to that measurement
# reddened the gate, and the advice in the red was a script that does nothing
# about it.
if [ -f "$ROOT/ci/provenance.sh" ]; then
    . "$ROOT/ci/provenance.sh"
    prov_stamp "$ROOT" "$OUT" "ci/use-item-on-golden.sh"
fi
python3 - "$OUT" <<'PYEOF'
import json, sys
d = json.load(open(sys.argv[1]))
print(f"useItemOn table -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['blocksProbed']} blocks over {d['statesProbed']} states x {d['itemsProbed']} items "
      f"= {d['gridProbes']} probes; {d['blocksSkipped']} blocks falsified in "
      f"{d['controlProbes']} more")
print(f"  {d['blocksWithBehaviour']} blocks with behaviour, {d['statesRecorded']} states "
      f"recorded, {d['cells']} (state, item) cells differ from their state's default")
print(f"  {d['probeDependentBlocks']} probe-dependent, {d['faceSensitiveBlocks']} face-sensitive, "
      f"{d['handSensitiveBlocks']} hand-sensitive, {d['refusedStates']} states refused outright, {d['partlyRefusedStates']} in part")
for k, v in list(d["refusedBy"].items())[:8]:
    print(f"    refused {v:5}  {k}")
PYEOF
