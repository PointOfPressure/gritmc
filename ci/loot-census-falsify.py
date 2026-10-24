#!/usr/bin/env python3
"""loot-census-falsify.py — show `ci/loot-census.py` can go red before believing it green.

This project's central defect class is the check that cannot fail. A census that
walks JSON and prints a percentage is a prime candidate: if `opcodes()` quietly
missed every nested token it would print a *smaller* vocabulary and a *cheaper*
cost curve, which is precisely the direction that flatters the argument the
census is being used to make. So the discriminating test is not "does the real
implementation pass" — it is "does a plausibly-wrong implementation FAIL".

Each case below is run twice: once against the real `loot_census`, which must
pass, and once against a deliberately-broken variant, which must fail. A case
where the broken variant also passes is reported as VACUOUS and exits non-zero,
because such a case is decoration.
"""
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


real = load(os.path.join(HERE, "loot-census.py"), "loot_census_real")


# ---------------------------------------------------------------- broken variants
def shallow_opcodes(table):
    """The wrong implementation this whole file exists to catch: depth-zero only.

    It reads pool-level entries but never recurses into `alternatives` children
    or `any_of` terms. Against real vanilla data it undercounts the vocabulary
    and therefore *understates* the cost of a loot interpreter.
    """
    seen = set()
    for pool in table.get("pools") or []:
        for e in pool.get("entries") or []:
            seen.add("entry:" + str(e.get("type", "?")).split(":")[-1])
        for c in pool.get("conditions") or []:
            seen.add("cond:" + str(c.get("condition", "?")).split(":")[-1])
    return frozenset(seen)


def lenient_runnable(ops, have):
    """The other wrong implementation: 'runnable' meaning *any* token supported.

    This is the vacuous-green shape — a table counts as ported because one of its
    conditions is understood, so coverage rises without behaviour changing.
    """
    return bool(ops & have) or not ops


# ---------------------------------------------------------------- fixtures
NESTED = {
    "pools": [{
        "rolls": 1,
        "entries": [{
            "type": "minecraft:alternatives",
            "children": [{
                "type": "minecraft:item",
                "name": "minecraft:diamond",
                "conditions": [{
                    "condition": "minecraft:any_of",
                    "terms": [{"condition": "minecraft:killed_by_player"}],
                }],
                "functions": [{
                    "function": "minecraft:set_count",
                    "count": {"type": "minecraft:uniform", "min": 1, "max": 3},
                }],
            }],
        }],
    }],
}

FLAT = {"pools": [{"rolls": 1, "entries": [{"type": "minecraft:item", "name": "minecraft:stone"}]}]}

TWO_TOKEN = {
    "pools": [{
        "rolls": 1,
        "entries": [{"type": "minecraft:item", "name": "minecraft:coal",
                     "functions": [{"function": "minecraft:furnace_smelt"}]}],
    }],
}


# ---------------------------------------------------------------- cases
def case_nested_tokens_are_seen():
    """A token three levels deep must appear in the opcode set."""
    def check(opcodes_fn):
        ops = opcodes_fn(NESTED)
        return {"cond:killed_by_player", "cond:any_of", "fn:set_count",
                "num:uniform", "entry:item"} <= ops
    return check


def case_unknown_token_is_not_runnable():
    """A table is runnable only when EVERY token it uses is implemented."""
    def check(opcodes_fn, runnable_fn=None):
        runnable_fn = runnable_fn or (lambda ops, have: ops <= have)
        ops = opcodes_fn(TWO_TOKEN)
        partial = {"entry:item"}          # furnace_smelt deliberately absent
        return not runnable_fn(ops, partial)
    return check


def case_curve_prices_every_opcode():
    """The curve must charge for EVERY token in the corpus and end at full coverage.

    Monotonicity alone was tried here first and was rejected by this file's own
    VACUOUS rule: `curve()` is monotone by construction, so a broken opcode walk
    passed it too, and a case both implementations pass measures nothing. The
    discriminating property is *completeness* — the curve has to end having paid
    for the whole vocabulary, so a walk that cannot see nested tokens produces a
    curve that terminates too early and too cheap.
    """
    def check(opcodes_fn, runnable_fn=None):
        tabs = {"a": opcodes_fn(NESTED), "b": opcodes_fn(FLAT), "c": opcodes_fn(TWO_TOKEN)}
        union = set()
        for ops in tabs.values():
            union |= ops
        cur = curve_with(tabs, runnable_fn) if runnable_fn else real.curve(tabs)
        counts = [n for _, _, n in cur]
        monotone = counts == sorted(counts)
        priced_all = len(cur) == len(union)
        full = bool(counts) and counts[-1] == len(tabs)
        # The corpus is fixed, so the true vocabulary is known independently of
        # whichever walk produced it: NESTED alone contributes five tokens.
        rich_enough = len(union) >= 8
        return monotone and priced_all and full and rich_enough
    return check


def curve_with(tabs, runnable_fn):
    """`real.curve` but with the runnable predicate swapped, to build a broken twin."""
    have, out = set(), []
    while True:
        runnable = {t for t, ops in tabs.items() if runnable_fn(ops, have)}
        cand = set()
        for t, ops in tabs.items():
            if t not in runnable:
                cand |= (ops - have)
        if not cand:
            break
        pick = sorted(cand)[0]
        have.add(pick)
        out.append((len(have), pick, sum(1 for ops in tabs.values() if runnable_fn(ops, have))))
    return out


def case_bundler_jar_reads_zero():
    """Reading the OUTER bundler jar must yield zero tables — and zero must be fatal.

    This is the trap that opened this investigation: `unzip -l server.jar |
    grep loot_table` returns nothing, and nothing looks like an answer.
    """
    jar = os.environ.get("LOOT_JAR")
    if not jar or not os.path.exists(jar):
        return None
    outer = real.read_tables(jar, prefix="data/minecraft/loot_table/")
    # The real reader descends into the nested jar, so it must find tables.
    return len(outer) > 1000


def main():
    results = []

    def run(name, real_fn, broken_fn):
        """A case earns its place only if it passes real AND fails broken."""
        try:
            r = real_fn()
        except Exception as exc:                       # noqa: BLE001
            r = f"raised {exc!r}"
        if broken_fn is None:
            b = "n/a"
        else:
            try:
                b = broken_fn()
            except Exception:                          # noqa: BLE001
                b = False                              # a crash counts as failing
        if r is None:
            results.append((name, "SKIP", r, b))
        elif r is not True:
            results.append((name, "RED-ON-REAL", r, b))
        elif broken_fn is not None and b is True:
            results.append((name, "VACUOUS", r, b))
        else:
            results.append((name, "OK", r, b))

    c1 = case_nested_tokens_are_seen()
    run("nested tokens are seen",
        lambda: c1(real.opcodes),
        lambda: c1(shallow_opcodes))

    c2 = case_unknown_token_is_not_runnable()
    run("an unknown token blocks runnable",
        lambda: c2(real.opcodes),
        lambda: c2(real.opcodes, lenient_runnable))

    c3 = case_curve_prices_every_opcode()
    run("the cost curve prices every opcode in the corpus",
        lambda: c3(real.opcodes),
        lambda: c3(shallow_opcodes))

    run("the cost curve is not fooled by partial support",
        lambda: c3(real.opcodes),
        lambda: c3(real.opcodes, lenient_runnable))

    run("the reader descends the bundler jar",
        case_bundler_jar_reads_zero,
        None)

    width = max(len(n) for n, _, _, _ in results)
    bad = 0
    for name, verdict, r, b in results:
        print(f"{verdict:12s} {name:{width}s}   real={r}  broken={b}")
        if verdict in ("RED-ON-REAL", "VACUOUS"):
            bad += 1
    print()
    if bad:
        print(f"loot-census-falsify: {bad} case(s) did not discriminate")
        return 1
    ran = sum(1 for _, v, _, _ in results if v == "OK")
    print(f"loot-census-falsify: {ran} case(s) passed on the real walk and failed on a broken one")
    return 0


if __name__ == "__main__":
    sys.exit(main())
