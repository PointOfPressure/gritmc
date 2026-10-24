#!/usr/bin/env bash
# command-parse-golden.sh — vanilla's own argument parsers on a fixed corpus.
#
# Produces `data/vanilla/command_parse.json`: for every (argument type, input)
# pair, whether vanilla accepted it and where its `StringReader` cursor ended
# up. `crates/rustmc-server/tests/command_parse_vanilla.rs` runs
# `Parser::parse` over the same rows and compares both fields.
#
# This is the fourth command dumper in `ci/` and the four answer four different
# questions, so before adding a fifth read which one you actually want:
#
#   * `ci/commands-golden.sh`      — vanilla's declare-commands BYTES for OUR
#                                    command set.
#   * `ci/command-tree-extract.sh` — vanilla's declare-commands BYTES for
#                                    VANILLA's whole tree, plus the semantics
#                                    needed to rebuild them.
#   * `ci/command-tree-golden.sh`  — the shape COUNTED: roots, nodes, parsers,
#                                    executors.
#   * this one                     — what an argument PARSE consumes. The other
#                                    three are all about the tree a client is
#                                    shown; none of them constrains what
#                                    happens when a player types a command into
#                                    it.
#
# Cheap: no world, no server, no build context. Brigadier's six primitives and
# `brigadier:string` need none, and those seven are exactly the seven
# `Parser::parse` transcribes — see the dumper's header for why it stops there
# rather than reaching for the `net.minecraft.commands.arguments` types.
#
# It runs twice and demands byte-identical output. There is nothing hash-ordered
# in it, which is the point: unlike `ci/command-tree-extract.sh`, whose
# `/gamerule` subtree is genuinely not stable across JVM runs, a difference here
# is a defect and is refused rather than characterised.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/command_parse.json}"

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
# dumper lives in `net.minecraft.commands` for the same reason
# `CommandTreeExtract` does. Same strip.
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
[ "$LIBCOUNT" -ge 10 ] || { echo "only $LIBCOUNT library jars under $LIBS — refusing a silently short classpath" >&2; exit 1; }

CP="$UNSIGNED:$(find "$LIBS" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

javac -nowarn -encoding UTF-8 -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/CommandParseGolden.java" 2>&1 | grep -v '^Note' || true
[ -s "$WORK/net/minecraft/commands/CommandParseGolden.class" ] || {
    echo "CommandParseGolden did not compile" >&2; exit 1; }

# `$?` is checked, for the reason written at length in
# `ci/command-tree-extract.sh`: a JVM that dies after writing part of a document
# leaves a file that is present and non-empty, and a check that reads only those
# two things calls it a success.
run() {
    java -Xmx2G -Dfile.encoding=UTF-8 -Drustmc.parse.out="$1" \
        -cp "$CP:$WORK" net.minecraft.commands.CommandParseGolden \
        2> >(grep -vE '^WARNING: ' >&2)
    status=$?
    [ "$status" -eq 0 ] || { echo "CommandParseGolden exited $status" >&2; exit "$status"; }
    [ -s "$1" ] || { echo "CommandParseGolden wrote nothing to $1" >&2; exit 1; }
}

mkdir -p "$(dirname "$OUT")"
run "$WORK/a.json"
run "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "command-parse-golden: two runs disagreed — this dumper has no hash-ordered" >&2
    echo "  input, so a difference here is a defect and not vanilla being vanilla" >&2
    diff <(head -60 "$WORK/a.json") <(head -60 "$WORK/b.json") >&2 || true
    exit 1
}
cp -f "$WORK/a.json" "$OUT"

python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
cases = d["cases"]
if len(cases) != d["specs"] * d["inputs"] or len(cases) != d["case_count"]:
    sys.exit(f"command-parse-golden: {len(cases)} cases, header says {d['case_count']} "
             f"for {d['specs']}x{d['inputs']}")
# A CORPUS OF ONLY-ACCEPTED INPUTS CANNOT CATCH A PARSER THAT ACCEPTS ANYTHING,
# and a corpus of only-rejected ones cannot catch one that accepts nothing. Both
# halves are asserted, per parser rather than in aggregate, because an aggregate
# split is satisfied by one greedy string accepting everything while five
# numeric parsers accept nothing.
by = {}
for c in cases:
    key = (c["parser"], json.dumps(c["props"], sort_keys=True))
    yes, no = by.setdefault(key, [0, 0])
    if c["accepted"]:
        by[key][0] += 1
    else:
        by[key][1] += 1
# TWO PARSERS CANNOT REJECT, AND THAT IS A FACT ABOUT VANILLA, NOT A HOLE.
# `StringArgumentType.greedyString` is `getRemaining()` and
# `StringArgumentType.word` is `readUnquotedString`, whose empty run is the
# empty string rather than an exception — neither has a throw on any path. They
# are named here rather than exempted by a blanket `if no == 0: pass`, and the
# assertion is turned round for them: if either ever rejects something, THAT is
# the red, because it would mean the reading above has stopped being true.
NEVER_REJECTS = {
    ('brigadier:string', '{"type": "greedy"}'),
    ('brigadier:string', '{"type": "word"}'),
    # `ObjectiveArgument.parse` is TWO instructions —
    # `readUnquotedString(); areturn` — so it is `brigadier:string word` under
    # another name and has no throwing path either. Every constraint on an
    # objective name lives in `ScoreboardCommand`'s executors, not here, which
    # is why `/scoreboard objectives remove nosuchthing` is a parse SUCCESS
    # followed by an executor refusal.
    ('minecraft:objective', '{}'),
}
for (parser, props), (yes, no) in sorted(by.items()):
    if yes == 0:
        sys.exit(f"command-parse-golden: {parser} {props} accepted nothing — "
                 "the corpus cannot distinguish it from a parser that always fails")
    if (parser, props) in NEVER_REJECTS:
        if no != 0:
            sys.exit(f"command-parse-golden: {parser} {props} rejected {no} inputs, and "
                     "this file records that it has no throwing path — one of the two is wrong")
    elif no == 0:
        sys.exit(f"command-parse-golden: {parser} {props} rejected nothing — "
                 "the corpus cannot distinguish it from a parser that always passes")
# The cursor is the field this file exists for; a dump where every accepted case
# landed at the same place is one where the cursor is not being read.
spans = {c["cursor"] for c in cases if c["accepted"]}
if len(spans) < 4:
    sys.exit(f"command-parse-golden: accepted cases end at only {len(spans)} distinct "
             "cursor positions — the corpus does not exercise consumption")
print(f"command-parse-golden: {len(cases)} cases over {len(by)} parser configurations, "
      f"{d['accepted']} accepted / {len(cases) - d['accepted']} rejected, "
      f"{len(spans)} distinct accepted cursor positions")
PY
