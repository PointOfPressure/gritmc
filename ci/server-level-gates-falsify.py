#!/usr/bin/env python3
"""Fire every refusal in `ci/server-level-gates.py`, on trees built to be wrong.

    ci/server-level-gates-falsify.py

Each arm builds a **shadow tree**: the real jar and the real tables reached
through symlinks, and copies of the two things an arm mutates — the gate ledger
and **the check itself**. Nothing is regenerated and nothing in the working tree
is touched.

# WHY HALF THESE ARMS MUTATE THE CHECK AND NOT THE LEDGER

A ledger arm proves the check notices a wrong *declaration*. It cannot prove the
check's own derivation is doing the work the file claims — and this check's
value is almost entirely in the derivation, because `hides` and `columns` are
derived and only acknowledged in the ledger.

So the derivation arms replace the mechanism with the naive version somebody
would reach for first, and require the ledger to go red:

  * `cfg-offset-range`  — `hides` from an offset range instead of a control-flow
    walk. This is the version that reads correctly on five of the eight rules
    and picks up `VegetationBlock.useWithoutItem` on `SweetBerryBushBlock`,
    whose `super` call sits at a *higher* offset than the gate's branch target
    while having nothing to do with the gate.
  * `member-boundary`   — members matched on `(` instead of two-space indent, so
    `static {};` has no signature and its `<clinit>` instructions are attributed
    to the previous method. This is a real bug that was in the first draft.
  * `indy-unmapped`     — `invokedynamic.accept` removed from `EFFECTS`. A gated
    `invokedynamic` means a lambda runs past the gate, and it must reach the
    vocabulary and be refused rather than be dropped as unrecognised.
  * `instanceof-any-class` — the gate matched on the `instanceof` **opcode** with
    no check of which class it names. This is the discrimination that carries
    weight: every one of these bodies also tests `instanceof SomeBlockEntity`,
    and admitting those adds **14 rules** that are not gates at all —
    `DispenserBlock`, `HopperBlock`, `BeaconBlock`, `JukeboxBlock`,
    `StructureBlock` and nine more.

# TWO GUARDS THAT ARE INERT IN 26.2, MEASURED AND KEPT

Written as arms, run, and found not to fire — so they are recorded here rather
than shipped as arms that assert nothing:

  * **the lambda union changes no rule today.** `SweetBerryBushBlock` is the only
    rule with a gated `invokedynamic`, and its `loot` token already comes from
    the `dropFromBlockInteractLootTable` call beside it, so walking through to
    `Block.popResource` adds a token that is already present. The union is kept
    because it is what makes `invokedynamic.accept -> None` safe rather than a
    guess, and `indy-unmapped` pins the half that is load-bearing.
  * **matching the `instanceof` operand by string finds the same 8 rules.** Of
    the 66 probe-method bodies in the jar, exactly 8 mention `ServerLevel` and
    all 8 gate on it: there are **no descriptor-only mentions** to discriminate
    against. `ci/probe-absences.py` gives `PiglinAi.angerNearbyPiglins
    (ServerLevel, ...)` as the case its `_gated()` regex protects against, and
    that call is inside `EnderChestBlock.useWithoutItem`, which already has a
    real gate — so it could never have produced a false positive on its own.
    The guard is right in direction and costs nothing; it is just not what is
    doing the work, and `instanceof-any-class` is.
  * `vocabulary-open`   — a mapping removed from `EFFECTS`, which must be an
    `UNMAPPED` refusal and not a silent drop. A vocabulary that quietly ignores
    what it has not seen cannot mean what `columns` says it means.
"""
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "ci" / "server-level-gates.py"
LEDGER = ROOT / "ci" / "checks" / "server-level-gates.toml"


def shadow(tmp):
    root = pathlib.Path(tmp)
    (root / "ci" / "checks").mkdir(parents=True)
    for name in ("oracle-work", "data", "docs", "crates"):
        (root / name).symlink_to((ROOT / name).resolve())
    shutil.copy(CHECK, root / "ci" / "server-level-gates.py")
    shutil.copy(LEDGER, root / "ci" / "checks" / "server-level-gates.toml")
    return root


def run(root, *args):
    """Runs the **copied** check, so an arm may mutate the derivation itself."""
    env = dict(os.environ, SERVER_LEVEL_GATES_ROOT=str(root))
    p = subprocess.run([sys.executable, str(root / "ci" / "server-level-gates.py"),
                        "--check", *args], capture_output=True, text=True, env=env)
    return p.returncode, p.stdout + p.stderr


def arm(name, want_rc, want_text, mutate, args=()):
    with tempfile.TemporaryDirectory() as tmp:
        root = shadow(tmp)
        mutate(root)
        rc, out = run(root, *args)
    ok = rc == want_rc and want_text in out
    print(f"  {'ok  ' if ok else 'FAIL'} {name:<24} rc={rc} "
          + (f"(saw {want_text!r})" if ok
             else f"— wanted rc={want_rc} and {want_text!r}"))
    if not ok:
        print("\n".join("      " + l for l in out.splitlines()[:12]))
    return ok


def ledger(root):
    return root / "ci" / "checks" / "server-level-gates.toml"


def check_src(root):
    return root / "ci" / "server-level-gates.py"


def edit(path_of, old, new, count=1):
    """Swap a marker, asserting it is still there to swap."""
    def go(root):
        p = path_of(root)
        src = p.read_text()
        assert old in src, f"{p.name} no longer contains {old[:60]!r}"
        p.write_text(src.replace(old, new, count))
    return go


# `SignBlock` is two rules now — one on each half of the right-click — so a
# selector keyed on the class alone matches twice and the arms that use it
# would silently mutate whichever came first. Every selector takes the probe.
def selects(b, cls, probe):
    return f'class = "{cls}"' in b and f'probe = "{probe}"' in b


def drop_gate(cls, probe="useWithoutItem"):
    def go(root):
        p = ledger(root)
        blocks = p.read_text().split("\n[[gate]]")
        kept = [b for b in blocks if not selects(b, cls, probe)]
        assert len(kept) == len(blocks) - 1, \
            f"{probe}.{cls} not found exactly once"
        p.write_text("\n[[gate]]".join(kept))
    return go


def add_gate(cls, probe="useWithoutItem"):
    def go(root):
        p = ledger(root)
        p.write_text(p.read_text() + f'''
[[gate]]
probe = "{probe}"
class = "{cls}"
announces = false
guard = "no-branch"
hides = ["menu"]
columns = ["opens"]
verdict = "reachable"
note = "injected by ci/server-level-gates-falsify.py"
''')
    return go


def set_field(cls, field, value, probe="useWithoutItem"):
    """Rewrite one field inside one `[[gate]]` block."""
    def go(root):
        p = ledger(root)
        blocks = p.read_text().split("\n[[gate]]")
        hit = 0
        for i, b in enumerate(blocks):
            if not selects(b, cls, probe):
                continue
            lines = b.splitlines(keepends=True)
            for j, line in enumerate(lines):
                if line.startswith(f"{field} ="):
                    lines[j] = f"{field} = {value}\n"
                    hit += 1
                    break
            else:
                lines.insert(1, f"{field} = {value}\n")
                hit += 1
            blocks[i] = "".join(lines)
        assert hit == 1, f"{probe}.{cls}.{field} matched {hit} blocks"
        p.write_text("\n[[gate]]".join(blocks))
    return go


def materialise_tables(root):
    """Replace the `data` symlink with a directory of per-file symlinks.

    So an arm can rewrite one table without touching the working tree. The
    cross-check between the bytecode walk and what the dumper stamped is the
    only arm that compares two independent routes, and it cannot be exercised
    at all while the tables are read-only.
    """
    (root / "data").unlink()
    (root / "data" / "vanilla").mkdir(parents=True)
    for f in (ROOT / "data" / "vanilla").iterdir():
        (root / "data" / "vanilla" / f.name).symlink_to(f.resolve())


def unstamp(name, what='", \"gated\": ['):
    """Strip every `gated` array from one table, leaving the summary alone."""
    def go(root):
        materialise_tables(root)
        p = root / "data" / "vanilla" / name
        doc = p.read_text()
        out, i = [], 0
        while True:
            j = doc.find('"gated": [', i)
            if j < 0:
                out.append(doc[i:])
                break
            k = doc.index("]", j) + 1
            # eat the separator in front of it too
            start = doc.rfind(",", i, j)
            out.append(doc[i:start if start > i else j])
            i = k
        p.unlink()
        p.write_text("".join(out))
    return go


def set_total(key, value):
    return edit(ledger, f"{key} = ", f"{key} = {value}  # was ", 1)


def main():
    print("server-level-gates-falsify: every arm on a shadow tree built to "
          "carry its defect")
    ok = True

    ok &= arm("control", 0, "SERVER-LEVEL-GATES OK", lambda r: None)

    # --- the ledger half: a declaration that stopped describing the jar ------

    # SweetBerryBushBlock, because it is the one rule whose false column
    # `crate::useblock` actually consumes. An unledgered gate here is a live
    # bug nobody is counting.
    ok &= arm("unledgered", 1, "UNLEDGERED  SweetBerryBushBlock.useWithoutItem",
              drop_gate("SweetBerryBushBlock"))

    ok &= arm("vanished", 1, "VANISHED  NotAGatedClass.useWithoutItem",
              add_gate("NotAGatedClass"))

    ok &= arm("verdict", 1, "is not one of",
              set_field("ChestBlock", "verdict", '"probably-fine"'))

    # **The arm that matters most over a version bump.** If vanilla drops the
    # `Util.pauseInIde` call, 1,344 sign refusals silently become rows reading
    # `consumes: true, writes: []`. Nothing else in the tree would notice.
    ok &= arm("announces-flipped", 1, "ANNOUNCES  SignBlock.useWithoutItem",
              set_field("SignBlock", "announces", "false"))
    ok &= arm("announces-claimed", 1, "ANNOUNCES  ChestBlock.useWithoutItem",
              set_field("ChestBlock", "announces", "true"))

    ok &= arm("hides", 1, "HIDES  ChestBlock.useWithoutItem",
              set_field("ChestBlock", "hides", '["menu"]'))
    ok &= arm("columns", 1, "COLUMNS  ChestBlock.useWithoutItem",
              set_field("ChestBlock", "columns", '[]'))

    ok &= arm("blocks", 1, "BLOCKS  ShulkerBoxBlock.useWithoutItem",
              set_field("ShulkerBoxBlock", "blocks", "16"))
    ok &= arm("rows", 1, "ROWS  ChestBlock.useWithoutItem",
              set_field("ChestBlock", "rows", "9"))
    ok &= arm("live", 1, "LIVE  SweetBerryBushBlock.useWithoutItem",
              set_field("SweetBerryBushBlock", "live", "1"))

    # --- the guard half: which cells reach the gate at all -------------------

    # `ChestBlock`'s gate is the body's first branch, so a predicate is a claim
    # that some chests do not reach it.
    ok &= arm("guard-spurious", 1, "GUARD  ChestBlock.useWithoutItem",
              set_field("ChestBlock", "guard", '"age>1"'))
    # `SweetBerryBushBlock` branches on AGE first. `no-branch` here stamps
    # `age=0` and `age=1`, which never reach the gate.
    ok &= arm("guard-stale", 1, "GUARD  SweetBerryBushBlock.useWithoutItem",
              set_field("SweetBerryBushBlock", "guard", '"no-branch"'))
    # `all-states` overrides a real conditional, so it owes a reason.
    ok &= arm("guard-why", 1, "GUARD-WHY  EnderChestBlock.useWithoutItem",
              edit(ledger, 'guard = "all-states"\nguard_why = """\nTwo conditions',
                   'guard = "all-states"\nunused_why = """\nTwo conditions'))

    # --- the totals, which are what gets quoted -----------------------------

    ok &= arm("total-false-cells", 1, "TOTAL  false_cells",
              set_total("false_cells", 369))
    ok &= arm("total-silent-blocks", 1, "TOTAL  silent_blocks",
              set_total("silent_blocks", 33))

    # --- the derivation half: the mechanism, replaced with the naive one -----

    # `hides` from an offset range. Reads correctly on five of the eight and
    # picks up the `super.useWithoutItem` at offset 130 on the berry bush,
    # which the gate has nothing to do with — and which the closed vocabulary
    # has never been given a token for, so it surfaces as UNMAPPED rather than
    # as a quietly wider category.
    ok &= arm("cfg-offset-range", 1, "UNMAPPED  SweetBerryBushBlock",
              edit(check_src,
                   "        taken = reachable(code, fallthrough) "
                   "if fallthrough is not None else set()",
                   "        taken = {o for o, _, _ in code if o >= gate_at}"))

    # Members matched on `(`, so `static {};` never closes the previous method
    # and `<clinit>` instructions land inside it.
    ok &= arm("member-boundary", 1, "UNMAPPED",
              edit(check_src, 'MEMBER = re.compile(r"^  \\S.*;\\s*$")',
                   'MEMBER = re.compile(r"^  \\S.*\\(.*\\).*;\\s*$")'))

    # A gated `invokedynamic` must reach the vocabulary and be refused, not be
    # dropped for being unrecognised.
    ok &= arm("indy-unmapped", 1, "UNMAPPED  SweetBerryBushBlock",
              edit(check_src, '    "invokedynamic.accept": None,\n', ""))

    # **Which class the `instanceof` names is the discrimination that matters.**
    # Every one of these bodies also tests `instanceof SomeBlockEntity`; taking
    # any `instanceof` admits 14 rules that are not gates.
    ok &= arm("instanceof-any-class", 1, "UNLEDGERED",
              edit(check_src,
                   '        if op != "instanceof" or f"// class {SERVER_LEVEL}" '
                   'not in rest:',
                   '        if op != "instanceof":'))

    # The vocabulary, opened. `setBlock` is the token that decides whether a
    # row is false or merely incomplete, so dropping it must be loud.
    ok &= arm("vocabulary-open", 1, "UNMAPPED",
              edit(check_src, '    "setBlock": "block-state",', ""))
    # ...and a mapping that silently becomes structural is the same fault
    # wearing a different hat: `hides` loses `block-state` and the berry bush
    # stops being a write.
    ok &= arm("vocabulary-muted", 1, "HIDES  SweetBerryBushBlock.useWithoutItem",
              edit(check_src, '    "setBlock": "block-state",',
                   '    "setBlock": None,'))
    # The column map, broken the other way: `menu` stops being a column and
    # 362 false cells become "incomplete".
    ok &= arm("columns-muted", 1, "COLUMNS  ChestBlock.useWithoutItem",
              edit(check_src, '    "menu": "opens",', '    "menu": None,'))

    # --- useItemOn: the fifth population, and what is new about it ----------

    # The largest of the seven, and the one the whole area was wrong about:
    # `SignBlock.useItemOn` does **not** call `Util.pauseInIde`, so it is not
    # refused and not announced. If a jar ever grows that call, 2,784 cells
    # stop being rows and this must say so.
    ok &= arm("useitemon-announces", 1, "ANNOUNCES  SignBlock.useItemOn",
              set_field("SignBlock", "announces", "true", probe="useItemOn"))

    ok &= arm("useitemon-unledgered", 1, "UNLEDGERED  PumpkinBlock.useItemOn",
              drop_gate("PumpkinBlock", probe="useItemOn"))

    # **The item axis, made load-bearing.** `minecraft:pumpkin` records two
    # cells and one of them is the shears cell. A guard that says `all-states`
    # claims both were decided by the branch, and the dumper — which evaluates
    # the same guard against the real `ItemStack` — stamped one.
    ok &= arm("guard-item-axis", 1, "STAMP-DERIVED  use_item_on.json",
              set_field("PumpkinBlock", "guard", '"all-states"',
                        probe="useItemOn"))
    ok &= arm("guard-syntax", 1, "GUARD-SYNTAX  TntBlock.useItemOn",
              set_field("TntBlock", "guard", '"item~minecraft:shears"',
                        probe="useItemOn"))

    # **The gate one call down.** `TntBlock.prime` is `instanceof ServerLevel`
    # at its own offset 0 and the walk of `useItemOn` cannot see it.
    ok &= arm("via-unledgered", 1, "VIA  TntBlock.useItemOn",
              edit(ledger, '[gate.via."TntBlock.prime"]',
                   '[gate.unused."TntBlock.prime"]'))
    # ...and the version of that detection that reports every rule, because
    # `super.useItemOn` resolves to `Block.useItemOn` and the lookup lands
    # back on the rule itself.
    ok &= arm("via-self-reflection", 1, "VIA  SweetBerryBushBlock.useWithoutItem",
              edit(check_src,
                   "            if key == (cls, meth):\n                continue\n",
                   ""))

    # **A column belongs to a table.** `use_item_on.json` has no
    # `blockEntityWritten` field — `UseItemOnGolden` does not track one — and
    # four of these seven rules hide a block-entity write. Pointing them at
    # the column the *other* two tables have would tell a consumer to
    # disbelieve a field the row has never carried.
    ok &= arm("columns-not-per-probe", 1, "COLUMNS  SignBlock.useItemOn",
              edit(check_src,
                   '        "block-entity-write": None,\n',
                   ""))

    # **A cell is not a state.** `use_item_on.json` prints a default plus
    # groups, so counting states calls `minecraft:oak_sign` 1,344 cells where
    # the figure is 2,784.
    ok &= arm("cell-is-a-state", 1, "LIVE  SignBlock.useItemOn",
              edit(check_src,
                   "            if \"default\" not in st:\n                cells += 1\n"
                   "                refused += 1 if \"refused\" in st else 0\n"
                   "                continue\n            cells += 1",
                   "            cells += 1\n            if True:\n"
                   "                refused += 1 if \"refused\" in st else 0\n"
                   "                continue\n            cells += 1"))

    # --- the optional fifth population, from a tree that does not have it ----
    #
    # **The regression these two exist for was live for four commits.** Wiring
    # `useItemOn` into the sweep made every tree without `use_item_on.json` red
    # — the seven ledger entries read as VANISHED, which is a claim that the
    # *jar* stopped gating them, and the totals were declared for fifteen
    # rules. That is precisely what `docs/cold-start/server-level-gate.md`
    # predicted would happen and why it left the population in `--census`.
    def without_table(root):
        materialise_tables(root)
        (root / "data" / "vanilla" / "use_item_on.json").unlink()

    ok &= arm("optional-absent-green", 0,
              "SERVER-LEVEL-GATES OK — 8 gated rule(s), 7 silent over 34 "
              "blocks, 404 cell(s) stamped", without_table)

    # ...and the subtotal is still *checked* there, rather than the whole
    # totals block being skipped along with the probe. A tree with two tables
    # must still reproduce the two-table number.
    def without_table_and_wrong(root):
        without_table(root)
        set_total("false_cells", 369)(root)

    ok &= arm("optional-absent-subtotal", 1, "TOTAL  false_cells",
              without_table_and_wrong)

    # --- the cross-check, which no arm has ever exercised --------------------
    #
    # Everything above compares the jar to a ledger, and two files can agree
    # with each other while the table says something else entirely. This is
    # the one arm that mutates what the **dumper wrote**: strip every `gated`
    # array out of the table and the walk and the stamp must stop agreeing.
    ok &= arm("stamp-stripped", 1, "STAMP-COUNT  use_item_on.json",
              unstamp("use_item_on.json"))
    ok &= arm("stamp-stripped-swept", 1, "STAMP-COUNT  use_without_item.json",
              unstamp("use_without_item.json"))

    print("server-level-gates-falsify: "
          + ("all arms fired" if ok else "AN ARM DID NOT FIRE"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
