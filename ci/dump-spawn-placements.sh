#!/usr/bin/env bash
# dump-spawn-placements.sh — `SpawnPlacements.<clinit>`, as
# `data/vanilla/spawn_placements.json`.
#
#     ./ci/dump-spawn-placements.sh        # ~2 s, javap only, no build, no box
#
# # What this is
#
# `NaturalSpawner.isValidSpawnPostitionForType` asks two questions of
# `SpawnPlacements`, and both are answered out of one `HashMap` populated by
# a single flat static initialiser:
#
#     SpawnPlacements.isSpawnPositionOk(type, level, pos)
#     SpawnPlacements.checkSpawnRules(type, level, NATURAL, pos, random)
#
# Each `register(type, placement, heightmap, predicate)` call compiles to a
# fixed five-instruction shape — three `getstatic`s, one `invokedynamic`
# whose bootstrap argument is a direct method handle to the predicate, and
# the `invokestatic register`. So the table is read by walking the
# initialiser and joining each `invokedynamic` to `BootstrapMethods`, which
# is what this script does. There is no ambiguity to resolve and no
# devirtualisation: `LambdaMetafactory`'s second bootstrap argument is the
# exact `REF_invokeStatic` target.
#
# **A reflection dump was considered and rejected.** `DATA_BY_TYPE` is a
# `HashMap` keyed on `EntityType` objects, so its iteration order is
# identity-hash order — different on every JVM run — and `docs/AGENT-BRIEF.md`
# and this project's golden rules both refuse a golden carrying an identity
# hash. javap gives the source order instead, which is stable, and this file
# then sorts by registry name so that a diff between two versions reads as a
# diff.
#
# # Registration order is NOT behaviour, and that is why it is not the output order
#
# The map is only ever read by `get(type)`. Nothing iterates it. So the
# order the 83 registrations appear in is a fact about the source file and
# not about the game, and emitting it would invite a later reader to depend
# on it. The `index` field is kept per row purely so that a bytecode diff
# can be located; nothing may key off it.
#
# # The cross-checks
#
# * every field name must lowercase to a key of `minecraft:entity_type` in
#   `generated/reports/registries.json` — a second source, produced by a
#   vanilla server run rather than by this parse. All 83 do on 26.2;
# * the registration count must equal the number of `invokestatic register`
#   sites in the initialiser, so a shape this parser fails to recognise is a
#   refusal rather than a short table;
# * `NO_RESTRICTIONS` and `MOTION_BLOCKING_NO_LEAVES` are the two defaults
#   `getPlacementType`/`getHeightmapType` return for an UNREGISTERED type,
#   and both are emitted so a loader does not have to hardcode them.
#
# The output is gitignored like the rest of `data/vanilla/`. Regenerate on a
# version bump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
REPORT="$VDIR/generated/reports/registries.json"
OUT="${1:-$ROOT/data/vanilla/spawn_placements.json}"

[ -f "$JAR" ]    || { echo "dump-spawn-placements: no jar at $JAR" >&2; exit 1; }
[ -f "$REPORT" ] || { echo "dump-spawn-placements: no report at $REPORT — run ci/vanilla-server.sh reports" >&2; exit 1; }
command -v javap >/dev/null || { echo "dump-spawn-placements: javap not on PATH (JDK 25)" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
unzip -q -o "$JAR" 'net/minecraft/world/entity/SpawnPlacements*.class' -d "$WORK"
javap -p -v -constants -cp "$WORK" net.minecraft.world.entity.SpawnPlacements > "$WORK/sp.txt"

python3 - "$WORK/sp.txt" "$REPORT" "$OUT" <<'PY'
import json, re, sys

dump, report, out = sys.argv[1], sys.argv[2], sys.argv[3]
lines = open(dump).read().splitlines()

with open(report) as f:
    known_types = set(json.load(f)["minecraft:entity_type"]["entries"])

# --- BootstrapMethods: index -> the predicate's declaring class and name.
# The metafactory's arguments are (samMethodType, implMethod, instantiatedType);
# implMethod is the only REF_invokeStatic among them, so the first one after
# the index line is it.
try:
    b0 = next(i for i, l in enumerate(lines) if l.startswith("BootstrapMethods:"))
except StopIteration:
    sys.exit("dump-spawn-placements: no BootstrapMethods table — javap output changed")
bsm, cur = {}, None
for ln in lines[b0:]:
    m = re.match(r"\s+(\d+): #\d+ REF_invokeStatic", ln)
    if m:
        cur = int(m.group(1))
        continue
    m = re.search(r"REF_invokeStatic (net/minecraft/\S+?)\.(\w+):", ln)
    if m and cur is not None and cur not in bsm:
        bsm[cur] = (m.group(1).replace("/", "."), m.group(2))

# --- the static initialiser.
try:
    s0 = next(i for i, l in enumerate(lines) if l.strip() == "static {};")
except StopIteration:
    sys.exit("dump-spawn-placements: no static initialiser — javap output changed")
body = lines[s0:b0]

FIELD = r"getstatic\s+#\d+\s+// Field {}\.(\w+):"
RE_TYPE = re.compile(FIELD.format(re.escape("net/minecraft/world/entity/EntityTypes")))
RE_PLACE = re.compile(FIELD.format(re.escape("net/minecraft/world/entity/SpawnPlacementTypes")))
RE_HEIGHT = re.compile(FIELD.format(re.escape("net/minecraft/world/level/levelgen/Heightmap$Types")))
RE_INDY = re.compile(r"invokedynamic\s+#\d+,\s+0\s+// InvokeDynamic #(\d+):test:")
RE_REG = re.compile(r"invokestatic\s+#\d+\s+// Method register:")

# The denominator, counted before the walk so that a registration the walk
# fails to assemble is a refusal and not a short table.
sites = sum(1 for ln in body if RE_REG.search(ln))
if sites == 0:
    sys.exit("dump-spawn-placements: the initialiser holds no register() call")

rows, pend = [], {}
for ln in body:
    for key, rx in (("type", RE_TYPE), ("placement", RE_PLACE), ("heightmap", RE_HEIGHT)):
        m = rx.search(ln)
        if m:
            pend[key] = m.group(1)
            break
    else:
        m = RE_INDY.search(ln)
        if m:
            pend["bsm"] = int(m.group(1))
            continue
        if not RE_REG.search(ln):
            continue
        if set(pend) != {"type", "placement", "heightmap", "bsm"}:
            sys.exit(f"dump-spawn-placements: register() at an unrecognised shape, "
                     f"collected {sorted(pend)}")
        if pend["bsm"] not in bsm:
            sys.exit(f"dump-spawn-placements: bootstrap #{pend['bsm']} has no "
                     "REF_invokeStatic target")
        cls, meth = bsm[pend["bsm"]]
        name = "minecraft:" + pend["type"].lower()
        if name not in known_types:
            sys.exit(f"dump-spawn-placements: EntityTypes.{pend['type']} lowercases to "
                     f"{name}, which is not in minecraft:entity_type")
        rows.append({"type": name, "index": len(rows),
                     "placement": pend["placement"], "heightmap": pend["heightmap"],
                     "predicate_class": cls, "predicate": meth})
        pend = {}

if len(rows) != sites:
    sys.exit(f"dump-spawn-placements: {sites} register() sites but {len(rows)} assembled")

rows.sort(key=lambda r: r["type"])
predicates = sorted({r["predicate_class"].rsplit(".", 1)[-1] + "." + r["predicate"] for r in rows})
with open(out, "w") as f:
    json.dump({
        # What getPlacementType/getHeightmapType answer for a type with NO
        # entry in the map. Both are read by isValidSpawnPostitionForType for
        # any type a datapack adds, so a loader that hardcodes them has two
        # copies of one fact.
        "default_placement": "NO_RESTRICTIONS",
        "default_heightmap": "MOTION_BLOCKING_NO_LEAVES",
        "placements": rows,
    }, f, indent=0)

print(f"{len(rows)} registrations, {len(predicates)} distinct spawn predicates -> {out}")
PY
