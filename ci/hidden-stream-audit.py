#!/usr/bin/env python3
"""What each check's `observe_from` removes, and whether the two servers differ in there.

    ci/hidden-stream-audit.py [<recording-dir>]

`<recording-dir>` holds `vanilla.json`, `rustmc.json` and `report.json` — any
tree's `oracle-work/gameplay` will do. Reads recordings that already exist; it
records nothing, needs no server, no build and no lock.

# The axis this is about

`ci/projection-drops.py` enumerates the *fields* the projection removes. This
is the other half: every scenario stands two clients, an actor and an
observer, and `observe_from` throws one of them away. Ninety-five of the 108
checks keep the actor alone, seven keep the observer alone, six keep both. So
for 102 checks there is a whole recorded stream that no comparison reads, and
a divergence sitting in it is invisible in exactly the way a dropped field is.

That is not a defect in `observe_from`. Restricting to one client is usually
the whole point of a check — `420` reads the bystander *because* the
interacting player is the one vanilla does not send the sound to, and reading
the actor would make it inert. What is a defect is nobody ever looking in the
half that was discarded.

# It is a screen, not a verdict, and the difference matters

Two of its first three hits were not divergences:

* **`490-a-bystander-sees-a-chest-open`** showed 10 `block_event` against our
  9. A chest re-announces its open state about every 250 ms and the window is
  a fixed length, so the count is decided by where the boundary falls against
  a free-running timer. Both servers repeat at the same rate. The same
  mechanism as `move_entity_heartbeat`, one packet down.
* **`605`, `606`, `500`, `530`** differ only in an item entity's `y_rot`,
  which is a `nextFloat()` draw and is exempted by name in `compare::project`.

So a hit means *look here*, and the looking is the part with judgement in it.
Entity ids are renumbered per stream before comparing, the way
`compare::renumber_entities` does, because raw ids are a counter no two runs
agree about and comparing them raw makes every hit a false one.

# What it found, and how its first reading of it was wrong

`551-attacking-from-out-of-range`, on the observer stream it discards: this
looked like a plain divergence on one pair — vanilla four step sounds walking
z=20 to z=3, ours five — **and it is not one. Corrected the same day, from the
recordings already on the box:**

    vanilla   4 in 32 runs, 5 in 39 runs   (n = 71)
    ours      5 in 66 runs, 4 in 0         (n = 66)

Vanilla is near a coin flip and we are pinned at five. That is still a real
difference between the two servers and it is a **distribution**, not an
observation, so no per-run comparison can reach it: a check reading the
observer stream would flap on vanilla's own variance and be declared unstable,
which is the right treatment of the reference and the wrong conclusion about
the subject. `ci/determinism-gap.py` is the instrument for that class, and it
exists because this claim was made wrongly from a single pair first.

What survives unchanged is the point about the axis: 551 keeps the actor, a
player is not sent their own step sounds, and so nothing about those sounds
can ever reach 551's comparison whatever their distribution. The hidden stream
is where the evidence was; one pair of it was not enough to say what the
evidence meant.
"""

import collections
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_files as cf  # noqa: E402 — must follow the sys.path line

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_REC = os.path.join(ROOT, "oracle-work/gameplay")


def renumber(obs):
    """Observations as a multiset, with entity ids replaced by order of first sight.

    The same normalisation `compare::renumber_entities` applies, and for the
    same reason: a raw entity id is a per-session counter, so comparing two
    recordings on it makes every entity-bearing observation differ.
    """
    seen, out = {}, []
    for o in obs:
        s = json.dumps(o["fields"], sort_keys=True)
        for eid in re.findall(r'"\$entity": (\d+)', s):
            if eid not in seen:
                seen[eid] = len(seen)
        s = re.sub(r'"\$entity": (\d+)', lambda g: '"$entity": %d' % seen[g.group(1)], s)
        out.append((o["kind"], s))
    return collections.Counter(out)


def main():
    rec = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_REC
    try:
        sides = {
            w: {s["name"]: s["obs"] for s in json.load(open(os.path.join(rec, f)))["scenarios"]}
            for w, f in (("ref", "vanilla.json"), ("sub", "rustmc.json"))
        }
        report = {
            o["name"]: o
            for o in json.load(open(os.path.join(rec, "report.json")))["outcomes"]
        }
    except (OSError, KeyError) as exc:
        # A refusal rather than an empty report: "no recordings" and "no hidden
        # divergences" must never print the same thing.
        sys.exit(f"hidden-stream-audit: cannot read a recording set at {rec}: {exc}")

    checks = cf.load()
    tally = collections.Counter(c.observe_from for c in checks.values() if c.observe)
    hits = []
    for name, c in sorted(checks.items()):
        if not c.observe or c.observe_from == "both":
            continue
        dropped = "observer" if c.observe_from == "actor" else "actor"
        kinds = set(c.observe)
        a, b = (
            renumber([o for o in sides[w].get(name, []) if o["who"] == dropped and o["kind"] in kinds])
            for w in ("ref", "sub")
        )
        if a != b:
            hits.append((c.file, report.get(name, {}).get("verdict"), dropped, a, b))

    print(
        f"hidden-stream-audit: {rec}\n"
        f"observe_from over {sum(tally.values())} checks: "
        + ", ".join(f"{v} {k}" for k, v in sorted(tally.items()))
        + f"\n{len(hits)} checks differ inside the stream their own projection removes\n"
    )
    for f, verdict, dropped, a, b in hits:
        print(f"{verdict or '?':<8} {f}   [hidden: {dropped}]")
        for k in sorted(set(a) | set(b)):
            if a[k] != b[k]:
                print(f"    vanilla x{a[k]}  ours x{b[k]}  {k[0]}  {k[1][:160]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
