#!/usr/bin/env python3
"""sound-coverage.py — what the use extract can and cannot say about sounds.

Census, not comparison. It reads `data/vanilla/use_without_item.json` and
counts the states whose sound `world_send::use_sound` can emit from the table
alone, against the states that sound and cannot be emitted. No server boots,
no build runs, and **the box-wide corpus lock is never taken** — it reads one
JSON file that already exists.

# Why this is a script and not a number in a doc

The figure it prints is registered in `ci/figures.toml`, and a registered
figure needs an artefact whose STAMP the checker can match. More usefully,
the number moved by a factor of seven under a one-line change to how
`soundPitchDrawn` is decided, and a figure that can do that needs to be
re-derivable rather than remembered.

# The two halves are different findings with different fixes

* **usable** — the extract names the event, the category, the volume, who is
  excluded, and a pitch that is vanilla's. `use_sound` emits these.
* **unusable** — the row sounds, and the extract will not publish its pitch
  because vanilla *drew* it. Every door, trapdoor and fence gate is here.
  Closing one means reading its class; regenerating the table will never move
  it, and reading this half as "the extraction is incomplete" is the mistake
  the number exists to prevent.

`--check` re-derives the counts and exits 1 if they disagree with the values
recorded in `ci/figures.toml`, so a re-extraction that moves coverage cannot
land while the ledger still quotes the old figure.
"""

import collections
import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def digest(path, n=12):
    """First `n` hex of a file's sha256, or `absent` when it is not there."""
    p = pathlib.Path(path)
    if not p.is_file():
        return "absent"
    h = hashlib.sha256()
    with p.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()[:n]


def census(table_path):
    """Count the sounding states by whether the table can emit them.

    Keyed on the same filter `UseTable::load` applies — a cell that refused,
    or that did not consume the click, never reaches `by_state` and so can
    never reach the emitter either. A census over a wider set than the
    consumer reads would be a number about a file rather than about the
    server.
    """
    doc = json.loads(pathlib.Path(table_path).read_text())
    usable, unusable, events, silent_writers = 0, 0, set(), []
    families = collections.Counter()
    for row in doc.get("table", []):
        for cell in row.get("states", []):
            if "refused" in cell or cell.get("consumes") is not True:
                continue
            if not cell.get("sound"):
                continue
            if cell.get("soundCount", 1) == 1 and "soundPitch" in cell:
                # **A describable row that writes nothing never reaches the
                # emitter, and counting it as usable would publish states the
                # server cannot sound.** `world_send`'s use path calls
                # `use_sound` inside `if let Some(next) = next`, so a rule
                # that consumed the click and wrote no block —
                # `Used::Consumed(None)`, an already-pressed button and 891
                # states like it — skips the sound entirely. This was zero on
                # the 184-block table of 2026-08-17 and the census refused on
                # the first one; on the 247-block table the probe environment
                # produces (`docs/cold-start/probe-answering-environment.md`)
                # there are sixteen, `minecraft:decorated_pot` first, and they
                # are a third bucket rather than a refusal: the table
                # describes them, the server's use path cannot reach them, and
                # the figure below counts them in its denominator and not in
                # its numerator. Listed by state so the reader can see which
                # class needs a use path that sounds without writing.
                if not cell.get("writes"):
                    silent_writers.append(cell.get("from"))
                    continue
                usable += 1
                events.add(cell.get("soundEvent"))
            else:
                unusable += 1
                # **The unusable half is not one bucket, and the split is the
                # finding.** Every state here is a door, a trapdoor or a fence
                # gate, and all three draw their pitch from the identical
                # `nextFloat() * 0.1F + 0.9F`. So `world_send::use_sound`
                # emits them anyway — the table names the event, which varies
                # per block set, and the shared formula names the pitch, which
                # the extract must not publish because the harness fixes its
                # seed. A fourth family appearing here would NOT be emitted
                # and would be silently silent, which is why this is counted
                # by name rather than totalled.
                event = cell.get("soundEvent") or ""
                if "_door." in event:
                    families["door"] += 1
                elif "trapdoor." in event:
                    families["trapdoor"] += 1
                elif "fence_gate." in event:
                    families["fence_gate"] += 1
                else:
                    families["UNEMITTED"] += 1
    return usable, unusable, sorted(events), silent_writers, families


def main():
    table = ROOT / "data/vanilla/use_without_item.json"
    if not table.is_file():
        print(f"no table at {table} — run ci/gen-gameplay-tables.sh", file=sys.stderr)
        return 2

    usable, unusable, events, silent_writers, families = census(table)
    if families["UNEMITTED"]:
        print(
            f"FAIL  {families['UNEMITTED']} sounding states are neither "
            f"fully described by the table nor a door/trapdoor/fence gate, "
            f"so world_send::use_sound emits nothing for them and no check "
            f"downstream would say so.",
            file=sys.stderr,
        )
        return 1
    unreached = len(silent_writers)
    jar = ROOT / "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
    dumper = ROOT / "tools/goldengen/vanilla/UseGolden.java"

    if "--check" in sys.argv:
        ledger = (ROOT / "ci/figures.toml").read_text()
        want = f"{usable:,} of {usable + unusable + unreached:,} sounding states"
        if want not in ledger:
            print(
                f"FAIL  sound coverage is {want}, and ci/figures.toml does not "
                f"say so. Re-run ci/sound-coverage.py and update the entry.",
                file=sys.stderr,
            )
            return 1
        print(f"OK    {want}")
        return 0

    print("SOUND COVERAGE — data/vanilla/use_without_item.json")
    print()
    print(f"  usable    {usable:6,}   the table names the event and a pitch vanilla chose")
    print(f"  unusable  {unusable:6,}   sounds, but vanilla DREW the pitch — needs its class read")
    print(f"  unreached {unreached:6,}   described, but the rule writes nothing, so the use path never sounds it")
    print(f"  sounding  {usable + unusable + unreached:6,}")
    print(f"  events    {len(events):6,}   distinct emittable sound events")
    print()
    print("  the unusable half, by family — all three share nextFloat()*0.1+0.9:")
    for fam in ("door", "trapdoor", "fence_gate"):
        print(f"    {families[fam]:6,}  {fam}")
    print(f"  emitted   {usable + unusable:6,}   of {usable + unusable + unreached:,} sounding states")
    print()
    if silent_writers:
        print("  the unreached bucket, by state:")
        for st in silent_writers:
            print(f"    {st}")
        print()
    for e in events:
        print(f"    {e}")
    print()
    print(
        f"STAMP        jar={digest(jar)} table={digest(table)} "
        f"dumper={digest(dumper)} usable={usable} unusable={unusable} "
        f"unreached={unreached} events={len(events)} door={families['door']} "
        f"trapdoor={families['trapdoor']} gate={families['fence_gate']}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
