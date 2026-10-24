#!/usr/bin/env bash
# fire-odds-golden.sh — `FireBlock`'s ignite/burn odds, off the pinned jar.
#
#     ./ci/fire-odds-golden.sh [out.json]     # default data/vanilla/fire_odds.json
#
# See `tools/goldengen/vanilla/FireOddsGolden.java` for what the two maps are
# and why they cannot be written by hand or derived from block names.
#
# # THE CROSS-CHECK, AND WHY IT IS NOT A NUMBER IN THIS FILE
#
# The dumper reflects a runtime map. A runtime map cannot tell you that a key
# was put twice, and it cannot tell you that a `setFlammable` call site was
# skipped by a branch nobody read — both come out as a shorter table, and a
# shorter table of a thing nobody has counted reads exactly like the right
# answer. So the expected size is re-derived from the **bytecode** on every
# run, by a route that shares nothing with the JVM walk:
#
#   direct   = `Method setFlammable` call sites inside `bootStrap()`'s own body
#   sweeps   = `ColorCollection.forEach` call sites inside that same body
#   colors   = `Consumer.accept` call sites inside `ColorCollection.forEach`
#   expected = direct + sweeps * colors
#
# On the pinned 26.2 jar that is 175 + 2 * 16 = **207**, and this script
# prints all four so a change in any of them is attributable. A written-down
# 207 would go stale at the next version and read as authority; a derived one
# fails loudly.
#
# **A mismatch is refused, not reported.** `ci/gen-gameplay-tables.sh` stamps
# whatever this writes and `crate::fire` loads it without re-checking, so a
# short table would make blocks silently non-flammable — which is the same
# output as a correct server standing next to a stone wall. That is the
# unfalsifiable class this project keeps paying for, and the refusal is the
# only place it can be caught.
#
# Run twice and `cmp`ed, like `ci/block-items-golden.sh`: a registry walk over
# a fixed jar has no business varying between runs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/fire_odds.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# --- the bytecode arm -------------------------------------------------------
#
# `javap` over the two classes, sliced to the one method each. The slice
# matters: `lambda$bootStrap$0` and `lambda$bootStrap$1` each hold a
# `setFlammable` call of their own, and counting them as direct sites would
# over-count by exactly two while the sweeps under-counted by thirty-two — a
# pair of errors that nearly cancel, which is the worst kind.
mkdir -p "$WORK/x"
( cd "$WORK/x" && unzip -o -q "$JAR" \
    'net/minecraft/world/level/block/FireBlock.class' \
    'net/minecraft/world/level/block/ColorCollection.class' )

slice() { # slice <file> <method-signature-prefix>
    awk -v sig="$2" 'index($0, sig) == 1 { inm = 1; next }
                     inm && /^  [a-zA-Z]/ { inm = 0 }
                     inm { print }' "$1"
}

javap -p -c -constants -cp "$WORK/x" net.minecraft.world.level.block.FireBlock > "$WORK/fire.txt"
javap -p -c -constants -cp "$WORK/x" net.minecraft.world.level.block.ColorCollection > "$WORK/color.txt"

BOOT="$(slice "$WORK/fire.txt" '  public static void bootStrap();')"
FOREACH="$(slice "$WORK/color.txt" '  public void forEach(java.util.function.Consumer')"

DIRECT="$(printf '%s\n' "$BOOT" | grep -c 'Method setFlammable' || true)"
SWEEPS="$(printf '%s\n' "$BOOT" | grep -c 'Method net/minecraft/world/level/block/ColorCollection.forEach' || true)"
COLORS="$(printf '%s\n' "$FOREACH" | grep -c 'InterfaceMethod java/util/function/Consumer.accept' || true)"

# A slice that matched nothing yields 0, and 0 + 0 * 0 = 0, which would then
# "agree" with an empty dump. Refuse each component on its own.
[ "$DIRECT" -gt 0 ] || { echo "REFUSED  read 0 setFlammable sites in bootStrap(); the javap slice has gone stale" >&2; exit 3; }
[ "$COLORS" -gt 0 ] || { echo "REFUSED  read 0 accept sites in ColorCollection.forEach; the javap slice has gone stale" >&2; exit 3; }
EXPECTED=$(( DIRECT + SWEEPS * COLORS ))

# --- the JVM arm ------------------------------------------------------------
javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/FireOddsGolden.java" \
    2>&1 | grep -v '^Note' || true

run() {
    java -cp "$CP:$WORK" FireOddsGolden 2>/dev/null | sed 's/.*\[STDOUT\]: //' > "$1"
}
run "$WORK/a.txt"
run "$WORK/b.txt"
cmp -s "$WORK/a.txt" "$WORK/b.txt" || { echo "REFUSED  two runs of the registry walk differ" >&2; exit 3; }

python3 - "$WORK/a.txt" "$OUT" "$EXPECTED" "$DIRECT" "$SWEEPS" "$COLORS" <<'PY'
import json, sys

src, out, expected = sys.argv[1], sys.argv[2], int(sys.argv[3])
direct, sweeps, colors = (int(x) for x in sys.argv[4:7])

rows, count = {}, None
for line in open(src):
    parts = line.rstrip("\n").split("\t")
    if parts[0] == "ROW":
        rows[parts[1]] = (int(parts[2]), int(parts[3]), parts[4])
    elif parts[0] == "COUNT":
        count = tuple(int(x) for x in parts[1:5])

if count is None:
    raise SystemExit("REFUSED  no COUNT line; the dumper did not finish")
if not rows:
    raise SystemExit("REFUSED  the registry walk produced no flammable blocks")

blocks, reported, ignite_size, burn_size = count
if len(rows) != reported:
    raise SystemExit(f"REFUSED  {len(rows)} rows against a reported {reported}")
# `setFlammable` writes both maps in one body, so a block in one and not the
# other cannot happen today. Asserted rather than assumed, because if it ever
# does the two-column table below would quietly carry a -1.
if ignite_size != burn_size:
    raise SystemExit(f"REFUSED  igniteOdds holds {ignite_size} keys, burnOdds {burn_size}")
if any(p != "11" for _, _, p in rows.values()):
    missing = sorted(b for b, (_, _, p) in rows.items() if p != "11")
    raise SystemExit(f"REFUSED  {len(missing)} blocks are in one map only: {missing[:5]}")
# A key in the map but not in the registry: the walk is over the registry, so
# such a block would be dropped silently.
if ignite_size != reported:
    raise SystemExit(
        f"REFUSED  {ignite_size} map keys against {reported} registry rows — "
        "a flammable block is not in BuiltInRegistries.BLOCK"
    )
# THE CROSS-CHECK. A duplicate key, a skipped call site or a widened sweep all
# land here.
if len(rows) != expected:
    raise SystemExit(
        f"REFUSED  {len(rows)} rows against {expected} expected from the bytecode "
        f"({direct} direct setFlammable sites + {sweeps} ColorCollection.forEach "
        f"sweeps x {colors} colours)"
    )

table = {b: {"ignite": i, "burn": u} for b, (i, u, _) in sorted(rows.items())}
json.dump(
    {
        "what": "FireBlock.igniteOdds / FireBlock.burnOdds, per block, after FireBlock.bootStrap()",
        "blocksInRegistry": blocks,
        "flammableBlocks": len(table),
        "bytecodeExpected": expected,
        "directSetFlammableSites": direct,
        "colorCollectionSweeps": sweeps,
        "colorsPerSweep": colors,
        "odds": table,
    },
    open(out, "w"),
    indent=1,
)
print(
    f"fire odds -> {out} ({len(table)} of {blocks} blocks flammable; "
    f"cross-checked against {direct} + {sweeps}x{colors} = {expected} bytecode sites)"
)
PY
