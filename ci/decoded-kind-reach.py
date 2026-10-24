#!/usr/bin/env python3
"""Which decoder arms any check can reach, and which are dead in practice."""

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/decoded-kind-reach.py RECORDING.json [more.json ...]
#     ci/decoded-kind-reach.py RECORDING.json --write-baseline
#
# Reports every kind in `trace::DECODED_KINDS` that appears in no scenario of
# the given recordings, and refuses if that set has grown.
#
# WHAT BREAKS IF YOU IGNORE THIS: you write a decoder arm, it is never
# exercised, and the checks that name it compare nothing on that channel while
# reading as covered.
# ===== end interface; everything below is why =====

# # THE MEASUREMENT THAT WAS MISSING
#
# `DECODED_KINDS` is a list of packets somebody wrote a decoder for. It is
# routinely read as a coverage figure -- 37 kinds decoded -- and it is not one.
# A decoder arm is only an instrument if a packet of that kind reaches a
# *measured window*, and a surprising number never do.
#
# Measured against a real 148-scenario vanilla recording, **8 of 37 appear in
# no scenario at all**. Two distinct reasons, and they are not the same finding:
#
#   * **drained.** The packet arrives during the join, and `record::record` logs
#     both players in and settles them before the first window opens. `commands`
#     is the worked example -- `ClientboundCommandsPacket` reaches every player
#     on join and reached no recording ever, which is why the command registry
#     went ungated for the project's whole life. `set_default_spawn_position`,
#     `player_position`, `game_event`, `set_health` and `set_experience` are the
#     same shape.
#   * **substrate.** The packet exists but nothing in this corpus provokes it.
#     `block_destruction` is survival mining progress and the corpus is creative
#     by construction, so no implementation of it could change a recording.
#
# The first is fixable in the harness -- `Session::declared_commands` is how
# `commands` was fixed, by capturing in `liveness` and reporting the fact per
# scenario -- and the second is fixable only by a scenario that provokes the
# packet. Telling them apart matters because only one of them is a porting job.
#
# # THE LOUDEST ROW IS ONE SOMEBODY ELSE ALREADY FOUND
#
# `damage_event` and `set_health` are declared in the `observe` list of two
# checks and appear in no scenario of the recording. A kind named by a check
# and absent from the reference is a check comparing nothing on that channel,
# and it reads as covered from every direction: the arm exists, the scenario
# names it, the verdict is `match`.
#
# **That row is already documented** -- the `combat` row of PARITY-ROADMAP 4,
# 52's audit item 2, and the Tier 4 row of the coverage audit, which has both
# a better denominator (0 times in 17 recordings, against the one this was
# first run on) and the explanation: a bare-stick attack deals no damage, so
# there is no damage packet to compare, and creative players are invulnerable.
# 52.7 lists what would settle it.
#
# This script is therefore not how that was found. It is how it stays found.
# The fact lived as prose in three documents, and a coverage fact written in
# three places is three places for it to go stale -- which is the argument the
# rest of PARITY-ROADMAP 82 makes about command tables, applied to this one.
#
# # WHY IT RATCHETS INSTEAD OF DEMANDING ZERO
#
# Eight dead arms is the state, and a gate that fails until it is zero says the
# same thing every day. It refuses when the set *grows*: a new decoder arm that
# nothing reaches, or an arm that used to fire and stopped. Both are worth an
# interruption; the standing eight are worth a worklist.

import json, re, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TRACE = ROOT / "tools" / "oracle-runner" / "src" / "trace.rs"
BASELINE = ROOT / "ci" / "checks" / "decoded-kind-reach.json"
SCEN = ROOT / "ci" / "checks" / "gameplay"


def decoded_kinds():
    m = re.search(r"pub const DECODED_KINDS: &\[&str\] = &\[(.*?)\];",
                  TRACE.read_text(), re.S)
    if not m:
        sys.exit("REFUSED — cannot find DECODED_KINDS in trace.rs")
    return set(re.findall(r'"([^"]+)"', m.group(1)))


def observed_kinds():
    """Kinds named in some scenario's `observe` list."""
    out = {}
    for p in sorted(SCEN.glob("*.toml")):
        for m in re.finditer(r"^observe = \[(.*?)\]", p.read_text(), re.M | re.S):
            for k in re.findall(r'"([^"]+)"', m.group(1)):
                out.setdefault(k, []).append(p.stem)
    return out


def present(paths):
    """Kinds appearing in at least one scenario of at least one recording."""
    counts, scenarios = {}, 0
    for path in paths:
        d = json.loads(Path(path).read_text())
        ss = d.get("scenarios") or []
        if not ss:
            sys.exit(f"REFUSED — {path} has no scenarios; a recording with nothing "
                     "in it makes every kind below look dead")
        scenarios += len(ss)
        for s in ss:
            for o in s.get("obs", []):
                counts[o["kind"]] = counts.get(o["kind"], 0) + 1
    if not counts:
        sys.exit("REFUSED — no observations in any recording given")
    return counts, scenarios


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if not args:
        sys.exit(__doc__.strip() +
                 "\n\n  ci/decoded-kind-reach.py RECORDING.json [more.json ...]")
    kinds = decoded_kinds()
    counts, n = present(args)
    dead = sorted(kinds - set(counts))
    declared = observed_kinds()

    print(f"{len(kinds)} decoded kinds against {n} scenarios in "
          f"{len(args)} recording(s)")
    print(f"  reached  {len(kinds) - len(dead)}")
    print(f"  dead     {len(dead)}: {' '.join(dead) or '(none)'}")
    worst = [k for k in dead if k in declared]
    if worst:
        print("  and NAMED BY A CHECK while absent from the reference, so that "
              "check compares nothing on that channel:")
        for k in worst:
            print(f"    {k}: {', '.join(declared[k])}")

    if "--write-baseline" in sys.argv:
        BASELINE.parent.mkdir(parents=True, exist_ok=True)
        BASELINE.write_text(json.dumps(
            {"note": "kinds in DECODED_KINDS that appear in no scenario of the "
                     "reference recording. Shrinking is progress; growing is a "
                     "decoder arm nothing reaches.",
             "dead": dead, "declared_but_dead": worst}, indent=2) + "\n")
        print(f"wrote {BASELINE.relative_to(ROOT)}")
        return 0

    if not BASELINE.exists():
        sys.exit(f"REFUSED — no {BASELINE.relative_to(ROOT)}; run with "
                 "--write-baseline and read what it wrote")
    base = json.loads(BASELINE.read_text())
    grew = [k for k in dead if k not in base["dead"]]
    shrank = [k for k in base["dead"] if k not in dead]
    if shrank:
        print(f"  NOW REACHED: {' '.join(shrank)} — re-run with "
              "--write-baseline to ratchet")
    if grew:
        print(f"REFUSED — these decoded kinds stopped being reached: "
              f"{' '.join(grew)}. Either a new arm was added that nothing "
              "provokes, or a scenario that used to provoke one no longer does.",
              file=sys.stderr)
        return 1
    print(f"  ratchet holds at {len(dead)} dead")
    return 0


if __name__ == "__main__":
    sys.exit(main())
