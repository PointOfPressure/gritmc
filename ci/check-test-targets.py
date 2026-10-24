#!/usr/bin/env python3
"""Adjudicate every cargo target in this workspace against a declared manifest.

# WHY THIS EXISTS

`ci/t0.sh` ran `cargo test --workspace --lib` for weeks and `tools/oracle-runner`
is a **binary** crate, so nineteen tests had never run in any gate and one of
them was failing. That was fixed to `--lib --bins` on 2026-08-03. On 2026-08-10
the same class came back one door along: an agent wrote an integration test
under `tests/`, T0 came back GREEN, and the green was because `--lib --bins`
**cannot build anything under `tests/`**. Its own deliverable was a check that
could not fail.

Both fixes were one target wide. Neither could answer the question that
actually matters, which is *what else is invisible* — and nothing in the tree
could answer it either, because nothing tied the set of targets that exist to
the set of targets a gate runs.

# WHY A FLOOR IS NOT ENOUGH, WHICH IS THE WHOLE ARGUMENT FOR THIS FILE

`ci/checks/t1-known-red.toml` already carries `min_binaries = 106`. That is a
real control and it catches the enumeration **collapsing** — a renamed crate, a
selector that stops matching, a workspace member dropped. It cannot catch the
defect above, and the asymmetry is the point: **adding** an unrun target moves
the count *up*, and every floor in the tree passes trivially when the number it
guards gets bigger. The failure this file exists for is silent in exactly the
direction a floor cannot see.

So this is a manifest, not a floor. Every target in the tree must be named in
`ci/checks/test-targets.toml` and placed in a bucket that says which gate runs
it and why. A target nobody classified is RED. A manifest entry with no target
behind it is RED too — a ledger that may rot by omission is not a ledger.

# THE DEFECT CLASS THIS MUST NOT JOIN

"A check that could not fail" has been found twenty-one times here, and a
manifest-driven checker is unusually easy to write in that shape: enumerate
nothing and it passes, declare a bucket nothing is in and the bucket is
decoration, mis-parse the gate's own command line and every answer is confident
and wrong. So this script asserts against **itself** as well as against the
tree:

* `[meta] min_targets` — a floor under the enumeration, so a `cargo metadata`
  that returns nothing is red rather than green. This is the one place a floor
  is the right instrument, and it guards the manifest rather than replacing it.
* every bucket must have at least one member and a non-empty `reason`. **A
  bucket no target is in is not a bucket**, the same rule `ci/check-figures.py`
  applies to its own rules.
* **the T0 selector is read out of `ci/t0.sh` and re-evaluated**, not trusted.
  The manifest claims a set of targets runs in T0; this script computes what
  T0's actual argv selects and refuses if the two disagree. Without this the
  manifest is a comment: an agent could narrow the selector and the manifest
  would still say the targets run.
* an argv token this script does not recognise is a **refusal**, never a
  silent skip. ORACLE rule 30 — an unrecognised knob is not a turned one. A
  selector parser that quietly ignores `--tests` would report the golden gates
  as unrun on a day somebody wired them in, or worse, the reverse.

# WHAT IT DOES NOT DO

It does not decide whether a target *should* run in T0. That is a judgement
about budget and about which gate owns a suite, and it belongs to whoever edits
the manifest. This script only makes the judgement **explicit and current**: an
unclassified target stops the gate, and a classification that no longer matches
what the gate does stops the gate.

# THE HOLE THIS FILE HAD UNTIL 2026-08-16, WHICH IS ITS OWN DEFECT CLASS

Everything above was true of T0 and **nothing whatever was true of T1**. The
manifest put 88 of the 110 targets in buckets marked `runs_in = "t1"`; twelve
of those buckets' `reason` fields argue at length that T1 covers them; the
summary line said `88 do not [run in T0]` and left the reader to believe the
prose. No line of this script had ever read `ci/t1.sh`.

So the ledger's central claim — *nothing in this workspace is outside every
gate* — was an assertion behind a check, in a file whose whole subject is that
an assertion is not a check. Narrow `ci/t1.sh` to `--lib --bins` and 88 targets
run nowhere, `ci/checks/test-targets.toml` still says they run in T1, and this
script still exits 0 while printing the same reassuring line. That is
`ACCEPT_TELEPORTATION` — a field read by nothing, behind a doc comment
describing a gate no line implemented — with a manifest wrapped round it.

It is fixed the only way it can be: **every stage's claim is re-evaluated
against that stage's own argv**, T1's exactly as T0's, and a target selected by
no stage is RED unless a bucket retires it on purpose. `retired` is what
`never` used to be called; the rename is so that the value reads as a decision
somebody made rather than as a fact about the world.

# WHY THE SUMMARY LINE CHANGED, WHICH IS NOT COSMETIC

It used to print `110 targets declared, 22 run in T0, 88 do not`. Every word of
that is true and the sentence is false: it names a hole of 88 where the
measured hole is 0, because the 88 are run by T1 on every merge. **A check that
cries wolf gets bypassed, and a bypassed check is worth less than no check** —
this is the same failure as a stale caveat list reading as authoritative. The
line now reports the partition it can defend: how many each stage is the first
to run, how many are retired, and how many run nowhere. The last number is the
one worth reading, and it is the one that was never printed.

# AND THE ONE BEHIND THAT, WHICH IT DID NOT SAY UNTIL 2026-08-16

The paragraph here used to read: "It also says nothing about whether a target
that runs actually *asserts* anything. A target with no `#[test]` in it passes
here and passes in cargo. `ci/checks/t1-known-red.toml`'s `min_tests` is the
instrument for that." Both sentences were true and the conclusion was wrong.
`min_tests` is an **aggregate** floor over the whole suite, and an aggregate
over 110 targets cannot show that twenty of them contributed nothing to it —
which is precisely what was happening: nine lib/bin binaries with no `#[test]`,
nine doctest targets with no runnable example, and two `#[ignore]`d
instruments. Nine of the twenty sat in `t0-unit`, so **T0 declared 22 targets
and 13 could go red**, over a ledger whose subject is checks that cannot fail.

So the question is asked here too, per target, and the answer is ratcheted:
`EXECUTES NO ASSERTION` refuses a target a stage runs that holds no runnable
test or doc example and is not in the manifest's `[[compile_only]]` category,
whose membership rule — every `#[test]` present and every one `#[ignore]`d —
is enforced rather than described so it cannot absorb whatever is left over.
`--calibrate <cargo test log>` is what stops the static derivation being taken
on trust.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "ci" / "checks" / "test-targets.toml"

# The stages that execute cargo targets, in the order a defect meets them. T2
# is deliberately absent and its absence is a measurement, not an oversight:
# `ci/t2-wire.sh`, `ci/t2-gameplay.sh`, `ci/t2-memory.sh`, `ci/t2-regiondiff.sh`,
# `ci/t2-vanilla-loads-ours.sh` and `ci/t3-determinism.sh` were grepped for
# `cargo test` on 2026-08-16 and run none. T2 drives built binaries against
# recorded vanilla traces; it is not a cargo-target stage, so a target cannot
# be "covered by T2" and no bucket may claim it is.
STAGE_FILE = {
    "t0": ROOT / "ci" / "t0.sh",
    "t1": ROOT / "ci" / "t1.sh",
}
# The bucket values `runs_in` may take. `retired` was spelled `never` until
# 2026-08-16 and nothing was ever in it; the rename is so the value reads as a
# decision with a name on it rather than as a claim about the universe.
RUNS_IN = tuple(STAGE_FILE) + ("retired",)

# The lines in `ci/t0.sh` that run this workspace's tests are found by this
# sentinel and not by pattern-matching `cargo test`, because `ci/t0.sh` holds a
# **second** `cargo test --workspace` with different semantics — the one inside
# the `cd "$PLUGINS_DIR"` subshell, which tests a different workspace entirely.
# Telling them apart by indentation or by line number is how a checker starts
# adjudicating the wrong command; the sentinel is unambiguous and its absence is
# a refusal rather than a fallback.
#
# **There may be more than one, and that is the expected shape rather than an
# edge case.** The rule T0 settled on is not "integration tests are T1", it is
# "tests that need minutes and the vanilla extract are T1" — so a test that
# gates a production wiring gets named on a line of its own. Every sentinel-
# marked line is read and the selections are unioned; what T0 runs is the union,
# and that is the set the manifest is adjudicated against.
#
# **The sentinel names its stage.** A `ci/t1.sh` line carrying T0's sentinel is
# a refusal rather than a match, because the commonest way a second stage gets
# wired up is by copying the first stage's line and its comment together, and a
# sentinel that matched either stage would then quietly attribute T1's argv to
# T0.
def sentinel(stage: str) -> str:
    return f"# check-test-targets: the next line is {stage.upper()}'s workspace test selector."


SENTINEL = sentinel("t0")
# And the counterpart, which is what stops an unmarked line being a silent hole.
# `ci/t0.sh` legitimately runs `cargo test` against the sibling `grit-plugins`
# workspace inside the `cd "$PLUGINS_DIR"` subshell; that line must be marked as
# foreign rather than left unlabelled, because "unlabelled" is exactly what a
# newly-added line for THIS workspace would also look like.
#
# **This is not bookkeeping and it was added for a merge that had not happened
# yet.** `joint-witness-607` adds `cargo test -p rustmc-server --test
# session_joint` to this file and merges ahead of this branch. Without the rule
# below, that line arrives carrying no sentinel, the checker computes T0's
# selection without it, the manifest also does not list it as a T0 target — and
# the two agree. A green gate, over a test that really is running in T0, with
# the ledger saying it is not. The comment telling the next author to add a
# sentinel is advice; this is the control.
FOREIGN = "# check-test-targets: the next line tests a DIFFERENT workspace, not this one."


class Refusal(Exception):
    """The instrument cannot answer, which is never the same as a pass."""


# ---------------------------------------------------------------------------
# enumeration


def key(pkg: str, kind: str, name: str) -> str:
    return f"{pkg}::{kind}::{name}"


def targets_from_metadata(meta: dict) -> list[dict]:
    """Every target cargo would build, plus the doctest targets cargo derives.

    A lib with `doctest = true` is **two** things to `cargo test`: a unittest
    binary and a doctest run. They are selected by different flags (`--lib` and
    `--doc`), so they are two entries here. Collapsing them is how a manifest
    ends up claiming that `--lib` covers a crate's doc examples.
    """
    out = []
    for p in meta["packages"]:
        for t in p["targets"]:
            kind = t["kind"][0]
            if kind not in ("lib", "bin", "test", "bench", "example"):
                continue
            out.append(
                {
                    "package": p["name"],
                    "kind": kind,
                    "name": t["name"],
                    "test": bool(t.get("test")),
                    "src_path": t.get("src_path"),
                }
            )
            if kind == "lib" and t.get("doctest"):
                out.append(
                    {
                        "package": p["name"],
                        "kind": "doc",
                        "name": t["name"],
                        "test": True,
                        # The doctest run reads the same sources the lib
                        # unittest binary is built from; it is a different
                        # *selector*, not a different tree.
                        "src_path": t.get("src_path"),
                    }
                )
    out.sort(key=lambda t: (t["package"], t["kind"], t["name"]))
    return out


def read_metadata() -> dict:
    """`cargo metadata --no-deps` reads manifests and compiles nothing.

    `--offline` first because T0 must not depend on the network, then a retry
    without it: a fresh clone with an unpopulated registry cache legitimately
    cannot resolve offline, and refusing there would make this script the
    reason a clean tree cannot gate.
    """
    # `--locked` here is defence in depth, and the measurement says so plainly
    # rather than letting the flag imply a fault it did not have.
    #
    # `Cargo.lock` is tracked and records `grit-plugin-api`, a path dependency
    # into the sibling `grit-plugins` checkout whose version this repository's
    # history cannot pin. When that sibling moved 0.2.1 -> 0.3.0 every cargo
    # command that RESOLVES the graph began rewriting the lockfile in whatever
    # worktree it ran in. Measured 2026-08-18 against a stale lockfile:
    #
    #     cargo tree --depth 1                        rewrote Cargo.lock
    #     cargo fetch                                 rewrote Cargo.lock
    #     cargo metadata --format-version 1           rewrote Cargo.lock
    #     cargo metadata --no-deps --format-version 1 did NOT
    #
    # **This call is the one that did not.** `--no-deps` reads manifests and
    # resolves nothing, so the enumeration was never a culprit; the first
    # draft of this comment said it was, on the strength of the full-graph
    # reading, and that was wrong. The culprits are T0's own `cargo
    # check/clippy/test` stages and any cargo an agent runs by hand.
    #
    # `--locked` is kept anyway because `--no-deps` is the reason this call is
    # safe, and a future edit that drops it would silently make this stage a
    # culprit too. It costs nothing: measured against a stale lockfile,
    # `--no-deps --locked` still exits 0 and leaves the tree clean, so it adds
    # no red that `--no-deps` alone would not have had.
    cmd = ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"]
    last = ""
    for extra in (["--offline"], []):
        try:
            r = subprocess.run(cmd + extra, cwd=ROOT, capture_output=True, text=True)
        except FileNotFoundError:
            # Not a pass and not a crash. An audit that cannot enumerate the
            # tree has measured nothing, and the one thing it must not do is
            # exit 0 — which is what a bare traceback through `set -e` in a
            # `|| { ... }` would have looked like to a reader of the log.
            raise Refusal("cargo is not on PATH — the enumeration could not run at all") from None
        if r.returncode == 0:
            return json.loads(r.stdout)
        last = r.stderr.strip()
    raise Refusal(f"cargo metadata failed: {last[-400:]}")


# ---------------------------------------------------------------------------
# whether a target can go red at all
#
# # WHY THIS IS HERE AND NOT ONLY IN THE T1 LOG READER
#
# The manifest above answers "does a gate run this target". It cannot answer
# "and could that run have failed", and on 2026-08-16 the difference was
# **twenty of the workspace's 110 targets** — nine lib/bin unittest binaries
# with no `#[test]` in them, nine doctest targets with no executable example,
# and two `#[ignore]`d instruments. All of them compiled, linked, ran, printed
# `test result: ok` and could not have failed; nine of the twenty sat inside
# `t0-unit`, the bucket this ledger holds up as the covered half. **T0 declared
# 22 targets and 13 of them could go red.**
#
# That measurement was taken by running the whole suite, which T0 cannot do —
# T0 compiles nothing. So the count here is derived **statically**, from the
# sources cargo would build, and the derivation is deliberately narrow:
#
#   * a test-binary target can go red if its module tree holds at least one
#     `#[test]`/`#[bench]` function that does not carry `#[ignore]`;
#   * a doctest target can go red if its lib's doc comments hold at least one
#     fenced example rustdoc will actually run.
#
# **A static model of a compiler's behaviour is exactly the kind of instrument
# this project has been wrong with before**, so it is not trusted: it is
# calibrated against a real measured suite log by `--calibrate <log>`, which
# refuses on any target where the model and the log disagree. The calibration
# on record is in `ci/checks/test-targets.toml` under `[assertions]`, and
# re-running it after a change to the walk below is one command over a T1 log
# that already exists.
#
# It is narrow in the safe direction on purpose. It counts *tests*, not
# assertions, so a test that runs and asserts nothing — `margin_probe` and
# `probe_margin_context` return early unless an env var is set — reads as
# red-capable here. That residual is declared in the ledger rather than
# quietly rounded off: see `[assertions] blind_to` there.

# `#[test]` / `#[bench]`, and `#[ignore]` in either spelling. Anchored to the
# start of a stripped line: an occurrence inside a string or a doc comment is
# not an attribute, and `//! #[test]` in a module header would otherwise be
# counted as a test that does not exist.
_ATTR = re.compile(r"^#\s*\[\s*(test|bench|ignore)\b")
_FENCE = re.compile(r"^(?:///|//!)\s*(`{3,}|~{3,})(.*)$")
_DOC_LINE = re.compile(r"^(?:///|//!)")
# The info-string tokens rustdoc still runs. Everything else — `ignore`,
# `no_run`, `compile_fail`, `text`, `bash`, `json`, a language name — means
# the example is not executed, and an executable-looking fence that is not
# executed is precisely what this file is for.
_RUST_INFO = {"", "rust", "should_panic", "edition2015", "edition2018", "edition2021", "edition2024"}


def module_files(root: Path) -> list[Path]:
    """`root` and every file reachable from it by `mod NAME;`.

    Cargo builds one target from one root file plus its module tree, so this
    is the file set of a target — and getting it right is what stops a
    package's `lib` tests being credited to its `bin`. `crates/rustmc-server`
    is the case that matters: `src/main.rs` declares no module and must be
    read as the 204-line file it is, not as the 589-test library beside it.

    `#[path = "..."]` is not handled, and its absence from this workspace was
    checked on 2026-08-16 rather than assumed — a `#[path]` module would drop
    out of this walk silently, so if one is ever added the calibration against
    a measured log is what catches it.
    """
    seen: set[Path] = set()
    out: list[Path] = []
    stack = [root]
    while stack:
        f = stack.pop()
        f = f.resolve()
        if f in seen or not f.is_file():
            continue
        seen.add(f)
        out.append(f)
        text = f.read_text(errors="replace")
        # `lib.rs`, `main.rs` and `mod.rs` own their own directory; any other
        # file owns a subdirectory named after it.
        base = f.parent if f.stem in ("lib", "main", "mod") else f.parent / f.stem
        for m in re.finditer(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", text, re.M):
            name = m.group(1)
            for cand in (base / f"{name}.rs", base / name / "mod.rs"):
                if cand.is_file():
                    stack.append(cand)
                    break
    return out


def scan_tests(files: list[Path]) -> tuple[int, int]:
    """`(runnable tests, tests carrying #[ignore])` over a target's sources.

    Attributes stack, so `#[test]` and `#[ignore]` on the same function are
    two lines and have to be associated. The association is the simple one
    cargo's own users rely on: a run of attribute lines belongs to the item
    that ends it.
    """
    runnable = ignored = 0
    for f in files:
        pending_test = False
        pending_ignore = False
        for raw in f.read_text(errors="replace").splitlines():
            line = raw.strip()
            m = _ATTR.match(line)
            if m:
                if m.group(1) in ("test", "bench"):
                    pending_test = True
                else:
                    pending_ignore = True
                continue
            if not line or line.startswith("//") or line.startswith("#"):
                # A `#[...]` that is not one of ours, a blank, or a comment
                # does not end the attribute run.
                continue
            if pending_test:
                if pending_ignore:
                    ignored += 1
                else:
                    runnable += 1
            pending_test = pending_ignore = False
    return runnable, ignored


def scan_doc_examples(files: list[Path]) -> tuple[int, int]:
    """`(examples rustdoc runs, fenced examples it does not)`."""
    runnable = declared_inert = 0
    for f in files:
        open_fence: str | None = None
        for raw in f.read_text(errors="replace").splitlines():
            line = raw.strip()
            if not _DOC_LINE.match(line):
                continue
            m = _FENCE.match(line)
            if not m:
                continue
            if open_fence is not None:
                if m.group(1)[0] == open_fence[0] and len(m.group(1)) >= len(open_fence):
                    open_fence = None
                continue
            open_fence = m.group(1)
            info = m.group(2).strip().strip("{}").replace(" ", "")
            tokens = {tok.strip() for tok in info.split(",")} if info else {""}
            if tokens <= _RUST_INFO:
                runnable += 1
            else:
                declared_inert += 1
    return runnable, declared_inert


def capability(targets: list[dict]) -> dict[str, dict]:
    """Per target: how many checks it carries, and how many are switched off."""
    out: dict[str, dict] = {}
    for t in targets:
        k = key(t["package"], t["kind"], t["name"])
        src = t.get("src_path")
        if not src:
            out[k] = {"runnable": 0, "inert": 0, "why": "no src_path in cargo metadata"}
            continue
        files = module_files(Path(src))
        if t["kind"] == "doc":
            runnable, inert = scan_doc_examples(files)
        else:
            runnable, inert = scan_tests(files)
        out[k] = {"runnable": runnable, "inert": inert, "why": ""}
    return out


# ---------------------------------------------------------------------------
# what T0's own command line selects


# A leading run of `VAR=value` assignments, optionally behind `env`, is part of
# the shell's command syntax and not part of cargo's argv. It has to be stripped
# rather than tripped over, and the *reason* is a hole rather than a nicety:
# until 2026-08-16 the unmarked-line scan below was `ln.strip().startswith(
# "cargo test")`, so `RUSTMC_FIXTURES=optional cargo test --workspace` on one
# line was not a `cargo test` line to this script at all. It would have carried
# no marker, raised no refusal, contributed no targets, and run a whole suite
# the ledger could not see. `ci/t1.sh` writes its env prefix on a continued
# line, so the tree never exercised the hole — which is exactly the kind of
# luck this project has stopped counting as a control.
_ENV_PREFIX = re.compile(r"^(?:env\s+)?(?:[A-Za-z_][A-Za-z0-9_]*=(?:\"[^\"]*\"|'[^']*'|\S*)\s+)*")


def logical_lines(text: str) -> list[tuple[int, str]]:
    """`(first physical line number, joined text)` with `\\`-continuations joined.

    `ci/t1.sh` runs its iceberg-override gate as an env prefix on one physical
    line and the `cargo test` on the next, joined by a backslash. Scanning
    physical lines sees a comment-able `cargo test` line whose real command
    begins two lines earlier — and inserting a sentinel between them would
    break the continuation and change what the shell runs. Joining first means
    the marker goes above the whole command, where a reader would put it.
    """
    out: list[tuple[int, str]] = []
    buf, start = "", 0
    for n, raw in enumerate(text.splitlines(), start=1):
        if not buf:
            start = n
        s = raw.strip()
        if s.endswith("\\"):
            buf += s[:-1].rstrip() + " "
            continue
        out.append((start, (buf + s).strip()))
        buf = ""
    if buf:
        out.append((start, buf.strip()))
    return out


# `ci/t1-par.sh <args>` runs `cargo <args>` with the test binaries launched
# in parallel through a runner; cargo still chooses every binary and argument
# (that script's header). So its argv selects exactly what `cargo <args>`
# would, and is read as that. Added on train-c over main 3973, where T1's
# workspace selector became `./ci/t1-par.sh test -q --workspace --no-fail-fast`
# and this reader refused it as "not a cargo test invocation".
_CARGO_WRAPPERS = ("./ci/t1-par.sh", "ci/t1-par.sh")


def is_cargo_test(line: str) -> str | None:
    """The argv of a `cargo test` command in `line`, env prefix stripped, or None."""
    cmd = _ENV_PREFIX.sub("", line)
    w = cmd.split()
    if w[:1] and w[0] in _CARGO_WRAPPERS:
        cmd = " ".join(["cargo", *w[1:]])
    return cmd if cmd.split()[:2] == ["cargo", "test"] else None


def read_selectors(text: str, stage: str, where: str) -> list[list[str]]:
    """Every argv `stage` uses to test THIS workspace, or a refusal.

    Generalised from a T0-only reader on 2026-08-16. The T1 half had never been
    written, so twelve buckets' `runs_in = "t1"` was a claim nothing read; see
    the module docstring.
    """
    want = sentinel(stage)
    lines = logical_lines(text)
    found: list[list[str]] = []
    for i, (n, ln) in enumerate(lines):
        if ln != want:
            continue
        if i + 1 >= len(lines):
            raise Refusal(f"a selector sentinel in {where} (line {n}) is the last line")
        cmd = is_cargo_test(lines[i + 1][1])
        if cmd is None:
            raise Refusal(
                f"the line after a selector sentinel in {where} is not a "
                f"`cargo test` invocation: {lines[i + 1][1]!r}"
            )
        found.append(cmd.split())

    # Every `cargo test` in the file must be claimed by one marker or the other.
    # An unmarked one is a refusal: it is either a new selector for this
    # workspace that the manifest knows nothing about, or a foreign one nobody
    # labelled, and this script cannot tell which — so it must not guess.
    #
    # The other stage's sentinel does not count as a marker. A `ci/t1.sh` line
    # under T0's sentinel is a copy-paste, and reading it as marked would file
    # T1's selection under T0.
    for i, (n, ln) in enumerate(lines):
        if is_cargo_test(ln) is None:
            continue
        prev = lines[i - 1][1] if i else ""
        if prev not in (want, FOREIGN):
            raise Refusal(
                f"{where} line {n} runs `{ln}` and carries neither marker. Put "
                f"`{want}` above it if it tests this workspace (and account for "
                "its targets in the manifest), or the DIFFERENT-workspace marker "
                "if it does not. An unmarked line is invisible here, and "
                "invisible is how a target ends up running in a gate while the "
                "ledger says it does not."
            )

    if not found:
        raise Refusal(
            f"no {stage.upper()} selector sentinel in {where} — this script cannot "
            "tell which of its `cargo test` lines tests this workspace, and "
            "guessing is how a checker adjudicates the wrong command"
        )
    return found


# Tokens that take no argument and select nothing. `-q`/`--quiet` and
# `--workspace` are here because they are what T0 actually passes; anything
# else is a refusal.
# `--locked` joined this set on 2026-08-18, with `ci/t0.sh`'s cargo stages. It
# removes cargo's permission to rewrite `Cargo.lock`, which a path dependency
# into the sibling `grit-plugins` checkout was making it do on every run in a
# worktree — see ci/lockfile-falsify.sh. It selects nothing, so it belongs
# here; the refusal that caught its absence is the right behaviour and is why
# the list is explicit rather than a prefix match.
_NOOP = {"-q", "--quiet", "--workspace", "--all", "--no-fail-fast", "--release",
         "--locked"}
# Tokens that consume the following token and select nothing.
_NOOP_ARG = {"--features", "-j", "--jobs", "--profile"}
# Tokens that consume the following token and narrow the selection to a package.
# `-p` is not a no-op: `cargo test -p rustmc-server --test session_joint` runs
# one target in one crate, and treating `-p` as noise would have this script
# claim T0 covers a same-named target in some other crate.
_PKG_ARG = {"-p", "--package"}


def select(targets: list[dict], argv: list[str]) -> set[str]:
    """Compute the set of targets `argv` selects, or refuse.

    Cargo's rule, which this mirrors: with **no** target selector at all,
    `cargo test` runs lib unittests, bin unittests, every integration test and
    every doctest. With any selector present, only what is named runs — which
    is precisely why `--lib --bins` runs no doctest and nothing under `tests/`,
    and precisely the fact both 2026-08 defects turned on.
    """
    if argv[:2] != ["cargo", "test"]:
        raise Refusal(f"not a cargo test invocation: {' '.join(argv)!r}")
    rest = argv[2:]
    pkgs = {rest[i + 1] for i in range(len(rest) - 1) if rest[i] in _PKG_ARG}
    if pkgs:
        known = {t["package"] for t in targets}
        for p in sorted(pkgs - known):
            raise Refusal(f"T0 selects `-p {p}` and no such workspace member exists")
        targets = [t for t in targets if t["package"] in pkgs]
    picked: set[str] = set()
    explicit = False
    i = 0
    while i < len(rest):
        tok = rest[i]
        if tok in _NOOP:
            i += 1
            continue
        if tok in _NOOP_ARG or tok in _PKG_ARG:
            i += 2
            continue
        if tok == "--":
            break
        if tok == "--lib":
            explicit = True
            picked |= {key(t["package"], "lib", t["name"]) for t in targets if t["kind"] == "lib" and t["test"]}
        elif tok == "--bins":
            explicit = True
            picked |= {key(t["package"], "bin", t["name"]) for t in targets if t["kind"] == "bin" and t["test"]}
        elif tok == "--tests":
            # `--tests` is every target with `test = true`, not just `tests/`.
            explicit = True
            picked |= {
                key(t["package"], t["kind"], t["name"])
                for t in targets
                if t["test"] and t["kind"] in ("lib", "bin", "test", "example")
            }
        elif tok == "--benches":
            explicit = True
            picked |= {key(t["package"], "bench", t["name"]) for t in targets if t["kind"] == "bench"}
        elif tok == "--examples":
            explicit = True
            picked |= {key(t["package"], "example", t["name"]) for t in targets if t["kind"] == "example"}
        elif tok == "--doc":
            explicit = True
            picked |= {key(t["package"], "doc", t["name"]) for t in targets if t["kind"] == "doc"}
        elif tok in ("--test", "--bin", "--bench", "--example"):
            explicit = True
            if i + 1 >= len(rest):
                raise Refusal(f"{tok} with no name in T0's selector")
            want, kind = rest[i + 1], tok[2:]
            hit = {key(t["package"], kind, t["name"]) for t in targets if t["kind"] == kind and t["name"] == want}
            if not hit:
                raise Refusal(f"T0 selects `{tok} {want}` and no such target exists")
            picked |= hit
            i += 2
            continue
        else:
            raise Refusal(
                f"unrecognised token {tok!r} in T0's test selector. An "
                "unrecognised knob is not a turned one (ORACLE rule 30): this "
                "script will not guess what T0 runs."
            )
        i += 1
    if not explicit:
        picked = {
            key(t["package"], t["kind"], t["name"])
            for t in targets
            if t["test"] and t["kind"] in ("lib", "bin", "test", "example", "doc")
        }
    return picked


# ---------------------------------------------------------------------------
# adjudication


def adjudicate(
    targets: list[dict],
    manifest: dict,
    stage_argvs: dict[str, list[list[str]]],
    cap: dict[str, dict] | None = None,
) -> list[str]:
    bad: list[str] = []
    meta = manifest.get("meta", {})
    buckets = manifest.get("bucket", [])

    if not buckets:
        return ["the manifest declares no buckets at all"]

    declared: dict[str, str] = {}
    runs_in_of: dict[str, str] = {}
    for b in buckets:
        bid = b.get("id", "<unnamed>")
        if not b.get("reason", "").strip():
            bad.append(f"bucket {bid!r} has no reason — an exclusion nobody wrote down is `unknown`, not a bucket")
        if b.get("runs_in") not in RUNS_IN:
            bad.append(f"bucket {bid!r} has runs_in={b.get('runs_in')!r}, not one of {'/'.join(RUNS_IN)}")
        members = b.get("members", [])
        if not members:
            bad.append(f"bucket {bid!r} has no members — a bucket no target is in is not a bucket")
        for m in members:
            if m in declared:
                bad.append(f"{m} is declared twice, in {declared[m]!r} and {bid!r}")
            declared[m] = bid
            runs_in_of[m] = b.get("runs_in")

    actual = {key(t["package"], t["kind"], t["name"]) for t in targets}

    floor = meta.get("min_targets")
    if floor is None:
        bad.append("[meta] min_targets is missing — the enumeration has no floor under it")
    elif len(actual) < floor:
        bad.append(
            f"the enumeration found {len(actual)} targets, floor {floor}. The selector has "
            "stopped reaching them; an audit over nothing passes exactly like a clean one"
        )

    for m in sorted(actual - set(declared)):
        bad.append(
            f"UNACCOUNTED TARGET {m} — it exists in the workspace and no bucket in "
            "ci/checks/test-targets.toml names it. Say which gate runs it and why, or "
            "say it is excluded and why; `unknown` is an allowed reason, silence is not"
        )
    for m in sorted(set(declared) - actual):
        bad.append(f"STALE MANIFEST ENTRY {m} — declared in bucket {declared[m]!r} and no such target exists")

    # Every stage's claim is checked against that stage's own argv. Until
    # 2026-08-16 only T0 was, and `runs_in = "t1"` on 88 targets was a sentence
    # in a TOML file that no line of code had ever read.
    real: dict[str, set[str]] = {}
    shown: dict[str, str] = {}
    for stage, argvs in stage_argvs.items():
        sel: set[str] = set()
        for argv in argvs:
            sel |= select(targets, argv)
        real[stage] = sel
        shown[stage] = " ; ".join(" ".join(a[2:]) for a in argvs)

    for stage in stage_argvs:
        up = stage.upper()
        claimed = {m for b in buckets if b.get("runs_in") == stage for m in b.get("members", [])}
        for m in sorted(claimed - real[stage]):
            bad.append(f"MANIFEST SAYS {up} RUNS {m} AND IT DOES NOT — {up}'s selectors are `{shown[stage]}`")

    # The reverse direction, and it is only symmetric for T0. T0 is the narrow
    # stage, so anything it runs must be attributed to it or the ledger
    # understates the fast gate. T1 is the wide one and legitimately re-runs
    # every T0 target, so `real[t1] - claimed[t1]` is not a fault; the fault it
    # would be hiding is caught by RUNS NOWHERE and RETIRED-BUT-RUN below,
    # which between them pin every target to a stage in both directions.
    claimed_t0 = {m for b in buckets if b.get("runs_in") == "t0" for m in b.get("members", [])}
    for m in sorted(real.get("t0", set()) - claimed_t0):
        bad.append(f"T0 RUNS {m} AND THE MANIFEST DOES NOT SAY SO — T0's selectors are `{shown['t0']}`")

    # **The check this file existed for eight days without having.** A target in
    # the tree, in a bucket, with a reason — and selected by no stage's argv. It
    # compiles, it is counted by `min_binaries`, it appears in the ledger as
    # covered, and no gate has ever executed it. A test that cannot go red is
    # worth nothing and a test nothing runs is the limiting case of that.
    everywhere = set().union(*real.values()) if real else set()
    for m in sorted(actual & set(declared)):
        if m in everywhere or runs_in_of.get(m) == "retired":
            continue
        bad.append(
            f"RUNS NOWHERE {m} — bucket {declared[m]!r} says runs_in="
            f"{runs_in_of.get(m)!r} and no stage's selector reaches it. Wire it "
            "into the stage it belongs in, or move it to a bucket with "
            "runs_in=\"retired\" and say there why it is not worth running. A "
            "target no stage runs is a check that cannot fail."
        )

    # And the reverse of that, which is the same fault read the other way: the
    # ledger says a target was retired and a gate is still running it. That is
    # not harmless — it is a live check whose failures nobody has agreed to own,
    # and a `retired` entry that lies is how the next reader concludes the
    # ledger cannot be trusted about the ones that do not lie.
    for m in sorted(m for m, r in runs_in_of.items() if r == "retired"):
        hit = sorted(s for s in real if m in real[s])
        if hit:
            bad.append(
                f"RETIRED BUT RUN {m} — bucket {declared[m]!r} retires it and "
                f"{', '.join(s.upper() for s in hit)} still selects it"
            )

    bad += assertion_floor(targets, manifest, real, everywhere, cap)
    return bad


# ---------------------------------------------------------------------------
# THE ASSERTION FLOOR — the refusal this file did not have until 2026-08-16
#
# `RUNS NOWHERE` closed "a target no stage executes". This closes the one
# behind it: **a target every stage executes and none of them could have
# failed**. They are the same defect a step apart, and the second one is
# harder to see because the gate log is identical to a healthy one — `test
# result: ok` over nothing.
#
# The measured state when this was written: 20 of 110 targets, 9 of them
# inside T0's own bucket, so T0 declared 22 checks and 13 could go red.


def compile_only_entries(manifest: dict) -> tuple[dict[str, str], list[str]]:
    """The declared compile-only set, and any complaints about the declaration.

    # WHY THIS CATEGORY IS NARROW, AND WHAT WOULD HAPPEN IF IT WERE NOT

    A target that proves something type-checks and executes nothing is a real
    and legitimate thing: `dig_throughput` drives the whole production dig
    path — frame decode, tick lock, `player_will_destroy`, `Level.setBlock`,
    the neighbour pass, the acknowledgement — and is `#[ignore]`d because its
    number only means anything beside a control tree's. A gate that compiles
    it is checking that the instrument still type-checks against the API it
    measures, which is worth something. So the category exists.

    **And a category that can absorb whatever is left over would be worse
    than not counting at all**, because it would look closed. So membership
    has a criterion this script enforces rather than a paragraph it trusts:

      * the target must hold at least one `#[test]`, and
      * every one of them must carry `#[ignore]`.

    Something is there, it is deliberately switched off, and the switch is an
    attribute somebody wrote in the source with a reason string beside it. A
    target with **no test at all** cannot qualify — that is the leftover case,
    and it is exactly the nine lib/bin targets this refusal was written for.
    A **doctest** target cannot qualify either, in either direction: an
    ` ```ignore ` fence is not compiled, so "it still type-checks" is not a
    claim it supports.
    """
    out: dict[str, str] = {}
    bad: list[str] = []
    for e in manifest.get("compile_only", []):
        name = e.get("target")
        if not name:
            bad.append("a [[compile_only]] entry names no target")
            continue
        if not e.get("reason", "").strip():
            bad.append(
                f"COMPILE-ONLY WITHOUT A REASON {name} — this category is the one "
                "place a target is allowed to be unable to fail, so the argument "
                "for it is the whole entry"
            )
        if name in out:
            bad.append(f"{name} is declared compile-only twice")
        out[name] = e.get("reason", "")
    return out, bad


def assertion_floor(
    targets: list[dict],
    manifest: dict,
    real: dict[str, set[str]],
    everywhere: set[str],
    cap: dict[str, dict] | None,
) -> list[str]:
    if cap is None:
        return []
    bad: list[str] = []
    compile_only, bad_decl = compile_only_entries(manifest)
    bad += bad_decl
    actual = {key(t["package"], t["kind"], t["name"]) for t in targets}
    kind_of = {key(t["package"], t["kind"], t["name"]): t["kind"] for t in targets}

    for name in sorted(set(compile_only) - actual):
        bad.append(f"STALE COMPILE-ONLY ENTRY {name} — no such target exists")

    for name in sorted(set(compile_only) & actual):
        c = cap.get(name, {"runnable": 0, "inert": 0})
        if kind_of[name] == "doc":
            bad.append(
                f"COMPILE-ONLY DOCTEST {name} — a doc target cannot be compile-only. "
                "rustdoc does not compile an `ignore`d fence, so there is nothing "
                "for the category to be true of; give the crate a runnable example "
                "or say why it has none"
            )
            continue
        if c["runnable"] > 0:
            bad.append(
                f"COMPILE-ONLY BUT ASSERTS {name} — it holds {c['runnable']} runnable "
                "test(s), so it can go red and belongs in the count. Take it out of "
                "[[compile_only]]; the floor below will rise, which is the point"
            )
        elif c["inert"] == 0:
            bad.append(
                f"COMPILE-ONLY AND EMPTY {name} — it holds no test at all, ignored or "
                "otherwise, so there is nothing switched off and nothing being kept "
                "type-checked. This category is for an instrument somebody wrote and "
                "deliberately disabled, not for a target that was never written. Give "
                "it an assertion"
            )

    # The refusal itself.
    for name in sorted(actual & everywhere):
        if name in compile_only:
            continue
        c = cap.get(name)
        if c is None:
            bad.append(f"NO CAPABILITY READING FOR {name} — the source walk found nothing to read")
            continue
        if c["why"]:
            bad.append(f"NO CAPABILITY READING FOR {name} — {c['why']}")
            continue
        if c["runnable"] == 0:
            where = ", ".join(s.upper() for s in sorted(real) if name in real[s])
            extra = (
                f" It holds {c['inert']} example(s)/test(s) that are declared not to run."
                if c["inert"]
                else " It holds no test at all."
            )
            bad.append(
                f"EXECUTES NO ASSERTION {name} — {where} runs it and it cannot fail."
                + extra
                + " Give it an assertion, or declare it in [[compile_only]] with the "
                "reason — which is only open to a target whose every `#[test]` carries "
                "`#[ignore]`. `test result: ok` over nothing is the limiting case of a "
                "check that cannot fail."
            )

    # --- the ratchets -------------------------------------------------------
    #
    # A floor and not an exact count, in both cases, because adding a check
    # must never be a red gate. What must not happen silently is the number
    # going DOWN: a `#[ignore]` added to the last test in a target, a test
    # deleted to make a suite green, a doc example demoted to ` ```ignore `.
    spec = manifest.get("assertions", {})
    if not spec:
        return bad + [
            "the manifest has no [assertions] section — the count of targets that "
            "can go red is unratcheted, and this file exists because that count was "
            "20 lower than the count of targets that run"
        ]
    for stage in sorted(real):
        field = f"min_red_capable_{stage}"
        got = sum(1 for m in real[stage] if m not in compile_only and cap.get(m, {}).get("runnable", 0) > 0)
        if field not in spec:
            bad.append(f"[assertions] declares no `{field}` — an absent floor is a floor that cannot fire")
        elif got < int(spec[field]):
            bad.append(
                f"RED-CAPABLE FLOOR {stage.upper()} — {got} of the targets {stage.upper()} "
                f"runs can go red, floor is {spec[field]}. A gate that declares more checks "
                "than it can fail is the defect this ledger exists for; if the drop is "
                "deliberate, lower the floor in the same commit and say why"
            )
    # `blind_to` and `calibrated_on` are the two fields here that describe
    # rather than bound, and a field nothing reads is how this whole class
    # starts. Neither can be fully checked — that is what makes one a blind
    # spot and the other a provenance note — but both can be checked for the
    # ways they go stale, and an unchecked declaration would be the defect
    # this file exists for, written into the file that closes it.
    if not str(spec.get("calibrated_on", "")).strip():
        bad.append(
            "[assertions] has no `calibrated_on` — the red-capable counts are "
            "derived from a source walk, and a derivation with no recorded "
            "agreement against a measured run is a model somebody trusted"
        )
    for name in spec.get("blind_to", []):
        if name not in actual:
            bad.append(f"STALE BLIND-SPOT ENTRY {name} — no such target exists")
            continue
        c = cap.get(name, {})
        if c.get("runnable", 0) == 0:
            bad.append(
                f"BLIND-SPOT ENTRY THAT IS NOT BLIND {name} — it is declared as a "
                "target this instrument counts as red-capable and is not, and it "
                "now holds no runnable test at all. That is the visible case: "
                "either it can go red or EXECUTES NO ASSERTION says so. Drop the "
                "entry"
            )

    if "min_runnable_tests_t0" not in spec:
        bad.append("[assertions] declares no `min_runnable_tests_t0` — a target-level floor cannot see one test of many being switched off")
    else:
        got = sum(cap.get(m, {}).get("runnable", 0) for m in real.get("t0", set()))
        if got < int(spec["min_runnable_tests_t0"]):
            bad.append(
                f"ASSERTION FLOOR T0 — the targets T0 runs hold {got} runnable tests, "
                f"floor is {spec['min_runnable_tests_t0']}. Something was deleted, "
                "`#[ignore]`d or filtered out. A target-level floor cannot see this: a "
                "target with 588 tests and a target with 1 both count as one"
            )
    return bad


# ---------------------------------------------------------------------------
# self-test — fixtures only, no cargo, no tree


def _selftest() -> int:
    T = [
        {"package": "a", "kind": "lib", "name": "a", "test": True},
        {"package": "a", "kind": "doc", "name": "a", "test": True},
        {"package": "a", "kind": "bin", "name": "abin", "test": True},
        {"package": "a", "kind": "test", "name": "atest", "test": True},
    ]
    ok = True

    def check(label, cond):
        nonlocal ok
        if not cond:
            print(f"selftest FAIL: {label}", file=sys.stderr)
            ok = False

    # --- the selector. These four are the whole reason both defects happened.
    check("--lib --bins misses tests/", select(T, "cargo test -q --workspace --lib --bins".split())
          == {"a::lib::a", "a::bin::abin"})
    check("--lib --bins misses doctests", "a::doc::a" not in select(T, "cargo test --lib --bins".split()))
    check("bare selector takes everything", select(T, "cargo test -q --workspace".split())
          == {"a::lib::a", "a::doc::a", "a::bin::abin", "a::test::atest"})
    check("--test names one target", select(T, "cargo test --lib --bins --test atest".split())
          == {"a::lib::a", "a::bin::abin", "a::test::atest"})
    check("--tests reaches tests/ and lib alike", select(T, "cargo test --tests".split())
          == {"a::lib::a", "a::bin::abin", "a::test::atest"})
    # A lib with doctest=false must not be selected by --doc, or a crate with no
    # doc examples would be reported as covered by a flag that runs nothing.
    check("--doc selects only doctest targets", select(T, "cargo test --doc".split()) == {"a::doc::a"})

    # --- the refusals. Each is a way this script could have been confidently wrong.
    for argv, why in [
        ("cargo test --workspace --frobnicate", "unrecognised token"),
        ("cargo test --test nosuch", "names a target that does not exist"),
        ("cargo test --test", "flag with no name"),
    ]:
        try:
            select(T, argv.split())
            check(f"refusal fires: {why}", False)
        except Refusal:
            pass

    # `-p` narrows, and a same-named target in another crate must not be claimed.
    T2 = T + [{"package": "b", "kind": "test", "name": "atest", "test": True}]
    check("-p narrows to one crate", select(T2, "cargo test -q -p a --test atest".split()) == {"a::test::atest"})
    try:
        select(T2, "cargo test -p nosuchcrate --lib".split())
        check("refusal fires: -p names an absent crate", False)
    except Refusal:
        pass

    def rd(text, stage="t0"):
        return read_selectors(text, stage, f"ci/{stage}.sh")

    try:
        rd("cargo test --workspace --lib\n")
        check("refusal fires: missing sentinel", False)
    except Refusal:
        pass
    try:
        rd(f"{SENTINEL}\necho not-a-cargo-line\n")
        check("refusal fires: sentinel points at a non-cargo line", False)
    except Refusal:
        pass
    check("sentinel is read", rd(f"x\n{SENTINEL}\ncargo test --lib\ny\n")
          == [["cargo", "test", "--lib"]])
    # The stage is part of the sentinel. A T1 file under T0's sentinel is a
    # copy-paste, and matching it would file T1's argv under T0.
    check("T1 reads its own sentinel", rd(f"{sentinel('t1')}\ncargo test --workspace\n", "t1")
          == [["cargo", "test", "--workspace"]])
    try:
        rd(f"{SENTINEL}\ncargo test --workspace\n", "t1")
        check("refusal fires: T0's sentinel in the T1 file", False)
    except Refusal:
        pass
    # The env prefix, on one line and continued over two. Both were invisible
    # to the physical-line scan this replaced: the first carried no marker and
    # raised nothing, the second could not be marked without breaking the
    # shell's continuation.
    try:
        rd(f"{SENTINEL}\ncargo test --lib\nRUSTMC_FIXTURES=optional cargo test --workspace\n")
        check("refusal fires: an env-prefixed unmarked cargo test line", False)
    except Refusal:
        pass
    check("an env prefix is stripped, not tripped over",
          rd(f"{SENTINEL}\nFOO=bar cargo test --lib\n") == [["cargo", "test", "--lib"]])
    check("a continued command is one logical line",
          rd(f'{SENTINEL}\nFOO="$PWD/x" \\\n    cargo test --lib\n') == [["cargo", "test", "--lib"]])
    # The unmarked-line refusal. This is the `joint-witness-607` merge rehearsed:
    # an honest sentinel-marked line, plus a second `cargo test` for this
    # workspace that arrived with no marker.
    try:
        rd(f"{SENTINEL}\ncargo test --lib --bins\n"
           "cargo test -p rustmc-server --test session_joint\n")
        check("refusal fires: an unmarked cargo test line", False)
    except Refusal:
        pass
    # A foreign-marked line is claimed, not selected: it must neither refuse nor
    # contribute targets, or the plugins subshell would be read as ours.
    ok_two = rd(f"{SENTINEL}\ncargo test --lib\n{FOREIGN}\n    cargo test -q --workspace || exit 1\n")
    check("foreign marker claims without selecting", ok_two == [["cargo", "test", "--lib"]])
    # Two sentinels union, which is the shape T0 takes the moment one
    # integration test is named explicitly beside the `--lib --bins` line.
    two = rd(f"{SENTINEL}\ncargo test --lib --bins\nx\n{SENTINEL}\ncargo test -p a --test atest\n")
    check("two sentinels are both read", len(two) == 2)
    check("two sentinels union", set().union(*(select(T, a) for a in two))
          == {"a::lib::a", "a::bin::abin", "a::test::atest"})

    # --- adjudication. The asymmetric fixture matters: a symmetric one cannot
    # tell an unaccounted target from a stale entry, and they have opposite fixes.
    good = {
        "meta": {"min_targets": 4},
        "bucket": [
            {"id": "t0", "runs_in": "t0", "reason": "r", "members": ["a::lib::a", "a::bin::abin"]},
            {"id": "rest", "runs_in": "t1", "reason": "r", "members": ["a::doc::a", "a::test::atest"]},
        ],
    }
    argv = {
        "t0": ["cargo test -q --workspace --lib --bins".split()],
        "t1": ["cargo test -q --workspace --no-fail-fast".split()],
    }
    check("a correct manifest is clean", adjudicate(T, good, argv) == [])

    import copy

    m = copy.deepcopy(good)
    m["bucket"][1]["members"].remove("a::test::atest")
    check("an unaccounted target is red", any("UNACCOUNTED" in x for x in adjudicate(T, m, argv)))

    m = copy.deepcopy(good)
    m["bucket"][1]["members"].append("a::test::ghost")
    check("a stale entry is red", any("STALE" in x for x in adjudicate(T, m, argv)))

    # The one that makes this more than bookkeeping: the manifest is honest and
    # complete, and T0's selector was narrowed underneath it.
    narrow_t0 = {"t0": [["cargo", "test", "--lib"]], "t1": argv["t1"]}
    check(
        "narrowing T0 without touching the manifest is red",
        any("MANIFEST SAYS T0 RUNS" in x for x in adjudicate(T, good, narrow_t0)),
    )
    # And the other direction: T0 widened, manifest not updated.
    m = copy.deepcopy(good)
    m["bucket"][0]["members"] = ["a::lib::a", "a::bin::abin"]
    m["bucket"][1]["members"] = ["a::doc::a"]
    m["bucket"].append({"id": "x", "runs_in": "t1", "reason": "r", "members": ["a::test::atest"]})
    wide_t0 = {"t0": [["cargo", "test", "--lib", "--bins", "--tests"]], "t1": argv["t1"]}
    check(
        "widening T0 without touching the manifest is red",
        any("T0 RUNS" in x and "MANIFEST DOES NOT" in x for x in adjudicate(T, m, wide_t0)),
    )

    # --- the T1 half, which did not exist until 2026-08-16 ------------------
    #
    # The manifest is untouched and honest; `ci/t1.sh` was narrowed to T0's own
    # selector underneath it. Before this, that edit took 2 targets out of every
    # gate in the project and the checker printed its usual green line.
    narrow_t1 = {"t0": argv["t0"], "t1": [["cargo", "test", "--lib", "--bins"]]}
    out = adjudicate(T, good, narrow_t1)
    check("narrowing T1 without touching the manifest is red",
          any("MANIFEST SAYS T1 RUNS" in x for x in out))
    check("and the targets it orphaned are named as running nowhere",
          any("RUNS NOWHERE a::test::atest" in x for x in out)
          and any("RUNS NOWHERE a::doc::a" in x for x in out))

    # The brief's exact case: a target is added to the tree and wired to
    # nothing. It is in a bucket — so the UNACCOUNTED rule is satisfied and the
    # author has written a reason — and no stage selects it.
    T3 = T + [{"package": "a", "kind": "test", "name": "orphan", "test": True}]
    m = copy.deepcopy(good)
    m["bucket"].append({"id": "new", "runs_in": "t1", "reason": "r", "members": ["a::test::orphan"]})
    only_named = {
        "t0": argv["t0"],
        "t1": [["cargo", "test", "--lib", "--bins", "--test", "atest", "--doc"]],
    }
    out = adjudicate(T3, m, only_named)
    check("a declared target no stage selects is red", any("RUNS NOWHERE a::test::orphan" in x for x in out))
    check("and it is not merely reported as unaccounted",
          not any("UNACCOUNTED a::test::orphan" in x for x in out))

    # Retiring it is the sanctioned repair, and it must be accepted — a rule
    # with no green form is a rule that only teaches people to route round it.
    m2 = copy.deepcopy(m)
    m2["bucket"][-1]["runs_in"] = "retired"
    check("retiring the orphan is accepted", adjudicate(T3, m2, only_named) == [])
    # And the retirement must be a claim that can itself be wrong: retire a
    # target a stage really does run and the ledger is lying in the other
    # direction.
    m3 = copy.deepcopy(good)
    m3["bucket"][1]["members"].remove("a::test::atest")
    m3["bucket"].append({"id": "gone", "runs_in": "retired", "reason": "r", "members": ["a::test::atest"]})
    check("retiring a target a stage still runs is red",
          any("RETIRED BUT RUN a::test::atest" in x for x in adjudicate(T, m3, argv)))

    m = copy.deepcopy(good)
    m["meta"]["min_targets"] = 99
    check("the floor fires", any("floor 99" in x for x in adjudicate(T, m, argv)))
    m = copy.deepcopy(good)
    del m["meta"]["min_targets"]
    check("a missing floor is red", any("min_targets is missing" in x for x in adjudicate(T, m, argv)))

    m = copy.deepcopy(good)
    m["bucket"][1]["reason"] = "  "
    check("a bucket with no reason is red", any("no reason" in x for x in adjudicate(T, m, argv)))
    m = copy.deepcopy(good)
    m["bucket"].append({"id": "empty", "runs_in": "t1", "reason": "r", "members": []})
    check("an empty bucket is red", any("no members" in x for x in adjudicate(T, m, argv)))
    m = copy.deepcopy(good)
    m["bucket"][1]["members"].append("a::lib::a")
    check("a double declaration is red", any("declared twice" in x for x in adjudicate(T, m, argv)))
    m = copy.deepcopy(good)
    m["bucket"][1]["runs_in"] = "sometimes"
    check("an unknown runs_in is red", any("runs_in=" in x for x in adjudicate(T, m, argv)))

    print("check-test-targets: self-test OK" if ok else "check-test-targets: self-test FAILED")
    return 0 if ok else 1


def main() -> int:
    if "--selftest" in sys.argv:
        return _selftest()
    if "--calibrate" in sys.argv:
        i = sys.argv.index("--calibrate")
        if i + 1 >= len(sys.argv):
            print("--calibrate needs a cargo test log", file=sys.stderr)
            return 2
        return _calibrate(sys.argv[i + 1])
    try:
        targets = targets_from_metadata(read_metadata())
        try:
            manifest = tomllib.loads(MANIFEST.read_text())
        except FileNotFoundError:
            raise Refusal(f"no manifest at {MANIFEST.relative_to(ROOT)}") from None
        except tomllib.TOMLDecodeError as e:
            raise Refusal(f"{MANIFEST.relative_to(ROOT)} is not valid TOML: {e}") from None
        stage_argvs = {}
        for stage, path in STAGE_FILE.items():
            try:
                text = path.read_text()
            except FileNotFoundError:
                raise Refusal(f"no {stage.upper()} script at {path.relative_to(ROOT)}") from None
            stage_argvs[stage] = read_selectors(text, stage, str(path.relative_to(ROOT)))
        cap = capability(targets)
        bad = adjudicate(targets, manifest, stage_argvs, cap)
        real = {s: set().union(*(select(targets, a) for a in v)) for s, v in stage_argvs.items()}
    except Refusal as e:
        print(f"check-test-targets REFUSED — {e}", file=sys.stderr)
        return 2
    if bad:
        for b in bad:
            print(f"check-test-targets: {b}", file=sys.stderr)
        return 1

    # The partition, not a difference against T0. The old line was
    # `N run in T0, M do not`, which named a hole of 88 where the measured hole
    # is 0 and taught its readers to discount it.
    seen: set[str] = set()
    parts = []
    for stage in STAGE_FILE:
        parts.append(f"{len(real[stage] - seen)} first run in {stage.upper()}")
        seen |= real[stage]
    retired = sorted(
        m
        for b in manifest.get("bucket", [])
        if b.get("runs_in") == "retired"
        for m in b.get("members", [])
    )
    # Not a count of "run nowhere": `adjudicate` has already refused if any
    # non-retired target were unselected, so a number printed here could only
    # ever be 0 and would be a statistic that cannot move — this file's own
    # subject. What is printed instead is the claim that number was standing in
    # for, and the retired names, because a retirement should cost its author a
    # line in every gate log until somebody deletes the target.
    compile_only, _ = compile_only_entries(manifest)
    print(
        f"check-test-targets: {len(targets)} targets — "
        + ", ".join(parts)
        + f", {len(retired)} retired; every target is run by a named stage or retired by name"
    )
    for m in retired:
        print(f"check-test-targets:   retired, run by nothing on purpose: {m}")

    # The second line, and it is the one the 2026-08-16 audit was about: how
    # many of the checks each stage declares could actually come back red.
    # Printed per stage rather than in total because the aggregate is what hid
    # this — "1455 assertions" over eighteen targets could not show that eight
    # of them contributed nothing.
    for stage in STAGE_FILE:
        runs = real[stage]
        red = sorted(m for m in runs if m not in compile_only and cap[m]["runnable"] > 0)
        tests = sum(cap[m]["runnable"] for m in runs)
        print(
            f"check-test-targets: {stage.upper()} declares {len(runs)} targets, "
            f"{len(red)} can go red ({tests} runnable tests), "
            f"{len(runs) - len(red)} compile-only by name"
        )
    for m in sorted(compile_only):
        print(f"check-test-targets:   compile-only, executes nothing on purpose: {m}")
    return 0


def _calibrate(log_path: str) -> int:
    """Compare the static model against a measured `cargo test` log.

    # WHY THIS EXISTS

    Everything above derives "can this target go red" by reading source. That
    is a model of what cargo and rustdoc do, and a model of a compiler's
    behaviour is exactly the sort of instrument this project has been
    confidently wrong with. So it is checked against a run: `cargo test
    --workspace --no-fail-fast` prints a `Running .../deps/<name>-<hash>` or
    `Doc-tests <crate>` line and then a `test result:` line for every target,
    and a target the log says executed 0 tests must be a target this model
    says holds 0 runnable ones — in both directions.

        ./ci/check-test-targets.py --calibrate /path/to/suite.log

    It is not in `ci/t0.sh`: T0 compiles nothing and has no log to read. It is
    the evidence behind the numbers in `ci/checks/test-targets.toml`, and it
    is one command to re-take over any T1 log that already exists.
    """
    try:
        targets = targets_from_metadata(read_metadata())
    except Refusal as e:
        print(f"check-test-targets REFUSED — {e}", file=sys.stderr)
        return 2
    cap = capability(targets)
    try:
        lines = Path(log_path).read_text(errors="replace").splitlines()
    except OSError as e:
        print(f"check-test-targets REFUSED — cannot read {log_path}: {e}", file=sys.stderr)
        return 2

    # `Running unittests src/lib.rs (target/debug/deps/rustmc_world-1a2b3c)`
    # and `Doc-tests rustmc-world`. The binary stem carries the target name
    # with `-` normalised to `_`, which is also how `cargo metadata` writes it.
    #
    # **The stem alone is not a key**, and that is not a hypothetical: this
    # workspace's `rustmc-server` has a lib target and a bin target, both of
    # which cargo builds to `deps/rustmc_server-<hash>`. Joining on the stem
    # maps two different result blocks — 588 executed and 0 executed — onto
    # the same pair of names, which is a calibration that cannot tell the
    # covered target from the empty one. The **source path** on the same line
    # is what separates them, so it is what the kind is read from.
    run = re.compile(r"^\s*(Running|Doc-tests)\s+(.*)$")
    dep = re.compile(r"\(target/[^/]+/deps/([A-Za-z0-9_]+)-[0-9a-f]+\)")
    result = re.compile(r"^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored;")

    def kind_from_path(where: str) -> str | None:
        where = where.split("(")[0].strip()
        if where.startswith("unittests "):
            src = where[len("unittests ") :].strip()
            if src.endswith("src/lib.rs"):
                return "lib"
            if src.endswith("src/main.rs") or "/src/bin/" in src or src.startswith("src/bin/"):
                return "bin"
            return None
        if where.startswith("tests/") or "/tests/" in where:
            return "test"
        if where.startswith("benches/") or "/benches/" in where:
            return "bench"
        if where.startswith("examples/") or "/examples/" in where:
            return "example"
        return None

    measured: list[tuple[str, str, int, int]] = []
    pending: tuple[str, str] | None = None
    unreadable: list[str] = []
    for line in lines:
        m = run.match(line)
        if m:
            if m.group(1) == "Doc-tests":
                pending = ("doc", m.group(2).strip().replace("-", "_"))
            else:
                d = dep.search(m.group(2))
                k = kind_from_path(m.group(2))
                if d and k:
                    pending = (k, d.group(1))
                else:
                    # Never a silent skip: a `Running` line this cannot read
                    # would drop a whole target out of the comparison and the
                    # calibration would pass over a hole.
                    pending = None
                    unreadable.append(m.group(2).strip())
            continue
        r = result.match(line)
        if r and pending:
            measured.append((pending[0], pending[1], int(r.group(1)) + int(r.group(2)), int(r.group(3))))
            pending = None

    by_norm: dict[tuple[str, str], list[str]] = {}
    for t in targets:
        norm = t["name"].replace("-", "_")
        by_norm.setdefault((t["kind"], norm), []).append(key(t["package"], t["kind"], t["name"]))

    problems: list[str] = [f"a `Running` line this parser cannot attribute: {u!r}" for u in unreadable]
    seen: set[str] = set()
    for slot, norm, executed, ignored in measured:
        names = by_norm.get((slot, norm), [])
        if len(names) != 1:
            problems.append(f"{slot}/{norm}: the log's target maps to {len(names)} declared targets {names}")
            continue
        name = names[0]
        seen.add(name)
        model = cap[name]["runnable"] > 0
        if model != (executed > 0):
            problems.append(
                f"{name}: the log executed {executed} test(s) and the static model "
                f"says {'it can' if model else 'it cannot'} go red"
            )
    missing = sorted(n for n in (key(t['package'], t['kind'], t['name']) for t in targets) if n not in seen)
    if missing:
        problems.append(f"{len(missing)} target(s) declared and absent from the log: {missing[:6]}")

    if problems:
        print(f"check-test-targets CALIBRATION FAILED — {len(problems)} disagreement(s)", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1
    zero = sorted(n for n in seen if cap[n]["runnable"] == 0)
    print(
        f"check-test-targets: calibrated — {len(measured)} result blocks in {log_path}, "
        f"the static model agrees with every one of them; {len(zero)} execute no assertion"
    )
    for n in zero:
        print(f"check-test-targets:   executes nothing: {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
