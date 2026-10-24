#!/usr/bin/env bash
# entity-spawn-probe.sh — which entities can this harness put in a recording,
# and which of them does vanilla reproduce against itself?
#
#     ./ci/entity-spawn-probe.sh              # 6 cold vanilla runs
#     ENTITY_RUNS=2 ./ci/entity-spawn-probe.sh   # cheaper, weaker
#
# `docs/PARITY-ROADMAP.md` §21.5 ranks mob AI third on its work order, on two
# numbers: **4 of vanilla's 158 entity types have ever been present in a
# recording** (`player`, `item`, `falling_block`, `pig`) and **33 of the 41
# provably-dead tick sections are per-mob sections for mobs no recording
# holds**. §21.2 explains the 4: `add_entity` is written for every entity that
# enters view, so absence over six runs is evidence rather than silence.
#
# This probe answers the question that has to be answered before any of that is
# corpus work: **can an entity be put into a measured window reproducibly, and
# does it depend on which mechanism put it there?**
#
# # The one thing already known, and it is a null
#
# `docs/cold-start/corpus-dark-systems.md` killed natural spawning: with
# `spawn_mobs true` the reference flapped **15/15 pairs over six distinct
# projections**, because `EntityType.create`'s spawn path draws the yaw from
# `serverLevel.getRandom().nextFloat()`. `080-spawn-egg-makes-a-mob` fails for
# the same reason at 25/25 (`ci/checks/gameplay-unstable.toml`) and is a floor
# member. So "a mob is unreproducible" is the standing reading, and the
# hypothesis this probe exists to test is narrower than that reading:
#
#   **the randomness belongs to the spawn MECHANISM, not to the entity type.**
#
# `javap -p -c -constants` on the pinned jar, 26.2:
#
#   EntityType.create(ServerLevel, PostSpawnProcessor, BlockPos, reason, Z, Z)
#     offsets 99-115   snapTo(x, y, z,
#                             Mth.wrapDegrees(level.getRandom().nextFloat()*360), 0)
#     offsets 118-152  if Mob: yHeadRot = yBodyRot = getYRot()
#     offset  168      Mob.finalizeSpawn(...)
#
#   CarvedPumpkinBlock.spawnGolemInWorld(Level, match, entity, pos)
#     offsets 5-35     snapTo(pos.getX()+0.5, pos.getY()+0.05, pos.getZ()+0.5,
#                             0.0F, 0.0F)
#     offsets 38-43    Level.addFreshEntity   — and no finalizeSpawn at all
#
# Both spawn a `Mob`. One draws its yaw and one writes a literal zero. If the
# hypothesis holds, a golem is reproducible where a spawn-egg pig is not, and
# the corpus can hold a mob today without a decoder change, without an entry in
# `ci/checks/gameplay-unstable.toml`, and without touching `spawn_mobs`.
#
# `Mob.finalizeSpawn` matters for the same reason: a mechanism that skips it
# skips its draws. **One of the two draws named here does not reach the wire,
# and this paragraph asserted for a day that it did.** `finalizeSpawn` draws
# `random.triangle(0.0, 0.11485)` into a permanent FOLLOW_RANGE modifier and
# `random.nextFloat() < 0.05` into `setLeftHanded`. The second rides
# `set_entity_data`'s mob-flags byte. The first was claimed to ride
# `update_attributes` and does not — that attribute is not client-syncable, and
# the N = 6 ensemble below records the spawn-egg control's `update_attributes`
# residue as `594df060bec3e117` in all six runs while its yaw takes six
# distinct values. A bytecode reading is not a wire prediction until something
# has read the wire. `docs/PARITY-ROADMAP.md` §30.3.
#
# The control's `set_entity_data` residue moves too, with **four** distinct
# values in six runs, and `setLeftHanded` at p = 0.05 gives two. So a third
# draw reaches that packet; it is on the `EntityType.create` path and absent
# from the golem path, and naming it needs the recorder to decode the metadata
# blob rather than digest it. §30.3 carries that as the next item.
#
# # Why a type-keyed decoder exemption is NOT the fix here
#
# `compare::exempt_item_spawn_yaw` drops `add_entity.y_rot` for
# `minecraft:item`, and the obvious next move is to widen it to mobs. It is the
# wrong move and the asymmetry is the reason: an `ItemEntity` draws its yaw in
# its **constructor**, so the field is random for every mechanism that can ever
# spawn one and the type is a sound key. A pig's yaw is random only on the
# `EntityType.create` path; the same pig from `spawnGolemInWorld` — or from any
# future placed mechanism — has a determinate one, and the wire carries no
# record of which path ran. A type-keyed exemption would therefore stop
# comparing a value that IS comparable, which is the "exemption that
# manufactures a check that cannot fail" this repository keeps catching.
#
# 908 is in this probe as the positive control for exactly that: the shipped
# spawn-egg check, copied byte for byte, expected to flap, run beside golems
# expected not to. Without it a clean golem floor is a statement about how
# quiet the box was.
#
# # WHAT THE FIRST RUN FOUND, AND WHY 777/788/789 DO NOT OBSERVE `add_entity`
#
# The hypothesis held and was not the whole story. Two cold runs, 2026-08-12:
#
#     probe / check          y_rot          y_head_rot
#     item frame             64    64        0    0
#     end crystal             0     0        0    0
#     minecart                0     0        0    0
#     boat                 -128  -128        0    0
#     armor stand          -128  -128        3    2      <- flaps
#     snow golem              0     0        3    2      <- flaps
#     copper golem            0     0        3    3
#     iron golem              0     0        1    2      <- flaps
#     spawn-egg control       4    75        4   75      <- flaps, both fields
#
# `y_rot` behaved exactly as the bytecode says: a die roll on the spawn-egg
# path, a determinate value on every placed one. `y_head_rot` is a **second,
# separate** draw nobody had named:
#
#   Entity.getYHeadRot()        0: fconst_0  1: freturn
#   LivingEntity.getYHeadRot()  0: aload_0   1: getfield yHeadRot  4: freturn
#   LivingEntity.<init>       176: getfield random:RandomSource
#                             ...  nextFloat()
#                             184: ldc 6.2831855f    // radians' worth of degrees
#                             187: invokevirtual setYRot:(F)V
#                             195: putfield yHeadRot:F
#   Entity.snapTo(DDDFF)      setPosRaw, setYRot, setXRot, setOldPosAndRot,
#                             reapplyPosition  -- and never yHeadRot
#
# So a non-`LivingEntity` carries a compile-time zero, and a `LivingEntity`
# carries an unseeded draw in [0, 6.2832) **degrees** — wire bytes 0..4, which
# is what 1/2/3 above are — unless the spawn path assigns the field. Exactly
# one path here does: `EntityType.create`'s Mob branch, offsets 133-140,
# `mob.yHeadRot = mob.getYRot()`, which is why the spawn egg's two columns are
# equal and why they are equally unusable.
#
# 777/788/789 therefore drop `add_entity` and buy the mechanism back through
# `set_entity_data`, `update_attributes`, `level_event` and the block updates,
# all of which the same two runs reproduced byte for byte. See
# `ci/checks/gameplay/777-...toml` for the argument and
# `docs/PARITY-ROADMAP.md` §30 for what removing the cause would cost.
#
# # It boots its own vanilla and touches nothing
#
# Its own port, its own world directory, its own throwaway corpus. It writes
# nothing into `ci/checks/gameplay/`, does not take the box-wide corpus lock,
# and does not touch `gameplay-baseline.toml` or the standing floor. It
# **copies** the shipped checks in rather than paraphrasing them, so the floor
# it prints is a floor for the files that ship. Same shape as
# `ci/dark-systems-probe.sh` and `ci/finish-dig-probe.sh`.
#
# # The substrate is the corpus's substrate, deliberately
#
# Same superflat, same seed, same four game rules, same server properties as
# `ci/t2-gameplay.sh`. A floor taken on a different world is a floor for a
# different check. Note `difficulty=peaceful`: it despawns hostiles, and every
# mob below is neutral or friendly, which is not a coincidence — a hostile mob
# is unreachable from this substrate whatever spawns it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${ENTITY_PROBE_PORT:-25916}"
RUNS="${ENTITY_RUNS:-6}"
WORK="$ROOT/oracle-work/gameplay/entity-probe"
VAN_DIR="$WORK/vanilla"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"

cleanup() {
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "$WORK"
mkdir -p "$SCEN"

# --------------------------------------------------------------------------
# THE CORPUS IS THE SHIPPED FILES, COPIED. Not paraphrased: a floor taken over
# a paraphrase of a check is a floor for the paraphrase, and every rig detail
# below — the `~ = AIR` positions in the iron golem pattern, the pitch of 45
# on the boat, which steps are `setup` and which are measured — is exactly
# what decides whether the recording reproduces.
#
# 908 is the POSITIVE CONTROL: `080-spawn-egg-makes-a-mob`, also copied, with
# only its `name` changed so it cannot collide with the shipped check in a
# report. It is a floor member at 25/25 over 26 cold vanilla servers and it
# must keep flapping here. A clean golem floor beside a clean control floor
# would be a statement about how quiet the box was, not about the mechanism.
# --------------------------------------------------------------------------
for f in 773-an-item-frame-hangs-on-a-blocks-side \
         774-an-end-crystal-sits-on-obsidian \
         775-a-minecart-sits-on-a-rail \
         776-a-boat-is-placed-by-right-clicking-the-world \
         777-an-armor-stand-is-placed-as-a-living-entity \
         788-a-carved-pumpkin-on-copper-makes-a-copper-golem \
         789-a-carved-pumpkin-on-iron-makes-an-iron-golem; do
    cp "$ROOT/ci/checks/gameplay/$f.toml" "$SCEN/$f.toml"
done

# ENTITY_PROBE_EXTRA names check files to copy in ON TOP of the shipped list —
# the arm of an A/B that adds a scenario the corpus does not ship. It exists
# for the snow golem, which this probe measured out of the corpus on
# 2026-08-12 (`docs/PARITY-ROADMAP.md` §30): re-adding it is the positive
# control for the claim that IT was the cause of the copper golem's flap, and a
# claim of that form is only worth its measurement if the removal can be undone
# in one variable.
for f in ${ENTITY_PROBE_EXTRA:-}; do
    cp "$f" "$SCEN/$(basename "$f")"
done

cp "$ROOT/ci/checks/gameplay/080-spawn-egg-makes-a-mob.toml" \
   "$SCEN/908-probe-spawn-egg-control.toml"
sed -i 's/^name = .*/name = "probe-spawn-egg-control"/' "$SCEN/908-probe-spawn-egg-control.toml"

cargo build -q -p oracle-runner --release
BIN="$ROOT/target/release/oracle-runner"

RECS=()
for run in $(seq 1 "$RUNS"); do
    echo
    echo "== run $run/$RUNS — cold vanilla on :$PORT"
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
    rm -rf "$VAN_DIR"
    VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$PORT" "minecraft:flat" "$SEED"
    for r in advance_time spawn_mobs random_tick_speed advance_weather; do
        case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
        "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
    done
    sleep 2
    out="$WORK/r$run.json"
    "$BIN" record "127.0.0.1:$PORT" "$SCEN" --out "$out"
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR"
    RECS+=("$out")
done

echo
python3 - "${RECS[@]}" <<'PY'
import json, sys, collections
runs = [json.load(open(p)) for p in sys.argv[1:]]
names = [s["name"] for s in runs[0]["scenarios"]]
for n in names:
    print(f"\n=== {n} ===")
    for i, r in enumerate(runs, 1):
        s = next(x for x in r["scenarios"] if x["name"] == n)
        if s.get("error"):
            print(f"  r{i}: ERROR {s['error']}")
            continue
        c = collections.Counter(o["kind"] for o in s["obs"])
        summary = ", ".join(f"{k}x{v}" for k, v in sorted(c.items())) or "(nothing)"
        print(f"  r{i}: {len(s['obs']):3d} obs  {summary}")
        for o in s["obs"]:
            print(f"        {o['who']:8s} {o['kind']:22s} {json.dumps(o['fields'])[:170]}")
PY

echo
echo "== floor over $RUNS runs, this probe's corpus only"
"$ROOT/ci/corpus-floor.sh" --checks "$SCEN" --no-adjudicate \
    --id "entity-probe-n$RUNS" --out "$WORK/floor" "${RECS[@]}"
