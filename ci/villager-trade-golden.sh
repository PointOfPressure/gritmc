#!/usr/bin/env bash
# villager-trade-golden.sh — every villager and wandering-trader trade listing.
#
#     ./ci/villager-trade-golden.sh [out-villager.json] [out-wandering.json]
#
# Produces `data/vanilla/villager_trades.json` and
# `data/vanilla/wandering_trader_trades.json` from the pinned 26.2 jar.
#
# In 26.2 a trade is a datapack registry entry, not a Java `ItemListing`:
# `minecraft:villager_trade` holds the listings, `minecraft:trade_set` holds
# one draw-specification per profession-level, and the pool each set draws from
# is a **tag** over the first registry. See the header of
# `tools/goldengen/vanilla/VillagerTradeGolden.java` for why that matters and
# what it refutes.
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`.
#
# # THE CROSS-CHECK IS INSIDE THE DUMPER, NOT BESIDE IT
#
# The same 529 files exist twice on this box — packed in the jar, and under
# `oracle-work/vanilla-<ver>/generated-data/`, which Mojang's own data
# generator wrote. The dumper reads both independently and refuses if any file
# is present in one and not the other or parses to a different tree. A dumper
# that read one source and compared it with itself would agree with itself
# every time, which is this repository's central defect class and the reason
# this paragraph is longer than the check.
#
# It then runs twice and the two outputs must be identical, for the same
# reason `ci/sound-types-golden.sh` does: the row order comes from a
# `TreeMap` over ids and from `BuiltInRegistries.VILLAGER_PROFESSION`'s
# iteration, and "that is stable" is a claim, not an observation, until two
# runs have been compared.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_V="${1:-$ROOT/data/vanilla/villager_trades.json}"
OUT_W="${2:-$ROOT/data/vanilla/wandering_trader_trades.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }
[ -d "$VDIR/generated-data" ] || {
    echo "no $VDIR/generated-data — the dumper's second source is absent and it will refuse" >&2
    exit 1
}

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT_V")" "$(dirname "$OUT_W")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/VillagerTradeGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.VillagerTradeGolden \
        "$UNSIGNED" "$VDIR/generated-data" "$1" "$2" 2>/dev/null
    test -s "$1" && test -s "$2"
}

run "$WORK/v.json" "$WORK/w.json" > "$WORK/log1"
run "$WORK/v2.json" "$WORK/w2.json" > /dev/null
cmp -s "$WORK/v.json" "$WORK/v2.json" && cmp -s "$WORK/w.json" "$WORK/w2.json" || {
    echo "two runs of VillagerTradeGolden disagree — the row order is not stable" >&2
    exit 1
}
cat "$WORK/log1"

mv "$WORK/v.json" "$OUT_V"
mv "$WORK/w.json" "$OUT_W"

python3 - "$OUT_V" "$OUT_W" <<'PYEOF'
import json, sys

v = json.load(open(sys.argv[1]))
w = json.load(open(sys.argv[2]))
fail = []

# The two tables partition one registry, so the pieces must add back up.
cc = v["crossCheck"]
if cc != w["crossCheck"]:
    fail.append("the two tables disagree about the cross-check they were both written from")
# The built-in `trade_rebalance` datapack ships 81 more listings and 10 tag
# overrides in the same jar under `data/minecraft/datapacks/trade_rebalance/`.
# It is off in a default world and a glob that swept `**/villager_trade/*.json`
# would merge it in silently, making every count here a mixture of an enabled
# pack and a disabled one. The dumper matches the prefix instead and refuses if
# that population moves; this is the reader's half of the same claim.
if cc.get("datapackEntriesExcluded") != 91:
    fail.append(f"the excluded datapack population is {cc.get('datapackEntriesExcluded')}, not 91")
if cc["mismatches"] != 0:
    fail.append(f"{cc['mismatches']} files differ between the jar and generated-data")
for a, b in (("jarTrades", "generatedTrades"),
             ("jarTradeSets", "generatedTradeSets"),
             ("jarTags", "generatedTags")):
    if cc[a] != cc[b]:
        fail.append(f"{a}={cc[a]} but {b}={cc[b]}")

for name, key in (("trades", "jarTrades"), ("tradeSets", "jarTradeSets"), ("tags", "jarTags")):
    total = v["counts"][name] + w["counts"][name]
    if total != cc[key]:
        fail.append(f"{name}: {v['counts'][name]} + {w['counts'][name]} = {total}, "
                    f"but the registry holds {cc[key]}")

# An EMPTY tag is not a defect and three of them ship: `common_smith/level_3`,
# `_4` and `_5` are `{"values": []}` in both sources, and the armorer,
# toolsmith and weaponsmith pools splice them in at those levels for nothing.
# What IS a defect is an empty pool reached by a trade set, and that is checked
# below against the sets rather than against every tag — which is also what a
# wrong flattener would produce on a `#`-only file, since `armorer/level_1`
# opens with `#minecraft:common_smith/level_1` and has no listing of its own
# before it.
empty = [t["id"] for t in v["tags"] + w["tags"] if not t["resolved"]]
if len(empty) != 3 or sorted(empty) != ["minecraft:common_smith/level_3",
                                        "minecraft:common_smith/level_4",
                                        "minecraft:common_smith/level_5"]:
    fail.append(f"empty pools moved: {sorted(empty)}")

# Every trade set must name a tag that exists on its own side of the split and
# resolve to at least as many listings as it draws.
oversubscribed = []
for table in (v, w):
    tags = {t["id"]: t["resolved"] for t in table["tags"]}
    ids = {t["id"] for t in table["trades"]}
    for s in table["tradeSets"]:
        ref = s["trades"]
        if not isinstance(ref, str) or not ref.startswith("#"):
            fail.append(f"trade set {s['id']} draws from {ref!r}, not a tag")
            continue
        tag = ref[1:]
        if ":" not in tag:
            tag = "minecraft:" + tag
        if tag not in tags:
            fail.append(f"trade set {s['id']} names tag {tag}, which is not in this table")
            continue
        for member in tags[tag]:
            if member not in ids:
                fail.append(f"tag {tag} names {member}, which is not a listing in this table")
        if not tags[tag]:
            fail.append(f"trade set {s['id']} draws from empty pool {tag}")
        # `addOffersFromItemListingsWithoutDuplicates` removes each pick, so a
        # set asking for more than its pool holds silently offers FEWER trades:
        # the loop's second guard is `list.isEmpty()`, not `i < count`. Every
        # shipped set omits `allow_duplicates` and so takes that path. Eight
        # sets are over-subscribed in 26.2 and that is vanilla's own data, not
        # a defect — but it is pinned, because a reader who sized an offer
        # array from `amount` would be wrong on exactly these eight.
        if not s["allowDuplicates"] and isinstance(s["amount"], (int, float)) \
                and s["amount"] > len(tags[tag]):
            oversubscribed.append(f"{s['id']} {s['amount']:g}>{len(tags[tag])}")

# The listings' own invariants. `max_uses` reaching the offer as 0 would make a
# trade permanently out of stock; `Math.max(..., 1)` in `VillagerTrade.getOffer`
# offsets 269..279 is why it cannot, and a 0 here would mean the default was
# not applied.
for table in (v, w):
    for t in table["trades"]:
        for field in ("wants", "gives", "maxUses", "reputationDiscount", "xp"):
            if t.get(field) is None:
                fail.append(f"{t['id']}: {field} is null")
        if isinstance(t["maxUses"], (int, float)) and t["maxUses"] < 1:
            fail.append(f"{t['id']}: maxUses {t['maxUses']}")
        if not isinstance(t["wants"], dict) or "id" not in t["wants"]:
            fail.append(f"{t['id']}: wants has no id")
        if not isinstance(t["gives"], dict) or "id" not in t["gives"]:
            fail.append(f"{t['id']}: gives has no id")

# 13 professions carry trade sets in 26.2; `none` and `nitwit` carry none. If
# this collapses to 0 the registry read failed and every villager would be a
# nitwit, which is a table that looks fine and is entirely wrong.
withtrades = [p for p in v["professions"] if p["tradeSetsByLevel"]]
if len(withtrades) < 13:
    fail.append(f"only {len(withtrades)} professions have trade sets")
for p in withtrades:
    missing = [str(n) for n in range(1, 6) if str(n) not in p["tradeSetsByLevel"]]
    if missing:
        fail.append(f"profession {p['id']} has no trade set for level(s) {','.join(missing)}")

if v["levelXpThresholds"][0]["maxXp"] != 10 or v["levelXpThresholds"][4]["maxXp"] != 0:
    fail.append("NEXT_LEVEL_XP_THRESHOLDS did not come through: level 1 max should be 10, level 5 0")

OVERSUBSCRIBED = [
    "minecraft:butcher/level_4 2>1",
    "minecraft:butcher/level_5 2>1",
    "minecraft:fisherman/level_4 2>1",
    "minecraft:librarian/level_5 3>2",
    "minecraft:shepherd/level_5 2>1",
    "minecraft:toolsmith/level_5 2>1",
    "minecraft:weaponsmith/level_3 2>1",
    "minecraft:weaponsmith/level_5 2>1",
]
if sorted(oversubscribed) != OVERSUBSCRIBED:
    fail.append("the over-subscribed trade sets moved: "
                f"{sorted(oversubscribed)} against the pinned {OVERSUBSCRIBED}")

print(f"  {len(oversubscribed)} trade sets ask for more listings than their pool holds "
      "and so offer fewer trades than `amount`")
print(f"  {v['counts']['trades']} villager listings in {v['counts']['tradeSets']} trade sets "
      f"over {v['counts']['tags']} pools, {len(withtrades)} professions x 5 levels")
print(f"  {w['counts']['trades']} wandering-trader listings in {w['counts']['tradeSets']} "
      f"trade sets over {w['counts']['tags']} pools")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
