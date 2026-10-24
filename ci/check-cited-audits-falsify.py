#!/usr/bin/env python3
"""Prove `ci/check-cited-audits.py` can go red.

    python3 ci/check-cited-audits-falsify.py     # ~1 s, no network, no lock

A citation checker that cannot fail is worse than no citation checker: it puts
a verification stamp on a number nobody verified. The three ways a citation
stops holding are injected here and the expected message is asserted by name.

The green control is the live file, unmutated, so the harness also fails if the
real citations have drifted — which is the case this exists for.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHECKER = ROOT / "ci" / "check-cited-audits.py"
LIVE = ROOT / "ci" / "checks" / "cited-audits.toml"


def emit(rows: list[dict]) -> str:
    out = []
    for r in rows:
        out.append("[[cited]]")
        for k, v in r.items():
            out.append(f"{k} = {v!r}".replace("'", '"') if isinstance(v, str)
                       else f"{k} = {v}")
        out.append("")
    return "\n".join(out)


BOX_ENV = ROOT / "ci" / "lib" / "box_env.py"


def _plant_checker(tree: Path) -> "Path":
    """Copy the checker into `<tree>/ci`, WITH the module it imports.

    `ci/check-cited-audits.py` reads its object store through
    `ci/lib/box_env.py`. A stand-in tree that carries the checker and not the
    module gives `ModuleNotFoundError`, the checker exits non-zero for a reason
    that has nothing to do with the citation, and every injected fault below is
    reported as having gone red for the right marker — which it did not.

    That is the same trap `ci/check-figures-falsify.py` fell into on this
    branch, one file over, and it is why the fixture names the files it needs
    instead of copying `ci/` wholesale: a new dependency has to be added here on
    purpose, and forgetting costs a loud red on the same run rather than a quiet
    pass later.
    """
    (tree / "ci" / "lib").mkdir(parents=True, exist_ok=True)
    checker = tree / "ci" / "check-cited-audits.py"
    checker.write_text(CHECKER.read_text(), encoding="utf-8")
    (tree / "ci" / "lib" / "box_env.py").write_text(
        BOX_ENV.read_text(), encoding="utf-8")
    return checker


def run(path: Path, checker: Path | None = None) -> tuple[int, str]:
    """Run the checker, keeping fixture skips out of the gate's skip ledger.

    # A FALSIFIER'S INJECTED BEHAVIOUR IS NOT EVIDENCE ABOUT THE TREE

    The SKIPPED case below plants the checker in a directory that is
    deliberately not a repository, because proving it stands down there is the
    point. `ci/t0.sh` exports `RUSTMC_BOX_SKIP_LOG` and every child appends to
    it, so that intentional skip landed in the gate's real ledger and
    `ci/box-skip-audit.py` reported

        FORBIDDEN SKIP  check-cited-audits — this environment can run it and it
        stood down anyway (no git object store in /tmp/cited-norepo.ar4ggt0m)

    turning a green T0 red over a temp path. The audit was right and its input
    was wrong.

    `checker` is the discriminator and it is exactly the right one: it is set
    only when the caller planted a copy in a fixture tree. The green control
    passes no `checker`, runs the real one against the real tree, and its skip
    IS evidence — so that one still reaches the gate's ledger.

    `ci/box-env-falsify.sh` had the same fault and the same fix. The rule is
    general: a falsifier that makes a check skip on purpose owns that skip's
    ledger.
    """
    env = dict(os.environ)
    if checker is not None:
        env["RUSTMC_BOX_SKIP_LOG"] = str(Path(tempfile.gettempdir())
                                         / f"cited-falsify-fixture.{os.getpid()}.log")
    p = subprocess.run([sys.executable, str(checker or CHECKER), str(path)],
                       capture_output=True, text=True, env=env)
    return p.returncode, p.stdout + p.stderr


def build_stand_in_repo(rows: list[dict], where: Path) -> tuple[Path, str]:
    """A throwaway repository holding the cited paths, and its one commit sha.

    THE THREE FAULT CASES NEED A REPOSITORY AND THE GATE DIRECTORY HAS NONE.
    2026-08-18. `ci/t0-remote.sh` rsyncs a working tree to the build box
    without `.git`, so `check-cited-audits.py` skips there — and a harness that
    injects a fault and asserts a red then watched all three injections go
    green, correctly, because the checker it was testing had switched itself
    off. Reporting that as a harness failure was right and fixing it by
    loosening the assertion would have been the wrong repair: it would have
    left the ONE file whose job is proving the checker can fail unable to prove
    it anywhere the checker actually runs in anger.

    So the harness stops borrowing the ambient repository and builds its own.
    Every cited `path` is written with its `witness` repeated `witness_count`
    times and committed; the rows are re-pointed at that commit. UNREACHABLE,
    DRIFTED-by-witness and DRIFTED-by-count are then all reachable on a box
    with no `.git`, on the laptop, and in CI, with no dependence on which
    branches happen to exist.

    The green control below is deliberately NOT moved onto this repo: its whole
    job is to read the real citations against the real tree, and a stand-in
    would make it assert nothing.
    """
    env = ["-c", "user.email=falsify@localhost", "-c", "user.name=falsify"]
    subprocess.run(["git", "init", "-q", str(where)], check=True,
                   capture_output=True)
    for r in rows:
        f = where / r["path"]
        f.parent.mkdir(parents=True, exist_ok=True)
        # `>= want` is what the checker tests for presence and `== want` for a
        # counted row, so writing exactly `want` copies satisfies both and
        # leaves the count mutation something to break.
        f.write_text((r["witness"] + "\n") * r.get("witness_count", 1),
                     encoding="utf-8")
    subprocess.run(["git", "-C", str(where), "add", "-A"], check=True,
                   capture_output=True)
    subprocess.run(["git", "-C", str(where), *env, "commit", "-q", "-m",
                    "stand-in for the cited blobs"], check=True,
                   capture_output=True)
    sha = subprocess.run(["git", "-C", str(where), "rev-parse", "HEAD"],
                         check=True, capture_output=True,
                         text=True).stdout.strip()
    checker = _plant_checker(where)
    return checker, sha


def main() -> int:
    rows = tomllib.loads(LIVE.read_text()).get("cited", [])
    if not rows:
        print("check-cited-audits-falsify: no live citations to mutate",
              file=sys.stderr)
        return 2
    work = Path(tempfile.mkdtemp(prefix="cited-falsify."))
    passed = failed = 0

    stand_in = Path(tempfile.mkdtemp(prefix="cited-standin."))
    stand_checker, stand_sha = build_stand_in_repo(rows, stand_in)

    def case(name: str, marker: str, mutate) -> None:
        nonlocal passed, failed
        # Re-pointed at the stand-in commit BEFORE the mutation, so a case that
        # overwrites `sha` still overwrites it and the other two get a sha that
        # resolves wherever this runs.
        muts = [dict(r, sha=stand_sha) for r in rows]
        mutate(muts)
        f = work / "c.toml"
        f.write_text(emit(muts), encoding="utf-8")
        code, out = run(f, stand_checker)
        if code == 1 and marker in out:
            passed += 1
            print(f"  ok    {marker:<22} {name}")
        else:
            failed += 1
            print(f"  FAIL  {marker:<22} {name}\n        exit {code}\n{out}")

    # A branch force-pushed or pruned. The ledger then cites nothing, which is
    # strictly worse than citing something stale: nothing points at it.
    case("a citation whose sha is no longer in this repository",
         "CITATION UNREACHABLE",
         lambda m: m[0].__setitem__("sha", "0123456789abcdef0123456789abcdef01234567"))

    # The finding moved on its own branch and this tree still publishes the old
    # number. This is the case the whole file exists for.
    case("a witness that no longer appears in the cited blob",
         "CITATION DRIFTED",
         lambda m: m[0].__setitem__("witness", "a sentence nobody ever wrote"))

    # A count is a different claim from a presence, and `projection-drops` is
    # counted rather than matched: 16 `[[drop]]` tables, not "at least one".
    case("a counted witness whose count moved",
         "CITATION DRIFTED",
         lambda m: m[0].__setitem__("witness_count", 99))

    # THE SKIP PATH IS AN ESCAPE HATCH UNLESS IT IS ALSO FALSIFIED. 2026-08-18.
    #
    # `check-cited-audits.py` returns 0 with a SKIPPED line when its tree is not
    # inside a git repository, because an rsynced gate directory cannot resolve
    # any sha and reporting "four branches were pruned" there is a red that says
    # nothing about any branch. A skip that could fire anywhere else would be a
    # way to make every citation stop being checked, so it is pinned from both
    # sides: it MUST fire outside a repository, and the case above proves it
    # does NOT fire inside one — the same unreachable sha still reports
    # CITATION UNREACHABLE and exits 1.
    #
    # The copy is deliberately laid out as `<tmp>/ci/check-cited-audits.py` so
    # the checker's own `ROOT = parent.parent` lands on a directory that is not
    # a repository, which is exactly the shape of a gate directory on the box.
    norepo = Path(tempfile.mkdtemp(prefix="cited-norepo."))
    _plant_checker(norepo)
    bogus = [dict(r) for r in rows]
    bogus[0]["sha"] = "0123456789abcdef0123456789abcdef01234567"
    f = norepo / "c.toml"
    f.write_text(emit(bogus), encoding="utf-8")
    # Through `run()`, so the planted checker's deliberate stand-down goes to a
    # fixture ledger rather than the gate's. A second `subprocess.run` here is
    # how the leak survived the first repair: the discriminator lives in
    # `run()` and a call site that does not use it does not get it.
    code, out = run(f, norepo / "ci" / "check-cited-audits.py")
    # `BOX-SKIP` as well as `SKIPPED`. Two lanes repaired the checker in
    # parallel: `f82b2ebb` printed a `SKIPPED` line, and this branch routes the
    # stand-down through `ci/lib/box_env.py`, whose marker is the one
    # `ci/box-skip-audit.py` reads. Asserting only the older string would fail
    # this case on a checker that is skipping correctly and more loudly than
    # before — a harness reporting a repair as a regression.
    if code == 0 and ("SKIPPED" in out or "BOX-SKIP" in out) \
            and "UNREACHABLE" not in out:
        passed += 1
        print(f"  ok    {'SKIPPED':<22} a tree with no repository skips loudly "
              f"rather than reporting every branch pruned")
    else:
        failed += 1
        print(f"  FAIL  {'SKIPPED':<22} exit {code}, expected a loud "
              f"skip outside a repository\n{out}")

    # AN EMPTY LEDGER IS A REFUSAL, NOT A PASS (ported from vacuous-green-audit
    # 8cf8f84d6). Before it, `cited = []` exited 0: every citation could be
    # de-registered by one bad merge and this check would stay green.
    f = work / "empty.toml"
    f.write_text("# every [[cited]] row removed\n", encoding="utf-8")
    code, out = run(f, stand_checker)
    if code == 2:
        passed += 1
        print(f"  ok    {'EMPTY LEDGER':<22} a ledger with no citations refuses at 2")
    else:
        failed += 1
        print(f"  FAIL  {'EMPTY LEDGER':<22} exit {code}, expected 2 on an empty ledger\n{out}")

    # The green control is the LIVE file. If this fails, a real citation has
    # drifted and that is the finding, not a harness fault.
    code, out = run(LIVE)
    # `BOX-SKIP` as well as `SKIPPED`: two lanes repaired the checker in
    # parallel and it now stands down through `ci/lib/box_env.py`, whose marker
    # is the one `ci/box-skip-audit.py` counts. Matching only the older string
    # would make this control count a stand-down as a pass, which is precisely
    # the vacuous green the branch below exists to refuse.
    if code == 0 and ("SKIPPED" in out or "BOX-SKIP" in out):
        # Not counted as a pass. On a gate directory there is no repository, so
        # the control asserts nothing and saying "ok" would be the vacuous green
        # this harness exists to prevent. The four injected cases above still
        # run here, because they build their own repository.
        print(f"  --    {'(green control)':<22} NOT RUN — no repository in this "
              f"tree, so the live citations were not read")
        sys.path.insert(0, str(ROOT / "ci" / "lib"))
        import box_env
        box_env.skip(
            "check-cited-audits-falsify:green-control",
            f"no git object store in {ROOT} — the live ledger's citations were "
            f"not re-verified. The injected cases ran against a repository this "
            f"harness builds itself and are unaffected.",
        )
    elif code == 0:
        passed += 1
        print(f"  ok    {'(green control)':<22} every live citation still holds")
    else:
        failed += 1
        print(f"  FAIL  {'(green control)':<22} a LIVE citation has drifted\n{out}")

    print(f"\ncheck-cited-audits-falsify: {passed} passed, {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
