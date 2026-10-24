#!/usr/bin/env python3
"""Render the three-position extract per block class, and against the census.

    ci/shape-three-byclass.py            # the per-class table
    ci/shape-three-byclass.py --cross    # census verdict vs extract verdict
    ci/shape-three-byclass.py --md       # the same, as a markdown table

# WHY THIS IS A SCRIPT

`docs/PARITY-ROADMAP.md` §83.9's per-class table was assembled by hand from
four separate `ci/shape-three.sh --only` runs, which cost four sweeps and
covered four classes. The whole-table artefact carries `byClass` for **every**
refusing class — 74 of them — and has since it was first written. Nobody read
it that way, so 69 classes and 11,901 pairs went undescribed while the
expensive path was used to describe five.

So this reads the artefact. A per-class figure now costs nothing once the
whole-table run has been paid for, and `--only` goes back to being what its
own header says it is: a way to price a class before paying for it.

# WHY `--cross` IS THE INTERESTING HALF

The census assigns each refused pair a verdict — `recoverable`, `connective`,
`inert`, `world`, `refused` — and those verdicts are about a level that
**invents** the world at one position. The extract's verdicts are about a level
that answers three positions and throws for a fourth. They are different
questions and the roadmap has one worked example of how differently they
answer: `FenceGateBlock` is 768 `recoverable` in the census and 768 `closed`
here, while `StairBlock` is 14,400 `recoverable` and **all 19,200 refuse**.

`--cross` is that comparison for the whole population instead of for two
classes. A `recoverable` verdict says three fillers at one position agreed. It
never said one position was all the rule reads, and the cross-tabulation is how
much that distinction is worth in pairs.
"""
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXTRACT = ROOT / "data/vanilla/update_shape3.json"
CENSUS_THREE = ROOT / "ci/checks/shape-three-census.json"
CENSUS = ROOT / "ci/checks/shape-refusal-census.json"
ORDER = ("closed", "connective", "inert", "refused")


def load():
    # The census is the extract minus the rows, so either answers `byClass`.
    # Prefer the committed census: a tree that ran `--check` has it and may not
    # have kept the table, which is gitignored.
    for p in (CENSUS_THREE, EXTRACT):
        if p.is_file():
            return json.loads(p.read_text()), p
    print(f"shape-three-byclass: neither {CENSUS_THREE.relative_to(ROOT)} nor "
          f"{EXTRACT.relative_to(ROOT)} exists — run ci/shape-three.sh",
          file=sys.stderr)
    raise SystemExit(2)


def main():
    d, src = load()
    if d.get("only"):
        print(f"shape-three-byclass: {src.name} is a --only {d['only']} run and "
              f"covers one class; the per-class table needs a whole-table run",
              file=sys.stderr)
        raise SystemExit(2)
    rows = sorted(d["byClass"], key=lambda r: (-r["pairs"], r["class"]))
    md = "--md" in sys.argv

    if "--cross" not in sys.argv:
        if md:
            print("| class | pairs | closed | connective | inert | refused |")
            print("|---|---:|---:|---:|---:|---:|")
        else:
            print(f"{'class':<32}{'pairs':>8}" + "".join(f"{k:>12}" for k in ORDER))
        for r in rows:
            v = r["verdicts"]
            cells = [v.get(k, 0) for k in ORDER]
            if md:
                print(f"| `{r['class']}` | {r['pairs']:,} | "
                      + " | ".join(f"{c:,}" for c in cells) + " |")
            else:
                print(f"{r['class']:<32}{r['pairs']:>8}"
                      + "".join(f"{c:>12,}" for c in cells))
        tot = {k: sum(r["verdicts"].get(k, 0) for r in rows) for k in ORDER}
        print()
        print(f"{len(rows)} classes, {d['refusedPairs']:,} pairs, "
              f"{d['rows']:,} rows, {d['probes']:,} probes")
        print("  " + "  ".join(f"{k} {tot[k]:,}" for k in ORDER))
        return 0

    if not CENSUS.is_file():
        print(f"shape-three-byclass: {CENSUS.relative_to(ROOT)} is absent, so the "
              f"census half of the cross-tabulation cannot be built",
              file=sys.stderr)
        raise SystemExit(2)
    c = json.loads(CENSUS.read_text())
    cby = {r["class"]: r["verdicts"] for r in c["byClass"]}

    # Per CLASS, not per pair: the two artefacts key on (state, direction) but
    # neither writes the individual pair's verdict out, so a pair-level join is
    # not available and claiming one would be inventing it. A class whose census
    # verdicts are not unanimous is reported as mixed rather than attributed.
    print("Census verdict vs extract verdict, by class. Classes whose census")
    print("verdicts are not unanimous are listed as `mixed`, because neither")
    print("artefact writes a per-pair verdict and a pair-level join would be")
    print("invented rather than derived.")
    print()
    buckets = {}
    for r in rows:
        cv = cby.get(r["class"], {})
        key = next(iter(cv)) if len(cv) == 1 else "mixed"
        ev = r["verdicts"]
        ekey = next((k for k in ORDER if ev.get(k, 0) == r["pairs"]), "mixed")
        b = buckets.setdefault((key, ekey), [0, 0, []])
        b[0] += 1
        b[1] += r["pairs"]
        if len(b[2]) < 4:
            b[2].append(r["class"])
    print(f"{'census':<14}{'extract':<14}{'classes':>8}{'pairs':>10}  examples")
    for (cv, ev), (n, p, ex) in sorted(buckets.items(), key=lambda kv: -kv[1][1]):
        print(f"{cv:<14}{ev:<14}{n:>8}{p:>10,}  {', '.join(ex)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
