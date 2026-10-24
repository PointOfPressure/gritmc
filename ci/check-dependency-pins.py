#!/usr/bin/env python3
"""Refuse a Cargo.lock whose pinned dependency versions have drifted.

# WHY THIS EXISTS

`crates/rustmc-server` depends on `grit-plugin-api` via a PATH dependency on
the sibling `../plugins` checkout (docs/AGENT-BRIEF.md, "THE TREE DOES NOT
BUILD ALONE"). A path dependency still carries a `version` in `Cargo.lock`,
resolved from whatever the sibling's `Cargo.toml` currently says. The sibling
is one shared, mutable checkout every worktree on the box points at, and it
moves on its own schedule — found 2026-08-17 at 0.3.0 while `origin/main`'s
`Cargo.lock` pinned 0.2.1, with no branch having bumped it on purpose. Every
agent builds, so every agent's own `Cargo.lock` silently re-resolves to
whatever the sibling is at that moment; the next one that commits
`Cargo.lock` for any reason carries the drift into main with no review.

This is the same class of thing `ci/checks/required-checks.toml` guards for
check scripts: a manifest names what must hold, a floor makes lowering it a
reviewed diff rather than a silent edit, and the failure names both sides
rather than just going red.

# WHY A RATCHETED PIN AND NOT A PATH PIN ON THE SIBLING

`../plugins` is deliberately not frozen — `ci/t0-remote.sh` ships it fresh
every run for the same reason `data/vanilla` is copied rather than symlinked:
a directory nobody re-checks becomes whichever branch last touched it.
Pinning the checkout itself would fight that design and require the two repos
to be bumped in lockstep by hand. This file makes a real bump a visible,
reviewed diff instead of removing the ability to bump at all.

# NO GIT, NO NETWORK

T0 usually runs on a tree rsynced without `.git` (`ci/setup-merge-drivers.sh`,
`ci/check-conflict-markers.py`). Everything here is decidable from
`Cargo.lock` and the manifest alone.
"""
from __future__ import annotations

import sys
import tomllib

MANIFEST = "ci/checks/dependency-pins.toml"
LOCKFILE = "Cargo.lock"


def load_pins(path: str) -> list[dict]:
    with open(path, "rb") as f:
        doc = tomllib.load(f)
    return doc.get("pin", [])


def load_lock_versions(path: str) -> dict[str, list[str]]:
    """package name -> list of versions present (Cargo.lock allows more than
    one version of the same crate name; a drifted pin must match ONE of
    them, and a package absent entirely is its own refusal, not a pass)."""
    with open(path, "rb") as f:
        doc = tomllib.load(f)
    out: dict[str, list[str]] = {}
    for pkg in doc.get("package", []):
        name = pkg.get("name")
        version = pkg.get("version")
        if name is None or version is None:
            continue
        out.setdefault(name, []).append(version)
    return out


def main() -> int:
    try:
        pins = load_pins(MANIFEST)
    except FileNotFoundError:
        print(f"check-dependency-pins: REFUSED — no manifest at {MANIFEST}", file=sys.stderr)
        return 1
    except tomllib.TOMLDecodeError as e:
        print(f"check-dependency-pins: REFUSED — {MANIFEST} does not parse: {e}", file=sys.stderr)
        return 1

    if not pins:
        print(f"check-dependency-pins: REFUSED — {MANIFEST} declares zero pins", file=sys.stderr)
        return 1

    try:
        lock_versions = load_lock_versions(LOCKFILE)
    except FileNotFoundError:
        print(f"check-dependency-pins: REFUSED — no {LOCKFILE} in this tree", file=sys.stderr)
        return 1
    except tomllib.TOMLDecodeError as e:
        print(f"check-dependency-pins: REFUSED — {LOCKFILE} does not parse: {e}", file=sys.stderr)
        return 1

    failed = 0
    for pin in pins:
        pkg = pin.get("package")
        want = pin.get("version")
        if pkg is None or want is None:
            print(f"check-dependency-pins: REFUSED — a [[pin]] in {MANIFEST} is missing "
                  "'package' or 'version'", file=sys.stderr)
            failed += 1
            continue
        have = lock_versions.get(pkg)
        if have is None:
            print(f"check-dependency-pins: RED — {pkg} is pinned at {want} in {MANIFEST} "
                  f"but is not in {LOCKFILE} at all", file=sys.stderr)
            failed += 1
        elif want not in have:
            have_str = ", ".join(sorted(set(have)))
            print(f"check-dependency-pins: RED — {pkg} drifted: {LOCKFILE} has {have_str}, "
                  f"{MANIFEST} pins {want}. If this bump is intentional, update the pin in the "
                  f"same commit and say why; if not, the sibling ../plugins checkout moved out "
                  f"from under this build and Cargo.lock should not be committed as-is.",
                  file=sys.stderr)
            failed += 1
        else:
            print(f"check-dependency-pins: ok — {pkg} == {want}")

    if failed:
        print(f"check-dependency-pins: {failed} pin(s) drifted", file=sys.stderr)
        return 1
    print(f"check-dependency-pins: {len(pins)} pin(s) match {LOCKFILE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
