#!/usr/bin/env python3
"""Kinds a check declares that vanilla has never sent for it.

# ===== AGENT INTERFACE =====
#
#     ./ci/corpus-dead-observe.py              the box's vanilla recordings
#     ./ci/corpus-dead-observe.py a.json ...   named recordings
#
# Prints. Never gates.
# ===== end interface; everything below is why =====

# THE TWO CLASSES, AND ONLY ONE OF THEM IS A FINDING

A check's `observe` list is what it claims to be about. A kind in that list
which vanilla never emits for that scenario contributes nothing to the declared
verdict — the check is decided by whatever else it named. Over 64 recordings on
the box there are 81 such `(check, kind)` pairs, and **most of them are
deliberate**:

  * `section_blocks_update` beside `block_update`. `ci/checks/gameplay/README.md`
    requires it: vanilla coalesces two changes in one section in one tick into
    the section packet, so a check that named only `block_update` goes `inert`
    the day its mechanic starts moving two blocks. Declaring both is INSURANCE
    against a future the recordings have not reached, and eight checks were
    reported inert on 2026-08-03 for want of it.
  * `block_update` beside `section_blocks_update`, the same pair the other way
    round, on the digging checks.

Those are correct and this script says so rather than listing them as debt.
What is left is the class worth reading: a check naming a kind that **nothing
in the corpus has ever produced, anywhere**, which is a different claim from
"not for this scenario". A kind that has never once appeared in 64 recordings
is not insurance against a plausible future; it is a claim the substrate cannot
answer.

# THE ONE THAT IS LEFT, AND IT IS THE WHOLE OF COMBAT

    damage_event    observed by 550 and 551, seen 0 times in 64 recordings
    set_health      observed by 550 and 551, seen 0 times in 64 recordings

`javap -p -c -constants` on the pinned jar, `net/minecraft/world/entity/player/
Player.class`:

    public boolean hurtServer(ServerLevel, DamageSource, float)
       3  invokevirtual isInvulnerableTo:(...)Z
       6  ifeq 11
       9  iconst_0 / ireturn
      12  getfield  abilities:Labilities;
      15  getfield  Abilities.invulnerable:Z
      18  ifeq 33
      21  getstatic DamageTypeTags.BYPASSES_INVULNERABILITY
      25  invokevirtual DamageSource.is:(TagKey)Z
      28  ifne 33
      31  iconst_0 / ireturn        <- returns here, before anything is sent

The corpus substrate is `gamemode=creative`, which sets
`Abilities.invulnerable`, and a plain player attack is not in
`BYPASSES_INVULNERABILITY`. So `hurtServer` returns at offset 31 **before any
damage is applied, any `damage_event` is broadcast and any `set_health` is
sent**. The two checks are decided entirely by the attack `sound`, which is the
one thing that does reach the wire.

**This is not "combat is unimplemented".** Both are baselined `absent` and read
as a queue item, and they are not one: no server-side work of any kind can make
`damage_event` appear here, because vanilla does not send it either. It is the
same shape as `ci/checks/gameplay/README.md`'s survival-mining paragraph —
`Step::FinishDig` is correctly numbered, used by zero checks, and closing it
needs a **survival player**, which is a substrate change and not a check.
Combat damage is the second member of that class and it had not been named.

# WHY THIS IS NOT A GATE

The `(check, kind)` pairs move with what the recordings on the box happen to
cover — a scenario run in 12 recordings and not 61 has thinner evidence, and the
per-check recording count is printed for that reason. A gate on a quantity that
moves with the contents of `~/rustmc*/` would be a gate on the box's tidiness.
"""

from __future__ import annotations

import glob as globmod
import json
import os
import sys
import tomllib
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")

DEFAULT = [
    os.path.expanduser("~/rustmc*/oracle-work/gameplay/vanilla.json"),
    os.path.join(ROOT, "oracle-work", "gameplay", "vanilla.json"),
]

# The two directions of the README's coalescing rule. A check naming one of
# these because it also names the other is insured, not idle.
INSURANCE = {
    "section_blocks_update": "block_update",
    "block_update": "section_blocks_update",
}


def main(argv: list[str]) -> int:
    decl: dict[str, set[str]] = {}
    for f in sorted(globmod.glob(os.path.join(CHECKS, "*.toml"))):
        with open(f, "rb") as fh:
            d = tomllib.load(fh)
        decl[d["name"]] = set(d.get("observe", []))

    emit: dict[str, set[str]] = defaultdict(set)
    seen: Counter = Counter()
    everywhere: set[str] = set()
    n = 0
    for pat in (argv or DEFAULT):
        for p in sorted(globmod.glob(pat)):
            try:
                with open(p) as fh:
                    d = json.load(fh)
                scen = d["scenarios"]
            except Exception:
                continue
            n += 1
            for sc in scen:
                seen[sc["name"]] += 1
                for o in sc.get("obs", []):
                    emit[sc["name"]].add(o["kind"])
                    everywhere.add(o["kind"])

    if not n:
        print("no recordings found; nothing to say.")
        return 0

    insured: list[tuple[str, str]] = []
    dead: list[tuple[str, str]] = []
    for name, kinds in sorted(decl.items()):
        if name not in emit:
            continue
        for k in sorted(kinds - emit[name]):
            partner = INSURANCE.get(k)
            if partner and partner in kinds:
                insured.append((name, k))
            else:
                dead.append((name, k))

    never = sorted({k for _, k in dead} - everywhere)

    print(f"N = {n} vanilla recording(s).\n")
    print(f"INSURANCE, and correct: {len(insured)} pair(s) where a check names "
          f"one of `block_update`/`section_blocks_update` and also names the "
          f"other. Required by ci/checks/gameplay/README.md — vanilla coalesces "
          f"two changes in one section in one tick, and eight checks were "
          f"reported inert on 2026-08-03 for want of it.\n")

    print(f"DECLARED AND NEVER EMITTED FOR THAT SCENARIO: {len(dead)} pair(s)")
    for name, k in dead:
        flag = "   <- NEVER EMITTED ANYWHERE" if k in never else ""
        print(f"  {name:52s} in {seen[name]:3d} recs   {k}{flag}")

    print()
    if never:
        print(f"NEVER EMITTED ANYWHERE IN {n} RECORDINGS: {', '.join(never)}")
        print("  A kind absent from one scenario is thin evidence. A kind absent "
              "from every scenario in every recording is a claim the substrate "
              "cannot answer, and no server-side work can move it — vanilla is "
              "not sending it either. See this file's header for `damage_event` "
              "and `set_health`, whose site is Player.hurtServer offset 31.")
    else:
        print("every declared kind has been emitted somewhere.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
