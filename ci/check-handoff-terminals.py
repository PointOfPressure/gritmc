#!/usr/bin/env python3
"""ci/check-handoff-terminals.py — every way ci/handoff.sh can stop is declared.

`ci/handoff.sh` is the only route to `main`. Its record is 41 GREEN / 32 RED and
until 2026-08-18 the chain had never once named a RED: 30 of the 32 arrived as
"no verdict line" and 2 as another branch's `MERGED`. That was repaired where
the verdicts are READ. This is the other half — whether the gate EMITS an
unambiguous verdict on every path it can take — and it is enforced rather than
described, because the thing that rotted was a claim about coverage.

WHAT MAKES THIS DIFFERENT FROM A TEST. A test proves the states it drives. This
proves there are no OTHER states: it re-derives the terminal set from the source
on every run and fails on one it has not been told about. A new `exit` cannot be
added to `ci/handoff.sh` without declaring what it prints, what it leaves
behind, and what forces it.

THE WORD IS DERIVED, NOT TRUSTED. For each terminal the checker finds the
nearest preceding `echo "WORD` and uses THAT as the verdict, not the word in the
declaration. A terminal whose declaration says `MERGED` and whose code prints
`MERGED-NOT-PUSHED` fails here, which is the defect that started this — a
declaration that merely restated the source would have agreed with it.

Exit 0 clean, 1 on any failure. No arguments.
"""
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GATE = ROOT / "ci" / "handoff.sh"
DECL = ROOT / "ci" / "checks" / "handoff-terminals.toml"
LANES = ROOT / "ci" / "lanes.sh"

# A verdict word as it appears at the start of an echoed line: capitals and
# hyphens only. `MERGED-NOT-PUSHED` is exactly why the hyphen is in the class —
# a pattern that stopped at the first hyphen would have read it as `MERGED` and
# agreed with the consumer that got this wrong.
WORD_RE = re.compile(r'echo "([A-Z][A-Z-]*)(?:\s|"|$)')
EXIT_RE = re.compile(r'\bexit\s+([0-9]+)\b')

failures: list[str] = []
notes: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)


def live_lines(text: str) -> list[tuple[int, str]]:
    """Lines of ci/handoff.sh that the laptop's bash actually executes.

    Heredoc bodies are stripped because they run on the BOX, in a different
    shell, and their `exit 1` is that shell's exit and not a verdict — stage 1's
    `no shared data to seed from` exits 1 inside `bash -s` and arrives here as
    `$STAGE1_RC`, which the caller turns into INFRA. Counting it as a terminal
    of this script would declare a RED that does not exist.

    Comments are stripped for the obvious reason and because this file's
    comments quote its own verdict lines constantly.
    """
    out, in_heredoc = [], None
    for n, line in enumerate(text.splitlines(), 1):
        if in_heredoc:
            if line.strip() == in_heredoc:
                in_heredoc = None
            continue
        m = re.search(r'<<-?\s*([A-Z_]+_EOF)\b', line)
        if m:
            in_heredoc = m.group(1)
            continue
        if line.lstrip().startswith("#"):
            continue
        out.append((n, line))
    return out


def derive(lines: list[tuple[int, str]], idx: int) -> tuple[str, int]:
    """The word a consumer would name for the terminal at lines[idx], and where
    that terminal's block starts.

    Scans backwards for the nearest `echo "WORD`, stopping at another terminal —
    a terminal cannot inherit the verdict of the one before it. Returns
    `(silent)` when nothing is printed, which is itself a finding: a terminal
    that prints no word is a terminal the chain records as "no verdict line".

    The block matters as much as the word. A declaration has to be able to name
    a terminal by the sentence it prints, and that sentence is almost never on
    the `exit` line: the multi-line `|| { echo ...; echo ...; exit N; }` shape is
    the normal one in this file. Matching the exit line alone would force every
    declaration to be written against whitespace, which is the least stable
    thing in the file and says nothing about which state it is.
    """
    for j in range(idx, max(-1, idx - 30), -1):
        text = lines[j][1]
        # The stop comes BEFORE the match, and the order is the whole
        # correctness of this function. A terminal must not inherit the verdict
        # of the terminal above it: `release_worktree "$BRANCH" || exit 2`
        # prints nothing itself, and the nearest `echo "WORD` above it belongs
        # to the `behind main` refusal three lines earlier. Reading that as this
        # terminal's word would have reported a silent terminal as a well-named
        # one — the precise error this file exists to catch, made by the file
        # itself.
        if j != idx and EXIT_RE.search(text):
            break
        m = WORD_RE.search(text)
        if m:
            return m.group(1), j
    return "(silent)", idx


def main() -> int:
    for p in (GATE, DECL, LANES):
        if not p.is_file():
            print(f"check-handoff-terminals: missing {p.relative_to(ROOT)}", file=sys.stderr)
            return 1

    decl = tomllib.loads(DECL.read_text())
    verdicts = {v["word"]: v for v in decl["verdict"]}
    terminals = decl["terminal"]
    max_unforced = decl["max_unforced"]

    # --- 1. the vocabulary is prefix-free -----------------------------------
    # The general form of the `MERGED-NOT-PUSHED` defect. An anchored prefix
    # match is the obvious way to classify a verdict line, so no word may be a
    # prefix of another or the two are not distinguishable by one.
    for a in verdicts:
        for b in verdicts:
            if a != b and b.startswith(a):
                fail(f"vocabulary: '{a}' is a prefix of '{b}' — `grep -E '^{a}'` matches both")

    # --- 2. every live terminal is declared, and by the word it prints -------
    lines = live_lines(GATE.read_text())
    found: list[tuple[int, str, int, str]] = []   # lineno, block, code, derived word
    for i, (n, text) in enumerate(lines):
        m = EXIT_RE.search(text)
        if not m:
            continue
        word, start = derive(lines, i)
        block = "\n".join(t for _, t in lines[start:i + 1])
        found.append((n, block, int(m.group(1)), word))

    # THE TERMINAL WITH NO `exit`. The script's last statement is the `MERGED`
    # line and bash then exits 0 by falling off the end. It is the single most
    # important terminal in the file and it is invisible to a scan for `exit`,
    # which is exactly the kind of state this check exists to stop anybody
    # forgetting. It is synthesised here so it has to be declared like the rest.
    # Only when the script does NOT end in an explicit `exit`. Since the merge
    # tail was reordered so that bookkeeping runs after the verdict, it does —
    # and synthesising a second terminal on the same line would claim it twice.
    # The synthesis stays because the property it guards is real: an `exit` can
    # be deleted from the end of a shell script without changing what it does,
    # and the most important terminal in the file would go undeclared.
    if not EXIT_RE.search(lines[-1][1]):
        tail_start = len(lines) - 1
        for j in range(len(lines) - 1, max(-1, len(lines) - 30), -1):
            if WORD_RE.search(lines[j][1]):
                tail_start = j
                break
        tail_word, _ = derive(lines, len(lines) - 1)
        found.append((lines[-1][0],
                      "\n".join(t for _, t in lines[tail_start:]),
                      0, tail_word))

    # `infra` and `unpushed` are functions whose bodies already appear above as
    # `exit 4` / `exit 5`; their call sites are not separate terminals and are
    # not counted twice.
    claimed: dict[int, list[dict]] = {}
    for t in terminals:
        pat = re.compile(t["match"])
        hits = []
        for n, text, code, word in found:
            if not pat.search(text):
                continue
            # Where a `match` is shared by two terminals — two bare `exit 0`
            # lines, for instance — the derived word is what separates them.
            if "emitted_by" not in t and word != "(silent)" and word != t["word"]:
                continue
            hits.append((n, text, code, word))
        want = t.get("count", 1)
        if len(hits) != want:
            fail(f"terminal '{t['id']}': match {t['match']!r} hit {len(hits)} line(s), declared {want}"
                 + (f" (lines {[h[0] for h in hits]})" if hits else " — stale declaration, or the terminal was removed"))
        for n, text, code, word in hits:
            claimed.setdefault(n, []).append(t)
            if code != t["exit"]:
                fail(f"terminal '{t['id']}' line {n}: code exits {code}, declaration says {t['exit']}")
            if "emitted_by" not in t and word != t["word"]:
                fail(f"terminal '{t['id']}' line {n}: prints '{word}', declaration says '{t['word']}'")
            if word == "(silent)" and "emitted_by" not in t:
                fail(f"terminal '{t['id']}' line {n}: prints NO verdict word. "
                     "A consumer records this as 'no verdict line'.")

    for n, text, code, word in found:
        if n not in claimed:
            fail(f"UNDECLARED TERMINAL at ci/handoff.sh:{n} (exit {code}, prints '{word}')\n"
                 f"    {text.strip()}\n"
                 f"    Add a row to ci/checks/handoff-terminals.toml saying what it leaves behind and what forces it.")
        elif len(claimed[n]) > 1:
            ids = ", ".join(t["id"] for t in claimed[n])
            fail(f"ci/handoff.sh:{n} is claimed by more than one declaration: {ids}")

    # --- 3. word and exit code cannot drift apart ---------------------------
    for t in terminals:
        v = verdicts.get(t["word"])
        if v is None:
            fail(f"terminal '{t['id']}': word '{t['word']}' is not in the vocabulary")
        elif v["exit"] != t["exit"]:
            fail(f"terminal '{t['id']}': prints {t['word']} but exits {t['exit']}; "
                 f"the vocabulary gives {t['word']} exit {v['exit']}")

    # --- 3b. the aggregate terminals declare every one of their causes ------
    #
    # `infra` and `unpushed` are functions: one `exit`, many call sites. A
    # control on the terminal alone proves only that the word can be printed,
    # not that each thing meant to reach it does — and the whole question this
    # file exists to answer is per-state, not per-word. The call sites are
    # counted in the source so the list cannot fall behind the code.
    # Counted over `live_lines`, not the raw source: this file's comments quote
    # its own call sites constantly, and a count that included them would be a
    # number that drifts every time somebody explains something.
    live = "\n".join(t for _, t in lines)
    for fn in ("infra", "unpushed"):
        # NOT `decl`: that name holds the parsed declaration for the whole
        # function, and rebinding it here left the `unforced` list below reading
        # a terminal dict instead of the top-level table — which reported all
        # six unforced terminals as newly unforced. Caught by the check itself.
        agg = next((t for t in terminals if t.get("id") == fn), None)
        if agg is None:
            continue
        # Call sites only. The definition is `fn() {`; a call is the name
        # followed by its first quoted argument.
        n = len(re.findall(rf'(?<![\w-]){fn} "', live))
        sites = agg.get("sites", [])
        if not sites:
            fail(f"terminal '{fn}': aggregates many causes and declares no `sites` list")
        elif n != len(sites):
            fail(f"terminal '{fn}': {n} call site(s) in ci/handoff.sh, {len(sites)} declared in `sites`")

    # --- 4. every terminal names a control, and the control exists ----------
    unforced = []
    for t in terminals:
        for site in t.get("sites", []):
            c = site.get("control", "")
            if not c:
                fail(f"terminal '{t['id']}': site {site.get('cause','?')!r} has no control")
            elif c.startswith("none"):
                unforced.append(f"{t['id']}:{site['cause'][:40]}")
            else:
                path, _, label = c.partition(":")
                f = ROOT / path
                if not f.is_file():
                    fail(f"terminal '{t['id']}' site {site['cause']!r}: control names {path}, which does not exist")
                elif label and label not in f.read_text():
                    fail(f"terminal '{t['id']}' site {site['cause']!r}: control names {path}:{label}, and that label is not in it")
        c = t.get("control", "")
        if not c:
            fail(f"terminal '{t['id']}': no control field at all")
            continue
        if c.startswith("none"):
            unforced.append(t["id"])
            if len(c) < 20:
                fail(f"terminal '{t['id']}': control is 'none' with no reason given")
            continue
        path, _, label = c.partition(":")
        f = ROOT / path
        if not f.is_file():
            fail(f"terminal '{t['id']}': control names {path}, which does not exist")
        elif label and label not in f.read_text():
            fail(f"terminal '{t['id']}': control names {path}:{label}, and that label is not in it")

    if len(unforced) > max_unforced:
        fail(f"{len(unforced)} terminals have no positive control and max_unforced is {max_unforced}: "
             + ", ".join(unforced))
    # The list, not only the count. A bare ceiling is satisfied by forcing one
    # terminal and un-forcing another, and it cannot tell "a repair added two
    # new terminals" from "two controls rotted away". The set must match
    # exactly, so adding to it is as loud as removing from it.
    declared_unforced = set(decl.get("unforced", []))
    actual_unforced = {u.split(":")[0] for u in unforced}
    if declared_unforced != actual_unforced:
        gained = sorted(actual_unforced - declared_unforced)
        lost = sorted(declared_unforced - actual_unforced)
        msg = "the `unforced` list does not match what the declaration actually says."
        if gained:
            msg += " Newly without a control: " + ", ".join(gained) + "."
        if lost:
            msg += " Now forced (remove from the list): " + ", ".join(lost) + "."
        fail(msg)
    notes.append(f"{len(terminals)} terminals, {len(verdicts)} verdicts, "
                 f"{len(unforced)} with no control (ceiling {max_unforced})")

    # --- 5. the only in-tree consumer names every word ----------------------
    # `ci/lanes.sh` classifies a gate's output with one hand-written anchored
    # alternation. A word this file adds that it does not carry reads to it as
    # `no verdict line`, which is the exact string that made 30 of 32 REDs
    # unnamable. The duplication is guarded here rather than hoped for.
    # TWO consumers, and the second one is worse when it is wrong. `ci/lanes.sh`
    # records a verdict it cannot name as `no verdict line` and moves on;
    # `docs/AGENT-BRIEF.md`'s waiter is an `until ... grep` loop, so a verdict
    # it cannot match makes the agent **wait forever**. Both patterns are
    # hand-written alternations of the same vocabulary, both were four of eight,
    # and the brief's own text records an earlier round of exactly this defect
    # one bullet above the line being checked. That is the argument for reading
    # them here rather than trusting them.
    consumers = [
        (LANES, r"grep -m1 -E '\^\(([A-Z|-]+)\)'",
         "A verdict it cannot name is reported as 'no verdict line'.", True),
        (ROOT / "docs" / "AGENT-BRIEF.md", r"\^\(([A-Z|-]+)\) \|\^FAIL",
         "It is an `until ... grep` waiter: a verdict it cannot match makes the agent wait forever.", True),
    ]
    # Consumers that live beside the checkout rather than in it — `gate-chain.sh`
    # is the chain itself. A gate copy does not ship them, so they are enforced
    # only where they exist. Enforcement happens where the file is, and a gate is
    # not reddened by the absence of something it was never given.
    for oot in decl.get("out_of_tree_consumer", []):
        p = (ROOT / oot["path"]).resolve()
        consumers.append((p, oot["match"], oot["why"], False))

    for path, rx, why, required in consumers:
        try:
            rel = path.relative_to(ROOT).as_posix()
        except ValueError:
            rel = path.as_posix()
        if not path.is_file():
            if required:
                fail(f"{rel}: missing, and it carries a verdict pattern this check enforces")
            else:
                notes.append(f"{rel} is not present here — not checked (it is enforced in the checkout that has it)")
            continue
        m = re.search(rx, path.read_text())
        if not m:
            fail(f"{rel}: could not find its verdict alternation — it moved, "
                 "and this check is now blind to it")
            continue
        known = set(m.group(1).split("|"))
        missing = sorted(set(verdicts) - known)
        if missing:
            fail(f"{rel}'s verdict pattern does not name: " + ", ".join(missing)
                 + f"\n    it has: {m.group(1)}"
                 + f"\n    {why}")

    for n in notes:
        print(f"handoff-terminals: {n}")
    if failures:
        print("", file=sys.stderr)
        for f in failures:
            print(f"handoff-terminals: {f}", file=sys.stderr)
        print(f"\nHANDOFF-TERMINALS RED — {len(failures)} problem(s)", file=sys.stderr)
        return 1
    print(f"HANDOFF-TERMINALS GREEN — {len(terminals)} terminals, every one declared and "
          f"{len(terminals) - len(unforced)} of them forced by a named control")
    return 0


if __name__ == "__main__":
    sys.exit(main())
