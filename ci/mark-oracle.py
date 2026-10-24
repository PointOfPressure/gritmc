#!/usr/bin/env python3
"""mark-oracle.py — compare our post-processing mark set against vanilla's,
on the only chunks where that comparison means anything.

usage:
  ci/mark-oracle.py <vanilla-ref> <vanilla-twin> <our-region-dir> <our-mark-dump>
                    [cx0 cz0 cx1 cz1]

`<our-mark-dump>` is what `RUSTMC_DUMP_POST_MARKS=<path>` writes: one
`x y z state` per line, in insertion order.


## Why this exists, and what it corrects

`LevelChunk.postProcessGeneration` clears the `PostProcessing` lists and
nothing else does, so a chunk saved at FULL whose pass never ran keeps them.
In a forceloaded square the interior is post-processed and the border ring is
not, which makes the border ring vanilla's own mark set — position by
position, duplicates included. That is `docs/ORACLE.md` §23 and it is right.

What §23 then did with it was not. It picked **one** chunk column, compared
mark sets there, and read the difference as missing calls on our side. The
column's *blocks* were never compared. They differ by 6,688 — vanilla builds
a trial chamber in chunk column 55 and we build a sulfur cave — while
vanilla's own twin differs from it by 444 with **no** `tuff_bricks`
disagreement at all. So the two mark sets being compared belonged to two
different worlds, and the resulting "2,261 missing marks" was mostly a
different world, not a missing call.

The border ring is where marks survive **and** the ring is exactly where the
two runs' generated extents disagree most: vanilla's ring is the outermost
chunk that reached FULL, so nothing beyond it ever decorated inward, while
ours has real neighbours there. That is not a bug in either world. It just
means a border chunk is only a mark oracle if it is also a *block* oracle.

So this script uses only the chunks that are both:

  * still carrying their `PostProcessing` list — the pass never ran there; and
  * **byte-identical to vanilla**, twin-masked, over every section.

On `oracle-work/vanilla-r7` that is 17-plus chunks and about 2,750 marks,
against 4,296 in the contaminated column. Fewer marks, and every one of them
a fact.


## Twin masking, which is ORACLE rule 6, and it applies TWICE

A position is only allowed to accuse us if the two independent vanilla runs
spell it the same way. Kelp, seagrass and the ore blobs differ between
vanilla's own runs; masking those is what promotes several more border chunks
into the oracle rather than throwing them out for noise that was never ours.

The mask has to cover the **marks** as well as the blocks, and the first
version of this script masked only the blocks. That is not a smaller version
of the same guarantee — it is a hole shaped exactly like the thing being
measured. It reported two marks as ours-and-not-vanilla's at (1179,26,1022)
and (1180,6,1017); vanilla's first run has air at both and vanilla's second
has a glow lichen at both. A position vanilla does not agree with itself
about cannot be evidence about us in *either* channel.

So a missing mark must be in both references, and an extra mark must be in
neither.


## What a difference means, in each direction

  * **in vanilla, not ours** — a `markPosForPostProcessing` call site we have
    not ported, or a predicate in front of one that is too narrow.
  * **in ours, not vanilla** — a call we make that vanilla does not. This has
    been zero every time it has been measured on the clean set, and it is the
    number that makes the other direction usable: with no false marks, every
    missing one is a missing call rather than a disagreement.

Pure stdlib, reads region files only, and is meant to run on the laptop.
"""
import sys
import os
import importlib.util
from collections import Counter

_HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location(
    'chunk_ticks_probe', os.path.join(_HERE, 'chunk-ticks-probe.py'))
ctp = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ctp)

# `ProtoChunk`'s section lists start at the world floor. The overworld's is
# y=-64, section -4. A dimension with a different floor needs this changed,
# which is why it is named rather than inlined.
YMIN_SECTION = -4


def twin_masked_mismatches(a, b, twin, cx, cz):
    """Positions where `a` and `twin` agree and `b` does not.

    `a` is vanilla, `b` is ours, `twin` is vanilla's second run. A section
    missing from any of the three is skipped rather than counted: an absent
    section is a question the file does not answer, not a zero.
    """
    n = 0
    for sy in range(YMIN_SECTION, 20):
        ka, kb, kt = a.get((cx, cz, sy)), b.get((cx, cz, sy)), twin.get((cx, cz, sy))
        if not ka or not kb or not kt:
            continue
        pa, ia = ka
        pb, ib = kb
        pt, it = kt
        for i in range(4096):
            va = pa[ia[i]]
            if va != pt[it[i]]:
                continue
            if va != pb[ib[i]]:
                n += 1
    return n


def decode(P, cx, cz):
    """One chunk's `PostProcessing` lists, in call order.

    `packOffsetCoordinates` is `x&15 | (y&15) << 4 | (z&15) << 8`, one
    `ShortList` per section in ascending section order, so the list index
    carries the section and the short carries only the offset within it.
    """
    out = []
    for index, lst in enumerate(P or []):
        if not isinstance(lst, list):
            continue
        sy = YMIN_SECTION + index
        for packed in lst:
            p = packed & 0xFFFF
            out.append((cx * 16 + (p & 15),
                        sy * 16 + ((p >> 4) & 15),
                        cz * 16 + ((p >> 8) & 15)))
    return out


def main():
    if len(sys.argv) not in (5, 9):
        print(__doc__)
        return 2
    ref, twin_dir, ours_dir, dump = sys.argv[1:5]
    if len(sys.argv) == 9:
        cx0, cz0, cx1, cz1 = map(int, sys.argv[5:9])
    else:
        cx0, cz0, cx1, cz1 = 55, 55, 73, 73

    sv, _, meta = ctp.load(ref, cx0, cz0, cx1, cz1)
    st, _, meta_twin = ctp.load(twin_dir, cx0, cz0, cx1, cz1)
    so, _, _ = ctp.load(ours_dir, cx0, cz0, cx1, cz1)
    twin_lists = {(cx, cz): P for cx, cz, _s, count, P in meta_twin if count}

    clean, dirty, dirty_marks = [], 0, 0
    for cx, cz, _status, count, P in meta:
        if not count:
            continue
        # The twin must also still be carrying its list. A chunk whose pass
        # ran in one reference and not the other compares a cleared list
        # against an uncleared one, which is the mistake §23 made and
        # corrected at the level of chunk columns.
        if (cx, cz) not in twin_lists:
            dirty += 1
            dirty_marks += count
            continue
        if twin_masked_mismatches(sv, so, st, cx, cz) == 0:
            clean.append((cx, cz, P))
        else:
            dirty += 1
            dirty_marks += count

    if not clean:
        print('no border chunk is both uncleared and block-identical — '
              'the oracle has nothing to stand on')
        return 1

    entries, twin_entries = [], []
    for cx, cz, P in clean:
        entries.extend(decode(P, cx, cz))
        twin_entries.extend(decode(twin_lists[(cx, cz)], cx, cz))
    seen_a, seen_b = set(entries), set(twin_entries)
    # Rule 6, on the marks: accuse in a direction only where both runs agree.
    vanilla = seen_a & seen_b
    disputed = seen_a ^ seen_b
    box = {(cx, cz) for cx, cz, _ in clean}

    ours = set()
    for line in open(dump):
        f = line.split()
        if len(f) < 3:
            continue
        x, y, z = int(f[0]), int(f[1]), int(f[2])
        if (x >> 4, z >> 4) in box:
            ours.add((x, y, z))

    missing = vanilla - ours
    extra = ours - seen_a - seen_b
    print(f'oracle chunks {len(clean)} clean, {dirty} rejected for '
          f'{dirty_marks} marks in blocks we do not reproduce')
    print(f'vanilla {len(vanilla)} positions both runs mark '
          f'({len(entries) - len(seen_a)} duplicate entries, '
          f'{len(disputed)} disputed between the two runs and masked out)')
    print(f'ours    {len(ours)} in the same chunks')
    print(f'MISSING {len(missing)}   EXTRA {len(extra)}')

    for label, hits in (('missing', missing), ('extra', extra)):
        if not hits:
            continue
        # The state printed is vanilla's, and it is the **pre-pass** block:
        # these chunks are exactly the ones whose pass never ran. It sorts the
        # set by cause; it is not a claim about what stands there in play.
        c = Counter((ctp.at(sv, *p) or '?').split('[')[0] for p in hits)
        print(f'  {label} by the block standing on it:')
        for k, v in c.most_common(12):
            print(f'    {v:5d}  {k}')
        print(f'  {label} positions:')
        for p in sorted(hits)[:40]:
            print('    %d %d %d  %s' % (p + ((ctp.at(sv, *p) or '?'),)))
        if len(hits) > 40:
            print(f'    ... {len(hits) - 40} more')
    return 0


if __name__ == '__main__':
    sys.exit(main())
