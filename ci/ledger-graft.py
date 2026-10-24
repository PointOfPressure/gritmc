#!/usr/bin/env python3
"""Copy named [[scenario]] entries from one gameplay-falsifiable ledger to another.

WHY THIS EXISTS
===============
A recording covers the scenarios present in the tree it ran against, and
`ci/corpus-falsifiable.py --sweep` writes the whole ledger for that tree. So one
recording on an integration tip carrying N branches' new scenarios produces
evidence for **all N at once** — the expensive resource is the ~40-minute corpus
lock acquisition, and `ci/sweep-tip.sh` buys one instead of N.

If the batch then lands, the ledger lands with it and there is nothing to do.
**If the batch goes RED, that evidence is sitting on a throwaway tip** and each
member is about to be re-gated on its own, needing its own ledger row. This
grafts the rows back onto the member branches so the recording is not wasted by a
verdict that had nothing to do with it.

WHAT IT REFUSES, AND WHY EACH REFUSAL IS LOAD-BEARING
=====================================================
The ledger is evidence, and its whole value is that an entry ties a scenario's
red-when-broken proof to the version of the scenario it was taken against — the
file says "do not hand-edit an entry: a hand-written digest is evidence of
nothing". So this tool moves entries VERBATIM and refuses everything else:

  * a requested scenario file with no entry in the source -> refuse. Silently
    grafting nothing would leave the member UNREGISTERED and look like a success.
  * an entry already present in the target and DIFFERENT -> refuse unless
    `--replace`. Two different pieces of evidence for one scenario means one of
    them was taken against a different version of it, and picking one by
    position is not a decision this script gets to make.
  * a source entry that does not parse, or a target that does not -> refuse.

It never synthesises, edits or reformats an entry. Blocks are copied as text,
which is also why it does not round-trip the TOML: `tomllib` is read-only and
re-emitting the file with a writer would reflow every comment in a file whose
header is half the evidence.

Entries are appended at the end of the target. Order carries no meaning in TOML
and the next real `--sweep` rewrites the file wholesale.

**THE COST OF USING THIS, STATED PLAINLY.**
`ci/checks/gameplay-falsifiable.toml` carries **no merge driver** (`.gitattributes`
names `merge=docregions` for `docs/WORKLIST.md`, `docs/PARITY-ROADMAP.md` and
`ci/figures.toml`, and `merge=appendonly` for `docs/DETERMINISM.md` — not this
file). So grafting onto N branches makes N branches append to one
un-driver-managed file, and they will conflict with each other when they rebase
onto main one after another. That is why `ci/batch.sh --sweep-back` is opt-in and
not the default: the default keeps the rows on the integration tip, where they
land once.

It is also a concrete argument that this ledger is a good candidate for
`merge=docregions`. Its entries are keyed rows — `file = "010-break-block.toml"`
is unique per entry — which is exactly the shape that driver already resolves for
WORKLIST's cold-start index.

USAGE
    ci/ledger-graft.py --from <src.toml> --to <dst.toml> <scenario-file>...
    ci/ledger-graft.py --from a --to b --list
    ci/ledger-graft.py --selftest
"""

import argparse
import re
import sys
import tempfile
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None

BLOCK_START = re.compile(r"^\[\[scenario\]\]\s*$", re.M)
FILE_FIELD = re.compile(r'^file\s*=\s*"([^"]+)"\s*$', re.M)


def split_blocks(text):
    """-> (preamble, {scenario-file: block-text}, order)

    A block runs from its `[[scenario]]` line to the line before the next
    top-level `[[...]]` table or to end of file. Nothing else in the file is
    touched or understood.
    """
    starts = [m.start() for m in BLOCK_START.finditer(text)]
    if not starts:
        return text, {}, []
    # Where does each block end? At the next line that opens ANY [[table]].
    any_table = [m.start() for m in re.finditer(r"^\[\[[a-z_]+\]\]\s*$", text, re.M)]
    blocks, order = {}, []
    for s in starts:
        later = [t for t in any_table if t > s]
        e = later[0] if later else len(text)
        block = text[s:e]
        m = FILE_FIELD.search(block)
        if not m:
            continue
        key = m.group(1)
        blocks[key] = block
        order.append(key)
    return text[: starts[0]], blocks, order


def graft(src_text, dst_text, wanted, replace=False):
    """-> (new_dst_text, grafted, errors)"""
    errors, grafted = [], []
    _, src_blocks, _ = split_blocks(src_text)
    _, dst_blocks, _ = split_blocks(dst_text)
    for name in wanted:
        if name not in src_blocks:
            errors.append(f"no [[scenario]] entry for {name} in the source ledger")
            continue
        if name in dst_blocks:
            if dst_blocks[name].strip() == src_blocks[name].strip():
                grafted.append(f"{name} (already identical)")
                continue
            if not replace:
                errors.append(
                    f"{name} already has a DIFFERENT entry in the target — two pieces of "
                    f"evidence for one scenario; --replace to overwrite")
                continue
    if errors:
        return dst_text, grafted, errors

    out = dst_text
    for name in wanted:
        if name in dst_blocks:
            if dst_blocks[name].strip() == src_blocks[name].strip():
                continue
            out = out.replace(dst_blocks[name], src_blocks[name], 1)
            grafted.append(f"{name} (replaced)")
        else:
            if not out.endswith("\n"):
                out += "\n"
            out += src_blocks[name].rstrip("\n") + "\n"
            grafted.append(name)
    return out, grafted, errors


def parses(text):
    if tomllib is None:
        return True, ""
    try:
        tomllib.loads(text)
        return True, ""
    except Exception as e:  # noqa: BLE001
        return False, str(e)


def selftest():
    fails = []

    def check(name, cond, detail=""):
        if cond:
            print(f"  PASS  {name}")
        else:
            fails.append(name)
            print(f"  FAIL  {name}{': ' + detail if detail else ''}")

    hdr = "# a ledger\n[meta]\nbattery = [\"edit\"]\n\n"
    a = (hdr
         + '[[dead_field]]\nkind = "k"\nfield = "f"\n\n'
         + '[[scenario]]\nname = "one"\nfile = "001-one.toml"\ntier = 2\n\n'
         + '[[scenario]]\nname = "two"\nfile = "002-two.toml"\ntier = 2\n')
    b = hdr + '[[scenario]]\nname = "one"\nfile = "001-one.toml"\ntier = 2\n'

    _, blocks, order = split_blocks(a)
    check("split finds both scenarios", sorted(blocks) == ["001-one.toml", "002-two.toml"],
          str(sorted(blocks)))
    check("split does not swallow the dead_field into a scenario block",
          "dead_field" not in blocks.get("001-one.toml", ""))
    check("preamble keeps the header", split_blocks(a)[0].startswith("# a ledger"))

    out, grafted, errs = graft(a, b, ["002-two.toml"])
    check("grafts a missing entry", not errs and grafted == ["002-two.toml"], f"{errs} {grafted}")
    check("grafted target has both", len(split_blocks(out)[1]) == 2)
    ok, why = parses(out)
    check("grafted target still parses as TOML", ok, why)
    check("grafted entry is verbatim",
          split_blocks(out)[1]["002-two.toml"].strip() == blocks["002-two.toml"].strip())

    out2, grafted2, errs2 = graft(a, b, ["001-one.toml"])
    check("an identical entry is a no-op, not a duplicate",
          not errs2 and len(split_blocks(out2)[1]) == 1, f"{errs2}")

    # THE REFUSALS.
    _, _, errs3 = graft(a, b, ["999-nope.toml"])
    check("refuses a scenario the source does not have", bool(errs3), str(errs3))

    b_diff = hdr + '[[scenario]]\nname = "one"\nfile = "001-one.toml"\ntier = 1\n'
    _, _, errs4 = graft(a, b_diff, ["001-one.toml"])
    check("refuses a DIFFERENT existing entry without --replace", bool(errs4), str(errs4))

    out5, grafted5, errs5 = graft(a, b_diff, ["001-one.toml"], replace=True)
    check("--replace overwrites it", not errs5 and "tier = 2" in out5, f"{errs5}")

    # A refusal must leave the target untouched, not half-written: the loop
    # validates every request before writing anything.
    out6, _, errs6 = graft(a, b, ["002-two.toml", "999-nope.toml"])
    check("one bad request refuses the WHOLE graft and writes nothing",
          bool(errs6) and out6 == b, f"{errs6}")

    # And an entry with no `file` field is not indexable, so it is skipped
    # rather than mis-keyed.
    weird = hdr + '[[scenario]]\nname = "nofile"\ntier = 2\n'
    check("an entry with no file field is not indexed", split_blocks(weird)[1] == {})

    with tempfile.TemporaryDirectory() as d:
        p1, p2 = Path(d) / "a.toml", Path(d) / "b.toml"
        p1.write_text(a)
        p2.write_text(b)
        rc = run(p1, p2, ["002-two.toml"], False, False)
        check("end to end via files returns 0", rc == 0, str(rc))
        check("the file on disk gained the entry", len(split_blocks(p2.read_text())[1]) == 2)

    print(f"\nledger-graft selftest: {'GREEN' if not fails else 'RED'} ({len(fails)} failed)")
    return 1 if fails else 0


def run(src, dst, wanted, replace, list_only):
    src_text = Path(src).read_text()
    dst_text = Path(dst).read_text()
    for label, text in (("source", src_text), ("target", dst_text)):
        ok, why = parses(text)
        if not ok:
            print(f"ledger-graft: the {label} ledger does not parse: {why}", file=sys.stderr)
            return 2
    if list_only:
        _, blocks, order = split_blocks(src_text)
        for k in order:
            print(f"  {k}")
        print(f"ledger-graft: {len(order)} scenario entries in {src}")
        return 0
    out, grafted, errors = graft(src_text, dst_text, wanted, replace)
    for e in errors:
        print(f"ledger-graft: REFUSED  {e}", file=sys.stderr)
    if errors:
        print(f"ledger-graft: nothing written to {dst}", file=sys.stderr)
        return 1
    ok, why = parses(out)
    if not ok:
        print(f"ledger-graft: the grafted result does not parse: {why}", file=sys.stderr)
        return 1
    Path(dst).write_text(out)
    for g in grafted:
        print(f"ledger-graft: grafted {g}")
    print(f"ledger-graft: {len(grafted)} entr{'y' if len(grafted) == 1 else 'ies'} -> {dst}")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--from", dest="src")
    ap.add_argument("--to", dest="dst")
    ap.add_argument("--replace", action="store_true")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("scenarios", nargs="*")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    if not a.src or not a.dst:
        print("ledger-graft: --from and --to are required", file=sys.stderr)
        return 2
    if not a.list and not a.scenarios:
        print("ledger-graft: name at least one scenario file, or use --list", file=sys.stderr)
        return 2
    return run(a.src, a.dst, a.scenarios, a.replace, a.list)


if __name__ == "__main__":
    sys.exit(main())
