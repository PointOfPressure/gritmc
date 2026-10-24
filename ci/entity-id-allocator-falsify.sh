#!/usr/bin/env bash
# entity-id-allocator-falsify.sh — show ci/entity-id-allocator.py failing
# before believing it when it passes.
#
# The headline this check produces is "all four required properties agree",
# which is a statement of SAMENESS. A reader that finds nothing on either side
# produces the same words, so every arm below either kills a reader and demands
# a refusal, or moves a real property and demands a red.
#
# The NEGATIVE CONTROL is the last arm and it is not decoration: a detector
# that refuses on any edit at all would pass every positive arm for free while
# measuring the file's bytes rather than the allocator.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
OURS=crates/rustmc-server/src/entities.rs
TMP="$(mktemp -d)"
cp "$OURS" "$TMP/entities.rs.orig"
restore() { cp "$TMP/entities.rs.orig" "$OURS"; }
trap 'restore; [ -f "$TMP/mob-blind.toml.orig" ] && cp "$TMP/mob-blind.toml.orig" ci/checks/mob-blind.toml; rm -rf "$TMP"' EXIT

fail=0
arm() { printf '  %-58s %s\n' "$1" "$2"; }

# --- 1. our reader must be alive: rename the allocator out from under it ----
sed -i 's/pub fn next_id(&self) -> i32 {/pub fn next_id_renamed(\&self) -> i32 {/' "$OURS"
out="$(python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "our allocator renamed away" "REFUSED (3), stdout empty  OK"
else arm "our allocator renamed away" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
restore

# --- 2. a REQUIRED property really moved: drop the +1 -----------------------
# Without the `+ 1` our first id is 0, which is the wire's no-entity sentinel.
# This is the defect the check exists to catch and it must go red, not refuse.
sed -i 's/self\.next_id\.fetch_add(1, Ordering::Relaxed) + 1/self.next_id.fetch_add(1, Ordering::Relaxed)/' "$OURS"
out="$(python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 1 ]; then arm "the +1 removed, so the first id becomes 0" "RED (1)  OK"
else arm "the +1 removed, so the first id becomes 0" "rc=$rc  FAIL"; fail=1; fi
restore

# --- 3. the counter narrowed: i32 -> i64 changes the wrap point -------------
sed -i 's/next_id: AtomicI32,/next_id: AtomicI64,/' "$OURS"
out="$(python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] || [ "$rc" -eq 1 ]; then arm "counter retyped to AtomicI64" "caught (rc=$rc)  OK"
else arm "counter retyped to AtomicI64" "rc=$rc  FAIL"; fail=1; fi
restore

# --- 4. the jar reader must be alive: point it at a jar that is not one -----
BAD="$TMP/notajar.jar"; printf 'this is not a zip' > "$BAD"
out="$(RUSTMC_JAR="$BAD" python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "javap pointed at a file that is not a jar" "REFUSED (3), stdout empty  OK"
else arm "javap pointed at a file that is not a jar" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# --- 5. THE GOAL-PHASE ROW MUST MOVE WHEN ITS SUBJECT MOVES ----------------
# The check does not grade `Mob.serverAiStep`'s goal phase, and says so in a
# row. Until `mob-in-world` (2026-09-24) that row was "no mob can exist" and a
# mob kind made the check refuse outright; with the pig and the bat landed the
# row is derived — which kinds are mobs, which mob checks observe behaviour —
# and pinned. So a further mob kind must redden `--check` against the pin,
# and so must the first mob check that observes behaviour (5b), or the row
# could go stale again without anybody re-reading it.
python3 - "$OURS" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = "pub const ITEM: EntityKind = EntityKind {"
assert a in s, "falsifier arm 5 lost its anchor"
inject = ("pub const FALSIFY_COW: EntityKind = EntityKind {\n"
          "    name: \"minecraft:cow\",\n};\n\n")
open(p, "w").write(s.replace(a, inject + a, 1))
EOP
out="$(python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 1 ]; then arm "a further mob EntityKind lands, so the row moves" "RED (1)  OK"
else arm "a further mob EntityKind lands, so the row moves" "rc=$rc  FAIL"; fail=1; fi
restore

BLIND=ci/checks/mob-blind.toml
cp "$BLIND" "$TMP/mob-blind.toml.orig"
python3 - "$BLIND" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
assert "observes_behaviour = false" in s, "falsifier arm 5b lost its anchor"
open(p, "w").write(s.replace("observes_behaviour = false", "observes_behaviour = true", 1))
EOP
out="$(python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 1 ]; then arm "a mob check starts observing behaviour, so the row moves" "RED (1)  OK"
else arm "a mob check starts observing behaviour, so the row moves" "rc=$rc  FAIL"; fail=1; fi
cp "$TMP/mob-blind.toml.orig" "$BLIND"

# --- 5c. AND A KIND THAT IS NOT A MOB MUST NOT BE COUNTED AS ONE ------------
# The arm above was here from the start and passed for the wrong reason: the
# check tested `k not in ("PLAYER", "ITEM")`, an allowlist, so ANY third
# constant refused. `stack-gameplay`'s `FALLING_BLOCK` is `MobCategory.MISC`
# and reddened T0 on a tree whose standing note is still exactly true. One
# arm can only ever show a refusal FIRING; it takes this one to show the
# refusal is about mobs. `minecraft:falling_block` is used because it is the
# real constant that exposed it, and the registry id is the whole payload —
# arm 5 injects `minecraft:pig` for the same reason.
python3 - "$OURS" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = "pub const ITEM: EntityKind = EntityKind {"
assert a in s, "falsifier arm 5b lost its anchor"
inject = ('pub const FALSIFY_NOT_A_MOB: EntityKind = EntityKind {\n'
          '    name: "minecraft:falling_block",\n};\n\n')
open(p, "w").write(s.replace(a, inject + a, 1))
EOP
inj=$?
# **AN ARM THAT ASSERTS AN ABSENCE MUST PROVE ITS INJECTION LANDED.** Arm 5
# above is self-protecting: if its edit does not apply, the check does not
# refuse and the arm fails. This one asserts the OPPOSITE — that nothing
# refuses — so a failed injection reads as a pass. The first draft of this arm
# had a broken heredoc, the python exited non-zero, the file was never touched
# and the arm printed OK: a false green inside the arm added to catch a false
# green. So the exit status is checked, and so is the constant.
if [ "$inj" -ne 0 ] || ! grep -q 'FALSIFY_NOT_A_MOB' "$OURS"; then
    arm "a NON-mob EntityKind lands, so it is not counted as a mob" \
        "the injection did not land (python rc=$inj)  FAIL"; fail=1
else
# Since `mob-in-world` there is no refusal to stay silent: the goal-phase row
# is derived, and `mob_kinds` is what a MISC kind must not enter. rc 1 from
# `--check` is expected (the pin's `entity_kinds` moved); the assertion is on
# the derived row.
kinds="$(python3 ci/entity-id-allocator.py --json 2>/dev/null | python3 -c 'import json,sys; print(",".join(json.load(sys.stdin)["ungraded"].get("mob_kinds", [])))' 2>/dev/null)"; rc=$?
if [ "$rc" -eq 0 ] && ! printf '%s' "$kinds" | grep -c 'FALSIFY_NOT_A_MOB' >/dev/null; then arm "a NON-mob EntityKind lands, so it is not counted as a mob" "mob_kinds=[$kinds]  OK"
else arm "a NON-mob EntityKind lands, so it is not counted as a mob" "rc=$rc mob_kinds=[$kinds]  FAIL"; fail=1; fi
fi
restore

# --- 6. the goal-phase reader dies -----------------------------------------
# `(tickCount + getId()) % 2` is the whole reason this file exists. If the
# opcode sequence stops matching, grading the allocator is grading something
# nothing reads, and the check must say so rather than stay green.
BADJAR="$TMP/empty.jar"
python3 -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1],'w').close()" "$BADJAR"
out="$(RUSTMC_JAR="$BADJAR" python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "a jar with no Mob in it" "REFUSED (3), stdout empty  OK"
else arm "a jar with no Mob in it" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# --- 5. NEGATIVE CONTROL: an unrelated edit must NOT be refused -------------
# A comment appended to the allocator's own file. If this trips the check, the
# check is hashing the file and arms 1-3 proved nothing.
printf '\n// entity-id-allocator-falsify.sh negative control; removed on exit.\n' >> "$OURS"
out="$(python3 ci/entity-id-allocator.py --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: unrelated comment appended" "green (0)  OK"
else arm "NEGATIVE CONTROL: unrelated comment appended" "rc=$rc  FAIL"; fail=1; fi
restore

# --- 6. NEGATIVE CONTROL: the check is green on the untouched tree ----------
python3 ci/entity-id-allocator.py --check > /dev/null 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: untouched tree" "green (0)  OK"
else arm "NEGATIVE CONTROL: untouched tree" "rc=$rc  FAIL"; fail=1; fi

if [ "$fail" -ne 0 ]; then echo "entity-id-allocator-falsify: FAILED"; exit 1; fi
echo "entity-id-allocator-falsify: 8 injected faults refused or reddened, 2 negative controls green"
