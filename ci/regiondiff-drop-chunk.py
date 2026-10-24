#!/usr/bin/env python3
"""Remove one chunk from a region file by clearing its location-table entry.

    ci/regiondiff-drop-chunk.py <dir> <chunk-x> <chunk-z>

This is the mutator `ci/t2-regiondiff.sh` uses to turn its own green red on
demand, and **the whole point of it is that it shares no code with the thing it
tests.** It reads no NBT, decompresses nothing, and knows nothing about
palettes, sections or block states. The Anvil location table is the first 4096
bytes of an `.mca`: 1024 entries of four bytes, at index
`(x & 31) + (z & 31) * 32`, holding a 3-byte big-endian sector offset and a
1-byte sector count. An entry of four zero bytes means "this chunk is not in
this file". That is the entire specification this script depends on, and it is
format, not implementation.

Why that matters: a control that mutated the subject with `rustmc-world`'s own
reader and writer would be measuring the parser with the parser. If both halves
of a round trip were wrong in the same direction the mutation would still look
correct and the control would still go green. Twenty lines of `struct.unpack`
cannot be wrong in the same direction as an NBT parser, because it is not an
NBT parser.

The effect is exactly computable without reading the chunk, which is what makes
it a known answer: `region-differ diff` compares only chunks present in **both**
directories, so dropping chunk `c` from one side removes exactly `c` from the
comparison — the denominator falls by one chunk's worth of blocks and the
mismatch count falls by exactly `c`'s own contribution. Both are numbers the
differ printed before the mutation, so the assertion is arithmetic, not a
tolerance.

Prints the region file it edited and the entry it cleared. Exits 2 if the chunk
was already absent, because a mutation that changed nothing would make the
control pass for the wrong reason — the one failure mode a falsification tool
must not have.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

SECTOR = 4096


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    d, cx, cz = Path(argv[1]), int(argv[2]), int(argv[3])
    rx, rz = cx >> 5, cz >> 5
    path = d / f"r.{rx}.{rz}.mca"
    if not path.exists():
        print(f"drop-chunk: no {path}", file=sys.stderr)
        return 2

    idx = (cx & 31) + (cz & 31) * 32
    off = idx * 4
    data = bytearray(path.read_bytes())
    if len(data) < SECTOR:
        print(f"drop-chunk: {path} is shorter than one sector", file=sys.stderr)
        return 2

    entry = bytes(data[off:off + 4])
    sector_off = struct.unpack(">I", b"\x00" + entry[:3])[0]
    count = entry[3]
    if sector_off == 0 and count == 0:
        print(f"drop-chunk: chunk ({cx},{cz}) is ALREADY ABSENT from {path.name} — "
              f"this mutation would change nothing", file=sys.stderr)
        return 2

    data[off:off + 4] = b"\x00\x00\x00\x00"
    path.write_bytes(bytes(data))
    print(f"drop-chunk: cleared ({cx},{cz}) from {path.name} "
          f"(was sector {sector_off}, {count} sectors)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
