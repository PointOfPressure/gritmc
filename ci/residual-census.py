#!/usr/bin/env python3
"""Decompose an admissible charge POSITIONALLY — by block pair, by y band, by
chunk — and put the SAME instrument's innocent floor beside every lump.

# WHY THIS EXISTS

`ci/window-resolving-power.py` already ranks a charge by `vanilla -> ours`
block pair with a per-pair floor, and that table is what named the `blue_ice`
absence on the seed-12345 r=16 window. It is also the only decomposition this
project has, and a block pair is the *weakest* of the three a residual can be
cut along:

* a **block pair** says what we spelled wrongly, not where. `packed_ice ->
  water` was one number covering two unrelated mechanisms until a positional
  census split it.
* a **y band** separates a surface-rule defect from a carver one from a
  decoration one without reading a line of code, because the stages do not
  overlap in y the way they overlap in block kind.
* a **chunk** map says whether a lump is one structure in one place or a thin
  film over the whole window. Those two have opposite next moves and a
  confusion table cannot tell them apart. The seed-12345 residual has now
  decomposed differently four times in a week, every time because somebody
  named it from a confusion table.
* a **ring** — chunk distance from the window's own edge — says whether a
  residual is about the world or about the window. Our dump generates a
  fixed margin outside the square it writes, so the outermost chunks are the
  ones whose decoration inputs are least complete, and a charge concentrated
  there is measuring the margin rather than the port. This is ORACLE rule 14
  (widen the context until the answer stops changing) turned into a number
  you can read off an existing diff instead of a second dump you have to pay
  for. It is added because on the seed-12345 r=16 window the outermost ring
  alone turned out to hold 46.6% of the charge, and no cut this project had
  could see it: a block pair, a y band and a chunk list all spread that lump
  across themselves.

So this script cuts the same charge four ways. It computes nothing new about
*what* is charged — the charge set is exactly `ci/admissible-floor.py`'s
intersection, and that is deliberate: a second implementation of the charge
would be a second thing to be wrong.

# EVERY LUMP CARRIES ITS OWN FLOOR, AND THE FLOOR IS PER-LUMP

The aggregate floor of a window is not the floor of a band inside it. On the
20260722 window the aggregate floor was unusable while `grass_block -> dirt`
had a floor of 1 (`docs/DETERMINISM.md` §17). The same is true across y and
across chunks, and more sharply: an innocent vanilla run's disagreements are
concentrated in exactly the y bands and chunks where lakes, speleothems and
overlapping ore blobs read the world before they write it, so a *flat* floor
would clear noise in one band and convict signal in another.

So for every band and every chunk the floor printed beside our charge is the
WORST charge the same instrument lays on a known-innocent candidate in that
same band or chunk — each reference in turn substituted for our dump and
adjudicated against N of the others, exactly as `ci/window-resolving-power.py`
does for block pairs.

A lump at or below its own floor is **not a defect yet**. Saying so is the
result; it is not a weaker version of finding one.

# WHAT IT REFUSES

* fewer than two references, or a candidate with no `.pos` against some
  reference — an admissible set built over a subset of the ensemble is not
  the set its N claims;
* `.pos` files whose windows differ in chunks or blocks;
* a parse that disagrees with the differ's own `mismatches=` counter — the
  guard is `ci/admissible-residual.py`'s `load()`, borrowed rather than
  re-implemented (`docs/DETERMINISM.md` §10).

Parsing, the window check and the `Malformed` type all come from
`ci/admissible-residual.py`. `ci/window-resolving-power.py` explains at length
why that module is loaded exactly once and cached; the same reasoning applies
here and the same cache is used.

`--window` is not inferred from the positions and that is deliberate: a ring
whose positions all agree contributes no lines, so an inferred window silently
shrinks to the charged region and every ring index shifts. Passing it means a
wrong one can be caught, and it is: a charged position outside the declared
window is a refusal, not a warning.

usage: residual-census.py <posdir> --refs c,d,e,f,g --cand zx
                          [--band 16] [--top 20] [--label TEXT]
                          [--window x1 z1 x2 z2] [--expect-charge N]
       residual-census.py --selftest

exit 0 fine, 1 `--expect-charge` did not match, 2 malformed or missing inputs.
"""

from __future__ import annotations

import collections
import importlib.util
import itertools
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
_PARSER = None


def _parser():
    """Borrow `load()` from admissible-residual.py. Cached, and the cache is
    load-bearing: loading the module twice makes two distinct `Malformed`
    classes and an `except` in one place stops catching a `raise` from the
    other. `ci/admissible-floor.py`'s self-test caught exactly that."""
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


def load_matrix(posdir: str, refs: list[str], cands: list[str]):
    """Load every `<ref>-vs-<cand>.pos` in the matrix, checked against itself.

    Returns (files, blocks, chunks) where `files[(ref, cand)]` is the position
    dict. A missing file is fatal: an admissible set built over the pairs that
    happen to exist is not the set its N claims to be.
    """
    ar = _parser()
    files: dict[tuple[str, str], dict] = {}
    blocks = chunks = -1
    for ref in refs:
        for cand in cands:
            if ref == cand:
                continue
            path = os.path.join(posdir, f"{ref}-vs-{cand}.pos")
            if not os.path.exists(path):
                raise ar.Malformed(f"missing {path}")
            pos, b, c = ar.load(path)
            if blocks < 0:
                blocks, chunks = b, c
            elif (b, c) != (blocks, chunks):
                raise ar.Malformed(
                    f"{path} covers {c} chunks / {b:,} blocks against "
                    f"{chunks} / {blocks:,}. A census has to be over one window."
                )
            files[(ref, cand)] = pos
    return files, blocks, chunks


def charge_set(files, subset: list[str], cand: str) -> set:
    """The admissible charge: positions where `cand` matches NO reference in
    `subset`. Exactly `ci/admissible-floor.py`'s intersection."""
    out: set | None = None
    for ref in subset:
        keys = set(files[(ref, cand)])
        out = keys if out is None else (out & keys)
    return out or set()


def band_of(y: int, width: int) -> int:
    """Lowest y of the band holding `y`. Floor division, so negative y — the
    overworld starts at -64 — lands in the band below, not above."""
    return (y // width) * width


def ring_of(cx: int, cz: int, window: tuple[int, int, int, int] | None) -> int:
    """Chunk distance from the nearest edge of the window. 0 is the outermost
    ring. `None` window means the cut is not available and every position
    lands in ring -1, which the report then suppresses."""
    if window is None:
        return -1
    x1, z1, x2, z2 = window
    return min(cx - x1, x2 - cx, cz - z1, z2 - cz)


def census(keys: set, files, subset: list[str], cand: str, width: int, window=None):
    """Cut one charge set four ways. Returns (pairs, bands, chunks_, rings).

    `pairs` keys on the vanilla spelling as the references SPELL it, which is
    a set where they disagree; that is rendered as `{a|b}` so a split position
    can never be silently merged into a unanimous pair.
    """
    pairs: collections.Counter = collections.Counter()
    bands: collections.Counter = collections.Counter()
    chunks_: collections.Counter = collections.Counter()
    rings: collections.Counter = collections.Counter()
    for p in keys:
        x, y, z = p
        ours = files[(subset[0], cand)][p][1]
        spellings = sorted({files[(r, cand)][p][0] for r in subset})
        van = spellings[0] if len(spellings) == 1 else "{" + "|".join(spellings) + "}"
        pairs[(van, ours)] += 1
        bands[band_of(y, width)] += 1
        cx, cz = x >> 4, z >> 4
        chunks_[(cx, cz)] += 1
        r = ring_of(cx, cz, window)
        if window is not None and r < 0:
            raise _parser().Malformed(
                f"charged position {p} is in chunk {cx},{cz}, outside the "
                f"declared window {window}. Either the window is wrong or the "
                "diff was taken over a different bbox; neither ring index "
                "below would mean anything."
            )
        rings[r] += 1
    return pairs, bands, chunks_, rings


def innocent_censuses(files, refs: list[str], n: int, width: int, window=None):
    """The same three cuts for every known-innocent candidate.

    Each reference in turn is the candidate, adjudicated against `n` of the
    others — every subset of that size, so the floor spans the choice of
    reference subset as well as the choice of candidate. That is the quantity
    `docs/DETERMINISM.md` §14 calls the innocent floor, cut per lump.
    """
    out = []
    for cand in refs:
        others = [r for r in refs if r != cand]
        if len(others) < n:
            continue
        for subset in itertools.combinations(others, n):
            sub = list(subset)
            keys = charge_set(files, sub, cand)
            out.append(census(keys, files, sub, cand, width, window))
    return out


def worst(innocents, index: int, key) -> int:
    """The WORST (largest) innocent charge for one lump. Absent means zero:
    a reference that is never charged in a band is charged 0 there."""
    return max((c[index].get(key, 0) for c in innocents), default=0)


def verdict(charged: int, floor: int) -> str:
    return "CLEAR" if charged > floor else "FLOOR"


def report(posdir, refs, cand, width, top, label, window=None) -> tuple[int, int]:
    files, blocks, chunks = load_matrix(posdir, refs, refs + [cand])
    n = len(refs)
    keys = charge_set(files, refs, cand)
    pairs, bands, chunks_, rings = census(keys, files, refs, cand, width, window)
    innocents = innocent_censuses(files, refs, n - 1, width, window)

    print(f"RESIDUAL CENSUS  {label}")
    print(f"  window      : {chunks} chunks, {blocks:,} blocks, no mask")
    print(f"  references  : {' '.join(refs)}  (N={n})")
    print(f"  candidate   : {cand}")
    print(f"  charge      : {len(keys):,} of {blocks:,}")
    print(
        f"  floors below are the WORST charge the same instrument lays on a\n"
        f"  known-innocent vanilla run at N={n - 1}, per lump, never subtracted."
    )

    print(f"\n  BY BLOCK PAIR  (top {top})")
    print(f"    {'vanilla -> ' + cand:<52}{'charged':>9}{'floor':>9}  verdict")
    cleared_pairs = 0
    for (van, ours), c in pairs.most_common(top):
        f = worst(innocents, 0, (van, ours))
        v = verdict(c, f)
        cleared_pairs += c if v == "CLEAR" else 0
        print(f"    {van + ' -> ' + ours:<52}{c:>9,}{f:>9,}  {v}")

    print(f"\n  BY Y BAND  (width {width}, every band that is charged)")
    print(f"    {'y':<14}{'charged':>9}{'floor':>9}  verdict")
    cleared_bands = 0
    for lo in sorted(bands):
        c = bands[lo]
        f = worst(innocents, 1, lo)
        v = verdict(c, f)
        cleared_bands += c if v == "CLEAR" else 0
        print(f"    {f'{lo}..{lo + width - 1}':<14}{c:>9,}{f:>9,}  {v}")

    print(f"\n  BY CHUNK  (top {top} of {len(chunks_):,} charged chunks)")
    print(f"    {'chunk':<14}{'charged':>9}{'floor':>9}  verdict")
    for (cx, cz), c in chunks_.most_common(top):
        f = worst(innocents, 2, (cx, cz))
        print(f"    {f'{cx},{cz}':<14}{c:>9,}{f:>9,}  {verdict(c, f)}")

    if window is not None:
        print("\n  BY RING  (chunk distance from the window edge; 0 is outermost)")
        print(f"    {'ring':<14}{'charged':>9}{'floor':>9}  verdict")
        for r in sorted(rings):
            c = rings[r]
            f = worst(innocents, 3, r)
            print(f"    {r:<14}{c:>9,}{f:>9,}  {verdict(c, f)}")
        # The two questions a ring table exists to answer, stated rather than
        # left for the reader to add up. Our dump's decoration margin is a
        # fixed number of chunks, so if the answer lives in the first two
        # rings the residual is about the window and not about the world.
        edge = rings.get(0, 0) + rings.get(1, 0)
        inner = len(keys) - edge
        inn_edge = [c[3].get(0, 0) + c[3].get(1, 0) for c in innocents]
        inn_inner = [
            sum(v for k, v in c[3].items() if k >= 2) for c in innocents
        ]
        pct = 100.0 * edge / len(keys) if keys else 0.0
        print(
            f"    two outermost rings   {edge:>9,}{max(inn_edge, default=0):>9,}"
            f"  {verdict(edge, max(inn_edge, default=0))}   ({pct:.1f}% of the charge)"
        )
        print(
            f"    interior (ring >= 2)  {inner:>9,}{max(inn_inner, default=0):>9,}"
            f"  {verdict(inner, max(inn_inner, default=0))}"
        )

    # Concentration. One structure in one place and a thin film over the whole
    # window are the same total and different items.
    ranked = sorted(chunks_.values(), reverse=True)
    half = len(keys) / 2.0
    run = 0
    n_half = 0
    for v in ranked:
        run += v
        n_half += 1
        if run >= half:
            break
    print(
        f"\n  CONCENTRATION: {len(chunks_):,} of {chunks} chunks charged; "
        f"half the charge is in {n_half:,} of them; worst chunk "
        f"{ranked[0] if ranked else 0:,}."
    )
    print(
        f"  CLEARS ITS FLOOR: {cleared_pairs:,} by block pair, "
        f"{cleared_bands:,} by y band (of {len(keys):,})."
    )
    return len(keys), blocks


def selftest() -> int:
    """Prove this script can fail, on fixtures, in T0, with no box and no jar.

    The properties pinned are the ones that would make every row wrong without
    making any row look wrong:

    * the charge is an INTERSECTION, so it can only shrink as references are
      added — an accidental union prints a bigger, entirely believable table;
    * a per-lump floor is a MAXIMUM over innocent candidates, so it can only
      grow as candidates are added — a `min` here would convict noise
      everywhere and is invisible in the output;
    * y banding uses FLOOR division, so y=-1 lands in band -16 and not in 0.
      Python's `int()` truncates toward zero and the overworld's y starts at
      -64, so this is a live off-by-one over a quarter of the window;
    * a position the references SPELL DIFFERENTLY is not merged into a
      unanimous pair;
    * the parse is checked against the differ's own counter.
    """
    import tempfile

    ok = True
    ar = _parser()

    def expect(lbl, got, want):
        nonlocal ok
        if got != want:
            ok = False
            print(f"SELFTEST FAIL [{lbl}]: {got!r}, expected {want!r}")
        else:
            print(f"SELFTEST ok   [{lbl}]")

    # y banding, the cheapest thing to get wrong and the hardest to see.
    expect("band(-1,16) is -16", band_of(-1, 16), -16)
    expect("band(-64,16) is -64", band_of(-64, 16), -64)
    expect("band(0,16) is 0", band_of(0, 16), 0)
    expect("band(15,16) is 0", band_of(15, 16), 0)
    expect("band(-65,16) is -80", band_of(-65, 16), -80)

    with tempfile.TemporaryDirectory() as d:

        def w(ref, cand, lines):
            path = os.path.join(d, f"{ref}-vs-{cand}.pos")
            with open(path, "w", encoding="utf-8") as fh:
                for ln in lines:
                    fh.write(ln + "\n")
                fh.write(
                    f"DIFF chunks=4 (a=4 b=4) blocks=1000 mismatches={len(lines)}\n"
                )

        # Three references and a candidate `q`. Positions:
        #   (0,-1,0)  every reference says stone, q says dirt  -> charged, band -16
        #   (0,70,0)  a and b say sand, c says gravel, q says air -> charged, split
        #   (17,70,0) only a disagrees with q                  -> NOT charged at N=3
        w("a", "q", ["0 -1 0 stone dirt", "0 70 0 sand air", "17 70 0 sand air"])
        w("b", "q", ["0 -1 0 stone dirt", "0 70 0 sand air"])
        w("c", "q", ["0 -1 0 stone dirt", "0 70 0 gravel air"])
        # The reference-vs-reference arms, which are the innocent floors.
        w("a", "b", ["0 70 0 sand x"])
        w("a", "c", ["0 70 0 sand x"])
        w("b", "a", ["0 70 0 sand x"])
        w("b", "c", ["0 70 0 sand x"])
        w("c", "a", ["0 70 0 gravel x"])
        w("c", "b", ["0 70 0 gravel x"])

        refs = ["a", "b", "c"]
        files, blocks, _ = load_matrix(d, refs, refs + ["q"])

        expect("charge is an intersection", len(charge_set(files, refs, "q")), 2)
        expect("charge shrinks with N", len(charge_set(files, ["a"], "q")), 3)
        keys = charge_set(files, refs, "q")
        pairs, bands, chunks_, _ = census(keys, files, refs, "q", 16)
        expect("split spelling is not merged", pairs[("{gravel|sand}", "air")], 1)
        expect("unanimous pair", pairs[("stone", "dirt")], 1)
        expect("negative y bands down", bands[-16], 1)
        expect("y=70 bands to 64", bands[64], 1)
        expect("one chunk holds both", chunks_[(0, 0)], 2)

        # THE FLOOR IS A MAXIMUM AND THE TEST MUST BE ABLE TO SAY SO. Asserting
        # `worst(...) == max(...)` over the same list is the implementation
        # restated, which is the "gate that cannot fail" this repository has
        # now found more than twenty times. So the fixture is built to make
        # max and min DIFFER, and both are named:
        fake = [
            (collections.Counter(), collections.Counter({64: 3}), collections.Counter()),
            (collections.Counter(), collections.Counter({64: 11}), collections.Counter()),
            (collections.Counter(), collections.Counter(), collections.Counter()),
        ]
        expect("floor is the WORST innocent reading", worst(fake, 1, 64), 11)
        expect("an uncharged innocent is 0, not absent", worst(fake, 1, 999), 0)
        expect("a floor of 11 clears a charge of 11", verdict(11, 11), "FLOOR")
        expect("a floor of 11 does not clear 12", verdict(12, 11), "CLEAR")

        # THE RING CUT. Its whole value is that it separates the window's edge
        # from its inside, so what is pinned is that a position ON the edge and
        # a position two chunks in do not land in the same bucket, and that a
        # declared window the positions fall outside REFUSES rather than
        # renumbering every ring silently.
        expect("ring 0 is the outermost chunk", ring_of(48, 48, (48, 48, 80, 80)), 0)
        expect("ring is a min over four edges", ring_of(50, 79, (48, 48, 80, 80)), 1)
        expect("the centre is the deepest ring", ring_of(64, 64, (48, 48, 80, 80)), 16)
        expect("no window means no ring cut", ring_of(64, 64, None), -1)
        # (0,0) and (17,70) in the fixture are chunks (0,0) and (1,4).
        _, _, _, rr = census(keys, files, refs, "q", 16, (0, 0, 4, 4))
        expect("edge positions land in ring 0", rr[0], 2)
        try:
            census(keys, files, refs, "q", 16, (2, 2, 4, 4))
            expect("a position outside the window refuses", "no raise", "Malformed")
        except ar.Malformed:
            expect("a position outside the window refuses", "Malformed", "Malformed")

        # And the real matrix still produces a floor at all, so the wiring
        # from `innocent_censuses` into `worst` is exercised too.
        inn = innocent_censuses(files, refs, 2, 16)
        expect("every reference takes its turn as candidate", len(inn), 3)
        expect("innocent floor in band 64 is 1", worst(inn, 1, 64), 1)
        expect("innocent floor in band -16 is 0", worst(inn, 1, -16), 0)

        # The parse guard, borrowed rather than re-implemented.
        bad = os.path.join(d, "a-vs-bad.pos")
        with open(bad, "w", encoding="utf-8") as fh:
            fh.write("0 0 0 stone dirt\n")
            fh.write("DIFF chunks=4 (a=4 b=4) blocks=1000 mismatches=9\n")
        try:
            ar.load(bad)
            expect("parse disagreeing with the differ refuses", "no raise", "Malformed")
        except ar.Malformed:
            expect("parse disagreeing with the differ refuses", "Malformed", "Malformed")

        try:
            load_matrix(d, ["a", "zz"], ["a", "zz", "q"])
            expect("a missing pair refuses", "no raise", "Malformed")
        except ar.Malformed:
            expect("a missing pair refuses", "Malformed", "Malformed")

        # The candidate must not be inside its own reference set: `load_matrix`
        # skips the `ref == cand` pair, so the charge would quietly be taken
        # over one fewer reference than its N claims, in the flattering
        # direction. Exercised through `main` because that is where the guard
        # lives and an unexercised guard is the defect class this file is about.
        expect(
            "a candidate inside its own --refs refuses",
            main([d, "--refs", "a,b,c", "--cand", "b"]),
            2,
        )
        expect(
            "a repeated reference refuses",
            main([d, "--refs", "a,a,b", "--cand", "q"]),
            2,
        )
        expect(
            "a good invocation still runs",
            main([d, "--refs", "a,b,c", "--cand", "q", "--expect-charge", "2"]),
            0,
        )
        expect(
            "a wrong --expect-charge is caught",
            main([d, "--refs", "a,b,c", "--cand", "q", "--expect-charge", "9"]),
            1,
        )

        expect("window blocks read through", blocks, 1000)

    print("SELFTEST " + ("GREEN" if ok else "RED"))
    return 0 if ok else 1


OPTIONS = (
    "--refs",
    "--cand",
    "--band",
    "--top",
    "--label",
    "--window",
    "--expect-charge",
)


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
            "usage: residual-census.py <posdir> --refs c,d,e,f,g --cand zx "
            "[--band 16] [--top 20] [--label TEXT] [--expect-charge N]",
            file=sys.stderr,
        )
        return 2

    refs = [r for r in opts["--refs"].split(",") if r]
    if len(refs) < 2:
        print("need at least two references", file=sys.stderr)
        return 2
    if len(set(refs)) != len(refs):
        print(f"--refs repeats a reference: {opts['--refs']}", file=sys.stderr)
        return 2
    # A candidate that is also in its own reference set would be adjudicated
    # against itself, and `load_matrix` skips the `ref == cand` pair, so the
    # charge would be computed over one fewer reference than its N claims —
    # silently, and in the flattering direction. Scoring a reference AS the
    # candidate is a legitimate and necessary thing to do (it is how the floor
    # is taken); doing it without removing it from the pool is not.
    if opts["--cand"] in refs:
        print(
            f"--cand {opts['--cand']} is also in --refs. A candidate cannot be "
            "adjudicated against itself; drop it from --refs to score it as an "
            "innocent run.",
            file=sys.stderr,
        )
        return 2

    window = None
    if "--window" in opts:
        parts = opts["--window"].split()
        if len(parts) != 4:
            print("--window takes 'x1 z1 x2 z2' in CHUNK coordinates", file=sys.stderr)
            return 2
        try:
            x1, z1, x2, z2 = (int(v) for v in parts)
        except ValueError:
            print("--window takes four integers", file=sys.stderr)
            return 2
        if x1 > x2 or z1 > z2:
            print("--window is inverted", file=sys.stderr)
            return 2
        window = (x1, z1, x2, z2)

    ar = _parser()
    try:
        charge, _ = report(
            args[0],
            refs,
            opts["--cand"],
            int(opts.get("--band", "16")),
            int(opts.get("--top", "20")),
            opts.get("--label", ""),
            window,
        )
    except ar.Malformed as exc:
        print(f"CENSUS MALFORMED: {exc}", file=sys.stderr)
        return 2
    except OSError as exc:
        print(f"CENSUS MALFORMED: {exc}", file=sys.stderr)
        return 2

    want = opts.get("--expect-charge")
    if want is not None and charge != int(want):
        print(
            f"CENSUS MISMATCH: charge {charge:,}, expected {int(want):,}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
