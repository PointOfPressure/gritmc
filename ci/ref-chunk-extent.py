#!/usr/bin/env python3
"""ref-chunk-extent.py — the widest window a reference can honestly be diffed on.

WHY THIS EXISTS. A reference directory holds region files, and a region file
covers 32x32 chunks whether or not those chunks were generated. So the presence
of `r.1.1.mca .. r.2.2.mca` looks like coverage of chunks 32..95 in both axes
and is nothing of the kind. On 2026-08-11 a brief reasoned from exactly that
listing that a 39x39 window (chunks 45..83) was "already inside the data" for
the seed-12345 r=16 pool. It is not: all 40 stamped references in that pool are
`minecraft:full` over chunks **46..82** only, and the ring at x or z in {45,83}
sits at `minecraft:initialize_light` — generated, but the outermost decorated
ring, so it never received the cross-border writes its own missing neighbours
would have made.

`minecraft:full` is not our judgement about a chunk, it is **vanilla's**: the
server writes that status only for a chunk it considers complete, and stops
short of it for the margin rings it generated in order to complete the others.
Reading it back off the reference is therefore the cheapest honest statement of
how wide that reference may be scored, and it costs no boots.

WHAT IT REFUSES. `--window A B C D` (chunk coordinates, inclusive, the same
four numbers `region-differ diff --bbox` takes) exits 3 if any chunk in that
window is not `minecraft:full` in any reference given, and prints how many and
where. A window one ring too wide is this project's DECORATION_MARGIN failure
by another door — ORACLE rule 14 — and it has already cost one window 604
blocks, which was more than its entire disagreement with vanilla.

usage:
  ref-chunk-extent.py <refdir>...                    report each one's extent
  ref-chunk-extent.py --window 46 46 82 82 <refdir>...   refuse if not covered
  ref-chunk-extent.py --centre 64 64 <refdir>...     largest centred square

exit 0 ok, 2 usage/unreadable, 3 the window is not covered.
"""
import sys
import os
import re
import struct
import zlib

FULL = 'minecraft:full'


def chunk_statuses(d):
    """{(cx, cz): status} for every chunk with a sector allocated in `d`."""
    out = {}
    try:
        names = os.listdir(d)
    except OSError as e:
        print(f'cannot read {d}: {e}', file=sys.stderr)
        sys.exit(2)
    for f in sorted(names):
        m = re.fullmatch(r'r\.(-?\d+)\.(-?\d+)\.mca', f)
        if not m:
            continue
        rx, rz = int(m.group(1)), int(m.group(2))
        with open(os.path.join(d, f), 'rb') as fh:
            header = fh.read(4096)
            if len(header) < 4096:
                continue
            for idx in range(1024):
                off = int.from_bytes(header[idx * 4:idx * 4 + 3], 'big')
                cnt = header[idx * 4 + 3]
                if off == 0 or cnt == 0:
                    continue
                cx, cz = rx * 32 + (idx % 32), rz * 32 + (idx // 32)
                fh.seek(off * 4096)
                raw = fh.read(cnt * 4096)
                if len(raw) < 5:
                    continue
                blen = struct.unpack('>I', raw[:4])[0]
                comp, payload = raw[4], raw[5:5 + blen - 1]
                try:
                    if comp == 1:
                        buf = zlib.decompress(payload, 47)
                    elif comp == 2:
                        buf = zlib.decompress(payload)
                    elif comp == 3:
                        buf = payload
                    else:
                        out[(cx, cz)] = f'<compression {comp}>'
                        continue
                except zlib.error:
                    out[(cx, cz)] = '<undecodable>'
                    continue
                # TAG_String(8) with a 6-byte name "Status" — the one place the
                # status is written, and cheaper than a full NBT parse.
                i = buf.find(b'\x08\x00\x06Status')
                if i < 0:
                    out[(cx, cz)] = '<nostatus>'
                    continue
                j = i + 9
                ln = struct.unpack('>H', buf[j:j + 2])[0]
                out[(cx, cz)] = buf[j + 2:j + 2 + ln].decode('utf-8', 'replace')
    return out


def full_bbox(st):
    f = [c for c, s in st.items() if s == FULL]
    if not f:
        return None
    xs, zs = [c[0] for c in f], [c[1] for c in f]
    return min(xs), min(zs), max(xs), max(zs), len(f)


def largest_centred_square(st, cx, cz):
    """Largest r with every chunk of [cx-r, cx+r] x [cz-r, cz+r] at full."""
    r = -1
    while True:
        n = r + 1
        ring = [(x, z)
                for x in range(cx - n, cx + n + 1)
                for z in range(cz - n, cz + n + 1)
                if abs(x - cx) == n or abs(z - cz) == n]
        if not all(st.get(c) == FULL for c in ring):
            return r
        r = n


def main():
    argv = sys.argv[1:]
    window = centre = None
    if '--window' in argv:
        i = argv.index('--window')
        try:
            window = tuple(int(v) for v in argv[i + 1:i + 5])
        except ValueError:
            print('--window needs four integers', file=sys.stderr)
            sys.exit(2)
        if len(window) != 4:
            print('--window needs four integers', file=sys.stderr)
            sys.exit(2)
        del argv[i:i + 5]
    if '--centre' in argv:
        i = argv.index('--centre')
        try:
            centre = tuple(int(v) for v in argv[i + 1:i + 3])
        except ValueError:
            print('--centre needs two integers', file=sys.stderr)
            sys.exit(2)
        del argv[i:i + 3]
    if not argv:
        print(__doc__.strip().splitlines()[-5], file=sys.stderr)
        sys.exit(2)

    bad_any = False
    for d in argv:
        st = chunk_statuses(d)
        bb = full_bbox(st)
        name = os.path.basename(os.path.normpath(d))
        if bb is None:
            print(f'{name}: NO full chunks at all')
            bad_any = True
            continue
        x0, z0, x1, z1, n = bb
        solid = n == (x1 - x0 + 1) * (z1 - z0 + 1)
        print(f'{name}: full bbox {x0} {z0} {x1} {z1}  n={n}  solid={solid}')
        if centre:
            r = largest_centred_square(st, *centre)
            lo, hi = centre[0] - r, centre[0] + r
            print(f'  largest square centred {centre[0]},{centre[1]}: '
                  f'r={r}  window {lo} {centre[1]-r} {hi} {centre[1]+r}')
        if window:
            a, b, c, e = window
            miss = [(x, z) for x in range(a, c + 1) for z in range(b, e + 1)
                    if st.get((x, z)) != FULL]
            if miss:
                bad_any = True
                kinds = sorted({st.get(m, '<absent>') for m in miss})
                xs, zs = [m[0] for m in miss], [m[1] for m in miss]
                print(f'  REFUSED window {a} {b} {c} {e}: {len(miss)} of '
                      f'{(c-a+1)*(e-b+1)} chunks are not {FULL}')
                print(f'    statuses seen: {", ".join(kinds)}')
                print(f'    they span x {min(xs)}..{max(xs)}  z {min(zs)}..{max(zs)}')
            else:
                print(f'  window {a} {b} {c} {e}: covered, all {FULL}')

    if window and bad_any:
        sys.exit(3)


if __name__ == '__main__':
    main()
