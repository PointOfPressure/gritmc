#!/usr/bin/env python3
"""Print the block states one recording carried, per scenario.

The intact leg of `ci/schema-check-falsify.sh`. It asserts nothing: it prints
what this server put on the wire so the two checks that are NOT pinned there
have their evidence in the same output as the two that are.
"""
import json
import sys

d = json.load(open(sys.argv[1]))
for sc in d.get("scenarios", []):
    states = []
    for o in sc.get("obs", ()):
        k, f = o.get("kind"), o.get("fields", {})
        if k == "block_update":
            states.append(f.get("state"))
        elif k == "section_blocks_update":
            states += [b.get("state") for b in f.get("blocks", ())]
    live = [s for s in states if s and not s.startswith("minecraft:air")]
    print(f"   {sc.get('name')}")
    print(f"      {len(states)} state(s) on the wire; non-air: {live or 'NONE'}")
