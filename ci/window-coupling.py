#!/usr/bin/env python3
"""Measure how much of a corpus verdict is a function of the *session's*
container-id counter, rather than of the scenario under test.

    ci/window-coupling.py --scenarios ci/checks/gameplay \\
        --trace oracle-work/gameplay/vanilla.json \\
        --runner target/release/oracle-runner [--json out.json]

# The question this exists to answer, and why reading the code cannot

`ServerPlayer.nextContainerCounter` is `containerCounter % 100 + 1` on a field
that lives as long as the player, and one actor plays every scenario in a
corpus run.  So the raw window id in the Nth menu check is a statement about
the N-1 menus before it, and for a long time the corpus documented that as a
hard ordering dependency: `ci/checks/gameplay/README.md`, `docs/SERVER.md`
§"Two things a new check inherits", `crates/rustmc-server/src/menu.rs` and two
scenario files all said the menu checks were "one indivisible item in corpus
order".  `compare::rebase_session_counters` was later written to rebase the
counter onto the scenario, and those notes were amended by hand.

Every one of those statements — the claim *and* its retraction — is a reading
of source.  ORACLE rule 9: measurements move suspects, readings of code do not.
This is the measurement.

# The instrument

A trace is compared against **a mutated copy of itself**.  Two identical traces
agree everywhere by construction, so the baseline is silent and every verdict
that moves is caused by the mutation and by nothing else — no branch delta, no
run-to-run flap, no vanilla-against-vanilla floor to subtract.  That is the
whole reason this takes one trace rather than a pair.

`--mutation shift:K` adds K to every allocated window id on a connection, which
is exactly what inserting K menu-opening scenarios ahead of the corpus does to
the wire.  The count of checks whose declared verdict leaves `match` under it
**is** the number of checks the counter couples.

# The mutations that are NOT the measurement, and why they ship with it

Rule 7: when a measurement clears every candidate, suspect the measurement.  A
projection that ignored the window field entirely would report zero coupling
too, and would be a defect rather than a fix — this project's recurring one.
So the same instrument injects genuine container-id defects and reports whether
the corpus still catches them.  A `shift` that moves nothing is only good news
beside a `zero` and a `desync` that move something.

  shift:K   K extra menus opened before the run.  Pure session history.
            THIS IS THE COUPLING MEASUREMENT.  Anything it moves is coupled.
  freeze    the server allocates the same id every time and never increments.
  zero      the server answers on window 0 — the player's own inventory, the
            one id `next_container_counter` must never return.
  desync    `container_set_slot`/`container_set_content` name a menu one id
            away from the `open_screen` that opened it, so the container
            packets belong to a menu that was never opened.
  drop-open the `open_screen` frame is never sent.

`freeze` is the sharp one and it is expected to move **nothing** on a corpus
whose scenarios open at most one menu each: with one menu per scenario the
rebase maps that menu to 1 whatever its raw id was, so "always allocates 1" and
"allocates correctly" are the same wire under the projection.  That is not a
bug in the rebase — no single-menu scenario can distinguish them, and a lens
cannot invent evidence — but it *is* a hole in the corpus, and the way to close
it is a scenario that opens two menus, where the ids 1 and 2 are a within-
scenario fact that carries no session history at all.
"""

import argparse
import copy
import json
import os
import subprocess
import sys
import tempfile

# `open_screen`, `container_set_slot` and `container_set_content` are the three
# decoder arms that write a `window` field today (`trace.rs`).  Matched on the
# field rather than on this list wherever a mutation does not care which kind
# it is, for the reason `rebase_session_counters` gives for the same choice.
OPEN = "open_screen"


def windowed(obs):
    """Every observation carrying an allocated (non-zero) window id.

    Window 0 is the player's own `InventoryMenu` — a name, not a draw — and a
    session-history shift cannot move it, so a mutation that moved it would be
    modelling something that does not happen.
    """
    f = obs.get("fields")
    if not isinstance(f, dict):
        return False
    w = f.get("window")
    return isinstance(w, int) and w != 0


def mutate(trace, kind, who):
    out = copy.deepcopy(trace)
    touched = 0
    for sc in out.get("scenarios", []):
        keep = []
        for o in sc.get("obs", []):
            if o.get("who") not in who:
                keep.append(o)
                continue
            if kind == "drop-open" and o.get("kind") == OPEN:
                touched += 1
                continue
            if windowed(o):
                w = o["fields"]["window"]
                if kind.startswith("shift:"):
                    o["fields"]["window"] = w + int(kind.split(":", 1)[1])
                    touched += 1
                elif kind == "freeze":
                    o["fields"]["window"] = 1
                    touched += 1
                elif kind == "zero":
                    o["fields"]["window"] = 0
                    touched += 1
                elif kind == "desync" and o.get("kind") != OPEN:
                    o["fields"]["window"] = w + 1
                    touched += 1
            keep.append(o)
        sc["obs"] = keep
    return out, touched


def verdicts(runner, scenarios, ref, subj, workdir):
    """`oracle-runner compare`, read out of its own --json rather than parsed
    off stdout."""
    out = os.path.join(workdir, "cmp.json")
    p = subprocess.run(
        [runner, "compare", scenarios, "--reference", ref, "--subject", subj, "--json", out],
        capture_output=True,
        text=True,
    )
    if not os.path.exists(out):
        sys.stderr.write(p.stdout[-4000:] + p.stderr[-4000:])
        raise SystemExit("compare produced no --json; see above")
    doc = json.load(open(out))
    return {o["name"]: (o["verdict"], o["strict"]) for o in doc["outcomes"]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scenarios", required=True)
    ap.add_argument("--trace", required=True)
    ap.add_argument("--runner", default="target/release/oracle-runner")
    ap.add_argument(
        "--who",
        default="actor,observer",
        help="connections to mutate; each has its own counter",
    )
    ap.add_argument(
        "--mutations",
        default="shift:1,shift:2,shift:7,freeze,zero,desync,drop-open",
    )
    ap.add_argument("--json")
    a = ap.parse_args()

    who = set(a.who.split(","))
    trace = json.load(open(a.trace))

    carriers = sorted(
        {
            sc["name"]
            for sc in trace.get("scenarios", [])
            for o in sc.get("obs", [])
            if o.get("who") in who and windowed(o)
        }
    )

    with tempfile.TemporaryDirectory() as td:
        base_path = os.path.join(td, "base.json")
        json.dump(trace, open(base_path, "w"))
        base = verdicts(a.runner, a.scenarios, base_path, base_path, td)

        print(f"trace                 {a.trace}")
        print(f"scenarios compared    {len(base)}")
        print(f"carry an allocated window id  {len(carriers)}")
        print()
        print("  the baseline is a trace against ITSELF, so every verdict that")
        print("  moves below was caused by the mutation and by nothing else.")
        print()

        report = {"trace": a.trace, "carriers": carriers, "mutations": {}}
        for m in a.mutations.split(","):
            mt, touched = mutate(trace, m, who)
            path = os.path.join(td, f"{m.replace(':', '_')}.json")
            json.dump(mt, open(path, "w"))
            got = verdicts(a.runner, a.scenarios, base_path, path, td)
            moved = sorted(
                n
                for n in base
                if base[n][0] != got.get(n, ("gone", "gone"))[0]
                and base[n][0] == "match"
            )
            other = sorted(
                n for n in base if base[n][0] != got.get(n, ("gone", "gone"))[0]
            )
            strict_moved = sorted(
                n for n in base if base[n][1] != got.get(n, ("g", "g"))[1]
            )
            report["mutations"][m] = {
                "observations_touched": touched,
                "declared_moved": moved,
                "declared_moved_any": other,
                "strict_moved": strict_moved,
            }
            tag = "COUPLING" if m.startswith("shift:") else "control "
            print(
                f"{tag} {m:<10} touched {touched:>4} obs  ->  "
                f"declared moved {len(other):>3}   strict moved {len(strict_moved):>3}"
            )
            for n in other[:12]:
                print(f"             {base[n][0]} -> {got.get(n, ('gone',))[0]}  {n}")
            if len(other) > 12:
                print(f"             ... and {len(other) - 12} more")

    if a.json:
        json.dump(report, open(a.json, "w"), indent=2)
        print(f"\nwrote {a.json}")


if __name__ == "__main__":
    main()
