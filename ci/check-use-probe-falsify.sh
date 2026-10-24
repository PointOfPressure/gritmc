#!/usr/bin/env bash
# check-use-probe-falsify.sh — prove `ci/check-use-probe.py` can go red.
#
#     ./ci/check-use-probe-falsify.sh          # ~1 s, no jar, no build, no box
#
# A gate that cannot fail is not a gate, and this project has shipped three of
# them: `ci/t0.sh` ran `--lib` against a binary crate for weeks, a datapack grep
# "proved" bearding inert because the marker is never in any datapack, and
# `ci/t2-gameplay.sh` sent a game rule vanilla did not recognise and never read
# the reply. The check this falsifies exists to stop a golden table recording
# one player's behaviour as the block's — so it is exactly the kind of check
# whose own silence would be indistinguishable from success.
#
# **The fixture is synthetic and that is deliberate.** Run against the real
# `data/vanilla/use_without_item.json` this would be hostage to a tree having
# generated one, and T0 runs in trees that have not. A five-row fixture built
# here exercises every rule, so this is hermetic and belongs in T0. The real
# table is used as an extra green control when it happens to be present, which
# is the half a fixture cannot give you: that the shape the dumper writes is
# still the shape the checker parses.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT/ci/provenance.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
PASS=0
FAIL=0

cat > "$WORK/table.json" <<'JSON'
{
  "what": "fixture",
  "probeDependentBlocks": 2,
  "probeStances": [{"name": "a"}, {"name": "b"}, {"name": "c"}],
  "probeStancesUnavailable": {"d": "no components"},
  "table": [
    {"block": "fix:clean", "class": "CleanBlock",
     "probeDependent": false, "probeAxes": [], "probeUnknown": []},
    {"block": "fix:moves", "class": "MovesBlock",
     "probeDependent": true, "probeAxes": ["a", "b"], "probeUnknown": []},
    {"block": "fix:unread", "class": "UnreadBlock",
     "probeDependent": false, "probeAxes": [], "probeUnknown": ["c"]}
  ]
}
JSON

cat > "$WORK/list.toml" <<'TOML'
[meta]
recorded = "fixture"
stances = 4
unavailable = ["d"]

[[block]]
id = "fix:moves"
class = "MovesBlock"
axes = ["a", "b"]
unknown = []

[[block]]
id = "fix:unread"
class = "UnreadBlock"
axes = []
unknown = ["c"]
TOML

# `expect green|red  name  <table> <list>`
expect() {
    local want="$1" name="$2" t="$3" l="$4" out rc
    out="$("$ROOT/ci/check-use-probe.py" "$t" "$l" 2>&1)"
    rc=$?
    if { [ "$want" = green ] && [ "$rc" -eq 0 ]; } || { [ "$want" = red ] && [ "$rc" -ne 0 ]; }; then
        PASS=$((PASS + 1))
        printf '  ok    %-34s %.60s\n' "$name" "$(echo "$out" | sed -n '2p;$p' | head -1)"
    else
        FAIL=$((FAIL + 1))
        printf '  FAIL  %-34s wanted %s, got rc=%d\n' "$name" "$want" "$rc"
        echo "$out" | sed 's/^/        /' | head -5
    fi
}

echo "check-use-probe-falsify: synthetic fixture"
expect green "control-before" "$WORK/table.json" "$WORK/list.toml"

# 1. NEW — a probe-dependent block nobody has written down. The whole point.
grep -v '^id = "fix:moves"' "$WORK/list.toml" | sed 's/^class = "MovesBlock"/id = "fix:other"\nclass = "MovesBlock"/' > "$WORK/new.toml"
expect red "new-block-not-listed" "$WORK/table.json" "$WORK/new.toml"

# 2. MOVED — a listed block that became wrong in a NEW way. Membership is not
#    enough: "already known to be wrong" is a different finding from "wrong in
#    a way nobody has looked at".
sed 's/^axes = \["a", "b"\]/axes = ["a"]/' "$WORK/list.toml" > "$WORK/axes.toml"
expect red "listed-block-gained-an-axis" "$WORK/table.json" "$WORK/axes.toml"

# 3. MOVED on the other list — a stance that went from "different" to "unread".
sed 's/^unknown = \["c"\]/unknown = []/' "$WORK/list.toml" > "$WORK/unknown.toml"
expect red "unknown-list-drifted" "$WORK/table.json" "$WORK/unknown.toml"

# 4. STALE — an allowance for a block that is no longer probe-dependent.
{ cat "$WORK/list.toml"; printf '\n[[block]]\nid = "fix:gone"\nclass = "X"\naxes = ["a"]\nunknown = []\n'; } > "$WORK/stale.toml"
expect red "stale-allowance" "$WORK/table.json" "$WORK/stale.toml"

# 5. The sweep stopped sweeping. This is the injection that matters most: a
#    stance list that shrank makes every row look constant, and without this the
#    checker would agree with a table that had measured nothing.
sed 's/^stances = 4/stances = 9/' "$WORK/list.toml" > "$WORK/stances.toml"
expect red "sweep-shrank" "$WORK/table.json" "$WORK/stances.toml"

# 6. An axis that silently stopped being turned.
sed 's/^unavailable = .*/unavailable = []/' "$WORK/list.toml" > "$WORK/unavail.toml"
expect red "unavailable-stance-drifted" "$WORK/table.json" "$WORK/unavail.toml"

expect green "control-after" "$WORK/table.json" "$WORK/list.toml"

# The one thing a fixture cannot check: that the dumper still writes the shape
# this checker reads. Only when a table has been generated in this tree.
#
# **Both tables, and the second one is not a duplicate of the first.**
# `use_item_on.json` is written by a different dumper with a different cell
# shape — one `default` per state plus the exceptions, because its second axis
# is the whole item registry — and the only reason one checker serves both is
# that `UseItemOnGolden` writes the probe columns with the same names and the
# same meanings. That is a property of the dumper, which is to say it is a
# thing that can stop being true, which is to say it belongs here rather than
# in a comment.
# **A TABLE THIS TREE DID NOT GENERATE IS NOT EVIDENCE ABOUT THIS TREE'S
# DUMPER — 2026-08-13.** The controls below used to run against whatever sat at
# that path. On every freshly provisioned tree that is *another tree's* copy: it
# arrives through the `cp -rn ~/rustmc/data/vanilla/.` in `docs/AGENT-BRIEF.md`'s
# setup snippet, and the copy on the box is dated 2026-08-08 and predates the
# stance sweep entirely, so it carries no `probeStances` key at all.
# `ci/check-use-probe.py` then computed `0 + 1` stances against a pin of 12 and
# `ci/t0.sh:229` died reporting **"the dumper swept 1 stances"** — a statement
# about a dumper that had not run, made by a check that had measured nothing.
#
# The reason it had not run is worth writing down here, because the message
# pointed away from it: `ci/gen-gameplay-tables.sh` runs its nineteen generators
# in sorted order, `ci/dump-recipe-unlocks.sh` is the twelfth and calls
# `cargo build -q -p oracle-runner`, and the setup snippet's `ssh` does not put
# `$HOME/.cargo/bin` on PATH. So the run died at 12 of 19 and
# `use_without_item.json` — which sorts **nineteenth** — was never reached.
#
# What that cost is the whole of this branch: every stage after `ci/t0.sh:229`
# — `cargo fmt`, `clippy`, `cargo check`, all twenty-nine test binaries and the
# entire `../plugins` contract block — had not executed in a freshly provisioned
# tree since 2026-08-12, and at least two agents read the red as a defect in the
# branch under test.
#
# So the stamp is consulted first. Absent table: skipped, exactly as before.
# Present but not this tree's: still **RED**, because a tree reading another
# branch's golden tables must never print `T0 GREEN` — but red about the right
# thing, and naming the repair instead of an innocent dumper.
real_table() {
    local name="$1" tbl="$2" list="$3" why
    if [ ! -s "$tbl" ]; then
        printf '  --    %-34s no %s in this tree\n' "$name" "${tbl#"$ROOT"/}"
        return
    fi
    if ! why="$(prov_check "$ROOT" "$tbl")"; then
        FAIL=$((FAIL + 1))
        printf '  FAIL  %-34s %s\n' "$name" "${tbl#"$ROOT"/}: $why"
        echo   "        This tree did not generate that table, so nothing below it is a"
        echo   "        reading of this tree's dumper. Repair: ./ci/gen-gameplay-tables.sh"
        echo   "        — and if that stops early, cargo must be on PATH:"
        echo   '        export PATH=$HOME/.cargo/bin:$PATH'
        return
    fi
    expect green "$name" "$tbl" "$list"
}

real_table "real-table" "$ROOT/data/vanilla/use_without_item.json" \
    "$ROOT/ci/checks/use-probe-dependent.toml"
real_table "real-table-use-item-on" "$ROOT/data/vanilla/use_item_on.json" \
    "$ROOT/ci/checks/use-item-on-probe-dependent.toml"

# The guard above is itself a check, so it gets an injection — adding an
# unfalsifiable guard to a falsification harness is the one way to make this
# file worse. An unstamped table must refuse; that is the direction that
# matters, and the green direction is exercised by the real tables whenever a
# tree has actually generated them.
mkdir -p "$WORK/data/vanilla"
cp "$WORK/table.json" "$WORK/data/vanilla/use_without_item.json"
if prov_check "$WORK" "$WORK/data/vanilla/use_without_item.json" > /dev/null 2>&1; then
    FAIL=$((FAIL + 1))
    printf '  FAIL  %-34s an unstamped table passed prov_check\n' "unstamped-table-refused"
else
    PASS=$((PASS + 1))
    printf '  ok    %-34s %.60s\n' "unstamped-table-refused" \
        "$(prov_check "$WORK" "$WORK/data/vanilla/use_without_item.json" 2>&1)"
fi

# The second allowlist, against the same synthetic fixture. Its `[meta]` is a
# separate declaration of the same twelve stances and could drift from the
# first one silently — an allowlist pinned to the wrong sweep width is the
# `sweep-shrank` injection above, arriving by copy-paste instead of by edit.
# So the shipped file's own `[meta]` is checked against the fixture's table,
# which knows how many stances a sweep has.
python3 - "$ROOT/ci/checks/use-item-on-probe-dependent.toml" \
    "$ROOT/ci/checks/use-probe-dependent.toml" <<'PY'
import sys, tomllib
a, b = (tomllib.load(open(p, "rb")).get("meta", {}) for p in sys.argv[1:3])
for key in ("stances", "unavailable"):
    if a.get(key) != b.get(key):
        sys.exit("  FAIL  allowlist-meta-agree               %s: %r vs %r"
                 % (key, a.get(key), b.get(key)))
print("  ok    allowlist-meta-agree               both pin stances=%s unavailable=%s"
      % (a.get("stances"), a.get("unavailable")))
PY
if [ $? -eq 0 ]; then PASS=$((PASS + 1)); else FAIL=$((FAIL + 1)); fi

echo "check-use-probe-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
