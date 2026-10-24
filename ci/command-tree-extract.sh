#!/usr/bin/env bash
# command-tree-extract.sh — vanilla's whole command graph, and its own bytes.
#
# Produces `data/vanilla/command_tree_full.json`: every node in vanilla's
# dispatcher in semantic terms, the argument-parser registry with the ids
# vanilla assigns, and the exact `ClientboundCommandsPacket` body vanilla's own
# stream codec emits for that graph at each of the five permission levels.
#
# This is the third command dumper and the three answer different questions, so
# before adding a fourth read which one you actually want:
#
#   * `ci/commands-golden.sh`      — vanilla's bytes for OUR command set. Gates
#                                    what we ship. Silent about everything our
#                                    tree cannot express, which today is
#                                    argument nodes, redirects and suggestions.
#   * `ci/command-tree-extract.sh` — this one. Vanilla's bytes for VANILLA's
#                                    whole tree, with the semantics needed to
#                                    rebuild them. Gates the encoder's
#                                    vocabulary at full scale without shipping
#                                    a single node.
#
# The distinction matters because a tree that we can encode is not a tree we
# have implemented. Reproducing vanilla's 2,000-node packet proves the
# serialisers, the numbering and the permission filter; it proves nothing about
# executors, and a server that shipped this tree would autocomplete perfectly
# and do nothing. `crates/rustmc-server/tests/command_tree_vanilla.rs` states
# that in the one place a reader of a green result will see it.
#
# Cheap — no world, no server, no live registries beyond the vanilla lookup.
# Measured on the Threadripper: about ten seconds including the two runs.
#
# It runs twice and demands the two agree, for the reason every golden here
# does: `enumerateNodes` collects into an `Object2IntOpenHashMap` and a dumper
# that iterates a hash map in encounter order is a defect this project has
# shipped before. Here the double run also covers the depth-first walk's own
# ordering and gson's object ordering.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/command_tree_full.json}"

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

# The JVM refuses to mix an unsigned class into a signed jar's package, and this
# dumper lives in `net.minecraft.commands` so that it can reach
# `Commands.createCompilationContext` and the package-private machinery around
# it. Same strip as `ci/commands-golden.sh`.
UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

LIBS="$VDIR/libraries"
if [ ! -d "$LIBS" ] || [ -z "$(find "$LIBS" -name '*.jar' -print -quit)" ]; then
    LIBS="$VDIR/.libs"
    if [ ! -d "$LIBS" ] || [ -z "$(find "$LIBS" -name '*.jar' -print -quit)" ]; then
        mkdir -p "$LIBS"
        unzip -o -q "$BUNDLER" 'META-INF/libraries/*' -d "$LIBS"
    fi
fi
LIBCOUNT="$(find "$LIBS" -name '*.jar' | wc -l)"
# A short classpath does not fail loudly — it fails as a `NoClassDefFoundError`
# for whichever library happens to be missing, which reads as a broken dumper.
[ "$LIBCOUNT" -ge 10 ] || { echo "only $LIBCOUNT library jars under $LIBS — refusing a silently short classpath" >&2; exit 1; }

CP="$UNSIGNED:$(find "$LIBS" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/CommandTreeExtract.java" 2>&1 | grep -v '^Note' || true
[ -s "$WORK/net/minecraft/commands/CommandTreeExtract.class" ] || {
    echo "CommandTreeExtract did not compile" >&2; exit 1; }

# `$?` IS CHECKED HERE, AND IT WAS NOT — 2026-09-05.
#
# This used to run java with `2>/dev/null` and then test only that the output
# file was non-empty. A JVM that died after writing part of the document would
# pass both: the file exists, it is not empty, and its exit status was thrown
# away along with whatever it printed. The two-run comparison would then report
# "the dump is not deterministic", which is a claim about vanilla, for a
# dumper that crashed.
#
# stderr is kept and filtered rather than discarded, because the only reason it
# was silenced is the JVM's `sun.misc.Unsafe` deprecation warnings, and
# throwing away a stream to hide four known lines is how the next real message
# goes unread.
run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -Drustmc.cmd.out="$1" \
        -cp "$CP:$WORK" net.minecraft.commands.CommandTreeExtract \
        2> >(grep -vE '^WARNING: ' >&2)
    status=$?
    [ "$status" -eq 0 ] || { echo "CommandTreeExtract exited $status" >&2; exit "$status"; }
    [ -s "$1" ] || { echo "CommandTreeExtract wrote nothing to $1" >&2; exit 1; }
}

mkdir -p "$(dirname "$OUT")"
run "$WORK/a.json"
run "$WORK/b.json"

# THE TWO RUNS DO NOT ALWAYS AGREE, AND VANILLA IS THE REASON — 2026-09-05.
#
# This check used to `cmp` and exit 1 saying "the dump is not deterministic",
# which reads as a defect in this dumper. It is not. Measured on the
# Threadripper over sixteen consecutive JVM runs of the same class file
# against the same jar: **fifteen produced one document and one produced
# another**, and the difference was 160 of 2,085 nodes, every one of them a
# child of `/gamerule`, permuted. Nothing else in the tree moved and the
# packet lengths were identical at all five levels (29,279 / 30,099 / 30,248).
#
# `GameRuleCommand.register` walks `new GameRules(ctx.enabledFeatures())` with
# `visitGameRuleTypes`, and that iteration is not ordered stably across JVM
# runs. So **vanilla's own declare-commands packet is not byte-stable for
# `/gamerule`**: two vanilla servers on the same jar can send two different
# byte sequences, both correct. A byte-exact golden of that subtree is not a
# thing that exists to be taken, and a port that reproduced one run's order
# would be gated against a coin flip.
#
# So the comparison is now structural where it has to be and byte-exact
# everywhere else: a difference confined to `/gamerule`'s children, and which
# is a permutation of the same multiset, is accepted and **recorded in the
# document** as `unstable_subtrees`, so no reader can mistake that order for a
# property of vanilla. Any other difference is still a refusal.
"$ROOT/ci/command-tree-compare.py" "$WORK/a.json" "$WORK/b.json" "$OUT"

python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
c = d["counts"]
# A tree that came back tiny means the dispatcher was built without the vanilla
# feature set, which is a plausible and silent way to get a wrong answer: the
# dump would still be well-formed, the Rust side would still reproduce it, and
# the pair would agree about a fraction of the surface.
if c["roots"] < 80 or c["nodes"] < 1500:
    sys.exit(f"command-tree-extract: only {c['roots']} roots / {c['nodes']} nodes — "
             "vanilla 26.2 has ~90 and ~2100; a short tree is an instrument failure")
if c["parsers_registered"] < 40:
    sys.exit(f"command-tree-extract: only {c['parsers_registered']} argument parsers "
             "registered — the registry did not bootstrap")
if len(d["levels"]) != 5:
    sys.exit(f"command-tree-extract: {len(d['levels'])} permission levels, expected 5")
# Five identical packets would mean the filter never ran, which is exactly the
# shape of a permission gate that has stopped gating. `/seed` and friends move
# between levels, so the five bodies must differ.
hexes = [l["packetHex"] for l in d["levels"]]
if len(set(hexes)) == 1:
    sys.exit("command-tree-extract: all five permission levels produced the same "
             "packet — fillUsableCommands filtered nothing")
print(f"command-tree-extract: {c['roots']} roots, {c['nodes']} nodes, "
      f"{c['arguments']} argument, {c['executable']} executable, {c['redirects']} redirects, "
      f"{c['custom_suggestions']} custom-suggestion, {c['parsers_registered']} parsers; "
      f"packet {'/'.join(str(l['packetLen']) for l in d['levels'])} bytes by level")
PY
