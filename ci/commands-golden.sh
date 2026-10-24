#!/usr/bin/env bash
# commands-golden.sh — vanilla's own declare-commands bytes for OUR command set.
#
# Produces `data/vanilla/commands_tree.json`: for each of the five permission
# levels, the exact bytes `ClientboundCommandsPacket`'s own stream codec emits
# for a dispatcher holding exactly the commands rustmc registers, after
# vanilla's own `fillUsableCommands` has filtered it for a source at that
# level.
#
# See `tools/goldengen/vanilla/CommandsGolden.java` for why the golden is
# built this way instead of captured from a live server: vanilla's dispatcher
# holds every command in the game, node indices are assigned by a walk over
# the whole tree, and there is therefore no byte in vanilla's real packet that
# our three-node packet could be compared against. The Java side uses
# vanilla's constructor, vanilla's inspector and vanilla's codec, so what is
# gated is our arithmetic and our ordering — not a second reading of the
# bytecode that produced them.
#
# Cheap: no world, no server, no registries beyond `Bootstrap.bootStrap()`.
# Under a second. It still runs twice and demands the two agree, for the same
# reason every other golden here does — a dumper that iterates a hash map in
# encounter order is a defect this project has shipped before, and
# `enumerateNodes` puts its output in an `Object2IntOpenHashMap`, which is
# precisely such a map. (`createEntries` writes each entry at its own index
# and so is order-independent; that is a claim this double run checks rather
# than assumes.)
#
# Same signature-stripping and same `2>/dev/null` as `ci/signal-golden.sh`:
# the dumper lives in vanilla's own package, the JVM refuses to mix an
# unsigned class into a signed one, and `Bootstrap.bootStrap()` re-emits
# stdout through log4j with a prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/commands_tree.json}"

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

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/CommandsGolden.java" 2>&1 | grep -v '^Note' || true

# TZ and LANG are pinned, and that is not tidiness.
#
# `/version`'s `build_time` line is `Date.toString()`, which renders in the
# JVM's default zone and locale: the same jar produces `Tue Jun 16 13:01:27
# BST 2026` on this box and `... 12:01:27 UTC 2026` under TZ=UTC. Unpinned,
# `data/vanilla/commands_tree.json` would differ between worktrees and between
# machines, and every byte assertion against it would be about where it was
# generated.
#
# **The two-run comparison below cannot catch that**, and it is worth being
# explicit about why: it re-runs in the *same* environment, so anything ambient
# but constant within a machine passes it. That guard tests for a clock and a
# random seed. It does not test for a locale. The control after it does.
run() {
    TZ="${2:-UTC}" LANG=C java -Xmx1G -Drustmc.data="$VDIR/generated-data" \
        -cp "$CP:$WORK" net.minecraft.commands.CommandsGolden 2>/dev/null > "$1"
    test -s "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of CommandsGolden disagree — it is reading something ambient" >&2
    exit 1
}

# The control the two-run comparison cannot be: a third run in a deliberately
# different zone. Both zones are fixed here rather than taken from the box, so
# the control means the same thing wherever it runs.
#
# It asserts the *shape* of the environment coupling rather than its absence,
# because the coupling is vanilla's own behaviour and not a defect: exactly one
# line may move, and it must be the one that renders a date. A second line
# moving means something else in this dump reads the environment, and zero
# lines moving means `build_time` stopped being a rendered date -- both are
# findings, and neither may pass quietly.
run "$WORK/tokyo.json" Asia/Tokyo
python3 - "$WORK/out.json" "$WORK/tokyo.json" <<'PYCTL'
import json, sys
a = json.load(open(sys.argv[1]))["versionLines"]
b = json.load(open(sys.argv[2]))["versionLines"]
if len(a) != len(b):
    sys.exit(f"the two zones dumped {len(a)} and {len(b)} version lines")
moved = [i for i, (x, y) in enumerate(zip(a, b)) if x != y]
def texts(h):
    s = bytes.fromhex(h)
    return "".join(chr(c) if 32 <= c < 127 else "." for c in s)
if len(moved) != 1:
    for i in moved:
        print(f"  line {i}: UTC {texts(a[i])[:80]}", file=sys.stderr)
        print(f"          JST {texts(b[i])[:80]}", file=sys.stderr)
    sys.exit(f"TZ control: {len(moved)} version lines moved between UTC and "
             "Asia/Tokyo, expected exactly 1 (build_time). Zero means build_time "
             "is no longer a rendered date; more than one means something else "
             "in this dump reads the environment.")
if "commands.version.build_time" not in texts(a[moved[0]]):
    sys.exit(f"TZ control: line {moved[0]} moved between zones and it is not "
             "build_time — an unexpected part of this dump is locale-dependent")
print(f"  TZ control: exactly 1 of {len(a)} version lines is zone-dependent "
      f"(line {moved[0]}, build_time); the artefact is pinned to UTC")
PYCTL

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
rows = d["levels"]
print(f"commands tree -> {sys.argv[1]} (identical across two runs)")

# A golden whose rows are all the same row is a golden that cannot fail, and
# that is a live risk here rather than a theoretical one: pass `false` for
# `SeedCommand.register`'s dedicated flag and every level below sees the same
# tree. So the shape is asserted, not just printed.
seen = {}
for r in rows:
    print(f"  {r['level']:<12} ordinal={r['ordinal']} "
          f"entityEvent={r['entityEventByte']} "
          f"len={r['packetLen']:<4} usable={r['usable']}")
    seen[r["level"]] = r

if len(rows) != 5:
    sys.exit(f"expected 5 permission levels, got {len(rows)}")

lo, hi = seen["ALL"], seen["GAMEMASTERS"]
if lo["packetHex"] == hi["packetHex"]:
    sys.exit("ALL and GAMEMASTERS produced identical packets — the permission "
             "filter is not being exercised, so this golden cannot fail")
if "seed" in lo["usable"]:
    sys.exit("/seed is usable at level ALL — SeedCommand was registered with "
             "the wrong dedicated flag")
if "seed" not in hi["usable"]:
    sys.exit("/seed is not usable at GAMEMASTERS — the filter is too strict")
for r in rows:
    if "list" not in r["usable"] or "list uuids" not in r["usable"]:
        sys.exit(f"/list missing at level {r['level']} — it requires no permission")
    if r["entityEventByte"] != 24 + r["ordinal"]:
        sys.exit(f"entity-event byte wrong at {r['level']}")
print("  the filter moves: /seed absent at ALL, present at GAMEMASTERS")

rep = d["replies"]
for k in ("listUuids", "seed", "seedAlt"):
    print(f"  reply {k:<10} {len(rep[k]) // 2} bytes")
# The reply encoder must read its argument. Two seeds through one code path
# that came out equal would mean the dump is a constant, and every Rust
# assertion against it would be vacuous.
if rep["seed"] == rep["seedAlt"]:
    sys.exit("the two seeds produced identical bytes — the reply dump is a constant")
if not rep["listUuids"].endswith("00"):
    sys.exit("system_chat body must end in the overlay boolean")

# `/version` is ten separate system_chat bodies, handed over by vanilla's own
# `VersionCommand.dumpVersion`. Three things are asserted rather than printed.
lines = d["versionLines"]
print(f"  reply version    {len(lines)} lines, "
      f"{sum(len(x) // 2 for x in lines)} bytes total")
if len(lines) < 5:
    sys.exit(f"/version dumped {len(lines)} lines; dumpVersion feeds one per "
             "field and a short dump means the consumer was not run")
# THE HEADER IS LINE 0 AND IT IS NOT ONE OF `dumpVersion`'s. `VersionCommand`'s
# executor sends `commands.version.header` first and then calls `dumpVersion`,
# so a golden of `dumpVersion` alone is nine tenths of the reply and a port
# gated against it would silently send nine packets where vanilla sends ten.
# `commands.version.header` takes no arguments, so its body is short and has no
# `with` list at all — which is what this asserts, since a header that acquired
# one would change the reply and nothing else here would see it.
def _texts(h):
    b = bytes.fromhex(h)
    return "".join(chr(c) if 32 <= c < 127 else "." for c in b)
head = _texts(lines[0])
if "commands.version.header" not in head:
    sys.exit(f"the first /version line is not the header: {head[:80]}")
if "with" in head:
    sys.exit("commands.version.header has a `with` list; it is a no-argument "
             "translatable and this golden's consumer assumes so")
# A dump whose lines are all equal is a constant, and every Rust assertion
# against it would hold for a port that emitted one line ten times.
if len(set(lines)) != len(lines):
    sys.exit("two /version lines are byte-identical — the dump is not "
             "distinguishing its fields")
for i, x in enumerate(lines):
    if not x.endswith("00"):
        sys.exit(f"/version line {i} does not end in the overlay boolean")
PYEOF
