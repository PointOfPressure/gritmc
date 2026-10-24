#!/usr/bin/env python3
"""Write the missing `vocabulary` into a floor file, from that floor's OWN
recordings, and touch nothing else.

# ===== AGENT INTERFACE =====
#
#     ./ci/floor-stamp-vocabulary.py ci/checks/gameplay-floor.toml r1.json ... r6.json
#     ./ci/floor-stamp-vocabulary.py --check ci/checks/gameplay-floor.toml    (tree-only)
#
# Refuses unless every recording's sha256 matches a `[[recording]]` row in the
# floor file, and unless the set of recordings is exactly the ensemble's.
# ===== end interface; everything below is why =====

# WHY THIS EXISTS AND WHY IT IS NOT `ci/corpus-floor.sh --update`

`docs/PARITY-ROADMAP.md` §50 built three outcomes — `NO VOCABULARY`,
`VOCABULARY STALE`, `VOCABULARY DRIFT` — so that a floor measured through one
decoder could not be quoted beside a run taken through another. The whole
mechanism was built for one concrete case: a recorder before 2026-08-09
digested `add_entity`'s four rotation bytes into `rest` and produced 91
`(kind, field)` pairs, and one after names them and produces 95.

**And the standing floor file records no `vocabulary` at all**, so
`ci/corpus-floor-report.py` prints `NO VOCABULARY` and withdraws the floor's
quotability on every corpus run, and `ci/corpus-floor.py check_floor_file`
appends `floor.vocabulary` to `bad` in any tree that holds a saved measurement
of the ensemble. §50.5 called that case "conditionally armed, and nothing says
so". It is armed.

**The reason it is unquotable is no longer true.** Measured 2026-08-18 from the
six recordings the floor names, whose sha256s still match:
`coverage92-selfcheck-n6-20260811` carries **95 pairs, identical in all six
runs**, including all four of `add_entity.{x_rot, y_rot, y_head_rot, data}`.
Against `~/rustmc-gate`'s corpus `vanilla.json` the drift is empty in both
directions. The floor was taken through the named-rotations recorder and agrees
with today's exactly; the only thing standing between it and being quotable is
a field nobody wrote.

# SO WHY NOT RE-MEASURE

Because re-measuring would answer a different question. `ci/corpus-floor.sh
--update` re-derives every number in the file through the projection of
**whatever tree runs it**, and the tree has moved since 2026-08-11. The file
declares `proj:7cefde0fb56d9f3e`; a re-measure would silently restamp it and
publish a floor of `4` that had been recomputed under a different comparator
while looking like the same figure. That is §15.4's defect exactly — one
projection id over two instruments — and `docs/cold-start/corpus-projection-audit.md`
says in as many words that the floor "must not be re-stamped from the
recordings on disk".

A vocabulary is not a measurement. It is the set of `(kind, field)` pairs
present in the recording FILES, read by `ci/trace_vocabulary.py` straight off
the JSON, with no comparator involved at all. Stamping it changes what the file
SAYS ABOUT ITS OWN INPUTS and cannot change any figure. This script therefore
writes exactly one key and refuses to write a second.

# THE SHA256 GUARD IS THE WHOLE SAFETY ARGUMENT

A vocabulary derived from the wrong recordings is worse than none: it would
make a floor look comparable to a run it is not comparable with, which is the
one failure the §50 machinery exists to prevent. So every recording offered
must hash to a `[[recording]]` row, every row must be matched, and the counts
must agree. Nothing is written otherwise.
"""

from __future__ import annotations

import hashlib
import os
import sys
import tomllib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import trace_vocabulary as tv  # noqa: E402


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main(argv: list[str]) -> int:
    if argv and argv[0] == "--check":
        if len(argv) != 2:
            print("usage: --check <floor.toml>", file=sys.stderr)
            return 2
        with open(argv[1], "rb") as f:
            d = tomllib.load(f)
        v = d.get("floor", {}).get("vocabulary")
        if not v:
            print(f"{argv[1]}: NO VOCABULARY — the floor does not say which "
                  f"(kind, field) pairs its recordings carried, so "
                  f"ci/corpus-floor-report.py has no baseline for VOCABULARY "
                  f"DRIFT and withdraws the floor's quotability on every run. "
                  f"Stamp it: ci/floor-stamp-vocabulary.py {argv[1]} <the "
                  f"ensemble's recordings>")
            return 1
        print(f"{argv[1]}: vocabulary {len(v)} pair(s), ensemble "
              f"{d['floor'].get('ensemble', '?')}")
        return 0

    # ONE recording is a legal invocation and must reach the SUBSET refusal,
    # not a usage error. The two are different answers: "you typed it wrong" is
    # exit 2 and says nothing about the ensemble, while "you offered 1 of 6" is
    # exit 1 and names the five that are missing and why a subset is the
    # dangerous direction. Conflating them made `ci/floor-stamp-vocabulary-falsify.sh`
    # case 2 unable to test the guard it exists for — the arg count swallowed
    # it — which is a small instance of the thing this whole script is about.
    if len(argv) < 2:
        print("usage: floor-stamp-vocabulary.py <floor.toml> r1.json r2.json ...",
              file=sys.stderr)
        return 2

    floor_path, recs = argv[0], argv[1:]
    with open(floor_path, "rb") as f:
        doc = tomllib.load(f)

    rows = doc.get("recording", [])
    if not rows:
        print("REFUSED: the floor file lists no [[recording]] rows, so nothing "
              "here can check that these recordings are its ensemble.",
              file=sys.stderr)
        return 1

    want = {r["sha256"]: r["id"] for r in rows}
    got: dict[str, str] = {}
    for p in recs:
        h = sha256(p)
        if h not in want:
            print(f"REFUSED: {p} (sha256 {h[:16]}) is not one of "
                  f"{doc['floor']['ensemble']}'s recordings. A vocabulary "
                  f"derived from the wrong files would make this floor look "
                  f"comparable to a run it is not comparable with, which is "
                  f"the one thing the §50 machinery exists to prevent.",
                  file=sys.stderr)
            return 1
        got[h] = p

    missing = sorted(want[h] for h in want if h not in got)
    if missing:
        print(f"REFUSED: {len(missing)} of the ensemble's {len(want)} "
              f"recordings were not offered: {', '.join(missing)}. A "
              f"vocabulary is the UNION over the ensemble, so a subset "
              f"understates it — and understating it is the direction that "
              f"produces a false VOCABULARY DRIFT on the next run.",
              file=sys.stderr)
        return 1

    per_run = {}
    union: set = set()
    for h, p in got.items():
        v = tv.read(p)
        per_run[want[h]] = v
        union |= set(v)

    if doc.get("floor", {}).get("vocabulary"):
        print(f"REFUSED: {floor_path} already declares a vocabulary. This "
              f"script writes a MISSING key and never overwrites a present "
              f"one — a changed vocabulary is a re-measurement and belongs to "
              f"ci/corpus-floor.sh.", file=sys.stderr)
        return 1

    lines = tv.fmt(union)
    body = open(floor_path).read()
    anchor = "\nrecorders = "
    if anchor not in body:
        print(f"REFUSED: no `recorders = ` line in {floor_path} to write beside.",
              file=sys.stderr)
        return 1
    ins = "\nvocabulary = [\n" + "".join(f'    "{x}",\n' for x in lines) + "]"
    i = body.index(anchor)
    body = body[:i] + ins + body[i:]
    with open(floor_path, "w") as f:
        f.write(body)

    sizes = {k: len(v) for k, v in sorted(per_run.items())}
    print(f"stamped {len(lines)} (kind, field) pair(s) into {floor_path}")
    print(f"  ensemble {doc['floor']['ensemble']}, per run: {sizes}")
    print(f"  every measured figure in the file is untouched — a vocabulary is "
          f"read off the recording JSON and no comparator is involved.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
