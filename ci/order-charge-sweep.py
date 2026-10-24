#!/usr/bin/env python3
"""order-charge-sweep.py — recompute the N-reference admissible charge under
every one of our chunk-visitation orders, exactly, from `.pos` files that
already exist.

`ci/order-stability.sh` asks which charged positions our order *moves*.  That
is a stability question and it cannot say whether moving helps.  This asks the
adjudicating question instead: **under order `o`, how large is the charge?**  If
some order scores far below the default, our default chunk order is itself the
defect and the delta is worth thousands of blocks; if every order scores about
the same, order is noise and the residual is not reachable this way.

It needs no new dumps because the `.pos` files are complete for this purpose:

* `<r>-vs-ours.pos` lists exactly the positions where reference `r` differs from
  our default dump inside the window.  So for any position `p` in the window
  that a given `r` does **not** list, `ref_r(p) == ours_default(p)` — which is
  what makes `V(p)`, the admissible set, recoverable everywhere rather than only
  where we already disagree.
* `default-vs-<o>.pos` lists exactly the positions order `o` moves, with our
  default answer in field 4 and order `o`'s answer in field 5.

so, over the union of every position any file mentions,

    V(p)        = {values the listing references give} u {ours_default(p)}
                  if fewer than all N references list p
    ours_o(p)   = order o's value if `default-vs-o.pos` lists p, else default
    charge(o)   = |{ p : ours_o(p) not in V(p) }|

Positions no file mentions have every reference and every order agreeing with
our default and can never be charged, so the union is the whole domain.

`charge(default)` is printed first and is the self-check: it must equal the
charge the N-reference instrument reports, or the reconstruction above is
wrong.  Pass `--expect-default N` to make that an exit code.

`--with-u` additionally loads every `<r1>-vs-<r2>.pos` pair and splits each
order's fixes and breaks by whether they fall inside `U`, the union of every
pairwise disagreement among the references — the region where vanilla is not
unanimous.  `docs/DETERMINISM.md` §14: *the part outside U is your result; the
part inside is a coin-flip you happened to win.*  A fix outside `U` is a
position where all N references agree, our default disagreed, and the order
now writes exactly what they agree on.

usage:
  order-charge-sweep.py --refdir <dir> --orderdir <dir> [--refs a,b,...]
                        [--orders zx,...] [--expect-default N] [--with-u]
"""

from __future__ import annotations

import os
import sys
from pathlib import Path


def _pack(x: int, y: int, z: int) -> int:
    return (((x + (1 << 20)) << 21) | (z + (1 << 20))) << 21 | (y + (1 << 20))


NAMES: dict[str, int] = {}


def nid(name: str) -> int:
    i = NAMES.get(name)
    if i is None:
        i = len(NAMES)
        NAMES[name] = i
    return i


def read_pos(path: str):
    with open(path, "r") as fh:
        for line in fh:
            f = line.split()
            if len(f) < 5:
                continue
            try:
                x = int(f[0]); y = int(f[1]); z = int(f[2])
            except ValueError:
                continue
            yield _pack(x, y, z), nid(f[3]), nid(f[4])


def die(msg: str) -> int:
    print(f"order-charge-sweep: {msg}", file=sys.stderr)
    return 2


def selftest() -> int:
    """The reconstruction's argument, mechanised.

    Every number this script prints rests on one inference: *a reference that
    does not list `p` in `<r>-vs-ours.pos` agrees with our default at `p`.*
    That is what lets `V(p)` be recovered at positions where we currently agree
    with vanilla, and it is the only place the whole method can be wrong
    without being obviously wrong.  So the fixture below contains a position of
    each kind, including the one that exercises exactly that inference: `P3`,
    which no reference lists and which an order moves — the reconstruction must
    charge it, because moving it moves us off an answer every reference gave.
    """
    import shutil
    import tempfile

    d = Path(tempfile.mkdtemp(prefix="ordsweep-selftest-"))
    fails = 0
    try:
        def w(name: str, body: str) -> None:
            (d / name).write_text(body)

        # P1  every reference says stone, we say dirt      -> charged
        # P2  a says stone, b says granite, we say dirt     -> charged
        # P3  no reference lists it: all agree with us      -> not charged
        # P4  every reference says stone, we say dirt       -> charged
        w("a-vs-ours.pos",
          "DIFF chunks=1 blocks=1 mismatches=3\n"
          "1 1 1 minecraft:stone minecraft:dirt\n"
          "2 2 2 minecraft:stone minecraft:dirt\n"
          "4 4 4 minecraft:stone minecraft:dirt\n")
        w("b-vs-ours.pos",
          "DIFF chunks=1 blocks=1 mismatches=3\n"
          "1 1 1 minecraft:stone minecraft:dirt\n"
          "2 2 2 minecraft:granite minecraft:dirt\n"
          "4 4 4 minecraft:stone minecraft:dirt\n")
        # zx fixes P1 (to stone, which both give) and P2 (to granite, which b
        # gives), leaves P4 alone, and breaks P3 by moving it off the answer
        # every reference agreed with.
        w("default-vs-zx.pos",
          "DIFF chunks=1 blocks=1 mismatches=3\n"
          "1 1 1 minecraft:dirt minecraft:stone\n"
          "2 2 2 minecraft:dirt minecraft:granite\n"
          "3 3 3 minecraft:sand minecraft:gravel\n")
        # `reverse` moves nothing, so it must score exactly the default.
        w("default-vs-reverse.pos", "DIFF chunks=1 blocks=1 mismatches=0\n")
        w("a-vs-b.pos",
          "DIFF chunks=1 blocks=1 mismatches=1\n"
          "2 2 2 minecraft:stone minecraft:granite\n")

        import io
        import contextlib

        def run(args):
            # stderr too: the negative cases below are *supposed* to print a
            # refusal, and a self-test that is green while printing red text
            # is a self-test nobody reads.
            buf, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(err):
                rc = main(args)
            return rc, buf.getvalue()

        base_args = [str(d), "--refdir", str(d), "--orderdir", str(d),
                     "--refs", "a,b", "--orders", "zx,reverse"]
        base_args = base_args[1:]

        def expect(label: str, cond: bool, got: str = "") -> None:
            nonlocal fails
            if not cond:
                fails += 1
                print(f"  FAIL {label} {got}", file=sys.stderr)

        rc, out = run(base_args + ["--with-u"])
        expect("runs clean", rc == 0, f"rc={rc}")
        # charge(default) = P1, P2, P4
        dline = [ln for ln in out.splitlines() if ln.strip().startswith("default ")]
        expect("default charge is 3",
               bool(dline) and dline[0].split()[1] == "3", str(dline))
        # charge(zx) = P4 (untouched) + P3 (broken) = 2, fixing 2, breaking 1
        line = [ln for ln in out.splitlines() if ln.strip().startswith("zx ")]
        expect("zx row present", len(line) == 1, out)
        if line:
            f = line[0].split()
            expect("zx charge is 2", f[1] == "2", line[0])
            expect("zx delta is -1", f[2] == "-1", line[0])
            expect("zx fixes 2", f[3] == "2", line[0])
            expect("zx breaks 1", f[4] == "1", line[0])
            # P2 is the only position inside U, so of the two fixes exactly one
            # is outside it, and the single break is outside it too.
            expect("zx fixes 1 outside U", f[5] == "1", line[0])
            expect("zx breaks 1 outside U", f[6] == "1", line[0])
        rline = [ln for ln in out.splitlines() if ln.strip().startswith("reverse ")]
        expect("an order that moves nothing scores the default",
               bool(rline) and rline[0].split()[1] == "3", str(rline))
        expect("charged under every order is 1",
               "charged under EVERY order" in out
               and out.split("charged under EVERY order")[1].split()[0] == "1", out)

        rc, _ = run(base_args + ["--expect-default", "3"])
        expect("right expectation is green", rc == 0, f"rc={rc}")
        rc, _ = run(base_args + ["--expect-default", "4"])
        expect("wrong expectation is red", rc == 1, f"rc={rc}")
        rc, _ = run([ "--refdir", str(d), "--orderdir", str(d),
                     "--refs", "a,b,zzz", "--orders", "zx"])
        expect("a missing reference is refused", rc == 2, f"rc={rc}")
    finally:
        shutil.rmtree(d, ignore_errors=True)
    if fails:
        print(f"order-charge-sweep selftest: {fails} failure(s)", file=sys.stderr)
        return 1
    print("order-charge-sweep selftest: ok")
    return 0


def main(argv: list[str]) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()

    def opt(name: str, default=None):
        if name in argv:
            i = argv.index(name)
            if i + 1 >= len(argv):
                raise SystemExit(die(f"{name} needs a value"))
            return argv[i + 1]
        return default

    refdir = opt("--refdir")
    orderdir = opt("--orderdir")
    if not refdir or not orderdir:
        return die("need --refdir and --orderdir")
    refs = opt("--refs", "a,b,c,d,e,f,g,h").split(",")
    orders = opt("--orders", "zx,zx_rev,zx_zrev,reverse,spiral,spiral_in,spawn,diag").split(",")

    ours_default: dict[int, int] = {}
    van: dict[int, set[int]] = {}
    listed_by: dict[int, int] = {}

    for r in refs:
        p = os.path.join(refdir, f"{r}-vs-ours.pos")
        if not os.path.exists(p):
            return die(f"missing {p}")
        for k, rv, ov in read_pos(p):
            ours_default[k] = ov
            s = van.get(k)
            if s is None:
                van[k] = {rv}
            else:
                s.add(rv)
            listed_by[k] = listed_by.get(k, 0) + 1

    moved: dict[str, dict[int, int]] = {}
    for o in orders:
        p = os.path.join(orderdir, f"default-vs-{o}.pos")
        if not os.path.exists(p):
            return die(f"missing {p}")
        m: dict[int, int] = {}
        for k, dv, ov in read_pos(p):
            ours_default.setdefault(k, dv)
            m[k] = ov
        moved[o] = m

    # Complete V(p): a reference that does not list p agrees with our default.
    n = len(refs)
    for k, d in ours_default.items():
        s = van.get(k)
        if s is None:
            van[k] = {d}
        elif listed_by.get(k, 0) < n:
            s.add(d)

    charged: dict[str, set[int]] = {}
    base = {k for k, d in ours_default.items() if d not in van[k]}
    charged["default"] = base
    for o in orders:
        m = moved[o]
        c = set()
        for k, d in ours_default.items():
            v = m.get(k, d)
            if v not in van[k]:
                c.add(k)
        charged[o] = c

    U: set[int] = set()
    if "--with-u" in argv:
        for i, r1 in enumerate(refs):
            for r2 in refs[i + 1:]:
                q = os.path.join(refdir, f"{r1}-vs-{r2}.pos")
                if not os.path.exists(q):
                    q = os.path.join(refdir, f"{r2}-vs-{r1}.pos")
                if not os.path.exists(q):
                    continue
                for k, _a, _b in read_pos(q):
                    U.add(k)

    print(f"DOMAIN  positions any file mentions   {len(ours_default):>9,}")
    print(f"  refs={len(refs)} orders={len(orders)}")
    if U:
        print(f"  U   vanilla not unanimous          {len(U):>9,}")
    print()
    b = len(base)
    head = f"  {'order':<12}{'charge':>10}   {'delta vs default':>17}   {'fixed':>7} {'broken':>7}"
    if U:
        head += f"   {'fix!U':>7} {'brk!U':>7}"
    print(head)
    print(f"  {'default':<12}{b:>10,}   {'-':>17}   {'-':>7} {'-':>7}")
    for o in orders:
        c = charged[o]
        row = (f"  {o:<12}{len(c):>10,}   {len(c) - b:>+17,}"
               f"   {len(base - c):>7,} {len(c - base):>7,}")
        if U:
            row += f"   {len((base - c) - U):>7,} {len((c - base) - U):>7,}"
        print(row)
    print()
    inter = set.intersection(*[charged[o] for o in ["default"] + orders])
    union = set.union(*[charged[o] for o in ["default"] + orders])
    best = min(["default"] + orders, key=lambda o: len(charged[o]))
    print(f"  charged under EVERY order            {len(inter):>9,}"
          f"   <- no order of ours can fix these")
    print(f"  charged under SOME order             {len(union):>9,}")
    print(f"  best single order                    {best} at {len(charged[best]):,}")
    print()

    # What kind of block does the best order actually fix?  A repair that only
    # moves the stone-variant family is moving the part of the residual §14
    # already attributes to vanilla's own decoration noise; one that moves
    # families outside it is doing something else.
    kinds_for = opt("--kinds-for")
    if kinds_for:
        if kinds_for not in charged:
            return die(f"--kinds-for {kinds_for} is not one of the orders")
        inv2 = {v: k for k, v in NAMES.items()}
        m = moved.get(kinds_for, {})
        for label, pts in (("FIXED", base - charged[kinds_for]),
                           ("BROKEN", charged[kinds_for] - base)):
            hist: dict[tuple[int, int], int] = {}
            for k in pts:
                was = ours_default[k]
                now = m.get(k, was)
                hist[(was, now)] = hist.get((was, now), 0) + 1
            print(f"  {label} by {kinds_for}: {len(pts):,}"
                  f"   (our default -> our {kinds_for})")
            for (a_, b_), n in sorted(hist.items(), key=lambda kv: -kv[1])[:12]:
                print(f"    {inv2[a_]:<34} -> {inv2[b_]:<34} {n:>7,}")
            print()

    dump = opt("--dump-charge")
    if dump:
        # Unpack: the packing is two 21-bit shifts over a 2^20 bias.
        with open(dump, "w") as fh:
            for k in sorted(base):
                y = (k & ((1 << 21) - 1)) - (1 << 20)
                z = ((k >> 21) & ((1 << 21) - 1)) - (1 << 20)
                x = (k >> 42) - (1 << 20)
                fh.write(f"{x} {y} {z}\n")
        print(f"  wrote {len(base):,} charged positions to {dump}")

    want = opt("--expect-default")
    if want is not None and b != int(want.replace(",", "")):
        print(f"SWEEP RED: charge(default) is {b:,}, expected {want}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
