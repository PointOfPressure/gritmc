#!/usr/bin/env bash
# Prove `ci/projection-drops.py` refuses each defect it claims to catch.
#
# A guard nobody has watched catch something is a guard, and this repo's
# central defect is exactly the check that cannot fail. So every refusal the
# script advertises is reconstructed here against a copy of the real tree: an
# unmarked discard, a marker with no ledger row, a ledger row with no site, a
# stale observer count, and a stale observer name. The control is the tree
# itself, unmodified, which must pass.
#
# Copies rather than edits: the mutations go into `$(mktemp -d)` and the
# checker is pointed at it with `PROJECTION_DROPS_ROOT`. Nothing here writes to
# the working tree, so it is safe to run at any time and needs no lock, no
# server and no recordings.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2
ROOT=$PWD
PASS=0
FAIL=0

run() { PROJECTION_DROPS_ROOT="$1" python3 "$ROOT/ci/projection-drops.py" >"$2" 2>&1; }

# `want <expect-exit> <name> <mutation-fn>`
want() {
    local expect=$1 name=$2 fn=$3
    local dir out
    dir=$(mktemp -d) || exit 2
    mkdir -p "$dir/ci/checks" "$dir/tools/oracle-runner/src" "$dir/crates/rustmc-proto/src"
    cp "$ROOT/ci/checks/projection-drops.toml" "$dir/ci/checks/"
    cp -r "$ROOT/ci/checks/gameplay" "$dir/ci/checks/"
    cp "$ROOT/tools/oracle-runner/src/trace.rs" "$ROOT/tools/oracle-runner/src/compare.rs" \
        "$dir/tools/oracle-runner/src/"
    # The packet id table, without which no `packet:0xNN` can be resolved to a
    # constant. The checker refuses outright when it is missing rather than
    # letting every id through unresolved, which is how this line came to be
    # needed: the `housekeeping-kind` case passed vacuously until it existed.
    cp "$ROOT/crates/rustmc-proto/src/ids.rs" "$dir/crates/rustmc-proto/src/"
    "$fn" "$dir"
    out=$(mktemp)
    run "$dir" "$out"
    local got=$?
    if [ "$got" = "$expect" ]; then
        PASS=$((PASS + 1))
        printf 'ok    %-34s exit %s\n' "$name" "$got"
    else
        FAIL=$((FAIL + 1))
        printf 'FAIL  %-34s exit %s, wanted %s\n' "$name" "$got" "$expect"
        sed 's/^/        /' "$out"
    fi
    rm -rf "$dir" "$out"
}

control() { :; }

# A new discard nobody marked: the defect this whole file exists for. A field
# that was in `rest` and being compared silently stops being either.
unmarked() {
    python3 - "$1" <<'PY'
import sys
p = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
old = '            f.insert("block".into(), num(read_varint(&mut r).ok()?));'
assert s.count(old) == 1
open(p, "w", encoding="utf-8").write(
    s.replace(old, old + "\n            let _extra = read_u8(&mut r).ok()?;")
)
PY
}

# Marked, but nobody ranked it. Half of the discipline is the ledger row.
undeclared() {
    python3 - "$1" <<'PY'
import sys
p = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
old = '            f.insert("block".into(), num(read_varint(&mut r).ok()?));'
open(p, "w", encoding="utf-8").write(
    s.replace(
        old,
        old + "\n            // DROP: block_event.mystery — invented by the falsifier"
        "\n            let _extra = read_u8(&mut r).ok()?;",
    )
)
PY
}

# The site goes away and the row stays: a ledger claiming coverage of a drop
# that no longer exists reads as an audited decoder and is not one.
stale_row() {
    python3 - "$1" <<'PY'
import sys
p = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
old = """            // DROP: sound.seed — `soundSeedGenerator.nextLong()`, one fresh draw per sound
            let _seed = read_i64(&mut r).ok()?;"""
assert s.count(old) == 1
open(p, "w", encoding="utf-8").write(s.replace(old, "            let seed_kept = read_i64(&mut r).ok()?;\n            f.insert(\"seed\".into(), num(seed_kept));"))
PY
}

# A check file grows an `observe` list and the ranking does not follow it. This
# is the refusal that makes the ledger more than a list, because check files
# move far more often than the decoder does.
# Widened by editing an existing `observe` list, not by appending a second one
# to the end of a file. A check file is read with `^observe = [...]` and the
# first match wins, so an appended list is invisible and the mutation is a
# no-op — which is a falsifier case that passes by doing nothing, the very
# thing this directory exists to refuse.
stale_count() {
    local f
    # A check that has a one-line `observe` list and does NOT already name
    # `sound`. Both halves matter: the kind added has to be one the ledger
    # actually ranks, or the count it moves is a count no row declares and the
    # mutation is invisible. `packet:0xFE` was the first attempt and did
    # nothing for exactly that reason.
    f="$(grep -L '"sound"' $(grep -l '^observe = \[' "$1"/ci/checks/gameplay/*.toml) | head -1)"
    [ -n "$f" ] || return 0
    sed -i '0,/^observe = \[/s//observe = [\n    "sound",/' "$f"
}

# The count is right and a name is wrong — a rename the ranking did not see.
stale_name() {
    python3 - "$1" <<'PY'
import sys
p = sys.argv[1] + "/ci/checks/projection-drops.toml"
s = open(p, encoding="utf-8").read()
assert "112-a-player-says-something" in s
open(p, "w", encoding="utf-8").write(s.replace("112-a-player-says-something", "112-a-player-speaks"))
PY
}

# A check asking to see a kind no arm of the decoder emits. Its projection is
# empty on both sides in every run, so the comparison is between two empty
# lists and the green is manufactured by the check file itself.
unemittable_kind() {
    python3 - "$1" <<'PYEOF'
import glob, sys
p = sorted(glob.glob(sys.argv[1] + "/ci/checks/gameplay/*.toml"))[0]
s = open(p, encoding="utf-8").read()
open(p, "w", encoding="utf-8").write(
    s.replace("observe = [", 'observe = [\n    "level_chunk_with_light",', 1)
)
PYEOF
}

# The same, by the other door: a kind dropped at RECORD time. `observe`
# returns `None` for it before `decode` is reached, so it cannot appear even
# under the `packet:0xNN` fallback that catches every other unknown id — which
# is exactly what makes it look like a plausible thing to ask for.
#
# Since 2026-09-24 (`recorder-housekeeping`) the real tree drops NOTHING at
# record time, so this case first puts one drop back — `PING`, in the only
# shape a record-time drop can take now, an early `return None` in `observe` —
# and then takes the id from the checker's OWN table, as it always has. It was
# a literal `packet:0x2D` until 2026-09-23 and went stale once already (see
# main's b286f2697); reading the checker's table is what stops that. An empty
# table still exits non-zero before mutating the check, which leaves the check
# files untouched and the case red.
housekeeping_kind() {
    python3 - "$1" <<'PYEOF'
import sys
p = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
a = ") -> Option<(String, BTreeMap<String, serde_json::Value>)> {\n    if let Some((kind, mut f, consumed)) = decode("
assert s.count(a) == 1, "anchor moved — this case has stopped reconstructing anything"
head, rest = a.split("\n", 1)
open(p, "w", encoding="utf-8").write(
    s.replace(a, head + "\n    if id == ids::play::cb::PING {\n        return None;\n    }\n" + rest, 1)
)
PYEOF
    PROJECTION_DROPS_ROOT="$1" python3 - "$1" "$ROOT/ci" <<'PYEOF'
import glob, importlib.util, sys
spec = importlib.util.spec_from_file_location("pd", sys.argv[2] + "/projection-drops.py")
pd = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pd)
hk = pd.housekeeping_ids()
if not hk:
    sys.exit("housekeeping_kind: the checker resolved no record-time drops")
pid = min(hk.values())
p = sorted(glob.glob(sys.argv[1] + "/ci/checks/gameplay/*.toml"))[0]
s = open(p, encoding="utf-8").read()
open(p, "w", encoding="utf-8").write(
    s.replace("observe = [", f'observe = [\n    "packet:0x{pid:02X}",', 1)
)
PYEOF
}

# A family made ambient in `trace::FAMILIES` without the ledger's
# `ambient_unless_observed` row moving: a new whole-kind drop nobody ranked.
ambient_unledgered() {
    python3 - "$1" <<'PYEOF'
import sys
p = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
old = 'kind: "forget_level_chunk",\n        class: Class::Graded,'
assert s.count(old) == 1, "anchor moved"
open(p, "w", encoding="utf-8").write(
    s.replace(old, 'kind: "forget_level_chunk",\n        class: Class::Ambient,', 1)
)
PYEOF
}

# The ambient filter with its marker removed: a whole-kind discard with no
# site. Before the pattern for it existed the scanner could not see this line
# at all, so this case would have passed.
ambient_unmarked() {
    python3 - "$1" <<'PYEOF'
import re, sys
p = sys.argv[1] + "/tools/oracle-runner/src/compare.rs"
s = open(p, encoding="utf-8").read()
out, n = re.subn(r"\n\s*// DROP: ambient_unless_observed[^\n]*", "", s)
assert n == 2, f"expected two ambient markers, found {n}"
open(p, "w", encoding="utf-8").write(out)
PYEOF
}

# A normalisation row anchors on a function rather than a line, because the
# loss is the whole of what the function does. Renaming it must go stale, or
# the ledger would go on declaring a rewrite that had moved or gone.
stale_anchor() {
    sed -i 's/fn renumber_entities/fn renumber_entity_indices/' \
        "$1/tools/oracle-runner/src/compare.rs"
}

# --- the real one -----------------------------------------------------------
#
# `docs/AGENT-BRIEF.md`: "a guard that cannot be shown catching a real past
# instance is itself a check that cannot fail." `ci/unfalsifiable-falsify.py`
# reconstructs three shipped defects for exactly that reason. This is this
# guard's.
#
# `569323e` (2026-08-07) dropped `sound.pitch`. It was the right call on the
# evidence it had — `ChestBlockEntity.playSound` draws
# `nextFloat() * 0.1f + 0.9f` and one event gave five different pitches in a
# single corpus run — and it silently emptied claim 420, whose step is a
# `place` and whose only surviving informative observation was that sound, with
# the pitch as the one field separating subtract mode from compare mode. Nobody
# noticed for ten days.
#
# Reconstructed exactly: the classified insert removed, the read left in place
# as a bare discard. Under this guard that commit does not land.
historical_569323e() {
    python3 - "$1" <<'PYEOF'
import sys

p = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
now = """            let pitch = read_f32(&mut r).ok()?;
            if id == 0 {
                // An inline sound's pitch is the command's argument, not a
                // draw: compared as it stands.
                f.insert("pitch".into(), num(f64::from(pitch)));
            } else if let Some(v) = crate::pitch::observed_by_id(id - 1, pitch) {
                f.insert("pitch".into(), v);
            }"""
then = """            let _pitch = read_f32(&mut r).ok()?;"""
assert s.count(now) == 1, f"{s.count(now)} matches — the pitch arm moved"
open(p, "w", encoding="utf-8").write(s.replace(now, then))
PYEOF
}

# And the repair accepted: the same discard, marked and declared. A guard that
# refused both would refuse every honest fix too, and be worth nothing.
historical_569323e_repaired() {
    historical_569323e "$1"
    python3 - "$1" "$ROOT" <<'PYEOF'
import sys

t = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(t, encoding="utf-8").read()
old = """            let _pitch = read_f32(&mut r).ok()?;"""
new = """            // DROP: sound.pitch — reconstructed 569323e, for the falsifier
            let _pitch = read_f32(&mut r).ok()?;"""
open(t, "w", encoding="utf-8").write(s.replace(old, new))

led = sys.argv[1] + "/ci/checks/projection-drops.toml"

# The row's rank is DERIVED from the mutated tree's own check files, not
# typed. It was typed once — six names, correct on the day — and the case
# started failing the moment the corpus grew a sound observer, reporting a
# stale rank in the falsifier as though it were a defect in the checker.
# A falsifier that carries a hard-coded copy of the number under test is the
# same memory this ledger exists to refuse, one level up.
sys.path.insert(0, sys.argv[2] + "/ci")
import check_files as cf

obs = sorted(cf.observers(sys.argv[1] + "/ci/checks/gameplay").get("sound", []))
names = ", ".join(f'"{n}"' for n in obs)
s = open(led, encoding="utf-8").read()
s += f"""
[[drop]]
name = "sound.pitch"
kind = "sound"
observed_by_count = {len(obs)}
stage = "decode"
wire = "one float"
why = "reconstructed 569323e, for the falsifier"
impact = "decides"
residual = "none"
observed_by = [{names}]
"""
open(led, "w", encoding="utf-8").write(s)
PYEOF
}

# --- the second real one ----------------------------------------------------
#
# `stage` says where a field is removed, and that is the field's
# RECOVERABILITY: a `decode` drop is gone from `rest` and needs the recorder
# rebuilt, a `project` drop was recorded and needs a projection edited. It was
# the one column of this ledger nothing compared against the tree.
#
# Reconstructed exactly. `a60e7b78` (2026-08-18) moved `add_entity.movement`
# from `trace::decode` to `compare::drop_add_entity_movement`. The ledger lived
# on a different branch and went on saying `decode`, while
# `ci/projection-drops.py --list` printed `project` for the same row in the same
# run. Under this guard that pair does not land.
misfiled_stage() {
    python3 - "$1" <<'PYEOF'
import re
import sys

p = sys.argv[1] + "/ci/checks/projection-drops.toml"
s = open(p, encoding="utf-8").read()
# The row as it ships, then its `stage` put back to the pre-move value.
m = re.search(r'(name = "add_entity\.movement".*?\n)stage = "project"', s, re.S)
assert m, "the movement row is not filed as a project drop — this case is stale"
open(p, "w", encoding="utf-8").write(
    s[: m.end(1)] + 'stage = "decode"' + s[m.end() :]
)
PYEOF
}

# A stage outside the vocabulary. `decode`, `project` and `informative` are the
# three places a field can go, and a typo in that column is a row that ranks
# itself against a stage the harness does not have.
bogus_stage() {
    sed -i '0,/^stage = "decode"$/s//stage = "projection"/' \
        "$1/ci/checks/projection-drops.toml"
}

want 0 control                      control
want 1 unmarked-discard             unmarked
want 1 undeclared-drop              undeclared
want 1 stale-ledger-row             stale_row
want 1 stale-observer-count         stale_count
want 1 stale-observer-name          stale_name
want 1 unemittable-kind             unemittable_kind
want 1 housekeeping-kind            housekeeping_kind
want 1 ambient-family-unledgered    ambient_unledgered
want 1 ambient-filter-unmarked      ambient_unmarked
want 1 stale-normalisation-anchor   stale_anchor
want 1 misfiled-stage               misfiled_stage
want 1 stage-outside-vocabulary     bogus_stage
want 1 historical-569323e           historical_569323e
want 0 historical-569323e-repaired  historical_569323e_repaired

echo
echo "projection-drops-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
