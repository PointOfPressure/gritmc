#!/usr/bin/env bash
# dump-items.sh — the item registry's numeric ids, from vanilla's own report.
#
# A creative client tells the server what it put in a slot by *item id*
# (`set_creative_mode_slot` carries `Item.STREAM_CODEC`, which is
# `holderRegistry` and writes a plain varint id). Without the mapping the
# server knows a player is holding something and not what, so it cannot place
# it — which is the whole of block placement.
#
# Registry ids are datapack-adjacent data, not code, and the rule this project
# already follows for them is: read them from the generator's own report,
# never from memory. `ci/vanilla-server.sh reports` produces
# `generated/reports/registries.json`; this pulls `minecraft:item` out of it.
#
# # THE SECOND TABLE — `data_component_types`, added 2026-08-13
#
# A stack on the wire is an id, a count and a `DataComponentPatch`, and the
# patch names each component by its **`minecraft:data_component_type` registry
# id** — a different registry from `minecraft:item`, 111 entries against 1,537.
# Until this table existed the harness could not name one, so `Step::Hold`
# wrote a literal `0` added components and **the corpus could not put a
# component on a stack at all**. That is not a small gap: it made
# `trace::read_item_stack`'s opaque arm unreachable by construction, so the
# `0 of 539 opaque` the item census reported was a property of this dumper's
# omission and not a reading of vanilla. See `docs/PARITY-ROADMAP.md` §44.5.
#
# It goes in the same file rather than a new one because `Tables::load` names
# every missing extract in its error and a fourth path is a fourth way for a
# tree to be half-set-up; the two tables are read by the same struct, in the
# same call, off the same report.
#
# The output is gitignored like the rest of `data/vanilla/`. Regenerate on a
# version bump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
REPORT="$ROOT/oracle-work/vanilla-$VER/generated/reports/registries.json"
OUT="${1:-$ROOT/data/vanilla/items.json}"

if [ ! -f "$REPORT" ]; then
    echo "dump-items: no report at $REPORT — run ci/vanilla-server.sh reports first" >&2
    exit 1
fi

python3 - "$REPORT" "$OUT" <<'PY'
import json, sys
report, out = sys.argv[1], sys.argv[2]
with open(report) as f:
    report = json.load(f)
entries = report["minecraft:item"]["entries"]
# Sorted by id so a diff between two versions reads as a diff and not as a
# reshuffle: the ids are what the wire carries and the order is what changed.
table = {name: e["protocol_id"] for name, e in entries.items()}
comps = {
    name: e["protocol_id"]
    for name, e in report["minecraft:data_component_type"]["entries"].items()
}
with open(out, "w") as f:
    json.dump(
        {
            "items": dict(sorted(table.items(), key=lambda kv: kv[1])),
            "data_component_types": dict(sorted(comps.items(), key=lambda kv: kv[1])),
        },
        f,
        indent=0,
    )
print(f"{len(table)} items, {len(comps)} data component types -> {out}")
PY
