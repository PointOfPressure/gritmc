#!/usr/bin/env python3
"""Refuse a comment that claims a guard the code does not have.

# ===== AGENT INTERFACE =====
#
#     ci/check-comment-guards.py            refuse on any new instance
#     ci/check-comment-guards.py --emit     print the TOML block a tree implies
#     ci/check-comment-guards.py --explain  say why each declared instance stands
#
# Reads `ci/checks/comment-guards.toml`, a ratchet in the shape of
# `t1-known-red.toml`: a count that a branch may lower and may not raise
# without saying so in the file.
# ===== end interface =====
"""

# # THE CLASS, AND WHY NOTHING CAUGHT IT
#
# `docs/PARITY-ROADMAP.md` §73.8 tabulates four comments in this repo that
# named a guard the code did not have:
#
#   state_spec::extract_or_skip   "# Panics: If the extract is missing"  returns None
#   entity_move.rs                "ci/t0.sh greps for this sentinel"     it cannot match
#   session_joint.rs              'grep -c "skipping: no vanilla" must be 0'  always 0
#   probe_placement_vs_servers    shipwreck_is_the_control               asserts nothing
#
# and recorded, as the finding: **"None was found by a check. All four were
# found by reading."** A comment is the one artefact in this project that
# nothing is run against. It is also the artefact a reader trusts most, because
# it is the only one that says what the code is *for* — so a false one is worse
# than no comment, and it survives every gate by construction.
#
# The three rules below are the mechanically checkable core of that class. Each
# is deliberately narrow: it fires on a claim whose *subject* is checkable from
# the tree, and stays silent on prose. A rule that tried to read intent would
# produce a refusal nobody could act on, which is how a check gets deleted.
#
# # WHY THE RULES ARE PRODUCER-SIDE
#
# §73.2 is the reason rule 2 is written the way it is. The guard it removed
# greps for `skipping: no vanilla`, a pattern "copied from the pattern rather
# than from the producer, so it can only agree with itself" — it matched 27 of
# 119 skip sites and 0 of 0 live shell emitters. So rule 2 does not ask whether
# a sentinel looks plausible. It enumerates the **producers** of the string and
# reports the fraction a grepper could see, and a sentinel with zero reachable
# producers is a refusal whatever the comment says about it.
#
# # THIS CHECK CAN FAIL, AND THAT IS DEMONSTRATED
#
# A checker for unfalsifiable claims that is itself unfalsifiable would be the
# joke writing itself. `ci/comment-guards-falsify.sh` re-introduces all four
# instances above, one at a time, into a scratch copy of the tree, and requires
# this script to refuse each one by name and to pass with them removed. That
# script is the positive control and it is what makes this file evidence.

import argparse
import importlib.util
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "ci" / "checks" / "comment-guards.toml"

# Macros that can actually make a test fail. `.expect()` and `.unwrap()` are
# deliberately NOT here: they are how this repo opens a file, not how it checks
# a result, and counting them would let `shipwreck_is_the_control` — which
# `expect`s three registry loads and then prints — read as a control.
ASSERT = re.compile(r"\b(assert|assert_eq|assert_ne|debug_assert|panic|unreachable|todo|unimplemented)\s*!")

# A test whose NAME makes a claim about being a check. Narrow on purpose: these
# are words that assert the test's own power, not its subject.
CLAIMS_TO_CHECK = re.compile(
    r"(^|_)(control|falsif\w*|guard|sentinel|refus\w*|cannot|must|proves?|catches)($|_)"
)


def rust_files():
    for p in sorted((ROOT / "crates").rglob("*.rs")):
        if "/target/" not in str(p):
            yield p
    for p in sorted((ROOT / "tools").rglob("*.rs")):
        if "/target/" not in str(p):
            yield p
    for p in sorted((ROOT / "plugins").rglob("*.rs")):
        if "/target/" not in str(p):
            yield p


# Braces inside a comment or a string literal are not structure, and neither
# comments nor strings in Rust stop at a line ending. Counting them line by
# line made `move_entity`'s body run 110 lines past its own closing brace and
# pick up the `assert!`s of the `#[cfg(test)]` module below it, reporting a
# function as panicking when it does not; blanking only `"..."` then broke the
# other way, because a `r#"{"table": ...}"#` fixture has quotes inside it and
# the naive strip ate its braces unevenly. Twelve tests that plainly assert
# were reported as asserting nothing. A checker that invents evidence is worse
# than no checker, so this blanks non-code over the WHOLE FILE, once, keeping
# every byte's position — line numbers stay exact and the brace count is over
# code alone.


def blank_noncode(text):
    """`text` with comments, strings and char literals replaced by spaces.

    Length- and newline-preserving, so an offset into the result is the same
    offset in the original. Handles `//`, `/* */`, `"..."`, `r"..."` and
    `r#"..."#` at any hash depth. A lifetime (`'a`) is left alone: only a
    quote pair that closes like a char literal is blanked.
    """
    out = list(text)
    n = len(text)
    i = 0

    def wipe(a, b):
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        nxt = text[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            wipe(i, j)
            i = j
        elif c == "/" and nxt == "*":
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            wipe(i, j)
            i = j
        elif c == "r" and nxt in '#"':
            k = i + 1
            hashes = 0
            while k < n and text[k] == "#":
                hashes += 1
                k += 1
            if k < n and text[k] == '"':
                term = '"' + "#" * hashes
                j = text.find(term, k + 1)
                j = n if j < 0 else j + len(term)
                wipe(i, j)
                i = j
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            wipe(i, j)
            i = j
        elif c == "'":
            # `'a'` or `'\n'` is a literal; `'a` alone is a lifetime.
            if i + 2 < n and text[i + 1] == "\\":
                j = text.find("'", i + 2)
                j = i + 1 if j < 0 else j + 1
            elif i + 2 < n and text[i + 2] == "'":
                j = i + 3
            else:
                i += 1
                continue
            wipe(i, j)
            i = j
        else:
            i += 1
    return "".join(out)


def body_after(lines, i, code=None):
    """The braced body of the item starting at or after line i.

    Brace-counting runs over `code` — the blanked-out twin of `lines` — while
    the text returned is the original, so `assert!` inside a string is not
    structure but `assert!` in code is still found. Not a parser; it is exact
    for anything rustfmt will produce.
    """
    src = code if code is not None else lines
    depth = 0
    started = False
    end = len(lines)
    for k in range(i, len(lines)):
        ln = src[k]
        depth += ln.count("{") - ln.count("}")
        if "{" in ln:
            started = True
        if started and depth <= 0:
            end = k + 1
            break
    return "\n".join(lines[i:end])


# --------------------------------------------------------------------------
# rule 1 — a documented panic the body cannot produce
# --------------------------------------------------------------------------
# Catches `state_spec::extract_or_skip` in its original form: a `# Panics`
# section over a body whose every path returns. Clippy has `missing_panics_doc`
# for the opposite direction and nothing for this one, which is why a doc
# section that outlived its code sat in the tree for a fortnight.

# A workspace-wide index of which functions can panic, built once and resolved
# ONE level deep. Deeper would be more faithful and much less useful: in a tree
# this size nearly every function reaches a panicking one eventually, so a
# fully transitive rule reports nothing and is the silent-check defect again.
# One level is the depth at which the rule still fires on real instances, and
# a genuine second-level panic is a false positive that gets adjudicated into
# `[allowed]` with its reason — the same contract `t1-known-red.toml` uses.
_PANIC_INDEX = None

# Constructs that panic where they stand. `unwrap_or_else` and `unwrap_or` are
# deliberately absent: they are how a caller AVOIDS the panic, and `tick_lock`
# — whose doc claims a poisoning panic over
# `.unwrap_or_else(PoisonError::into_inner)` — is found precisely because of
# that distinction.
PANICKY = re.compile(
    r"\b(assert|assert_eq|assert_ne|debug_assert|panic|unreachable|todo|unimplemented)\s*!"
    r"|\.unwrap\(\)"
    r"|\.expect\("
    r"|\.expect_err\("
    r"|fixture::available"
)


def _fn_bodies():
    """(name -> [body]) for every fn defined in the workspace."""
    out = {}
    for p in rust_files():
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        lines = text.splitlines()
        code = blank_noncode(text).splitlines()
        for i, ln in enumerate(code):
            m = re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([a-z0-9_]+)", ln)
            if m:
                out.setdefault(m.group(1), []).append(body_after(lines, i, code))
    return out


def panic_index():
    global _PANIC_INDEX
    if _PANIC_INDEX is not None:
        return _PANIC_INDEX
    bodies = _fn_bodies()
    direct = {n for n, bs in bodies.items() if any(PANICKY.search(b) for b in bs)}
    _PANIC_INDEX = (bodies, direct)
    return _PANIC_INDEX


def can_panic(body, level="any"):
    """Does this body panic? `direct` = in these lines; `any` = or via a callee.

    **The two rules below need opposite conservatisms and this is where that
    is spent.** Callees resolve by NAME across the workspace, because there is
    no type resolution here — so `new(` matches every constructor in the tree
    and a handful of those panic. That is fine for suppressing a report and
    useless for making one: it flagged `move_entity`, whose doc says
    `# Panics / Never`, on the strength of some unrelated `new`.

    So `# Panics` with nothing found suppresses on `any` — the loosest reading,
    which reports only when no path at all was found — and `# Panics / Never`
    fires on `direct` — the strictest, which reports only what is visible in
    the function itself. Both directions err towards silence, which for a
    ratchet is the side that keeps it honest.
    """
    if PANICKY.search(body):
        return True
    if level == "direct":
        return False
    _bodies, direct = panic_index()
    for m in re.finditer(r"\b([a-z0-9_]{3,})\s*\(", body):
        if m.group(1) in direct:
            return True
    return False


def rule_documented_panic(path, lines, code):
    """A `# Panics` section that disagrees with the body, in EITHER direction.

    The two directions are separate findings and both are real. `# Panics`
    over a total function is a contract the caller defends against for
    nothing; `# Panics / Never` over a body that can panic is a contract the
    caller RELIES on, and is the more dangerous of the two.
    """
    hits = []
    for i, ln in enumerate(lines):
        if not re.match(r"\s*///\s*#\s*Panics\s*$", ln):
            continue
        # the prose of the section, then the item it documents
        j = i + 1
        prose = []
        while j < len(lines) and lines[j].lstrip().startswith("///"):
            prose.append(lines[j].lstrip()[3:].strip())
            j += 1
        while j < len(lines) and lines[j].lstrip().startswith("#["):
            j += 1
        if j >= len(lines):
            continue
        body = body_after(lines, j, code)
        sig = lines[j].strip()
        where = f"{path.relative_to(ROOT)}:{i + 1}"
        text = " ".join(prose).strip()

        # `# Panics / Never; ...` is the claim that it CANNOT, so the finding
        # is the mirror image. Written as its own branch rather than folded in,
        # because reporting "documents a panic it does not have" over a comment
        # that says "Never" would be the check making the same class of false
        # claim it exists to refuse.
        # `Never.` and `Never;` are the absolute claim. `Never for a status in
        # ChunkStatus::ALL` is a QUALIFIED one — the body is supposed to panic
        # outside that set — and neither direction of this rule can adjudicate
        # it, so it is left alone rather than guessed at.
        if re.match(r"(?i)never\s*[.;,]", text) or re.fullmatch(r"(?i)never", text):
            if can_panic(body, level="direct"):
                hits.append(
                    (
                        "DOCUMENTED-NEVER-PANICS-BUT-CAN",
                        where,
                        f"`# Panics / {text[:40]}` over `{sig[:60]}` whose body can panic",
                    )
                )
            continue

        if can_panic(body):
            continue
        hits.append(
            (
                "DOCUMENTED-PANIC-WITHOUT-PANIC",
                where,
                f"`# Panics` over `{sig[:60]}` whose body has no panicking path",
            )
        )
    return hits


# --------------------------------------------------------------------------
# rule 2 — a grep pattern nothing reachable produces
# --------------------------------------------------------------------------
# §73.2's finding, generalised and pointed at the producers instead of the
# prose. The guard `ci/t1-gate.sh` removed grepped a log for `skipping: no
# vanilla` under a comment calling it "the whole point" of the stage, and it
# could not fire for two independent reasons: `cargo test` never prints a
# PASSING test's output, so the Rust `eprintln!`s that wrote those bytes were
# captured and discarded; and the pattern was copied from one example rather
# than from the producers, so it matched 27 of 119 skip sites and — among live
# shell emitters, the only kind a log grep can see — 0 of 0.
#
# The rule therefore does not read the comment at all. It takes every literal
# pattern a script in `ci/` greps for and asks **which producers of that string
# could reach the log being grepped**:
#
#   shell `echo`/`printf`      reaches it. this is the only unconditional one
#   python `print()`           reaches it
#   Rust `panic!`/`assert!`    reaches it — cargo prints a FAILING test's output
#   Rust `println!`/`eprintln!` in a test  does NOT, without `--nocapture`
#
# A pattern with no producer in the first three classes is a guard that cannot
# fire, whatever its comment says, and `ci/handoff.sh` still holds one.
#
# Patterns are compared as substrings in both directions: `ci/t0.sh` greps
# `skipping: no vanilla` and the sites emit `skipping: no vanilla extract`, so
# neither is equal to the other and the shorter matching the longer is exactly
# what a grep does.

GREP_LITERAL = re.compile(r"""grep\s+(?:-[a-zA-Z]+\s+)*["']([^"'$]{8,60})["']""")

# Patterns that are regex machinery rather than a sentinel a producer writes.
NOT_A_SENTINEL = re.compile(r"[|\\\[\]()*+?^]|\.\*")


def _slurp(globs):
    out = {}
    for g in globs:
        for p in sorted(ROOT.glob(g)):
            if "/target/" in str(p):
                continue
            try:
                out[p] = p.read_text(encoding="utf-8", errors="replace")
            except OSError:
                pass
    return out


_CORPUS = None
_BLANK_CACHE = {}


def _blanked(path, text):
    """`blank_noncode` memoised. Without this the producer scan re-tokenised
    every Rust file once per grep pattern — 40 patterns over 400 files, and the
    check went from 14 s to past two minutes."""
    hit = _BLANK_CACHE.get(path)
    if hit is None:
        hit = blank_noncode(text)
        _BLANK_CACHE[path] = hit
    return hit


_spec = importlib.util.spec_from_file_location("shellstrip", ROOT / "ci" / "shellstrip.py")
shellstrip = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(shellstrip)


def corpus():
    global _CORPUS
    if _CORPUS is None:
        _CORPUS = {
            # **HEREDOC BODIES ARE BLANKED, 2026-08-19, and it is not
            # tidiness.** `ci/comment-guards-falsify.sh` arm 4 writes a
            # stand-in emitter into a temp tree with
            #
            #     cat > "$WORK/tree/ci/zz-falsify-emit.sh" <<'INNER'
            #     echo "zzfalsify sentinel absent"
            #     INNER
            #
            # and the day that falsifier was wired into `ci/t0.sh`,
            # `script_is_invoked` started returning True for it, so that
            # FIXTURE counted as a live shell emitter of the sentinel in the
            # real tree. Arm 3 — whose whole job is to prove this rule fires
            # when the only producer is a captured Rust print — silently
            # stopped being able to go red. A check switched off by another
            # check citing it, which is this file's own defect class committed
            # against this file.
            #
            # A string inside a heredoc is a fixture being written, not a byte
            # being emitted. Comments and string literals are left alone: shell
            # comments are already skipped line-wise where it matters, and in
            # shell a string is a place code runs.
            "sh": {p: shellstrip.blank(t, comments=False, heredocs=True, strings=False)
                   for p, t in _slurp(["ci/*.sh", "tools/**/*.sh", "swarm/*.sh"]).items()},
            "py": _slurp(["ci/*.py", "tools/**/*.py"]),
            "rs": _slurp(["crates/**/*.rs", "tools/**/*.rs", "plugins/**/*.rs"]),
        }
    return _CORPUS


SHELL_EMIT = re.compile(r"(?:^|[;&|]|\bthen\b|\bdo\b|\{)\s*(?:echo|printf)\b")

# A grep whose target names a source file is not a grep of a log, and cargo's
# output capture has nothing to do with it. See the use site for the instance
# that found this.
SOURCE_TARGET = re.compile(r"\.(rs|py|sh|toml|json|md|lock|yaml|yml)\b[\"']?\s*(?:[;)]|$|\|\||&&|2?>)")

# The gate entry points. Everything else has to be called by something to be
# able to write into a log anyone greps.
ENTRY_POINTS = {
    "t0.sh", "t1.sh", "t1-gate.sh", "t2.sh", "handoff.sh",
    "t2-gameplay.sh", "t2-gameplay-selfcheck.sh", "t0-remote.sh",
}

_INVOKED = {}


def script_is_invoked(path):
    """Is this script called by anything, or is it an entry point?

    **An `echo` in a script nothing runs cannot reach a log.** §73.2 measured
    exactly this and wrote the result as "matched its producers in 0 of 0":
    the tree's only two live shell emitters of `skipping: no vanilla`,
    `ci/village-pieces.sh` and `ci/village-oracle.sh`, **are called by
    nothing**. Counting them as producers made this rule report the merge
    gate's dead guard as healthy, which is the guard-believed-to-cover-more
    failure one level up.
    """
    name = path.name
    if name in ENTRY_POINTS:
        return True
    hit = _INVOKED.get(name)
    if hit is not None:
        return hit
    needle = re.compile(r"(?<![\w/-])(?:\./)?(?:ci/|swarm/|tools/[\w-]+/)?" + re.escape(name))
    found = False
    c = corpus()
    for kind in ("sh", "py"):
        for q, text in c[kind].items():
            if q == path:
                continue
            for ln in text.splitlines():
                st = ln.strip()
                if st.startswith("#"):
                    continue
                mt = needle.search(ln)
                if not mt:
                    continue
                # A backticked name is a CITATION, not a call. Every mention of
                # `ci/village-pieces.sh` in this tree is one — including the
                # paragraph in this very file explaining that nothing calls it,
                # which is what made the script look alive and the merge gate's
                # dead guard look healthy. A docstring is not a comment line, so
                # `startswith("#")` never saw it.
                before = ln[: mt.start()].rstrip()
                after = ln[mt.end():].lstrip()
                if before.endswith("`") or after.startswith("`"):
                    continue
                # **AND A NAME BEING PRINTED IS NOT A NAME BEING RUN, 2026-08-19.**
                # The backtick rule above catches a citation in prose. It does
                # not catch the shape that actually disabled a checker: an
                # advice message. `print("./ci/comment-guards-falsify.sh")`
                # inside a "run this next" string made this function report the
                # falsifier as live, so the rule that depends on it went quiet
                # — a checker switched off by its own advice.
                #
                # Live in this tree at the moment of writing: `ci/t0.sh` calls
                # this falsifier on one line and, on the next, prints
                # `echo "T0 RED — ci/comment-guards-falsify.sh: ..."`. The
                # first is a call, the second is a message, and this function
                # counted both. Where the real call to be removed, the message
                # alone would have kept the script looking alive.
                #
                # Scoped to the COMMAND SEGMENT, not the line: `echo running;
                # ./ci/x.sh` is a call, and treating the whole line as a
                # display would lose it.
                seg = re.split(r"[;|]|&&|\|\|", ln[: mt.start()])[-1]
                if EMIT_HEAD.search(seg):
                    continue
                found = True
                break
            if found:
                break
        if found:
            break
    _INVOKED[name] = found
    return found
# The head of a command whose arguments are DISPLAYED rather than executed.
EMIT_HEAD = re.compile(r"(?:^|\s)(?:echo|printf|print|warn|die|note|say)\s*\(?\s*[\"']?")

PY_EMIT = re.compile(r"\bprint\s*\(")
RS_FAIL_EMIT = re.compile(r"\b(panic|assert|assert_eq|assert_ne|unreachable|expect|todo)\b")
RS_PASS_EMIT = re.compile(r"\b(println|eprintln)\s*!")


def producers(pattern):
    """(reachable, captured) counts of lines that write `pattern`."""
    c = corpus()
    reachable = 0
    captured = 0
    for _p, text in c["sh"].items():
        if not script_is_invoked(_p):
            continue
        for ln in text.splitlines():
            st = ln.strip()
            if st.startswith("#") or pattern not in ln:
                continue
            if SHELL_EMIT.search(ln):
                reachable += 1
    for _p, text in c["py"].items():
        for ln in text.splitlines():
            st = ln.strip()
            if st.startswith("#") or pattern not in ln:
                continue
            if PY_EMIT.search(ln):
                reachable += 1
    for _p, text in c["rs"].items():
        code = _blanked(_p, text)
        for orig, blank in zip(text.splitlines(), code.splitlines()):
            if pattern not in orig:
                continue
            # a literal only counts where it IS a literal: blanked out in the
            # code twin means it was inside a string, which is what we want.
            if pattern in blank:
                continue  # it survived blanking, so it was not a string
            if RS_FAIL_EMIT.search(orig):
                reachable += 1
            elif RS_PASS_EMIT.search(orig):
                captured += 1
    return reachable, captured


def rule_unreachable_grep(path, lines, code):
    """Runs once, over `ci/`, not per Rust file."""
    return []


def scan_grep_patterns():
    hits = []
    seen = set()
    for p, text in sorted(corpus()["sh"].items()):
        for i, ln in enumerate(text.splitlines()):
            st = ln.strip()
            if st.startswith("#"):
                continue
            for m in GREP_LITERAL.finditer(ln):
                pat = m.group(1).strip()
                if not pat or NOT_A_SENTINEL.search(pat):
                    continue
                key = pat
                if key in seen:
                    continue
                seen.add(key)
                reachable, captured = producers(pat)
                if reachable:
                    continue
                # **`captured > 0` is the whole narrowing, and the first draft
                # of this rule did not have it.** Without it the rule reported
                # 21 patterns, and all but one were greps of something this
                # repo does not write: `FAILED TO BIND TO PORT` comes out of
                # the vanilla Java server, `REBASE CONFLICT` out of git,
                # `GAME_VERSION: &str =` is a grep of SOURCE and not of a log
                # at all. Refusing those would be a check copied from one
                # example and pointed at everything — §73.2's exact defect,
                # committed by the check written to find it.
                #
                # A pattern with a Rust print and no reachable producer is a
                # different thing entirely: the bytes are written by OUR code,
                # somebody greps for them, and `cargo test` throws them away
                # because a test that prints and returns early passes. That is
                # the §73.2 shape and nothing else in the tree matches it.
                if not captured:
                    continue
                # **AND THE TARGET HAS TO BE A LOG, 2026-08-19.** The second
                # narrowing, found the same way as the first: by this rule
                # reporting something. `ci/plugin-hook-cost.sh:70` is
                #
                #     grep -q 'plugins:' "$WORK/.../src/world_send.rs"
                #
                # a grep of SOURCE, asserting that a control worktree does not
                # already carry the hook. `plugins:` does have a Rust print
                # somewhere in the tree, so `captured` is non-zero and the
                # first narrowing passes it straight through — but no log is
                # involved and cargo's capture is irrelevant. Reporting it
                # would be the same defect as the 21 patterns above: a check
                # copied from one example and pointed at everything.
                #
                # A grep of a file this repo can name at edit time is a grep of
                # source. A grep of a log is a grep of a path the run creates.
                if SOURCE_TARGET.search(ln):
                    continue
                hits.append(
                    (
                        "GREP-PATTERN-ONLY-CAPTURED-PRODUCER",
                        f"{p.relative_to(ROOT)}:{i + 1}",
                        f"{pat!r}: {captured} Rust print(s) write it and 0 producers a "
                        f"grepped log can carry — cargo discards a passing test's output",
                    )
                )
    return hits


# --------------------------------------------------------------------------
# rule 3 — a test whose name claims a power its body does not have
# --------------------------------------------------------------------------
# Catches `shipwreck_is_the_control`. §73.7 counted it as one of only two
# binaries carrying an in-file control and then had to write "and only one of
# them is real".

def rule_toothless_claim(path, lines, code):
    hits = []
    for i, ln in enumerate(lines):
        m = re.match(r"\s*(?:async\s+)?fn\s+([a-z0-9_]+)\s*\(", ln)
        if not m:
            continue
        name = m.group(1)
        if not CLAIMS_TO_CHECK.search(name):
            continue
        # must be a test, and #[should_panic] is itself an assertion
        attrs = "\n".join(lines[max(0, i - 6):i])
        if "#[test]" not in attrs:
            continue
        if "should_panic" in attrs:
            continue
        body = body_after(lines, i, code)
        if ASSERT.search(body):
            continue
        hits.append(
            (
                "CLAIM-WITHOUT-ASSERTION",
                f"{path.relative_to(ROOT)}:{i + 1}",
                f"`{name}` names itself a check and its body asserts nothing",
            )
        )
    return hits


RULES = [rule_documented_panic, rule_toothless_claim]


def scan():
    hits = []
    for p in rust_files():
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        lines = text.splitlines()
        code = blank_noncode(text).splitlines()
        for rule in RULES:
            hits.extend(rule(p, lines, code))
    hits.extend(scan_grep_patterns())
    return sorted(hits)


def declared():
    """Parse the ratchet without a TOML dependency: `total = N` and the
    `[allowed]` keys."""
    if not LEDGER.exists():
        return 0, set()
    total = 0
    allowed = set()
    in_allowed = False
    for ln in LEDGER.read_text().splitlines():
        s = ln.strip()
        if s.startswith("#") or not s:
            continue
        if s == "[allowed]":
            in_allowed = True
            continue
        if s.startswith("["):
            in_allowed = False
            continue
        m = re.match(r'total\s*=\s*(\d+)', s)
        if m and not in_allowed:
            total = int(m.group(1))
        if in_allowed:
            m = re.match(r'"([^"]+)"\s*=\s*', s)
            if m:
                allowed.add(m.group(1))
    return total, allowed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--emit", action="store_true")
    ap.add_argument("--explain", action="store_true")
    args = ap.parse_args()

    hits = scan()

    if args.emit:
        print("total = %d\n" % len(hits))
        print("[allowed]")
        for kind, where, why in hits:
            print(f'"{where}" = "{kind}: {why}"')
        return 0

    total, allowed = declared()
    found = {h[1] for h in hits}
    new = [h for h in hits if h[1] not in allowed]

    if args.explain:
        for kind, where, why in hits:
            mark = "declared" if where in allowed else "NEW"
            print(f"{mark:9} {kind:32} {where}\n          {why}")
        stale = allowed - found
        for s in sorted(stale):
            print(f"{'STALE':9} declared and not found: {s}")
        return 0

    rc = 0
    for kind, where, why in new:
        print(f"COMMENT-GUARD {kind} {where}")
        print(f"              {why}")
        rc = 1
    stale = sorted(allowed - found)
    for s in stale:
        # A declared instance that stopped being found is as red as a new one:
        # same rule as `t1-known-red.toml`. Either it was fixed — lower the
        # ledger in that commit — or the rule stopped seeing it.
        print(f"COMMENT-GUARD STALE-DECLARATION {s}")
        print("              declared in ci/checks/comment-guards.toml and not found.")
        print("              If it was fixed, remove the line and lower `total` here.")
        rc = 1
    if rc == 0:
        print(f"COMMENT-GUARDS OK — {len(hits)} declared, 0 new, 0 stale")
    return rc


if __name__ == "__main__":
    sys.exit(main())
