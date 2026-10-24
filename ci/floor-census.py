#!/usr/bin/env python3
"""What is a FLOOR made of? A positional and per-block census of the charge an
innocent unmodified vanilla run takes on a window.

# WHY THIS EXISTS

`ci/window-resolving-power.py` measures a floor: substitute a genuine
unmodified vanilla run for our dump, adjudicate it against N of the others,
and read what the instrument charges it. On the seed-12345 r=16 overworld
window that is **2,577..4,835 at N=5** (`ow-r16-12345-floor-n5`).

That is a range, and a range means the six innocent readings disagree with
each other by ~2,258 blocks. Nobody had looked at *where*. A residual is
censused positionally and by block before anybody is allowed to open an
investigation on it (ORACLE rule 9); a floor is a measurement in exactly the
same sense and had never been given the same treatment. It was being quoted
as one scalar, as though vanilla's nondeterminism were a uniform fog over the
window.

It is not, and that is the whole point of this script. The floor is
overwhelmingly concentrated in a handful of block families that read the
world before they write it, and it is **empty** over most of the block
vocabulary. A `vanilla -> ours` pair whose own floor is 0 is adjudicable
against 0 no matter how large the window's aggregate floor is. That is what
turns "this window is at 1.6x its floor and is nearly spent" into "this
window is spent *for aggregate claims* and is still sharp for most named
ones".

# THE CONSTRUCTION, AND THE ONE SUBTLETY IN IT

For a pool of references R and each x in R, the innocent charge is

    charge(x) = { p : x(p) != r(p) for every r in R \\ {x} }

which is exactly the intersection of the |R|-1 pairwise position sets, and is
what `window-resolving-power.py` computes. This script takes the same sets and
asks three further questions of them:

1. **Is the floor one set or |R| sets?** If the six innocent charges land on
   the *same* positions, the floor is a fixed hard set — a named list of
   places where vanilla is unsure — and it is a property of the window that
   more sampling cannot remove. If they land on disjoint positions, the floor
   is per-run noise. The multiplicity histogram separates these, and the
   answer decides whether candidate 2 in the brief (pinning the
   nondeterminism) is even coherent.

2. **What is it made of?** The per-block table, keyed on the spelling the
   *references* agree on. Its complement is the deliverable: a block that
   never appears has a measured floor of zero.

3. **Where is it?** y profile and chunk concentration. A floor confined to a
   few chunks is a floor a sub-window can be chosen to avoid.

The subtlety is the key of the per-block table. Keying it on the ordered pair
`(vanilla, candidate)` — which is what `window-resolving-power.py --kinds`
does, correctly, for *our* charge — is WRONG for a lookup table, and wrong in
the unsafe direction. Our port writes `blue_ice -> packed_ice`; an innocent
vanilla run at the same positions writes `blue_ice -> water`. Keyed on the
ordered pair, `blue_ice -> packed_ice` has a floor of 0 and our charge is
adjudicated against nothing, when in truth vanilla is demonstrably unsure
about `blue_ice` at those positions. So the lookup table this script emits is
keyed on the **vanilla block alone** and marginalises over what the candidate
wrote. It is the conservative key, it is the one a future item must use, and
the ordered-pair table is printed beside it for reading only.

# INPUT

The same `<ref>-vs-<cand>.pos` matrix `window-resolving-power.py` takes, and
the same borrowed anchored parser, for the same reason: a parse that does not
equal the differ's own `mismatches=` is refused rather than trusted
(`docs/DETERMINISM.md` §10).

# ADJUDICATING A REAL CANDIDATE AGAINST THE TABLE

`--adjudicate <cand>` is the reason the table exists. It takes our charge —
the intersection over every reference, the same quantity
`ci/admissible-residual.py` publishes — splits it by the vanilla spelling, and
puts each block's charge beside that block's own floor.

Nothing is subtracted anywhere. A floor is quoted BESIDE a charge and never
taken off it (ORACLE, and `ci/check-figures.py` enforces the doc form). What
the split buys is not a smaller number, it is a **ranked verdict**: the part
of a residual that sits on blocks vanilla reproduces perfectly is adjudicable
at any window size, and the part that sits on stone-variant blobs is not, and
an aggregate floor cannot tell you which is which.

usage: floor-census.py <posdir> --refs a,b,c,d,e,f [--top N] [--label TEXT]
                       [--emit <path.toml>] [--expect-range LO..HI]
                       [--adjudicate <cand>] [--cand-refs a,b,c] [--expect-charge N]
                       [--stamps <oracle-work>] [--ref-prefix <p>]   (both required by --emit)
                       [--holdout]   (validate the table on half the pool; needs --adjudicate)
       floor-census.py --selftest
exit 0 fine, 1 an `--expect-*` did not match, 2 malformed or missing inputs.
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
    """Borrow `load()` from admissible-residual.py. Cached for the reason its
    sibling caches it: two module objects make two `Malformed` classes and an
    `except` in one stops catching a `raise` from the other."""
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


def load_matrix(posdir: str, refs: list[str], extra: list[str] | None = None):
    """{(ref, cand): positions} for every ordered pair within `refs`, plus
    `<ref>-vs-<extra>` for each adjudicated candidate."""
    ar = _parser()
    out: dict[tuple[str, str], dict] = {}
    blocks = chunks = None
    for cand in refs + list(extra or []):
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


def short(name: str) -> str:
    return name.replace("minecraft:", "")


def brief(vals: list[int]) -> str:
    """Same form `window-resolving-power.py` prints, so the two are comparable."""
    if not vals:
        return "—"
    if len(vals) == 1:
        return f"{vals[0]:,}"
    return (
        f"{min(vals):,}..{max(vals):,}  (median {int(statistics.median(vals)):,}, "
        f"n={len(vals)})"
    )


def read_stamps(stampdir: str, prefix: str, refs: list[str]):
    """Read each reference's own HARVEST-ORDER. Refuse on an unstamped one.

    ORACLE rule 30: an assertion is not a stamp. This table is emitted into the
    tree and will outlive the turn that made it, so the order it belongs to is
    read off the references rather than typed in by the caller. An unstamped
    reference's order is *recoverable* but it is not *known*, and AGENT-BRIEF is
    explicit that such a reference is not usable for a level — so it is refused
    here rather than silently labelled.

    A stamp may be DERIVED rather than original, and the difference is carried
    rather than erased. `ci/classify-20260722-order.sh` recovers the order of an
    unstamped reference by diffing it against a stamped reference of each order
    and writes a stamp holding a `classified_from=` line with the two distances
    and the controls they were read against. That is a measurement, which is
    what rule 30 asks for; it is still not the same thing as a stamp written at
    generation time, so the ones that came that way are named in the report and
    in the emitted table's `harvest_classified` field. A reader must never have
    to guess which kind a table was built on.
    """
    ar = _parser()
    gen: dict[str, str] = {}
    dump: dict[str, str] = {}
    derived: dict[str, str] = {}
    for r in refs:
        p = os.path.join(stampdir, f"{prefix}{r}", "HARVEST-ORDER")
        try:
            with open(p, encoding="utf-8") as fh:
                body = fh.read()
        except OSError as exc:
            raise ar.Malformed(
                f"{p}: no HARVEST-ORDER stamp ({exc.strerror}). A reference "
                "whose harvest order is not stamped is not usable for a level "
                "and must not be baked into a floor table."
            ) from exc
        for field, into in (("gen_order", gen), ("dump_order", dump)):
            hit = [
                ln.split("=", 1)[1].strip()
                for ln in body.splitlines()
                if ln.startswith(field + "=")
            ]
            if not hit:
                raise ar.Malformed(f"{p}: stamp carries no `{field}=`")
            into[r] = hit[0]
        why = [
            ln.split("=", 1)[1].strip()
            for ln in body.splitlines()
            if ln.startswith("classified_from=")
        ]
        if why:
            derived[r] = why[0]
    return gen, dump, derived


def verdict(charge: int, floor: int) -> str:
    """The one place the CLEAR / marginal / at-or-under rule is written.

    It was inline in the miss-direction adjudication and had to be given a name
    when the surplus direction was added, because two copies of a threshold are
    two thresholds as soon as one of them is edited. `max(f, 1)` is what stops a
    charge of 1 on a floor of 0 reading as CLEAR — a floor of zero is an
    estimate from finitely many innocent runs, not a proof of impossibility.
    """
    if charge > 2 * max(floor, 1):
        return "CLEAR"
    if charge > floor:
        return "marginal"
    return "AT/UNDER FLOOR"


_RANK = {"AT/UNDER FLOOR": 0, "marginal": 1, "CLEAR": 2}
_NAME = {v: k for k, v in _RANK.items()}


def innocent_charge(files, refs: list[str], x: str) -> set:
    others = [r for r in refs if r != x]
    return set.intersection(*[set(files[(r, x)]) for r in others])


def spelling(files, refs: list[str], x: str, p) -> str:
    """What the OTHER references call position `p`, or `<refs-split>`."""
    others = [r for r in refs if r != x]
    s = {files[(r, x)][p][0] for r in others}
    return short(s.pop()) if len(s) == 1 else "<refs-split>"


def run(posdir, refs, top, label, emit, expect_range, adjudicate, expect_charge,
        cand_refs, stampdir, prefix, holdout, holdout_cap) -> int:
    ar = _parser()
    if holdout and not adjudicate:
        raise ar.Malformed(
            "--holdout validates the per-block table by adjudicating a real "
            "candidate against a floor built from half the pool, so it needs "
            "--adjudicate. Silently doing nothing is how a validation stops "
            "validating."
        )
    if emit and not stampdir:
        raise ar.Malformed(
            "--emit writes a floor table into the tree, and a floor belongs to "
            "the ensemble AND the harvest order that produced it. Give "
            "--stamps <oracle-work dir> --ref-prefix <prefix> so the order is "
            "read off each reference's own HARVEST-ORDER rather than asserted."
        )
    gen = dump = None
    derived: dict[str, str] = {}
    if stampdir:
        gen, dump, derived = read_stamps(stampdir, prefix, refs)
    files, blocks, chunks = load_matrix(posdir, refs, [adjudicate] if adjudicate else [])
    n_loo = len(refs) - 1

    charges = {x: innocent_charge(files, refs, x) for x in refs}
    counts = sorted(len(c) for c in charges.values())

    print(f"FLOOR CENSUS  {label}")
    print(f"  window      : {chunks} chunks, {blocks:,} blocks, no mask")
    print(f"  ensemble    : {' '.join(refs)}   pool={len(refs)}, leave-one-out N={n_loo}")
    if gen:
        orders = sorted(set(gen.values()))
        tally = ", ".join(f"{o} x{sum(1 for v in gen.values() if v == o)}" for o in orders)
        print(f"  harvest     : {tally}   (read off each HARVEST-ORDER stamp)")
        if derived:
            print(
                f"    DERIVED STAMPS: {' '.join(sorted(derived))} "
                f"({len(derived)} of {len(refs)}) were not stamped at generation;\n"
                "    their order was recovered by classification. ORACLE rule 30 — the\n"
                "    anchor of the ensemble must be one of the stamped ones."
            )
            for r in sorted(derived):
                print(f"      {r}: {derived[r]}")
            if len(derived) == len(refs):
                raise ar.Malformed(
                    "EVERY reference in this pool carries a derived stamp. A "
                    "classification is read against a stamped reference of each "
                    "order, so a pool with no original stamp in it has no anchor "
                    "and its order is asserted after all. ORACLE rule 30."
                )
        if len(orders) > 1:
            print(
                "    MIXED-ORDER POOL. A floor belongs to its harvest order; the\n"
                "    minority order's members are adjudicated mostly against the\n"
                "    majority's and are charged for the order, not for being a\n"
                "    different run. Read the per-candidate column before quoting a max."
            )
    print(
        f"  floor       : {counts[0]:,}..{counts[-1]:,}  "
        f"(median {int(statistics.median(counts)):,}, n={len(counts)})"
    )
    print("\n  PER-CANDIDATE CHARGE — each reference in turn plays the innocent candidate")
    for x in refs:
        print(f"    {x:<8} {len(charges[x]):>8,}")

    # ---- 1. one set or |R| sets? -------------------------------------------
    mult: collections.Counter = collections.Counter()
    for x in refs:
        for p in charges[x]:
            mult[p] += 1
    union = len(mult)
    shared = sum(1 for v in mult.values() if v > 1)
    print("\n  IS THE FLOOR ONE SET OR MANY?  multiplicity over the union")
    print(f"    union of all {len(refs)} innocent charges : {union:>8,}")
    print(f"    positions charged more than once   : {shared:>8,} "
          f"({shared / max(union, 1) * 100:.2f}%)")
    hist: collections.Counter = collections.Counter(mult.values())
    for k in sorted(hist):
        print(f"      charged in {k} of {len(refs)} : {hist[k]:>8,}")
    print(
        "    A floor made of ONE set is a named list of places vanilla cannot\n"
        "    reproduce, and sampling harder will not remove it. A floor made of\n"
        f"    {len(refs)} disjoint sets is per-run noise, which more references DO thin."
    )

    # ---- 2. what is it made of? --------------------------------------------
    # Conservative key: the vanilla spelling alone. See the header.
    by_block: dict[str, collections.Counter] = {x: collections.Counter() for x in refs}
    by_pair: dict[str, collections.Counter] = {x: collections.Counter() for x in refs}
    # The OTHER direction, and it is not optional. `by_block` is keyed on what
    # vanilla says, so it floors a MISS — vanilla puts B here and we do not. A
    # SURPLUS is the opposite claim, "we put B here and vanilla does not", and
    # the floor for it is keyed on what the innocent CANDIDATE placed. Looking a
    # surplus up in the miss table is a category error that reads as a number.
    by_placed: dict[str, collections.Counter] = {x: collections.Counter() for x in refs}
    for x in refs:
        others = [r for r in refs if r != x]
        first = others[0]
        for p in charges[x]:
            v = spelling(files, refs, x, p)
            o = short(files[(first, x)][p][1])
            by_block[x][v] += 1
            by_placed[x][o] += 1
            by_pair[x][(v, o)] += 1

    block_floor: collections.Counter = collections.Counter()
    for x in refs:
        for k, v in by_block[x].items():
            block_floor[k] = max(block_floor[k], v)
    placed_floor: collections.Counter = collections.Counter()
    for x in refs:
        for k, v in by_placed[x].items():
            placed_floor[k] = max(placed_floor[k], v)
    pair_floor: collections.Counter = collections.Counter()
    for x in refs:
        for k, v in by_pair[x].items():
            pair_floor[k] = max(pair_floor[k], v)

    total_floor = sum(block_floor.values())
    print(f"\n  BY VANILLA BLOCK — floor(B) = WORST of the {len(refs)} innocent readings")
    print(f"    {'vanilla block':<34}{'floor':>8}{'median':>8}{'runs':>6}  share")
    for b, f in block_floor.most_common(top):
        per = [by_block[x].get(b, 0) for x in refs]
        nz = sum(1 for v in per if v)
        print(
            f"    {b:<34}{f:>8,}{int(statistics.median(per)):>8,}{nz:>6}"
            f"  {f / max(total_floor, 1) * 100:5.1f}%"
        )
    print(f"    {len(block_floor)} distinct vanilla blocks carry ANY floor at all.")
    print(
        "    EVERY OTHER BLOCK IN THE WINDOW HAS A MEASURED FLOOR OF ZERO over this\n"
        "    ensemble, and that is the usable half of this table: a charge on a block\n"
        "    absent here is adjudicated against 0, not against the window's aggregate."
    )

    print(f"\n  BY BLOCK PLACED — the SURPLUS direction: floor for `we put B here")
    print(f"  and vanilla does not`. Look a surplus up HERE, never in the table above.")
    print(f"    {'block an innocent run placed':<34}{'floor':>8}{'median':>8}{'runs':>6}")
    for b, f in placed_floor.most_common(top):
        per = [by_placed[x].get(b, 0) for x in refs]
        print(f"    {b:<34}{f:>8,}{int(statistics.median(per)):>8,}"
              f"{sum(1 for v in per if v):>6}")
    print(f"    {len(placed_floor)} distinct blocks were placed by any innocent run.")

    print(f"\n  BY ORDERED PAIR — reading only, NOT a lookup key (see the header)")
    print(f"    {'vanilla -> innocent candidate':<50}{'floor':>8}")
    for (v, o), f in pair_floor.most_common(min(top, 12)):
        print(f"    {v + ' -> ' + o:<50}{f:>8,}")

    # ---- 3. where is it? ----------------------------------------------------
    ys: collections.Counter = collections.Counter()
    chs: collections.Counter = collections.Counter()
    for p in mult:
        ys[(p[1] // 16) * 16] += 1
        chs[(p[0] >> 4, p[2] >> 4)] += 1
    print("\n  Y PROFILE of the union (16-block bands, non-empty only)")
    peak = max(ys.values()) if ys else 1
    for y in sorted(ys):
        bar = "#" * int(28 * ys[y] / peak)
        print(f"    y {y:>5}..{y + 15:<5} {ys[y]:>8,}  {bar}")
    occupied = len(chs)
    ranked = [c for _, c in chs.most_common()]
    top1pct = max(1, occupied // 100)
    print(f"\n  CHUNK CONCENTRATION of the union")
    print(f"    chunks holding any floor      : {occupied:,} of {chunks:,}")
    print(f"    worst chunk                   : {ranked[0]:,} positions")
    print(
        f"    top {top1pct} chunk(s) hold             : "
        f"{sum(ranked[:top1pct]):,} ({sum(ranked[:top1pct]) / max(union, 1) * 100:.1f}%)"
    )
    half = 0
    got = 0
    for c in ranked:
        got += c
        half += 1
        if got * 2 >= union:
            break
    print(f"    half the floor lives in       : {half:,} chunks "
          f"({half / max(occupied, 1) * 100:.1f}% of the occupied ones)")

    # ---- 4. adjudicate a real candidate against the table -------------------
    rc = 0
    if adjudicate:
        sub = cand_refs or refs
        cand_charge = set.intersection(*[set(files[(r, adjudicate)]) for r in sub])
        cand_t: collections.Counter = collections.Counter()
        for p in cand_charge:
            s = {files[(r, adjudicate)][p][0] for r in sub}
            cand_t[short(s.pop()) if len(s) == 1 else "<refs-split>"] += 1
        print(
            f"\n  ADJUDICATED — candidate `{adjudicate}` charge {len(cand_charge):,} "
            f"over refs {','.join(sub)} (N={len(sub)}), against the per-block "
            f"floor at N={n_loo}"
        )
        # A CHARGE BELONGS TO ITS SUBSET EXACTLY AS A FLOOR BELONGS TO ITS PAIR.
        # The floor has been registered as a range since it was first taken;
        # the charge beside it was registered as a scalar, which quietly asserts
        # that the choice of which N of the pool to adjudicate against does not
        # matter. It does, and this prints by how much.
        if len(sub) < len(refs):
            alt = sorted(
                len(set.intersection(*[set(files[(r, adjudicate)]) for r in s]))
                for s in itertools.combinations(refs, len(sub))
            )
            print(
                f"    the same charge over every other {len(sub)}-of-{len(refs)} "
                f"subset: {brief(alt)}\n"
                f"    — that spread is the charge's OWN sampling wobble and it is "
                f"quoted for\n      the same reason the floor's is."
            )
        print(f"    {'vanilla block':<40}{'charged':>9}{'floor':>8}{'ratio':>8}  verdict")
        tot = {"CLEAR": 0, "marginal": 0, "AT/UNDER FLOOR": 0}
        for rank, (b, c) in enumerate(cand_t.most_common()):
            f = block_floor.get(b, 0)
            v = verdict(c, f)
            tot[v] += c
            if rank < top:
                ratio = f"{c / f:.1f}x" if f else "—"
                print(f"    {b:<40}{c:>9,}{f:>8,}{ratio:>8}  {v}")
        print(
            f"    {'':<40}{'-' * 9}\n"
            f"    {'CLEAR — over twice its own block floor':<40}{tot['CLEAR']:>9,}\n"
            f"    {'marginal — above it but not twice':<40}{tot['marginal']:>9,}\n"
            f"    {'at or under its own block floor':<40}{tot['AT/UNDER FLOOR']:>9,}"
        )
        print(
            "    NOTHING IS SUBTRACTED. The three totals partition the charge by\n"
            "    whether the block it sits on is one vanilla reproduces exactly."
        )

        # ---- the SURPLUS direction, and it is not optional ------------------
        # Everything above is keyed on the VANILLA spelling, so it adjudicates
        # one claim about these positions: `vanilla puts B here and we do not`,
        # a MISS. The same positions carry a second and different claim — `we
        # put C here and vanilla does not`, a SURPLUS — whose floor lives in
        # the `[[placed]]` table and nowhere else. Looking a surplus up in the
        # miss table is a category error that reads as a number rather than as
        # an error: it is how a 439-block item was reported at 2.5x when the
        # matched pair says 1.06x. Both are printed here so that neither can be
        # quoted without the other having been available.
        cand_p: collections.Counter = collections.Counter()
        for p in cand_charge:
            cand_p[short(files[(sub[0], adjudicate)][p][1])] += 1
        print(
            f"\n    SURPLUS DIRECTION — the same {len(cand_charge):,} positions, keyed on\n"
            f"    what the CANDIDATE placed, against the [[placed]] floor at N={n_loo}"
        )
        print(f"    {'block we placed':<40}{'charged':>9}{'floor':>8}{'ratio':>8}  verdict")
        tot_s = {"CLEAR": 0, "marginal": 0, "AT/UNDER FLOOR": 0}
        for rank, (b, c) in enumerate(cand_p.most_common()):
            f = placed_floor.get(b, 0)
            v = verdict(c, f)
            tot_s[v] += c
            if rank < top:
                ratio = f"{c / f:.1f}x" if f else "—"
                print(f"    {b:<40}{c:>9,}{f:>8,}{ratio:>8}  {v}")
        print(
            f"    {'':<40}{'-' * 9}\n"
            f"    {'CLEAR — over twice its own placed floor':<40}{tot_s['CLEAR']:>9,}\n"
            f"    {'marginal — above it but not twice':<40}{tot_s['marginal']:>9,}\n"
            f"    {'at or under its own placed floor':<40}{tot_s['AT/UNDER FLOOR']:>9,}"
        )

        # ---- and the conservative join of the two ---------------------------
        # A position is only evidence of a defect if vanilla reproduces BOTH
        # what it put there and what we put there. If either block is one an
        # innocent run is itself charged for at this rate, the position is
        # consistent with being a different run rather than a different
        # generator. So each position takes the WEAKER of its two verdicts.
        # This is the number to quote; the two tables above say which side a
        # position lost on, which is what tells you where to look.
        joint = {"CLEAR": 0, "marginal": 0, "AT/UNDER FLOOR": 0}
        for p in cand_charge:
            s = {files[(r, adjudicate)][p][0] for r in sub}
            vb = short(s.pop()) if len(s) == 1 else "<refs-split>"
            ob = short(files[(sub[0], adjudicate)][p][1])
            joint[
                _NAME[
                    min(
                        _RANK[verdict(cand_t[vb], block_floor.get(vb, 0))],
                        _RANK[verdict(cand_p[ob], placed_floor.get(ob, 0))],
                    )
                ]
            ] += 1
        print(
            f"\n    BOTH DIRECTIONS — each position takes the WEAKER of its two verdicts\n"
            f"    {'CLEAR in both directions':<40}{joint['CLEAR']:>9,}\n"
            f"    {'marginal in the weaker direction':<40}{joint['marginal']:>9,}\n"
            f"    {'at or under a floor in one direction':<40}{joint['AT/UNDER FLOOR']:>9,}\n"
            f"    {'':<40}{'-' * 9}\n"
            f"    {'total charge':<40}{len(cand_charge):>9,}"
        )

        if expect_charge is not None:
            if len(cand_charge) != int(expect_charge.replace(",", "")):
                print(
                    f"FLOOR CENSUS RED: candidate charge is {len(cand_charge):,}, "
                    f"expected {expect_charge}",
                    file=sys.stderr,
                )
                rc = 1
            else:
                print(f"    reproduces the registered charge {expect_charge}")

    # ---- 5. does the table GENERALISE, or is it selection on n=|R|? --------
    #
    # This is the guard on everything above, and it is not optional reassurance:
    # the first sub-window this census suggested looked spectacular and was
    # entirely an artefact. Any "floor of zero" is estimated from |R| innocent
    # readings, and a bucket that is small enough will read zero by luck. So the
    # floor is estimated on half the pool and TESTED on the other half, which is
    # the only way to tell a real zero from an unsampled one.
    if holdout and adjudicate:
        k = len(refs) // 2
        sub = cand_refs or refs
        cand_charge = set.intersection(*[set(files[(r, adjudicate)]) for r in sub])
        cand_t = collections.Counter()
        cand_ch = collections.Counter()
        for p in cand_charge:
            s = {files[(r, adjudicate)][p][0] for r in sub}
            cand_t[short(s.pop()) if len(s) == 1 else "<refs-split>"] += 1
            cand_ch[(p[0] >> 4, p[2] >> 4)] += 1
        blk_sel, blk_surv, chunk_leak, chunk_ours, clean_n = [], [], [], [], []
        # C(16,8) is 12,870 and every split costs a pass over the charges, so
        # the enumeration is capped. The sample is drawn with a FIXED seed and
        # then sorted, so the same pool always yields the same splits and a
        # re-run reproduces the printed range exactly — a validation whose
        # answer moves between runs is not one.
        splits = list(itertools.combinations(refs, k))
        if len(splits) > holdout_cap:
            import random
            splits = sorted(random.Random(0).sample(splits, holdout_cap))
            sampled = f", {holdout_cap} of {len(list(itertools.combinations(refs, k))):,} sampled at seed 0"
        else:
            sampled = ""
        for sel in splits:
            held = [x for x in refs if x not in sel]
            fs: collections.Counter = collections.Counter()
            fh: collections.Counter = collections.Counter()
            for x in sel:
                for kk, v in by_block[x].items():
                    fs[kk] = max(fs[kk], v)
            for x in held:
                for kk, v in by_block[x].items():
                    fh[kk] = max(fh[kk], v)
            clear = [b for b, c in cand_t.items() if c > 2 * max(fs.get(b, 0), 1)]
            blk_sel.append(sum(cand_t[b] for b in clear))
            blk_surv.append(
                sum(cand_t[b] for b in clear if cand_t[b] > 2 * max(fh.get(b, 0), 1))
            )
            dirty = {(p[0] >> 4, p[2] >> 4) for x in sel for p in charges[x]}
            clean_n.append(chunks - len(dirty))
            chunk_leak.append(
                max(sum(1 for p in charges[x] if (p[0] >> 4, p[2] >> 4) not in dirty)
                    for x in held)
            )
            chunk_ours.append(sum(v for c, v in cand_ch.items() if c not in dirty))
        print(
            f"\n  HELD-OUT VALIDATION — floor estimated on {k} of {len(refs)} "
            f"candidates, tested on the other {len(refs) - k} "
            f"({len(blk_sel)} splits{sampled})"
        )
        print("    PER-BLOCK arm — the table this script emits")
        print(f"      charge called CLEAR on the {k}   : {brief(blk_sel)}")
        print(f"      still CLEAR against the held out : {brief(blk_surv)}")
        rate = [100.0 * s / t for s, t in zip(blk_surv, blk_sel) if t]
        print(f"      survival                         : "
              f"{min(rate):.1f}%..{max(rate):.1f}%")
        print("    PER-CHUNK arm — the sub-window the same census suggests")
        print(f"      chunks with no floor on the {k}   : {brief(clean_n)}")
        print(f"      held-out charge INSIDE them      : {brief(chunk_leak)}")
        print(f"      our charge inside them           : {brief(chunk_ours)}")
        print(
            "    A zero that survives the held-out half is a zero. A zero that\n"
            "    does not is the bucket being smaller than the sampling."
        )

    if emit:
        with open(emit, "w", encoding="utf-8") as fh:
            fh.write(
                "# GENERATED by ci/floor-census.py — do not hand-edit.\n"
                "#\n"
                "# Per-vanilla-block floor on one window and one ensemble. The key is the\n"
                "# VANILLA spelling alone, marginalised over what the candidate wrote: see\n"
                "# the script header for why the ordered pair is the unsafe key.\n"
                "#\n"
                "# A block ABSENT from this table was never charged to any of the\n"
                f"# {len(refs)} innocent vanilla runs and its floor here is 0.\n"
                "#\n"
                "# A floor belongs to the ensemble AND the harvest order that produced it.\n"
                "# Both are recorded below; do not carry this table to another ensemble.\n"
                f'ensemble = "{" ".join(refs)}"\n'
                f"pool = {len(refs)}\n"
                f"leave_one_out_n = {n_loo}\n"
                f"window_blocks = {blocks}\n"
                f"window_chunks = {chunks}\n"
                f"aggregate_floor_min = {counts[0]}\n"
                f"aggregate_floor_max = {counts[-1]}\n"
                f"aggregate_floor_median = {int(statistics.median(counts))}\n"
                f"distinct_blocks_with_floor = {len(block_floor)}\n"
                f"distinct_blocks_placed = {len(placed_floor)}\n"
                f'label = "{label}"\n'
                f'harvest_order = "{",".join(sorted(set(gen.values())))}"\n'
                f'dump_order = "{",".join(sorted(set(dump.values())))}"\n'
                f"harvest_mixed = {str(len(set(gen.values())) > 1).lower()}\n"
                # Which of the pool were stamped at generation and which had
                # their order recovered afterwards. Emitted always, including
                # the empty case, so that a table WITHOUT the field is
                # identifiable as one written before the distinction existed
                # rather than as one whose references were all stamped.
                f'harvest_classified = "{" ".join(sorted(derived))}"\n'
                f"harvest_classified_n = {len(derived)}\n"
            )
            for b, f in sorted(block_floor.items(), key=lambda kv: (-kv[1], kv[0])):
                per = [by_block[x].get(b, 0) for x in refs]
                fh.write(
                    "\n[[block]]\n"
                    f'name = "{b}"\n'
                    f"floor = {f}\n"
                    f"median = {int(statistics.median(per))}\n"
                    f"runs_charged = {sum(1 for v in per if v)}\n"
                )
            fh.write(
                "\n# THE SURPLUS DIRECTION. Everything above floors a MISS — vanilla\n"
                "# spells B here and the candidate does not. A claim of the form `we\n"
                "# place B where vanilla does not` is the opposite direction and must\n"
                "# be looked up below, keyed on what the innocent run PLACED. A block\n"
                "# absent from this list was never placed by any of the innocent runs\n"
                "# at a position the others did not, so its surplus floor here is 0.\n"
            )
            for b, f in sorted(placed_floor.items(), key=lambda kv: (-kv[1], kv[0])):
                per = [by_placed[x].get(b, 0) for x in refs]
                fh.write(
                    "\n[[placed]]\n"
                    f'name = "{b}"\n'
                    f"floor = {f}\n"
                    f"median = {int(statistics.median(per))}\n"
                    f"runs_charged = {sum(1 for v in per if v)}\n"
                )
        print(f"\n  emitted per-block floor table -> {emit}")

    if expect_range:
        try:
            lo, hi = (int(v.replace(",", "")) for v in expect_range.split(".."))
        except ValueError:
            print(f"--expect-range wants LO..HI, got {expect_range!r}", file=sys.stderr)
            return 2
        if (counts[0], counts[-1]) != (lo, hi):
            print(
                f"FLOOR CENSUS RED: floor is {counts[0]:,}..{counts[-1]:,}, "
                f"expected {lo:,}..{hi:,}",
                file=sys.stderr,
            )
            return 1
        print(f"\n  FLOOR CENSUS GREEN: reproduces the registered {expect_range}")
    return rc


def selftest() -> int:
    """Prove the arithmetic and the refusals on fixtures — no box, no jar.

    Same discipline as `admissible-residual.py` and `window-resolving-power.py`:
    a checker whose refusals never fire is this project's most-found defect, and
    an exit-code-only self-test lets a wrong operator print a believable table.
    So the printed arithmetic is asserted too, on a fixture built to be
    ASYMMETRIC — a symmetric one cannot tell `max` from `min` or a union from an
    intersection.
    """
    import contextlib
    import io as _io
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

    S = "DIFF chunks=1 (a=1 b=1)  blocks=1000 mismatches={n}  biomes=4/0\n"
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)

        def w(name, lines):
            (d / name).write_text(
                "".join(lines) + S.format(n=len(lines)), encoding="utf-8"
            )

        # Three references a, b, c and a candidate `ours`, over a 1000-block
        # window, all in one chunk. The fixture is built backwards from the
        # MUTATIONS it has to catch, because the first version of it was
        # mutation-tested and TWO survived: `max` written as "last one wins" in
        # the per-block floor, and the held-out survival test hard-wired to
        # pass. Both survived because the old fixture was too uniform — every
        # per-block vector was [1,0,0] or [1,1,1], where max, min and last-wins
        # all agree, and held-out survival was 100% anyway so forcing it to
        # pass changed nothing. Asymmetry is the whole design here.
        #
        #   z=1,2  a says stone, b and c granite   -> charged to a  (granite)
        #   z=3    b says stone, a and c granite   -> charged to b  (granite)
        #   z=4,5  c says andesite, a and b tuff   -> charged to c  (tuff)
        #   z=12   all three differ                -> charged to ALL THREE
        #                                             (<refs-split> for each)
        #
        # charge(a)={z1,z2,z12}=3, charge(b)={z3,z12}=2, charge(c)={z4,z5,z12}=3
        #   -> floor 2..3 median 3; union 6 with z12 charged 3 times.
        # per-block floor, keyed on what the OTHERS say:
        #   granite [2,1,0] -> floor 2   <-- MAX, not last-wins (which gives 1)
        #   tuff    [0,0,2] -> floor 2, median 0, charged in 1 run of 3
        #   <refs-split> [1,1,1] -> floor 1   <-- MAX, not sum (which gives 3)
        #
        # z=6,7,8 all three say tuff and only `ours` differs; z=9,10,11 all
        # three say stone and only `ours` differs. Invisible to everything
        # above, and they carry the adjudication and held-out assertions:
        #   ours on tuff  3 against floor 2 -> 3 > 2 but not > 4 -> marginal
        #   ours on stone 3 against floor 0 -> CLEAR
        # and in the held-out arm with k=1, `tuff` is CLEAR when the floor is
        # estimated on a or b (neither of which is charged on tuff) and is NOT
        # clear against the held-out pair (which contains c, charged 2 on it).
        # That makes survival 50%..100% rather than a flat 100%, which is what
        # makes "survival always passes" a detectable mutation.
        #
        # WHAT `ours` PLACES IS CHOSEN TO MAKE THE TWO DIRECTIONS DISAGREE.
        # At z=6,7,8 it writes `dirt`, which no innocent run ever placed, so
        # its surplus floor is 0 and that direction reads CLEAR while the miss
        # direction reads marginal. At z=9,10,11 it writes `andesite`, whose
        # PLACED floor is 3, so the surplus direction reads at-or-under while
        # the miss direction reads CLEAR. The two totals are therefore
        # (CLEAR 3, marginal 3, under 0) and (CLEAR 3, marginal 0, under 3),
        # and the conservative join is (CLEAR 0, marginal 3, under 3) — three
        # different triples. Before `andesite` was chosen here every position
        # was CLEAR on the surplus side, the join equalled the miss totals, and
        # a mutation that simply reprinted the miss totals as the join survived
        # the whole self-test. It does not now.
        V = {
            "a":    {1: "stone",   2: "stone",   3: "granite", 4: "tuff",
                     5: "tuff",    12: "stone"},
            "b":    {1: "granite", 2: "granite", 3: "stone",   4: "tuff",
                     5: "tuff",    12: "granite"},
            "c":    {1: "granite", 2: "granite", 3: "granite", 4: "andesite",
                     5: "andesite", 12: "andesite"},
            "ours": {1: "granite", 2: "granite", 3: "granite", 4: "tuff",
                     5: "tuff",    12: "stone"},
        }
        # z=13: all three references agree on `gravel` — so it is invisible to
        # every innocent charge and changes no floor — and `ours` is charged
        # exactly ONCE there, on a block whose floor is 0. That single position
        # is the only thing in this fixture that separates `2 * max(floor, 1)`
        # from `2 * floor`, and without it the mutation that deletes the
        # `max(…, 1)` SURVIVED the whole self-test (`ci/floor-census-falsify.sh`,
        # 2026-08-10, 16 of 17 caught). It matters because a floor of zero is an
        # estimate from finitely many innocent runs and not a proof that vanilla
        # never varies there: without the clamp, a charge of ONE block on an
        # unobserved block reads as CLEAR, which is the cheapest possible way to
        # manufacture a defect out of a sampling gap.
        # z=14: all three agree on `stone` again, and `ours` places
        # `andesite[axis=y]` there — a BLOCK STATE whose bare name is a block
        # the innocent runs really did place, with a placed floor of 3. Keyed
        # on the full state its floor is 0 and it reads marginal; keyed on the
        # bare name it merges into `andesite` and the surplus totals move. That
        # is the whole point of it: this table is keyed on the FULL BLOCK STATE
        # and a charge pooled to the bare name silently misses the lookup, so
        # every block in the fixture being unstated left the project's
        # most-warned-about floor-table mistake untested.
        for who in ("a", "b", "c"):
            V[who].update({6: "tuff", 7: "tuff", 8: "tuff",
                           9: "stone", 10: "stone", 11: "stone",
                           13: "gravel", 14: "stone"})
        V["ours"].update({z: "dirt" for z in (6, 7, 8, 13)})
        V["ours"].update({z: "andesite" for z in (9, 10, 11)})
        V["ours"][14] = "andesite[axis=y]"
        full = {k: {(1, 1, z): v for z, v in d.items()} for k, d in V.items()}

        def write_pair(ref, cand):
            lines = []
            for p in sorted(full[ref]):
                if full[ref][p] != full[cand][p]:
                    lines.append(
                        f"{p[0]} {p[1]} {p[2]} minecraft:{full[ref][p]} "
                        f"minecraft:{full[cand][p]}\n"
                    )
            w(f"{ref}-vs-{cand}.pos", lines)

        for ref, cand in itertools.permutations("abc", 2):
            write_pair(ref, cand)
        for ref in "abc":
            write_pair(ref, "ours")

        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = main([str(d), "--refs", "a,b,c", "--top", "10"])
        out = " ".join(buf.getvalue().split())
        expect("happy path", rc, 0)
        for want in (
            "floor : 2..3 (median 3, n=3)",
            "union of all 3 innocent charges : 6",
            "positions charged more than once : 1 (16.67%)",
            "charged in 1 of 3 : 5",
            "charged in 3 of 3 : 1",
            # floor, median, runs. granite's per-run vector is [2,1,0]: a max
            # gives 2, last-wins gives 1, a min gives 0 and a sum gives 3, so
            # this single row separates all four.
            "granite 2 1 2",
            "tuff 2 0 1",
            "<refs-split> 1 1 3",     # [1,1,1]: a max is 1 where a sum is 3
            "3 distinct vanilla blocks carry ANY floor",
            # The surplus direction, keyed on what the innocent run PLACED.
            # stone [3,1,0], andesite [0,0,3], granite [0,1,0] — asymmetric on
            # purpose, and different from the miss table above, so keying one
            # on the other's counter is a detectable mutation.
            "stone 3 1 2",
            "andesite 3 0 1",
            "3 distinct blocks were placed by any innocent run",
        ):
            if want in out:
                print(f"SELFTEST ok   [arithmetic: {want[:38]}…]")
            else:
                print(f"SELFTEST FAIL [arithmetic]: {want!r} not in output")
                ok = False

        with contextlib.redirect_stdout(_io.StringIO()):
            rc_ok = main([str(d), "--refs", "a,b,c", "--expect-range", "2..3"])
            rc_bad = main([str(d), "--refs", "a,b,c", "--expect-range", "2..4"])
        expect("expect-range matches", rc_ok, 0)
        expect("expect-range mismatch is red", rc_bad, 1)

        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = main([str(d), "--refs", "a,b,c", "--adjudicate", "ours",
                       "--expect-charge", "8", "--top", "10"])
        out = " ".join(buf.getvalue().split())
        expect("adjudicate happy path", rc, 0)
        for want in (
            "candidate `ours` charge 8 over refs a,b,c (N=3), against the "
            "per-block floor at N=2",
            "stone 4 0 — CLEAR",
            "tuff 3 2 1.5x marginal",
            "CLEAR — over twice its own block floor 4",
            "marginal — above it but not twice 4",
            # The surplus direction. `dirt` was never placed by an innocent
            # run so its floor is 0; `andesite` was placed 3 times by c, so a
            # charge of 3 is at its floor. Reading either of these out of the
            # miss table instead would give dirt 0 (same by luck) and andesite
            # 0 (WRONG, and wrong in the unsafe direction) — which is the
            # 439-at-2.5x mistake reproduced in miniature.
            "SURPLUS DIRECTION — the same 8 positions",
            "dirt 4 0 — CLEAR",
            "andesite 3 3 1.0x AT/UNDER FLOOR",
            # KEYED ON THE FULL BLOCK STATE. `andesite[axis=y]` was never
            # placed by an innocent run so its floor is 0 and 1 charge reads
            # marginal; pooled to the bare name it would merge into the
            # `andesite` bucket above and the surplus totals would move.
            "andesite[axis=y] 1 0 — marginal",
            "marginal — above it but not twice 1",
            "CLEAR — over twice its own placed floor 4",
            "at or under its own placed floor 3",
            # The join, and it must equal NEITHER of the two triples above.
            "CLEAR in both directions 0",
            "marginal in the weaker direction 5",
            "at or under a floor in one direction 3",
            "total charge 8",
        ):
            if want in out:
                print(f"SELFTEST ok   [adjudicate: {want[:38]}…]")
            else:
                print(f"SELFTEST FAIL [adjudicate]: {want!r} not in output")
                ok = False
        with contextlib.redirect_stdout(_io.StringIO()):
            rc_bad = main([str(d), "--refs", "a,b,c", "--adjudicate", "ours",
                           "--expect-charge", "9"])
        expect("adjudicate wrong expectation is red", rc_bad, 1)

        # `--cand-refs` must actually change the charge, and must print the
        # spread over the other subsets of the same size. Over {b,c} the
        # candidate is charged at (3,3,3) as well, so 5 rather than 4, and the
        # three 2-of-3 subsets read 4, 4 and 5. A `--cand-refs` that was
        # silently ignored would print 4 and no spread line at all.
        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = main([str(d), "--refs", "a,b,c", "--adjudicate", "ours",
                       "--cand-refs", "b,c", "--expect-charge", "9"])
        out = " ".join(buf.getvalue().split())
        expect("cand-refs subset", rc, 0)
        for want in (
            "charge 9 over refs b,c (N=2)",
            "subset: 8..9 (median 8, n=3)",
        ):
            if want in out:
                print(f"SELFTEST ok   [cand-refs: {want[:34]}…]")
            else:
                print(f"SELFTEST FAIL [cand-refs]: {want!r} not in output")
                ok = False

        expect("missing file refused",
               main([str(d), "--refs", "a,b,z"]), 2)
        expect("one reference refused", main([str(d), "--refs", "a"]), 2)
        expect("no refs refused", main([str(d)]), 2)

        # --emit bakes a table into the tree, so the harvest order must come off
        # the references' own stamps. Three refusals and one happy path, because
        # a guard whose refusal never fires is this project's most-found defect.
        out_toml = str(d / "out.toml")
        with contextlib.redirect_stdout(_io.StringIO()):
            rc_nostamp = main([str(d), "--refs", "a,b,c", "--emit", out_toml])
        expect("emit without --stamps refused", rc_nostamp, 2)

        stamps = d / "refs"
        for r in "abc":
            (stamps / f"ref-{r}").mkdir(parents=True)
        with contextlib.redirect_stdout(_io.StringIO()):
            rc_unstamped = main([str(d), "--refs", "a,b,c", "--emit", out_toml,
                                 "--stamps", str(stamps), "--ref-prefix", "ref-"])
        expect("unstamped reference refused", rc_unstamped, 2)

        for r in "abc":
            (stamps / f"ref-{r}" / "HARVEST-ORDER").write_text(
                "gen_order=rows\nseed=1\ndump_order=zx\n", encoding="utf-8"
            )
        (stamps / "ref-c" / "HARVEST-ORDER").write_text(
            "seed=1\ndump_order=zx\n", encoding="utf-8"
        )
        with contextlib.redirect_stdout(_io.StringIO()):
            rc_nofield = main([str(d), "--refs", "a,b,c", "--emit", out_toml,
                               "--stamps", str(stamps), "--ref-prefix", "ref-"])
        expect("stamp missing gen_order refused", rc_nofield, 2)

        (stamps / "ref-c" / "HARVEST-ORDER").write_text(
            "gen_order=rows_rev\nseed=1\ndump_order=zx_zrev\n", encoding="utf-8"
        )
        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc_emit = main([str(d), "--refs", "a,b,c", "--emit", out_toml,
                            "--stamps", str(stamps), "--ref-prefix", "ref-"])
        expect("emit with stamps", rc_emit, 0)
        body = (d / "out.toml").read_text(encoding="utf-8")
        printed = " ".join(buf.getvalue().split())
        for want, where, lbl in (
            ('harvest_order = "rows,rows_rev"', body, "emitted order"),
            ("harvest_mixed = true", body, "emitted mixed flag"),
            ("MIXED-ORDER POOL", printed, "mixed pool warned"),
            # Emitted even when empty, so a table lacking the field is
            # identifiable as pre-dating the distinction rather than as one
            # whose whole pool was stamped at generation.
            ('harvest_classified = ""', body, "no derived stamps recorded"),
            ("harvest_classified_n = 0", body, "derived count recorded"),
        ):
            if want in where:
                print(f"SELFTEST ok   [{lbl}]")
            else:
                print(f"SELFTEST FAIL [{lbl}]: {want!r} missing")
                ok = False

        # A DERIVED stamp is usable and is never silently equivalent to an
        # original one. ORACLE rule 30: recovering an unstamped reference's
        # order by classification is a measurement, but the anchor it is read
        # against has to be a reference that was stamped at generation — so a
        # pool in which EVERY member is derived is refused rather than emitted.
        (stamps / "ref-c" / "HARVEST-ORDER").write_text(
            "gen_order=rows\nseed=1\ndump_order=zx\n"
            "classified_from=vs a(rows)=11 vs rr(rows_rev)=99\n",
            encoding="utf-8",
        )
        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc_der = main([str(d), "--refs", "a,b,c", "--emit", out_toml,
                           "--stamps", str(stamps), "--ref-prefix", "ref-"])
        expect("derived stamp accepted", rc_der, 0)
        body = (d / "out.toml").read_text(encoding="utf-8")
        printed = " ".join(buf.getvalue().split())
        for want, where, lbl in (
            ('harvest_classified = "c"', body, "derived stamp named in table"),
            ("harvest_classified_n = 1", body, "derived count is 1"),
            ("DERIVED STAMPS: c (1 of 3)", printed, "derived stamps reported"),
            ("vs a(rows)=11 vs rr(rows_rev)=99", printed, "derivation evidence shown"),
        ):
            if want in where:
                print(f"SELFTEST ok   [{lbl}]")
            else:
                print(f"SELFTEST FAIL [{lbl}]: {want!r} missing")
                ok = False
        for r in "ab":
            (stamps / f"ref-{r}" / "HARVEST-ORDER").write_text(
                "gen_order=rows\nseed=1\ndump_order=zx\n"
                "classified_from=vs nothing at all\n", encoding="utf-8"
            )
        with contextlib.redirect_stdout(_io.StringIO()):
            rc_all = main([str(d), "--refs", "a,b,c", "--emit", out_toml,
                           "--stamps", str(stamps), "--ref-prefix", "ref-"])
        expect("all-derived pool refused (no stamped anchor)", rc_all, 2)
        for r in "abc":
            (stamps / f"ref-{r}" / "HARVEST-ORDER").write_text(
                "gen_order=rows\nseed=1\ndump_order=zx\n", encoding="utf-8"
            )

        # --holdout: refuses without a candidate, and reports both arms with one.
        expect("holdout without --adjudicate refused",
               main([str(d), "--refs", "a,b,c", "--holdout"]), 2)
        buf = _io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = main([str(d), "--refs", "a,b,c", "--adjudicate", "ours",
                       "--holdout", "--top", "0"])
        out = " ".join(buf.getvalue().split())
        expect("holdout runs", rc, 0)
        for want in (
            "floor estimated on 1 of 3 candidates, tested on the other 2 (3 splits)",
            "charge called CLEAR on the 1 : 4..7",
            "still CLEAR against the held out : 4",
            "survival : 57.1%..100.0%",
        ):
            if want in out:
                print(f"SELFTEST ok   [holdout: {want[:34]}…]")
            else:
                print(f"SELFTEST FAIL [holdout]: {want!r} not in output")
                ok = False

        (d / "a-vs-b.pos").write_text(
            "1 1 1 minecraft:stone minecraft:granite\n" + S.format(n=9),
            encoding="utf-8",
        )
        expect("miscounted file refused", main([str(d), "--refs", "a,b,c"]), 2)

    print("SELFTEST " + ("GREEN" if ok else "RED"))
    return 0 if ok else 1


OPTIONS = ("--refs", "--top", "--label", "--emit", "--expect-range",
           "--adjudicate", "--expect-charge", "--cand-refs", "--stamps",
           "--ref-prefix", "--holdout", "--holdout-splits")


def main(argv: list[str]) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()
    opts: dict[str, str] = {}
    args: list[str] = []
    i = 0
    while i < len(argv):
        if argv[i] == "--holdout":
            opts["--holdout"] = "1"
            i += 1
            continue
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
    if len(args) != 1 or "--refs" not in opts:
        print(
            "usage: floor-census.py <posdir> --refs a,b,c [--top N] [--label TEXT] "
            "[--emit <path.toml>] [--expect-range LO..HI]",
            file=sys.stderr,
        )
        return 2
    refs = [r for r in opts["--refs"].split(",") if r]
    if len(refs) < 2:
        print("need at least two references — a floor needs a pool", file=sys.stderr)
        return 2
    ar = _parser()
    try:
        return run(
            args[0],
            refs,
            int(opts.get("--top", "25")),
            opts.get("--label", ""),
            opts.get("--emit"),
            opts.get("--expect-range"),
            opts.get("--adjudicate"),
            opts.get("--expect-charge"),
            [r for r in opts.get("--cand-refs", "").split(",") if r] or None,
            opts.get("--stamps"),
            opts.get("--ref-prefix", ""),
            "--holdout" in opts,
            int(opts.get("--holdout-splits", "40")),
        )
    except (ar.Malformed, OSError) as exc:
        print(f"FLOOR CENSUS MALFORMED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
