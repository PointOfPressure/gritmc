#!/usr/bin/env python3
"""check-use-probe.py — refuse a use table whose rows are readings of the probe.

Called by `ci/use-golden.sh` **before** it writes `use_without_item.json`, and
by `ci/use-item-on-golden.sh` before it writes `use_item_on.json`, so a block
that reads the probe's own player cannot enter either table quietly. See
`ci/checks/use-probe-dependent.toml` and
`ci/checks/use-item-on-probe-dependent.toml` for what an allowlist is and why
carrying an entry is worse than porting the block.

    ./ci/check-use-probe.py <table.json> [allowlist.toml]

**One checker, two tables, and that is a property of the tables rather than a
generalisation of the checker.** `UseItemOnGolden` records `probeDependent`,
`probeAxes`, `probeUnknown`, `probeStances` and `probeStancesUnavailable` with
the same names and the same meanings as `UseGolden`, because it sweeps the same
`UseGolden.STANCES` over the same `UseGolden.HarnessPlayer`. Nothing below
mentions `useWithoutItem`; the second table needed a second allowlist and no
second checker. The one thing that had to change is that the messages name the
allowlist they came from — with two of them, "the file says" is ambiguous
exactly when it matters.

Separate from the generators for one reason: a gate that cannot fail is not a
gate, and inline in a generator this one could only be falsified by a JVM run
per injection. `ci/check-use-probe-falsify.sh` runs its injections against real
tables in under a second.
"""
import json
import os
import sys
import tomllib

doc = json.load(open(sys.argv[1]))
allowlist = (sys.argv[2] if len(sys.argv) > 2 else
    os.path.join(os.path.dirname(os.path.abspath(__file__)),
                 "checks/use-probe-dependent.toml"))
known = tomllib.load(open(allowlist, "rb"))
# Repo-relative when it is inside the repo, absolute otherwise. A `..`-laden
# relative path to a fixture in /tmp is harder to act on than the path itself.
_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
listed = (os.path.relpath(allowlist, _root)
          if os.path.abspath(allowlist).startswith(_root + os.sep) else allowlist)
meta = known.get("meta", {})
by_id = {b["id"]: b for b in known.get("block", [])}
bad = []

# The sweep must actually have swept. A stance list that shrank, or a stance
# that became unavailable, would make every row look constant — which is the
# failure this check exists to catch, wearing the check's own uniform.
seen_stances = len(doc.get("probeStances", [])) + 1
if seen_stances != meta.get("stances"):
    bad.append("the dumper swept %d stances, %s pins %s. A sweep that stops "
               "sweeping reports every row as constant."
               % (seen_stances, listed, meta.get("stances")))
gone = sorted(doc.get("probeStancesUnavailable", {}))
if gone != sorted(meta.get("unavailable", [])):
    bad.append("stances unavailable in this run: %s; the file expects %s. "
               "An axis that silently stopped being turned is an axis reported "
               "clean on no evidence." % (gone or "none", meta.get("unavailable")))

for row in doc["table"]:
    dep = bool(row.get("probeDependent"))
    unk = list(row.get("probeUnknown", []))
    ent = by_id.get(row["block"])
    if not dep and not unk:
        continue
    if ent is None:
        bad.append("NEW  %s (%s) is probe-dependent on %s%s and is not in "
                   "%s. Its recorded row is a fact about one player. Port it by "
                   "hand from its bytecode, or add it there with the axes named."
                   % (row["block"], row["class"], row.get("probeAxes"),
                      ", unread under %s" % unk if unk else "", listed))
        continue
    if list(ent.get("axes", [])) != list(row.get("probeAxes", [])):
        bad.append("MOVED %s: axes %s, the file says %s"
                   % (row["block"], row.get("probeAxes"), ent.get("axes")))
    if list(ent.get("unknown", [])) != unk:
        bad.append("MOVED %s: unknown %s, the file says %s"
                   % (row["block"], unk, ent.get("unknown")))

live = {r["block"] for r in doc["table"]
        if r.get("probeDependent") or r.get("probeUnknown")}
for stale in sorted(set(by_id) - live):
    bad.append("STALE %s is listed as probe-dependent and no longer is. Delete "
               "its entry — a carried allowance nobody re-checks is how a list "
               "of known defects becomes a list of things that used to be true."
               % stale)

if bad:
    print("check-use-probe: refusing to write %s."
          % doc.get("what", sys.argv[1]), file=sys.stderr)
    for b in bad:
        print("  " + b, file=sys.stderr)
    sys.exit(1)
n = sum(1 for r in doc["table"] if r.get("probeDependent"))
print("  probe sweep:  %d of %d rows move with the player, all %d allowed by %s"
      % (n, len(doc["table"]), len(by_id), listed))
