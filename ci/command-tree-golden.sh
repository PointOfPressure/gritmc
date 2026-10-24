#!/usr/bin/env bash
# command-tree-golden.sh — the shape of every vanilla command, as data.
#
# Produces `data/vanilla/command_tree.json`: all 90 roots, every literal and
# argument node with its Brigadier parser, every redirect, and which nodes carry
# an executor.
#
# This is NOT `ci/commands-golden.sh`. That one encodes the declare-commands
# packet for the tree *rustmc* registers, and gates our wire arithmetic. This one
# dumps the tree *vanilla* registers, and answers a different question: how much
# of the command surface is shape rather than code.
#
# Cheap — no world, no server. `CommandBuildContext.simple(lookup,
# FeatureFlags.VANILLA_SET)` is enough to build the whole dispatcher.
#
# The two numbers to read together, and never apart: the node count is the
# extractable half, and the executable-node count is the half that must be
# written. A tree with every node placed and no executors is a server that
# autocompletes perfectly and does nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/command_tree.json}"

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
[ "$LIBCOUNT" -ge 10 ] || { echo "only $LIBCOUNT library jars unpacked — refusing a silently short classpath" >&2; exit 1; }

CP="$UNSIGNED:$(find "$LIBS" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/CommandTreeGolden.java" 2>&1 | grep -v '^Note' || true
[ -s "$WORK/net/minecraft/commands/CommandTreeGolden.class" ] || { echo "CommandTreeGolden did not compile" >&2; exit 1; }

mkdir -p "$(dirname "$OUT")"
java -Drustmc.cmd.out="$OUT" -cp "$CP:$WORK" net.minecraft.commands.CommandTreeGolden
[ -s "$OUT" ] || { echo "CommandTreeGolden wrote nothing" >&2; exit 1; }

python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
# A tree that came back tiny means the dispatcher was built without the vanilla
# feature set, which is a plausible and silent way to get a wrong answer.
if d["roots_total"] < 80 or d["nodes_total"] < 1500:
    sys.exit(f"command-tree: only {d['roots_total']} roots / {d['nodes_total']} nodes — "
             "vanilla 26.2 has ~90 and ~2100; a short tree is an instrument failure")
print(f"command-tree: {d['roots_total']} roots, {d['nodes_total']} nodes, "
      f"{d['arguments']} argument nodes, {d['distinct_argument_parsers']} distinct parsers, "
      f"{d['executable_nodes']} executable nodes, {d['redirects']} redirects")
PY
