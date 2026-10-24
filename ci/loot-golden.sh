#!/usr/bin/env bash
# loot-golden.sh — regenerate the loot vectors from the pinned jar.
#
# Same signature-stripping and classpath construction as the other golden
# runners, for the same reason: the dumper lives inside vanilla's own package
# and the JVM refuses to mix an unsigned class into a signed one.
#
# Two things here are NOT shared with the other runners, and both were bought
# the hard way:
#
#   * The libraries are unpacked from the **outer bundler jar**, not read from
#     `oracle-work/vanilla-$VER/libraries`. That directory is created by the
#     vanilla launcher on its first real start and is empty until then, so a
#     classpath built by `find`ing it silently omits gson and datafixerupper and
#     the compile fails on `package com.google.gson does not exist` — which
#     reads like a missing JDK rather than a jar that was never unpacked.
#
#   * The dumper writes its JSON to a **file**, never to stdout.
#     `Bootstrap.bootStrap()` redirects `System.out` through log4j, which
#     re-emits every line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix.
#     A 2.6 MB JSON document written to stdout comes back unparseable.
#     `Bootstrap.STDOUT` holds the original stream if a caller needs it.
#
# Grep any diagnostic output unanchored, for the log4j reason above, and never
# pipe the java invocation into `tail` — `$?` then reports the exit status of
# `tail`, which is the same trap as `cargo build | tail` reporting 0 when cargo
# is not installed at all. This script runs java bare and checks `$?` directly.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/loot.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="${RUSTMC_VANILLA_DIR:-$ROOT/oracle-work/vanilla-$VER}"
BUNDLER="$VDIR/server.jar"
JAR="$VDIR/versions/$VER/server-$VER.jar"

if [ ! -s "$JAR" ]; then
    [ -s "$BUNDLER" ] || { echo "no jar at $BUNDLER — run ci/vanilla-server.sh fetch" >&2; exit 1; }
    mkdir -p "$(dirname "$JAR")"
    unzip -o -q "$BUNDLER" "META-INF/versions/$VER/server-$VER.jar" -d "$VDIR/.unbundle"
    cp -f "$VDIR/.unbundle/META-INF/versions/$VER/server-$VER.jar" "$JAR"
fi

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

LIBS="$VDIR/.libs"
if [ ! -d "$LIBS" ] || [ -z "$(find "$LIBS" -name '*.jar' -print -quit)" ]; then
    mkdir -p "$LIBS"
    unzip -o -q "$BUNDLER" 'META-INF/libraries/*' -d "$LIBS"
fi
LIBCOUNT="$(find "$LIBS" -name '*.jar' | wc -l)"
[ "$LIBCOUNT" -ge 10 ] || { echo "only $LIBCOUNT library jars unpacked from $BUNDLER — refusing to build a classpath that is silently short" >&2; exit 1; }

# The loot tables themselves, out of the nested jar.
LOOT="$VDIR/.loot"
if [ ! -d "$LOOT" ]; then
    mkdir -p "$LOOT"
    unzip -o -q "$UNSIGNED" 'data/minecraft/loot_table/*' 'data/minecraft/tags/*' -d "$LOOT"
fi
NTABLES="$(find "$LOOT" -name '*.json' | wc -l)"
[ "$NTABLES" -gt 1000 ] || { echo "only $NTABLES loot tables found under $LOOT — a low count here means the jar layout moved, and it is an instrument failure, not a finding" >&2; exit 1; }

# THE TWO FIXTURES `LootGolden` DOES NOT WRITE — added 2026-09-05.
#
# `LootGolden` emits the vectors (`loot.json`) and the block->table map
# (`block_loot_map.json`) and nothing else. But `crates/rustmc-loot`'s three
# test targets also read `loot_tables.json` (the 1,355 table documents the run
# evaluated) and `item_tags.json` (224 item tags, flattened), and **no committed
# step produced either.** Both existed only in the worktree the crate was
# written in, so from a clean checkout two of the three targets panicked on a
# missing fixture and the crate's headline figure rested on files nothing could
# reproduce. That is the same defect class as a check that cannot go red: the
# evidence was real and the instrument that makes it was not in the tree.
#
# Written here rather than in Java because both are pure restatements of files
# already unzipped out of the jar above — a JVM adds a dependency and no
# information. Verified on 2026-09-05 to reproduce the two hand-made fixtures
# **exactly**, key sets and values, 1,355 tables and 224 tags, so the vectors
# taken against them still stand.
#
# Item tags are flattened recursively (a tag may list `#other/tag`) and sorted.
# Sorting is safe and is not an assumption: the only reader is `item_matches`
# in `crates/rustmc-loot/src/lib.rs`, which asks whether an id is a MEMBER.
python3 - "$LOOT/data/minecraft/loot_table" "$LOOT/data/minecraft/tags" "$(dirname "$OUT")" <<'GENPY'
import json, os, sys, pathlib

loot_dir, tags_root, out_dir = sys.argv[1], sys.argv[2], sys.argv[3]

lr = pathlib.Path(loot_dir)
tables = {str(p.relative_to(lr))[:-5]: json.load(open(p)) for p in sorted(lr.rglob("*.json"))}
if len(tables) < 1000:
    sys.exit(f"loot-golden: only {len(tables)} table documents under {loot_dir} — instrument failure")

item_dir = pathlib.Path(tags_root) / "item"
raw = {
    "minecraft:" + str(p.relative_to(item_dir))[:-5]: json.load(open(p)).get("values", [])
    for p in sorted(item_dir.rglob("*.json"))
}
if not raw:
    sys.exit(f"loot-golden: no item tags under {item_dir} — instrument failure")


def resolve(name, seen):
    """Flatten one tag. `seen` breaks the cycle a malformed pack could contain."""
    if name in seen:
        return set()
    seen = seen | {name}
    out = set()
    for v in raw.get(name, []):
        if isinstance(v, dict):
            v = v.get("id", "")
        if not isinstance(v, str) or not v:
            continue
        if v.startswith("#"):
            t = v[1:]
            out |= resolve(t if ":" in t else "minecraft:" + t, seen)
        else:
            out.add(v if ":" in v else "minecraft:" + v)
    return out


item_tags = {"#" + k: sorted(resolve(k, frozenset())) for k in sorted(raw)}
# Instrument tags ride in the same file under `instrument/#<id>`, and these
# are NOT sorted: `set_instrument` draws `nextInt(size)` over the tag and the
# draw picks by position, so the list is in `TagLoader` order — a
# `LinkedHashSet` over the tag's own `values`, nested tags spliced in place at
# first occurrence (the order `EnchantmentGolden` extracts for enchantment
# tags). An item-tag reader never sees these keys: it looks up `#<id>`.
inst_dir = pathlib.Path(tags_root) / "instrument"
inst_raw = {
    "minecraft:" + str(p.relative_to(inst_dir))[:-5]: json.load(open(p)).get("values", [])
    for p in sorted(inst_dir.rglob("*.json"))
}
def ordered(name, seen):
    if name in seen:
        return []
    seen = seen | {name}
    out = []
    for v in inst_raw.get(name, []):
        if isinstance(v, dict):
            v = v.get("id", "")
        if not isinstance(v, str) or not v:
            continue
        if v.startswith("#"):
            t = v[1:]
            members = ordered(t if ":" in t else "minecraft:" + t, seen)
        else:
            members = [v if ":" in v else "minecraft:" + v]
        for m in members:
            if m not in out:
                out.append(m)
    return out
if not inst_raw:
    sys.exit(f"loot-golden: no instrument tags under {inst_dir} — instrument failure")
for k in sorted(inst_raw):
    item_tags["instrument/#" + k] = ordered(k, frozenset())
json.dump(tables, open(os.path.join(out_dir, "loot_tables.json"), "w"), sort_keys=True)
json.dump(item_tags, open(os.path.join(out_dir, "item_tags.json"), "w"), indent=0, sort_keys=True)
print(f"loot-golden: {len(tables)} table documents and {len(item_tags)} item tags written")
GENPY

CP="$UNSIGNED:$(find "$LIBS" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/LootGolden.java" 2>&1 | grep -v '^Note' || true
[ -s "$WORK/net/minecraft/world/level/storage/loot/LootGolden.class" ] || { echo "LootGolden did not compile" >&2; exit 1; }

mkdir -p "$(dirname "$OUT")"
# `cmd` then a bare `status=$?` is a dead handler under `set -e` (this file is
# `set -euo pipefail`): the shell exits AT java and the two checks below never
# run, so "LootGolden exited N" — the one line that names the cause — is never
# printed. 2026-09-06.
status=0
java -Drustmc.version="$VER" \
     -Drustmc.loot.dir="$LOOT/data/minecraft/loot_table" \
     -Drustmc.tags.dir="$LOOT/data/minecraft/tags" \
     -Drustmc.blockmap.out="$(dirname "$OUT")/block_loot_map.json" \
     -Drustmc.loot.out="$OUT" \
     -cp "$CP:$WORK" \
     net.minecraft.world.level.storage.loot.LootGolden || status=$?
[ "$status" -eq 0 ] || { echo "LootGolden exited $status" >&2; exit "$status"; }
[ -s "$OUT" ] || { echo "LootGolden wrote nothing to $OUT" >&2; exit 1; }

python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
ok = [c for c in d["cases"] if "stacks" in c]
print(f"loot-golden: {d['decoded']} tables decoded, {len(ok)} cases evaluated, {d['failed']} failed")
print(f"loot-golden: failure kinds {d['failure_kinds']}")
# A file full of empty successes is the silent-oracle shape; refuse it.
nonempty = sum(1 for c in ok if c["stacks"])
if nonempty < len(ok) // 4:
    sys.exit(f"loot-golden: only {nonempty} of {len(ok)} successful cases dropped anything — "
             "that is the shape this dumper produced when item components were unbound, "
             "and it is not a result")
print(f"loot-golden: {nonempty} of {len(ok)} successful cases dropped at least one item")
PY
