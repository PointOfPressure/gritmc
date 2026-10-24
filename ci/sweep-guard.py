#!/usr/bin/env python3
"""Refuse a swept ledger that DEMOTES anything.

    ci/sweep-guard.py <committed-ledger> <swept-ledger>

**This is an agent-side guard around a sweep, not a T0 check**, and it is
deliberately not named `ci/check-*.py` so it does not join the set
`ci/check-checks-present.py` ratchets. It has no place in T0: T0 runs
`ci/corpus-falsifiable.py --check`, which reads the committed ledger, and by
then the damage this prevents is already committed. The window it guards is
the one between `--sweep` writing a file and a human staging it.

The rule this mechanises, and why it is a script rather than a look: on
2026-09-05 a sweep over a corpus broken by a leaked world border rewrote
`ci/checks/gameplay-falsifiable.toml` with fifteen checks moved from
`tier = 2` to `tier = 0` and `dead_fields` from 4 to 0. Both quantities are
RATCHETED, so committing it would have retired fifteen working checks and
widened two guards on the strength of a recording in which their scenarios had
merely errored. `--check` caught it that time. It is not guaranteed to: a
demotion from tier 2 to tier 1 that stays above the floor passes `--check`
and is still a check scoring a claim it no longer reads.

Compares the swept file against the committed one, per scenario id.
Exits 1 naming every demotion. Exits 0 on promotions and additions, which are
what a repaired corpus is expected to produce.
"""
import re, sys

def tiers(text):
    out, cur = {}, None
    for line in text.splitlines():
        m = re.match(r'\s*(?:\[\[scenario\]\]|\[\[check\]\])', line)
        if m:
            cur = None
            continue
        m = re.match(r'\s*(?:id|name|scenario|file)\s*=\s*"([^"]+)"', line)
        if m and cur is None:
            cur = m.group(1)
            continue
        m = re.match(r'\s*tier\s*=\s*(-?\d+)', line)
        if m and cur is not None:
            out[cur] = int(m.group(1))
            cur = None
    return out

def scalar(text, key):
    m = re.search(rf'^\s*{key}\s*=\s*(-?\d+)', text, re.M)
    return int(m.group(1)) if m else None

old = open(sys.argv[1], encoding="utf-8").read()
new = open(sys.argv[2], encoding="utf-8").read()
o, n = tiers(old), tiers(new)

bad = []
for k, ov in o.items():
    nv = n.get(k)
    if nv is not None and nv < ov:
        bad.append(f"  DEMOTED {k}: tier {ov} -> {nv}")
    elif nv is None:
        bad.append(f"  VANISHED {k}: tier {ov} -> absent")

for key in ("dead_fields",):
    ov, nv = scalar(old, key), scalar(new, key)
    if ov is not None and nv is not None and nv != ov:
        bad.append(f"  {key} moved {ov} -> {nv} (ratcheted; a sweep must not change it silently)")

promoted = sum(1 for k, v in n.items() if k in o and v > o[k])
added = sum(1 for k in n if k not in o)
print(f"sweep-guard: {len(o)} scenarios before, {len(n)} after; "
      f"{added} added, {promoted} promoted, {len(bad)} demoted/vanished")
if bad:
    print("SWEEP-GUARD RED — this ledger demotes, and a demotion over a bad "
          "recording is how fifteen working checks get retired:")
    print("\n".join(bad))
    sys.exit(1)
print("SWEEP-GUARD GREEN — nothing demoted")
