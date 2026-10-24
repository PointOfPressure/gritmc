#!/usr/bin/env python3
"""Charge a block to us only if it matches NEITHER of two unmodified vanilla runs.

# WHY THIS EXISTS

The twin mask (`region-differ --twin`, ORACLE rule 6) *removes* every position
the two vanilla references spell differently. That is correct — a position
vanilla cannot reproduce is not evidence about us — but it leaves a blind
spot the size of the mask, and rule 6's disqualifier fires when that blind
spot grows larger than the residual. On the r=16 overworld window it did:
floor 29,560 against a residual of 29,490, so no exactness figure from the
masked route is publishable. (Those two numbers were 35,531 and 29,927 until
2026-08-09; the reference pair that produced them was lost in the 2026-08-07
disk failure and the window was re-measured on a regenerated pair. Rule 6's
verdict is unchanged, but it was re-checked and not converted — a floor
belongs to the pair of runs that produced it.)

This script closes the blind spot instead of measuring it. For each position
`p` the two references define an **admissible set** `V(p) = {A(p), B(p)}`,
which is a set of size 2 exactly where the mask used to be and size 1
everywhere else. We are charged at `p` if and only if `ours(p) is not in
V(p)` — that is, if no vanilla run of this seed and window produced what we
produced there.

Three properties, and they are the whole argument for the method:

1. **Nothing is excluded.** Every position in the window is adjudicated, so
   the denominator is the whole window and there is no mask, no floor and
   nothing for rule 6's disqualifier to be about. Rule 6 is not overridden
   here, its premise is absent.
2. **It can only over-charge us.** With N references the admissible set is
   under-sampled — a third vanilla run might well produce our answer at a
   position the first two do not. So the count is an UPPER bound on our
   defects and the percentage is a LOWER bound on our exactness. That is the
   safe direction, and it is the direction a published figure has to err in.
3. **It improves with N where the mask degrades with N.** Adding a reference
   can only add answers to `V(p)`, so the charge is monotonically
   non-increasing in N. The twin mask does the opposite: `docs/DETERMINISM.md`
   §4 measures the floor growing 35,531 -> 73,820 from N=2 to N=6, because a
   floor is the size of a union. (That ladder was built on the reference
   ensemble lost in the 2026-08-07 disk failure and has not been re-taken; the
   monotonicity argument does not depend on the particular references.) Resolving power under this instrument never
   gets worse by sampling vanilla more.

What it does NOT claim: that we would match a third vanilla run at the
positions it clears. It says only that at those positions we produced
something a real unmodified vanilla run of this seed produced.

# THE ARITHMETIC

`|ours not in {A,B}|` is exactly the positional intersection of two unmasked
diffs, `A vs ours` and `B vs ours`:

  * where `A(p) == B(p)`, `p` is in both iff `ours(p) != A(p)`;
  * where `A(p) != B(p)`, `p` is in both iff `ours(p)` is neither.

So no third pass over the region files is needed.

# THE FILTER, AND WHY IT REFUSES

`region-differ --positions` writes position lines into the same stream as its
summary, its confusion table and its y histogram. `docs/DETERMINISM.md` §10
records what a loose filter cost the last time: `awk 'NF>=5'` let the string
`DIFF chunks=1089 (a=1369` survive five intersections and produced a
spectacular wrong answer. So the filter here is anchored, AND the script
refuses to print anything unless the number of lines it parsed equals the
`mismatches=` the differ itself reported. A parse that silently loses or
gains lines is the failure mode; it is checked rather than trusted.

usage: admissible-residual.py <A-vs-ours.pos> <B-vs-ours.pos> [--floor <A-vs-B.pos>]
                              [--expect-charge N]
exit 0 fine, 2 the inputs are malformed or disagree with their own summaries,
1 `--expect-charge` did not match.
"""

from __future__ import annotations

import re
import sys

POS = re.compile(r"^(-?\d+) (-?\d+) (-?\d+) (\S+) (\S+)$")
SUMMARY = re.compile(
    r"^DIFF chunks=(\d+) \(a=(\d+) b=(\d+)\)\s+blocks=(\d+) mismatches=(\d+)"
)


class Malformed(Exception):
    pass


def load(path: str) -> tuple[dict[tuple[int, int, int], tuple[str, str]], int, int]:
    """Return (positions, blocks, chunks), having checked the file against itself."""
    pos: dict[tuple[int, int, int], tuple[str, str]] = {}
    blocks = chunks = declared = -1
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            m = POS.match(line)
            if m:
                key = (int(m[1]), int(m[2]), int(m[3]))
                if key in pos:
                    raise Malformed(f"{path}: duplicate position {key}")
                pos[key] = (m[4], m[5])
                continue
            s = SUMMARY.match(line)
            if s:
                if declared >= 0:
                    raise Malformed(f"{path}: two DIFF summary lines")
                chunks, blocks, declared = int(s[1]), int(s[4]), int(s[5])
    if declared < 0:
        raise Malformed(
            f"{path}: no `DIFF ... mismatches=N` line. Without the differ's own "
            "counter there is nothing to check the parse against, and an "
            "unchecked parse is how the End's residual was once reported as 1."
        )
    if len(pos) != declared:
        raise Malformed(
            f"{path}: parsed {len(pos):,} position lines, the differ reported "
            f"{declared:,} mismatches. The filter and the instrument disagree; "
            "neither number is usable."
        )
    return pos, blocks, chunks


OPTIONS = ("--floor", "--expect-charge")
SELFTEST = "--selftest"


def selftest() -> int:
    """Prove this script can fail, on fixtures, in T0, with no box and no jar.

    A checker whose refusals are never exercised is the defect class this
    project has now found twenty-one times. Each case below is one way a real
    run can be wrong, and each is asserted to be caught rather than assumed to
    be. It needs no box, no jar and no build, which is why it can live in T0.
    """
    import tempfile
    from pathlib import Path

    ok = True

    def write(d: Path, name: str, body: str) -> str:
        p = d / name
        p.write_text(body, encoding="utf-8")
        return str(p)

    def expect(label: str, rc: int, want: int) -> None:
        nonlocal ok
        if rc != want:
            print(f"SELFTEST FAIL [{label}]: exit {rc}, expected {want}")
            ok = False
        else:
            print(f"SELFTEST ok   [{label}]: exit {rc}")

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        # Three positions. A and B are the references, `ours` is the third
        # token on each line. p1 we get wrong against both; p2 we match B;
        # p3 we match A (so it appears in the B file only).
        a = write(
            d,
            "a.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "4 5 6 minecraft:stone minecraft:dirt\n"
            "DIFF chunks=1 (a=1 b=1)  blocks=100 mismatches=2  biomes=4/0\n",
        )
        b = write(
            d,
            "b.pos",
            "1 2 3 minecraft:granite minecraft:air\n"
            "7 8 9 minecraft:stone minecraft:gravel\n"
            "DIFF chunks=1 (a=1 b=1)  blocks=100 mismatches=2  biomes=4/0\n",
        )
        # Exactly one position (1,2,3) is in both, so the charge is 1 of 100.
        expect("happy path", main([a, b, "--expect-charge", "1"]), 0)
        expect("wrong expectation is red", main([a, b, "--expect-charge", "2"]), 1)

        # The failure that produced a spectacular wrong answer once: a parse
        # that does not match the differ's own counter.
        bad = write(
            d,
            "bad.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "DIFF chunks=1 (a=1 b=1)  blocks=100 mismatches=2  biomes=4/0\n",
        )
        expect("parse disagrees with the differ", main([bad, b]), 2)

        nosum = write(d, "nosum.pos", "1 2 3 minecraft:stone minecraft:air\n")
        expect("no summary line to check against", main([nosum, b]), 2)

        other = write(
            d,
            "other.pos",
            "1 2 3 minecraft:stone minecraft:air\n"
            "DIFF chunks=1 (a=1 b=1)  blocks=200 mismatches=1  biomes=4/0\n",
        )
        expect("two windows cannot share an admissible set", main([a, other]), 2)

    print("SELFTEST GREEN" if ok else "SELFTEST RED")
    return 0 if ok else 1


def main(argv: list[str]) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()
    # An option and its value both leave the positional list. Filtering only
    # on the leading `--` leaves the value behind, and the value here is a
    # path, so the script sees three positionals and prints usage — which is
    # exactly how this was broken on its first run.
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
    if len(args) != 2:
        print(
            "usage: admissible-residual.py <A-vs-ours.pos> <B-vs-ours.pos> "
            "[--floor <A-vs-B.pos>] [--expect-charge N]",
            file=sys.stderr,
        )
        return 2

    def opt(name: str) -> str | None:
        return opts.get(name)

    try:
        a_pos, a_blocks, a_chunks = load(args[0])
        b_pos, b_blocks, b_chunks = load(args[1])
    except (Malformed, OSError) as exc:
        print(f"ADMISSIBLE MALFORMED: {exc}", file=sys.stderr)
        return 2

    if (a_blocks, a_chunks) != (b_blocks, b_chunks):
        print(
            "ADMISSIBLE MALFORMED: the two diffs cover different windows "
            f"({a_chunks} chunks / {a_blocks} blocks against {b_chunks} / "
            f"{b_blocks}). An admissible set has to be built over one window.",
            file=sys.stderr,
        )
        return 2

    charge = set(a_pos) & set(b_pos)
    pct = 100.0 * (1.0 - len(charge) / a_blocks)

    print(f"ADMISSIBLE window       : {a_chunks} chunks, {a_blocks:,} blocks, no mask")
    print(f"  ours != reference A   : {len(a_pos):>9,}")
    print(f"  ours != reference B   : {len(b_pos):>9,}")
    print(f"  ours matches NEITHER  : {len(charge):>9,}")
    print(f"ADMISSIBLE EXACTNESS {pct:.6f}%  ({len(charge):,} of {a_blocks:,})")

    floor_path = opt("--floor")
    if floor_path:
        try:
            fl, f_blocks, _ = load(floor_path)
        except (Malformed, OSError) as exc:
            print(f"ADMISSIBLE MALFORMED: {exc}", file=sys.stderr)
            return 2
        if f_blocks != a_blocks:
            print(
                "ADMISSIBLE MALFORMED: the floor was measured over a different "
                f"window ({f_blocks:,} blocks against {a_blocks:,})",
                file=sys.stderr,
            )
            return 2
        on_floor = charge & set(fl)
        # The whole point, stated as one number: the twin mask hid `len(fl)`
        # positions and we are actually wrong at `len(on_floor)` of them.
        print(
            f"  twin mask would hide  : {len(fl):>9,} positions "
            f"(that is the floor rule 6 fires on)"
        )
        print(
            f"  of those we get wrong : {len(on_floor):>9,} "
            f"({len(on_floor) / max(len(fl), 1) * 100:.2f}% of the blind spot)"
        )
        print(f"  twin-masked residual  : {len(charge) - len(on_floor):>9,}")

    want = opt("--expect-charge")
    if want is not None:
        if len(charge) != int(want.replace(",", "")):
            print(
                f"ADMISSIBLE RED: charge is {len(charge):,}, expected {want}",
                file=sys.stderr,
            )
            return 1
        print(f"ADMISSIBLE GREEN: charge matches the registered {want}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
