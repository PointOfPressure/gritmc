#!/usr/bin/env bash
# shape-three.sh — the three-position extract for `update_shape.json`'s refusals.
#
#     ./ci/shape-three.sh                 # regenerate the table and the census
#     ./ci/shape-three.sh --check         # re-derive and refuse if either drifted
#     ./ci/shape-three.sh --only WallBlock   # one block class, for measuring cost
#
# Writes two things, and the split is the point:
#
#   data/vanilla/update_shape3.json      the table. Derived per tree, gitignored,
#                                        like every other table under data/vanilla.
#   ci/checks/shape-three-census.json    the counts, committed, quotable.
#
# # WHY THE TABLE IS NOT COMMITTED AND THE CENSUS IS
#
# The same reason `ci/shape-reads.sh` gives, one step further on. A table the
# server may read has to match the tree it is gated in, so it is generated per
# tree and never carried. The counts are a **claim about the jar**, and a claim
# is worth committing precisely so a later run can be made to disagree with it.
# `--check` is that disagreement.
#
# **This table is different from the census in one way that matters, though:**
# `ci/checks/shape-refusal-census.json` describes a level that invents the world
# and so nothing under `crates/` may ever read it. This one does not invent
# anything — it answers three positions and throws for a fourth — so it *is* a
# table a port may read, once a port exists. Nothing reads it yet.
#
# # THE ASSERTION THAT MAKES IT A LEDGER AND NOT A SECOND OPINION
#
# The extract re-derives `update_shape.json`'s refusal set with its own copy of
# `ShapeGolden`'s narrow probe, including both of that probe's early exits. If
# the number of pairs it finds refused is not the table's `refusedPairs`, the
# two harnesses have drifted and every number here is about a different table
# than the one the server loads. Checked below, hard failure, same as
# `ci/shape-reads.sh`.
#
# **`--only` runs ONCE, and its second opinion is the whole-table run.** A
# whole-table generation runs twice and `cmp`s, because a table that differs
# between two runs of one generator is a finding that outranks whatever it was
# generated to measure. Doubling a forty-five minute per-class measurement to
# check itself is not that trade — and it does not have to be, because the
# whole-table run already measures every class under a **different work
# partition** and its `byClass` entry is an independent second measurement of
# the same number. `--only` compares against it wherever a whole-table census
# exists, and says out loud that it is a single run where one does not. §83.9
# claims that cross-check for its per-class table; this performs it.
#
# **`--only` is checked too, and it was not.** A per-class run exits long
# before the whole-table assertion, so every figure `--only` has ever printed
# was a count of what this harness's own discovery found, with nothing
# comparing it to the set `update_shape.json` refuses. `--only` is the path
# that publishes a number *instead of* paying for the whole run, which makes it
# the one that most needs the check, and it is the path that did not have it.
# The per-class denominator lives in `ci/checks/shape-refusal-census.json`'s
# `byClass` rather than in `update_shape.json`, which carries one total.
#
# # COST, MEASURED RATHER THAN ESTIMATED
#
# The third axis is partitioned at 1,196 x 1,196 per pair and each distinct
# behaviour then pays one exhaustive 32,366-neighbour sweep. `--only` exists
# because that is a number nobody should guess at: run one block class, read
# `probes` out of the JSON, and multiply. `WallBlock` alone is 41,472 of the
# 78,797 position refusals, so it is the honest thing to measure on.
#
# Same signature-stripping and same unanchored-grep warning as
# `ci/shape-golden.sh`: the dumper lives in vanilla's own package and the JVM
# refuses to mix an unsigned class into a signed one, and `Bootstrap.bootStrap()`
# re-emits stdout through log4j with a prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/data/vanilla/update_shape3.json"
CENSUS="$ROOT/ci/checks/shape-three-census.json"
TABLE="$ROOT/data/vanilla/update_shape.json"

MODE=""
ONLY=""
while [ $# -gt 0 ]; do
    case "$1" in
        --check) MODE=check; shift ;;
        --only)  ONLY="${2:-}"; [ -n "$ONLY" ] || { echo "--only needs a class name" >&2; exit 2; }; shift 2 ;;
        *)       echo "usage: $0 [--check] [--only <BlockClass>]" >&2; exit 2 ;;
    esac
done

[ -s "$TABLE" ] || {
    echo "shape-three: no $TABLE — run ci/gen-gameplay-tables.sh first" >&2
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
    "$ROOT/tools/goldengen/vanilla/ShapeThreeGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx8G -Drustmc.data="$VDIR/generated-data" \
        -Drustmc.shape3.only="$ONLY" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.ShapeThreeGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree — `ci/shape-golden.sh`'s rule, and this harness is
# parallel across subjects for the same reason and so has the same exposure.
# `--check` has the committed census as its second opinion and runs once; a
# `--only` measuring run is not an artefact anyone keeps and runs once too.
run "$WORK/three.json"
if [ "$MODE" != check ] && [ -z "$ONLY" ]; then
    run "$WORK/again.json"
    cmp -s "$WORK/three.json" "$WORK/again.json" || {
        echo "two runs of ShapeThreeGolden disagree — something in it depends on thread order" >&2
        exit 1
    }
fi

# The assertion. See the header.
if [ -z "$ONLY" ]; then
    python3 - "$WORK/three.json" "$TABLE" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
t = json.load(open(sys.argv[2]))
if c["states"] != t["states"] or c["refusedPairs"] != t["refusedPairs"]:
    print("shape-three: the extract's narrow probe disagrees with update_shape.json",
          file=sys.stderr)
    print(f"  states:      extract {c['states']}, table {t['states']}", file=sys.stderr)
    print(f"  refusedPairs: extract {c['refusedPairs']}, table {t['refusedPairs']}",
          file=sys.stderr)
    sys.exit(1)
PY
else
    # THE SAME ASSERTION, FOR THE PATH THAT PUBLISHED FIVE NUMBERS WITHOUT IT.
    #
    # A whole-table run asserts its re-derived refusal set against
    # `update_shape.json` above, and refuses when the two harnesses have
    # drifted. `--only` used to exit before ever reaching that block, so every
    # per-class figure in the roadmap's §83.9 table — four classes, 25,740
    # pairs — was a count of whatever *this* harness's discovery happened to
    # find, with nothing anywhere comparing it to the set the table actually
    # refuses. The numbers were right. Nothing established that they were, and
    # the whole point of `--only` is to publish a figure before paying for the
    # whole run, which makes it the path that most needs the check.
    #
    # The per-class denominator is not in `update_shape.json` — that table
    # carries one total — so this reads `byClass` out of the committed
    # refusal census, which is the artefact that has it. That makes the
    # assertion conditional on the census being present, and a missing census
    # is a skip with a printed reason rather than a silent pass: a check that
    # quietly does nothing when its input is absent is the defect this repo
    # keeps finding in its own tree, and it is not being added here on purpose.
    python3 - "$WORK/three.json" "$ROOT/ci/checks/shape-refusal-census.json" <<'PY'
import json, os, sys
c = json.load(open(sys.argv[1]))
if not os.path.exists(sys.argv[2]):
    print("shape-three --only: no ci/checks/shape-refusal-census.json — the "
          "per-class count is NOT checked against the table this run", file=sys.stderr)
    sys.exit(0)
d = json.load(open(sys.argv[2]))
by = {r["class"]: r["pairs"] for r in d["byClass"]}
want = by.get(c["only"])
if want is None:
    print(f"shape-three --only: {c['only']!r} is not a class the census refuses "
          f"anything in — it has no refusals to widen", file=sys.stderr)
    sys.exit(1)
if c["refusedPairs"] != want:
    print("shape-three --only: the extract's narrow probe disagrees with the "
          "refusal census", file=sys.stderr)
    print(f"  {c['only']}: extract {c['refusedPairs']}, census {want}", file=sys.stderr)
    print("  the two harnesses have drifted and this run measured a different "
          "set than update_shape.json refuses", file=sys.stderr)
    sys.exit(1)
if c["states"] != d["states"]:
    print(f"shape-three --only: states differ — extract {c['states']}, "
          f"census {d['states']}", file=sys.stderr)
    sys.exit(1)
print(f"shape-three --only: {c['only']} refusal set agrees with the census "
      f"at {want} pairs")
PY
    # THE DETERMINISM HALF, WHICH `--only` DOES NOT HAVE AND CANNOT AFFORD.
    #
    # A whole-table run generates twice and `cmp`s, because a table that
    # differs between two runs of one generator is a finding that outranks
    # whatever it was generated to measure. `--only` runs ONCE — the comment
    # above the `run` calls says so, and doubling a forty-five minute
    # measurement to check itself is not the trade.
    #
    # It does not have to be. A whole-table run already measures every class,
    # under a **different work partition** — `--only` parallelises across one
    # class's subjects, the whole run across all of them — so its `byClass`
    # entry is an independent second measurement of exactly this number, and a
    # free one wherever a whole-table census exists. That is the cross-check
    # §83.9 claims for its per-class table, performed rather than asserted.
    #
    # Where no whole-table census exists this says so. A single-run figure is
    # publishable — §83.9's four rows are — but it is a different thing from a
    # figure two runs agreed on, and the difference belongs in the output
    # rather than in whoever remembers it.
    if [ -s "$CENSUS" ]; then
        python3 - "$WORK/three.json" "$CENSUS" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
w = json.load(open(sys.argv[2]))
if w.get("only"):
    print(f"shape-three --only: the committed census is itself a --only "
          f"{w['only']} run; NOT cross-checked", file=sys.stderr)
    sys.exit(0)
row = next((r for r in w.get("byClass", []) if r["class"] == c["only"]), None)
if row is None:
    print(f"shape-three --only: {c['only']} is absent from the whole-table "
          f"census's byClass — the two runs disagree about which classes "
          f"refuse at all", file=sys.stderr)
    sys.exit(1)
mine = {k: v for k, v in c["verdicts"].items() if v}
theirs = {k: v for k, v in row["verdicts"].items() if v}
if mine != theirs or row["pairs"] != c["refusedPairs"]:
    print(f"shape-three --only: this run and the whole-table run disagree "
          f"about {c['only']}", file=sys.stderr)
    print(f"  --only      {c['refusedPairs']} pairs {mine}", file=sys.stderr)
    print(f"  whole table {row['pairs']} pairs {theirs}", file=sys.stderr)
    print("  Two runs of one generator over one jar produced two answers. That "
          "outranks", file=sys.stderr)
    print("  whatever this run was measuring: fix the harness before quoting "
          "either.", file=sys.stderr)
    sys.exit(1)
print(f"shape-three --only: agrees with the whole-table run's byClass entry "
      f"for {c['only']} — {row['pairs']} pairs, {theirs}")
PY
    else
        echo "shape-three --only: no $CENSUS, so this is a SINGLE run and its"
        echo "  verdicts are cross-checked by nothing. Run ci/shape-three.sh to"
        echo "  get the whole-table byClass entry that would check them."
    fi
fi

# The census is the table minus the rows. Derived here rather than emitted
# separately so the two can never describe different runs.
python3 - "$WORK/three.json" "$WORK/census.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
census = {k: v for k, v in d.items() if k != "table"}
with open(sys.argv[2], "w") as f:
    json.dump(census, f, indent=2, sort_keys=True)
    f.write("\n")
PY

if [ -n "$ONLY" ]; then
    python3 - "$WORK/three.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(f"shape-three --only {d['only']}: {d['refusedPairs']} refused pairs, "
      f"{d['probes']:,} probes, {d['rows']} rows")
print(f"  verdicts {d['verdicts']}")
print(f"  extra classes per pair {d['byExtraClassCount']}")
# The cause is the whole point of a per-class run: "3,072 refused" says the
# class resisted and says nothing about what it wanted, and what it wanted is
# the offset a fourth position would have to answer.
for r in sorted(d["stillRefused"], key=lambda r: -r["pairs"]):
    print(f"    {r['pairs']:>7}  {r['cause']}")
for r in sorted(d["byOffset"], key=lambda r: -r["pairs"]):
    print(f"    at {r['at']:<10} {r['pairs']:>7}  {r['verdicts']}")
PY
    exit 0
fi

if [ "$MODE" = check ]; then
    for f in "$OUT" "$CENSUS"; do
        [ -s "$f" ] || { echo "shape-three: $f is missing — run ci/shape-three.sh" >&2; exit 1; }
    done
    if ! cmp -s "$WORK/census.json" "$CENSUS"; then
        echo "SHAPE-THREE-CENSUS DRIFTED from the jar — re-run ci/shape-three.sh" >&2
        diff "$CENSUS" "$WORK/census.json" | head -40 >&2
        exit 1
    fi
    if ! cmp -s "$WORK/three.json" "$OUT"; then
        echo "update_shape3.json DRIFTED from the jar — re-run ci/shape-three.sh" >&2
        exit 1
    fi
    python3 - "$CENSUS" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
v = d["verdicts"]
print(f"SHAPE-THREE OK — {d['refusedPairs']} refused pairs, {v['closed']} closed, "
      f"{v['connective']} connective, {v['inert']} inert, {v['refused']} still refused")
PY
    exit 0
fi

mkdir -p "$(dirname "$OUT")" "$(dirname "$CENSUS")"
mv "$WORK/three.json" "$OUT"
mv "$WORK/census.json" "$CENSUS"

# Stamp it, for the reason `ci/use-item-on-golden.sh` gives at length: a table
# generated by hand and left unstamped is a table `prov_check` calls foreign,
# and the red it produces points at `ci/gen-gameplay-tables.sh`, which will not
# generate this one and so cannot repair it. Nothing checks this table's stamp
# today — it is not in the required set, because no source names it — and it is
# written anyway, because "nothing checks it yet" is how the last one got here.
if [ -f "$ROOT/ci/provenance.sh" ]; then
    . "$ROOT/ci/provenance.sh"
    prov_stamp "$ROOT" "$OUT" "ci/shape-three.sh"
fi
python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
v = d["verdicts"]
n = d["refusedPairs"]
print(f"update_shape3 -> {sys.argv[1]} (identical across two runs)")
print(f"  {n} refused (state, direction) pairs re-derived from the jar")
print(f"  {d['probes']:,} probes, neighbour breadth {d['neighbourBreadth']}, "
      f"third-position breadth {d['extraBreadth']}")
for k in ("closed", "connective", "inert", "refused"):
    print(f"    {v[k]:>7}  {v[k]*100.0/n:5.1f}%  {k}")
print(f"  {d['rows']} rows emitted")
print("  by the position the rule read:")
for r in sorted(d["byOffset"], key=lambda r: -r["pairs"]):
    print(f"    {r['pairs']:>7}  {r['at']:<10} {r['verdicts']}")
print("  what three positions still could not answer:")
for r in d["stillRefused"]:
    print(f"    {r['pairs']:>7}  {r['cause']}")
PY
