#!/usr/bin/env python3
"""Did the survival substrate actually take? Refuse if it did not.

    ci/survival-substrate-assert.py <vanilla.json>

Exit 0 if the reference emitted at least one kind that is gated behind a
creative early return; exit 4 with an explanation if it emitted none.

# WHY THIS IS A SEPARATE SCRIPT AND NOT THREE LINES IN THE HARNESS

**A game mode that did not take produces a recording that looks exactly like a
green.** Every scenario runs, every packet the creative substrate emits still
arrives, and the comparison is perfectly well-formed — it is simply a
measurement of the wrong server. There is no downstream check that would catch
it: `assert_recording` counts scenarios, the readiness proofs test sockets, and
`compare` has no opinion about which substrate produced its inputs.

That makes this the one assertion in `ci/t2-gameplay-survival.sh` whose absence
is invisible, which is exactly the class this repo keeps being caught by. So it
is a file with a falsifier (`ci/survival-substrate-assert-falsify.sh`) rather
than an inline block nobody can run on a synthetic input.

# WHAT IT DOES *NOT* CLAIM

Only that the substrate opened. It says nothing about whether this server
matched vanilla on any of it — that is `compare`'s job — and nothing about how
many observations arrived, which is the number the baseline wants and which
must come from more than one run.
"""

import json
import sys

# Kinds behind a creative early return. See ci/substrate-gate-audit.py, which
# reads each gate out of the pinned jar and refuses if one has moved.
GATED = ("damage_event", "set_health", "block_destruction", "set_experience")


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    try:
        d = json.load(open(sys.argv[1]))
    except (OSError, ValueError) as e:
        print(f"survival-substrate-assert: cannot read {sys.argv[1]}: {e}", file=sys.stderr)
        return 2

    seen = {}
    for s in d.get("scenarios", []):
        for o in s.get("obs", []):
            if o.get("kind") in GATED:
                seen.setdefault(o["kind"], set()).add(s.get("name", "?"))

    if seen:
        print("  substrate-gated kinds now on the reference wire:")
        for k in sorted(seen):
            print(f"    {k:20s} in {len(seen[k])} scenario(s): "
                  f"{', '.join(sorted(seen[k])[:4])}")
        return 0

    print(f"""
SURVIVAL PASS REFUSED — vanilla emitted none of {', '.join(GATED)}.

Every one of those is gated behind an early return that `gamemode=survival` is
supposed to open, so this recording is the CREATIVE substrate wearing a
survival label, and no verdict taken against it means anything.

This failure looks exactly like a green everywhere else in the pipeline, which
is why it is asserted here.

Check:
  * the work dir's server.properties actually says `gamemode=survival`, and
    that no later line overrode it — VANILLA_EXTRA_PROPS is APPENDED, so a
    duplicate key later in the file wins;
  * the scenarios reached the steps that provoke damage or mining at all;
  * `ci/substrate-gate-audit.py` still verifies every gate against the jar —
    if a gate MOVED, the reading this pass rests on has gone stale.
""".rstrip(), file=sys.stderr)
    return 4


if __name__ == "__main__":
    sys.exit(main())
