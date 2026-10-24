#!/usr/bin/env bash
# sound-particle-golden.sh — the sound-event, particle-type and level-event
# tables, from a bootstrapped vanilla registry.
#
#     ./ci/sound-particle-golden.sh [out-dir]
#
# Writes three files into `data/vanilla/`:
#
#     sound_events.json    1,968 events, with the fixedRange that would decide
#                          how far ServerLevel broadcasts each — empty for all
#                          1,968 in 26.2, which is measured and asserted
#     particle_types.json  125 types, each with whether it carries options
#     level_events.json    every int constant on LevelEvent
#     note_block.json      the NoteBlockInstrument enum and NOTE_VOLUME
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`. See `tools/goldengen/vanilla/SoundParticleGolden.java`
# for what each column is and why the range is a table rather than a constant.
#
# # The assertions below do not come from this dumper
#
# A generator that checks its own output against its own output is the defect
# class this project keeps paying for. The two counts asserted here —
# 1,968 sound events and 125 particle types — are read out of **Mojang's own
# `generated/reports/registries.json`**, which is produced by a different code
# path (the data generator, not a bootstrapped server) and shipped in the same
# version directory. If the dumper walked a half-built registry, or bootstrap
# failed silently and left it empty, the two disagree and this script exits 1.
#
# The report is also what `ci/dump-protocol-ids.sh` numbers the wire against,
# so agreeing with it is precisely the property `crate::sounds::send` needs:
# the id it writes into the packet and the range it looks up have to be the
# same registry.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTDIR="${1:-$ROOT/data/vanilla}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="${RUSTMC_VANILLA_DIR:-$ROOT/oracle-work/vanilla-$VER}"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

REPORT="$VDIR/generated/reports/registries.json"
[ -s "$REPORT" ] || { echo "no registry report at $REPORT — it is the only independent check this dumper has, and without it the counts below would be self-referential" >&2; exit 1; }

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

mkdir -p "$OUTDIR"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/SoundParticleGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    mkdir -p "$1"
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.SoundParticleGolden "$1" 2>/dev/null
    test -s "$1/sound_events.json"
}

# Twice, and byte for byte. All three tables are walks over a registry in id
# order, which is stable by construction — so this run costs ten seconds to
# check a property that ought to hold, and would catch the one thing that
# would make every number below meaningless: a bootstrap that is not
# deterministic. `ci/dump-registries.sh`'s sibling check found exactly that
# class of drift once.
run "$WORK/a"
run "$WORK/b"
for f in sound_events.json particle_types.json level_events.json note_block.json; do
    cmp -s "$WORK/a/$f" "$WORK/b/$f" || {
        echo "two runs of SoundParticleGolden disagree on $f — the registry walk is not stable" >&2
        exit 1
    }
done

python3 - "$WORK/a" "$OUTDIR" "$REPORT" <<'PYEOF'
import json, os, shutil, struct, sys

src, outdir, report_path = sys.argv[1], sys.argv[2], sys.argv[3]
report = json.load(open(report_path))
fail = []

# ---- sound events -------------------------------------------------------
se = json.load(open(os.path.join(src, "sound_events.json")))
want_sounds = len(report["minecraft:sound_event"]["entries"])
if se["events"] != want_sounds:
    fail.append(f"dumper says {se['events']} sound events, the registry report says {want_sounds}")
if len(se["table"]) != se["events"]:
    fail.append(f"{len(se['table'])} sound rows for {se['events']} events")

# Ids must be dense and ascending from zero: `Registry.byId` is what the
# packet's varint indexes, and a table that is merely *sorted* would put every
# lookup one place out from the first gap onwards.
for i, row in enumerate(se["table"]):
    if row["id"] != i:
        fail.append(f"sound row {i} carries id {row['id']} — the table is not id-ordered")
        break

# The report is a name -> {protocol_id} map; every name and number must agree
# with ours. This is the check that says our table and `protocol_ids.json`
# number the same registry, which is the whole premise of looking a range up
# by the name we just wrote an id for.
report_sounds = {k: v["protocol_id"] for k, v in report["minecraft:sound_event"]["entries"].items()}
ours = {row["name"]: row["id"] for row in se["table"]}
if ours != report_sounds:
    only_ours = set(ours) - set(report_sounds)
    only_theirs = set(report_sounds) - set(ours)
    moved = [n for n in set(ours) & set(report_sounds) if ours[n] != report_sounds[n]]
    fail.append(f"sound registry disagrees with the report: {len(only_ours)} only here, "
                f"{len(only_theirs)} only there, {len(moved)} at a different id")

# ZERO IS THE MEASURED ANSWER FOR 26.2 AND IT IS ASSERTED, NOT ASSUMED.
#
# This check was written the other way round — "zero means a broken read" —
# and the first run of the generator failed on it. It is zero: no built-in
# sound event carries a fixed range in this version, three ways (see
# SoundParticleGolden's header). So the assertion is inverted, and the reason
# it is an assertion at all rather than a deletion is that a version which
# adds one changes how far that sound carries, silently, in a table nobody
# re-reads. Make this red and read the diff.
#
# `fixedRangeReadWorks` is what keeps the zero from being an instrument
# failure: the dumper builds a fixed-range event by hand and checks the field
# is legible through it. A zero without that control is unfalsifiable.
if not se.get("fixedRangeReadWorks"):
    fail.append("the dumper could not read `fixedRange()` off an event built to have one — "
                "the zero below would be an instrument failure reported as a fact about vanilla")
if se["fixedRangeEvents"] != 0:
    fail.append(f"{se['fixedRangeEvents']} sound events now carry a fixed range where 26.2 had "
                "none — that changes how far those sounds broadcast; read them and update "
                "this check and crates/rustmc-server/src/sound_events.rs's header")

# Every emitted range is a positive finite float, and the readable form and the
# raw bits agree. A NaN or a zero here would broadcast a sound to nobody.
for row in se["table"]:
    if row["fixedRangeBits"] is None:
        if row["fixedRange"] is not None:
            fail.append(f"{row['name']}: range without bits")
        continue
    bits = row["fixedRangeBits"] & 0xFFFFFFFF
    from_bits = struct.unpack("<f", struct.pack("<I", bits))[0]
    from_text = struct.unpack("<f", struct.pack("<f", row["fixedRange"]))[0]
    if from_bits != from_text:
        fail.append(f"{row['name']}: {row['fixedRange']} does not round-trip to bits {bits}")
    if not (from_bits > 0.0) or from_bits != from_bits or from_bits > 1.0e6:
        fail.append(f"{row['name']}: fixed range {from_bits} is not a usable radius")

# ---- particle types -----------------------------------------------------
pt = json.load(open(os.path.join(src, "particle_types.json")))
want_particles = len(report["minecraft:particle_type"]["entries"])
if pt["types"] != want_particles:
    fail.append(f"dumper says {pt['types']} particle types, the report says {want_particles}")
if len(pt["table"]) != pt["types"]:
    fail.append(f"{len(pt['table'])} particle rows for {pt['types']} types")
report_particles = {k: v["protocol_id"] for k, v in report["minecraft:particle_type"]["entries"].items()}
if {row["name"]: row["id"] for row in pt["table"]} != report_particles:
    fail.append("particle registry disagrees with the report")
# Both halves have to be non-empty. All-simple would mean `instanceof` never
# matched and every options-carrying particle would be sent as a bare varint;
# none-simple would mean it always matched.
if pt["simpleTypes"] == 0 or pt["simpleTypes"] == pt["types"]:
    fail.append(f"{pt['simpleTypes']} of {pt['types']} particle types are simple — "
                "the instanceof test has collapsed to one answer")

# ---- level events -------------------------------------------------------
le = json.load(open(os.path.join(src, "level_events.json")))
if len(le["table"]) != le["levelEvents"]:
    fail.append(f"{len(le['table'])} level-event rows for {le['levelEvents']}")
by_name = {row["name"]: row["id"] for row in le["table"]}
# Three ids this server already sends from hand-written constants, so the
# table has to agree with the code that predates it or one of the two is
# wrong. `crate::shapes::LEVEL_EVENT_DESTROY`,
# `crate::power::LEVEL_EVENT_REDSTONE_TORCH_BURNOUT`, and the composter's,
# which is the one nothing in this repo asserted before now.
for name, want in (("PARTICLES_DESTROY_BLOCK", 2001),
                   ("REDSTONE_TORCH_BURNOUT", 1502),
                   ("COMPOSTER_FILL", 1500)):
    if by_name.get(name) != want:
        fail.append(f"LevelEvent.{name} is {by_name.get(name)}, this tree's constant says {want}")
if len(set(by_name.values())) != len(by_name):
    dupes = [i for i in by_name.values() if list(by_name.values()).count(i) > 1]
    fail.append(f"duplicate level-event ids: {sorted(set(dupes))}")

# ---- note block ---------------------------------------------------------
nb = json.load(open(os.path.join(src, "note_block.json")))
if len(nb["table"]) != nb["instruments"]:
    fail.append(f"{len(nb['table'])} instrument rows for {nb['instruments']}")
# Every instrument's sound event must be in the sound registry, by id AND by
# name, because `NoteBlock.triggerEvent` sends it as a Holder of that registry
# and a -1 here would be a sound the server cannot number.
for row in nb["table"]:
    if row["soundId"] < 0:
        fail.append(f"instrument {row['name']}: sound {row['sound']} has no registry id")
    elif report_sounds.get(row["sound"]) != row["soundId"]:
        fail.append(f"instrument {row['name']}: sound {row['sound']} is id {row['soundId']} "
                    f"here and {report_sounds.get(row['sound'])} in the report")
# `harp` is the default `setInstrument` falls back to and the one the corpus
# hears; if it moved, every note block in the corpus plays the wrong sound.
harp = [r for r in nb["table"] if r["name"] == "harp"]
if not harp:
    fail.append("no `harp` instrument — it is setInstrument's fallback and the corpus's only one")
elif harp[0]["sound"] != "minecraft:block.note_block.harp" or not harp[0]["tunable"]:
    fail.append(f"harp is {harp[0]['sound']} tunable={harp[0]['tunable']}")
# The volume above 1.0 that makes `SoundEvent.getRange`'s multiply fire.
if nb["noteVolume"] != 3:
    fail.append(f"NoteBlock.NOTE_VOLUME is {nb['noteVolume']}, not 3 — the note block's audible "
                "radius is 16*volume and moves with it")

if fail:
    for f in fail:
        print("sound-particle-golden: " + f, file=sys.stderr)
    sys.exit(1)

for f in ("sound_events.json", "particle_types.json", "level_events.json", "note_block.json"):
    shutil.copyfile(os.path.join(src, f), os.path.join(outdir, f))

print(f"sound events   -> {outdir}/sound_events.json (identical across two runs)")
print(f"  {se['events']} events, {se['fixedRangeEvents']} with a fixed range "
      f"(the read is controlled: {se['fixedRangeReadWorks']}), "
      f"{se['keyDiffersFromLocation']} whose registry key is not their sound location")
print(f"particle types -> {outdir}/particle_types.json")
print(f"  {pt['types']} types, {pt['simpleTypes']} simple, "
      f"{pt['types'] - pt['simpleTypes']} carrying options on the wire")
print(f"note block     -> {outdir}/note_block.json")
print(f"  {nb['instruments']} instruments, {nb['tunableInstruments']} tunable, "
      f"{nb['customSoundInstruments']} with a custom sound; NOTE_VOLUME {nb['noteVolume']}")
print(f"level events   -> {outdir}/level_events.json")
print(f"  {le['levelEvents']} int constants on LevelEvent")
PYEOF
