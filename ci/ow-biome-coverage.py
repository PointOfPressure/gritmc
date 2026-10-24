#!/usr/bin/env python3
"""ow-biome-coverage.py — which overworld biomes carry a published figure, and
the fewest new windows that would cover the rest.

`ci/figures.toml` registers 41 overworld figures. Every one of them is centre
chunk (64,64) at r=16, on three seeds. `docs/PARITY.md` section 2's two
narrower rows (chunks 57..71 and 59..69) are sub-squares of that same centre.
Three points on the map, and the window was chosen by centre and radius in
every case — never by biome.

Measured by self-diff (`region-differ diff X X --by-biome`, zero mismatches
and a free decomposition), those three points hold eighteen of the overworld's
fifty-five biomes between them. The other thirty-seven have no figure at all.
That is not a weak measurement of them; it is the absence of one, and nothing
in the ledger said so because a ledger entry records the window it was taken
in and not the windows it was not.

This reads `ci/ow-biome-scout.sh`'s output and answers the follow-on question:
given the squares scouted, which few of them would cover the most unmeasured
biomes? Greedy set cover, with a minimum share so a window is not credited
with a biome it holds four hundred blocks of.

Usage: ow-biome-coverage.py <scout-dir> [--min-share 0.5] [--pick 8]
"""
import json, os, sys, glob

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# The eighteen, measured rather than assumed: `region-differ diff X X
# --by-biome` on gen-wide-20260722-r16-ow-a, gen-wide-12345-r16-ow-c,
# gen-wide-12345-r18-ow-x1 and floors-ow31415926r16-a over their own published
# bboxes.
#
# THIS LIST SAID TWELVE AND THE FOURTH WINDOW WAS MISSED. Seed 31415926's
# references are not named `gen-wide-*`; they are `floors-ow31415926r16-*`
# under a different root entirely. A search keyed on the naming family of the
# other two windows returned nothing and looked conclusive. Search by content.
MEASURED = {
    # 20260722 r=16
    "savanna", "lukewarm_ocean", "river", "beach", "sulfur_caves",
    "windswept_savanna",
    # 12345 r=16 and r=18
    "frozen_ocean", "snowy_plains", "snowy_beach", "frozen_river",
    "snowy_taiga", "ice_spikes",
    # 31415926 r=16 — the window that carries no quotable figure of its own
    "jungle", "lush_caves", "plains", "deep_lukewarm_ocean", "forest",
    "sparse_jungle",
}


def all_overworld_biomes():
    """From the pinned jar's own extracted parameter list, not from a
    hand-kept list — a hand-kept one is how `pale_garden` gets forgotten."""
    p = os.path.join(ROOT, "data", "vanilla", "biome_parameters_overworld.json")
    with open(p) as fh:
        return sorted({e[-1].split(":")[1] for e in json.load(fh)})


def read_scout(d):
    """tag -> {biome: share}. Shares are of the scouted square, not of the
    world."""
    out = {}
    for f in sorted(glob.glob(os.path.join(d, "*.census"))):
        tag = os.path.basename(f)[:-len(".census")]
        comp = {}
        for line in open(f):
            parts = line.split()
            if len(parts) != 3:
                continue
            biome, _blocks, share = parts
            comp[biome] = float(share.rstrip("%"))
        if comp:
            out[tag] = comp
    return out


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    scout_dir = args[0]
    min_share = 0.5
    pick = 8
    if "--min-share" in args:
        min_share = float(args[args.index("--min-share") + 1])
    if "--pick" in args:
        pick = int(args[args.index("--pick") + 1])

    biomes = all_overworld_biomes()
    unmeasured = [b for b in biomes if b not in MEASURED]
    print(f"overworld biomes (pinned jar parameter list): {len(biomes)}")
    print(f"  measured in some published window:          {len(biomes) - len(unmeasured)}")
    print(f"  NO FIGURE AT ALL:                           {len(unmeasured)}")
    print()

    scout = read_scout(scout_dir)
    if not scout:
        print(f"no scout censuses under {scout_dir} — run ci/ow-biome-scout.sh first")
        return 2
    print(f"scouted squares: {len(scout)}")

    # Which of the unmeasured biomes does the scout reach at all?
    reach = {}
    for tag, comp in scout.items():
        for b, share in comp.items():
            if b in unmeasured and share >= min_share:
                reach.setdefault(b, []).append((share, tag))
    unreachable = [b for b in unmeasured if b not in reach]
    print(f"unmeasured biomes the scout reaches at >= {min_share}%: {len(reach)}")
    if unreachable:
        print(f"unmeasured and NOT reached by any scouted square: {len(unreachable)}")
        print("   " + ", ".join(unreachable))
    print()

    # Greedy set cover.
    todo = set(reach)
    chosen = []
    while todo and len(chosen) < pick:
        best, gain = None, set()
        for tag, comp in scout.items():
            got = {b for b, s in comp.items() if b in todo and s >= min_share}
            if len(got) > len(gain):
                best, gain = tag, got
        if not best:
            break
        chosen.append((best, sorted(gain, key=lambda b: -scout[best][b])))
        todo -= gain

    print(f"greedy cover — {len(chosen)} windows cover {len(reach) - len(todo)} of {len(reach)}:")
    for tag, gain in chosen:
        cx, cz = tag[1:].split("_")
        got = ", ".join(f"{b} {scout[tag][b]:.1f}%" for b in gain)
        print(f"  centre ({cx},{cz})  +{len(gain):>2}  {got}")
    if todo:
        print(f"  still uncovered after {len(chosen)}: {', '.join(sorted(todo))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
