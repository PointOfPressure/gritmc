#!/usr/bin/env python3
"""chunk-ticks-probe.py — read saved Anvil regions and report the things a
block diff cannot see: chunk `Status`, the leftover `PostProcessing` lists, the
pending `block_ticks`/`fluid_ticks`, and every non-source fluid block with its
immediate neighbourhood.

It exists because a block diff cannot distinguish "vanilla's generator wrote
this" from "vanilla's server simulated it after generating". The tick lists can:
a scheduled tick still sitting in the file at `t=0` is a tick that was DUE and
never ran, which is proof the game loop never turned. Conversely an empty
`PostProcessing` list is proof that `LevelChunk.postProcessGeneration` DID run,
because clearing those lists is the last thing it does.

Pure stdlib. It reads eight files with zlib and is the one probe meant to run on
the laptop — it touches neither rustc nor a JVM.

usage:
  chunk-ticks-probe.py summary <regiondir> <cx0> <cz0> <cx1> <cz1>
  chunk-ticks-probe.py fluid   <regiondir> <cx0> <cz0> <cx1> <cz1>   # + context
  chunk-ticks-probe.py marks   <regiondir> <cx0> <cz0> <cx1> <cz1> [ymin_section]
"""
import sys, os, zlib, struct, gzip
from collections import Counter

TAG_END, TAG_BYTE, TAG_SHORT, TAG_INT, TAG_LONG = 0, 1, 2, 3, 4
TAG_FLOAT, TAG_DOUBLE, TAG_BA, TAG_STR = 5, 6, 7, 8
TAG_LIST, TAG_COMP, TAG_IA, TAG_LA = 9, 10, 11, 12


class R:
    def __init__(s, b): s.b, s.i = b, 0
    def u1(s): v = s.b[s.i]; s.i += 1; return v
    def i1(s): v = struct.unpack_from('>b', s.b, s.i)[0]; s.i += 1; return v
    def i2(s): v = struct.unpack_from('>h', s.b, s.i)[0]; s.i += 2; return v
    def u2(s): v = struct.unpack_from('>H', s.b, s.i)[0]; s.i += 2; return v
    def i4(s): v = struct.unpack_from('>i', s.b, s.i)[0]; s.i += 4; return v
    def i8(s): v = struct.unpack_from('>q', s.b, s.i)[0]; s.i += 8; return v
    def f4(s): v = struct.unpack_from('>f', s.b, s.i)[0]; s.i += 4; return v
    def f8(s): v = struct.unpack_from('>d', s.b, s.i)[0]; s.i += 8; return v
    def st(s):
        n = s.u2(); v = s.b[s.i:s.i + n].decode('utf-8', 'replace'); s.i += n
        return v


def payload(r, t):
    if t == TAG_BYTE: return r.i1()
    if t == TAG_SHORT: return r.i2()
    if t == TAG_INT: return r.i4()
    if t == TAG_LONG: return r.i8()
    if t == TAG_FLOAT: return r.f4()
    if t == TAG_DOUBLE: return r.f8()
    if t == TAG_BA:
        n = r.i4(); v = r.b[r.i:r.i + n]; r.i += n; return v
    if t == TAG_STR: return r.st()
    if t == TAG_LIST:
        et = r.u1(); n = r.i4()
        return [payload(r, et) for _ in range(n)] if n > 0 else []
    if t == TAG_COMP:
        d = {}
        while True:
            tt = r.u1()
            if tt == TAG_END: return d
            # The name MUST be read into a local before the payload. Python
            # evaluates the right-hand side of `d[r.st()] = payload(r, tt)`
            # first, which reads the payload before the name and desyncs the
            # stream — silently. It parses, it just yields a chunk with no
            # `Status` and no fluid, i.e. a clean-looking zero.
            nm = r.st()
            d[nm] = payload(r, tt)
    if t == TAG_IA:
        n = r.i4(); v = list(struct.unpack_from('>%di' % n, r.b, r.i)); r.i += 4 * n
        return v
    if t == TAG_LA:
        n = r.i4(); v = list(struct.unpack_from('>%dq' % n, r.b, r.i)); r.i += 8 * n
        return v
    raise ValueError('tag %d' % t)


def parse_nbt(data):
    r = R(data); t = r.u1()
    if t == TAG_END: return {}
    r.st()
    return payload(r, t)


def region_chunks(path):
    """yields (localX, localZ, nbt) for every stored chunk in one .mca"""
    with open(path, 'rb') as f:
        buf = f.read()
    if len(buf) < 8192: return
    for idx in range(1024):
        off, = struct.unpack_from('>I', buf, idx * 4)
        sec = off >> 8
        if sec == 0: continue
        p = sec * 4096
        if p + 5 > len(buf): continue
        ln, = struct.unpack_from('>I', buf, p)
        comp = buf[p + 4]
        raw = buf[p + 5:p + 4 + ln]
        try:
            if comp == 1: d = gzip.decompress(raw)
            elif comp == 2: d = zlib.decompress(raw)
            elif comp == 3: d = raw
            else: continue
            nbt = parse_nbt(d)
        except Exception:
            continue
        yield (idx % 32, idx // 32, nbt)


def statestr(e):
    n = e.get('Name', '?')
    p = e.get('Properties')
    if not p: return n
    return n + '[' + ','.join('%s=%s' % (k, p[k]) for k in sorted(p)) + ']'


def unpack_states(sec):
    bs = sec.get('block_states')
    if not bs: return None, None
    pal = [statestr(x) for x in bs.get('palette', [])]
    data = bs.get('data')
    if data is None: return pal, [0] * 4096
    bits = max(4, (len(pal) - 1).bit_length())
    per = 64 // bits
    mask = (1 << bits) - 1
    out = []
    for w in data:
        w &= (1 << 64) - 1
        for k in range(per):
            if len(out) >= 4096: break
            out.append((w >> (k * bits)) & mask)
    while len(out) < 4096:
        out.append(0)
    return pal, out


def load(regdir, cx0, cz0, cx1, cz1):
    secs, ticks, meta = {}, [], []
    for fn in sorted(os.listdir(regdir)):
        if not fn.endswith('.mca'): continue
        p = fn.split('.')
        rx, rz = int(p[1]), int(p[2])
        for lx, lz, nbt in region_chunks(os.path.join(regdir, fn)):
            cx, cz = rx * 32 + lx, rz * 32 + lz
            if not (cx0 <= cx <= cx1 and cz0 <= cz <= cz1): continue
            P = nbt.get('PostProcessing')
            meta.append((cx, cz, nbt.get('Status', '?'),
                         None if P is None else
                         sum(len(x) for x in P if isinstance(x, list)),
                         P))
            for key in ('block_ticks', 'fluid_ticks'):
                for t in (nbt.get(key) or []):
                    ticks.append((key, t.get('i'), t.get('x'), t.get('y'),
                                  t.get('z'), t.get('t'), t.get('p')))
            for sec in nbt.get('sections', []):
                pal, idxs = unpack_states(sec)
                if pal: secs[(cx, cz, sec['Y'])] = (pal, idxs)
    return secs, ticks, meta


def at(secs, x, y, z):
    e = secs.get((x >> 4, z >> 4, y >> 4))
    if not e: return None
    pal, idxs = e
    return pal[idxs[((y & 15) << 8) | ((z & 15) << 4) | (x & 15)]]


def nonsource(secs):
    hits = []
    for (cx, cz, sy), (pal, idxs) in secs.items():
        bad = {j: s for j, s in enumerate(pal)
               if (s.startswith('minecraft:water[') or s.startswith('minecraft:lava['))
               and 'level=0]' not in s}
        if not bad: continue
        for i, v in enumerate(idxs):
            if v in bad:
                hits.append((cx * 16 + (i & 15), sy * 16 + (i >> 8),
                             cz * 16 + ((i >> 4) & 15), bad[v]))
    return sorted(hits)


def marks(meta, secs, ymin_section):
    """Decode a chunk's surviving PostProcessing lists into world positions.

    `ProtoChunk.packOffsetCoordinates` is `x&15 | (y&15) << 4 | (z&15) << 8`,
    one `ShortList` per section in ascending section order, and the list index
    is the section index — so the y a short carries is only its offset inside
    the section it was stored in.

    This is readable at all because a chunk saved at FULL whose
    `postProcessGeneration` never ran still carries the lists: the pass clears
    them and nothing else does. In a forceloaded square the interior is
    cleared and the border ring is not, so the border is vanilla's own mark
    set, position by position.
    """
    out = []
    for cx, cz, status, count, P in meta:
        if not P:
            continue
        for index, lst in enumerate(P):
            if not isinstance(lst, list):
                continue
            sy = ymin_section + index
            for packed in lst:
                p = packed & 0xFFFF
                x = cx * 16 + (p & 15)
                y = sy * 16 + ((p >> 4) & 15)
                z = cz * 16 + ((p >> 8) & 15)
                out.append((x, y, z))
    return out


def main():
    mode = sys.argv[1]
    regdir = sys.argv[2]
    cx0, cz0, cx1, cz1 = map(int, sys.argv[3:7])
    secs, ticks, meta = load(regdir, cx0, cz0, cx1, cz1)
    hits = nonsource(secs)

    print('== %s   chunks (%d,%d)..(%d,%d)   %d chunks ==' %
          (regdir, cx0, cz0, cx1, cz1, len(meta)))
    print('Status: %s' % dict(Counter(m[2] for m in meta)))
    print('PostProcessing entries still marked: %d   '
          '(0 == postProcessGeneration ran and cleared them)' %
          sum(m[3] for m in meta if m[3] is not None))
    print('NON-SOURCE FLUID BLOCKS: %d' % len(hits))
    for k, v in sorted(Counter(h[3] for h in hits).items()):
        print('   %-40s %d' % (k, v))
    print('PENDING SCHEDULED TICKS  (t == delay remaining at save time;'
          ' t=0 means DUE and never executed)')
    for k, v in sorted(Counter((t[0], t[1], t[5]) for t in ticks).items(),
                       key=lambda kv: -kv[1]):
        print('   %-12s %-28s t=%-4s %d' % (k[0], k[1], k[2], v))

    if mode == 'marks':
        # -4 is the overworld's lowest section (y=-64). Passed rather than
        # derived because the nether and End start elsewhere.
        ymin = int(sys.argv[7]) if len(sys.argv) > 7 else -4
        positions = marks(meta, secs, ymin)
        # Every mark line is prefixed, because the summary above it is
        # human-readable prose and a `NF > 3` filter picks some of it up. A
        # mark set is compared with `comm`, so one stray line is a wrong
        # answer rather than a visible mess.
        print('\n== %d marked positions, with the state standing on them ==' %
              len(positions))
        for x, y, z in positions:
            print('MARK %d %d %d %s' % (x, y, z, at(secs, x, y, z)))
        return

    if mode != 'fluid':
        return
    tickmap = {(t[2], t[3], t[4]): t for t in ticks if t[0] == 'fluid_ticks'}
    print('\n== non-source fluid, with context ==')
    for x, y, z, s in hits:
        tk = tickmap.get((x, y, z))
        print('\n%-26s (%d,%d,%d)  own fluid tick: %s' %
              (s, x, y, z, ('%s t=%s' % (tk[1], tk[5])) if tk else 'NONE'))
        print('   up    %-28s down  %s' % (at(secs, x, y + 1, z), at(secs, x, y - 1, z)))
        print('   west  %-28s east  %s' % (at(secs, x - 1, y, z), at(secs, x + 1, y, z)))
        print('   north %-28s south %s' % (at(secs, x, y, z - 1), at(secs, x, y, z + 1)))
        best = None
        for dx in range(-3, 4):
            for dy in range(-3, 4):
                for dz in range(-3, 4):
                    if at(secs, x + dx, y + dy, z + dz) == 'minecraft:water[level=0]':
                        d = abs(dx) + abs(dy) + abs(dz)
                        if best is None or d < best[0]: best = (d, dx, dy, dz)
        print('   nearest source: %s' %
              (('manhattan %d at %+d%+d%+d' % best) if best else 'none within 3'))


if __name__ == '__main__':
    main()
