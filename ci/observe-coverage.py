#!/usr/bin/env python3
"""Which of a check's declared kinds actually appear, and which check rests on one.

    ci/observe-coverage.py [<recording-dir>]

Reads a recording pair that already exists — any tree's `oracle-work/gameplay`.
Records nothing, needs no server, no build and no lock.

# The two questions, and only one of them is a defect

An `observe` list is a *declaration*, not a prediction, so a kind that never
appears is usually fine and sometimes the point. 74 checks declare
`section_blocks_update` and almost none produce one; if this server ever
started sending one where vanilla does not, those 74 would catch it. **A
declared kind that never appears on either side is a one-sided guard**, and a
corpus with none of them would be a corpus that only ever looked where it
already expected something.

The defect is the other question: **what is left when the dead ones are taken
away.** `informative` then subtracts `block_changed_ack` outright and one
`block_update` per position the step was always going to change, and what
survives *that* is the whole evidence for the claim. When it is one
observation, the check's name is making a promise its projection cannot keep
on its own.

# What it found

* **`550-attacking-another-player` and `551-attacking-from-out-of-range`
  declare four kinds and three of them — `damage_event`, `set_health`,
  `entity_event` — never appear on either side.** Vanilla emits none of the
  three for a player hitting a player in these scenarios. Both checks are
  `match`, both rest on `sound` alone, and the sound is
  `entity.player.attack.nodamage` at a literal volume: their green is about a
  sound being played and a reach test, and it is not about damage.
* **Four checks are `match` on zero informative observations** — 561, 571, 581
  and 608. All four are "changes nothing" or "is ignored" claims, so an empty
  measured step *is* the claim rather than a hole in it, and all four reach
  tier 2 in `ci/checks/gameplay-falsifiable.toml`. Reported, not refused.

Both are printed rather than gated. A gate here would need recordings, and T0
is tree-only; and the judgement — is an empty step this claim, or a hole in it
— is the part a script should not be making.
"""

import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_files as cf  # noqa: E402 — must follow the sys.path line

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_REC = os.path.join(ROOT, "oracle-work/gameplay")


def main():
    rec = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_REC
    try:
        counts = {
            w: {
                s["name"]: collections.Counter(o["kind"] for o in s["obs"])
                for s in json.load(open(os.path.join(rec, f)))["scenarios"]
            }
            for w, f in (("ref", "vanilla.json"), ("sub", "rustmc.json"))
        }
        report = {
            o["name"]: o for o in json.load(open(os.path.join(rec, "report.json")))["outcomes"]
        }
    except (OSError, KeyError) as exc:
        # "no recordings" and "nothing to report" must not print the same thing.
        sys.exit(f"observe-coverage: cannot read a recording set at {rec}: {exc}")

    checks = cf.load()
    resting, any_dead = [], 0
    for name, c in sorted(checks.items()):
        r, s = counts["ref"].get(name), counts["sub"].get(name)
        if r is None:
            continue
        if not c.observe:
            continue
        dead = [k for k in c.observe if not r.get(k) and not s.get(k)]
        live = [k for k in c.observe if r.get(k) or s.get(k)]
        if dead:
            any_dead += 1
        n = report.get(name, {}).get("measured_reference_obs")
        if n is not None and n <= 1:
            resting.append((n, report[name]["verdict"], c.file, live, dead))

    print(f"observe-coverage: {rec}")
    print(f"{any_dead} of {len(checks)} checks declare a kind that never appears on either side")
    print("  (that is a one-sided guard, not a defect — it fires if this server starts")
    print("   emitting one where vanilla does not)\n")
    print(f"{len(resting)} checks rest on one informative observation or none:")
    for n, verdict, f, live, dead in sorted(resting):
        print(f"  n={n} {verdict:<9} {f}")
        print(f"        rests on: {live or '<nothing>'}")
        if dead:
            print(f"        declared but never seen: {dead}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
