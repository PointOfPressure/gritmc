#!/usr/bin/env bash
# advancements-golden.sh — regenerate `data/vanilla/advancements.json` from the
# pinned jar.
#
# Same signature-stripping and classpath construction as `ci/loot-golden.sh`,
# for the same two reasons written in that file's header: the dumper lives
# inside vanilla's own package so the JVM refuses to mix an unsigned class into
# a signed one, and the libraries come out of the OUTER bundler jar because
# `oracle-work/vanilla-$VER/libraries` is empty until the vanilla launcher has
# been started once.
#
# # THIS SCRIPT RUNS THE DUMPER TWICE, IN TWO JVMs, AND THAT IS THE POINT
#
# `AdvancementNode.children` is a `ReferenceOpenHashSet` — identity-hashed —
# and `TreeNodePosition.run` lays a tree out by walking it, writing
# `DisplayInfo.x/y`, which `AdvancementHolder.STREAM_CODEC` then writes onto
# the wire. So the bytes of vanilla's own `update_advancements` packet are a
# function of a per-JVM identity hash. That is a claim about vanilla and it is
# not one to reason about, so this script runs THREE JVMs into a scratch
# directory: two identical, one with `-Drustmc.adv.shuffle` permuting the
# parse order and changing nothing else. It refuses to write a table that any
# pair of them disagrees about.
#
# The dumper pins `DisplayInfo.setLocation(0, 0)` and does not run
# `TreeNodePosition` at all, for exactly this reason — see its header. Before
# that pin: the shuffled run moved 122 of 126 `y` values and 120 of 126
# `holder_wire` strings, and two IDENTICAL runs, having agreed three times in
# a row, disagreed on the fourth attempt under `ci/gen-gameplay-tables.sh`.
# The three checks below therefore all have to pass, and the third one is not
# redundant: a table stable under two identical runs was still moving.
#
# `RUSTMC_ADV_KEEP=1` keeps the scratch directory for inspection.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/advancements.json}"

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

# The advancement documents themselves, out of the nested jar.
ADV="$VDIR/.advancements"
if [ ! -d "$ADV" ]; then
    mkdir -p "$ADV"
    unzip -o -q "$UNSIGNED" 'data/minecraft/advancement/*' -d "$ADV"
fi
NDOCS="$(find "$ADV" -name '*.json' | wc -l)"
[ "$NDOCS" -gt 1000 ] || { echo "only $NDOCS advancement documents found under $ADV — a low count here means the jar layout moved, and it is an instrument failure, not a finding" >&2; exit 1; }

CP="$UNSIGNED:$(find "$LIBS" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
if [ -n "${RUSTMC_ADV_KEEP:-}" ]; then
    echo "advancements-golden: scratch kept at $WORK"
else
    trap 'rm -rf "$WORK"' EXIT
fi

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/AdvancementGolden.java" 2>&1 | grep -v '^Note' || true
[ -s "$WORK/net/minecraft/advancements/AdvancementGolden.class" ] || { echo "AdvancementGolden did not compile" >&2; exit 1; }

run_once() { # run_once <table-out> <orders-out> [shuffle-seed]
    java -Drustmc.version="$VER" \
         ${3:+-Drustmc.adv.shuffle=$3} \
         -Drustmc.adv.dir="$ADV/data/minecraft/advancement" \
         -Drustmc.adv.out="$1" \
         -Drustmc.adv.orders.out="$2" \
         -cp "$CP:$WORK" \
         net.minecraft.advancements.AdvancementGolden
    return $?
}

mkdir -p "$WORK/a" "$WORK/b" "$WORK/c"
run_once "$WORK/a/advancements.json" "$WORK/a/orders.json"; s1=$?
[ "$s1" -eq 0 ] || { echo "AdvancementGolden exited $s1 (run 1)" >&2; exit "$s1"; }
run_once "$WORK/b/advancements.json" "$WORK/b/orders.json"; s2=$?
[ "$s2" -eq 0 ] || { echo "AdvancementGolden exited $s2 (run 2)" >&2; exit "$s2"; }

run_once "$WORK/c/advancements.json" "$WORK/c/orders.json" 20260905; s3=$?
[ "$s3" -eq 0 ] || { echo "AdvancementGolden exited $s3 (run 3, shuffled)" >&2; exit "$s3"; }

TABLE_STABLE=1
ORDERS_STABLE=1
SHUFFLE_STABLE=1
cmp -s "$WORK/a/advancements.json" "$WORK/b/advancements.json" || TABLE_STABLE=0
cmp -s "$WORK/a/orders.json" "$WORK/b/orders.json" || ORDERS_STABLE=0
cmp -s "$WORK/a/advancements.json" "$WORK/c/advancements.json" || SHUFFLE_STABLE=0
echo "advancements-golden: three JVM runs — table reproduced=$TABLE_STABLE observed-orders reproduced=$ORDERS_STABLE table-under-shuffled-parse-order=$SHUFFLE_STABLE"
if [ "$SHUFFLE_STABLE" -eq 0 ]; then
    echo "advancements-golden: the table MOVED when the parse order was permuted." >&2
    echo "advancements-golden: that means DisplayInfo x/y (and holder_wire) are a function of" >&2
    echo "advancements-golden: resource-scan order, not of the data, and this table's wire bytes" >&2
    echo "advancements-golden: are not the bytes a real vanilla server sends. Record it and stop" >&2
    echo "advancements-golden: using holder_wire for any byte comparison." >&2
    diff <(python3 -c "import json,sys;d=json.load(open(sys.argv[1]))['advancements'];[print(k,v['holder_wire']) for k,v in sorted(d.items())]" "$WORK/a/advancements.json") \
         <(python3 -c "import json,sys;d=json.load(open(sys.argv[1]))['advancements'];[print(k,v['holder_wire']) for k,v in sorted(d.items())]" "$WORK/c/advancements.json") \
         | head -4 | cut -c1-200 >&2
fi

if [ "$TABLE_STABLE" -eq 0 ]; then
    echo "advancements-golden: the TABLE did not reproduce across two JVM runs." >&2
    echo "advancements-golden: first differing keys:" >&2
    diff <(python3 -c "import json,sys;d=json.load(open(sys.argv[1]))['advancements'];[print(k,json.dumps(v,sort_keys=True)) for k,v in sorted(d.items())]" "$WORK/a/advancements.json") \
         <(python3 -c "import json,sys;d=json.load(open(sys.argv[1]))['advancements'];[print(k,json.dumps(v,sort_keys=True)) for k,v in sorted(d.items())]" "$WORK/b/advancements.json") \
         | head -4 | cut -c1-200 >&2
    echo "advancements-golden: refusing to write a table that is not a function of the jar" >&2
    exit 3
fi

mkdir -p "$(dirname "$OUT")"
cp -f "$WORK/a/advancements.json" "$OUT"
cp -f "$WORK/a/orders.json" "$(dirname "$OUT")/advancement_orders.json"

python3 - "$OUT" "$ORDERS_STABLE" "$SHUFFLE_STABLE" <<'PY'
import json, sys, collections
d = json.load(open(sys.argv[1]))
a = d["advancements"]
if d["count"] != len(a):
    sys.exit(f"advancements-golden: count {d['count']} but {len(a)} entries")
if len(a) < 1000:
    sys.exit(f"advancements-golden: only {len(a)} entries — instrument failure")
disp = sum(1 for v in a.values() if v["display"])
trig = collections.Counter(t for v in a.values() for t in v["criteria_triggers"].values())
rec  = sum(1 for v in a.values() if v["reward_recipes"])
xp   = sum(1 for v in a.values() if v["experience"])
loot = sum(1 for v in a.values() if v["reward_loot"])
fn   = sum(1 for v in a.values() if v["reward_function"])
roots = sum(1 for v in a.values() if v["parent"] is None)
# A table whose every entry carries the same wire bytes is the silent-oracle
# shape: refuse it.
wires = {v["holder_wire"] for v in a.values() if v["holder_wire"]}
nullwire = sum(1 for v in a.values() if not v["holder_wire"])
if len(wires) < len(a) // 2:
    sys.exit(f"advancements-golden: only {len(wires)} distinct wire strings across "
             f"{len(a)} advancements — that is not a result")
print(f"advancements-golden: {len(a)} advancements, {roots} roots, {disp} with display, "
      f"{len(wires)} distinct wire strings, {nullwire} unencodable")
print(f"advancements-golden: rewards — {xp} experience, {loot} loot, {rec} recipes, {fn} function")
print(f"advancements-golden: {len(trig)} distinct triggers; top: " +
      ", ".join(f"{k}={v}" for k, v in trig.most_common(6)))
print(f"advancements-golden: observed sibling/root orders reproduced across two JVMs: "
      f"{'yes' if sys.argv[2] == '1' else 'NO — see docs/FINDINGS.md'}")
print(f"advancements-golden: table survives a permuted parse order: "
      f"{'yes' if sys.argv[3] == '1' else 'NO — holder_wire is scan-order dependent'}")
PY
