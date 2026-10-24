#!/usr/bin/env python3
"""Refuse a `pitch.rs` draw range that the pinned jar does not agree with.

# Why this exists

`tools/oracle-runner/src/pitch.rs` declares, for each sound event vanilla
*draws* a pitch for, the half-open range it is drawn from. Those ranges decide
whether a recorded pitch reads as "the draw" or survives as a number that can
diverge, so a wrong range is not a cosmetic error: too wide and a broken server
matches, too narrow and an honest one diverges every run.

The ranges were read out of the pinned jar by hand with
`ci/sound-pitch-callsites.sh`. **A constant read by hand once and never
re-read is exactly the kind of claim this project keeps finding stale**, and it
would go stale silently the first time the pinned version moved. So this script
re-derives each range *from the bytecode* and compares it to the declaration.

It is a cross-check and not a restatement: the declaration says `[0.9, 1.0)`,
the jar says `ldc 0.1f; fmul; ldc 0.9f; fadd` after a `nextFloat()`, and the
script computes `[0.9, 0.9 + 0.1)` from the second and asserts it equals the
first. Neither side is copied from the other.

# Red-when-broken

`--falsify` reconstructs the failure this guard exists to catch: it perturbs
each declared range in turn and asserts the check refuses, then asserts the
unperturbed table passes. A guard that cannot be shown refusing a wrong table
is itself a check that cannot fail (`docs/AGENT-BRIEF.md`, and
`ci/unfalsifiable-falsify.py` for the pattern).

Needs no server, no corpus lock and no recording. Seconds.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PITCH_RS = ROOT / "tools/oracle-runner/src/pitch.rs"
JAR = ROOT / "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"

# const name -> (class, method, shape, the SoundEvent the arm must mention)
#
# `shape` is how the draw combines with the literals:
#   "mul_add"       nextFloat() * A + B                  -> [B, B + A)
#   "diff_mul_add"  (nextFloat() - nextFloat()) * A + B   -> (B - A, B + A)
#
# The SoundEvent name is required so a method holding several draws cannot have
# one arm's range silently validated against another arm's literals -- which is
# a live hazard here: PistonBaseBlock draws PISTON_EXTEND and PISTON_CONTRACT
# from the same method with different multipliers.
SITES = {
    "NINE_TENTHS": (
        "net.minecraft.world.level.block.entity.ChestBlockEntity",
        "playSound", "mul_add", None,
    ),
    "FLINT": (
        "net.minecraft.world.item.FlintAndSteelItem",
        "useOn", "mul_add", None,
    ),
    "VOICE": (
        "net.minecraft.world.entity.LivingEntity",
        "getVoicePitch", "diff_mul_add", None,
    ),
    "PISTON_EXTEND": (
        "net.minecraft.world.level.block.piston.PistonBaseBlock",
        None, "mul_add", "PISTON_EXTEND",
    ),
    "PISTON_CONTRACT": (
        "net.minecraft.world.level.block.piston.PistonBaseBlock",
        None, "mul_add", "PISTON_CONTRACT",
    ),
}

FLOAT_PUSH = re.compile(
    r"^\s*(\d+):\s+(ldc|ldc_w|fconst_0|fconst_1|fconst_2)\b.*?(?://.*?float\s+([-\d.E]+)f?)?\s*$"
)
OP = re.compile(r"^\s*(\d+):\s+(\w+)")


def declared() -> dict[str, tuple[float, float]]:
    """The (lo, hi) each `Class::Drawn` const declares in pitch.rs."""
    src = PITCH_RS.read_text(encoding="utf-8")
    out = {}
    for m in re.finditer(
        r"const\s+(\w+)\s*:\s*Class\s*=\s*Class::Drawn\s*\{(.*?)\}\s*;",
        src, re.S,
    ):
        name, body = m.group(1), m.group(2)
        lo = re.search(r"lo:\s*([-\d.]+)", body)
        hi = re.search(r"hi:\s*([-\d.]+)", body)
        if lo and hi:
            out[name] = (float(lo.group(1)), float(hi.group(1)))
    return out


def bytecode(cls: str) -> list[str]:
    if not JAR.exists():
        sys.exit(f"REFUSED  no pinned jar at {JAR}")
    r = subprocess.run(
        ["javap", "-p", "-c", "-constants", "-classpath", str(JAR), cls],
        capture_output=True, text=True, check=False,
    )
    if r.returncode != 0:
        sys.exit(f"REFUSED  javap failed for {cls}: {r.stderr.strip()[:200]}")
    return r.stdout.splitlines()


def draws(lines: list[str], method: str | None, event: str | None) -> list[tuple[float, float]]:
    """Every `nextFloat()`-fed `* A + B` in the region, as (A, B) pairs.

    `method` narrows to one method; `event` narrows further to the window
    between a `SoundEvents.<event>` push and the `playSound` that consumes it,
    which is what keeps two arms of one method apart.
    """
    if method:
        region, inm = [], False
        for ln in lines:
            if re.match(r"^  [^ ].*\(", ln):
                inm = f" {method}(" in ln
            if inm:
                region.append(ln)
        lines = region

    if event:
        region, live = [], False
        for ln in lines:
            if f"SoundEvents.{event}:" in ln:
                live = True
            if live:
                region.append(ln)
            if live and "playSound" in ln:
                live = False
        lines = region

    pairs: list[tuple[float, float]] = []
    last_float: float | None = None
    seen_next_float = False
    mul: float | None = None
    for ln in lines:
        if "RandomSource.nextFloat" in ln:
            seen_next_float = True
            continue
        m = OP.match(ln)
        if not m:
            continue
        op = m.group(2)
        if op in ("ldc", "ldc_w"):
            f = re.search(r"//\s*float\s+([-\d.E]+)f", ln)
            last_float = float(f.group(1)) if f else None
        elif op == "fconst_0":
            last_float = 0.0
        elif op == "fconst_1":
            last_float = 1.0
        elif op == "fconst_2":
            last_float = 2.0
        elif op == "fmul" and seen_next_float and last_float is not None:
            mul, last_float = last_float, None
        elif op == "fadd" and mul is not None and last_float is not None:
            pairs.append((mul, last_float))
            mul, last_float, seen_next_float = None, None, False
    return pairs


def check() -> int:
    decl = declared()
    if not decl:
        print("REFUSED  pitch.rs declares no Class::Drawn ranges at all — either "
              "the file moved or the parser stopped matching, and a guard that "
              "silently checks nothing is the defect it exists to catch")
        return 1
    bad, ok = [], 0
    for name, (lo, hi) in sorted(decl.items()):
        site = SITES.get(name)
        if site is None:
            bad.append(f"{name:16} declared [{lo}, {hi}) but no call site is registered "
                       f"in ci/check-sound-pitch-table.py — add one or the range is unverified")
            continue
        cls, method, shape, event = site
        pairs = draws(bytecode(cls), method, event)
        if not pairs:
            bad.append(f"{name:16} no nextFloat()-fed draw found in {cls}"
                       f"{'.' + method if method else ''}"
                       f"{' @' + event if event else ''}")
            continue
        cands = []
        for a, b in pairs:
            cands.append((b - a, b + a) if shape == "diff_mul_add" else (b, b + a))
        hit = any(abs(c[0] - lo) < 1e-6 and abs(c[1] - hi) < 1e-6 for c in cands)
        if hit:
            ok += 1
            print(f"ok   {name:16} [{lo}, {hi})  <- {cls.rsplit('.', 1)[1]}"
                  f"{'.' + method if method else ''} {pairs}")
        else:
            bad.append(f"{name:16} declares [{lo}, {hi}) but the jar gives "
                       f"{['[%g, %g)' % c for c in cands]} from {pairs}")
    for b in bad:
        print(f"REFUSED  {b}")
    if bad:
        print(f"\nSOUND-PITCH-TABLE REFUSED — {len(bad)} of {len(decl)} declared "
              f"ranges disagree with the pinned jar")
        return 1
    print(f"\nSOUND-PITCH-TABLE GREEN — {ok} declared draw ranges each re-derived "
          f"from the pinned jar's own literals")
    return 0


def falsify() -> int:
    """Perturb each declared range and assert the check refuses."""
    original = PITCH_RS.read_text(encoding="utf-8")
    decl = declared()
    if not decl:
        print("FALSIFY REFUSED  nothing declared to perturb")
        return 1
    caught, survived = 0, []
    try:
        for name, (lo, hi) in sorted(decl.items()):
            for label, new_lo, new_hi in (
                ("widened", lo - 0.1, hi + 0.1),
                ("narrowed", lo + 0.01, hi - 0.01),
            ):
                patched = re.sub(
                    r"(const\s+" + name + r"\s*:\s*Class\s*=\s*Class::Drawn\s*\{[^}]*?lo:\s*)"
                    r"[-\d.]+([^}]*?hi:\s*)[-\d.]+",
                    lambda m: f"{m.group(1)}{new_lo}{m.group(2)}{new_hi}",
                    original, count=1, flags=re.S,
                )
                if patched == original:
                    survived.append(f"{name} {label}: the patch did not apply — "
                                    f"a mutation that stopped matching is a hole in this sweep")
                    continue
                PITCH_RS.write_text(patched, encoding="utf-8")
                if check() == 0:
                    survived.append(f"{name} {label} to [{new_lo}, {new_hi}) — NOT caught")
                else:
                    caught += 1
                    print(f"    ^ caught {name} {label}\n")
    finally:
        PITCH_RS.write_text(original, encoding="utf-8")
    print("=" * 70)
    if check() != 0:
        print("FALSIFY REFUSED  the restored table does not pass — the original "
              "was not put back, or was wrong to begin with")
        return 1
    for s in survived:
        print(f"SURVIVED  {s}")
    if survived:
        print(f"\nSOUND-PITCH-FALSIFY REFUSED — caught {caught}, survived {len(survived)}")
        return 1
    print(f"\nSOUND-PITCH-FALSIFY GREEN — every one of {caught} perturbed ranges was "
          f"refused, and the restored table passes")
    return 0


if __name__ == "__main__":
    sys.exit(falsify() if "--falsify" in sys.argv[1:] else check())
