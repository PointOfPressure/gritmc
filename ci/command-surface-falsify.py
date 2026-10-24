#!/usr/bin/env python3
"""Prove `ci/command-surface.py`'s refusals can fire, and say which ones this cannot reach.

    python3 ci/command-surface-falsify.py    # ~1 min, needs the jar, no build, no box

# WHY THIS EXISTS

`ci/command-surface.py --check` prints one line and exits 0. Every refusal in
it guards against an input nobody has produced yet: the report and the executed
dump have agreed at 90 roots every time they have been compared. A refusal
that has never fired and is never tested is a refusal you are trusting, not
one you have.

Two of them fired for real while the script was being written -- `debugconfig`,
and `uuids` being reported as a root -- and both were fixed, which removed the
only inputs that violated them. That is the same position `ci/check-figures.py`
documents for its rules 9-13, and the same answer: inject the fault.

# THE FIXTURE, AND WHY IT IS A TREE AND NOT A MOCK

A hermetic tree in `mktemp -d` holding a copy of the checker, following
`ci/check-figures-falsify.py`. `ci/command-surface.py` derives its root from
`__file__`, so a copy at `<tmp>/ci/` reads `<tmp>` for everything: the jar is
symlinked because javap only reads it, and the four inputs a case perturbs are
real copies.

Run against the real tree instead, each case would have to mutate tracked
files and put them back, and a crash mid-case would leave a worktree that
looks like a lane's work.

# WHAT THIS DOES NOT COVER, STATED RATHER THAN IMPLIED

Three refusals read the constructor's bytecode, and this cannot reach them
without a jar built to be wrong:

  * `CommandsReport` not using `CommandSelection.ALL`;
  * a command class with two `dispatcher.register` calls, or none;
  * two classes recovering the same root.

They are the ones guarding the fragile step -- the first-literal rule -- so
their being untested here is the honest gap in this file, not a detail.
`--check` running on every `ci/t2-gameplay.sh` is what exercises them against
the real jar, and the second of the three is the one that fired on
`StopSoundCommand` during development.
"""

import hashlib, json, os, shutil, subprocess, sys, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "ci" / "command-surface.py"
COPIED = [
    "crates/rustmc-proto/src/lib.rs",
    "crates/rustmc-server/src/commands.rs",
    "data/vanilla/command_surface.json",
    "docs/generated/command-surface.tsv",
]


def build(work):
    """A tree the real checker will read as its own."""
    (work / "ci").mkdir(parents=True)
    shutil.copy2(SCRIPT, work / "ci" / "command-surface.py")
    os.symlink(ROOT / "oracle-work", work / "oracle-work")
    # Symlinked, not copied: the checker only reads these, and leaving them out
    # is what made the green control fail on its first run -- `invoked_by` went
    # to 0 for every root and the derived surface no longer matched the
    # committed one. The control earned its place before any refusal did.
    (work / "ci" / "checks").mkdir(parents=True, exist_ok=True)
    # Copied rather than symlinked so a case can *add* a scenario. Two of them
    # do, and they are the only way to show the step parser reads TOML rather
    # than one shape of TOML.
    shutil.copytree(ROOT / "ci" / "checks" / "gameplay",
                    work / "ci" / "checks" / "gameplay")
    for rel in COPIED:
        dst = work / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / rel, dst)


def check(work):
    p = subprocess.run([sys.executable, str(work / "ci" / "command-surface.py"), "--check"],
                       capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def drop_a_root(w):
    p = w / "data/vanilla/command_surface.json"
    d = json.loads(p.read_text())
    d["registered"].remove("difficulty")
    p.write_text(json.dumps(d))


def filter_does_nothing(w):
    p = w / "data/vanilla/command_surface.json"
    d = json.loads(p.read_text())
    d["usableAtAll"] = list(d["registered"])
    p.write_text(json.dumps(d))


def wrong_selection(w):
    p = w / "data/vanilla/command_surface.json"
    d = json.loads(p.read_text())
    d["selection"] = "ALL"
    p.write_text(json.dumps(d))


def no_dump(w):
    (w / "data/vanilla/command_surface.json").unlink()


def no_committed_surface(w):
    (w / "docs/generated/command-surface.tsv").unlink()


def edited_by_hand(w):
    """One cell of the generated table, edited by hand.

    THIS ARM WENT INERT ON 2026-09-05 AND THAT IS WHY IT IS WRITTEN THIS WAY.
    It used to be `l.replace("\\tno\\t", "\\tyes\\t", 1)` on the row starting
    `gamerule\\t` -- an edit that depends on the *content* of one named row.
    `commands: /gamerule end to end` flipped that row's `ported` cell from
    `no` to `yes`, the row then held no `\\tno\\t` at all, the mutator wrote the
    file back byte-identical, and `--check` correctly reported no drift. The
    case wanted rc=1, got rc=0, and `ci/t2-gameplay.sh` refused the gate --
    which is the falsifier working: an injection that has stopped matching is
    a hole in the sweep, and this one said so instead of going quiet.

    So the edit is now addressed by *column* and flips whatever it finds:
    `ported` is a two-valued cell, so a flip is a change for either value, on
    whichever data row comes first. `assert_mutated` in the harness is the
    general form of the same guard and covers the other eight mutators.
    """
    p = w / "docs/generated/command-surface.tsv"
    # root selection corpus_reach vanilla_class ported ... -- the header row
    # that names them is the last `#`-free line before the data.
    PORTED = 4
    out, done = [], False
    for line in p.read_text().splitlines(True):
        fields = line.rstrip("\n").split("\t")
        if (not done and not line.startswith("#")
                and len(fields) > PORTED and fields[PORTED] in ("yes", "no")):
            fields[PORTED] = "no" if fields[PORTED] == "yes" else "yes"
            line = "\t".join(fields) + "\n"
            done = True
        out.append(line)
    p.write_text("".join(out))


def scenario_with_reversed_keys(w):
    """A step whose `text` precedes its `do`.

    Valid TOML and invisible to the first version of `corpus_invocations`,
    which matched `do = "command"` followed immediately by `text` -- the order
    the three existing scenarios happen to use. This is the control for the
    per-block parse: it must be counted, so the surface drifts.
    """
    (w / "ci/checks/gameplay/900-reversed-keys.toml").write_text(
        'name = "reversed-keys"\nabout = "x"\nobserve_from = "actor"\n'
        'observe = ["chat"]\n\n[[step]]\n'
        '# a comment between the keys, too\n'
        'text = "gamerule doFireTick false"\ndo = "command"\n')


def scenario_invokes_a_nonexistent_command(w):
    """An invocation of something no dedicated server registers.

    The table is built by walking the surface and looking each root up, so
    without the stray check this contributes to no row and reads as no
    invocation at all.
    """
    (w / "ci/checks/gameplay/901-not-a-command.toml").write_text(
        'name = "not-a-command"\nabout = "x"\nobserve_from = "actor"\n'
        'observe = ["chat"]\n\n[[step]]\ndo = "command"\n'
        'text = "notacommand"\n')


def our_tree_moved(w):
    p = w / "crates/rustmc-server/src/commands.rs"
    p.write_text(p.read_text().replace('Node::leaf("seed")', 'Node::leaf("gamerule")', 1))


def tree_unreadable(w):
    p = w / "crates/rustmc-server/src/commands.rs"
    p.write_text(p.read_text().replace("roots: vec![", "roots: Vec::new(); //", 1))


CASES = [
    ("unperturbed", None, 0, "matches the jar"),
    ("executed-dump-lost-a-root", drop_a_root, 2,
     "the static and executed derivations of the dedicated surface disagree"),
    ("permission-filter-filters-nothing", filter_does_nothing, 2,
     "the permission filter cannot be being applied"),
    ("dump-is-not-dedicated", wrong_selection, 2,
     "was not dumped for CommandSelection.DEDICATED"),
    ("no-executed-dump", no_dump, 2, "Run ci/command-surface-dump.sh"),
    ("no-committed-surface", no_committed_surface, 2, "does not exist"),
    ("surface-edited-by-hand", edited_by_hand, 1, "has drifted"),
    ("our-tree-moved-without-regenerating", our_tree_moved, 1, "has drifted"),
    ("our-tree-unreadable", tree_unreadable, 2, "no root Node::leaf"),
    ("scenario-with-reversed-keys", scenario_with_reversed_keys, 1,
     "900-reversed-keys"),
    ("scenario-invokes-a-nonexistent-command", scenario_invokes_a_nonexistent_command, 1,
     "SCENARIOS INVOKE COMMANDS NO DEDICATED SERVER REGISTERS"),
]


def snapshot(work):
    """Every real file under the fixture, by content.

    Symlinks are skipped: `oracle-work` is 3.8 GB of shared references and no
    mutator touches it.
    """
    seen = {}
    for path in sorted(work.rglob("*")):
        if path.is_symlink() or not path.is_file():
            continue
        seen[str(path.relative_to(work))] = hashlib.sha256(path.read_bytes()).hexdigest()
    return seen


def main():
    bad = 0
    with tempfile.TemporaryDirectory() as d:
        for name, mutate, want_rc, want_text in CASES:
            work = Path(d) / name
            build(work)
            if mutate:
                # AN INJECTION THAT CHANGES NOTHING IS NOT AN INJECTION.
                # Added 2026-09-05, after `surface-edited-by-hand` spent an
                # unknown number of runs writing its file back byte-identical
                # because the row it named had stopped carrying the string it
                # searched for. That case still failed -- it wanted a refusal
                # and got a green -- but only because its expected rc happened
                # to differ from the unperturbed one. A mutator whose case
                # expects rc=0, or one whose refusal fires for some *other*
                # reason, would have gone inert in silence. This asks the
                # question directly, for all nine, and costs a directory walk.
                before = snapshot(work)
                mutate(work)
                if snapshot(work) == before:
                    bad += 1
                    print(f"FAIL {name}: the injection left the tree unchanged "
                          "-- it has stopped matching what it perturbs, so this "
                          "case proves nothing about the checker")
                    continue
            rc, out = check(work)
            ok = rc == want_rc and want_text in out
            if not ok:
                bad += 1
                print(f"FAIL {name}: rc={rc} (wanted {want_rc})")
                print("  " + out.strip().replace("\n", "\n  ")[:600])
            else:
                print(f"  ok  {name}: rc={rc}")

    # The unperturbed case is the discriminator. Without it a checker that
    # refused on absolutely everything would pass every line above, and
    # "8 refusals reachable" would mean nothing at all.
    if CASES[0][2] != 0:
        bad += 1
        print("FAIL the unperturbed case must be green or this proves nothing")

    if bad:
        print(f"\ncommand-surface-falsify: {bad} case(s) wrong")
        return 1
    print(f"\ncommand-surface-falsify: {len(CASES)} cases, 8 refusals and 2 "
          "detections reachable, "
          "1 green control. Three bytecode refusals are NOT covered here — see "
          "the header.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
