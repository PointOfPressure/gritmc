#!/usr/bin/env bash
# shape-reads.sh — characterise `update_shape.json`'s refusals from the jar.
#
#     ./ci/shape-reads.sh              # regenerate ci/checks/shape-refusal-census.json
#     ./ci/shape-reads.sh --check      # re-derive and refuse if it has drifted
#
# `update_shape.json` refuses 76.57% of its pairs and the number had never
# been taken apart. This runs `tools/goldengen/vanilla/ShapeReadsGolden.java`,
# which re-derives that refusal set with `ShapeGolden`'s own narrow probe and
# then re-asks each refused pair against a **widened** level — one that
# answers every position with a filler instead of throwing on the third — to
# find out which refusals are debt and which are artefacts of a two-position
# harness.
#
# # WHY THE CENSUS IS COMMITTED AND THE TABLE IS NOT
#
# `data/vanilla/` is derived per tree and gitignored, and `ci/t2-gameplay.sh`
# refuses a table that came from another tree. That is right for a table the
# server *reads*: it must match the tree it is gated in.
#
# This is not one of those. Nothing under `crates/` opens it and nothing may —
# a widened level **invents** the world past the neighbour, and a row derived
# from an invented world is exactly the "plausible answer" this project
# refuses. What it is instead is a *claim about the jar*, and a claim is
# worth committing precisely so a later run can be made to disagree with it.
# `--check` is that disagreement, and it is why the numbers in
# `docs/cold-start/refusal-ledger.md` may be quoted: they are re-derived from
# the pinned jar on every gate, not transcribed once.
#
# # THE ASSERTION THAT MAKES IT A LEDGER AND NOT A SECOND OPINION
#
# The census recomputes `sensitive`, `carried`, `connective` and
# `refusedPairs` with its own copy of the narrow probe. If those four do not
# equal `update_shape.json`'s, the two harnesses have drifted and **every
# number in the census is about a different table than the one the server
# loads**. That is checked below, against the tree's own table, and it is a
# hard failure rather than a warning: a census that silently describes some
# other extract is worse than no census.
#
# # WHICH OF THIS SCRIPT'S REFUSALS ARE FALSIFIED, AND WHICH ARE NOT
#
# Stated rather than left for a reader to assume, because "every check ships
# with a falsifier" is a standing rule here and this one only half obeys it.
#
#   * **the narrow-probe assertion** — the census's `sensitive`, `carried`,
#     `connective` and `refusedPairs` against the tree's own table. Falsified,
#     cheaply and hermetically, by `ci/refusal-ledger-falsify.py`'s
#     `census-stale` / `census-agrees` / `census-absent` arms, which carry the
#     same comparison against synthetic tables. That is the refusal that
#     matters, because it is the one that fires when the census stops
#     describing the extract the server loads.
#   * **`--check`'s byte comparison** against the committed census. **Not**
#     falsified, and deliberately not: every arm would cost a five-minute JVM
#     run, and the thing under test is a `cmp`. What it protects against is a
#     census regenerated on a different jar, which the narrow-probe assertion
#     above catches first in every case that also moves those four numbers.
#
# The gap that leaves, named so nobody reads the pairing as complete: a change
# that moves a *verdict* without moving any of the four narrow counts — a new
# filler, a different `getBlockEntity` answer in the widened probe — is caught
# only by running this script, and nothing in T0 runs it. Five minutes is why.
# Run it after touching `ShapeReadsGolden.java` and commit what it writes.
#
# Same signature-stripping and same unanchored-grep warning as
# `ci/shape-golden.sh`: the dumper lives in vanilla's own package and the JVM
# refuses to mix an unsigned class into a signed one, and `Bootstrap.bootStrap()`
# re-emits stdout through log4j with a prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/ci/checks/shape-refusal-census.json"
TABLE="$ROOT/data/vanilla/update_shape.json"

MODE=""
case "${1:-}" in
    --check) MODE=check ;;
    "")      ;;
    *)       echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

[ -s "$TABLE" ] || {
    echo "shape-reads: no $TABLE — run ci/gen-gameplay-tables.sh first" >&2
    exit 2
}

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
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/ShapeReadsGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx8G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.ShapeReadsGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree — `ci/shape-golden.sh`'s rule, and this harness
# is parallel across subjects for the same reason and so has the same
# exposure. `--check` re-derives and compares against the committed census,
# which is already a second opinion, so it runs once; a generation run has no
# such second opinion and pays for one here.
run "$WORK/census.json"
if [ "$MODE" != check ]; then
    run "$WORK/again.json"
    cmp -s "$WORK/census.json" "$WORK/again.json" || {
        echo "two runs of ShapeReadsGolden disagree — something in it depends on thread order" >&2
        exit 1
    }
fi

# The assertion. See the header — if these four disagree the census is about
# a different extract than the one the server loads.
python3 - "$WORK/census.json" "$TABLE" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
t = json.load(open(sys.argv[2]))
want = {"states": t["states"], "sensitive": t["sensitive"],
        "carried": len(t["table"]), "connective": t["connective"],
        "refusedPairs": t["refusedPairs"]}
got = {k: c[k] for k in want}
if got != want:
    print("shape-reads: the census's narrow probe disagrees with update_shape.json", file=sys.stderr)
    for k in want:
        if got[k] != want[k]:
            print(f"  {k}: census {got[k]}, table {want[k]}", file=sys.stderr)
    sys.exit(1)
PY

if [ "$MODE" = check ]; then
    if [ ! -s "$OUT" ]; then
        echo "shape-reads: $OUT is missing — run ci/shape-reads.sh" >&2
        exit 1
    fi
    if ! cmp -s "$WORK/census.json" "$OUT"; then
        echo "SHAPE-REFUSAL-CENSUS DRIFTED from the jar — re-run ci/shape-reads.sh" >&2
        diff <(python3 -m json.tool "$OUT") \
             <(python3 -m json.tool "$WORK/census.json") | head -40 >&2
        exit 1
    fi
    python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
v = d["verdicts"]
print(f"SHAPE-REFUSAL-CENSUS OK — {d['refusedPairs']} refused pairs, "
      f"{v['world']} owed, {v['refused']} unmodelled, "
      f"{v['inert']} inert, {v['recoverable']} recoverable, {v['connective']} connective")
PY
    exit 0
fi

mkdir -p "$(dirname "$OUT")"
mv "$WORK/census.json" "$OUT"
python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
v = d["verdicts"]
n = d["refusedPairs"]
print(f"shape refusal census -> {sys.argv[1]}")
print(f"  {d['sensitive']} sensitive pairs: {d['carried']} carried, "
      f"{d['connective']} connective, {n} refused")
print(f"  {d['probes']:,} probes, neighbour breadth {d['breadth']['neighbours']}, "
      f"fillers {', '.join(d['fillers'])}")
print("  the refusals, re-asked against a level that can see past the neighbour:")
for k in ("world", "refused", "connective", "recoverable", "inert"):
    print(f"    {v[k]:>7}  {v[k]*100.0/n:5.1f}%  {k}")
print(f"    of which inert over ALL {d['states']} neighbour states: {d['inertExhaustive']}")
print("  how many positions past its own neighbour the rule read:")
for k, c in sorted(d["byExtraPositionCount"].items(), key=lambda kv: int(kv[0])):
    print(f"    {c:>7} pairs read {k} extra position(s)")
print("  what a widened level still could not answer:")
for r in d["stillUnmodelled"]:
    print(f"    {r['pairs']:>7}  {r['method']}")
print(f"  what discovery does not look at: {d['insensitive']} pairs that no state "
      f"in the game moves")
print(f"    {d['discoveryMissed']} of them move anyway (must be 0 — discovery is exhaustive)")
print(f"    {d['discoveryLateRefusal']} reach past the harness anyway (must be 0)")
print(f"  what the OLD {d['breadth']['neighbours']}-default discovery breadth would still be dropping:")
print(f"    {d['missedAtDefaults']} pairs that move only on a non-default neighbour")
print(f"    {d['lateRefusalAtDefaults']} that reach past the harness in DISCOVERY only on one")
print(f"    {d['resolutionRefusalAtDefaults']} that reach past it in RESOLUTION only on one")
print(f"      — these two are different passes and the second is NOT bounded by the first;")
print(f"        see PARITY-ROADMAP §83.3, which arrived at it by subtracting two runs")
for k, v in sorted(d["discoveryMissedByClass"].items(), key=lambda kv: -kv[1]):
    print(f"      {v:>5}  {k}")
PY
