#!/usr/bin/env python3
"""Positive control for `ci/click-surface.py`: doctor the bytecode, demand a refusal.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/click-surface-falsify.py        exits 0 when every case refused as it should
#
# WHAT BREAKS IF YOU IGNORE THIS: `click-surface.py --check` prints GREEN, and
# a checker that has never been shown printing RED is not evidence. Every case
# here is a jar that says something different from what the table claims, and
# the check must refuse each one. A case that stops refusing is a hole in the
# checker, not in this file.
# ===== end interface; everything below is why =====

# # WHY THE FIXTURES ARE DOCTORED REAL OUTPUT, NOT HAND-WRITTEN
#
# The parser reads javap's exact text layout. A hand-written fixture drifts
# from that layout and then this file tests a parser that does not exist —
# which is the shape `ci/unfalsifiable-falsify.py` found twice in its own
# cases. So every fixture here starts as the real `javap` output for the real
# pinned jar and has ONE substitution applied to it. If the real output stops
# parsing, every case here fails loudly rather than passing vacuously.
#
# # THE CONTROL THAT MATTERS MOST IS THE LAST ONE
#
# `unchanged_output_is_green` is not padding. Four cases that all refuse would
# also be satisfied by a `--check` that refuses everything, and a checker that
# always says no is exactly as useless as one that always says yes. The clean
# fixture pins the other end.
"""

import os
import subprocess
import sys
import tempfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import importlib.util

spec = importlib.util.spec_from_file_location("click_surface", os.path.join(HERE, "click-surface.py"))
cs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cs)

FAILED = []
PASSED = 0


def case(name, doctor, expect_refusal=True):
    """Run `classes()`/`enum_ids()` over a doctored jar extract and judge it."""
    global PASSED
    with tempfile.TemporaryDirectory() as td:
        with zipfile.ZipFile(cs.JAR) as z:
            z.extractall(td)
        real_javap = cs.javap
        real_die = cs.die

        class Refused(Exception):
            pass

        def fake_die(msg):
            raise Refused(msg)

        def fake_javap(workdir, clsname):
            return doctor(clsname, real_javap(workdir, clsname))

        cs.javap = fake_javap
        cs.die = fake_die
        try:
            ids = cs.enum_ids(td)
            surface = cs.classes(td)
            # `classes()` returning is not yet a pass: `--check`'s own
            # assertions are the rest of the gate, so run them too.
            if surface["SWAP"] != list(range(9)) + [40]:
                raise Refused(f"SWAP is {surface['SWAP']}")
            if surface["PICKUP"] != [0, 1] or surface["QUICK_MOVE"] != [0, 1]:
                raise Refused("PICKUP/QUICK_MOVE guard moved")
            if len(ids) != 7:
                raise Refused("enum arity")
            refused = None
        except Refused as e:
            refused = str(e)
        finally:
            cs.javap = real_javap
            cs.die = real_die

    if expect_refusal and refused is None:
        FAILED.append(f"{name}: the check PASSED a doctored jar")
    elif not expect_refusal and refused is not None:
        FAILED.append(f"{name}: the check REFUSED the real jar — {refused}")
    else:
        PASSED += 1
        what = f"refused: {refused[:70]}" if refused else "green"
        print(f"  ok  {name:44s} {what}")


def sub(cls_wanted, old, new):
    def doctor(clsname, text):
        if clsname != cls_wanted:
            return text
        if old not in text:
            raise AssertionError(f"fixture drift: {old!r} not in real javap of {clsname}")
        return text.replace(old, new, 1)

    return doctor


def main():
    ACM, CI = cs.ACM, cs.CI

    # 1. The hotbar bound moves. `SWAP` accepts 0..8 today; a jar with ten
    #    hotbar slots must not read as the same surface.
    case("swap hotbar bound 9 -> 10", sub(ACM, "1091: bipush        9", "1091: bipush        10"))

    # 2. The offhand index moves off 40. `Inventory.SLOT_OFFHAND` is 40 and the
    #    SWAP arm compares against it literally.
    case("swap offhand 40 -> 41", sub(ACM, "1097: bipush        40", "1097: bipush        41"))

    # 3. The shared PICKUP/QUICK_MOVE guard loses its `button == 1` leg, so the
    #    arm would accept only 0. This is the exact shape of the divergence
    #    this instrument was written to make visible, pointed the other way.
    case(
        "pickup guard drops its second leg",
        sub(ACM, "580: iconst_1\n       581: if_icmpne     1079", "580: iconst_0\n       581: if_icmpne     1079"),
    )

    # 4. `CLONE` starts reading the button. The table says the arm never loads
    #    it, and that claim has to be falsifiable.
    case(
        "clone starts comparing the button",
        sub(ACM, "1392: aload         4", "1392: iload_2\n      1393: ifeq          1449\n      1394: aload         4"),
    )

    # 5. The enum loses a constant. **Renaming one is not this test**: the
    #    first draft substituted `String PICKUP_ALL` -> `String REMOVED_ALL`
    #    and the check passed, correctly — seven constants are still seven
    #    whatever they are called. The arity only moves if the parser stops
    #    seeing the constant at all, so the `// String` marker itself goes.
    case("ContainerInput loses a constant", sub(CI, "// String PICKUP_ALL", "// Field PICKUP_ALL"))

    # 6. THE OTHER END. The real jar, untouched, must come out green.
    case("unchanged output is green", lambda c, t: t, expect_refusal=False)

    print()
    if FAILED:
        for f in FAILED:
            print(f"CLICK-SURFACE-FALSIFY FAILED — {f}", file=sys.stderr)
        return 1
    print(f"CLICK-SURFACE-FALSIFY GREEN — {PASSED} cases, 5 doctored jars refused, 1 clean jar accepted")
    return 0


if __name__ == "__main__":
    sys.exit(main())
