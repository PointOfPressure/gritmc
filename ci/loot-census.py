#!/usr/bin/env python3
"""loot-census.py — what vocabulary does vanilla's whole loot surface actually use?

Reads the **pinned jar** and nothing else. The 1,355 loot tables ship inside
`META-INF/versions/<ver>/server-<ver>.jar` at `data/minecraft/loot_table/**.json`
as ordinary datapack JSON, so this instrument never touches `data/vanilla/`,
never boots a server, and cannot be biased by where our own lines happen to be.
That is the whole point of it: `ci/unported-inventory.py` ranks by where work has
already been done (marker count correlates with lines written at r = 0.760,
p = 0.0001), so it cannot answer a question about work that has *not* been done.

# The question this answers, stated with the number

**"If a loot interpreter implements N opcodes, how many of the 1,355 vanilla
loot tables can it run end to end?"** Not "how many loot tables are there" —
that is 1,355 and it is not a rate. The cost of porting loot is the size of the
*vocabulary*, not the size of the corpus, and those two numbers differ by a
factor of thirty.

An "opcode" here is a typed token: an entry type, a condition, a function, a
number-provider kind, or an `apply_bonus` formula. A table is *runnable* under a
set of opcodes when every token it mentions, at every nesting depth, is in that
set. That is a deliberately strict definition — partial support for a condition
is not support, because a condition that silently answers `true` when it does
not understand itself is exactly the vacuous-green class this project keeps
finding.

# What this instrument cannot see, said before its output is quoted

It reads *declarations*. It does not and cannot know:

  * what any opcode **means** — `explosion_decay` is one token here and a
    method in bytecode there;
  * the **order random values are drawn in**, which is what parity actually
    turns on;
  * whether a table is ever **invoked**, which is a call-site fact;
  * which block or entity **maps** to which table.

Quoting a coverage percentage from this file as a parity percentage would be
the `use_item_on` `result`-column mistake in a new costume.
"""
import argparse
import collections
import json
import os
import re
import sys
import zipfile

DEFAULT_PREFIX = "data/minecraft/loot_table/"


def game_version(root):
    """Read the pinned version out of the proto crate, as the golden scripts do."""
    src = os.path.join(root, "crates", "rustmc-proto", "src", "lib.rs")
    with open(src, encoding="utf-8") as fh:
        m = re.search(r'GAME_VERSION: &str = "([^"]+)"', fh.read())
    if not m:
        sys.exit(f"loot-census: no GAME_VERSION in {src}")
    return m.group(1)


def read_tables(jar, prefix=DEFAULT_PREFIX):
    """Every loot table in the jar, as {id: parsed json}.

    The server jar shipped by Mojang is a *bundler*: it holds the real server at
    `META-INF/versions/<ver>/server-<ver>.jar` and the datapack lives in there,
    not in the outer archive. Grepping the outer jar for `loot_table` returns
    zero hits and that zero means nothing at all.
    """
    with zipfile.ZipFile(jar) as outer:
        inner = [n for n in outer.namelist()
                 if re.fullmatch(r"META-INF/versions/[^/]+/server-[^/]+\.jar", n)]
        if inner:
            import io
            with outer.open(inner[0]) as fh:
                data = io.BytesIO(fh.read())
            archive = zipfile.ZipFile(data)
        else:
            archive = outer
        out = {}
        for name in archive.namelist():
            if name.startswith(prefix) and name.endswith(".json"):
                with archive.open(name) as fh:
                    out[name[len(prefix):-len(".json")]] = json.load(fh)
        return out


def opcodes(table):
    """The typed tokens one table needs, at every nesting depth.

    Recurses through `any_of`/`all_of` terms and `inverted`'s single term, and
    through `alternatives`/`group`/`sequence` children, because a token buried
    three levels down is exactly as load-bearing as one at the top and an
    instrument that only looked at depth zero would undercount the vocabulary
    while looking thorough.
    """
    seen = set()

    def bare(x):
        return str(x).split(":")[-1]

    def number(v):
        if isinstance(v, dict):
            seen.add("num:" + bare(v.get("type", "?")))
            # `uniform` nests min/max providers, and either may itself be a score.
            for k in ("min", "max", "n", "p", "value"):
                if isinstance(v.get(k), dict):
                    number(v[k])
        elif isinstance(v, (int, float)):
            seen.add("num:constant")

    def condition(c):
        if not isinstance(c, dict):
            return
        seen.add("cond:" + bare(c.get("condition", "?")))
        for t in c.get("terms") or []:
            condition(t)
        if isinstance(c.get("term"), dict):
            condition(c["term"])
        if isinstance(c.get("predicate"), dict):
            for t in c["predicate"].get("terms") or []:
                condition(t)

    def function(f):
        if not isinstance(f, dict):
            return
        name = bare(f.get("function", "?"))
        seen.add("fn:" + name)
        if name == "set_count":
            number(f.get("count"))
        elif name == "apply_bonus":
            seen.add("bonus:" + bare(f.get("formula", "?")))
        elif name == "limit_count":
            lim = f.get("limit")
            if isinstance(lim, dict):
                for k in ("min", "max"):
                    number(lim.get(k))
        elif name in ("set_damage", "enchant_with_levels", "set_ominous_bottle_amplifier"):
            for k in ("damage", "levels", "amplifier"):
                if k in f:
                    number(f[k])
        for c in f.get("conditions") or []:
            condition(c)

    def entry(e):
        if not isinstance(e, dict):
            return
        seen.add("entry:" + bare(e.get("type", "?")))
        for c in e.get("conditions") or []:
            condition(c)
        for f in e.get("functions") or []:
            function(f)
        for ch in e.get("children") or []:
            entry(ch)

    for pool in table.get("pools") or []:
        number(pool.get("rolls"))
        if "bonus_rolls" in pool:
            number(pool.get("bonus_rolls"))
        for c in pool.get("conditions") or []:
            condition(c)
        for f in pool.get("functions") or []:
            function(f)
        for e in pool.get("entries") or []:
            entry(e)
    for f in table.get("functions") or []:
        function(f)
    return frozenset(seen)


def curve(tabs):
    """Minimum-opcode-set growth curve: (k, opcode, tables runnable with k opcodes).

    Greedy, choosing at each step the opcode that makes the most tables runnable
    and breaking ties by raw frequency among tables not yet runnable. Greedy is
    not optimal for set cover, so **this curve is an upper bound on cost** — the
    true minimum set for any given coverage is no larger than the one printed.
    Stating that direction matters: an upper bound on cost is the safe side of
    the argument being made from it.
    """
    freq = collections.Counter()
    for ops in tabs.values():
        for o in ops:
            freq[o] += 1
    have, out = set(), []
    while True:
        runnable = {t for t, ops in tabs.items() if ops <= have}
        cand = set()
        for t, ops in tabs.items():
            if t not in runnable:
                cand |= (ops - have)
        if not cand:
            break
        pick = max(cand, key=lambda o: (sum(1 for ops in tabs.values() if ops <= have | {o}), freq[o]))
        have.add(pick)
        out.append((len(have), pick, sum(1 for ops in tabs.values() if ops <= have)))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--jar", help="server jar; default is this tree's pinned one")
    ap.add_argument("--json", action="store_true", help="emit machine-readable output")
    ap.add_argument("--curve", action="store_true", help="print the full cost curve")
    args = ap.parse_args()

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    jar = args.jar
    if not jar:
        ver = game_version(root)
        for cand in (os.path.join(root, "oracle-work", f"vanilla-{ver}", "server.jar"),
                     os.path.join(root, "oracle-work", f"vanilla-{ver}", "versions", ver, f"server-{ver}.jar")):
            if os.path.exists(cand):
                jar = cand
                break
    if not jar or not os.path.exists(jar):
        sys.exit("loot-census: no jar; pass --jar (this reads the jar and nothing else)")

    tables = read_tables(jar)
    if not tables:
        sys.exit("loot-census: zero loot tables read — the jar layout changed, and a "
                 "zero here is an instrument failure, not a finding")
    tabs = {k: opcodes(v) for k, v in tables.items()}

    union = set()
    for ops in tabs.values():
        union |= ops
    cur = curve(tabs)
    per_table = collections.Counter(len(o) for o in tabs.values())
    by_family = collections.Counter(k.split("/")[0] if "/" in k else "(root)" for k in tabs)

    if args.json:
        json.dump({
            "jar": jar,
            "tables": len(tabs),
            "opcodes": len(union),
            "curve": [{"opcodes": k, "added": o, "runnable": n} for k, o, n in cur],
            "by_family": dict(by_family),
            "opcodes_per_table": dict(sorted(per_table.items())),
        }, sys.stdout, indent=2)
        print()
        return

    print(f"jar          {jar}")
    print(f"loot tables  {len(tabs)}")
    print(f"opcodes      {len(union)}   <- the vocabulary the whole surface is written in")
    print()
    print("by family:")
    for fam, n in by_family.most_common():
        print(f"  {n:5d}  {fam}")
    print()
    print("opcodes needed by a single table:")
    for k, n in sorted(per_table.items()):
        print(f"  {k:3d} opcodes  {n:5d} tables")
    print()
    print("cost curve — tables runnable per opcode implemented (greedy: upper bound on cost)")
    for k, o, n in cur:
        if args.curve or k <= 12 or n == len(tabs) or k % 10 == 0:
            print(f"  {k:3d}  {o:44s} {n:5d}  {100 * n / len(tabs):6.2f}%")


if __name__ == "__main__":
    main()
