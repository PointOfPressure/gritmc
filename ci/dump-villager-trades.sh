#!/usr/bin/env bash
# dump-villager-trades.sh — every villager trade, the pool each level draws
# from, and how many it draws, out of vanilla's own datapack.
#
#     ./ci/dump-villager-trades.sh [out.json]
#
# Produces `data/vanilla/villager_trade_registry.json`.
#
# # This is a version where the brief was wrong, and the difference is large
#
# Villager trades used to be `VillagerTrades.ItemListing` lambdas compiled into
# the jar: to read them you had to instantiate each one against a seeded
# `RandomSource` and record what came out. In 26.2 they are **registry data**.
# `net.minecraft.world.item.trading.VillagerTrades` is now a list of
# `ResourceKey<VillagerTrade>` constants and nothing else, and the trades
# themselves live in the datapack:
#
#   data/minecraft/villager_trade/<profession>/<level>/<name>.json   388 files
#   data/minecraft/tags/villager_trade/<profession>/level_N.json      73 files
#   data/minecraft/trade_set/<profession>/level_N.json                68 files
#
# So there is no RNG to seed and no lambda to instantiate: the table is exact,
# it is read rather than derived, and a JVM is not involved at any point. The
# `trade_set` is what ties them together — `{amount, random_sequence, trades}`,
# where `trades` is a tag reference and `amount` is how many of that pool a
# villager of that level is given.
#
# The one thing that is still RNG is the *selection*, and this file does not
# make it: `random_sequence` names the sequence a server draws from, which is a
# separate mechanism (`RandomSequences`) and a separate item.
#
# Numbers are kept as they appear. `max_uses`, `reputation_discount` and `xp`
# are `NumberProvider`s in the codec and could in principle be `uniform` or
# `binomial` objects; measured on this extract, all 1,026 of them are plain
# scalars, and the script refuses rather than flattening one that is not.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/villager_trade_registry.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
DATA="$ROOT/oracle-work/vanilla-$VER/generated-data/data/minecraft"
[ -d "$DATA/villager_trade" ] || {
    echo "dump-villager-trades: no datapack extract at $DATA — run ci/vanilla-server.sh data first" >&2
    exit 1
}

mkdir -p "$(dirname "$OUT")"
python3 - "$DATA" "$OUT" <<'PY'
import json, os, sys

data, out = sys.argv[1], sys.argv[2]
fail = []


def ids(root):
    """Every json under `root`, keyed by its registry path."""
    got = {}
    base = os.path.join(data, root)
    for dirpath, _, files in os.walk(base):
        for f in sorted(files):
            if not f.endswith(".json"):
                continue
            p = os.path.join(dirpath, f)
            key = os.path.relpath(p, base)[: -len(".json")]
            got[key] = json.load(open(p))
    return got


def scalar(v, what):
    # A NumberProvider that is not a bare number would silently become one if
    # this cast were unchecked, and the trade would then be wrong in a way no
    # later check reads.
    if not isinstance(v, (int, float)) or isinstance(v, bool):
        fail.append(f"{what} is not a scalar: {v!r}")
        return 0
    return v


def cost(c, what):
    if not isinstance(c, dict) or "id" not in c:
        fail.append(f"{what} is not an item cost: {c!r}")
        return None
    o = {"id": c["id"], "count": int(scalar(c.get("count", 1), f"{what}.count"))}
    if "components" in c:
        # Kept verbatim. One trade has one (the wandering trader's water
        # bottle) and modelling a component predicate here would be a second,
        # worse copy of the item layer.
        o["components"] = c["components"]
    return o


trades_raw = ids("villager_trade")
tags_raw = ids("tags/villager_trade")
sets_raw = ids("trade_set")

trades = {}
for key, d in trades_raw.items():
    t = {
        "wants": cost(d["wants"], f"{key}.wants"),
        "gives": cost(d["gives"], f"{key}.gives"),
        "max_uses": int(scalar(d.get("max_uses", 4.0), f"{key}.max_uses")),
        "reputation_discount": float(
            scalar(d.get("reputation_discount", 0.0), f"{key}.reputation_discount")
        ),
        "xp": int(scalar(d.get("xp", 1.0), f"{key}.xp")),
    }
    if "additional_wants" in d:
        t["additional_wants"] = cost(d["additional_wants"], f"{key}.additional_wants")
    # Kept verbatim rather than modelled: these are loot functions and
    # predicates, and a rounded-off version here would be a wrong row in the
    # table that is supposed to say we are wrong.
    for k in ("given_item_modifiers", "merchant_predicate", "double_trade_price_enchantments"):
        if k in d:
            t[k] = d[k]
    trades[key] = t

# Tags nest. `toolsmith/level_4` is `["#minecraft:common_smith/level_4", ...]`
# — a tag entry beginning with `#` is a reference to another tag, and eight of
# the 73 pools use one. A reader that treats those as trade ids ends up with a
# pool smaller than the `amount` its trade set draws, which is what the
# consistency check below caught on the first run. Resolved transitively, in
# vanilla's own order: a referenced tag's members are spliced in where the
# reference sits.
def resolve(key, seen):
    if key in seen:
        fail.append(f"tag {key} is cyclic through {sorted(seen)}")
        return []
    d = tags_raw.get(key)
    if d is None:
        fail.append(f"tag {key} does not exist")
        return []
    out = []
    for v in d["values"]:
        if not isinstance(v, str):
            fail.append(f"tag {key}: unexpected entry {v!r}")
        elif v.startswith("#minecraft:"):
            out.extend(resolve(v[len("#minecraft:") :], seen | {key}))
        elif v.startswith("minecraft:"):
            out.append(v[len("minecraft:") :])
        else:
            fail.append(f"tag {key}: unexpected entry {v!r}")
    return out


tags = {key: resolve(key, frozenset()) for key in tags_raw}

sets = {}
for key, d in sets_raw.items():
    ref = d["trades"]
    if not ref.startswith("#minecraft:"):
        fail.append(f"trade_set {key}: `trades` is {ref!r}, not a #minecraft: tag")
        continue
    sets[key] = {
        "amount": int(scalar(d["amount"], f"{key}.amount")),
        # `TradeSet.allow_duplicates`, default false in the codec.
        "allow_duplicates": bool(d.get("allow_duplicates", False)),
        "random_sequence": d["random_sequence"],
        "tag": ref[len("#minecraft:") :],
    }

# Every tag entry must name a trade that exists, and every trade must be
# reachable from some tag. Both directions, because each catches a different
# kind of extract error and a one-directional check would pass on half of them.
for key, vals in tags.items():
    for v in vals:
        if v not in trades:
            fail.append(f"tag {key} names {v}, which is not a trade")
reachable = {v for vals in tags.values() for v in vals}
orphans = sorted(set(trades) - reachable)
if orphans:
    fail.append(f"{len(orphans)} trades are in no tag: {orphans[:5]}")
for key, s in sets.items():
    if s["tag"] not in tags:
        fail.append(f"trade_set {key} points at tag {s['tag']}, which does not exist")


doc = {
    "source": f"{os.path.basename(os.path.dirname(data))} datapack extract, read not derived",
    "trades": dict(sorted(trades.items())),
    "tags": dict(sorted(tags.items())),
    "trade_sets": dict(sorted(sets.items())),
}
with open(out, "w") as f:
    json.dump(doc, f, indent=0, sort_keys=True)

print(f"villager trades -> {out}")
print(f"  {len(trades)} trades, {len(tags)} pools, {len(sets)} trade sets")
professions = sorted({k.split('/')[0] for k in sets})
print(f"  {len(professions)} professions with trade sets: {', '.join(professions)}")
print(f"  {sum(len(v) for v in tags.values())} tag entries, "
      f"{sum(s['amount'] for s in sets.values())} offers dealt across every set")
# Eight sets ask for more than their pool holds, and that is legal rather than
# a defect: `AbstractVillager.addOffersFromItemListingsWithoutDuplicates` stops
# when the pool empties. Reported so the number is a measurement rather than a
# silent truncation, and because a set that asked for more than its pool while
# `allow_duplicates` was FALSE and the loop did NOT stop would deal a
# duplicate.
short = {k: (s["amount"], len(tags.get(s["tag"], []))) for k, s in sets.items()
         if s["amount"] > len(tags.get(s["tag"], []))}
dupes = sorted(k for k, s in sets.items() if s["allow_duplicates"])
print(f"  {len(short)} sets ask for more than their pool holds "
      f"(legal: the deal loop stops when the pool empties): "
      f"{', '.join(f'{k} {a}/{p}' for k, (a, p) in sorted(short.items()))}")
print(f"  {len(dupes)} sets allow duplicates: {', '.join(dupes) or 'none'}")

if fail:
    for x in fail:
        print(f"  !! {x}")
    sys.exit(1)
PY
