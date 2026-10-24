#!/usr/bin/env bash
# floor-census-falsify.sh — prove `ci/floor-census.py --selftest` can go RED.
#
# WHY THIS EXISTS
#
# `floor-census.py` carries a self-test that asserts printed arithmetic on an
# asymmetric fixture, and the commit that added the surplus-direction floor
# says the fixture was mutation-tested by hand and found blind to two of nine
# mutations. That is the right thing to have done and the wrong place to leave
# it: a hand mutation test protects the code as it stood that afternoon, and
# the next person to add a branch to this script has no way to check that the
# fixture reaches it. AGENT-BRIEF's rule — a gate that cannot fail is not a
# gate — applies to a self-test exactly as it applies to a gate, and this
# project has caught six instruments reporting green over code they never ran.
#
# So the mutations are written down and re-run. Each one is a single textual
# substitution into a COPY of the script that a reader can check is a genuine
# defect and not a typo, and each must turn the self-test red. A mutation that
# SURVIVES is the finding: it names an assertion the fixture does not make.
#
# `admissible-residual.py` is copied beside the mutant because `floor-census.py`
# borrows its parser from its own directory, and a mutant left in `ci/` would
# be a tracked file for as long as the run takes.
#
# usage: ci/floor-census-falsify.sh
# exit 0 every mutation was caught, 1 one or more survived, 2 setup failed.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
SRC=ci/floor-census.py
[ -f "$SRC" ] || { echo "no $SRC" >&2; exit 2; }

TMP=$(mktemp -d) || exit 2
trap 'rm -rf "$TMP"' EXIT
cp ci/admissible-residual.py "$TMP/" || exit 2

# The control. If the unmutated copy is not green in the temp directory then
# every "caught" verdict below is really a broken copy, and the whole run would
# pass while measuring nothing.
cp "$SRC" "$TMP/mut.py"
if ! python3 "$TMP/mut.py" --selftest > "$TMP/control.log" 2>&1; then
    echo "CONTROL RED — the unmutated copy fails its own self-test:" >&2
    tail -20 "$TMP/control.log" >&2
    exit 2
fi
echo "CONTROL GREEN — unmutated copy passes, so a RED below is the mutation"

pass=0; fail=0
mutate() { # label | old | new
    local lbl="$1" old="$2" new="$3"
    python3 - "$SRC" "$TMP/mut.py" "$old" "$new" <<'PY'
import sys
src, dst, old, new = sys.argv[1:5]
body = open(src, encoding="utf-8").read()
n = body.count(old)
if n != 1:
    print(f"PATTERN-MISS {n} occurrences", file=sys.stderr)
    sys.exit(3)
open(dst, "w", encoding="utf-8").write(body.replace(old, new))
PY
    case $? in
        0) ;;
        3) printf '  %-52s PATTERN NOT UNIQUE — mutation not applied\n' "$lbl"
           fail=$((fail + 1)); return;;
        *) printf '  %-52s SETUP FAILED\n' "$lbl"; fail=$((fail + 1)); return;;
    esac
    if python3 "$TMP/mut.py" --selftest > "$TMP/m.log" 2>&1; then
        printf '  %-52s SURVIVED — the fixture does not test this\n' "$lbl"
        fail=$((fail + 1))
    else
        printf '  %-52s caught\n' "$lbl"
        pass=$((pass + 1))
    fi
}

echo
echo "=== mutations of the FLOOR construction ==="
# max -> min, and max -> "last one wins". Both were live defects in the first
# version of this fixture and both survived it.
mutate "per-block floor takes min instead of max" \
    "            block_floor[k] = max(block_floor[k], v)" \
    "            block_floor[k] = min(block_floor[k], v) if block_floor[k] else v"
mutate "per-block floor is last-one-wins" \
    "            block_floor[k] = max(block_floor[k], v)" \
    "            block_floor[k] = v"
mutate "surplus floor takes min instead of max" \
    "            placed_floor[k] = max(placed_floor[k], v)" \
    "            placed_floor[k] = min(placed_floor[k], v) if placed_floor[k] else v"
mutate "innocent charge is a union, not an intersection" \
    "    return set.intersection(*[set(files[(r, x)]) for r in others])" \
    "    return set.union(*[set(files[(r, x)]) for r in others])"

echo
echo "=== mutations of the MISS/SURPLUS split — the 439-at-2.5x class ==="
# The whole point of two tables. Each of these makes the script quote one
# direction against the other's floor, which is the mistake that reported a
# 439-block item at 2.5x when its matched pair says 1.06x.
mutate "surplus table keyed on the VANILLA spelling" \
    "            cand_p[short(files[(sub[0], adjudicate)][p][1])] += 1" \
    "            cand_p[short(files[(sub[0], adjudicate)][p][0])] += 1"
mutate "surplus charge looked up in the MISS floor" \
    "            f = placed_floor.get(b, 0)" \
    "            f = block_floor.get(b, 0)"
mutate "miss charge looked up in the SURPLUS floor" \
    "            f = block_floor.get(b, 0)" \
    "            f = placed_floor.get(b, 0)"
mutate "innocent surplus census keyed on the miss spelling" \
    "            o = short(files[(first, x)][p][1])" \
    "            o = short(files[(first, x)][p][0])"

# THE NAMED HAZARD. The floor tables are keyed on the FULL BLOCK STATE, and a
# charge pooled to the bare block name misses the lookup and reports a floor of
# ZERO — silently, and in the unsafe direction. Until 2026-08-10 every block in
# the fixture was unstated, so this mutation was invisible to the self-test.
mutate "block state stripped from the lookup key" \
    '    return name.replace("minecraft:", "")' \
    '    return name.replace("minecraft:", "").split("[")[0]'

echo
echo "=== mutations of the JOIN — each position takes the WEAKER verdict ==="
mutate "join takes the stronger of the two verdicts" \
    "                    min(
                        _RANK[verdict(cand_t[vb], block_floor.get(vb, 0))]," \
    "                    max(
                        _RANK[verdict(cand_t[vb], block_floor.get(vb, 0))],"
mutate "join ignores the surplus direction" \
    "                        _RANK[verdict(cand_p[ob], placed_floor.get(ob, 0))]," \
    "                        _RANK[verdict(cand_t[vb], block_floor.get(vb, 0))],"
mutate "join ignores the miss direction" \
    "                        _RANK[verdict(cand_t[vb], block_floor.get(vb, 0))]," \
    "                        _RANK[verdict(cand_p[ob], placed_floor.get(ob, 0))],"

echo
echo "=== mutations of the VERDICT thresholds ==="
# `max(floor, 1)` is what stops a charge of 1 on a floor of 0 reading as CLEAR.
mutate "a floor of zero is treated as proof of impossibility" \
    "    if charge > 2 * max(floor, 1):" \
    "    if charge > 2 * floor:"
mutate "CLEAR threshold is 1x rather than 2x its floor" \
    "    if charge > 2 * max(floor, 1):" \
    "    if charge > max(floor, 1):"
mutate "marginal collapses into at-or-under" \
    "    if charge > floor:
        return \"marginal\"" \
    "    if False:
        return \"marginal\""

echo
echo "=== mutations of the STAMP guards — ORACLE rule 30 ==="
mutate "a derived stamp is recorded as an original one" \
    "        if why:
            derived[r] = why[0]" \
    "        if False:
            derived[r] = why[0]"
mutate "a pool with no stamped anchor is accepted" \
    "            if len(derived) == len(refs):" \
    "            if False:"
mutate "an unstamped reference is accepted" \
    "            raise ar.Malformed(
                f\"{p}: no HARVEST-ORDER stamp ({exc.strerror}). A reference \"" \
    "            gen[r] = dump[r] = \"unknown\"
            continue
            raise ar.Malformed(
                f\"{p}: no HARVEST-ORDER stamp ({exc.strerror}). A reference \""

echo
echo "  caught $pass, survived $fail"
if [ "$fail" = 0 ]; then
    echo "FLOOR-CENSUS-FALSIFY GREEN — every mutation was caught"
    exit 0
fi
echo "FLOOR-CENSUS-FALSIFY RED — a mutation survived; the fixture is blind to it"
exit 1
