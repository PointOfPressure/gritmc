#!/usr/bin/env python3
"""sound-emission-gap.py — which sound events vanilla emits and this server cannot.

    ./ci/sound-emission-gap.py
    ./ci/sound-emission-gap.py --recordings '~/rustmc*/oracle-work/gameplay/vanilla.json'
    ./ci/sound-emission-gap.py --check          # exit 1 if the gap has grown

# The denominator problem this solves

`ci/figures.toml` carries a sound-coverage ceiling of 256 of 1,968 and says, in
its own reason field, that it is "THE ONE CEILING IN THIS SECTION THAT IS NOT
DERIVED FROM THE JAR" — it counts what *this server* can emit, which caps what a
scenario can adjudicate rather than what vanilla can do. The complementary
number, and the one that says where to work, is the other direction: of the
events **vanilla actually sent while the corpus ran**, how many can this server
not produce at all?

That set is a measurement, not a reading. Two thirds of the 1,968 are
unreachable from a creative superflat with two players — a gap report counting
`entity.ender_dragon.death` is arithmetic. What vanilla emitted across every
recording on the box is the reachable set, sampled, and every event in it is
something a scenario could adjudicate today if this server could say it.

# How each side is derived, and where each is blind

* **vanilla's side** is every `sound` observation in every
  `oracle-work/gameplay/vanilla.json` on the box, by numeric event id, named
  through `generated/reports/registries.json`. It is blind to any event no
  scenario provokes, which is the point: those are not gaps, they are
  out of scope.

* **our side** is two sets unioned, and the second is a heuristic with a
  stated blind spot:

  1. every `minecraft:…` string literal anywhere under `crates/` or `tools/`;
  2. every event ending `.place` or `.step`, because `crate::sound_types`
     drives those two columns from the table and no source line names them.

  **The blind spot is (2), and it over-credits.** `entity.pig.step` ends in
  `.step` and is *not* a block sound type — a pig's step comes from the mob,
  not from the block it walks on — so this script counts it as covered when
  nothing here emits it. That is the wrong direction for a gap report, which
  means the gap this prints is a **lower bound**. It is left that way
  deliberately: the alternative is a hand-maintained list of which `.step`
  events are block-driven, which goes stale silently, and a gap report that
  is too small is a worse instrument than one that is too large only if
  somebody quotes it as a ceiling. Do not.

# What --check ratchets

The count of missing events, against `MAX_MISSING` below. It can only be
lowered by a commit that also lowers this constant, which is the point: an
emitter that lands takes the number down and pins it there, and a refactor
that silently stops emitting something puts it back up and reds.
"""

import argparse
import collections
import glob
import json
import os
import pathlib
import re
import sys

# Measured 2026-09-05 on 32 recordings: 98 distinct events emitted by vanilla, 20
# beyond this tree before the piston sounds landed, 18 after, 17 with flint and
# steel, 16 with the note block. Lower it when you land an emitter; never raise
MAX_MISSING = 16

ROOT = pathlib.Path(__file__).resolve().parent.parent


def registry_names():
    """id -> name, from Mojang's own report."""
    ver = re.search(
        r'GAME_VERSION: &str = "([^"]+)"',
        (ROOT / "crates/rustmc-proto/src/lib.rs").read_text(),
    ).group(1)
    path = ROOT / f"oracle-work/vanilla-{ver}/generated/reports/registries.json"
    if not path.exists():
        # Every tree links the shared extract in; the canonical one is the
        # fallback so this runs from a worktree that has not been set up.
        path = pathlib.Path(
            os.path.expanduser(f"~/rustmc/oracle-work/vanilla-{ver}/generated/reports/registries.json")
        )
    if not path.exists():
        sys.exit(f"no registry report at {path} — cannot name a sound id, and printing raw ids would be a table nobody can act on")
    doc = json.load(open(path))
    entries = doc["minecraft:sound_event"]["entries"]
    if len(entries) < 1000:
        sys.exit(f"the report lists {len(entries)} sound events — that is not this version, and a short registry would make every id name the wrong sound")
    return {v["protocol_id"]: k for k, v in entries.items()}


def emitted(pattern):
    """Every sound event id vanilla put on a stream, with how often."""
    counts = collections.Counter()
    files = sorted(glob.glob(os.path.expanduser(pattern)))
    scenarios = 0
    used = 0
    for f in files:
        try:
            doc = json.load(open(f))
        except (OSError, json.JSONDecodeError):
            continue
        used += 1
        for sc in doc.get("scenarios", []):
            scenarios += 1
            for o in sc.get("obs", []) or []:
                if o.get("kind") == "sound":
                    counts[o["fields"]["sound"]] += 1
    if used == 0:
        sys.exit(f"no readable recordings matched {pattern} — an empty set here reads as 'vanilla emits nothing', which is the shape of a coverage report that measures its own input")
    if not counts:
        sys.exit(f"{used} recordings, {scenarios} scenarios, and not one `sound` observation — the recorder's projection has moved and this comparison is measuring nothing")
    return counts, used, scenarios


def ours():
    """Every event name this tree can produce, and how each is reached."""
    literal = set()
    for base in ("crates", "tools"):
        for p in (ROOT / base).rglob("*.rs"):
            literal |= set(re.findall(r"minecraft:[a-z0-9_.]+", p.read_text(errors="ignore")))
    if len(literal) < 50:
        sys.exit(f"only {len(literal)} resource ids found in our own source — the scrape has gone stale, and an empty `ours` makes every event look like a gap")
    return literal


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recordings", default="~/rustmc*/oracle-work/gameplay/vanilla.json")
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    names = registry_names()
    counts, files, scenarios = emitted(args.recordings)
    literal = ours()

    missing = []
    covered_literal = 0
    covered_table = 0
    for sid, n in counts.most_common():
        name = names.get(sid)
        if name is None:
            sys.exit(f"sound id {sid} is in a recording and not in the registry report — the recording is from another version")
        if name in literal:
            covered_literal += 1
            continue
        # See the module docstring: this arm over-credits and the gap is
        # therefore a lower bound.
        if name.rsplit(".", 1)[-1] in ("place", "step"):
            covered_table += 1
            continue
        missing.append((n, sid, name))

    print(f"{files} recordings, {scenarios} scenario recordings (not distinct scenarios), {sum(counts.values())} sound observations")
    print(f"{len(counts)} distinct sound events emitted by vanilla")
    print(f"  {covered_literal} named in our source")
    print(f"  {covered_table} reached through sound_types' place/step columns (over-credited — see the header)")
    print(f"  {len(missing)} BEYOND THIS TREE, a lower bound:")
    for n, sid, name in missing:
        print(f"    {n:6d}  {sid:5d}  {name}")

    if args.check:
        if len(missing) > MAX_MISSING:
            print(f"\nsound-emission-gap: {len(missing)} events beyond this tree, ratchet is {MAX_MISSING}", file=sys.stderr)
            return 1
        if len(missing) < MAX_MISSING:
            print(f"\nsound-emission-gap: {len(missing)} events beyond this tree, ratchet is still {MAX_MISSING} — lower MAX_MISSING in this file", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
