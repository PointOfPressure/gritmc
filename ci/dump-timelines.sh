#!/usr/bin/env bash
# dump-timelines.sh — extract vanilla's environment timelines and the
# dimension-type attribute maps they layer onto, into
# data/vanilla/timelines.json.
#
# WHY THIS IS AN EXTRACT AND NOT A CONSTANT TABLE
#
# In 26.2 the sky is data. `Level.getSkyDarken()` is
# `(int)(15.0f - environmentAttributes().getDimensionValue(SKY_LIGHT_LEVEL))`
# and `DaylightDetectorBlock.updateSignalStrength` reads
# `environmentAttributes().getValue(SUN_ANGLE, pos)` — neither is computed
# from the time of day by any expression in the code. Both are sampled off
# **keyframe tracks** carried by `minecraft:timeline` entries, which is a
# datapack registry: `data/minecraft/timeline/day.json` holds eighteen
# tracks, of which `visual/sun_angle` and `gameplay/sky_light_level` are the
# two this server reads today.
#
# So a hand-written `sunAngle(t)` would be a transcription of *data*, and
# every earlier version of this game had exactly such a formula in the code —
# which is the trap: the old trigonometric one is close enough to the new
# keyframe curve to look right and is wrong by up to a whole power level. The
# curve is extracted for the same reason `flat_preset.json` and
# `block_props.json` are.
#
# Nothing here is transcribed from the bytecode. The *consumer* of this table
# — `crate::sky` — is, and its own header carries those offsets.
#
# Output is gitignored with the rest of data/vanilla/.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
GEN="$ROOT/oracle-work/vanilla-$VER/generated-data/data/minecraft"
OUT="$ROOT/data/vanilla/timelines.json"

"$ROOT/ci/vanilla-server.sh" data >/dev/null

# REFUSAL, NOT A SKIP — same rule as ci/dump-flat-preset.sh, and for the same
# reason: this script is not asked to run, it is asked to produce $OUT, and
# "the datagen made nothing" is a failure of the thing it wraps.
if [ ! -d "$GEN/timeline" ]; then
    echo "dump-timelines: no vanilla datagen extract at $GEN/timeline" >&2
    echo "    ci/vanilla-server.sh data ran but produced no timeline registry" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUT")"
python3 - "$GEN" "$OUT" <<'PY'
import json, os, sys

gen, out = sys.argv[1], sys.argv[2]


def load_dir(sub):
    d = os.path.join(gen, sub)
    got = {}
    for name in sorted(os.listdir(d)):
        if name.endswith(".json"):
            got["minecraft:" + name[:-5]] = json.load(open(os.path.join(d, name)))
    return got


timelines = load_dir("timeline")
tags = load_dir("tags/timeline")
dims = load_dir("dimension_type")

if "minecraft:day" not in timelines:
    raise SystemExit("no minecraft:day timeline — the registry moved")

# Resolve the timeline tags one level at a time. `#minecraft:in_overworld` is
# `[#minecraft:universal, day, moon, early_game]`, so a flat read of one tag
# silently drops whatever `universal` carries. Refusing on a cycle rather than
# recursing forever is deliberate: a cyclic tag is a datapack error, not a
# shape this loader should have an opinion about.
def resolve(tag, seen):
    if tag in seen:
        raise SystemExit(f"timeline tag cycle at {tag}")
    seen = seen | {tag}
    out = []
    for v in tags[tag]["values"]:
        if v.startswith("#"):
            out += resolve(v[1:], seen)
        else:
            out.append(v)
    return out


resolved = {t: resolve(t, frozenset()) for t in tags}

# Every dimension type carries its own static attribute map plus the tag naming
# the timelines that layer over it. Both halves are kept: the static map is
# what `getDimensionValue` falls back to and the timelines are what move.
dim_out = {}
for name, d in dims.items():
    tl = d.get("timelines")
    dim_out[name] = {
        "has_skylight": d["has_skylight"],
        "has_ceiling": d["has_ceiling"],
        "ambient_light": d["ambient_light"],
        "min_y": d["min_y"],
        "height": d["height"],
        "timelines_tag": tl,
        "timelines": resolved[tl[1:]] if isinstance(tl, str) and tl.startswith("#")
        else ([tl] if isinstance(tl, str) else list(tl or [])),
        "attributes": d.get("attributes", {}),
    }

json.dump(
    {
        "what": "minecraft:timeline registry, its tags, and the dimension_type attribute maps",
        "timelines": timelines,
        "timeline_tags": resolved,
        "dimension_types": dim_out,
    },
    open(out, "w"),
    indent=2,
    sort_keys=True,
)

day = timelines["minecraft:day"]
print(
    f"timelines: {len(timelines)} timelines, {len(resolved)} tags, "
    f"{len(dim_out)} dimension types; minecraft:day has {len(day['tracks'])} tracks "
    f"over {day['period_ticks']} ticks -> {out}"
)
PY
