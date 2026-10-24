#!/usr/bin/env python3
"""Decompose ONE `vanilla -> ours` confusion row by chunk order, by chunk and
by y band — and ask the vanilla-side question a confusion table cannot.

# WHY THIS EXISTS

`ci/floor-census.py --adjudicate` ranks a residual by ordered pair and puts
each row beside its own floor. That is where an item comes from. It cannot say
*why* a row is charged, and on this project the commonest why is not a block
placement defect at all: it is which chunk decorated first (ORACLE rule 29,
`docs/DETERMINISM.md` §15/§16).

`ci/order-charge-sweep.py` answers the aggregate version of that question —
what does the whole window's charge become under each of our orders — from
`.pos` files that already exist. This asks the same question of a **single
row**, and adds the three decompositions the aggregate one has no place for:

1. **by order** — the row's charge under each of two dumps of ours, with the
   positional set relations between them. A row that is charged under one
   order and *empty* under the other is a sequencing statement, not a
   placement one, and no amount of reading the feature's bytecode will show
   it;
2. **by chunk and by y band** — a row concentrated in one chunk is a
   different finding from one spread over the window, and the concentrated
   case names the chunk to trace;
3. **by what VANILLA does at the same positions under a different harvest
   order** — the control that decides whether the order sensitivity is ours
   alone. `AGENT-BRIEF.md` is explicit that "we are order-dependent where
   vanilla is not" is a hypothesis that has to clear vanilla's own order
   floor before it is worth an hour, and until this script that comparison
   was being made by eye.

# THE THIRD ONE IS THE LOAD-BEARING ONE, AND IT NEEDS NO NEW REFERENCE

To learn what a `rows_rev` reference wrote at position `p`, you do not need a
dump of it: you need one `.pos` file. `<anchor>-vs-<rr>.pos` lists exactly the
positions where the anchor and the cross-order reference differ, with the
anchor's spelling in field 4 and the cross-order one's in field 5. So for any
`p` the file does **not** list, the cross-order reference wrote what the
anchor wrote. The anchor's spelling at a charged position is already known —
it is the `vanilla` half of the row — so a single membership test answers
"does vanilla's other harvest order agree with itself here?"

That is the whole control, it costs one file read, and its two outcomes point
opposite ways:

* **vanilla agrees with itself across orders at these positions** — the
  positions are order-INVARIANT in vanilla. Then a row of ours that appears
  and disappears with our chunk order is ours, and it is a real defect;
* **vanilla disagrees with itself across orders at these positions** — the
  positions are order-VARIANT in vanilla too, our disagreement is inside a
  region where vanilla is not unanimous, and the row is corroboration at best.

# INPUT

Two `<ref>-vs-<cand>.pos` matrices over the same reference set and the same
window, one per chunk order of ours, produced VANILLA FIRST. Optionally a
directory holding `<anchor>-vs-<cross>.pos` for the cross-order control; it
may be the same directory or a shared one, and it is read, never written.

The parse is `admissible-residual.py`'s, borrowed rather than copied, so a
parse that does not equal the differ's own `mismatches=` counter is refused
(`docs/DETERMINISM.md` §10).

# WHAT IT DOES NOT DO

It subtracts nothing from anything. A floor is quoted beside a charge and
never taken off it. This script does not know about floors at all —
`ci/floor-census.py` owns that — and it prints charges, set sizes and
positions only.

usage:
  order-pair-decompose.py --a <posdir> --b <posdir> --refs c,d,e --cand ours
                          [--label-a NAME] [--label-b NAME]
                          [--pair 'sulfur->cinnabar'] [--top N]
                          [--cross-dir <dir> --cross-anchor c --cross-refs rr-a,rr-b]
                          [--chunks N] [--expect-a N] [--expect-b N]
  order-pair-decompose.py --selftest
exit 0 fine, 1 an `--expect-*` did not match, 2 malformed or missing inputs.
"""

from __future__ import annotations

import collections
import importlib.util
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
_PARSER = None


def _parser():
    """Borrow `load()` from admissible-residual.py, cached for the reason its
    two siblings cache it: two module objects make two `Malformed` classes and
    an `except` in one stops catching a `raise` from the other."""
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


def short(name: str) -> str:
    return name[10:] if name.startswith("minecraft:") else name


def load_arm(posdir: str, refs: list[str], cand: str):
    """{ref: {pos: (ref_spelling, cand_spelling)}} for one chunk order of ours."""
    ar = _parser()
    out = {}
    blocks = chunks = None
    for r in refs:
        p = os.path.join(posdir, f"{r}-vs-{cand}.pos")
        pos, b, c = ar.load(p)
        if blocks is None:
            blocks, chunks = b, c
        elif (b, c) != (blocks, chunks):
            raise ar.Malformed(
                f"{p} covers {c} chunks / {b:,} blocks against {chunks} / "
                f"{blocks:,}. One window, or the intersection is meaningless."
            )
        out[r] = pos
    if blocks is None:
        raise ar.Malformed("no position files were loaded")
    return out, blocks, chunks


def charge(arm: dict, refs: list[str]) -> set:
    """The admissible charge: positions where our answer matches NO reference."""
    return set.intersection(*[set(arm[r]) for r in refs])


def rows_for(arm: dict, refs: list[str], positions) -> collections.Counter:
    """Ordered `vanilla -> ours` pairs over a position set. The vanilla half is
    taken from the anchor reference; every reference in the charge disagrees
    with us there, but they need not agree with each other, so the pair is
    reported with the anchor's spelling and a `*` when the references differ."""
    anchor = refs[0]
    c: collections.Counter = collections.Counter()
    for p in positions:
        spellings = {arm[r][p][0] for r in refs}
        van = short(arm[anchor][p][0]) + ("" if len(spellings) == 1 else "*")
        c[(van, short(arm[anchor][p][1]))] += 1
    return c


def select(arm: dict, refs: list[str], positions, want: tuple[str, str]) -> set:
    anchor = refs[0]
    out = set()
    for p in positions:
        if (short(arm[anchor][p][0]), short(arm[anchor][p][1])) == want:
            out.add(p)
    return out


def band(y: int) -> str:
    lo = (y // 16) * 16
    return f"{lo:>4}..{lo + 15:<4}"


def histogram(title: str, counts: collections.Counter, limit: int) -> None:
    print(f"    {title}")
    if not counts:
        print("      (empty)")
        return
    for key, n in counts.most_common(limit):
        print(f"      {n:>7,}  {key}")
    if len(counts) > limit:
        print(f"      … {len(counts) - limit:,} more")


def cross_control(crossdir: str, anchor: str, crossrefs: list[str], positions) -> None:
    """Does vanilla agree with ITSELF at these positions across harvest orders?

    A position the `<anchor>-vs-<cross>.pos` file does not list is one where
    the cross-order reference wrote exactly what the anchor wrote."""
    ar = _parser()
    print(f"    cross-order vanilla control over {len(positions):,} positions")
    print(f"      anchor {anchor} (the harvest order the charge was taken in)")
    for x in crossrefs:
        p = os.path.join(crossdir, f"{anchor}-vs-{x}.pos")
        if not os.path.exists(p):
            print(f"      {x:<6} MISSING {p} — no control taken")
            continue
        pos, _, _ = ar.load(p)
        moved = {q for q in positions if q in pos}
        spell: collections.Counter = collections.Counter()
        for q in moved:
            spell[(short(pos[q][0]), short(pos[q][1]))] += 1
        pct = 100.0 * len(moved) / len(positions) if positions else 0.0
        print(
            f"      {x:<6} vanilla's other order differs at "
            f"{len(moved):,}/{len(positions):,}  ({pct:.2f}%)"
        )
        for key, n in spell.most_common(6):
            print(f"               {n:>7,}  {key[0]} -> {key[1]}")


def run(argv_a, argv_b, refs, cand, label_a, label_b, pair, top, chunks_n,
        crossdir, crossanchor, crossrefs, expect_a, expect_b) -> int:
    ar = _parser()
    try:
        arm_a, blocks, chunks = load_arm(argv_a, refs, cand)
        arm_b, blocks_b, chunks_b = load_arm(argv_b, refs, cand)
    except (ar.Malformed, OSError) as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        return 2
    if (blocks, chunks) != (blocks_b, chunks_b):
        print(
            f"REFUSED: the two arms cover different windows "
            f"({chunks}/{blocks:,} against {chunks_b}/{blocks_b:,})",
            file=sys.stderr,
        )
        return 2

    ca, cb = charge(arm_a, refs), charge(arm_b, refs)
    print(f"  window {chunks:,} chunks / {blocks:,} blocks, N={len(refs)} refs "
          f"({','.join(refs)}), candidate `{cand}`")
    print(f"  CHARGE  {label_a:<10} {len(ca):>9,}")
    print(f"  CHARGE  {label_b:<10} {len(cb):>9,}")
    print(f"  both orders (order-invariant core)  {len(ca & cb):>9,}")
    print(f"  {label_a} only {len(ca - cb):>9,}     {label_b} only {len(cb - ca):>9,}")

    rows_a, rows_b = rows_for(arm_a, refs, ca), rows_for(arm_b, refs, cb)
    keys = set(rows_a) | set(rows_b)
    print(f"\n  RANKED ROWS — charge under each order "
          f"(`*` = the references do not agree on the vanilla spelling)")
    print(f"      {label_a:>10} {label_b:>10}   vanilla -> ours")
    for k in sorted(keys, key=lambda k: -max(rows_a[k], rows_b[k]))[:top]:
        print(f"      {rows_a[k]:>10,} {rows_b[k]:>10,}   {k[0]} -> {k[1]}")

    rc = 0
    if pair:
        want = tuple(x.strip() for x in pair.split("->"))
        if len(want) != 2:
            print(f"REFUSED: --pair wants 'vanilla->ours', got {pair!r}", file=sys.stderr)
            return 2
        pa, pb = select(arm_a, refs, ca, want), select(arm_b, refs, cb, want)
        print(f"\n  ROW `{want[0]} -> {want[1]}` DECOMPOSED")
        print(f"    {label_a:<10} {len(pa):>7,}      {label_b:<10} {len(pb):>7,}")
        print(f"    in both {len(pa & pb):>7,}   {label_a} only {len(pa - pb):>7,}   "
              f"{label_b} only {len(pb - pa):>7,}")
        for lbl, s in ((label_a, pa), (label_b, pb)):
            if not s:
                continue
            print(f"\n    --- {lbl} ---")
            histogram(
                "by chunk (x>>4, z>>4)",
                collections.Counter((p[0] >> 4, p[2] >> 4) for p in s),
                chunks_n,
            )
            histogram(
                "by y band",
                collections.Counter(band(p[1]) for p in s),
                12,
            )
        if crossdir and crossanchor and crossrefs:
            print()
            cross_control(crossdir, crossanchor, crossrefs, pa | pb)
        if expect_a is not None and len(pa) != expect_a:
            print(f"  EXPECT-A MISMATCH: {len(pa)} != {expect_a}", file=sys.stderr)
            rc = 1
        if expect_b is not None and len(pb) != expect_b:
            print(f"  EXPECT-B MISMATCH: {len(pb)} != {expect_b}", file=sys.stderr)
            rc = 1
    return rc


# ---------------------------------------------------------------------------
# SELF-TEST. A gate that cannot fail is not a gate, so every case below is a
# mutation: the fixture is built so that the wrong arithmetic gives a
# different, checkable number.
# ---------------------------------------------------------------------------
def _write(path, mismatches, lines):
    with open(path, "w", encoding="utf-8") as fh:
        for x, y, z, v, o in lines:
            fh.write(f"{x} {y} {z} minecraft:{v} minecraft:{o}\n")
        fh.write(
            f"DIFF chunks=4 (a=4 b=4)  blocks=1024 mismatches={mismatches}"
            "  biomes=0/0\n"
        )


def selftest() -> int:
    import tempfile

    fails = 0

    def expect(what, got, want):
        nonlocal fails
        if got != want:
            print(f"SELFTEST FAIL [{what}]: {got!r} != {want!r}")
            fails += 1
        else:
            print(f"SELFTEST ok   [{what}] {want!r}")

    with tempfile.TemporaryDirectory() as d:
        a, b, x = os.path.join(d, "a"), os.path.join(d, "b"), os.path.join(d, "x")
        for p in (a, b, x):
            os.makedirs(p)
        # ARM A. Three references. Position (0,0,0) is charged by all three —
        # it is in the charge. (1,0,0) is charged by two of three, so it is
        # NOT: an intersection, never a union.
        _write(f"{a}/c-vs-ours.pos", 2, [(0, 0, 0, "sulfur", "cinnabar"),
                                         (1, 0, 0, "sulfur", "cinnabar")])
        _write(f"{a}/d-vs-ours.pos", 2, [(0, 0, 0, "sulfur", "cinnabar"),
                                         (1, 0, 0, "sulfur", "cinnabar")])
        _write(f"{a}/e-vs-ours.pos", 1, [(0, 0, 0, "sulfur", "cinnabar")])
        # ARM B. The same row is EMPTY: our other order agrees with vanilla at
        # (0,0,0) and is charged somewhere else entirely, on another row.
        for r in ("c", "d", "e"):
            _write(f"{b}/{r}-vs-ours.pos", 1, [(9, 9, 9, "tuff", "granite")])
        # CROSS-ORDER CONTROL. The anchor `c` and the cross-order reference
        # `rr-a` differ at (5,5,5) and NOT at (0,0,0) — so vanilla agrees with
        # itself across orders at the charged position, and the control must
        # report 0 of 1, not 1 of 1.
        _write(f"{x}/c-vs-rr-a.pos", 1, [(5, 5, 5, "sulfur", "cinnabar")])

        refs = ["c", "d", "e"]
        import io
        import contextlib

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = run(a, b, refs, "ours", "x-major", "zx", "sulfur->cinnabar",
                     10, 8, x, "c", ["rr-a"], None, None)
        out = buf.getvalue()
        expect("selftest run rc", rc, 0)
        expect("charge is an intersection, not a union",
               "CHARGE  x-major            1" in out, True)
        # Whitespace-insensitive on purpose: an assertion that pins column
        # widths fails on a cosmetic edit and teaches the next person to
        # delete it rather than to read it.
        import re

        row = re.search(
            r"ROW `sulfur -> cinnabar` DECOMPOSED\s*\n\s*"
            r"x-major\s+(\d+)\s+zx\s+(\d+)",
            out,
        )
        expect("the row is charged under one order and EMPTY under the other",
               (row[1], row[2]) if row else None, ("1", "0"))
        expect("order-invariant core is 0", "core)          0" in out, True)
        expect("cross-order control finds vanilla stable",
               "differs at 0/1" in out, True)
        expect("by-chunk decomposition names the chunk",
               "(0, 0)" in out, True)

        # --expect-a is a real gate: give it the wrong number and it must
        # return 1 rather than print and pass.
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc_bad = run(a, b, refs, "ours", "x-major", "zx", "sulfur->cinnabar",
                         10, 8, None, None, None, 99, None)
        expect("--expect-a mismatch is an exit code", rc_bad, 1)

        # A parse that does not equal the differ's own counter is refused.
        _write(f"{a}/c-vs-ours.pos", 77, [(0, 0, 0, "sulfur", "cinnabar")])
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc_mal = run(a, b, refs, "ours", "x-major", "zx", None, 10, 8,
                         None, None, None, None, None)
        expect("a miscounted .pos file is refused", rc_mal, 2)

    print("SELFTEST FAILURES:", fails)
    return 1 if fails else 0


def main(argv: list[str]) -> int:
    if argv and argv[0] == "--selftest":
        return selftest()
    opt = {
        "--a": None, "--b": None, "--refs": None, "--cand": "ours",
        "--label-a": "order-A", "--label-b": "order-B", "--pair": None,
        "--top": "20", "--chunks": "12", "--cross-dir": None,
        "--cross-anchor": None, "--cross-refs": None,
        "--expect-a": None, "--expect-b": None,
    }
    i = 0
    while i < len(argv):
        if argv[i] not in opt:
            print(f"unknown option {argv[i]!r}\n{__doc__}", file=sys.stderr)
            return 2
        if i + 1 >= len(argv):
            print(f"{argv[i]} wants a value", file=sys.stderr)
            return 2
        opt[argv[i]] = argv[i + 1]
        i += 2
    if not opt["--a"] or not opt["--b"] or not opt["--refs"]:
        print(__doc__, file=sys.stderr)
        return 2
    return run(
        opt["--a"], opt["--b"], opt["--refs"].split(","), opt["--cand"],
        opt["--label-a"], opt["--label-b"], opt["--pair"],
        int(opt["--top"]), int(opt["--chunks"]),
        opt["--cross-dir"], opt["--cross-anchor"],
        opt["--cross-refs"].split(",") if opt["--cross-refs"] else None,
        int(opt["--expect-a"]) if opt["--expect-a"] else None,
        int(opt["--expect-b"]) if opt["--expect-b"] else None,
    )


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
