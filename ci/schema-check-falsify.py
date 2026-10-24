#!/usr/bin/env python3
"""Does each of `960`-`963` actually go red when the behaviour it names breaks?

`docs/AGENT-BRIEF.md`: *ask of any green result, what would have had to happen
for this to come back red? If you cannot answer, you have measured the
instrument.* A corpus check is a sequence of actions plus a declared
projection, and it is entirely possible to write one whose projection does not
carry the field the check is about — in which case it passes for ever and
nothing says so. `ci/checks/gameplay/730` is the worked example in this tree's
history: every connected wall side it produces is `low`, so a server using a
boolean connection model passes it.

So this script does not ask whether vanilla and rustmc agree. It asks whether
**the check can tell the difference at all**, by breaking exactly the
behaviour the check names and requiring the verdict to move.

# The method

Take any real recording as the *reference* — vanilla's or our own; this needs
no vanilla boot and no corpus lock. Build a *subject* from it in which one
property value has been rewritten to the other value the check discriminates
between, and nothing else has changed. Run the corpus's own comparator,
`oracle-runner compare`, over the pair.

    reference  intact.json
    subject    intact.json with `oak_log[axis=y]` rewritten to `axis=x`
    required   the scenario goes non-`match`, and every other scenario stays
               exactly where the control put it

Each flip is a **specific wrong server**, not noise:

    960  axis    y <-> x       a RotatedPillarBlock that hardcodes Axis.X
    961  facing  west <-> north  one shared rule for the whole `facing` schema
                               (getHorizontalDirection().getOpposite()), which
                               is right for the pumpkin and wrong for the anvil
    962  lit     false <-> true  a lamp with no falling edge — neighborChanged
                               lights it and nothing on the scheduled-tick
                               path ever clears it
    963  facing  down <-> up   the natural DirectionalBlock rule,
                               facing <- clickedFace, with no neighbour read

The pairs are written unordered because the direction depends on which server
made the recording, and this server is currently the wrong model for `962` and
`963` — see `pick_direction`. Against our own recording the flip for those two
runs *towards* vanilla's answer, and it then proves the narrower thing that is
still worth proving: that the scenario's projection carries the field at all.
`ci/schema-check-falsify.sh` is the other half, and pins the server itself.

# The two ways this could lie, and what stops each

**A mutation that hits nothing.** If the recording never contained
`oak_log[axis=y]`, rewriting it changes no bytes, the verdict does not move,
and the honest reading is "the check is dead" — but a careless one is "the
mutation was too subtle". So a mutation that matches zero observations is a
**refusal**, printed as such, and never a pass. This is the same rule
`oracle-runner`'s `vacuous` verdict mechanises.

**A comparator that reports everything as different.** If `compare` returned
non-`match` for unrelated scenarios too, a red here would mean nothing. So
every run is paired with a **control**: the identical recording compared
against an unmutated copy of itself, which must be `match` for every scenario
in the corpus. A mutation is only credited if the control is clean and the
delta is confined to the one scenario named.

# Usage

Runs on the box, after `ci/t2-gameplay.sh` has left a recording:

    ci/schema-check-falsify.py \\
        --recording oracle-work/gameplay/vanilla.json \\
        --scenarios ci/checks/gameplay

`--binary` defaults to `target/release/oracle-runner`; build it first. Exits 0
only if every mutation was caught and every control was clean.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# scenario name -> (block, property, the two values it discriminates between, why)
#
# The pair is unordered ON PURPOSE. This script is run against whichever
# recording is to hand, and the two servers do not agree about which value is
# there to be flipped: vanilla puts `end_rod[facing=down]` on the wire and
# this server puts `facing=up`, because it has no neighbour read. A fixed
# `down -> up` mutation would find nothing in one of those two recordings and
# refuse, which would look like the check being dead when it is only the
# direction being wrong. So the script takes whichever of the pair the
# recording actually holds and flips it to the other, and refuses only if
# NEITHER is present — which really does mean the rig produced nothing.
MUTATIONS = [
    ("a-log-clicked-on-the-top-and-the-side-lies-two-ways",
     "minecraft:oak_log", "axis", ("y", "x"),
     "a RotatedPillarBlock that hardcodes Axis.X — passes 030 for ever"),
    ("an-anvil-and-a-pumpkin-from-one-yaw-are-a-quarter-turn-apart",
     "minecraft:anvil", "facing", ("west", "north"),
     "one shared rule for the whole `facing` schema, "
     "getHorizontalDirection().getOpposite() — right for the pumpkin, "
     "wrong for the anvil"),
    ("a-redstone-lamp-goes-out-when-the-lever-goes-back",
     "minecraft:redstone_lamp", "lit", ("false", "true"),
     "a lamp with no falling edge — nothing on the scheduled-tick path. "
     "This server IS that lamp today (power.rs lamp_wants_a_tick), so here "
     "the flip runs the other way and proves only that the projection "
     "carries the lamp's `lit` at all"),
    ("an-end-rod-placed-on-an-end-rod-points-the-other-way",
     "minecraft:end_rod", "facing", ("down", "up"),
     "the natural DirectionalBlock rule, facing <- clickedFace, with no "
     "neighbour read — which is this server today"),
]


def pick_direction(rec, block, prop, pair):
    """Which of the pair is on the wire? Returns (from, to), or None."""
    present = set()
    for sc in rec.get("scenarios", []):
        for o in sc.get("obs", ()):
            k, f = o.get("kind"), o.get("fields", {})
            fields = [f] if k == "block_update" else (
                f.get("blocks", ()) if k == "section_blocks_update" else ())
            for b in fields:
                s = b.get("state") or ""
                if s.startswith(block + "["):
                    kv = dict(x.split("=", 1) for x in s[len(block) + 1:-1].split(","))
                    if kv.get(prop) in pair:
                        present.add(kv[prop])
    if len(present) == 2:
        # Both values already on the wire: the property moved on its own, so
        # flipping either still changes the trace. Flip the first.
        return pair[0], pair[1]
    if len(present) == 1:
        got = present.pop()
        return got, (pair[1] if got == pair[0] else pair[0])
    return None


def die(msg):
    print(f"schema-check-falsify: {msg}", file=sys.stderr)
    sys.exit(2)


def selftest():
    """Falsify `pick_direction`, which decides every flip below.

    It is the piece that turned a fixed `down -> up` into something that works
    against either server's recording, and a bug in it reads as "the check is
    dead" rather than as a bug — the most expensive kind here.
    """
    def rec(*states):
        return {"scenarios": [{"name": "s", "obs": [
            {"kind": "block_update", "fields": {"state": s}} for s in states]}]}

    ok = [0, 0]

    def check(label, cond):
        ok[cond is not True] += 1
        print(f"{'ok  ' if cond else 'FAIL'} {label}")

    B, P, PAIR = "minecraft:end_rod", "facing", ("down", "up")
    check("vanilla's value alone flips towards ours",
          pick_direction(rec(f"{B}[{P}=down]"), B, P, PAIR) == ("down", "up"))
    check("our value alone flips towards vanilla's",
          pick_direction(rec(f"{B}[{P}=up]"), B, P, PAIR) == ("up", "down"))
    check("neither present refuses",
          pick_direction(rec(f"{B}[{P}=north]"), B, P, PAIR) is None)
    check("an empty recording refuses",
          pick_direction(rec(), B, P, PAIR) is None)
    check("both present still yields a flip",
          pick_direction(rec(f"{B}[{P}=down]", f"{B}[{P}=up]"), B, P, PAIR)
          == ("down", "up"))
    check("another block with the same property is ignored",
          pick_direction(rec("minecraft:anvil[facing=up]"), B, P, PAIR) is None)
    # A multi-property state must still parse — end rods have one property but
    # anvils and lamps do not, and the splitter is shared.
    check("a multi-property state is parsed, not skipped",
          pick_direction(rec("minecraft:piston[extended=true,facing=north]"),
                         "minecraft:piston", "facing", ("north", "south"))
          == ("north", "south"))
    print(f"\nSCHEMA-CHECK-FALSIFY SELFTEST "
          f"{'GREEN' if not ok[1] else 'RED'} — {ok[1]} failed")
    return 1 if ok[1] else 0


def state_ids(blocks_json):
    """(block, prop, value) -> the state ids of that block holding that value.

    Built from the pinned jar's own report so a mutation names a state that
    really exists rather than an id arrived at by arithmetic on another one.
    """
    bj = json.load(open(blocks_json))
    out, by_props = {}, {}
    for name, v in bj.items():
        for st in v["states"]:
            props = st.get("properties", {})
            by_props[(name, tuple(sorted(props.items())))] = st["id"]
            for p, val in props.items():
                out.setdefault((name, p, val), set()).add(st["id"])
    return out, by_props


def rewrite(obs_fields, block, prop, frm, to, by_props, counter):
    """Rewrite one observation's state, if it is the one being broken."""
    s = obs_fields.get("state")
    if not s or not s.startswith(block + "["):
        return
    props = dict(kv.split("=", 1) for kv in s[len(block) + 1:-1].split(","))
    if props.get(prop) != frm:
        return
    props[prop] = to
    key = (block, tuple(sorted(props.items())))
    if key not in by_props:
        die(f"{block} has no state with {prop}={to} and the rest unchanged — "
            "the mutation is not a real block state")
    obs_fields["state"] = block + "[" + ",".join(
        f"{k}={v}" for k, v in sorted(props.items())) + "]"
    obs_fields["state_id"] = by_props[key]
    counter[0] += 1


def mutate(rec, block, prop, frm, to, by_props):
    """A copy of the recording with exactly one property value rewritten."""
    out = json.loads(json.dumps(rec))
    n = [0]
    for sc in out.get("scenarios", []):
        for o in sc.get("obs", ()):
            k, f = o.get("kind"), o.get("fields", {})
            if k == "block_update":
                rewrite(f, block, prop, frm, to, by_props, n)
            elif k == "section_blocks_update":
                for b in f.get("blocks", ()):
                    rewrite(b, block, prop, frm, to, by_props, n)
    return out, n[0]


def verdicts(binary, scenarios, ref, sub):
    """scenario name -> verdict, from the corpus's own comparator."""
    r = subprocess.run([binary, "compare", scenarios, "--reference", ref,
                        "--subject", sub], capture_output=True, text=True)
    if r.returncode not in (0, 1):
        die(f"oracle-runner compare exited {r.returncode}\n{r.stderr[:2000]}")
    # `cmd_compare` prints one row per scenario as `  {verdict:<8} {name}`,
    # verdict first — anchored on that rather than on a loose split, because
    # the continuation lines it prints underneath ("we never sent: ...") are
    # also indented and would otherwise be read as scenarios.
    out = {}
    for line in r.stdout.splitlines():
        m = re.match(r"^  (match|diverge|absent|vacuous|inert|error)\s+(\S+)\s*$", line)
        if m:
            out[m.group(2)] = m.group(1)
    if not out:
        die("could not read a single verdict out of `oracle-runner compare` — "
            f"its output format has moved:\n{r.stdout[:1500]}")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recording")
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--scenarios", default=os.path.join(ROOT, "ci", "checks", "gameplay"))
    ap.add_argument("--binary", default=os.path.join(ROOT, "target", "release", "oracle-runner"))
    ap.add_argument("--blocks", default=os.path.join(
        ROOT, "oracle-work", "vanilla-26.2", "generated", "reports", "blocks.json"))
    args = ap.parse_args()

    if args.selftest:
        return selftest()
    if not args.recording:
        return die("need --recording (or --selftest)")

    for p in (args.recording, args.binary, args.blocks):
        if not os.path.exists(p):
            die(f"{p} is absent")

    rec = json.load(open(args.recording))
    _, by_props = state_ids(args.blocks)
    tmp = tempfile.mkdtemp(prefix="schema-falsify-")
    fails = []

    # ---- the control, first. A recording against an unmutated copy of itself
    # must be `match` everywhere, or a red below says nothing about the check.
    ctrl = os.path.join(tmp, "control.json")
    shutil.copy(args.recording, ctrl)
    #
    # `vacuous` is ALLOWED here and `match` is not the only clean answer. A
    # scenario that produced no observations at all compares `vacuous` against
    # itself, which is a fact about the scenario and not about the comparison:
    # `random-ticks-turn-covered-grass-to-dirt` is `vacuous` in every selfcheck
    # recording, because `ci/t2-gameplay.sh` shuts the `randomTick` section for
    # every other check and the recording therefore holds nothing for it.
    # Requiring all-`match` here failed on a perfectly good recording the first
    # time this was run. What must never appear is `diverge` or `error` — a
    # recording that differs from ITSELF means the comparator is not
    # deterministic, and then nothing below can be attributed to a mutation.
    base = verdicts(args.binary, args.scenarios, args.recording, ctrl)
    bad = sorted(k for k, v in base.items() if v in ("diverge", "error", "absent"))
    vac = sorted(k for k, v in base.items() if v == "vacuous")
    print(f"control: {len(base)} scenarios compared, "
          f"{sum(1 for v in base.values() if v == 'match')} match, "
          f"{len(vac)} vacuous, {len(bad)} diverge/error/absent")
    if bad:
        print(f"FAIL control is not clean: {', '.join(bad[:8])}")
        print("      a recording compared against an unmutated copy of itself "
              "must never diverge; until it does not, no mutation below "
              "proves anything")
        return 1
    print()

    for name, block, prop, pair, why in MUTATIONS:
        if name not in base:
            print(f"FAIL {name}: not in the recording at all — the corpus run "
                  "predates this check, or the check did not load")
            fails.append(name)
            continue
        # The intact leg, and it is the one that stops this whole script from
        # being theatre. If the scenario produced NOTHING when the server was
        # untouched, then breaking the server cannot make it produce less, the
        # verdict will not move, and a green line here would mean the check is
        # dead rather than that the server is right.
        if base[name] == "vacuous":
            print(f"FAIL {name}: `vacuous` INTACT — the rig emitted no "
                  "observations before anything was broken, so nothing below "
                  "could have moved it. Fix the rig, not this script.")
            fails.append(name)
            continue
        direction = pick_direction(rec, block, prop, pair)
        if direction is None:
            print(f"REFUSE {name}: the recording holds no {block} with "
                  f"{prop} at either {pair[0]} or {pair[1]}, so no flip could "
                  "change anything and a green verdict would be the "
                  "instrument, not the check")
            fails.append(name)
            continue
        frm, to = direction
        mutated, hits = mutate(rec, block, prop, frm, to, by_props)
        if hits == 0:
            print(f"REFUSE {name}: {block}[{prop}={frm}] was found by the "
                  "direction probe but rewrote nothing — the two disagree, "
                  "which is a bug in this script, not a result")
            fails.append(name)
            continue
        path = os.path.join(tmp, name + ".json")
        json.dump(mutated, open(path, "w"))
        got = verdicts(args.binary, args.scenarios, args.recording, path)
        moved = sorted(k for k in got if got.get(k) != base.get(k))
        ok = got.get(name) != "match" and moved == [name]
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        print(f"       broke {block}[{prop}={frm}] -> {to} at {hits} "
              f"observation(s): {why}")
        print(f"       verdict {base[name]} -> {got.get(name)}; "
              f"scenarios moved: {len(moved)}")
        if not ok:
            fails.append(name)
            if moved != [name]:
                print(f"       COLLATERAL: {', '.join(m for m in moved if m != name)[:200]}")
                print("       a mutation confined to one scenario's rig must "
                      "move one verdict; more than one means the rigs are "
                      "coupled and the check is not isolated")

    shutil.rmtree(tmp, ignore_errors=True)
    print(f"\nSCHEMA-CHECK-FALSIFY {'GREEN' if not fails else 'RED'} — "
          f"{len(MUTATIONS) - len(fails)}/{len(MUTATIONS)} checks go red when "
          "the behaviour they name is broken")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
