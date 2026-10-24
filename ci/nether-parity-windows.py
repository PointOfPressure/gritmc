#!/usr/bin/env python3
"""nether-parity-windows.py — our nether against EVERY vanilla world of each window.

Vanilla's own nether harvest is not reproducible: three harvests of one
window are three different worlds, and `-Dmax.bg.threads=1` does not clamp
it in this dimension (docs/DETERMINISM.md §46). So a figure taken against one
reference, or against one twin-masked pair, is a figure about that pair. This
script takes every same-order reference of each window and reports, per biome
as **vanilla records it at each position**:

  ours   ours against each reference, unmasked: N numbers, printed min/med/max
  floor  each reference against each other one: N(N-1)/2 numbers, min/med/max
  x      our median over the floor's median — under ~1 the window no longer
         separates us from vanilla, and a level read off it is not a finding

Run counts are printed beside every row: `ours n=N`, `floor n=N(N-1)/2`.
A reference whose `HARVEST-ORDER` names a `dump_order` other than the one our
dump was taken in is refused, never mixed in (AGENT-BRIEF: a floor belongs to
one harvest order). The `rr` references are rows_rev and are a different
ensemble; they are skipped by name and said so.

    ci/nether-parity-windows.py [--tag TAG] [--only WINDOW]... [--out DIR]

Dumps go to `$OUT/<TAG>/<window>`; an existing complete dump is reused, so a
second TAG is the before/after control. Exits 2 on a missing binary, a binary
older than the tree, or a window with fewer than two references.
"""

import argparse
import itertools
import os
import re
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DUMP = ROOT / "target/release/worldgen-dump"
DIFF = ROOT / "target/release/region-differ"

# name, seed, radius, centre chunk x, centre chunk z, reference glob prefix.
# A name starting `end-` is an End window: dumped with `full-end` and
# compared over y 0..255 (the End generates its full 256), where a nether
# window uses `full-nether` and y 0..127.
WINDOWS = [
    ("end-r7-20260722-c0_0", 20260722, 7, 0, 0, "van-end-20260722-c0_0-"),
    ("end-r7-20260722-c8362", 20260722, 7, 83, 62, "van-end-20260722-c8362-"),
    ("end-r16-20260722-c6464", 20260722, 16, 64, 64, "gen-wide-20260722-r16-end-"),
    # Harvested 2026-10-04 on `end-parity`, with `entities/`: the main island
    # (spikes, cages, crystals) at three seeds, and the two return gateways
    # seed 20260722 places within 3,200 blocks of the origin.
    ("end-r7-12345-c0_0", 12345, 7, 0, 0, "van-end-12345-c0_0-ent-"),
    ("end-r7-31415926-c0_0", 31415926, 7, 0, 0, "van-end-31415926-c0_0-ent-"),
    ("end-r7-20260722-c0_0-ent", 20260722, 7, 0, 0, "van-end-20260722-c0_0-ent-"),
    ("end-r7-20260722-c-99_-5", 20260722, 7, -99, -5, "van-end-20260722-c-99_-5-gw-"),
    ("end-r7-20260722-c116_109", 20260722, 7, 116, 109, "van-end-20260722-c116_109-gw-"),
    # Two end cities WITH a ship, which c(83,62)'s city lacks.
    ("end-r7-20260722-c81_3", 20260722, 7, 81, 3, "van-end-20260722-c81_3-ship-"),
    ("end-r7-20260722-c107_-37", 20260722, 7, 107, -37, "van-end-20260722-c107_-37-ship-"),
    # The outer End at the two other seeds, each around a ship city.
    ("end-r7-12345-c-16_83", 12345, 7, -16, 83, "van-end-12345-c-16_83-ship-"),
    ("end-r7-31415926-c25_87", 31415926, 7, 25, 87, "van-end-31415926-c25_87-ship-"),
    # 2026-10-04: outer islands at three seeds, r=24, GEN_ORDER=rows.
    ("end-r24-12345-c130_-70", 12345, 24, 130, -70, "van-end-12345-c130_-70-r24-"),
    ("end-r24-31415926-c130_-70", 31415926, 24, 130, -70, "van-end-31415926-c130_-70-r24-"),
    ("end-r24-20260722-c95_-20", 20260722, 24, 95, -20, "van-end-20260722-c95_-20-r24-"),
    # 2026-10-04: far out (x -50,000, z 100,000 blocks), for precision.
    ("end-r7-20260722-c-3125_6250", 20260722, 7, -3125, 6250, "van-end-20260722-c-3125_6250-far-"),
    ("end-r7-12345-c-3125_6250", 12345, 7, -3125, 6250, "van-end-12345-c-3125_6250-far-"),
    ("r7-20260722-c6464", 20260722, 7, 64, 64, "van-nether-20260722-c6464-"),
    ("r7-20260722-c100100", 20260722, 7, 100, 100, "van-nether-20260722-c100100-"),
    ("r7-31415926-c6464", 31415926, 7, 64, 64, "van-nether-31415926-c6464-"),
    ("r7-31415926-c0_0", 31415926, 7, 0, 0, "van-nether-31415926-c0_0-"),
    ("r7-31415926-c-200_200", 31415926, 7, -200, 200, "van-nether-31415926-c-200_200-"),
    ("r16-20260722-c6464", 20260722, 16, 64, 64, "gen-wide-20260722-r16-nether-"),
    # Harvested 2026-10-02 on `nether-parity` around the two fortress starts
    # seed 31415926 has near the forest windows, so a fortress is measured.
    ("r7-31415926-c-189_198", 31415926, 7, -189, 198, "van-nether-31415926-c-189_198-"),
    ("r7-31415926-c12_3", 31415926, 7, 12, 3, "van-nether-31415926-c12_3-"),
]

ROW = re.compile(r"^\s+([a-z_]+)\s+(\d+)\s+(\d+)\s+[\d.]+%")


def refuse(msg):
    print(f"REFUSED: {msg}", file=sys.stderr)
    sys.exit(2)


def by_biome(a, b, bbox, top=127):
    out = subprocess.run(
        [str(DIFF), "diff", str(a), str(b), "--bbox", *map(str, bbox),
         "--ybox", "0", str(top), "--by-biome"],
        capture_output=True, text=True, check=False,
    ).stdout
    rows = {}
    total = None
    for line in out.splitlines():
        m = re.match(r"^DIFF .* blocks=(\d+) mismatches=(\d+)", line)
        if m:
            total = (int(m.group(1)), int(m.group(2)))
        m = ROW.match(line)
        if m:
            rows[m.group(1)] = (int(m.group(2)), int(m.group(3)))
    if total is None:
        refuse(f"region-differ printed no DIFF line for {a} vs {b}:\n{out[:400]}")
    return total, rows


def order_of(ref):
    try:
        text = (ref / "HARVEST-ORDER").read_text()
    except OSError:
        return None
    m = re.search(r"^dump_order=(\S+)", text, re.M)
    return m.group(1) if m else None


def stats(values):
    if not values:
        return "-"
    return f"{min(values)}/{int(statistics.median(values))}/{max(values)}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", default="current")
    ap.add_argument("--only", action="append", default=[])
    ap.add_argument("--out", default=os.path.expanduser("~/nether-parity-windows"))
    ap.add_argument("--refs", default=str(ROOT / "oracle-work"))
    args = ap.parse_args()

    for b in (DUMP, DIFF):
        if not b.exists():
            refuse(f"{b} missing — cargo build --release -p worldgen-dump -p region-differ")
    newest = max(
        p.stat().st_mtime
        for d in ("crates", "tools")
        for p in (ROOT / d).rglob("*.rs")
    )
    if DUMP.stat().st_mtime < newest:
        refuse(f"{DUMP} is older than the tree; it would measure another build")

    refs_dir = Path(args.refs)
    for name, seed, r, cx, cz, prefix in WINDOWS:
        if args.only and name not in args.only:
            continue
        refs = sorted(
            p for p in refs_dir.glob(prefix + "*")
            if p.is_dir() and not p.name[len(prefix):].startswith("rr-")
            # `van-end-20260722-c8362` and `-b` predate HARVEST-ORDER; only
            # the stamped `open-*` set is an ensemble.
            and (p / "HARVEST-ORDER").exists()
        )
        orders = {p: order_of(p) for p in refs}
        missing = [p.name for p, o in orders.items() if o is None]
        if missing:
            refuse(f"{name}: no dump_order stamped on {missing}")
        distinct = set(orders.values())
        if len(distinct) != 1:
            refuse(f"{name}: references mix dump orders {distinct}")
        order = distinct.pop()
        if len(refs) < 2:
            refuse(f"{name}: {len(refs)} reference(s); a floor needs two")
        bbox = (cx - r, cz - r, cx + r, cz + r)

        ours = Path(args.out) / args.tag / name
        if not (ours / "region").is_dir() or not any((ours / "region").glob("*.mca")):
            ours.mkdir(parents=True, exist_ok=True)
            env = dict(os.environ, RUSTMC_DECORATE_ORDER=order)
            with open(f"{ours}.log", "w") as log:
                rc = subprocess.run(
                    ["nice", "-n", "10", str(DUMP),
                     "full-end" if name.startswith("end-") else "full-nether", str(seed), str(r),
                     str(ours), str(cx), str(cz)],
                    env=env, stdout=log, stderr=subprocess.STDOUT, check=False,
                ).returncode
            if rc != 0:
                refuse(f"{name}: worldgen-dump exited {rc}, see {ours}.log")

        top = 255 if name.startswith("end-") else 127
        mine = [by_biome(ref, ours / "region", bbox, top) for ref in refs]
        pairs = [by_biome(a, b, bbox, top) for a, b in itertools.combinations(refs, 2)]

        print(f"=== {name} seed={seed} r={r} centre=({cx},{cz}) dump_order={order} "
              f"refs={len(refs)} [{', '.join(p.name[len(prefix):] for p in refs)}]")
        tot_ours = [t[1] for t, _ in mine]
        tot_floor = [t[1] for t, _ in pairs]
        print(f"  {'ALL':18} blocks={mine[0][0][0]:>9}  ours n={len(tot_ours)} "
              f"{stats(tot_ours):>24}  floor n={len(tot_floor)} {stats(tot_floor):>24}")
        biomes = sorted({b for _, rows in mine for b in rows})
        for biome in biomes:
            blocks = max(rows.get(biome, (0, 0))[0] for _, rows in mine)
            o = [rows.get(biome, (0, 0))[1] for _, rows in mine]
            f = [rows.get(biome, (0, 0))[1] for _, rows in pairs]
            fm = statistics.median(f) if f else 0
            ratio = f"{statistics.median(o) / fm:.2f}x" if fm else "inf"
            print(f"  {biome:18} blocks={blocks:>9}  ours n={len(o)} {stats(o):>24}  "
                  f"floor n={len(f)} {stats(f):>24}  {ratio}")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
