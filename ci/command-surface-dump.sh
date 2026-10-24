#!/usr/bin/env bash
# command-surface-dump.sh — vanilla's own answer to "what commands are there".
#
#     ./ci/command-surface-dump.sh [out.json]     default data/vanilla/command_surface.json
#
# Runs `tools/goldengen/vanilla/CommandSurfaceDump.java`, which constructs a
# real `new Commands(CommandSelection.DEDICATED, ctx)` and reports the roots it
# holds, the roots a `PermissionLevel.ALL` source may use, and the bytes of the
# declare-commands packet such a source receives.
#
# This is the *executed* derivation of the command surface.
# `ci/command-surface.py` is the *static* one, read out of the constructor's
# bytecode and the CommandSelection.ALL report. They are independent, they can
# disagree, and `ci/command-surface.py --check` compares them when this file is
# present. Two derivations that agree are worth something only because a
# derivation that could not disagree is worth nothing.
#
# Run twice and compared, like ci/commands-golden.sh: a dumper that reads
# something ambient is a dumper whose output means nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/command_surface.json}"

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

javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/CommandSurfaceDump.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.commands.CommandSurfaceDump 2>/dev/null > "$1"
    test -s "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of CommandSurfaceDump disagree — it is reading something ambient" >&2
    exit 1
}

mkdir -p "$(dirname "$OUT")"
mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
d = json.load(open(sys.argv[1]))
reg, usable = d["registered"], d["usableAtAll"]
print(f"dedicated command surface -> {sys.argv[1]} (identical across two runs)")
print(f"  registered   {len(reg)}")
print(f"  usable@ALL   {len(usable)}")
print(f"  packet       {len(d['packetHexAtAll']) // 2} bytes")
if not reg:
    sys.exit("the dispatcher registered nothing — the dump is empty, not cheap")
if "publish" in reg or "unpublish" in reg:
    sys.exit("a DEDICATED dispatcher holds /publish — the selection was ignored")
# The filter must move. If every registered root were usable at level 0 this
# dump would agree with a rustmc that ignored permissions entirely, which is
# the one thing it must not do quietly.
if len(usable) >= len(reg):
    sys.exit("every registered root is usable at PermissionLevel.ALL — the "
             "permission filter is not being exercised, so this dump cannot fail")
missing = [c for c in usable if c not in reg]
if missing:
    sys.exit(f"usable names roots that are not registered: {missing}")
print(f"  the filter moves: {len(reg) - len(usable)} roots need a permission")
PYEOF
