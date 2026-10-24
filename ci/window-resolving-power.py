#!/usr/bin/env python3
"""Can a window still adjudicate a worldgen change, and down to what size?

# THE QUESTION THIS ANSWERS, AND WHY IT NEEDED A SCRIPT

`ci/admissible-residual.py` charges us at a position only when our block
matches neither of two unmodified vanilla references. `docs/DETERMINISM.md`
§12 argues that this construction has no *mask* — nothing is excluded, the
denominator is the whole window — and then writes **"there is no floor"**.

The first clause is true and the second does not follow. No mask means no
blind spot; it does not mean no noise. Two references under-sample the set of
blocks vanilla can put at a position, so part of the charge falls on *any*
run that is neither A nor B, and an unmodified vanilla run is such a run. §12
had no third reference of this window, so it compared the admissible charge
to the **twin mask's** width instead — 29,605 against 29,560, "45 blocks of
headroom" — and concluded the window was spent. Those two numbers are not
commensurable: one is a charge under an unmasked rule, the other is the size
of a mask that rule does not use.

The like-for-like number is the one this script measures. Substitute a
**known-innocent candidate** — a genuine unmodified vanilla run — for our
dump and read what the instrument charges it:

    floor(N) = |X ∉ {R_1 … R_N}|   for X vanilla, X not among the R_i

That is a positive control in the strict sense. Whatever it charges an
innocent candidate is noise, and it is the number every future claim on this
seed has to clear.

# THE THREE ANSWERS IT PRINTS, AND WHAT EACH ONE GOVERNS

1. **Level.** Our charge against the floor at the same N. This governs
   whether a *residual* may be quoted at all.
2. **Spread.** The floor's range across choices of candidate and reference
   subset. This governs how big a claimed *improvement* has to be before
   it can be told from having picked a luckier pair of references — the
   resolution the brief asks for.
3. **Depth.** Every charge split into **unanimous** (all references agree and
   the candidate does not) and **split** (references disagree and the
   candidate matches none).

   **This line used to add "rule 6 is met at that position", and that is
   false — measured 2026-08-10, `docs/DETERMINISM.md` §24.5.** On the
   seed-12345 r=16 window at N=15 the sixteen *innocent* candidates are
   charged 140..2,169 unanimous against 0..72 split, so unanimity is ~99% of
   the innocent floor rather than the complement of it. §18.1 says why it is
   structural: no floor position is charged to more than one candidate, so a
   floor position is by construction a place where N runs agree and one does
   not. Read the two numbers as a description of the window, never as
   defect-vs-noise.

A fourth thing falls out of the same files for free and is the reason this
window is not finished: the **per-kind** table. The window's aggregate floor
is enormous, but it is made almost entirely of a handful of block families
that read the world before they write it. A `vanilla -> ours` pair whose own
floor is 2 is adjudicable on this window even though the window's total is
not, and that is what turns "the instrument is spent" into a ranked worklist.

# INPUT

A directory of `region-differ diff <ref> <cand> --bbox … --positions` dumps
named `<ref>-vs-<cand>.pos`, ordered: the position set is symmetric but the
first column is the *reference's* block, which is what the unanimous/split
split reads. Parsing is delegated to `ci/admissible-residual.py`'s `load()`
rather than re-implemented — it is anchored and it refuses unless the line
count equals the differ's own `mismatches=`, which is the guard
`docs/DETERMINISM.md` §10 exists to explain.

usage: window-resolving-power.py <posdir> --refs a,b,c --cand ours
                                 [--max-n N] [--kinds N] [--label TEXT]
       window-resolving-power.py --selftest
exit 0 fine, 2 malformed or missing inputs.
"""

from __future__ import annotations

import collections
import importlib.util
import itertools
import os
import statistics
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
_PARSER = None


def _parser():
    """Borrow `load()` from admissible-residual.py. Cached — see below.

    The cache is load-bearing, not an optimisation: loading the module twice
    makes two distinct `Malformed` classes and an `except` in one place stops
    catching a `raise` from the other. `ci/admissible-floor.py`'s self-test
    caught exactly that.
    """
    global _PARSER
    if _PARSER is not None:
        return _PARSER
    path = os.path.join(_HERE, "admissible-residual.py")
    spec = importlib.util.spec_from_file_location("admissible_residual", path)
    if spec is None or spec.loader is None:  # pragma: no cover - defensive
        raise RuntimeError(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    _PARSER = mod
    return mod


def load_all(posdir: str, refs: list[str], cands: list[str]):
    """Load every `<ref>-vs-<cand>.pos` we will need. Returns {(ref,cand): dict}."""
    ar = _parser()
    out: dict[tuple[str, str], dict] = {}
    blocks = chunks = None
    for cand in cands:
        for ref in refs:
            if ref == cand:
                continue
            p = os.path.join(posdir, f"{ref}-vs-{cand}.pos")
            pos, b, c = ar.load(p)
            if blocks is None:
                blocks, chunks = b, c
            elif (b, c) != (blocks, chunks):
                raise ar.Malformed(
                    f"{p} covers {c} chunks / {b:,} blocks against "
                    f"{chunks} / {blocks:,}. One window, or the intersection "
                    "is meaningless."
                )
            out[(ref, cand)] = pos
    if blocks is None:
        raise ar.Malformed("no position files were loaded")
    return out, blocks, chunks


def charge(files, subset: list[str], cand: str) -> set:
    sets = [set(files[(r, cand)]) for r in subset]
    return set.intersection(*sets)


def depth(files, keys: set, subset: list[str], cand: str) -> tuple[int, int]:
    """(unanimous, split) over `keys` — do the references agree with each other?"""
    unanimous = 0
    for p in keys:
        if len({files[(r, cand)][p][0] for r in subset}) == 1:
            unanimous += 1
    return unanimous, len(keys) - unanimous


def brief(vals: list[int]) -> str:
    if not vals:
        return "—"
    if len(vals) == 1:
        return f"{vals[0]:,}"
    return (
        f"{min(vals):,}..{max(vals):,}  (median {int(statistics.median(vals)):,}, "
        f"n={len(vals)})"
    )


def run(posdir, refs, cand, max_n, kinds, label) -> int:
    ar = _parser()
    files, blocks, chunks = load_all(posdir, refs, refs + [cand])

    print(f"WINDOW RESOLVING POWER  {label}")
    print(f"  window      : {chunks} chunks, {blocks:,} blocks, no mask")
    print(f"  references  : {' '.join(refs)}  (N={len(refs)})")
    print(f"  candidate   : {cand}")

    twins = sorted(len(files[(x, y)]) for x, y in itertools.combinations(refs, 2))
    print(f"\n  TWIN FLOOR |X != Y| over {len(twins)} reference pairs")
    print(f"    {brief(twins)}")
    print(
        "    A floor belongs to the pair that produced it. Any single one of "
        "these\n    is a legitimate 'the' twin floor and none of them is the "
        "window's."
    )

    # The region where vanilla is not unanimous: the union of every pairwise
    # disagreement. Outside it, all N references spell the position the same
    # way, and a delta there is a delta against vanilla rather than against a
    # pair of runs.
    union: set = set()
    for x, y in itertools.combinations(refs, 2):
        union |= set(files[(x, y)])
    print(
        f"\n  NOT-UNANIMOUS REGION (union of all pairwise disagreements)"
        f"\n    {len(union):,} positions, {len(union) / blocks * 100:.6f}% of the window"
    )

    print("\n  ADMISSIBLE CHARGE BY N  — candidate vs N references")
    print(
        f"    {'N':<3} {'candidate ' + cand:<34} {'innocent vanilla floor':<34} sep"
    )
    verdict_rows = []
    for n in range(1, min(max_n, len(refs) - 1) + 1):
        ours_vals = [
            len(charge(files, list(s), cand))
            for s in itertools.combinations(refs, n)
        ]
        floor_vals = []
        for x in refs:
            others = [r for r in refs if r != x]
            for s in itertools.combinations(others, n):
                floor_vals.append(len(charge(files, list(s), x)))
        sep = int(statistics.median(ours_vals)) - int(statistics.median(floor_vals))
        verdict_rows.append((n, ours_vals, floor_vals, sep))
        print(f"    {n:<3} {brief(ours_vals):<34} {brief(floor_vals):<34} {sep:+,}")
    print(
        "    'sep' is median(candidate) - median(floor): what is left of the "
        "residual\n    after an innocent run is charged the same way."
    )

    # Depth at the largest N, for the candidate and for every innocent run.
    n = min(max_n, len(refs) - 1)
    full = list(refs)
    keys = charge(files, full, cand)
    un, sp = depth(files, keys, full, cand)
    print(f"\n  DEPTH at N={len(full)}")
    print(f"    {cand:<10} charge {len(keys):>8,}   unanimous {un:>8,}   split {sp:>7,}")
    for x in refs:
        others = [r for r in refs if r != x]
        k = charge(files, others, x)
        u, s = depth(files, k, others, x)
        print(
            f"    {x:<10} charge {len(k):>8,}   unanimous {u:>8,}   split {s:>7,}"
            f"   (innocent, N={len(others)})"
        )

    if kinds:
        print(f"\n  RANKED BY BLOCK PAIR — candidate at N={len(full)}, floor is the")
        print("  WORST any innocent vanilla run is charged for the same pair.")
        print(f"    {'vanilla -> ' + cand:<48}{'charged':>9}{'floor':>9}  verdict")
        ours_t: collections.Counter = collections.Counter()
        for p in keys:
            spell = {files[(r, cand)][p][0] for r in full}
            v = spell.pop() if len(spell) == 1 else "<refs-split>"
            ours_t[(v.replace("minecraft:", ""),
                    files[(full[0], cand)][p][1].replace("minecraft:", ""))] += 1
        floor_t: collections.Counter = collections.Counter()
        for x in refs:
            others = [r for r in refs if r != x]
            k = charge(files, others, x)
            t: collections.Counter = collections.Counter()
            for p in k:
                spell = {files[(r, x)][p][0] for r in others}
                v = spell.pop() if len(spell) == 1 else "<refs-split>"
                t[(v.replace("minecraft:", ""),
                   files[(others[0], x)][p][1].replace("minecraft:", ""))] += 1
            for key, val in t.items():
                floor_t[key] = max(floor_t[key], val)
        for (v, o), c in ours_t.most_common(kinds):
            f = floor_t.get((v, o), 0)
            verdict = "CLEAR" if c > 2 * max(f, 1) else ("marginal" if c > f else "FLOOR")
            print(f"    {v + ' -> ' + o:<48}{c:>9,}{f:>9,}  {verdict}")
        print(
            "    CLEAR = more than twice the worst innocent reading, so the pair "
            "is an\n    item. FLOOR = at or under it, so the pair is vanilla "
            "disagreeing with\n    itself and opening an investigation on it "
            "would be measuring noise."
        )
    return 0


def selftest() -> int:
    """Prove the arithmetic and the refusals on fixtures — no box, no jar.

    Same discipline and the same reason as `admissible-residual.py`'s: a
    checker whose refusals never fire is this project's most-found defect.
    """
    import tempfile
    from pathlib import Path

    ok = True

    def expect(lbl, rc, want):
        nonlocal ok
        if rc != want:
            print(f"SELFTEST FAIL [{lbl}]: exit {rc}, expected {want}")
            ok = False
        else:
            print(f"SELFTEST ok   [{lbl}]: exit {rc}")

    S = "DIFF chunks=1 (a=1 b=1)  blocks=100 mismatches={n}  biomes=4/0\n"
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)

        def w(name, lines):
            (d / name).write_text(
                "".join(lines) + S.format(n=len(lines)), encoding="utf-8"
            )

        # Three references a,b,c and a candidate `ours`. Every ordered pair is
        # written because the unanimous/split split reads the first column.
        w("a-vs-b.pos", ["1 1 1 minecraft:stone minecraft:granite\n"])
        w("b-vs-a.pos", ["1 1 1 minecraft:granite minecraft:stone\n"])
        w("a-vs-c.pos", ["1 1 1 minecraft:stone minecraft:granite\n"])
        w("c-vs-a.pos", ["1 1 1 minecraft:granite minecraft:stone\n"])
        w("b-vs-c.pos", [])
        w("c-vs-b.pos", [])
        # ours differs from all three at (2,2,2) where they are unanimous, and
        # at (1,1,1) it writes a fourth answer where they are split.
        # TWO unanimous and ONE split, deliberately not one of each: with a
        # symmetric fixture, inverting the unanimity test swaps 1 and 1 and the
        # mutation is invisible. Asymmetry is what makes the assertion below
        # able to fail. (Found by mutation-testing this very self-test.)
        for r, v in (("a", "stone"), ("b", "granite"), ("c", "granite")):
            w(
                f"{r}-vs-ours.pos",
                [
                    f"1 1 1 minecraft:{v} minecraft:diorite\n",
                    f"2 2 2 minecraft:tuff minecraft:andesite\n",
                    f"3 3 3 minecraft:deepslate minecraft:stone\n",
                ],
            )
        # An exit code is not enough, and this is not a hypothetical. The first
        # version of this self-test checked only exit codes; a mutation that
        # inverted the unanimity test (`== 1` -> `!= 1`) passed it clean. So
        # the arithmetic is asserted on the printed line, not just the status.
        # By construction: (2,2,2) is unanimous — a, b and c all say tuff and
        # we say andesite — and (1,1,1) is split, a says stone while b and c
        # say granite and we say a fourth thing. Charge 2, unanimous 1, split 1.
        import contextlib
        import io as _io

        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = main([str(d), "--refs", "a,b,c", "--cand", "ours", "--kinds", "0"])
        out = " ".join(buf.getvalue().split())
        expect("happy path", rc, 0)
        for want in (
            "ours charge 3 unanimous 2 split 1",
            "NOT-UNANIMOUS REGION (union of all pairwise disagreements) 1 positions",
        ):
            if want in out:
                print(f"SELFTEST ok   [arithmetic: {want[:34]}…]")
            else:
                print(f"SELFTEST FAIL [arithmetic]: {want!r} not in output")
                ok = False
        expect("missing file refused", main([str(d), "--refs", "a,b,z",
                                             "--cand", "ours", "--kinds", "0"]), 2)
        expect("no refs refused", main([str(d), "--cand", "ours"]), 2)
        # A file whose own summary disagrees with its line count.
        (d / "a-vs-ours.pos").write_text(
            "1 1 1 minecraft:stone minecraft:diorite\n"
            + S.format(n=7),
            encoding="utf-8",
        )
        expect("miscounted file refused", main([str(d), "--refs", "a,b,c",
                                                "--cand", "ours", "--kinds", "0"]), 2)

    # --- THE PER-BLOCK VERDICT, WHICH NOTHING ABOVE EXERCISES --------------
    #
    # Every case above runs with `--kinds 0`, so the whole `if kinds:` block —
    # the floor table and the CLEAR/marginal/FLOOR arithmetic that briefs are
    # written from — was never executed by this self-test. Three live
    # mutations survived it:
    #
    #   * `floor_t[key] = max(floor_t[key], val)` -> `min`, which reports the
    #     *kindest* innocent run instead of the worst and understates a floor;
    #   * dropping the `max(f, 1)` clamp, which calls a pair with a zero floor
    #     CLEAR on a single block;
    #   * `c > f` -> `c >= f`, which promotes a pair sitting exactly on its
    #     floor from FLOOR to marginal.
    #
    # A verdict word alone cannot catch the first, so the printed floor
    # NUMBER is asserted too. The fixture is built from per-reference worlds
    # rather than hand-written pos files, because the sixteen ordered pairs
    # four references need are not writable by hand without a mistake.
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        default = "minecraft:bedrock"
        worlds: dict[str, dict[tuple[int, int, int], str]] = {
            r: {} for r in ("a", "b", "c", "d", "ours")
        }

        def put(pos, **blocks):
            for who, name in blocks.items():
                worlds[who][pos] = f"minecraft:{name}"

        # tuff -> andesite: charged 2, floor 0. Only the clamp keeps this off
        # CLEAR, so it is the clamp's witness.
        for z in (1, 2):
            put((1, 1, z), a="tuff", b="tuff", c="tuff", d="tuff", ours="andesite")
        # sand -> gravel: charged 3 against a floor of exactly 3, so it sits
        # ON its floor — the witness for `c > f` against `c >= f`.
        for z in (1, 2, 3):
            put((2, 2, z), a="sand", b="sand", c="sand", d="sand", ours="gravel")
            put((3, 3, z), a="sand", b="sand", c="sand", d="gravel", ours="sand")
        # deepslate -> stone: two innocent runs are charged this pair, 1 and 2.
        # The floor is the WORSE of them, so max reports 2 and min reports 1.
        put((4, 4, 1), a="deepslate", b="deepslate", c="stone", d="deepslate",
            ours="deepslate")
        for z in (1, 2):
            put((5, 5, z), a="deepslate", b="deepslate", c="deepslate", d="stone",
                ours="deepslate")
        for z in (1, 2, 3, 4, 5):
            put((6, 6, z), a="deepslate", b="deepslate", c="deepslate",
                d="deepslate", ours="stone")

        names = ("a", "b", "c", "d", "ours")
        everywhere = sorted({p for w in worlds.values() for p in w})
        for left in names:
            for right in names:
                if left == right:
                    continue
                lines = []
                for p in everywhere:
                    lv = worlds[left].get(p, default)
                    rv = worlds[right].get(p, default)
                    if lv != rv:
                        lines.append(f"{p[0]} {p[1]} {p[2]} {lv} {rv}\n")
                (d / f"{left}-vs-{right}.pos").write_text(
                    "".join(lines) + S.format(n=len(lines)), encoding="utf-8"
                )

        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = main([str(d), "--refs", "a,b,c,d", "--cand", "ours",
                       "--kinds", "5"])
        expect("per-block happy path", rc, 0)
        rows = [" ".join(ln.split()) for ln in buf.getvalue().splitlines()]
        for want in (
            # charged, floor, verdict — the floor number is the mutation witness
            "deepslate -> stone 5 2 CLEAR",
            "sand -> gravel 3 3 FLOOR",
            "tuff -> andesite 2 0 marginal",
        ):
            if want in rows:
                print(f"SELFTEST ok   [per-block: {want}]")
            else:
                got = [r for r in rows if r.split(" ->")[0] == want.split(" ->")[0]]
                print(f"SELFTEST FAIL [per-block]: wanted {want!r}, got {got!r}")
                ok = False
    print("SELFTEST " + ("GREEN" if ok else "RED"))
    return 0 if ok else 1


OPTIONS = ("--refs", "--cand", "--max-n", "--kinds", "--label")


def main(argv: list[str]) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()
    opts: dict[str, str] = {}
    args: list[str] = []
    i = 0
    while i < len(argv):
        if argv[i] in OPTIONS:
            if i + 1 >= len(argv):
                print(f"{argv[i]} needs a value", file=sys.stderr)
                return 2
            opts[argv[i]] = argv[i + 1]
            i += 2
            continue
        if argv[i].startswith("--"):
            print(f"unknown option {argv[i]}", file=sys.stderr)
            return 2
        args.append(argv[i])
        i += 1
    if len(args) != 1 or "--refs" not in opts or "--cand" not in opts:
        print(
            "usage: window-resolving-power.py <posdir> --refs a,b,c --cand ours "
            "[--max-n N] [--kinds N] [--label TEXT]",
            file=sys.stderr,
        )
        return 2
    refs = [r for r in opts["--refs"].split(",") if r]
    if len(refs) < 2:
        print("need at least two references", file=sys.stderr)
        return 2
    ar = _parser()
    try:
        return run(
            args[0],
            refs,
            opts["--cand"],
            int(opts.get("--max-n", "99")),
            int(opts.get("--kinds", "30")),
            opts.get("--label", ""),
        )
    except (ar.Malformed, OSError) as exc:
        print(f"RESOLVING-POWER MALFORMED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
