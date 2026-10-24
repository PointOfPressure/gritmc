#!/usr/bin/env python3
"""The admissible instrument's OWN noise floor, and its N-reference ladder.

# WHY THIS EXISTS

`ci/admissible-residual.py` charges us at a position only when our block
matches neither of two unmodified vanilla references, and argues — correctly —
that this construction has no *mask*: nothing is excluded, the denominator is
the whole window, and rule 6's disqualifier has no premise. `docs/DETERMINISM.md`
§12 states that as **"there is no floor"**.

**That sentence is true about the mask and false about the instrument, and the
difference is what this script exists to measure.** "No mask" means no
*blind spot*. It does not mean no *noise*. The admissible charge is
`|ours ∉ {A,B}|`, and two references under-sample the set of blocks vanilla
can put at a position, so some of what is charged to us is charged to any run
that is not A and is not B — including a genuine unmodified vanilla run. The
size of that is a floor in exactly rule 6's sense: the reading the instrument
gives when the thing it measures is known to be innocent.

Nobody had measured it. §12 compares the admissible charge to the **twin
mask's** floor instead (29,605 against 29,560, "45 blocks of headroom") and
concludes the window is spent. Those two numbers are not commensurable — one
is a charge under an unmasked N=2 admissible rule, the other is the width of a
mask that rule does not use — and the comparison can mislead in either
direction. This script replaces it with the like-for-like number: run the
admissible rule with a **genuine vanilla run substituted for ours**.

    floor(N) = |X ∉ {R_1 … R_N}|   for X an unmodified vanilla run
                                   not among the R_i

That is a positive control in the strict sense: the candidate is known to be
innocent, so whatever the instrument charges it is noise. A residual below
floor(N) is corroboration; a *delta* is resolvable only if it is larger than
the run-to-run spread of floor(N) across choices of X.

# THE SECOND NUMBER: UNANIMOUS vs SPLIT

Every charged position carries the references' own answers, so each one can be
put in exactly one of two classes, and they have different evidentiary status:

* **unanimous** — every reference spells the position the same way and we
  spell it differently.
* **split** — the references disagree among themselves and we match none of
  them. Vanilla is not sure what goes here; a further reference might well
  produce our answer.

**AND THE EVIDENTIARY WEIGHT THIS SPLIT WAS BUILT TO CARRY IS NOT THERE —
MEASURED 2026-08-10, `docs/DETERMINISM.md` §24.5.** These lines used to read
"Two independent vanilla runs agree, so rule 6 is *satisfied at that position*,
not waived. **These are defects**", and "the unanimous count is the part of the
residual that no amount of further vanilla sampling can explain away". Both
claims are false, and the way they are false is the reason to say so loudly
rather than quietly delete them: they are *reassuring*.

Run the instrument on a candidate known to be innocent — a genuine unmodified
vanilla run — over the seed-12345 r=16 window at N=15, and the sixteen innocent
candidates are charged **140..2,169 unanimous** positions against **0..72**
split. **Unanimity is ~99% of the innocent floor, not the complement of it.**
Our own charge on that window splits 1,327 unanimous / 2 split, which under the
old reading would have convicted 1,327 defects; candidate `m` is an unmodified
vanilla run and reads 2,169 by the same rule.

The mechanism is in §18.1 and it makes this structural rather than unlucky:
over that ensemble **no floor position is charged to more than one candidate**.
Vanilla's noise is per-run private resampling, so a floor position is by
construction a place where N runs happen to agree and one does not — which is
the definition of unanimous. A split position needs *two* runs to land on the
same wrong answer, which is exactly what per-run-private noise does not do.

So: report both numbers, because the ratio between them characterises the
window, but **do not read `unanimous` as `defect`.** The only thing that
adjudicates a charge is the charge of a known-innocent candidate under the
identical rule at the identical N. `ci/floor-vs-n.py` measures that.

# INPUT

The positional arguments are `<ref> vs <candidate>` position dumps from
`region-differ diff <ref> <candidate> --bbox … --positions`, one per
reference, **all against the same candidate and the same window**. The
candidate may be our dump or another vanilla run; the arithmetic does not care
and that symmetry is the whole point.

`region-differ` writes position lines into the same stream as its summary, so
parsing is delegated to `ci/admissible-residual.py`'s `load()` rather than
re-implemented: it is anchored, it refuses unless the line count equals the
differ's own `mismatches=`, and `docs/DETERMINISM.md` §10 records what a loose
filter cost the last time. One parser, one refusal path.

usage: admissible-floor.py <ref1-vs-X.pos> <ref2-vs-X.pos> [...]
                           [--label TEXT] [--ladder] [--expect-charge N]
exit 0 fine, 2 malformed inputs or fewer than two references,
1 `--expect-charge` did not match.
"""

from __future__ import annotations

import importlib.util
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))


_PARSER = None


def _load_parser():
    """Borrow `load()` from admissible-residual.py — hyphen, so no plain import.

    Cached, and the cache is load-bearing rather than an optimisation: loading
    the module twice makes two distinct `Malformed` *classes*, so the `except`
    in `main` would not catch what `report` raised. The self-test found exactly
    that on this script's first run.
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


OPTIONS = ("--label", "--expect-charge")
FLAGS = ("--ladder",)


def report(paths: list[str], label: str, ladder: bool) -> tuple[int, int, int, int]:
    """Print the charge for `paths` and return (charge, unanimous, split, blocks)."""
    ar = _load_parser()
    loaded = []
    for p in paths:
        pos, blocks, chunks = ar.load(p)
        loaded.append((p, pos, blocks, chunks))

    blocks = loaded[0][2]
    chunks = loaded[0][3]
    for p, _, b, c in loaded[1:]:
        if (b, c) != (blocks, chunks):
            raise ar.Malformed(
                f"{p} covers {c} chunks / {b:,} blocks against "
                f"{chunks} / {blocks:,}. An admissible set has to be built "
                "over one window."
            )

    print(f"ADMISSIBLE-FLOOR {label}")
    print(f"  window                : {chunks} chunks, {blocks:,} blocks, no mask")
    for p, pos, _, _ in loaded:
        print(f"  candidate != {os.path.basename(p):<24}: {len(pos):>9,}")

    charge = set(loaded[0][1])
    rungs: list[tuple[int, int]] = [(1, len(charge))]
    for i, (_, pos, _, _) in enumerate(loaded[1:], start=2):
        charge &= set(pos)
        rungs.append((i, len(charge)))

    # Split the charge by whether the references agree with EACH OTHER there.
    # `pos[p]` is (reference_block, candidate_block); the candidate block is
    # the same in every file by construction, so unanimity is a property of
    # the first element alone.
    unanimous = 0
    for p in charge:
        spellings = {loaded[i][1][p][0] for i in range(len(loaded))}
        if len(spellings) == 1:
            unanimous += 1
    split = len(charge) - unanimous

    if ladder:
        print("  ladder (charge as references are added):")
        for n, c in rungs:
            print(f"    N={n:<2} {c:>9,}")
    pct = 100.0 * (1.0 - len(charge) / blocks)
    print(f"  N={len(loaded)} charge          : {len(charge):>9,} of {blocks:,}")
    print(f"    unanimous (rule 6 met): {unanimous:>9,}")
    print(f"    split (refs disagree) : {split:>9,}")
    print(f"  ADMISSIBLE EXACTNESS  : {pct:.6f}%")
    return len(charge), unanimous, split, blocks


def selftest() -> int:
    """Prove this script can fail, on fixtures, in T0, with no box and no jar.

    Same discipline as `admissible-residual.py --selftest`, and for the same
    reason: a checker whose refusals are never exercised is this project's
    most-found defect class. Every case is one way a real run can be wrong.
    """
    import tempfile
    from pathlib import Path

    ok = True

    def write(d: Path, name: str, body: str) -> str:
        p = d / name
        p.write_text(body, encoding="utf-8")
        return str(p)

    def expect(lbl: str, rc: int, want: int) -> None:
        nonlocal ok
        if rc != want:
            print(f"SELFTEST FAIL [{lbl}]: exit {rc}, expected {want}")
            ok = False
        else:
            print(f"SELFTEST ok   [{lbl}]: exit {rc}")

    S = "DIFF chunks=1 (a=1 b=1)  blocks=100 mismatches={n}  biomes=4/0\n"

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        # Three references against one candidate.
        #   (1,2,3): all three refs say stone, candidate says air  -> UNANIMOUS
        #   (4,5,6): a/b say stone, c says granite, candidate dirt -> SPLIT
        #   (7,8,9): in a only, so not charged at N>=2
        a = write(
            d,
            "a.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "4 5 6 minecraft:stone minecraft:dirt\n"
            "7 8 9 minecraft:stone minecraft:gravel\n" + S.format(n=3),
        )
        b = write(
            d,
            "b.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "4 5 6 minecraft:stone minecraft:dirt\n" + S.format(n=2),
        )
        c = write(
            d,
            "c.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "4 5 6 minecraft:granite minecraft:dirt\n" + S.format(n=2),
        )
        expect("happy path N=3", main([a, b, c, "--expect-charge", "2"]), 0)
        expect("charge falls with N", main([a, b, "--expect-charge", "2"]), 0)
        expect("wrong expectation is caught", main([a, b, c, "--expect-charge", "9"]), 1)
        expect("one reference is refused", main([a]), 2)

        # A parse that silently loses lines: the differ says 3, the file has 2.
        bad = write(
            d,
            "bad.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "4 5 6 minecraft:stone minecraft:dirt\n" + S.format(n=3),
        )
        expect("miscounted file is refused", main([bad, b]), 2)

        # A summary line has five fields too — the §10 defect. If it were
        # parsed as a position it would survive every intersection.
        sneaky = write(
            d,
            "sneaky.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "DIFF chunks=1 (a=1369 b=1369)  blocks=100 mismatches=1  biomes=4/0\n",
        )
        expect("summary line is not a position", main([sneaky, b, "--expect-charge", "1"]), 0)

        # Two windows cannot be intersected.
        other = write(
            d,
            "other.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "DIFF chunks=4 (a=4 b=4)  blocks=999 mismatches=1  biomes=4/0\n",
        )
        expect("mismatched windows refused", main([a, other]), 2)

    print("SELFTEST " + ("GREEN" if ok else "RED"))
    return 0 if ok else 1


def main(argv: list[str]) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()
    opts: dict[str, str] = {}
    flags: set[str] = set()
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
        if argv[i] in FLAGS:
            flags.add(argv[i])
            i += 1
            continue
        if argv[i].startswith("--"):
            print(f"unknown option {argv[i]}", file=sys.stderr)
            return 2
        args.append(argv[i])
        i += 1
    if len(args) < 2:
        print(
            "usage: admissible-floor.py <ref1-vs-X.pos> <ref2-vs-X.pos> [...] "
            "[--label TEXT] [--ladder] [--expect-charge N]",
            file=sys.stderr,
        )
        return 2

    ar = _load_parser()
    try:
        charge, _, _, _ = report(args, opts.get("--label", ""), "--ladder" in flags)
    except (ar.Malformed, OSError) as exc:
        print(f"ADMISSIBLE-FLOOR MALFORMED: {exc}", file=sys.stderr)
        return 2

    want = opts.get("--expect-charge")
    if want is not None and charge != int(want.replace(",", "")):
        print(
            f"ADMISSIBLE-FLOOR EXPECTATION: charge {charge:,} is not the "
            f"expected {int(want.replace(',', '')):,}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
