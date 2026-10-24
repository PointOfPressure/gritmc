#!/usr/bin/env python3
"""Fire every refusal in `ci/probe-absences.py`, on trees built to be wrong.

    ci/probe-absences-falsify.py

Each arm builds a **shadow tree**: the real jar and the real tables reached
through symlinks, and copies of only the two things an arm mutates — the
absence ledger and the generators' Java sources. Nothing is regenerated and
nothing in the working tree is touched, so an arm cannot pass by having
quietly repaired the thing it was meant to break.

# THE DEFECT EACH ARM PINS

  * `unexplained` — an override the jar declares, with no row in its table and
    no entry in the ledger. That is `DiodeBlock` before anyone looked, and it
    is the state this whole check exists to make impossible to be in.
  * `stale` — an entry for a class the table now *does* carry. A ledger that
    keeps explaining a closed gap is how a ledger stops describing the tree,
    and it reads exactly like a live entry.
  * `vanished` — an entry for a class the jar no longer declares. A version
    bump does this silently.
  * `structure-three` / `structure-three-fourth` — `ShapeThreeGolden` started
    inventing, either above its throw or below it. Both leave the `throw`
    substring intact, which is why the assertion counts ways out.
  * `structure-dual` — `ShapeReadsGolden` lost the half that throws, so its
    narrow probe silently became an inventing one while its counts went on
    balancing against `update_shape.json`.
  * `structure-immune` — `ShapeGolden` or `SignalGolden` changed from throwing
    for an unheld position to answering a default. That is a one-line edit
    that would move `update_shape.json` and `signal.json` into the exposed set
    and turn tens of thousands of refusals into silent absences, and nothing
    else in the tree would notice.
  * `structure-exposed` — a level in `EXPOSED` changed from answering a default
    to throwing. That is the *repair*, and it must fail here too: the ledger
    entries explaining its absences would all be wrong and have to be retired,
    not left. The set is empty since 2026-08-18 — `UseGolden` moved to
    `FLAGGING` — so this arm has nothing to point at and is kept as an empty
    loop rather than deleted, because the category is still reachable.
  * `flagging-*` — the four ways `UseGolden` can go back to being silent while
    every grep still passes. This is the branch's whole mechanism and it is
    four independent switches, so it is four arms:
      - `flagging-unflagged`    the default answer stops setting the flag
      - `flagging-early-return` the flag stays and an early return jumps it,
                                which no substring check can see
      - `flagging-unemitted`    `interesting()` stops reading `reads`, so the
                                flag is set and the cell is dropped anyway —
                                this is exactly what `blockEntityAsked` was for
                                four months
      - `flagging-be-null`      `getBlockEntity` answers `null` for a block that
                                declares one and cannot be built, instead of
                                refusing
"""
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "ci" / "probe-absences.py"
LEDGER = ROOT / "ci" / "checks" / "probe-absences.toml"
GOLDEN = ROOT / "tools" / "goldengen" / "vanilla"


def shadow(tmp, citations=None, files=None):
    """A tree `ci/probe-absences.py` can run in, sharing the real inputs.

    `docs` and `crates` are symlinked as well as the extract, because the
    citation half of the check reads the sentences that quote its figures and
    a shadow without them reports every one as `CITATION-GONE` — which is
    correct behaviour and useless as a control.
    """
    root = pathlib.Path(tmp)
    (root / "ci" / "checks").mkdir(parents=True)
    (root / "tools" / "goldengen").mkdir(parents=True)
    # Symlinked: 2.3 GB of extract, the tables, and everything the citations
    # read. Nothing here writes to any of them.
    for name in ("oracle-work", "data", "docs", "crates"):
        (root / name).symlink_to((ROOT / name).resolve())
    # Copied: the two things an arm mutates.
    shutil.copytree(GOLDEN, root / "tools" / "goldengen" / "vanilla")
    shutil.copy(LEDGER, root / "ci" / "checks" / "probe-absences.toml")
    if citations is not None:
        p = root / "ci" / "checks" / "probe-absences.toml"
        p.write_text(p.read_text() + citations)
    for rel, text in (files or {}).items():
        f = root / rel
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
    return root


def run(root, *args):
    env = dict(os.environ, PROBE_ABSENCES_ROOT=str(root))
    p = subprocess.run([sys.executable, str(CHECK), "--check", *args],
                       capture_output=True, text=True, env=env)
    return p.returncode, p.stdout + p.stderr


def arm(name, want_rc, want_text, mutate, args=(), citations=None, files=None):
    with tempfile.TemporaryDirectory() as tmp:
        root = shadow(tmp, citations, files)
        mutate(root)
        rc, out = run(root, *args)
    ok = rc == want_rc and want_text in out
    print(f"  {'ok  ' if ok else 'FAIL'} {name:<20} rc={rc} "
          + (f"(saw {want_text!r})" if ok
             else f"— wanted rc={want_rc} and {want_text!r}"))
    if not ok:
        print("\n".join("      " + l for l in out.splitlines()[:10]))
    return ok


def drop_entry(cls):
    def go(root):
        p = root / "ci" / "checks" / "probe-absences.toml"
        blocks = p.read_text().split("\n[[absent]]")
        kept = [b for b in blocks if f'class   = "{cls}"' not in b]
        assert len(kept) == len(blocks) - 1, f"{cls} not found once"
        p.write_text("\n[[absent]]".join(kept))
    return go


def add_entry(cls, probe="setPlacedBy"):
    def go(root):
        p = root / "ci" / "checks" / "probe-absences.toml"
        p.write_text(p.read_text() + f'''
[[absent]]
probe   = "{probe}"
class   = "{cls}"
blocks  = 1
verdict = "environment"
reads   = "a fixture"
note    = "injected by ci/probe-absences-falsify.py"
''')
    return go


def swap(name, old, new):
    def go(root):
        p = root / "tools" / "goldengen" / "vanilla" / name
        src = p.read_text()
        assert old in src, f"{name} no longer contains the marker being swapped"
        p.write_text(src.replace(old, new, 1))
    return go


# The `EXPOSED` arms, by generator. Empty since `UseGolden` moved to
# `FLAGGING`; the loop stays so that a level put back into `EXPOSED` gets an arm
# with it rather than an arm somebody has to remember to write.
EXPOSED_ARMS = {}


def main():
    print("probe-absences-falsify: every arm on a shadow tree built to carry its defect")
    ok = True

    ok &= arm("control", 0, "PROBE-ABSENCES OK", lambda root: None)

    # # THE ARM POINTED AT AN ENTRY THAT HAD BEEN RETIRED, AND CRASHED
    #
    # It was `DiodeBlock` until 2026-08-18 — the corpse this check was cut
    # from — and then `CommandBlock`, the last `environment` entry. Then
    # `docs/cold-start/server-level-gate.md` closed `CommandBlock` by putting
    # the gate in the row and **retired its ledger entry**, and did not move
    # this arm. `drop_entry` raised `AssertionError: CommandBlock not found
    # once` before the check ran at all.
    #
    # That is this repo's own defect class wearing the falsifier's coat: an
    # arm that dies in its own setup is an arm that asserts nothing, and it is
    # louder than a silent pass only because it happens to crash. The lesson
    # from both moves is the same one — **an arm has to point at something
    # that is still there**, so it points at `StructureBlock`, which is one of
    # the three remaining absences and is `out-of-scope` rather than
    # `environment`, so nothing is going to close it out from under this.
    ok &= arm("unexplained", 1, "UNEXPLAINED  setPlacedBy StructureBlock",
              drop_entry("StructureBlock"))

    # BedBlock has 16 rows in set_placed_by.json, so an entry for it is an
    # explanation of something that is not absent.
    ok &= arm("stale", 1, "STALE  setPlacedBy BedBlock", add_entry("BedBlock"))

    ok &= arm("vanished", 1, "VANISHED  setPlacedBy NotAClassInTheJar",
              add_entry("NotAClassInTheJar"))

    ok &= arm("structure-immune", 1, "ShapeGolden.java no longer throws",
              swap("ShapeGolden.java",
                   'throw new UnsupportedOperationException(\n'
                   '                "updateShape read " + relative(pos)',
                   'if (true) { return net.minecraft.world.level.block.Blocks.AIR'
                   '.defaultBlockState(); }\n'
                   '            throw new UnsupportedOperationException(\n'
                   '                "updateShape read " + relative(pos)'))

    # The same defeat, on the harness this branch added. A grep for the throw
    # is green through both of these: the first leaves the throw in place and
    # returns above it, and the second leaves it in place and answers a fourth
    # position below it. Counting the ways out is what catches them.
    ok &= arm("structure-three", 1, "ShapeThreeGolden.java no longer throws",
              swap("ShapeThreeGolden.java",
                   "            readAt = relative(pos);",
                   "            if (true) { return subject; }\n"
                   "            readAt = relative(pos);"))

    ok &= arm("structure-three-fourth", 1, "ShapeThreeGolden.java no longer throws",
              swap("ShapeThreeGolden.java",
                   "            readAt = relative(pos);",
                   "            if (pos.equals(POS.above().above())) {\n"
                   "                return neighbor;\n"
                   "            }\n"
                   "            readAt = relative(pos);"))

    # And the census harness, which has to keep BOTH halves. Losing the throw
    # turns its narrow probe into an inventing one, and the four counts it
    # asserts against `update_shape.json` start describing a different extract
    # while still balancing.
    ok &= arm("structure-dual", 1, "ShapeReadsGolden.java is no longer both",
              swap("ShapeReadsGolden.java",
                   "            if (filler == null) {\n"
                   "                throw new UnsupportedOperationException(\n"
                   "                    \"updateShape read \" + relative(pos) "
                   "+ \", not its own neighbour\");\n"
                   "            }\n",
                   ""))

    for name in EXPOSED_ARMS:
        ok &= arm(f"structure-exposed-{name}", 1, "no longer answers a default",
                  swap(name, *EXPOSED_ARMS[name]))

    # **The flag, removed.** The default answer is still air; nothing on the
    # row says so; every one of the 90 blocks this branch recovered goes back
    # to being silent, and no substring anywhere else in the tree changes.
    ok &= arm("flagging-unflagged", 1, "getBlockState no longer records",
              swap("UseGolden.java",
                   "                readUnheldPosition = true;\n", ""))

    # **The flag, kept, and jumped over.** This is the arm the whole
    # count-the-ways-out design exists for: `readUnheldPosition = true` is
    # still in the file, a grep for it passes, and it never executes.
    ok &= arm("flagging-early-return", 1, "getBlockState no longer records",
              swap("UseGolden.java",
                   "            BlockState here = blocks.get(pos);",
                   "            if (true) {\n"
                   "                return Blocks.AIR.defaultBlockState();\n"
                   "            }\n"
                   "            BlockState here = blocks.get(pos);"))

    # **The flag, set, and the cell dropped anyway.** A flag nothing emits is
    # the `blockEntityAsked` field as it stood from March to August: set on
    # every ask, read by nobody, and 100 blocks missing behind it.
    ok &= arm("flagging-unemitted", 1, "interesting no longer records",
              swap("UseGolden.java",
                   "return refusal != null || !writes.isEmpty() || !reads.isEmpty()",
                   "return refusal != null || !writes.isEmpty()"))

    # **The refusal, softened to a null.** `MovingPistonBlock` declares
    # `EntityBlock` and answers `null`, so a real moving piston has a block
    # entity this harness cannot build. Answering `null` there is a plausible
    # empty answer for a method the harness does not model, which is the whole
    # class of fault.
    ok &= arm("flagging-be-null", 1, "getBlockEntity no longer records",
              swap("UseGolden.java",
                   "                return no(\"a block entity for \"",
                   "                return null; // was: no(\"a block entity for \""))

    # **The gate census, which is derived from bytecode and not from a table.**
    # A `ServerLevel` in a *descriptor* is a call, not a gate —
    # `PiglinAi.angerNearbyPiglins(ServerLevel, ...)` is one, inside
    # `EnderChestBlock.useWithoutItem` itself — and counting those inflates the
    # figure with sites the harness reaches perfectly well. The arm proves the
    # discrimination by citing the count and moving it.
    gate_cite = ('\n[[cite]]\nfile = "sandbox/G.md"\n'
                 'value = "serverLevel.gates"\nquote = "gates: {}"\n')
    ok &= arm("gate-census-green", 0, "PROBE-ABSENCE-CITATIONS OK", lambda r: None,
              args=("--cite",), citations=gate_cite,
              files={"sandbox/G.md": "gates: 8\n"})
    ok &= arm("gate-census-stale", 1, "where the jar says 8", lambda r: None,
              args=("--cite",), citations=gate_cite,
              files={"sandbox/G.md": "gates: 9\n"})
    ok &= arm("gate-silent-green", 0, "PROBE-ABSENCE-CITATIONS OK", lambda r: None,
              args=("--cite",),
              citations=('\n[[cite]]\nfile = "sandbox/G.md"\n'
                         'value = "serverLevel.silent"\nquote = "silent: {}"\n'),
              files={"sandbox/G.md": "silent: 7\n"})

    # The citation half. Same tie the refusal ledger puts on its own counts,
    # and the same two answers: a sentence with the wrong number in it, and a
    # sentence that no longer exists to be checked.
    # **`setPlacedBy.StructureBlock.blocks` is 1, and it is the third class
    # these fixtures have named.** It was `useWithoutItem.SignBlock.blocks` at
    # 48 until the signs got a refusal and stopped being a figure this check
    # publishes; then `setPlacedBy.CommandBlock.blocks` at 3 until
    # `docs/cold-start/server-level-gate.md` closed `CommandBlock` by putting
    # the gate in the row and retired its entry, at which point three arms here
    # cited a key nothing derives and reported `CITATION-UNRESOLVED` instead of
    # what each was written to catch — so two of the three passed for the wrong
    # reason and one crashed.
    #
    # `StructureBlock` is chosen for durability rather than for reading well:
    # it is `out-of-scope` rather than `environment`, so no branch is on its
    # way to closing it. Naming a *live* absence is the point — the fixture has
    # to resolve, or every arm below collapses into the one branch that says it
    # does not.
    sandbox = ('\n[[cite]]\nfile = "sandbox/X.md"\n'
               'value = "setPlacedBy.StructureBlock.blocks"\n'
               'quote = "structure blocks: {} of them"\n')
    ok &= arm("citation-green", 0, "PROBE-ABSENCE-CITATIONS OK", lambda r: None,
              args=("--cite",), citations=sandbox,
              files={"sandbox/X.md": "structure blocks: 1 of them\n"})
    ok &= arm("citation-stale", 1, "where the jar says 1", lambda r: None,
              args=("--cite",), citations=sandbox,
              files={"sandbox/X.md": "structure blocks: 12 of them\n"})
    # Reworded means the sentence HEAD is gone. "signs: plenty" keeps the head
    # and so is a CITATION, not a CITATION-GONE — which is the right answer and
    # was this arm's first fixture, so the arm was testing the other branch.
    ok &= arm("citation-gone", 1, "reworded or moved", lambda r: None,
              args=("--cite",), citations=sandbox,
              files={"sandbox/X.md": "the count of structure blocks is 1\n"})
    ok &= arm("citation-unresolved", 1, "not a figure this check derives",
              lambda r: None, args=("--cite",),
              citations=('\n[[cite]]\nfile = "sandbox/X.md"\n'
                         'value = "no.such.figure"\nquote = "x {}"\n'),
              files={"sandbox/X.md": "x 1\n"})
    ok &= arm("citation-reddens-check", 1, "CITATION  sandbox/X.md",
              lambda r: None, citations=sandbox,
              files={"sandbox/X.md": "structure blocks: 12 of them\n"})

    print("probe-absences-falsify: " + ("all arms fired" if ok else "AN ARM DID NOT FIRE"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
