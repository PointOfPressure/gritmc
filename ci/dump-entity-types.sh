#!/usr/bin/env bash
# dump-entity-types.sh — the entity_type registry's numeric ids, from
# vanilla's own report.
#
# `ClientboundAddEntityPacket` writes its type through
# `ByteBufCodecs.registry(Registries.ENTITY_TYPE)`, which is a plain varint of
# the registry id, so a server that cannot map a name to that number cannot
# announce an entity at all.
#
# # Why this is not read out of `registries.bin`, which was tried first
#
# `data/vanilla/registries.bin` is the capture of everything vanilla sends in
# the `registry_data` packets during configuration, and it is where this
# server already gets `minecraft:world_clock` from. **It does not contain
# `minecraft:entity_type` and never will.** Only datapack-driven registries
# are synced to a client; the built-in ones — entity types, items, blocks —
# are compiled into the client and are never on the wire.
#
# The symptom, in case it recurs for another registry: `entry_ids` returns
# `Ok(None)`, every entity resolves to no kind id, `pairing_bundle` sends
# nothing, and the server looks exactly like one with no entity tracking at
# all. It cost one subset run to find, and only because the failure path
# printed a line rather than defaulting the id to zero — which would have
# announced every player as an `allay` and read as a behaviour bug.
#
# Same rule as `ci/dump-items.sh`, for the same reason: registry ids are
# generated data, read from the generator's own report and never from memory.
# `ci/vanilla-server.sh reports` produces `generated/reports/registries.json`.
#
# The output is gitignored like the rest of `data/vanilla/`. Regenerate on a
# version bump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
REPORT="$ROOT/oracle-work/vanilla-$VER/generated/reports/registries.json"
OUT="${1:-$ROOT/data/vanilla/entity_types.json}"

if [ ! -f "$REPORT" ]; then
    echo "dump-entity-types: no report at $REPORT — run ci/vanilla-server.sh reports first" >&2
    exit 1
fi

python3 - "$REPORT" "$OUT" <<'PY'
import json, sys
report, out = sys.argv[1], sys.argv[2]
with open(report) as f:
    entries = json.load(f)["minecraft:entity_type"]["entries"]
# Sorted by id, so a diff between two versions reads as a diff rather than as
# a reshuffle: the ids are what the wire carries and the order is what moved.
table = {name: e["protocol_id"] for name, e in entries.items()}
with open(out, "w") as f:
    json.dump({"entity_types": dict(sorted(table.items(), key=lambda kv: kv[1]))}, f, indent=0)
print(f"{len(table)} entity types -> {out}")
PY
