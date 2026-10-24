#!/usr/bin/env python3
"""Prove `ci/command-tree-parity.py` can go red, and that it does not go red on everything.

    python3 ci/command-tree-parity-falsify.py    # ~0.2 s, no build, no jar, no box

# WHY THIS EXISTS

The parity gate holds a ratchet rather than demanding parity, which is the
right shape and also the shape that rots quietly. We declare 1 of the 10 roots
a non-operator is told about, and the gate prints that and exits 0. If any of
its three refusals stopped working, the output would be identical on the day
it broke and for every day after: the same line, the same exit code, and a
ratchet that has stopped ratcheting.

Nothing in the real recordings violates any of the three, and nothing should.
So each fault is injected here against synthetic recordings and the expected
refusal is asserted by name.

# THE GREEN CASES ARE THE ARGUMENT, NOT THE PADDING

Two pairs matter more than the reds.

**`we-gained` green against `we-lost` red.** They differ only in *direction*:
one recording declares a root the baseline did not have, the other is missing
one it did. A gate that refused on both would be a gate that refuses on any
change, which is indistinguishable from a working ratchet by exit code alone
and is not one -- it would refuse on exactly the commit that closes the gap it
exists to measure.

**`derived-disagrees` green against `recorded-disagrees` red.** Same
recordings, same disagreement, one bit of provenance apart. A derived baseline
is a prediction made without the corpus lock, and a wrong prediction must
report rather than fail five other lanes' runs; a recorded one is a
measurement, and disagreeing with it is a regression. If those two collapsed
into one behaviour, arming the gate offline would either be impossible or
would be a live grenade.
"""

import json, subprocess, sys, tempfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "command-tree-parity.py"
VAN10 = ["me", "help", "list", "msg", "tell", "w", "random", "teammsg", "tm", "trigger"]


def rec(path, trees):
    """A synthetic recording: one entry per scenario, each with its own tree."""
    path.write_text(json.dumps({"scenarios": [
        {"name": f"s{i}", "declared_commands": t} for i, t in enumerate(trees)]}))


def run(work, van, ours, baseline):
    (work / "ci" / "checks").mkdir(parents=True, exist_ok=True)
    (work / "ci" / "command-tree-parity.py").write_bytes(SCRIPT.read_bytes())
    (work / "ci" / "checks" / "command-tree-baseline.json").write_text(
        json.dumps(baseline, indent=2))
    rec(work / "van.json", van)
    rec(work / "ours.json", ours)
    p = subprocess.run([sys.executable, str(work / "ci" / "command-tree-parity.py"),
                        str(work / "van.json"), str(work / "ours.json")],
                       capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


CASES = [
    # name, vanilla trees, our trees, baseline, expect_rc, must appear in output
    ("baseline-holds", [VAN10], [["list"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     0, "ratchet holds"),
    ("we-gained", [VAN10], [["list", "me"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     0, "NEW: rustmc now declares me"),
    ("we-lost", [VAN10], [["list"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list", "me"]},
     1, "no longer declares me"),
    ("vanilla-moved", [VAN10 + ["seed"]], [["list"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     1, "vanilla's level-0 tree is not the baseline's"),
    ("never-saw-the-packet", [VAN10], [[]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     1, "carries no declared_commands"),
    ("tree-varies-mid-run", [VAN10, ["list"]], [["list"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     1, "different trees to different scenarios"),
    ("derived-disagrees", [VAN10 + ["seed"]], [["list"]],
     {"provenance": "derived", "vanilla": VAN10, "rustmc": ["list"]},
     0, "DISAGREES"),
    ("recorded-disagrees", [VAN10 + ["seed"]], [["list"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     1, "vanilla's level-0 tree is not the baseline's"),
    # The asymmetry the file exists to catch, named rather than left to
    # `vanilla-moved` to cover generically. Op-ing OUR players only puts
    # level-2 roots in our tree and none in vanilla's, and until 2026-09-06
    # that printed `ours only 3` and exited 0 — so the one state in which
    # every Step::Command in the corpus runs at a different permission level
    # on the two servers was a pass.
    ("we-are-opped-and-vanilla-is-not", [VAN10],
     [["list", "weather", "kill", "gamerule"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     1, "which a non-operator on vanilla is never told about"),
    # The mirror. An opped reference declares the level-2 roots too, and that
    # is a moved reference: the pin is what says the harness's permission set
    # has not changed underneath every number in the corpus.
    ("vanilla-is-opped-and-we-are-not", [VAN10 + ["weather", "kill", "gamerule"]],
     [["list"]],
     {"provenance": "recorded", "vanilla": VAN10, "rustmc": ["list"]},
     1, "vanilla's level-0 tree is not the baseline's"),
    # And a derived baseline still keeps the exit code, which is what let the
    # 1-against-6 disagreement print on every corpus run for weeks and pass.
    ("derived-hides-an-opped-subject", [VAN10],
     [["list", "weather"]],
     {"provenance": "derived", "vanilla": VAN10, "rustmc": ["list"]},
     0, "which a non-operator on vanilla is never told about"),
]


def main():
    bad = 0
    with tempfile.TemporaryDirectory() as d:
        for name, van, ours, base, want_rc, want_text in CASES:
            work = Path(d) / name
            rc, out = run(work, van, ours, base)
            ok = rc == want_rc and want_text in out
            if not ok:
                bad += 1
                print(f"FAIL {name}: rc={rc} (wanted {want_rc}), "
                      f"{'missing' if want_text not in out else 'has'} {want_text!r}")
                print("  " + out.strip().replace("\n", "\n  "))
            else:
                print(f"  ok  {name}: rc={rc}, said {want_text!r}")

    # The two discriminating pairs, restated as assertions so that a future
    # edit collapsing either of them fails here and not in a reader's head.
    pairs = [("we-gained", "we-lost"), ("derived-disagrees", "recorded-disagrees"),
             ("derived-hides-an-opped-subject", "we-are-opped-and-vanilla-is-not")]
    for a, b in pairs:
        ra = next(c[4] for c in CASES if c[0] == a)
        rb = next(c[4] for c in CASES if c[0] == b)
        if ra == rb:
            bad += 1
            print(f"FAIL {a} and {b} expect the same exit code; the pair proves "
                  "nothing and one of them is mis-specified")

    if bad:
        print(f"\ncommand-tree-parity-falsify: {bad} case(s) wrong — the gate is "
              "not doing what its header says")
        return 1
    print(f"\ncommand-tree-parity-falsify: {len(CASES)} cases, "
          "5 refusals reachable, 3 discriminating pairs held")
    return 0


if __name__ == "__main__":
    sys.exit(main())
