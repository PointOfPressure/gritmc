#!/usr/bin/env python3
"""Compare the command tree two servers declare to the same player."""

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/command-tree-parity.py VANILLA.json RUSTMC.json [--write-baseline]
#
# Reads `declared_commands` out of two gameplay recordings and refuses if the
# gap between them got worse, or if vanilla's side moved at all.
#
# WHAT BREAKS IF YOU IGNORE THIS: the command registry goes back to being
# gated only by ci/commands-golden.sh, which encodes *our own* registration
# with vanilla's encoder and therefore cannot name a command vanilla has and
# we do not.
# ===== end interface; everything below is why =====

# # WHY THIS IS NOT A SCENARIO CHECK
#
# `ClientboundCommandsPacket` arrives during the join, and `record::record`
# logs both players in and settles them before the first scenario's measured
# window opens. So the packet can never appear in a scenario's `obs`, and a
# scenario that named `commands` in its `observe` list would record nothing
# and pass by being empty. `Session` captures it in `liveness`, which every
# drain routes through, and reports it per scenario the way `anchor` and
# `spawn` are reported.
#
# That makes it one fact repeated 108 times rather than 108 checks, which is
# why the comparison lives here and runs once instead of becoming a per
# scenario verdict that would be the same verdict 108 times over.
#
# # THE FOUR WAYS THIS REFUSES, AND WHY EACH IS NEEDED
#
# **Empty.** A recording whose `declared_commands` is missing or empty is a
# recording taken by a runner that could not see the packet. Passing on it
# would be the exact failure this file exists to end, so it is a refusal and
# not a skip.
#
# **Vanilla moved.** The reference side is pinned. If vanilla's level-0 tree
# is not what the baseline says, either the jar changed or the harness's
# player is no longer the source we think it is — and in the second case every
# number below is about a different question than the one it claims. Read it
# before re-baselining.
#
# **We went backwards.** A root we used to declare and no longer do.
#
# **We declare something vanilla does not.** Added 2026-09-06; it was printed
# and unscored before, which left the one asymmetry this file exists to catch
# exiting 0. A root a non-operator on vanilla is never told about means our
# players are operators and vanilla's are not, or our own `fillUsableCommands`
# is wrong. In the first case every `Step::Command` in the corpus runs at a
# different permission level on the two servers.
#
# It does **not** refuse for the gap itself. We declare **six** of the ten
# roots a non-operator sees on vanilla — `help list me msg tell w`, missing
# `random teammsg tm trigger` — and a gate that fails until that is ten is a
# gate that says the same thing every day and is read by nobody. The gap is
# printed, the ratchet holds it, and closing it moves a number.
#
# That six was **one** in this comment and in the baseline until 2026-09-06,
# because the baseline shipped `provenance: "derived"` — a prediction from
# `ci/command-surface-dump.sh` that no run ever confirmed. A derived baseline
# keeps the exit code at 0 by design, so every corpus run since has printed
# `this run DISAGREES with it` and passed. It is `recorded` now, cross-checked
# on three independent corpus pairs a week apart (`rustmc-combat`,
# `rustmc-misc2`, `rustmc-stack`, all 10 against 6), so the ratchet bites.
#
# # WHY THE COMPARISON IS AGAINST TEN AND NOT NINETY
#
# A dedicated vanilla server registers 90 roots; `ci/command-surface.py`
# generates that list. `Commands.sendCommands` runs `fillUsableCommands`
# first, so what any player is *told* is the part their permission set may
# use, and the corpus's players are not operators. Comparing our tree against
# all 90 would be comparing it against a tree no scenario could ever receive.

import json, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "ci" / "checks" / "command-tree-baseline.json"
DUMP = ROOT / "data" / "vanilla" / "command_surface.json"
SURFACE = ROOT / "docs" / "generated" / "command-surface.tsv"


def declared(path, label):
    """The roots one recording says it was told, or a refusal."""
    trace = json.loads(Path(path).read_text())
    seen = {}
    for s in trace.get("scenarios", []):
        seen.setdefault(tuple(s.get("declared_commands") or []), []).append(s["name"])
    seen.pop((), None)
    if not seen:
        sys.exit(f"REFUSED — {label} ({path}) carries no declared_commands. Either the "
                 "recording predates the field or the runner never saw the packet; "
                 "either way this comparison would be about nothing.")
    if len(seen) > 1:
        shapes = {len(k): v[0] for k, v in seen.items()}
        sys.exit(f"REFUSED — {label} declares different trees to different scenarios "
                 f"{shapes}. The tree is a join-time fact and must not vary; a server "
                 "that re-sends a changed tree mid-run makes every row below ambiguous.")
    return list(next(iter(seen)))


def derive():
    """A baseline predicted from the offline instruments, not from a recording.

    The corpus lock was six deep when this gate was written, and wiring in a
    ratchet with no baseline would have refused for every other lane on the
    box. So the baseline can be *derived*: vanilla's side from the executed
    dedicated dispatcher `ci/command-surface-dump.sh` builds, filtered through
    `fillUsableCommands` for a `PermissionLevel.ALL` source, and our side from
    the roots the generated surface says we register at that level.

    This is a **prediction**, and it is written down as one. Three instruments
    are involved and none of them is a recording: a real vanilla dispatcher, a
    static reading of the jar, and our own tree. The first run against real
    recordings either confirms all three at once, which is worth more than a
    baseline copied out of a recording could ever be, or it disagrees and has
    found something.

    Until that run, `provenance` is "derived" and a disagreement is reported
    loudly rather than failing the gate: a prediction that turns out wrong is
    a finding for whoever made it, not grounds for breaking five other lanes'
    corpus runs. Re-baselining from a real recording flips it to "recorded"
    and every arm becomes fatal.
    """
    if not DUMP.exists():
        sys.exit(f"REFUSED — no {DUMP.relative_to(ROOT)}; run ci/command-surface-dump.sh")
    van = list(json.loads(DUMP.read_text())["usableAtAll"])
    if not SURFACE.exists():
        sys.exit(f"REFUSED — no {SURFACE.relative_to(ROOT)}; run ci/command-surface.py --write")
    ours = []
    for line in SURFACE.read_text().splitlines():
        if line.startswith("#") or line.startswith("root\t"):
            continue
        f = line.split("\t")
        # Ported *and* reachable by a level-0 source: `seed` is ours and is
        # not in this list, because a non-operator is never told about it.
        if f[4] == "yes" and f[2] == "level0":
            ours.append(f[0])
    ours = [c for c in van if c in ours]
    return van, ours


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    write = "--write-baseline" in sys.argv
    if "--derive-baseline" in sys.argv:
        van, ours = derive()
        BASELINE.parent.mkdir(parents=True, exist_ok=True)
        BASELINE.write_text(json.dumps(
            {"provenance": "derived",
             "note": "predicted from ci/command-surface-dump.sh and the generated "
                     "surface, not from a recording. The first corpus run either "
                     "confirms three instruments at once or has found something; "
                     "re-run with --write-baseline to make it recorded and fatal.",
             "vanilla": van, "rustmc": ours}, indent=2) + "\n")
        print(f"wrote {BASELINE.relative_to(ROOT)} (derived): "
              f"vanilla {len(van)}, rustmc {len(ours)}")
        return 0
    if len(args) != 2:
        sys.exit(__doc__.strip() + "\n\n  ci/command-tree-parity.py VANILLA.json RUSTMC.json")
    van = declared(args[0], "vanilla")
    ours = declared(args[1], "rustmc")

    if write:
        BASELINE.parent.mkdir(parents=True, exist_ok=True)
        BASELINE.write_text(json.dumps(
            {"provenance": "recorded", "vanilla": van, "rustmc": ours}, indent=2) + "\n")
        print(f"wrote {BASELINE.relative_to(ROOT)}: vanilla {len(van)}, rustmc {len(ours)}")
        return 0

    if not BASELINE.exists():
        sys.exit(f"REFUSED — no {BASELINE.relative_to(ROOT)}; run with --write-baseline "
                 "and read what it wrote before committing it.")
    base = json.loads(BASELINE.read_text())

    print(f"vanilla declares {len(van)}: {' '.join(van)}")
    print(f"rustmc  declares {len(ours)}: {' '.join(ours)}")
    missing = [c for c in van if c not in ours]
    extra = [c for c in ours if c not in van]
    print(f"  missing {len(missing)}: {' '.join(missing) or '(none)'}")
    if extra:
        print(f"  ours only {len(extra)}: {' '.join(extra)}")

    # A derived baseline is a prediction. Report a disagreement in full and
    # keep the exit code, because a wrong prediction is a finding for whoever
    # wrote it and not grounds for failing every other lane's corpus run.
    predicted = base.get("provenance") == "derived"
    rc = 0
    if van != base["vanilla"]:
        print("REFUSED — vanilla's level-0 tree is not the baseline's.\n"
              f"  baseline: {' '.join(base['vanilla'])}\n"
              f"  recorded: {' '.join(van)}\n"
              "  The reference side is pinned. Find out which of the jar and the "
              "harness's permission set moved before re-baselining.", file=sys.stderr)
        rc = 0 if predicted else 1
    # **A root we declare that vanilla does not is a REFUSAL, not a gain.**
    #
    # This was printed and not scored until 2026-09-06, and what it hid is the
    # asymmetry the whole file is about. `Commands.sendCommands` sends
    # `fillUsableCommands`, so the tree a player is told about *is* their
    # permission set made visible. A root here that a non-operator on vanilla
    # is never told about means one of two things and both are substrate
    # differences reported as agreement:
    #
    #   * our players are operators and vanilla's are not, so every
    #     `Step::Command` in the corpus runs at a different level on the two
    #     servers — a level-2 command refused on one side and executed on the
    #     other, compared as though the two servers disagreed about the
    #     mechanic;
    #   * or our permission filter is wrong, and we are telling a
    #     non-operator about commands they may not run.
    #
    # `docs/AGENT-BRIEF.md`'s rule is that both sides are opped identically or
    # neither, and that the choice is visible in the recording rather than
    # assumed. `declared_commands` is where it is visible; this is what makes
    # it fail.
    if extra:
        print(f"REFUSED — rustmc declares {' '.join(extra)}, which a non-operator "
              "on vanilla is never told about.\n"
              "  Either our players are operators and vanilla's are not — in which "
              "case every Step::Command in the corpus runs at a different "
              "permission level on the two servers — or fillUsableCommands is "
              "being applied wrongly on our side.", file=sys.stderr)
        rc = 0 if predicted else 1
    lost = [c for c in base["rustmc"] if c not in ours]
    if lost:
        print(f"REFUSED — rustmc no longer declares {' '.join(lost)}.", file=sys.stderr)
        rc = 0 if predicted else 1
    gained = [c for c in ours if c not in base["rustmc"]]
    if gained:
        print(f"  NEW: rustmc now declares {' '.join(gained)} — "
              "re-run with --write-baseline to ratchet.")
    if predicted:
        agree = van == base["vanilla"] and ours == base["rustmc"]
        print("  baseline is DERIVED, not recorded: "
              + ("this run confirms it against real recordings — re-run with "
                 "--write-baseline to make the ratchet fatal"
                 if agree else
                 "and this run DISAGREES with it. Read the rows above; the "
                 "prediction or one of the three instruments is wrong."))
    elif rc == 0:
        print(f"  ratchet holds: {len(ours)}/{len(van)} of the roots a "
              "non-operator is told about")
    return rc


if __name__ == "__main__":
    sys.exit(main())
