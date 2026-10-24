#!/usr/bin/env bash
# dump-flat-preset.sh — extract vanilla's superflat world preset into
# data/vanilla/flat_preset.json.
#
# Why this exists at all: the gameplay oracle needs a world both servers
# agree about *before* a single gameplay packet is sent. On the noise world
# they do not — rustmc's terrain is measured against vanilla's to nine
# decimal places elsewhere in this repo, but "nine decimal places" is not
# "identical", and a gameplay check that fails because a tree grew in one
# world and not the other tells you nothing about gameplay. Superflat is
# vanilla's own answer to "a world with no terrain in it": four layers, one
# biome, no features, no lakes. Every column is the same column, so a
# scenario written relative to where the player is standing means the same
# thing on both servers by construction.
#
# The preset is **datapack data**, so it is extracted rather than
# transcribed, exactly like registries, items and block properties. Mojang's
# own generator emits it; `FlatLevelGeneratorSettings`'s builder calls are
# not the source and never were.
#
# Output is gitignored with the rest of data/vanilla/. Without it, the server
# refuses `--level-type flat` by name rather than inventing four layers,
# because a substrate this harness is quoted against must never be a guess.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
SRC="$ROOT/oracle-work/vanilla-$VER/generated-data/data/minecraft/worldgen/world_preset/flat.json"
OUT="$ROOT/data/vanilla/flat_preset.json"

"$ROOT/ci/vanilla-server.sh" data >/dev/null

# REFUSAL, NOT A SKIP — changed 2026-08-12. This read `exit 0`, and it is the
# only one of the nineteen generators `ci/gen-gameplay-tables.sh` drives that
# could return success having written no table.
#
# A skip is the right shape for a *test* that cannot run without the extract —
# which is where the `skipping: no vanilla` idiom comes from, and it is still
# correct in the dozen `crates/rustmc-worldgen/tests/*.rs` that use it. It is
# the wrong shape here. This script is not asked to run; it is asked to
# produce `data/vanilla/flat_preset.json`, its caller has already established
# that the table is missing or stale, and there is no state of the world in
# which "I did not make it" is a success. Line 30 above runs the datagen, so
# reaching here means the datagen ran and produced nothing — which is a
# failure of the thing this script exists to wrap, reported as a green.
#
# What the green cost: the caller sees status 0, finds a non-empty
# `flat_preset.json` on disk — another tree's, copied in by the `cp -rn` that
# starts every gate directory — and stamps it with this tree's provenance.
# `ci/gen-gameplay-tables.sh` now also catches that from the outside by
# checking the mtime moved, so this is belt and braces; both exist because a
# false zero here is laundered into the one instrument that could otherwise
# still catch it.
if [ ! -s "$SRC" ]; then
    echo "dump-flat-preset: no vanilla datagen extract at $SRC" >&2
    echo "    ci/vanilla-server.sh data ran but produced nothing; cannot generate $OUT" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUT")"
python3 - "$SRC" "$OUT" <<'PY'
import json, sys

src, out = sys.argv[1], sys.argv[2]
preset = json.load(open(src))
# The overworld stem is the only one superflat replaces; the nether and the
# end keep their noise generators, which is why this reaches past
# "dimensions" for one named key rather than taking the first.
gen = preset["dimensions"]["minecraft:overworld"]["generator"]
if gen["type"] != "minecraft:flat":
    raise SystemExit(f"overworld generator is {gen['type']}, not minecraft:flat")
s = gen["settings"]
# `features` and `lakes` are carried even though both are false in the
# shipped preset: a harness substrate that quietly gained decoration would
# otherwise look like a gameplay divergence.
json.dump(
    {
        "biome": s["biome"],
        "features": s.get("features", False),
        "lakes": s.get("lakes", False),
        "layers": [{"block": l["block"], "height": l["height"]} for l in s["layers"]],
    },
    open(out, "w"),
    indent=2,
)
print(f"flat preset: {sum(l['height'] for l in s['layers'])} layers deep, biome {s['biome']} -> {out}")
PY
