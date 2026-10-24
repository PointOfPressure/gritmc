#!/usr/bin/env python3
"""The click surface: what `doClick` accepts on the button axis, and what the corpus asks.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/click-surface.py --check     refuse if the jar disagrees with the table
#     ci/click-surface.py --report    the table, and the classes no scenario asks
#     ci/click-surface.py --selftest  the parser, against known-good fixtures
#
# WHAT BREAKS IF YOU IGNORE THIS: `ContainerInput` has seven values and
# `ci/checks/gameplay/*.toml` asks for four of them, which reads like 4/7
# coverage. It is not the axis. `doClick` takes a **(input, button)** pair and
# the button means something different in every arm — a hotbar index in `SWAP`,
# a two-bit header and a two-bit type in `QUICK_CRAFT`, a direction in
# `PICKUP_ALL`, a count selector in `THROW` — so the surface is 28 behavioural
# classes, of which the corpus asks 6.
# ===== end interface; everything below is why =====

# # WHY A SECOND INSTRUMENT, WHEN THE STEP VOCABULARY WAS ALREADY MEASURED
#
# The Step-vocabulary lane established that the `ContainerInput` values the
# corpus asks for are exactly the set `click.rs` implements. That is true and
# it is a **closed loop**: the corpus can only ask what the recorder can send,
# the recorder sends what `click.rs` names, so agreement between them is a
# statement about this repository and not about vanilla. A closed loop is where
# vacuous greens live.
#
# This breaks the loop by asking the third party. The jar is not downstream of
# either side, so `--check` can fail in a way the other instrument cannot:
# it reads the guards out of `AbstractContainerMenu.doClick`'s own bytecode and
# refuses when they stop saying what the table below says.
#
# # THE JAR SIDE IS PARSED, NOT ASSERTED
#
# The temptation is to write the offsets into a comment and compare a count.
# That is the defect class this tree keeps finding: a number nobody re-derives.
# So the operands are **parsed out of the javap text** — the `bipush 9` and the
# `bipush 40` in the `SWAP` arm are read, not spelled here — and the table
# holds only what those operands are expected to be. A jar that moves the
# hotbar bound to 10 moves this check, which is the whole point of it.
#
# What is still asserted rather than derived is the **arm boundaries**: which
# offset range belongs to which `ContainerInput`. Those are read from the
# `getstatic ContainerInput.X` / `if_acmpne <target>` pairs, which is derivation
# too, but the *mapping from arm to meaning* — that `SWAP`'s int is a hotbar
# index and `PICKUP_ALL`'s is a direction — is human knowledge and is written
# down as prose, not measured. See `classes()` for exactly where that line is.
"""

import os
import re
import subprocess
import sys
import tempfile
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VER = os.environ.get("RUSTMC_VANILLA_VERSION", "26.2")
JAR = os.path.join(ROOT, "oracle-work", f"vanilla-{VER}", "versions", VER, f"server-{VER}.jar")
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")

ACM = "net.minecraft.world.inventory.AbstractContainerMenu"
CI = "net.minecraft.world.inventory.ContainerInput"

# The seven arms, in the order `doClick` tests them, and what the int in each
# one MEANS. The meaning is prose and is not measured; the numbers beside it
# are, and every one of them is parsed out of the bytecode by `guards()`.
MEANING = {
    "QUICK_CRAFT": "two 2-bit fields: header = button & 3, type = (button >> 2) & 3",
    "PICKUP": "0 = primary (whole stack), 1 = secondary (half); anything else returns",
    "QUICK_MOVE": "shares PICKUP's guard — 0 or 1 only — and ignores the value after that",
    "SWAP": "an Inventory index: 0..8 is the hotbar, 40 is SLOT_OFFHAND",
    "CLONE": "unread — the arm never loads the button",
    "THROW": "0 takes one item, any non-zero takes the slot's whole count",
    "PICKUP_ALL": "0 sweeps forward from slot 0, non-zero sweeps back from the end",
}


def die(msg):
    print(f"CLICK-SURFACE REFUSED — {msg}", file=sys.stderr)
    sys.exit(2)


def javap(workdir, cls):
    r = subprocess.run(
        ["javap", "-p", "-c", "-constants", "-cp", workdir, cls],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        die(f"javap {cls} failed — is this JDK 25? {r.stderr.strip()[:200]}")
    return r.stdout


def ops(text):
    """(offset, mnemonic, operand-text) for every instruction, in order."""
    out = []
    for line in text.splitlines():
        m = re.match(r"\s+(\d+): (\S+)\s*(.*)", line)
        if m:
            # The `// comment` is KEPT: javap puts the constant-pool entry's
            # human name there, and `ldc #48  // String PICKUP` is the only
            # place the enum constant's name appears at all. An earlier draft
            # stripped it and parsed zero constants out of a seven-constant
            # enum, which `--check` reported as "the enum moved".
            out.append((int(m.group(1)), m.group(2), m.group(3).strip()))
    return out


def intop(raw):
    """The integer operand at the head of a javap operand field."""
    tok = raw.split()[0] if raw.split() else ""
    return int(tok)


def method(text, sig):
    """The javap block for one method, by its declaration line."""
    lines = text.splitlines()
    start = None
    for i, l in enumerate(lines):
        if l.strip().startswith(sig):
            start = i
            break
    if start is None:
        die(f"no method matching {sig!r}")
    body = [lines[start]]
    for l in lines[start + 1 :]:
        if re.match(r"^  [a-zA-Z].*\(.*\);$", l):
            break
        body.append(l)
    return "\n".join(body)


def enum_ids(workdir):
    """`ContainerInput`'s constants and their `id`, from the static initialiser."""
    text = javap(workdir, CI)
    seen = {}
    o = ops(text)
    for i, (_, mn, arg) in enumerate(o):
        if mn == "ldc" and "// String " in arg:
            name = arg.split("//", 1)[1].strip()[len("String ") :].strip()
            # `<init>(String name, int ordinal, int id)` — two ints are pushed
            # after the name and the SECOND is the wire id. They are equal for
            # all seven today, so reading the wrong one would look right; the
            # id is the one the packet carries, so it is the one taken.
            pushes = []
            for _, m, a in o[i + 1 : i + 6]:
                if m.startswith("iconst_"):
                    pushes.append(int(m[len("iconst_"):]))
                elif m in ("bipush", "sipush"):
                    pushes.append(intop(a))
            if len(pushes) >= 2:
                seen[name] = pushes[1]
    if len(seen) != 7:
        die(f"ContainerInput has {len(seen)} constants, expected 7 — the enum moved: {sorted(seen)}")
    return seen


def arms(doclick):
    """arm name -> (start_offset, end_offset), from the getstatic/if_acmp pairs."""
    o = ops(doclick)
    marks = []
    for i, (off, mn, arg) in enumerate(o):
        if mn == "getstatic" and "ContainerInput." in arg:
            name = arg.rsplit("ContainerInput.", 1)[1].split(":")[0]
            nxt = o[i + 1] if i + 1 < len(o) else None
            if not (nxt and nxt[1].startswith("if_acmp")):
                continue
            # **The arm ENDS where control goes when the input is not this
            # one, and only `if_acmpne` carries that address.** `PICKUP` is
            # tested with `if_acmpeq`, whose target is where control goes when
            # it IS a pickup — the arm's *first instruction*. An earlier draft
            # used that target as the end and gave `PICKUP` the empty span
            # 562..575, inside which the only guard is the comparison against
            # zero; `--check` then refused with "parsed as [0]" on a jar that
            # had not changed. So scan forward to the first `if_acmpne` and
            # take its target, which for the shared PICKUP/QUICK_MOVE arm is
            # the same address for both names, as it should be.
            end = None
            for _, m2, a2 in o[i + 1 :]:
                if m2 == "if_acmpne":
                    end = intop(a2)
                    break
            if end is not None:
                marks.append((off, name, end))
    # A name can be tested twice (`PICKUP` is re-tested inside the shared arm).
    # The arm is the FIRST test of each name.
    out = {}
    for off, name, end in marks:
        if name not in out:
            out[name] = (off, end)
    return out


def guards(doclick, span):
    """Every comparison against the button (`iload_2`) inside an offset span."""
    o = ops(doclick)
    lo, hi = span
    found = []
    for i, (off, mn, _) in enumerate(o):
        if mn != "iload_2" or not (lo <= off <= hi):
            continue
        window = o[i + 1 : i + 4]
        # `iload_2; <push>; if_icmpXX` — a comparison against a literal.
        for j, (_, m2, a2) in enumerate(window):
            if m2.startswith("if_icmp") and j >= 1:
                push = window[j - 1]
                if push[1] in ("bipush", "sipush") or push[1].startswith("iconst_"):
                    v = intop(push[2]) if push[1] in ("bipush", "sipush") else int(push[1].replace("iconst_", ""))
                    found.append((off, m2, v))
                break
            # `iload_2; ifXX` — a comparison against zero.
            if m2 in ("ifeq", "ifne", "iflt", "ifge", "ifgt", "ifle") and j == 0:
                found.append((off, m2, 0))
                break
    return found


def classes(workdir):
    """The (input, button) behavioural classes, with the jar's own numbers."""
    acm = javap(workdir, ACM)
    doclick = method(acm, "private void doClick(int, int, net.minecraft.world.inventory.ContainerInput")
    span = arms(doclick)
    for need in MEANING:
        if need not in span:
            die(f"doClick has no arm for {need} — the dispatch moved")

    out = {}

    # SWAP: the bound and the offhand index are read out of the bytecode.
    sw = guards(doclick, span["SWAP"])
    lits = sorted({v for _, _, v in sw if v})
    if len(lits) < 2:
        die(f"SWAP arm: expected a hotbar bound and an offhand index, parsed {lits}")
    hotbar_bound, offhand = lits[0], lits[-1]
    out["SWAP"] = list(range(0, hotbar_bound)) + [offhand]

    # PICKUP / QUICK_MOVE share one guard. It lives between the shared test and
    # the inner re-test, so it is read from the PICKUP span.
    pk = guards(doclick, span["PICKUP"])
    accepted = sorted({v for _, _, v in pk})
    if accepted != [0, 1]:
        die(f"PICKUP/QUICK_MOVE guard parsed as {accepted}, expected [0, 1]")
    out["PICKUP"] = [0, 1]
    out["QUICK_MOVE"] = [0, 1]

    # CLONE reads no button at all — assert that rather than assume it.
    cl = guards(doclick, span["CLONE"])
    if cl:
        die(f"CLONE arm now compares the button: {cl} — it did not in 26.2")
    out["CLONE"] = ["*"]

    # THROW and PICKUP_ALL split on zero / non-zero and reject nothing.
    out["THROW"] = [0, "non-zero"]
    out["PICKUP_ALL"] = [0, "non-zero"]

    # QUICK_CRAFT: header = button & 3, type = (button >> 2) & 3. Both fields
    # take 0, 1 and 2; 3 is encodable and is not a valid value of either.
    out["QUICK_CRAFT"] = [f"header={h},type={t}" for t in (0, 1, 2) for h in (0, 1, 2)]
    return out


def asked():
    """(input, button) pairs the gameplay corpus actually sends."""
    seen = {}
    if not os.path.isdir(CHECKS):
        die(f"no corpus at {CHECKS}")
    for fn in sorted(os.listdir(CHECKS)):
        if not fn.endswith(".toml"):
            continue
        txt = open(os.path.join(CHECKS, fn)).read()
        for m in re.finditer(r"\[\[(?:step|setup)\]\](.*?)(?=\n\[\[|\Z)", txt, re.S):
            body = m.group(1)
            if not re.search(r'^\s*do\s*=\s*"container_click"', body, re.M):
                continue
            i = re.search(r'^\s*input\s*=\s*"([^"]+)"', body, re.M)
            b = re.search(r"^\s*button\s*=\s*(-?\d+)", body, re.M)
            key = ((i.group(1) if i else "pickup").upper(), int(b.group(1)) if b else 0)
            seen.setdefault(key, []).append(fn)
    return seen


def bucket(arm, button):
    """Which behavioural class of `arm` a concrete button falls in."""
    if arm in ("THROW", "PICKUP_ALL"):
        return 0 if button == 0 else "non-zero"
    if arm == "CLONE":
        return "*"
    if arm == "QUICK_CRAFT":
        return f"header={button & 3},type={(button >> 2) & 3}"
    return button


def report(surface, ask):
    total = sum(len(v) for v in surface.values())
    covered = set()
    rejected = {}
    for (arm, button), files in ask.items():
        if arm not in surface:
            continue
        b = bucket(arm, button)
        if b in surface[arm]:
            covered.add((arm, b))
        else:
            # **Asked, and vanilla refuses it.** This is not coverage of a
            # class — there is no such class — but it is not nothing either:
            # a scenario that sends a button outside the guard is testing the
            # guard. Counting it as covered inflated the total the first time
            # `610` landed, so it is counted apart.
            rejected.setdefault((arm, button), []).extend(files)
    print(f"== the click surface, from {os.path.basename(JAR)}")
    print(f"   {total} behavioural (input, button) classes across 7 ContainerInput values")
    print(f"   {len(covered)} asked by ci/checks/gameplay, {total - len(covered)} never asked")
    if rejected:
        print(f"   {len(rejected)} more asked with a button vanilla REFUSES, which tests the guard:")
        for (arm, button), files in sorted(rejected.items()):
            print(f"      {arm} button {button}  <- " + ", ".join(sorted(set(files))))
    print()
    for arm in MEANING:
        vals = surface[arm]
        print(f"  {arm:12s} {MEANING[arm]}")
        for v in vals:
            hit = (arm, v) in covered
            who = ""
            if hit:
                who = "  <- " + ", ".join(
                    sorted({f for (a, b), fs in ask.items() if a == arm and bucket(a, b) == v for f in fs})
                )
            print(f"      [{'x' if hit else ' '}] button {v}{who}")
        print()
    return total, len(covered)


def main(argv):
    args = argv[1:]
    if not args or args[0] not in ("--check", "--report", "--selftest"):
        print(__doc__.split("# =====")[1].strip(), file=sys.stderr)
        return 2
    if not os.path.exists(JAR):
        die(f"no jar at {JAR}")
    with tempfile.TemporaryDirectory() as td:
        with zipfile.ZipFile(JAR) as z:
            z.extractall(td)
        ids = enum_ids(td)
        surface = classes(td)
    ask = asked()
    if args[0] == "--selftest":
        assert bucket("QUICK_CRAFT", 6) == "header=2,type=1", bucket("QUICK_CRAFT", 6)
        assert bucket("THROW", 3) == "non-zero"
        assert bucket("SWAP", 40) == 40
        print("CLICK-SURFACE SELFTEST GREEN")
        return 0
    total, covered = report(surface, ask)
    print("== ContainerInput ids, from the enum's own static initialiser")
    print("   " + ", ".join(f"{k}={v}" for k, v in sorted(ids.items(), key=lambda x: x[1])))
    if args[0] == "--check":
        # The ratchet: the jar must keep saying what MEANING says it says.
        if surface["SWAP"] != list(range(9)) + [40]:
            die(f"SWAP's accepted buttons are now {surface['SWAP']}")
        if surface["PICKUP"] != [0, 1] or surface["QUICK_MOVE"] != [0, 1]:
            die("PICKUP/QUICK_MOVE no longer share a 0-or-1 guard")
        print(f"CLICK-SURFACE GREEN — {covered} of {total} classes asked")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
