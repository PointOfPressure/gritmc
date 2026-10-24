#!/usr/bin/env python3
"""Compare two ensembles of DIFFERENT sizes without letting the size do the work.

    ci/corpus-floor-subsample.py <big-ensemble.json> --n 6 --draws 200 \
        [--reference <small-floor.json>] [--out DIR]

# WHY THIS EXISTS, AND IT IS A CORRECTION TO A DESIGN I HAD ALREADY COMMITTED

`docs/CORPUS-LOCK-HOLD.md` §17.1 pre-registered n=6 in the serial arm and n=18
in the concurrent arm, on the grounds that the two arms get equal wall clock and
the concurrent one converts that into three times the recordings. That is a fair
way to spend the time and an **unfair way to compare the floors**, because

    the floor is monotone increasing in n.

More runs is more chances for a scenario to be seen flapping, and nothing else
has to change. `docs/PARITY.md` §4 is the standing example: the End's floor was
0 across two references and 252 across six. So a concurrent arm at n=18 would be
expected to report a higher floor than a serial arm at n=6 **with no
interference whatsoever**, and reading that as evidence would be reading the
sample size.

The fix costs no recordings. `ci/corpus-floor.sh` writes one projection per
recording into its output directory and lists them in `ensemble.json`, so a
floor over any SUBSET of an arm can be measured by rewriting the manifest.
This draws subsets of the big arm at the small arm's size and reports the
distribution, which is what the serial arm's single number should be read
against.

# WHAT IT DOES NOT DO

It does not make n=6 into n=18. The subsample distribution answers "is the
serial arm's floor unusual among concurrent ensembles of the same size", and
the concurrent arm's own full-size floor still answers the more sensitive
question at its own resolving power. Both are reported; neither replaces the
other.
"""
import argparse, itertools, json, math, random, subprocess, sys, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def measure(manifest_path, out_path):
    subprocess.run(
        [str(ROOT / "ci" / "corpus-floor.py"), "measure", str(manifest_path),
         "--out", str(out_path)],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return json.loads(Path(out_path).read_text())


def bad_names(doc):
    """The scenarios the floor is made of.

    `ci/corpus-floor.py` writes `floor_names` and a `measured` map keyed by
    scenario. The first version of this read a `scenarios` LIST that does not
    exist, found nothing, and printed an empty table under a floor of 5 without
    complaining, which is the shape of defect this repo has the most of. It now
    reads `floor_names` and refuses a document that has neither."""
    if "floor_names" in doc:
        return set(doc["floor_names"])
    if "measured" in doc:
        return {n for n, r in doc["measured"].items()
                if isinstance(r.get("distinct"), int) and r["distinct"] > 1}
    raise SystemExit("floor.json has neither floor_names nor measured — "
                     "ci/corpus-floor.py's output shape moved and this reader "
                     "would silently report an empty set")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ensemble")
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--draws", type=int, default=200)
    ap.add_argument("--reference", help="a floor.json to place in the distribution")
    ap.add_argument("--seed", type=int, default=20260818)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()

    ens = json.loads(Path(a.ensemble).read_text())
    runs = ens["runs"]
    N = len(runs)
    if a.n < 2:
        sys.exit("subsets of fewer than two recordings measure nothing")
    if a.n > N:
        sys.exit(f"asked for subsets of {a.n} from an ensemble of {N}")

    total = math.comb(N, a.n)
    if total <= a.draws:
        subsets = list(itertools.combinations(range(N), a.n))
        how = f"all {total} subsets"
    else:
        rnd = random.Random(a.seed)
        seen, subsets = set(), []
        while len(subsets) < a.draws:
            s = tuple(sorted(rnd.sample(range(N), a.n)))
            if s in seen:
                continue
            seen.add(s)
            subsets.append(s)
        how = f"{len(subsets)} random subsets of {total}, seed {a.seed}"

    work = Path(a.out) if a.out else Path(tempfile.mkdtemp(prefix="floor-subsample."))
    work.mkdir(parents=True, exist_ok=True)

    floors, names = [], {}
    for i, sub in enumerate(subsets):
        m = dict(ens)
        m["id"] = f"{ens.get('id','sub')}-sub{i}"
        m["runs"] = [runs[k] for k in sub]
        mp = work / f"m{i}.json"
        mp.write_text(json.dumps(m))
        doc = measure(mp, work / f"f{i}.json")
        floors.append(doc["floor"])
        for nm in bad_names(doc):
            names[nm] = names.get(nm, 0) + 1
        mp.unlink()
        (work / f"f{i}.json").unlink()

    floors.sort()
    def pct(p):
        return floors[min(len(floors) - 1, int(p * len(floors)))]

    print(f"ensemble {a.ensemble}")
    print(f"  N={N} recordings, subsets of n={a.n}: {how}")
    print(f"  floor over subsets: min {floors[0]}  p05 {pct(0.05)}  median {pct(0.5)}"
          f"  p95 {pct(0.95)}  max {floors[-1]}")
    print(f"  mean {sum(floors)/len(floors):.2f}")
    print(f"  per-scenario resolving power at n={a.n}: {(1-0.05**(1.0/a.n))*100:.2f}%")
    print("  scenarios seen not reproducing, and in what fraction of subsets:")
    for nm, c in sorted(names.items(), key=lambda kv: -kv[1]):
        print(f"    {c/len(subsets):5.0%}  {nm}")

    if a.reference:
        ref = json.loads(Path(a.reference).read_text())
        rf = ref["floor"]
        below = sum(1 for f in floors if f < rf)
        equal = sum(1 for f in floors if f == rf)
        print()
        print(f"  reference floor {rf} sits at percentile "
              f"{(below + equal/2)/len(floors)*100:.0f} of this distribution")
        rn = bad_names(ref)
        un = rn - set(names)
        print(f"  scenarios in the reference that NO subset of this arm ever showed: "
              f"{len(un)}")
        for nm in sorted(un):
            print(f"    {nm}")


if __name__ == "__main__":
    main()
