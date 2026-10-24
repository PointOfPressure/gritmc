#!/usr/bin/env python3
"""Rank a candidate's charge per block, with the floor taken AT THE SAME
GRANULARITY as the charge.

WHY THIS EXISTS — a floor table is keyed, and a key is part of the number.

`ci/floor-census.py --adjudicate` is correct and this does not replace it. It
keys both the charge and the floor on the full block STATE, so
`spruce_leaves[distance=2,persistent=false,waterlogged=false]` is one row and
`[distance=3,...]` is another. That is the right default: the two are different
blocks to the differ and a floor that pooled them would hide a state-only
defect.

But a *reader* wants the tree, not the leaf: "how wrong are we about spruce
leaves" is a question about the bare name, and the six state rows have to be
added up to answer it. On 2026-08-10 that addition was done by hand, in a
throwaway script, with

    def short(s):
        return s.split("[")[0].replace("minecraft:", "")

applied to the CHARGE and then looked up in a floor table whose keys had been
written by `floor-census.py`'s own `short()`, which strips only
`minecraft:` and KEEPS the state. Every stateful block therefore missed its
key and `dict.get(name, 0)` returned the documented default for "this block was
never charged to any innocent run" — a floor of **zero**. It is the failure
this repository keeps paying for: a lookup that returns a number instead of an
error.

Fourteen of the twenty-eight published rows were wrong and every one of them
was wrong in the same direction, because a missing floor always reads as a
clean one:

    spruce_leaves   1,422 against 0        ->  1,422 against 262     5.3x
    snow              452 against 0        ->    452 against 182     2.5x
    grass_block       351 against 0        ->    351 against 134     2.6x
    water             617 against 0        ->    617 against  39    15.8x
    deepslate         453 against 0        ->    453 against  23    19.7x
    placed spruce_leaves 1,024 against 0   ->  1,024 against 426     2.4x
    placed spruce_log      120 against 0   ->    120 against  54     2.2x

The blocks that came through unharmed are exactly the ones with a single state
— `air`, `granite`, `andesite`, `diorite`, `packed_ice`, `cave_air`, `sulfur`,
`cinnabar`, `ice` — which is why the table looked plausible: half of it was
right, and the half that was wrong was wrong silently.

So the rule this file mechanises: **never join a charge to a floor across a
key transformation.** Compute both from the same matrix under the same key
function. `--key bare` aggregates leaves into their block; `--key state` is
`floor-census.py`'s granularity and is here so the two can be compared in one
run. A floor of zero printed by this script means the bucket really was never
charged to an innocent run, which is a finding; a floor of zero produced by a
missed lookup means nothing at all and looks identical.

The floor is the leave-one-out construction of `ci/floor-census.py`: for each
reference `x`, charge it against the other N-1 and count what they say it got
wrong; the floor of a bucket is the WORST of those N readings. Nothing is
subtracted from anything, ever.

usage: rank-block-floor.py <refposdir> --refs c,d,..,r --cand-pos <dir>
                           --cand <name> [--key bare|state|both] [--top N]
                           [--label TEXT] [--expect-charge N]
       rank-block-floor.py --selftest

`<refposdir>` needs `<r>-vs-<x>.pos` for every ordered pair of `--refs`;
`--cand-pos` needs `<r>-vs-<cand>.pos` for every ref. They may be the same
directory. VANILLA FIRST in every one of them.

exit 0 fine, 1 `--expect-charge` did not match, 2 malformed or missing inputs.
"""
import collections
import importlib.util
import os
import sys


def _parser():
    """Reuse `admissible-residual.py`'s reader so a .pos file means one thing."""
    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location(
        "ar", os.path.join(here, "admissible-residual.py")
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def bare(name: str) -> str:
    return name.split("[")[0].replace("minecraft:", "")


def state(name: str) -> str:
    return name.replace("minecraft:", "")


KEYFN = {"bare": bare, "state": state}


def load_pairs(ar, posdir, pairs):
    out = {}
    blocks = chunks = -1
    for a, b in pairs:
        p = os.path.join(posdir, f"{a}-vs-{b}.pos")
        if not os.path.exists(p):
            raise ar.Malformed(
                f"{p}: missing. A rank needs every ordered pair of its pool; a "
                "pool that is quietly smaller than its label is how a floor "
                "gets quoted for an ensemble that never produced it."
            )
        d, bl, ch = ar.load(p)
        if blocks < 0:
            blocks, chunks = bl, ch
        elif (bl, ch) != (blocks, chunks):
            raise ar.Malformed(
                f"{p} covers {ch} chunks / {bl:,} blocks against "
                f"{chunks} / {blocks:,}. A rank has to be over one window."
            )
        out[(a, b)] = d
    return out, blocks, chunks


def floors(ar, files, refs, keyfn):
    """Leave-one-out per-bucket floor, both directions, at one granularity."""
    miss: collections.Counter = collections.Counter()
    surp: collections.Counter = collections.Counter()
    per_run = []
    for x in refs:
        others = [r for r in refs if r != x]
        keys = set.intersection(*[set(files[(r, x)]) for r in others])
        m: collections.Counter = collections.Counter()
        s: collections.Counter = collections.Counter()
        first = others[0]
        for p in keys:
            spellings = {files[(r, x)][p][0] for r in others}
            v = (
                keyfn(spellings.pop())
                if len(spellings) == 1
                else "<refs-split>"
            )
            m[v] += 1
            s[keyfn(files[(first, x)][p][1])] += 1
        per_run.append((x, len(keys)))
        for k, v in m.items():
            miss[k] = max(miss[k], v)
        for k, v in s.items():
            surp[k] = max(surp[k], v)
    return miss, surp, per_run


def charge_of(files, refs, cand, keyfn):
    keys = set.intersection(*[set(files[(r, cand)]) for r in refs])
    m: collections.Counter = collections.Counter()
    s: collections.Counter = collections.Counter()
    first = refs[0]
    for p in keys:
        spellings = {files[(r, cand)][p][0] for r in refs}
        m[keyfn(spellings.pop()) if len(spellings) == 1 else "<refs-split>"] += 1
        s[keyfn(files[(first, cand)][p][1])] += 1
    return keys, m, s


def granularity_violations(bare_floor, state_floor):
    """THE POSITIVE CONTROL. A floor of zero is a claim, not a result.

    Computing the floor at the charge's own granularity is what makes this
    script correct, but "correct by construction" is the sentence six
    instruments died of this week. So the two granularities are computed and
    *compared*, and the comparison is an invariant rather than a taste:

        floor_bare(B) = max over runs of (sum over states of that run's count)
        floor_state(B[s]) = max over runs of that run's count for one state

    A max of sums is never smaller than any single term's max, so
    `floor_bare(B) >= max_s floor_state(B[s])` must hold for every block. A
    missed lookup — the 2026-08-10 defect — makes the left side read exactly
    0 while the right side is 113, and this fires. It fires on *any* path
    that loses the state, not only on the one that was written.

    Returns a list of (block, bare, worst_state_floor, state_name).
    """
    by_bare: dict[str, tuple[int, str]] = {}
    for k, v in state_floor.items():
        b = k.split("[")[0]
        if v > by_bare.get(b, (-1, ""))[0]:
            by_bare[b] = (v, k)
    out = []
    for b, (worst, name) in sorted(by_bare.items()):
        got = bare_floor.get(b, 0)
        if got < worst:
            out.append((b, got, worst, name))
    return out


def verdict(c, f):
    # Same three-way split floor-census.py uses, so the two are comparable.
    return "CLEAR" if c > 2 * max(f, 1) else "marginal" if c > f else "AT/UNDER FLOOR"


def report(kind, cnt, floor, top):
    print(f"\n    {kind:<44}{'charged':>9}{'floor':>8}{'ratio':>11}  verdict")
    tot = {"CLEAR": 0, "marginal": 0, "AT/UNDER FLOOR": 0}
    for rank, (b, c) in enumerate(cnt.most_common()):
        f = floor.get(b, 0)
        v = verdict(c, f)
        tot[v] += c
        if rank < top:
            print(
                f"    {b[:44]:<44}{c:>9,}{f:>8,}"
                f"{(f'{c / f:.1f}x' if f else 'unbounded'):>11}  {v}"
            )
    print(
        f"    {'':<44}{'-' * 9}\n"
        f"    {'CLEAR — over twice its own floor':<44}{tot['CLEAR']:>9,}\n"
        f"    {'marginal — above it but not twice':<44}{tot['marginal']:>9,}\n"
        f"    {'at or under its own floor':<44}{tot['AT/UNDER FLOOR']:>9,}"
    )
    return tot


def run(refpos, refs, candpos, cand, key, top, label, expect_charge):
    ar = _parser()
    if len(refs) < 3:
        print(
            "a leave-one-out floor needs at least three references — with two, "
            "every reference is its own only witness",
            file=sys.stderr,
        )
        return 2
    if cand in refs:
        print(
            f"--cand {cand} is also in --refs. A candidate cannot be a member "
            "of the ensemble it is adjudicated against.",
            file=sys.stderr,
        )
        return 2
    try:
        rr, blocks, chunks = load_pairs(
            ar, refpos, [(a, b) for a in refs for b in refs if a != b]
        )
        rc_, b2, c2 = load_pairs(ar, candpos, [(r, cand) for r in refs])
    except ar.Malformed as exc:
        print(str(exc), file=sys.stderr)
        return 2
    if (b2, c2) != (blocks, chunks):
        print(
            f"the candidate matrix covers {c2} chunks / {b2:,} blocks against "
            f"the reference matrix's {chunks} / {blocks:,}. A charge and a "
            "floor from different windows do not belong on one line.",
            file=sys.stderr,
        )
        return 2

    print(f"RANK BY BLOCK FLOOR  {label}")
    print(f"  window      : {chunks} chunks, {blocks:,} blocks, no mask")
    print(f"  references  : {' '.join(refs)}  (pool {len(refs)}, floor N={len(refs) - 1})")
    print(f"  candidate   : {cand}")
    print(
        "  THE FLOOR IS TAKEN AT THE SAME KEY AS THE CHARGE. A bare-name charge\n"
        "  looked up in a state-keyed table misses every stateful block and\n"
        "  reads zero — see this file's header for the 2026-08-10 instance."
    )

    # THE POSITIVE CONTROL, run before any row is printed and on every
    # invocation. Both granularities are taken from the same matrix and
    # checked against each other; a bare floor that has lost its states reads
    # zero and this refuses instead of printing it.
    bm, bs, _ = floors(ar, rr, refs, bare)
    sm, ss, _ = floors(ar, rr, refs, state)
    bad = granularity_violations(bm, sm) + granularity_violations(bs, ss)
    if bad:
        print(
            "GRANULARITY CONTROL FAILED — a pooled floor is smaller than one of\n"
            "its own states, which is arithmetically impossible and is what a\n"
            "lost block state looks like:",
            file=sys.stderr,
        )
        for b, got, worst, name in bad[:10]:
            print(f"  {b}: pooled floor {got}, but {name} alone floors {worst}", file=sys.stderr)
        return 2
    stateful = sum(1 for k in sm if "[" in k) + sum(1 for k in ss if "[" in k)
    print(
        f"  granularity control: OK — {stateful} stateful buckets, every pooled\n"
        f"  floor at least its own worst state. A zero below is a reading."
    )

    rc = 0
    for k in ["bare", "state"] if key == "both" else [key]:
        keyfn = KEYFN[k]
        miss_f, surp_f = (bm, bs) if k == "bare" else (sm, ss)
        per_run = floors(ar, rr, refs, keyfn)[2]
        keys, miss_c, surp_c = charge_of(rc_, refs, cand, keyfn)
        print(
            f"\n  ==== KEY = {k.upper()} "
            f"({'block name, states pooled' if k == 'bare' else 'full block state'})"
        )
        print(
            f"  charge {len(keys):,} of {blocks:,} over N={len(refs)}; "
            f"innocent leave-one-out charges "
            f"{min(v for _, v in per_run):,}..{max(v for _, v in per_run):,}"
        )
        report("VANILLA SPELLING — floors a MISS", miss_c, miss_f, top)
        report("WE PLACED — floors a SURPLUS", surp_c, surp_f, top)
        print(
            "    NOTHING IS SUBTRACTED. A floor sits beside its charge and the\n"
            "    ratio is quoted, never the difference."
        )
        if expect_charge is not None and len(keys) != int(expect_charge.replace(",", "")):
            print(
                f"RANK RED: charge is {len(keys):,}, expected {expect_charge}",
                file=sys.stderr,
            )
            rc = 1
    return rc


def selftest() -> int:
    """Prove this script can fail, on fixtures, with no box and no jar.

    The mutation that motivated the file is the one that must not survive:
    keying the charge bare and the floor by state. The fixture is built so
    that mistake changes a printed floor from 3 to 0 — i.e. so the bug is
    VISIBLE in the output rather than merely present in the code.
    """
    import tempfile

    ok = True

    def expect(lbl, got, want):
        nonlocal ok
        if got == want:
            print(f"SELFTEST ok   [{lbl}]")
        else:
            print(f"SELFTEST FAIL [{lbl}]: got {got!r} want {want!r}")
            ok = False

    with tempfile.TemporaryDirectory() as d:
        def w(name, lines):
            with open(os.path.join(d, name), "w", encoding="utf-8") as fh:
                for x, y, z, a, b in lines:
                    fh.write(f"{x} {y} {z} {a} {b}\n")
                fh.write(
                    f"DIFF chunks=4 (a=4 b=4) blocks=1000 mismatches={len(lines)}\n"
                )

        L = "minecraft:oak_leaves[distance=%d,persistent=false]"
        # Three references. Each innocent pair disagrees on leaves in a way
        # that pools to 3 under `bare` and splits 2/1 under `state`, so the two
        # granularities MUST print different floors — a script that silently
        # used one key for the charge and the other would show it here.
        for a, b in [(x, y) for x in "abc" for y in "abc" if x != y]:
            w(f"{a}-vs-{b}.pos", [
                (1, 70, 1, L % 2, "minecraft:air"),
                (2, 70, 1, L % 2, "minecraft:air"),
                (3, 70, 1, L % 3, "minecraft:air"),
            ])
        # The candidate is charged on four leaf positions, two of each state.
        for r in "abc":
            w(f"{r}-vs-ours.pos", [
                (1, 70, 1, L % 2, "minecraft:air"),
                (2, 70, 1, L % 2, "minecraft:air"),
                (3, 70, 1, L % 3, "minecraft:air"),
                (4, 70, 1, L % 3, "minecraft:air"),
            ])

        import io
        import contextlib

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = run(d, list("abc"), d, "ours", "both", 20, "selftest", "4")
        out = buf.getvalue()
        expect("both-key run is green", rc, 0)

        # The whole point: pooled, oak_leaves is charged 4 against a floor of 3.
        expect(
            "bare pools the states and carries a NONZERO floor",
            "oak_leaves                                          4       3" in out,
            True,
        )
        # And per state it is 2 against 2 and 2 against 1 — the same charge,
        # a different and equally honest reading. If a bare charge were looked
        # up in this state-keyed table it would read 4 against 0.
        expect(
            "state keeps them apart",
            f"{'oak_leaves[distance=3,persistent=false]':<44}{2:>9,}{1:>8,}" in out,
            True,
        )
        expect("no zero floor is printed for a sampled bucket", "unbounded" in out, False)

        # A candidate inside the pool is a category error, not a smaller
        # number. Each refusal is run with its noise captured but its verdict
        # printed — a selftest whose own results are swallowed is the failure
        # this repository names "a gate that cannot fail".
        def quiet(*a):
            b = io.StringIO()
            with contextlib.redirect_stdout(b), contextlib.redirect_stderr(b):
                return run(*a)

        # ---- THE POSITIVE CONTROL, broken on purpose ----------------------
        #
        # The invariant is only worth having if it fires, so the exact 2026
        # defect is reconstructed here: a floor pooled to the bare name that
        # has lost its states and reads zero, against a state table that says
        # 113. `granularity_violations` is the production path — this is the
        # same function the run above calls, not a copy of it.
        state_side = {
            "spruce_leaves[distance=2,persistent=false,waterlogged=false]": 113,
            "spruce_leaves[distance=3,persistent=false,waterlogged=false]": 85,
            "air": 579,
        }
        healthy = {"spruce_leaves": 262, "air": 579}
        expect(
            "control is silent when the pooling is right",
            granularity_violations(healthy, state_side),
            [],
        )
        missed = {"air": 579}  # the stateful key missed; dict.get -> 0
        fired = granularity_violations(missed, state_side)
        expect("control FIRES on a missed stateful lookup", len(fired), 1)
        expect(
            "and it names the block, the zero and the state that contradicts it",
            fired[0][:3],
            ("spruce_leaves", 0, 113),
        )
        # A pooled floor BELOW its worst state is impossible even when nonzero,
        # so the control is not merely a zero-detector.
        expect(
            "control fires on a too-small nonzero pooling too",
            len(granularity_violations({"spruce_leaves": 40, "air": 579}, state_side)),
            1,
        )
        # And it must not fire on a legitimately absent block — `blue_ice` and
        # `sulfur_spike` really were never charged, in either table.
        expect(
            "a genuinely absent block is not a violation",
            granularity_violations({}, {}),
            [],
        )

        expect("cand in refs refused", quiet(d, list("abc"), d, "b", "bare", 5, "", None), 2)
        expect("two references refused", quiet(d, list("ab"), d, "ours", "bare", 5, "", None), 2)
        expect("missing pair refused", quiet(d, list("abz"), d, "ours", "bare", 5, "", None), 2)
        expect(
            "wrong --expect-charge is RED",
            quiet(d, list("abc"), d, "ours", "bare", 5, "", "5"),
            1,
        )
    print("SELFTEST GREEN" if ok else "SELFTEST RED")
    return 0 if ok else 1


def main(argv):
    if argv[:1] == ["--selftest"]:
        return selftest()
    if not argv:
        print(__doc__.split("usage:")[1].split("exit 0")[0].strip(), file=sys.stderr)
        return 2
    posdir, rest = argv[0], argv[1:]
    opts = {}
    i = 0
    known = {
        "--refs", "--cand", "--cand-pos", "--key", "--top", "--label",
        "--expect-charge",
    }
    while i < len(rest):
        if rest[i] not in known:
            print(f"unknown option {rest[i]!r}", file=sys.stderr)
            return 2
        if i + 1 >= len(rest):
            print(f"{rest[i]} needs a value", file=sys.stderr)
            return 2
        opts[rest[i]] = rest[i + 1]
        i += 2
    if "--refs" not in opts or "--cand" not in opts:
        print("--refs and --cand are required", file=sys.stderr)
        return 2
    key = opts.get("--key", "bare")
    if key not in ("bare", "state", "both"):
        print("--key takes bare, state or both", file=sys.stderr)
        return 2
    return run(
        posdir,
        [r for r in opts["--refs"].split(",") if r],
        opts.get("--cand-pos", posdir),
        opts["--cand"],
        key,
        int(opts.get("--top", "20")),
        opts.get("--label", ""),
        opts.get("--expect-charge"),
    )


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
