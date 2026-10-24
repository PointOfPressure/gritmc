#!/usr/bin/env python3
"""Could a check that observed this packet kind ever be stable?

    ci/selfcheck-kind-stability.py <kind> <r1.json> <r2.json> ... <rN.json>
    ci/selfcheck-kind-stability.py set_entity_data \
        oracle-work/gameplay-selfcheck/r*.json

Reads the **recordings** `ci/t2-gameplay-selfcheck.sh` leaves behind — not its
pairwise reports — and asks, per scenario, whether the sequence of one kind's
observations is identical across all N runs of vanilla against itself.

# WHY THIS IS A DIFFERENT QUESTION FROM THE SELFCHECK'S OWN

The selfcheck answers "is this *check* reproducible", over the `observe` list
the check happens to name. That is the right question for a corpus that
exists, and the wrong one for a corpus somebody is deciding whether to extend:
a kind can be perfectly reproducible while every check that names it is
unstable for a reason nothing to do with it.

`a-dropped-item-goes-into-your-inventory` is exactly that shape. It is
declared unstable because `Block.popResource` scatters the drop by
`Mth.nextDouble(random, -0.25, 0.25)` per axis, and that randomness rides
`add_entity`, which the check observes. So the check can never come back
`match` for anybody, and adding a kind to *its* `observe` list cannot buy
coverage — a check that cannot pass cannot be made to cover more. The only
way to cover a kind that shares a scenario with a random one is a new check
that observes the kind and **not** the randomness, and whether that is worth
writing depends entirely on whether the kind itself reproduces.

That is what this measures, and it measures it from recordings already paid
for rather than from a new run.

# WHAT IS COMPARED, AND WHAT IS DELIBERATELY NOT

Per scenario, the ordered list of `(who, step, kind, fields)` for observations
of the named kind, with two normalisations that mirror what
`compare::project` does, and no others:

  * **entity ids are renumbered by first appearance** within that scenario's
    filtered sequence. A raw id is a counter no two runs agree about; the
    projection numbers over what a check looks at, so this does too. Doing it
    any other way reports every entity-bearing kind as unstable and says
    nothing.
  * **`at_ms` is dropped**, because the projection's default timing is
    `Ignore`.

Everything else is compared as recorded, `rest` digest included — which is the
point for a kind like `set_entity_data`, whose whole payload is in that digest
and is therefore already compared bit-exactly by anything that looks at it.

**This is a diagnostic and not a gate.** It says whether a check *could* be
stable, never whether one is correct.
"""

import json
import sys
from collections import Counter

# Must match `crate::trace::ENTITY_REF`. A wrong value here would silently
# stop renumbering anything and report every entity-bearing kind as unstable.
ENTITY_REF = "$entity"


def renumber(value, seen):
    """Entity ids to first-appearance order, mirroring `compare::project`."""
    if isinstance(value, dict):
        if len(value) == 1 and ENTITY_REF in value:
            raw = value[ENTITY_REF]
            if raw not in seen:
                seen[raw] = len(seen)
            return seen[raw]
        return {k: renumber(v, seen) for k, v in sorted(value.items())}
    if isinstance(value, list):
        out = [renumber(v, seen) for v in value]
        # `remove_entities` carries a set in the server's own iteration order.
        if out and all(isinstance(v, int) for v in out) and len(value) > 1:
            all_refs = all(
                isinstance(v, dict) and len(v) == 1 and ENTITY_REF in v for v in value
            )
            if all_refs:
                out.sort()
        return out
    return value


def sequence_for(run, scenario_name, kind):
    for sc in run["scenarios"]:
        if sc["name"] != scenario_name:
            continue
        seen = {}
        return [
            [o["who"], o["step"], o["kind"], renumber(o["fields"], seen)]
            for o in sc["obs"]
            if o["kind"] == kind
        ]
    return None


def main(argv):
    if len(argv) < 4:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    kind, paths = argv[1], argv[2:]
    runs = [json.load(open(p)) for p in paths]
    names = [sc["name"] for sc in runs[0]["scenarios"]]

    stable, unstable, silent = [], [], []
    for name in names:
        seqs = [sequence_for(r, name, kind) for r in runs]
        if any(s is None for s in seqs):
            continue
        counts = Counter(len(s) for s in seqs)
        if all(len(s) == 0 for s in seqs):
            silent.append(name)
        elif all(json.dumps(s) == json.dumps(seqs[0]) for s in seqs):
            stable.append((name, len(seqs[0])))
        else:
            unstable.append((name, dict(sorted(counts.items()))))

    print(f"== {kind}, {len(runs)} recordings, {len(names)} scenarios")
    for name, n in stable:
        print(f"  IDENTICAL IN ALL {len(runs)}  {name}  ({n} observation(s) each)")
    for name, counts in unstable:
        print(f"  DIFFERS               {name}  (per-run counts: {counts})")
    print(
        f"  {len(stable)} identical, {len(unstable)} differ, "
        f"{len(silent)} scenarios never saw this kind"
    )
    # A kind nothing produced is not evidence about the kind. Say so rather
    # than letting an empty run read as a clean one.
    if not stable and not unstable:
        print(f"  NO SCENARIO PRODUCED {kind} — this measured nothing")
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
