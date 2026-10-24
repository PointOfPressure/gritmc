#!/usr/bin/env python3
"""Refuse a readiness proof that cannot say "not ready".

# The defect, exactly

`ci/t2-gameplay.sh` proves this server is up by grepping its log for
`listening on` and breaking out of a wait loop. `crates/rustmc-server/src/
main.rs` prints that line at 162 and calls `status::listen` at 170 — **the
message precedes the bind**, and a failed bind prints `listen: …` and exits
after the grep has already succeeded. A server that never bound reads as
ready, and the harness goes on to record a corpus against a port nobody is
listening on.

The shape generalises past that one line. *Anything a harness treats as
evidence that a subject is alive is a predicate whose green must be turnable
red, and the way to show that is a log from a subject that is NOT ready which
the predicate rejects.* If no such log can be written, the predicate is not a
proof of anything.

# What this does

1. **Discovers** every readiness proof under `ci/` — a wait loop whose body
   greps a log and breaks. Discovery is what makes registration compulsory: a
   new proof that nobody registers is found and refused by name.
2. **Requires** each to be registered in `ci/checks/liveness-proofs.toml` with
   a `negative` fixture — the log text a subject that is *not* ready produces —
   and a `positive` one.
3. **Runs the real predicate** against both fixtures. Not a reading of the
   code: the registry carries the predicate as executable shell with `$LOG`
   bound to a file holding the fixture, and it is run. A predicate that
   accepts its own negative fixture is UNSOUND.

There is no way to satisfy this by writing a better comment, and no way to
satisfy it for `listening on` at all: no log containing that string is
rejected by `grep -q 'listening on'`, which is the refusal.

# The three that fail and the one that does not

Measured 2026-08-13. Three sites — `ci/t2-gameplay.sh`,
`ci/t2-gameplay-subset.sh`, `ci/block-entity-subset.sh` — wait on
`listening on` and accept a log whose next line is
`listen: Address already in use`. `ci/vanilla-server.sh` waits on `Done (`,
which vanilla prints only after it has bound, and it **rejects** the matching
negative fixture. So the gate discriminates: it is not refusing every proof it
can see, it is separating the sound one from the unsound ones.

**The three are not fixed here.** They are pinned as declared-unsound, by
name, with `MAX_UNSOUND` in this file bounding the population — so their
owners can repair them and nobody can add a fourth. Widening the allowance is
a code change with a reviewer, not a line in a data file.

`ci/corpus-falsifiable.py` is the other half of this pair and the two are not
separable: that one refuses a *scenario* that cannot go red, this one refuses
a *readiness proof* that cannot go red. See `docs/AGENT-BRIEF.md`.
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CI = ROOT / "ci"
DEFAULT_REGISTRY = ROOT / "ci" / "checks" / "liveness-proofs.toml"

# How many registered proofs may be declared unsound. **Zero**, since
# 2026-08-13: the three `listening on` sites were the whole population and
# branch `ci-readiness-proofs` repaired both halves of them — the server no
# longer prints the line before the bind, and the three proofs no longer stop
# at a string. In code, deliberately: raising it is a reviewed change, not a
# data edit made while adding a fourth.
#
# It was 3 when this file was written, and the count is the item. Anything
# that needs it above 0 is claiming there is a subject whose readiness cannot
# be observed, which has not been true of anything here yet.
MAX_UNSOUND = 0

# A readiness proof, as it looks in shell: a `grep -q` on a log file inside a
# loop, whose success ends the wait. Deliberately broad — a false positive
# costs one registry entry, and a false negative costs the thing this file is
# about.
#
# # THE THREE SHAPES DISCOVERY USED TO MISS — 2026-09-05
#
# Discovery is what makes registration compulsory, so **a proof discovery
# cannot see is a proof nobody has to register**, and the count it prints
# ("6 discovered in ci/*.sh") reads as coverage while measuring only which
# formatting the regexes happen to match. That is this project's own defect
# class applied to the instrument that exists to catch it. Three shapes were
# invisible, all three of them real proofs in this tree:
#
#   1. **`grep -qa`, `grep -qE`, `grep -qF`, `grep -qP`.** `-q\b` cannot match
#      `-qa`: `\b` after `q` needs a non-word character and `a` is a word
#      character. `ci/run-detached-falsify.sh`'s `await()` waits on
#      `grep -qa "^DETACHED-DONE …"` and was never discovered.
#   2. **A probe split across a backslash continuation.**
#      `ci/t2-gameplay-survival.sh:202` greps for the announcement on one
#      physical line and reaches `{ RUSTMC_READY=1; break; }` two lines later,
#      so `ENDS_WAIT` never saw a `break` on the probe's line. That is the
#      whole survival pass's only readiness proof.
#   3. **A multi-line `if … then` / `break` / `fi`.**
#      `ci/readiness-falsify.sh:163` — the boot wait inside the falsifier for
#      *this very gate*.
#
# So the source is folded into LOGICAL lines (continuations joined) before
# either regex is applied, and a probe that opens a `then` block is followed
# into that block for its `break`. `norm()`'d logical lines are also what the
# STALE arm compares a registered `source_line` against, so the two halves
# cannot disagree about what a line is.
LOOP = re.compile(r"^\s*(for\s|while\s|until\s|do\b)")
# `-[A-Za-z]*q[A-Za-z]*` rather than `-q`: any short-option cluster containing
# `q`, so `-qa`, `-qE`, `-qF`, `-qP` and `-aq` all count. `-q\b` matches none
# of them because a letter is a word character, and the narrow form found 6 of
# 9 wait loops in this directory — missing both of
# `ci/run-detached-falsify.sh`'s, which are the waits on the sentinel of the
# blessed detached runner every agent is told to use.
#
# AND THE FILE HALF HAD THE SAME SHAPE OF HOLE — 2026-09-05, second instance,
# found independently of the first and after it was fixed. `\b(log|LOG)\b`
# matches `$log` and `$LOG` and matches NEITHER `$PLOG` nor `$srvlog`, because
# a word boundary before `log` needs a non-word character before it and `P` is
# a word character. It is the same bug one operand to the right — and a name
# somebody chose because they already had a `$LOG` is exactly the SECOND wait
# loop in a script, which is the one a two-server gate has.
#
# Found by `ci/t2-sigterm-saves.sh`, whose two waits are on `$LOG` and
# `$PLOG`: the first was discovered and refused as unregistered, the second
# was invisible. The narrow form would have reported that gate as fully
# covered while half of it sat outside this check.
#
# Widened to any variable or path whose name ENDS in `log`, case-insensitively.
# The cost is in the direction this file's own comment calls cheap: a false
# positive is one registry entry.
PROBE = re.compile(r"\bgrep\s+-[A-Za-z]*q[A-Za-z]*\b.*[A-Za-z0-9_/.$\"]*[Ll][Oo][Gg]\b")
ENDS_WAIT = re.compile(r"\b(break|return\s+0)\b")
# A probe line that opens a block: the `break` is in the body, not beside the
# grep. Followed for at most BLOCK_WINDOW logical lines and never past the
# point the branch closes.
OPENS_BLOCK = re.compile(r"\bthen\s*$")
CLOSES_BLOCK = re.compile(r"^\s*(fi|else|elif)\b")
BLOCK_WINDOW = 6
# The pattern the PARAPHRASE arm pulls out of a source line, widened with
# PROBE: a paraphrase check that silently stops applying to `grep -qE` is a
# check that went green by not looking.
QUOTED_PATTERN = re.compile(r"grep\s+-[A-Za-z]*q[A-Za-z]*\s+(['\"])(.+?)\1")


def die(msg: str, code: int = 2) -> None:
    print(f"LIVENESS-PROOFS REFUSED — {msg}", file=sys.stderr)
    sys.exit(code)


def norm(s: str) -> str:
    return " ".join(s.split())


def logical_lines(text: str) -> list[tuple[int, str]]:
    """[(1-based line of the first physical line, joined source)].

    Backslash continuations are folded into one entry, because a shell
    statement split across three lines is one statement and a regex applied to
    the first third of it answers a question nobody asked.
    """
    lines = text.splitlines()
    out: list[tuple[int, str]] = []
    i = 0
    while i < len(lines):
        start = i
        buf = lines[i]
        # A comment is never continued: `# foo \` starts no statement, and
        # folding the next line into it would hide that line from discovery.
        if not buf.lstrip().startswith("#"):
            while buf.rstrip().endswith("\\") and i + 1 < len(lines):
                i += 1
                buf = buf.rstrip()[:-1] + " " + lines[i]
        out.append((start + 1, buf))
        i += 1
    return out


# Fragments after which a `;` must NOT be inserted when lines are rejoined.
_NO_SEP = ("then", "do", "else", "{", "(", "&&", "||", ";", "|")


def parses(text: str) -> bool:
    """Does this text parse as a shell statement, inside a loop body?

    The loop wrapper is not decoration: `ci/readiness-falsify.sh` runs a
    registered line as `for _ in 1; do eval "$line"; done`, so a bare `break`
    is legal there and must be legal here too.
    """
    r = subprocess.run(["bash", "-n", "-c", "for _ in 1; do\n" + text + "\ndone"],
                       capture_output=True, text=True)
    return r.returncode == 0


def _join(fragments: list[str]) -> str:
    out = ""
    for t in fragments:
        t = t.strip()
        if not t:
            continue
        if not out:
            out = t
        elif any(out.rstrip().endswith(w) for w in _NO_SEP):
            out += " " + t
        else:
            out += "; " + t
    return out


def runnable_span(logical: list[tuple[int, str]], idx: int) -> tuple[int, str] | None:
    """The smallest span around `idx` that PARSES and ends a wait, or None.

    **A `source_line` has to be a runnable statement, and two shapes were
    being reported as fragments. 2026-09-05.**

    `ci/readiness-falsify.sh` is a consumer that `eval`s a registered source
    line against real captures of a booting server. Registering
    `if grep -q … ; then` on its own gave it `eval: syntax error: unexpected
    end of file` — and, much worse, four of that script's five REFUSAL arms
    then printed `ok`, because a line that cannot be parsed reads exactly like
    a line that examined the log and said no. A gate arm passing on a syntax
    error is this project's defect class inside the instrument built to catch
    it. `ci/run-detached-falsify.sh:45` was the same thing by another door: a
    probe inside a `|| { … }` group whose opening brace is on the line above.

    Rather than a rule per shape, the span is grown — backward first, because
    an unclosed opener is behind you, then forward — and the FIRST span that
    both parses and contains a wait-ending verb is reported. `bash -n` decides,
    so a shape nobody anticipated is handled the same way, and `--check` runs
    the same parse over every registered line so an unrunnable one is refused
    at registration instead of discovered by a consumer.
    """
    own = logical[idx][1]
    # **A probe that is already a complete statement is reported alone, and is
    # never grown.** Growing it could only annex neighbouring lines, and a
    # span that borrowed some other loop's `break` would report a bare
    # post-loop `grep … || exit 1` as a readiness proof — which it did, in the
    # selftest fixture, the first time this was written. The reason to grow is
    # that the probe is a FRAGMENT, not that the wider text reads better.
    if parses(own):
        return (logical[idx][0], own) if ENDS_WAIT.search(own) else None
    for width in range(1, BLOCK_WINDOW + 1):
        for lo in range(max(0, idx - width), idx + 1):
            hi = lo + width
            if hi >= len(logical):
                continue
            frags = [t for _, t in logical[lo:hi + 1]
                     if t.strip() and not t.lstrip().startswith("#")]
            text = _join(frags)
            if not ENDS_WAIT.search(text):
                continue
            if parses(text):
                return logical[lo][0], text
    return None


def discover(ci: Path) -> list[tuple[str, int, str]]:
    """(script, line, source) for every readiness proof under `ci/`."""
    out: list[tuple[str, int, str]] = []
    # `**/*.sh`, not `*.sh`. The glob was non-recursive, so `ci/lib/` — the one
    # directory in this tree where a SHARED readiness helper would go, and
    # where `ci/server-ready.sh` will end up the day a second caller wants it —
    # was outside discovery's reach entirely. Nothing is hiding there today
    # (checked 2026-09-05: no wait loop outside `ci/*.sh` anywhere in the
    # tree), which is exactly when to close it: a scope that happens to be
    # complete is not a scope that is enforced.
    for p in sorted(ci.glob("**/*.sh")):
        logical = logical_lines(p.read_text(errors="replace"))
        for idx, (lineno, line) in enumerate(logical):
            if line.lstrip().startswith("#"):
                continue
            if not PROBE.search(line):
                continue
            # A probe whose wait-ending verb is on its own line is reported
            # as-is; otherwise the smallest span around it that PARSES and
            # ends a wait — see `runnable_span`. A probe that no span can
            # present as a runnable statement is not a wait this can test.
            span = runnable_span(logical, idx)
            if span is None:
                continue
            lineno, text = span
            # inside a loop: any loop keyword in the preceding few lines
            window = [s for _, s in logical[max(0, idx - 6):idx]]
            if not any(LOOP.match(w) for w in window):
                continue
            out.append((p.relative_to(ci.parent).as_posix(), lineno, norm(text)))
    return out


def run_predicate(predicate: str, fixture: str) -> bool:
    """True if the predicate calls this log READY."""
    with tempfile.NamedTemporaryFile("w", suffix=".log", delete=False) as fh:
        fh.write(fixture)
        path = fh.name
    try:
        r = subprocess.run(["bash", "-c", predicate], env={"LOG": path, "PATH": "/usr/bin:/bin"},
                           capture_output=True, text=True)
        return r.returncode == 0
    finally:
        Path(path).unlink(missing_ok=True)


def check(ci: Path, registry: Path, max_unsound: int = MAX_UNSOUND) -> int:
    if not registry.exists():
        die(f"no registry at {registry}")
    try:
        reg = tomllib.loads(registry.read_text())
    except tomllib.TOMLDecodeError as e:
        die(f"{registry} does not parse: {e}")
    proofs = reg.get("proof", [])
    fail: list[str] = []
    found = discover(ci)
    # Script paths in the registry are repo-relative (`ci/t2-gameplay.sh`), so
    # they resolve against the parent of the directory being scanned. That is
    # what lets the selftest point both at a fixture tree and exercise every
    # arm below on real files rather than on mocks.
    base = ci.resolve().parent

    by_src: dict[tuple[str, str], dict] = {}
    for p in proofs:
        for key in ("script", "source_line", "predicate", "what"):
            if key not in p:
                fail.append(f"MALFORMED   a proof entry has no `{key}`")
        if "script" in p and "source_line" in p:
            by_src[(p["script"], norm(p["source_line"]))] = p

    # **A registered source line must PARSE.** `ci/readiness-falsify.sh` evals
    # these against real captures, and on 2026-09-05 an `if … ; then` fragment
    # gave it `syntax error: unexpected end of file` — after which four of its
    # five REFUSAL arms printed `ok`, because a line that cannot be parsed
    # reads exactly like a line that looked at the log and said no. Refused
    # here, at registration, rather than found by a consumer.
    for p in proofs:
        src = p.get("source_line")
        if not isinstance(src, str):
            continue
        r = subprocess.run(["bash", "-n", "-c", f"for _ in 1; do {src}\n done"],
                           capture_output=True, text=True)
        if r.returncode != 0:
            fail.append(f"UNPARSEABLE {p.get('script')} registers a source_line that is not "
                        f"a runnable statement — a consumer evals these\n"
                        f"               {src}\n"
                        f"               {r.stderr.strip().splitlines()[-1] if r.stderr.strip() else ''}")

    # **Every proof about this server must say whether it is cross-examined.**
    # `ci/readiness-falsify.sh` boots the real binary into four not-ready
    # states and runs each proof's line against every capture. It selected its
    # population with `"rustmc.log" in source_line`, which is a proxy for the
    # real condition — that the line implements the harness contract, reading
    # `$WORK/rustmc.log`, matching `$OUR_PORT`, attributing to `$RUSTMC_PID`
    # and signalling with `RUSTMC_READY=1`. A proof that mentions the log and
    # does something else was swept in and could not run.
    #
    # So the contract is declared, and the declaration is compulsory: a proof
    # naming `rustmc.log` with neither `contract` nor a written
    # `why_no_contract` is refused. Opting out of cross-examination by
    # omission is how a proof stops being examined without anyone deciding it.
    for p in proofs:
        src = p.get("source_line", "")
        if "rustmc.log" not in src:
            continue
        if p.get("contract") == "rustmc-ready":
            continue
        if not str(p.get("why_no_contract", "")).strip():
            fail.append(f"UNDECLARED  {p.get('script')} names rustmc.log but declares neither "
                        f"contract = \"rustmc-ready\" nor why_no_contract — say whether "
                        f"ci/readiness-falsify.sh\n               cross-examines it against a "
                        f"real boot, because it selects its population from this field")

    # 1. everything discovered must be registered
    for script, line, src in found:
        if (script, src) not in by_src:
            fail.append(f"UNREGISTERED {script}:{line} waits on a log and is not in "
                        f"{registry.name} — every readiness proof ships with a log it "
                        f"REJECTS\n               {src}")

    # 2. every registered proof must still be in its script, verbatim
    for p in proofs:
        script, src = p.get("script"), norm(p.get("source_line", ""))
        path = base / str(script)
        if not path.exists():
            fail.append(f"ORPHAN      {script} does not exist")
            continue
        # The same view discovery reports from — folded continuations AND
        # rejoined `if … then … fi` blocks — or every proof of one of those
        # shapes would be permanently STALE the moment it was registered.
        lg = logical_lines(path.read_text(errors="replace"))
        haystack = {norm(l) for _, l in lg}
        haystack |= {norm(t) for i in range(len(lg))
                     if PROBE.search(lg[i][1]) and (sp := runnable_span(lg, i))
                     for _, t in [sp]}
        if src not in haystack:
            fail.append(f"STALE       {script} no longer contains the line this proof's "
                        f"fixtures were taken against — re-take them\n               {src}")

    # 3. the fixtures, run through the real predicate
    unsound: list[str] = []
    for p in proofs:
        name = f"{p.get('script')} ({p.get('what', '?')})"
        pred = p.get("predicate", "")
        if "negative" not in p:
            fail.append(f"NO-NEGATIVE {name} declares no log that a NOT-ready subject "
                        f"produces; a proof with no refutation is not a proof")
            continue
        # The registered predicate has to be the one in the script, not a
        # convenient paraphrase: whatever `grep` is matching in the source line
        # must be what the fixtures are run against.
        pat = QUOTED_PATTERN.search(p.get("source_line", ""))
        if pat and pat.group(2) not in pred:
            fail.append(f"PARAPHRASE  {name} registers a predicate that does not contain "
                        f"the source line's pattern {pat.group(2)!r}")
            continue
        if "positive" in p and not run_predicate(pred, p["positive"]):
            fail.append(f"BROKEN      {name} rejects its own POSITIVE fixture — a proof "
                        f"that never says ready is not a proof either")
        if run_predicate(pred, p["negative"]):
            unsound.append(name)
            if not p.get("unsound"):
                fail.append(f"UNSOUND     {name} accepts a log from a subject that is NOT "
                            f"ready; its green cannot be turned red")
        elif p.get("unsound"):
            fail.append(f"OVERSTATED  {name} is declared unsound and rejects its negative "
                        f"fixture — delete the declaration and let it count as sound")

    if len(unsound) > max_unsound:
        fail.append(f"RATCHET     {len(unsound)} unsound readiness proofs, allowance is "
                    f"{max_unsound} (MAX_UNSOUND in ci/liveness-proofs.py)")

    if fail:
        print("LIVENESS-PROOFS RED", file=sys.stderr)
        for f in fail:
            print("  " + f, file=sys.stderr)
        return 1
    sound = len(proofs) - len(unsound)
    print(f"liveness-proofs: {sound}/{len(proofs)} readiness proofs reject a log from a "
          f"subject that never became ready ({len(unsound)} pinned unsound, "
          f"{len(found)} discovered under {ci})")
    return 0


def selftest() -> int:
    bad: list[str] = []

    def want(case: str, ok: bool, detail: str = "") -> None:
        if not ok:
            bad.append(f"{case}: {detail}")

    # the predicate runner really runs shell against the fixture
    want("predicate accepts a matching log",
         run_predicate("grep -q 'listening on' \"$LOG\"", "rustmc listening on 0.0.0.0:1\n"))
    want("predicate rejects a log without the marker",
         not run_predicate("grep -q 'listening on' \"$LOG\"", "boom\n"))

    # THE HISTORICAL DEFECT, both directions. This is the reconstruction the
    # gate exists for: the real `listening on` predicate cannot reject a log
    # from a server whose bind failed, and a repaired one can.
    never_bound = ("rustmc 26.2 listening on 0.0.0.0:25599\n"
                   "listen: Address already in use (os error 98)\n")
    want("the historical predicate ACCEPTS a server that never bound",
         run_predicate("grep -q 'listening on' \"$LOG\"", never_bound))
    repaired = "grep -q 'listening on' \"$LOG\" && ! grep -q '^listen: ' \"$LOG\""
    want("a repaired predicate REJECTS it", not run_predicate(repaired, never_bound))
    want("and still accepts a server that did bind",
         run_predicate(repaired, "rustmc 26.2 listening on 0.0.0.0:25599\n"))

    # discovery finds a wait loop and ignores a comment and a bare grep
    root = Path(tempfile.mkdtemp(prefix="lp-selftest-"))
    tmp = root / "ci"
    tmp.mkdir()
    LINE = "grep -q 'up' \"$WORK/x.log\" && break"
    (tmp / "a.sh").write_text(
        "#!/bin/bash\n"
        "for _ in $(seq 1 60); do\n"
        f"    {LINE}\n"
        "    sleep 1\n"
        "done\n"
        f"# {LINE}\n"
        "grep -q 'up' \"$WORK/x.log\" || exit 1\n")
    got = discover(tmp)
    want("discovery finds the wait loop", len(got) == 1, f"found {got}")
    want("discovery reports the right line", got and got[0][1] == 3, f"{got}")

    # THE THREE SHAPES DISCOVERY USED TO MISS — 2026-09-05. Each one is a
    # proof that really exists in `ci/*.sh` and was invisible; each arm here
    # fails against the pre-widening regexes, which is the red-when-broken
    # evidence for the widening itself. `_OLD_*` below reconstruct them.
    shapes = root / "shapes"
    shapes.mkdir()
    (shapes / "a.sh").write_text(
        "#!/bin/bash\n"
        # shape 1 — a short-option cluster containing q (ci/run-detached-falsify.sh)
        "while [ $i -lt 200 ]; do\n"
        "    grep -qa \"^DONE $name \" \"$log\" 2>/dev/null && return 0\n"
        "    sleep 1\n"
        "done\n"
        # shape 2 — the probe and its break split by a backslash continuation
        # (ci/t2-gameplay-survival.sh)
        "for _ in $(seq 1 120); do\n"
        "    grep -q 'listening on' \"$WORK/rustmc.log\" 2>/dev/null \\\n"
        "        && owner_ok \\\n"
        "        && { READY=1; break; }\n"
        "    sleep 1\n"
        "done\n"
        # shape 3 — a multi-line if/then/break (ci/readiness-falsify.sh)
        "for _ in $(seq 1 600); do\n"
        "    if grep -q 'listening on' \"$W/ready/rustmc.log\" 2>/dev/null; then\n"
        "        break\n"
        "    fi\n"
        "    sleep 0.1\n"
        "done\n"
        # and a NON-proof of the same shape: an `if` whose branch does not end
        # the wait. A widening that swept this in would be trading a false
        # negative for a false positive.
        "for _ in $(seq 1 10); do\n"
        "    if grep -q 'oops' \"$W/other.log\" 2>/dev/null; then\n"
        "        echo saw it\n"
        "    fi\n"
        "    sleep 1\n"
        "done\n")
    # A probe in a SUBDIRECTORY of ci/. `ci/lib/` is where a shared readiness
    # helper goes, and the glob was `*.sh`, not `**/*.sh` — so that directory
    # was outside discovery's reach and a helper there would be unregistered
    # and unfindable. Nothing lives there today; a scope that happens to be
    # complete is not a scope that is enforced.
    sub = shapes / "lib"
    sub.mkdir()
    (sub / "ready.sh").write_text(
        "#!/bin/bash\n"
        "until [ -n \"$ok\" ]; do\n"
        "    grep -q 'Done (' \"$dir/server.log\" && return 0\n"
        "    sleep 1\n"
        "done\n")

    lines3 = discover(shapes)
    at = {n for _, n, _ in lines3}
    want("shape 1: grep -qa is a probe", 3 in at, f"{sorted(at)}")
    want("shape 2: a continued probe finds its break two lines on", 7 in at, f"{sorted(at)}")
    want("shape 3: if/then/break across lines", 13 in at, f"{sorted(at)}")
    want("an if branch that does not end the wait is NOT a proof",
         len([x for x in lines3 if x[0].endswith("a.sh")]) == 3, f"{sorted(at)}")
    want("a probe in a ci/ SUBDIRECTORY is discovered",
         any(s_ == "shapes/lib/ready.sh" for s_, _, _ in lines3),
         f"{[x[0] for x in lines3]}")

    # Discovery as it stood before 2026-09-05, reconstructed verbatim — the
    # `-q\b` probe applied to PHYSICAL lines, with the break required beside
    # the grep — run over the same fixture. It sees none of the three. That
    # 0-against-3 is what "the count measures which formatting the regex
    # matches" looked like, and it is this widening's red-when-broken evidence.
    _OLD_PROBE = re.compile(r"\bgrep\s+-q\b.*\b(log|LOG)\b")
    phys = (shapes / "a.sh").read_text().splitlines()
    old = [i + 1 for i, l in enumerate(phys)
           if not l.lstrip().startswith("#")
           and _OLD_PROBE.search(l) and ENDS_WAIT.search(l)
           and any(LOOP.match(w) for w in phys[max(0, i - 6):i])]
    want("the old discovery saw NONE of the three", old == [], f"old found {old}")

    # A folded continuation is what the STALE arm compares against, or every
    # shape-2 proof is permanently stale the moment it is registered.
    fold = {norm(s) for _, s in logical_lines((shapes / "a.sh").read_text())}
    # THE FRAGMENT, both shapes. A source line that does not parse is not
    # something a consumer can eval, and `ci/readiness-falsify.sh` evals these
    # — where a syntax error read as "refused" and four arms printed `ok`.
    want("shape 3 is reported as a COMPLETE statement, not the `if …; then` line",
         parses(next((s_ for _, n, s_ in lines3 if n == 13), "")),
         next((s_ for _, n, s_ in lines3 if n == 13), "_none_"))
    want("and every discovered line parses, whatever its shape",
         all(parses(s_) for _, _, s_ in lines3),
         str([s_ for _, _, s_ in lines3 if not parses(s_)]))
    want("a probe inside an unclosed group grows BACKWARD to the opener",
         parses(_join(["x || { sleep 1", "grep -q y \"$log\" && return 0; }"])))
    want("the fragment alone does NOT parse — the arm above is not vacuous",
         not parses('if grep -q y "$log"; then'))

    want("the folded line is what a registry entry can name",
         next((norm(s) for _, n, s in lines3 if n == 7), "_none_") in fold)

    reg = tmp / "r.toml"
    Q = '"""'

    def write(*extra: str, source: str = LINE, pred: str = "grep -q 'up' \\\"$LOG\\\"",
              neg: str | None = "down\n", pos: str | None = "up\n") -> None:
        body = ["[[proof]]", 'script = "ci/a.sh"', 'what = "x is up"',
                f'source_line = "{source.replace(chr(34), chr(92) + chr(34))}"',
                f'predicate = "{pred}"']
        if neg is not None:
            body.append(f"negative = {Q}\n{neg}{Q}")
        if pos is not None:
            body.append(f"positive = {Q}\n{pos}{Q}")
        body += list(extra)
        reg.write_text("\n".join(body) + "\n")

    write()
    want("a sound, registered proof passes", check(tmp, reg) == 0)

    write(neg=None)
    want("NO-NEGATIVE refused", check(tmp, reg) == 1)

    # the historical defect itself, in the gate rather than in the runner: a
    # predicate that accepts its own negative fixture
    write(neg="up\nthen it died\n")
    want("UNSOUND refused", check(tmp, reg) == 1)
    # `max_unsound=1` rather than the shipped constant: this case is about the
    # declaration being honoured, and it must keep testing that after
    # MAX_UNSOUND reached 0 on 2026-08-13. A selftest whose expectation is
    # derived from the constant it is checking around goes quiet exactly when
    # the constant changes.
    write("unsound = true", neg="up\nthen it died\n")
    want("UNSOUND tolerated once declared", check(tmp, reg, max_unsound=1) == 0)
    want("UNSOUND still refused when the allowance is 0", check(tmp, reg, max_unsound=0) == 1)
    write("unsound = true")
    want("OVERSTATED refused — a sound proof may not claim to be unsound",
         check(tmp, reg) == 1)

    write(pos="nothing here\n")
    want("BROKEN refused — a proof that rejects its positive fixture", check(tmp, reg) == 1)

    write(pred="true")
    want("PARAPHRASE refused — the fixtures must run the source line's pattern",
         check(tmp, reg) == 1)

    write(source="grep -q 'up' \"$WORK/other.log\" && break")
    want("STALE refused — the source line is no longer in the script",
         check(tmp, reg) == 1)

    reg.write_text("")
    want("UNREGISTERED refused — a discovered proof nobody registered",
         check(tmp, reg) == 1)

    # the ratchet: one more declared-unsound proof than the allowance permits.
    # The allowance is passed rather than read, so this arm keeps meaning
    # "beyond the allowance" whatever the shipped allowance is.
    (tmp / "b.sh").write_text("#!/bin/bash\nwhile :; do\n" + f"    {LINE}\n" + "done\n")
    one = ("[[proof]]\nscript = \"ci/{s}\"\nwhat = \"x\"\n"
           f'source_line = "{LINE.replace(chr(34), chr(92) + chr(34))}"\n'
           'predicate = "grep -q \'up\' \\"$LOG\\""\n'
           f"negative = {Q}\nup\n{Q}\nunsound = true\n")
    reg.write_text("".join(one.format(s=s) for s in ["a.sh"] * 2))
    # Only the refusal, deliberately. `ci/b.sh` exists in the fixture tree by
    # now and this registry does not name it, so a "tolerated at the
    # allowance" arm here would be reading an UNREGISTERED refusal and
    # calling it a ratchet. The tolerance case is the `max_unsound=1` arm
    # further up, where the registry covers every discovered script.
    want("RATCHET refused beyond the allowance", check(tmp, reg, max_unsound=1) == 1)

    shutil_rmtree(root)
    if bad:
        print("LIVENESS-PROOFS SELFTEST FAILED", file=sys.stderr)
        for f in bad:
            print("  " + f, file=sys.stderr)
        return 1
    print("liveness-proofs selftest: ok")
    return 0


def shutil_rmtree(p: Path) -> None:
    import shutil
    shutil.rmtree(p, ignore_errors=True)


USAGE = """usage:
  ci/liveness-proofs.py --check    [--ci DIR] [--registry FILE]
  ci/liveness-proofs.py --discover [--ci DIR]
  ci/liveness-proofs.py --selftest"""


def main(argv: list[str]) -> int:
    args = argv[1:]
    ci, registry = DEFAULT_CI, DEFAULT_REGISTRY
    for flag in ("--ci", "--registry"):
        while flag in args:
            i = args.index(flag)
            if i + 1 >= len(args):
                die(f"{flag} needs a value")
            if flag == "--ci":
                ci = Path(args[i + 1])
            else:
                registry = Path(args[i + 1])
            del args[i:i + 2]
    if not args:
        print(USAGE, file=sys.stderr)
        return 2
    if args[0] == "--selftest":
        return selftest()
    if args[0] == "--discover":
        for script, line, src in discover(ci):
            print(f"{script}:{line}  {src}")
        return 0
    if args[0] == "--check":
        return check(ci, registry)
    print(USAGE, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
