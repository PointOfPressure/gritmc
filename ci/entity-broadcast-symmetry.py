#!/usr/bin/env python3
"""Does anything in the corpus rest on the client `observe_from` throws away?

# ===== AGENT INTERFACE =====
#
#     ./ci/entity-broadcast-symmetry.py           paired recordings on the box
#     ./ci/entity-broadcast-symmetry.py DIR ...   trees to read
#
# Prints. Never gates.
# ===== end interface; everything below is why =====

# THE WORRY, STATED PROPERLY

`project.observe_from` keeps one client's stream and discards the other's
whole. `ci/checks/projection-drops.toml` ranked the drop `decides` on "95 of
108 checks keep the actor alone, 7 keep the observer alone, 6 keep both", so
102 of 108 discard a client. **Re-derived on the tree that lands the entity
family: 95 actor, 7 observer, 13 both, of 115** — the ledger now carries that
split, and this paragraph kept the pre-merge figure so the movement is legible — the seven new entity checks
all declare `both`, so the discarding count is unchanged at 102 and the
denominator moved. 11 of the 13 checks in the whole corpus that keep both
clients are entity checks, which is the opposite of the shape the worry
assumes. Measured here over 64 recordings, **8,111
observations of checks' OWN declared kinds are discarded, across 81 of 115
checks.**

The worry that follows is specific and it is worth stating precisely, because
the vague version ("entities are invisible") is not true: a `match` on a check
that keeps the actor is evidence about the actor's stream and about nothing
else, so a server that told the acting player the truth and the bystander
nothing at all would be **green**. Every entity claim is a claim about what a
second player sees, so this is the class where that would bite hardest.

# THE ANSWER, AND IT IS A NEGATIVE WITH ONE ASTERISK

Measured over 61 PAIRED recordings — vanilla and ours from the same tree —
across every scenario in `ci/checks/entity-reach.toml`:

  * **38 `(scenario, kind)` pairs where we emit nothing at all.** Not
    asymmetries. `set_entity_motion`, `set_entity_data`, `move_entity` and
    `add_entity` for a falling block or a mob are absent from BOTH our clients,
    which is `500`'s and `080`'s declared `diverge`/`absent` and is already on
    the roadmap.
  * **18 pairs both-sided on both servers.** Wherever this server emits an
    entity packet at all, it emits it to the bystander too.
  * **One row flagged asymmetric**, and it does not survive reading:
    `container-click-throw-drops-one-item`'s `entity_position`. Vanilla told
    the bystander in 58 of 58; we sent 5 to the actor and 0 to anyone else over
    61 recordings. Five observations in sixty-one runs is the stray rate of a
    kind `ci/checks/projection-drops.toml` already carries a whole row for —
    `entity_position_unless_observed`, measured at "3 strays on vanilla's side
    and 2 on ours across 23 scenarios", dropped for every check that does not
    name the kind. It is a coin flip on `wasOnGround != onGround()`, not a
    broadcast decision.

**So: nothing in this corpus is green because a check read the one client it
was told about.** That is the useful form of the answer. The vague worry —
"entities are invisible because 102 of 115 checks discard a client" — is not
true of what this server does, and it is worth having measured, because the
hypothesis is cheap to state and would otherwise be re-raised by every reader
who meets the 102-of-115 figure.

**What the discarded client DOES hide is a proportion, not a presence**, and
the two places it shows are the two kinds that already have their own drops:
`a-dropped-item-goes-into-your-inventory`'s `entity_position` is 41/41 on
vanilla's bystander and 2/41 on ours, and `torch-falls-with-its-support`'s
`move_entity_heartbeat` is 58/58 against 19/58. Both kinds are removed from
every check that does not name them (`entity_position_unless_observed`,
`heartbeat_unless_observed`), so what is hidden here is hidden twice over and
by a declared line rather than by this one.

# THE TRAP THIS SCRIPT WALKED INTO FIRST, KEPT AS A COMMENT

The first version flagged a row when vanilla had observer observations and we
had none, and lit up on six of seven scenarios. Every one of those was a
scenario where we emit the kind **to nobody** — the actor count was zero too.
"We never send `set_entity_motion`" is a real gap and it is `500`'s, already
declared; it is not a broadcast asymmetry, and reporting it as one would have
been a finding about a bug that does not exist. The test has to be a RATIO and
not a presence, and it has to be skipped where the numerator is zero.

# NOT A DIVERGENCE, AND WHY THE NUMBERS ARE PRINTED AS DISTRIBUTIONS

This compares counts per `(scenario, kind, client)` and not projected
observations, so nothing it prints is a corpus verdict. Counts differ between
two runs for reasons that are not defects — the `corpus-projection-audit` branch retracted claim `551` on
exactly that, where vanilla emitted four step sounds in 32 runs and five in 39.
So every row carries the number of recordings behind it, and a single pair is
never reported.
"""

from __future__ import annotations

import glob as globmod
import json
import os
import sys
import tomllib
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

ENTITY_KINDS = {
    "add_entity", "remove_entities", "set_entity_data", "set_entity_motion",
    "move_entity", "move_entity_rot", "move_entity_heartbeat", "entity_position",
    "rotate_head", "take_item_entity", "damage_event", "entity_event",
    "set_equipment", "update_attributes", "animate", "teleport_entity",
}


def scenarios_of_interest() -> set[str]:
    p = os.path.join(ROOT, "ci", "checks", "entity-reach.toml")
    with open(p, "rb") as f:
        return {r["check"] for r in tomllib.load(f).get("spawns", [])}


def main(argv: list[str]) -> int:
    want = scenarios_of_interest()
    pats = argv or [os.path.expanduser("~/rustmc*/oracle-work/gameplay/vanilla.json")]

    # (scenario, kind) -> list of (van_actor, van_obs, our_actor, our_obs)
    rows: dict[tuple[str, str], list[tuple[int, int, int, int]]] = defaultdict(list)
    n = 0
    for pat in pats:
        for vp in sorted(globmod.glob(pat)):
            rp = vp.replace("vanilla.json", "rustmc.json")
            if not os.path.exists(rp):
                continue
            try:
                with open(vp) as f:
                    V = json.load(f)
                with open(rp) as f:
                    R = json.load(f)
            except Exception:
                continue
            n += 1
            c: dict[tuple[str, str, str, str], int] = Counter()
            for tag, D in (("van", V), ("our", R)):
                for sc in D["scenarios"]:
                    if sc["name"] not in want:
                        continue
                    for o in sc.get("obs", []):
                        if o["kind"] in ENTITY_KINDS:
                            c[(tag, sc["name"], o["who"], o["kind"])] += 1
            for (tag, s, who, k) in list(c):
                if tag != "van" or who != "actor":
                    continue
                rows[(s, k)].append((
                    c[("van", s, "actor", k)], c[("van", s, "observer", k)],
                    c[("our", s, "actor", k)], c[("our", s, "observer", k)],
                ))

    if not n:
        print("no paired (vanilla.json, rustmc.json) recordings found.")
        return 0

    print(f"N = {n} paired recording(s) — vanilla and ours from the same tree.")
    print("Counts per (scenario, kind, client). NOT a corpus verdict: a count "
          "difference is a distribution, not a divergence.\n")

    asym, silent, ok = [], [], []
    for (s, k), obs in sorted(rows.items()):
        vb = sum(1 for a, b, _, _ in obs if b > 0)      # vanilla told the bystander
        oa = sum(c for _, _, c, _ in obs)               # we said anything at all
        ob = sum(1 for _, _, _, d in obs if d > 0)      # we told the bystander
        if not vb:
            continue
        if oa == 0:
            silent.append((s, k, len(obs)))
        elif ob == 0:
            asym.append((s, k, len(obs)))
        else:
            ok.append((s, k, len(obs), vb, ob))

    print(f"BROADCAST ASYMMETRY — vanilla tells the bystander and we tell only "
          f"the actor: {len(asym)}")
    if not asym:
        print("  (none). Nothing in this corpus is green because a check read "
              "the one client it was told about.")
    for s, k, m in asym:
        print(f"  {s:52s} {k:22s} over {m} recording(s)")
    print()

    print(f"WE EMIT NOTHING AT ALL — not an asymmetry, a gap, and declared "
          f"elsewhere: {len(silent)}")
    for s, k, m in silent:
        print(f"  {s:52s} {k:22s} over {m} recording(s)")
    print()

    print(f"BOTH-SIDED ON BOTH SERVERS: {len(ok)}")
    for s, k, m, vb, ob in ok:
        print(f"  {s:52s} {k:22s} vanilla told the bystander in {vb}/{m}, "
              f"we did in {ob}/{m}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
