#!/usr/bin/env bash
# mob-tick-census-falsify.sh — inject faults, assert ci/mob-tick-census.py catches each.
#
# The census's headline is a ZERO — no mob kind in the crate — and a zero from
# a broken reader looks exactly like a zero from a correct one. `710` is the
# standing example of what that costs: a check green for two months over a
# mechanic that never ran. So the census carries a positive control, and this
# file is the proof that the control is not itself decorative.
#
# Every fault is injected into a SCRATCH tree, never into the worktree. The
# census locates its root from its own path, so a directory holding `ci/` and
# `crates/rustmc-server/src/entities.rs` is a complete input to it.
#
# Ten assertions:
#   0  a cold re-derivation agrees with the cache, figure for figure
#   1  clean scratch tree: the census runs and reports the pinned mob kinds
#   2  clean scratch tree: --check matches the pin
#   3  the positive control BREAKS when PLAYER's registry name is corrupted,
#      the census exits 3, and prints no finding on stdout
#   4  same, for ITEM — one control passing is not the control passing
#   5  a figure moved in the pin makes --check refuse, naming the figure
#   6  a mob EntityKind added to entities.rs moves our_mob_kinds up by one
#   7  corrupting either constant in crates/rustmc-entity/src/tick.rs moves
#      transcription_disagreements off 0
#   8  EntityType carries exactly one RandomSource.nextFloat site, create@103
#   9  a headline figure edited out of a document is caught, and the document
#      is named — ci/check-figures.py scans percentages and these are counts
#
# 3 and 4 are the ones that matter. 6 is the other direction and is the reason
# the intersection is computed against the jar's own registrations rather than
# against a hand-written list of mob names: a list would have to be edited to
# make this assertion pass, and an assertion you edit to pass is not one.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$HERE")"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
FAIL=0

note() { printf '  %-5s %s\n' "$1" "$2"; }
ok()   { note "ok" "$1"; }
bad()  { note "FAIL" "$1"; FAIL=1; }

mkdir -p "$SCRATCH/ci/checks" "$SCRATCH/crates/rustmc-server/src" \
         "$SCRATCH/crates/rustmc-entity/src" "$SCRATCH/docs/cold-start"
cp "$HERE/mob-tick-census.py" "$HERE/jarcallgraph.py" "$SCRATCH/ci/"
cp "$HERE/checks/mob-surface.json" "$SCRATCH/ci/checks/"
cp "$ROOT/crates/rustmc-server/src/entities.rs" "$SCRATCH/crates/rustmc-server/src/"
cp "$ROOT/crates/rustmc-entity/src/tick.rs" "$SCRATCH/crates/rustmc-entity/src/"
cp "$ROOT/docs/PARITY-ROADMAP.md" "$ROOT/docs/WORKLIST.md" "$ROOT/docs/SERVER.md" "$SCRATCH/docs/"
cp "$ROOT/docs/cold-start/mob-tick-surface.md" "$SCRATCH/docs/cold-start/"
# The jar is pinned and shared; the census falls back to ~/rustmc for it, so
# no link is needed here. If that fallback ever goes, assertion 1 fails loudly
# rather than every later assertion passing vacuously.
CENSUS="$SCRATCH/ci/mob-tick-census.py"
ENT="$SCRATCH/crates/rustmc-server/src/entities.rs"
PIN="$SCRATCH/ci/checks/mob-surface.json"

echo "== 0: the cache is not answering for the derivation"
# Every later assertion runs against a warm cache, which is what makes this
# file cheap enough to gate. That is also how it could become decorative: a
# JarGraph that had stopped parsing would be invisible behind a cache written
# when it still worked. So derive once, cold, and require the two to agree.
COLD="$(python3 "$CENSUS" --no-cache --json 2>/dev/null)"
WARM="$(python3 "$CENSUS" --json 2>/dev/null)"
if [ -n "$COLD" ] && [ "$COLD" = "$WARM" ]; then
    ok "cold derivation and cached read agree, figure for figure"
elif [ -z "$COLD" ]; then
    bad "the cold derivation produced nothing — the census cannot read the jar"
else
    bad "cold and cached disagree: the cache is stale or the key is wrong"
    diff <(printf '%s\n' "$COLD") <(printf '%s\n' "$WARM") | head -6
fi

echo "== 1: the clean scratch tree measures"
# The pinned mob count, not a literal zero: `mob-in-world` (2026-09-05) landed
# the pig and the bat, and a literal 0 here turned into a case that could only
# pass on a tree with no mobs. Read from the pin so 1 and 6 follow a re-pin.
MOBS="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["figures"]["our_mob_kinds"])' "$PIN")"
OUT="$(python3 "$CENSUS" 2>&1)"; RC=$?
if [ $RC -eq 0 ] && printf '%s' "$OUT" | grep -cE "of which are mobs +$MOBS( |$)" >/dev/null; then
    ok "census runs and reports the pinned $MOBS mob kind(s)"
else
    bad "the scratch tree does not measure (rc=$RC) — every later assertion would pass vacuously"
    printf '%s\n' "$OUT" | tail -5
fi

echo "== 2: --check matches the pin"
if python3 "$CENSUS" --check >/dev/null 2>&1; then
    ok "--check green on an unmodified pin"
else
    bad "--check red before any fault was injected"
fi

echo "== 3: corrupting PLAYER's registry name kills the positive control"
cp "$ENT" "$SCRATCH/entities.rs.orig"
sed -i 's|name: "minecraft:player"|name: "minecraft:player_XX"|' "$ENT"
OUT="$(python3 "$CENSUS" 2>/dev/null)"; RC=$?
ERR="$(python3 "$CENSUS" 2>&1 >/dev/null)"
if [ $RC -eq 3 ]; then ok "exit 3 (reader not live)"; else bad "expected exit 3, got $RC"; fi
if [ -z "$OUT" ]; then ok "printed no finding on stdout"; else bad "printed a finding anyway"; fi
if printf '%s' "$ERR" | grep -c "REFUSED" >/dev/null; then ok "said REFUSED and why"; else bad "refusal was silent"; fi
cp "$SCRATCH/entities.rs.orig" "$ENT"

echo "== 4: corrupting ITEM's registry name kills it too"
sed -i 's|name: "minecraft:item"|name: "minecraft:item_XX"|' "$ENT"
python3 "$CENSUS" >/dev/null 2>&1; RC=$?
if [ $RC -eq 3 ]; then ok "exit 3 on the second control"; else bad "expected exit 3, got $RC"; fi
cp "$SCRATCH/entities.rs.orig" "$ENT"

echo "== 5: a moved figure makes --check refuse"
python3 - "$PIN" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p))
d["figures"]["hierarchy_declarations"] = d["figures"]["hierarchy_declarations"] + 1
json.dump(d, open(p,"w"), indent=1, sort_keys=True)
PY
ERR="$(python3 "$CENSUS" --check 2>&1 >/dev/null)"; RC=$?
if [ $RC -ne 0 ]; then ok "--check refused"; else bad "--check stayed green over a moved figure"; fi
if printf '%s' "$ERR" | grep -c "hierarchy_declarations" >/dev/null; then
    ok "named the figure that moved"
else
    bad "refused without naming the figure"
fi
# A jar-derived figure and a figure that is supposed to move as work lands are
# both "the pin disagrees", and telling the next agent which is which is the
# difference between a re-pin and a bug hunt.
if printf '%s' "$ERR" | grep -c "jar-derived" >/dev/null; then
    ok "said a jar-derived figure is not a number to edit"
else
    bad "refused without saying whether re-pinning is the right response"
fi
cp "$HERE/checks/mob-surface.json" "$PIN"
python3 - "$PIN" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p))
d["figures"]["our_mob_kinds"] = 1
json.dump(d, open(p,"w"), indent=1, sort_keys=True)
PY
ERR="$(python3 "$CENSUS" --check 2>&1 >/dev/null)"
if printf '%s' "$ERR" | grep -ci "A MOB LANDED" >/dev/null; then
    ok "a figure that is supposed to move says so, and names the docs to fix"
else
    bad "our_mob_kinds moving reads the same as a broken derivation"
fi
cp "$HERE/checks/mob-surface.json" "$PIN"

echo "== 6: adding a mob kind moves our_mob_kinds"
# An armadillo, because a kind this server already declares would not add one
# to the intersection: the pig (`mob-in-world`) and, since `train-f`, the cow,
# mooshroom, sheep and chicken (`passive-mobs`) are all taken. The armadillo
# is a jar mob nobody has ported; pick another the day one does.
grep -q '"minecraft:armadillo"' "$ENT" && bad "the armadillo is declared now: pick an unported mob for case 6"
cat >> "$ENT" <<'RS'

pub const FALSIFY_MOB: EntityKind = EntityKind {
    name: "minecraft:armadillo",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};
RS
N="$(python3 "$CENSUS" --json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["our_mob_kinds"])' 2>/dev/null)"
if [ "$N" = "$((MOBS + 1))" ]; then
    ok "our_mob_kinds $MOBS -> $N, matched against the jar's own registrations"
else
    bad "our_mob_kinds did not move (got '${N:-<none>}')"
fi
cp "$SCRATCH/entities.rs.orig" "$ENT"

echo "== 7: corrupting the crate's transcription is seen"
# `crates/rustmc-entity/src/tick.rs` writes down three constants it read out of
# MobCategory's bytecode, and until now nothing re-read them. All three were
# correct on 2026-08-19 — which is exactly when a transcription is cheapest to
# put under a check, because a correct one that nothing checks stays correct
# only until somebody edits it.
TICK="$SCRATCH/crates/rustmc-entity/src/tick.rs"
cp "$TICK" "$SCRATCH/tick.rs.orig"
D0="$(python3 "$CENSUS" --json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["transcription_disagreements"])' 2>/dev/null)"
if [ "$D0" = "0" ]; then
    ok "clean tree: crate and jar agree"
else
    bad "the crate and the jar already disagree ($D0) — fix that before reading this file"
fi
sed -i 's/Self::WaterAmbient => 64,/Self::WaterAmbient => 65,/' "$TICK"
D1="$(python3 "$CENSUS" --json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["transcription_disagreements"])' 2>/dev/null)"
if [ "${D1:-0}" -ge 1 ] 2>/dev/null; then
    ok "a wrong despawn distance moved the count 0 -> $D1"
else
    bad "a wrong despawn distance was not seen; the cross-check is decorative"
fi
cp "$SCRATCH/tick.rs.orig" "$TICK"
python3 - "$TICK" <<'PY'
import re,sys
p=sys.argv[1]; s=open(p).read()
s=s.replace("pub const fn no_despawn_distance(self) -> i32 {\n        32",
            "pub const fn no_despawn_distance(self) -> i32 {\n        33")
open(p,"w").write(s)
PY
D2="$(python3 "$CENSUS" --json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["transcription_disagreements"])' 2>/dev/null)"
if [ "${D2:-0}" -ge 1 ] 2>/dev/null; then
    ok "a wrong no-despawn constant moved the count too"
else
    bad "the no-despawn constant is not being compared"
fi
cp "$SCRATCH/tick.rs.orig" "$TICK"

echo "== 8: the yaw-draw site is read, not assumed"
Y="$(python3 "$CENSUS" --json 2>/dev/null | python3 -c 'import json,sys; print(",".join(json.load(sys.stdin)["entitytype_nextfloat_sites"]))' 2>/dev/null)"
if [ "$Y" = "create@103" ]; then
    ok "EntityType's only nextFloat site is create@103"
else
    bad "expected create@103, got '${Y:-<none>}' — three findings rest on there being exactly one"
fi

echo "== 9: a figure that drifts out of the prose is caught"
# `ci/check-figures.py` scans percentages and every figure this lane publishes
# is a count, so nothing else watches these. Four had already drifted by hand
# on the day this was written.
DOC="$SCRATCH/docs/cold-start/mob-tick-surface.md"
cp "$DOC" "$SCRATCH/doc.orig"
sed -i 's/3,870/3,871/g' "$DOC"
ERR="$(python3 "$CENSUS" --check 2>&1 >/dev/null)"; RC=$?
if [ $RC -ne 0 ]; then ok "refused"; else bad "a figure edited out of a doc went unrefused"; fi
if printf '%s' "$ERR" | grep -c "mob-tick-surface.md" >/dev/null; then
    ok "named the document"
else
    bad "refused without naming the document"
fi
cp "$SCRATCH/doc.orig" "$DOC"
if python3 "$CENSUS" --check >/dev/null 2>&1; then
    ok "green again once the prose is restored"
else
    bad "still red after restoring the doc; the check is not reading it"
fi

echo "== 9b: a figure buried inside a LONGER number does not count as quoted"
# The rule this arm exists for was green on exactly that for as long as it had
# existed: `mob_subtypes = 114` is declared as quoted in `docs/SERVER.md`, was
# never written there, and the rule passed because that file carried
# `11491 connective`. Case 9 above cannot see it — it edits the figure out and
# watches the refusal, which a substring test also does. What separates the two
# is replacing the figure with a longer number that CONTAINS it.
sed -i 's/3,870/13,8705/g' "$DOC"
ERR="$(python3 "$CENSUS" --check 2>&1 >/dev/null)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$ERR" | grep -c "mob-tick-surface.md" >/dev/null; then
    ok "3,870 inside 13,8705 is not the figure"
else
    bad "a figure found only as a substring of a longer number went unrefused (rc=$RC)"
fi
cp "$SCRATCH/doc.orig" "$DOC"
if python3 "$CENSUS" --check >/dev/null 2>&1; then
    ok "green again once the prose is restored"
else
    bad "still red after restoring the doc"
fi

echo
if [ $FAIL -eq 0 ]; then
    echo "mob-tick-census-falsify: all assertions held"
else
    echo "mob-tick-census-falsify: RED — an injected fault went unrefused" >&2
fi
exit $FAIL
