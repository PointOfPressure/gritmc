#!/usr/bin/env bash
# placed-golden.sh — extract `Block.setPlacedBy` per block state.
#
# `placement.json` answers "what state does this click put down", and for
# 1,040 of 1,054 block items that is the whole placement. It is not for a
# door or a bed: `BlockItem.place` calls `setPlacedBy` after the write, and
# four blocks use that call to put down a *second* block the placement state
# never mentioned. See `tools/goldengen/vanilla/PlacedGolden.java`.
#
# It shares `UseGolden`'s level, so both are compiled here. That is not
# convenience — the two dumpers ask the same question ("what `setBlock` calls
# does this call make") of the same world, and two levels that answer it
# differently would be a difference between the tables rather than between
# the mechanics.
#
# Same signature-stripping as `ci/use-golden.sh`, for the same reason: the
# dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed jar.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# both streams through log4j, which re-emits each line with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so `grep -E "^SOMETHING"`
# matches nothing and exits 1 — indistinguishable from a clean run that found
# nothing. That prefix is also why this dumper writes its JSON to stdout and
# is captured with `>` rather than through a pipe.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/set_placed_by.json}"

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

GATES="$ROOT/data/vanilla/server_level_gates.json"
mkdir -p "$(dirname "$GATES")"
"$ROOT/ci/server-level-gates.py" --emit >&2 || exit 1

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/UseGolden.java" \
    "$ROOT/tools/goldengen/vanilla/PlacedGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -Drustmc.gates="$GATES" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.PlacedGolden > "$1" 2> "$1.log" || true
    if [ "$(head -c 1 "$1" 2>/dev/null)" != "{" ]; then
        echo "PlacedGolden did not produce JSON; the last of what it said:" >&2
        head -20 "$1" >&2
        tail -30 "$1.log" >&2
        return 1
    fi
}

# Twice, and they must agree. `MossyCarpetBlock.setPlacedBy` calls
# `level.getRandom()`, so this is not a formality — a random block that
# slipped through as cells rather than as `"random": true` would churn this
# file on every regeneration.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of PlacedGolden disagree — something in it is random or iterates a map" >&2
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

# The regression oracle, and it is built to fail on the one mistake a
# consumer of this table would otherwise never notice.
#
# A door's second half is at a **constant** offset — always `above` — so a
# consumer that hard-coded `[0, 1, 0]` would pass every door test forever. A
# bed's is not: `BedBlock.setPlacedBy` writes to `pos.relative(FACING)`, so
# the offset is a function of the state. The bed assertion below therefore
# checks the offset *against the state's own facing*, and requires all four
# facings to be present and to disagree with each other. Two inputs that
# differ only in the thing being claimed.
#
# **Both rules are unconditional, and the first version of this oracle said
# otherwise.** It asserted that an `half=upper` door writes nothing, which is
# what a reasonable person assumes and is not what the 23 bytes of
# `DoorBlock.setPlacedBy` do: there is no branch on `HALF` at all, so an
# upper half handed to it writes another upper half above itself. The dumper
# was right and the oracle was wrong. It is left unconditional here because
# the table's job is to record the call, not to record when the call happens
# to be made — `getStateForPlacement` never returns an upper half, so a
# consumer that applies these rows *at placement time* never reaches them.
# A consumer that applied them anywhere else would stack doors to the sky,
# and that is a property of the consumer.
python3 - "$WORK/out.json" <<'PYEOF'
import json, sys, re

doc = json.load(open(sys.argv[1]))
rows = {r["block"]: r for r in doc["table"]}
bad = []


def props(state):
    m = re.search(r"\[(.*)\]", state)
    return dict(p.split("=", 1) for p in m.group(1).split(",")) if m else {}


door = rows.get("minecraft:oak_door")
if door is None:
    sys.exit("PlacedGolden recorded nothing for minecraft:oak_door")
doors = 0
for cell in door["states"]:
    if "refused" in cell:
        bad.append(f"door {cell['from']} refused: {cell['refused']}")
        continue
    p = props(cell["from"])
    writes = cell["writes"]
    doors += 1
    if len(writes) != 1 or writes[0]["at"] != [0, 1, 0]:
        bad.append(f"door {cell['from']} wrote {writes}, expected one write at [0, 1, 0]")
        continue
    got, want = props(writes[0]["state"]), dict(p, half="upper")
    if got != want:
        bad.append(f"door {cell['from']} -> {writes[0]['state']}, expected {want}")
if doors != 64:
    bad.append(f"{doors} oak_door states carried, expected all 64")

# `Direction.relative` as vanilla defines it, written out so the test does
# not borrow the consumer's idea of which way south is.
OFFSET = {"north": [0, 0, -1], "south": [0, 0, 1],
          "west": [-1, 0, 0], "east": [1, 0, 0]}
bed = rows.get("minecraft:red_bed")
if bed is None:
    sys.exit("PlacedGolden recorded nothing for minecraft:red_bed")
seen = {}
for cell in bed["states"]:
    if "refused" in cell:
        bad.append(f"bed {cell['from']} refused: {cell['refused']}")
        continue
    p = props(cell["from"])
    writes = cell["writes"]
    want_at = OFFSET[p["facing"]]
    if len(writes) != 1 or writes[0]["at"] != want_at:
        bad.append(f"bed {cell['from']} wrote {writes}, expected one write at {want_at}")
        continue
    seen[p["facing"]] = tuple(writes[0]["at"])
    got, want = props(writes[0]["state"]), dict(p, part="head")
    if got != want:
        bad.append(f"bed {cell['from']} -> {writes[0]['state']}, expected {want}")
if set(seen) != set(OFFSET):
    bad.append(f"bed facings covered: {sorted(seen)}, expected all four")
elif len(set(seen.values())) != 4:
    bad.append(f"bed offsets do not vary with facing: {seen}")

# **The world-sensitivity detector has to be able to fire.**
#
# It reports which rules give a different answer in water, and its first
# version reported zero — not because no rule reads water but because the
# only path that asks is `LevelReader.isWaterAt`, a tag lookup, and
# `Bootstrap.bootStrap()` binds no tags. A clean zero from a blind
# instrument is the exact shape this project keeps paying for, so the one
# block whose bytecode is known to read it is asserted rather than trusted:
# `SmallDripleafBlock.setPlacedBy` calls `DoublePlantBlock.copyWaterloggedFrom`,
# which is `isWaterAt` and nothing else.
dripleaf = rows.get("minecraft:small_dripleaf")
if dripleaf is None:
    bad.append("no row for minecraft:small_dripleaf, whose rule reads water")
elif not dripleaf.get("worldSensitive"):
    bad.append("small_dripleaf is not flagged worldSensitive — the water probe is blind, "
               "and every other zero it reports means nothing")
if doc["worldSensitiveBlocks"] < 1:
    bad.append("the water probe flagged nothing at all")

# **The class map has to cover the whole registry, and name the two classes
# rules are actually keyed on.**
#
# This is the project's single answer to "what kind of block is this", and it
# exists so no rule is ever keyed on a block's *name*. A map that is merely
# present is not enough: it is consumed with `.get(...)`, so a block missing
# from it answers "not a DoorBlock" — a false negative indistinguishable from
# a correct answer, and the un-press silently stops happening. That exact
# failure cost a corpus run: `class` was added to the use table and the table
# was not regenerated, so every lookup returned nothing and the button check
# did not move with no error anywhere.
classes = doc.get("classes") or {}
if len(classes) != doc.get("blocks"):
    bad.append(f"class map has {len(classes)} entries, `blocks` says {doc.get('blocks')}")
if len(classes) < 1000:
    bad.append(f"class map covers {len(classes)} blocks; 26.2's registry is ~1,196 — "
               "a partial map answers 'not that class' for everything it omits")
for block, want in (("minecraft:oak_door", "DoorBlock"),
                    ("minecraft:stone_button", "ButtonBlock"),
                    ("minecraft:red_bed", "BedBlock")):
    got = classes.get(block)
    if got != want:
        bad.append(f"class of {block} is {got!r}, expected {want!r} — a rule keys on this")

# **And the superclass chain, because the bytecode tests `instanceof`.**
#
# `DoorBlock.updateShape` asks `nb.getBlock() instanceof DoorBlock`. Four
# blocks answer yes without being a `DoorBlock` by name — the unwaxed copper
# doors are `WeatheringCopperDoorBlock` — so a consumer comparing simple names
# gives those four no door rule at all. The corpus only ever places an *oak*
# door, so no check would catch it. This is the assertion that does.
supers = doc.get("superclasses") or {}
if not supers:
    bad.append("no `superclasses` map — every instanceof test degrades to a name compare")
else:
    chain = supers.get("WeatheringCopperDoorBlock")
    if not chain or "DoorBlock" not in chain:
        bad.append(f"WeatheringCopperDoorBlock's chain is {chain!r}; it must contain DoorBlock, "
                   "or the four unwaxed copper doors silently get no door rule")
    resolved_doors = [b for b, c in classes.items()
                      if c == "DoorBlock" or "DoorBlock" in (supers.get(c) or [])]
    if len(resolved_doors) < 21:
        bad.append(f"{len(resolved_doors)} blocks resolve to DoorBlock; 26.2 has 21 "
                   "(17 DoorBlock + 4 WeatheringCopperDoorBlock)")

if bad:
    print("PlacedGolden disagrees with the bytecode:", file=sys.stderr)
    for b in bad[:10]:
        print("  " + b, file=sys.stderr)
    sys.exit(1)
print(f"  door oracle: {doors} oak_door states each write one upper half at [0, 1, 0]; "
      f"{len(resolved_doors)} blocks resolve to DoorBlock via instanceof")
print(f"  bed oracle:  four facings, four distinct head offsets {dict(sorted(seen.items()))}")
PYEOF

mv "$WORK/out.json" "$OUT"

# **Stamped, or a tree that followed the cold start fails T0.**
# `ci/check-use-probe-falsify.sh`'s `real-table` arm refuses a table with no
# provenance stamp and points at `ci/gen-gameplay-tables.sh` to repair it.
# `ci/use-item-on-golden.sh` grew this on 2026-08-18 for exactly that reason
# and the other two generators did not, so any tree that ran *this* script
# directly — which is what every cold start in `docs/cold-start/` tells you to
# do — had an unstamped table and a red gate whose advice was to run a
# different script. Found 2026-08-19 by following `server-level-gate.md`'s own
# command list from a fresh worktree.
if [ -f "$ROOT/ci/provenance.sh" ]; then
    . "$ROOT/ci/provenance.sh"
    prov_stamp "$ROOT" "$OUT" "ci/placed-golden.sh"
fi
python3 - "$OUT" <<'PYEOF'
import json, sys
d = json.load(open(sys.argv[1]))
print(f"setPlacedBy table -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['blocksWithBehaviour']} blocks with behaviour, "
      f"{d['statesWriting']} of {d['statesProbed']} states do something, "
      f"{d['refusals']} refusals, {d['randomBlocks']} random blocks dropped")
for k, v in list(d["refusedBy"].items())[:8]:
    print(f"    refused {v:5}  {k}")
for r in d["table"]:
    n = len(r["states"])
    print(f"    {r['block']:44} {'RANDOM' if r['random'] else str(n) + ' states'}")
PYEOF
