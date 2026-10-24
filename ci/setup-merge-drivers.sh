#!/usr/bin/env bash
# setup-merge-drivers.sh — register this checkout's custom git merge drivers.
#
# `.gitattributes` names `merge=appendonly` for `docs/DETERMINISM.md`, but an
# attribute naming a driver git does not have silently falls back to the
# default 3-way merge — there is no warning, no error, just the ordinary
# textual collision this driver exists to remove. Git resolves `appendonly`
# through **local, per-checkout config** (`merge.appendonly.driver`), which
# `.gitattributes` cannot carry itself because it is a tracked file and the
# driver command is a local path. So this step is not optional scaffolding;
# without it the attribute is decoration.
#
# **Idempotent, safe to run on every T0 — but T0 usually runs somewhere this
# script cannot configure anything.** `ci/t0-remote.sh` rsyncs with
# `--exclude '.git'`, on purpose (see that script's header): the box only
# ever needs a build, never a merge. So `ci/t0.sh` — called both by
# `ci/t0-remote.sh --run` and by `ci/handoff.sh`'s stage 4 — runs in a plain
# tree with no `.git` at all for every gate this project actually runs.
# Refusing there would make T0 red everywhere, for a driver that was never
# going to be exercised in that directory anyway; measured directly on
# 2026-08-11, first version of this script did exactly that.
#
# So there are two paths:
#
# * **A real git checkout** (an agent's own worktree, or the canonical
#   checkout `ci/handoff.sh` drives) — configure `merge.appendonly.driver`
#   and verify it through `git check-attr`, which is the only way to ask git
#   what it would actually do rather than trust that the write landed.
#   Refuses loudly on any drift.
# * **No `.git`** (the rsynced remote build tree) — merges structurally
#   cannot happen here, so there is nothing to register. What's left worth
#   checking needs no git: that `.gitattributes` still carries the literal
#   line, and that the driver script is still present and syntactically
#   valid Python. Both are refusals too — a rebase that lost the
#   `.gitattributes` line, or a syntax error in the driver, are real defects
#   this copy of the tree can still see even without `git check-attr`.
#
# **The path that actually matters is called every time an agent preps their
# box**, not merely when `ci/t0.sh` happens to run inside a git checkout:
# `ci/t0-remote.sh` calls this script locally, against the real worktree,
# before it ships anything — see that script's header.
#
#     ./ci/setup-merge-drivers.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DET_LINE='docs/DETERMINISM.md merge=appendonly'

# Every path that must resolve to `docregions`, and the region-carrying file
# behind it. `--check` on these is the T0-visible half: a rebase that dropped a
# MERGE-REGION marker leaves a file whose attribute promises a driver behaviour
# the file no longer declares, and the driver's own answer to that is to resolve
# NOTHING and fall back to git's default — correct, silent, and exactly the
# shape of defect this project keeps paying for. So it is a refusal here.
REGION_FILES='docs/WORKLIST.md docs/PARITY-ROADMAP.md ci/figures.toml ci/checks/gameplay-falsifiable.toml ci/checks/survival-falsifiable.toml ci/checks/gameplay-baseline.toml ci/t0.sh docs/FINDINGS.md ci/checks/test-targets.toml'

# THIS LIST AND `.gitattributes` ARE TWO HAND-MAINTAINED COPIES OF ONE FACT,
# AND NOTHING USED TO CHECK THEY AGREED — 2026-08-18.
#
# `.gitattributes` decides which files the driver RUNS on; $REGION_FILES decides
# which files get `--check`ed for a lost MERGE-REGION marker. Add a seventh
# `merge=docregions` line and forget this variable and the new file is driven
# but never checked — so a lost marker in it is silent, which is precisely the
# state `--check` exists to make loud. The reverse drift is quieter still: a
# name here that `.gitattributes` no longer carries makes this script assert a
# registration that is not in force.
#
# Derive and compare rather than replacing the literal, because the literal is
# what a reader needs to see and the whole point is that the two must match.
ATTR_DOCREGIONS="$(sed -n 's/^\([^# ][^ ]*\)[[:space:]]\+merge=docregions[[:space:]]*$/\1/p' \
                   .gitattributes 2>/dev/null | sort | tr '\n' ' ')"
LIST_DOCREGIONS="$(printf '%s\n' $REGION_FILES | sort | tr '\n' ' ')"
if [ -n "$ATTR_DOCREGIONS" ] && [ "$ATTR_DOCREGIONS" != "$LIST_DOCREGIONS" ]; then
    echo "setup-merge-drivers RED — .gitattributes and \$REGION_FILES disagree about which" >&2
    echo "  files carry merge=docregions. A file in one and not the other is either driven" >&2
    echo "  without its markers being checked, or checked without the driver applying." >&2
    echo "  .gitattributes: $ATTR_DOCREGIONS" >&2
    echo "  REGION_FILES:   $LIST_DOCREGIONS" >&2
    exit 1
fi

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git config merge.appendonly.name \
        "keep the common base, then keep both sides' new tail, each in its own order"
    git config merge.appendonly.driver "python3 ci/merge-appendonly.py %O %A %B"
    git config merge.docregions.name \
        "resolve an add/add hunk only inside a declared MERGE-REGION; conflict everywhere else"
    git config merge.docregions.driver "python3 ci/merge-docregions.py %O %A %B %P"

    DRIVER="$(git config --get merge.appendonly.driver || true)"
    if [ -z "$DRIVER" ]; then
        echo "setup-merge-drivers RED — merge.appendonly.driver did not take; git config write failed" >&2
        exit 1
    fi
    RDRIVER="$(git config --get merge.docregions.driver || true)"
    if [ -z "$RDRIVER" ]; then
        echo "setup-merge-drivers RED — merge.docregions.driver did not take; git config write failed" >&2
        exit 1
    fi

    ATTR="$(git check-attr merge -- docs/DETERMINISM.md | awk -F': ' '{print $NF}')"
    if [ "$ATTR" != "appendonly" ]; then
        echo "setup-merge-drivers RED — docs/DETERMINISM.md resolves merge attribute" \
             "'$ATTR', not 'appendonly'. .gitattributes has drifted or this is not" \
             "run from a tree that carries it." >&2
        exit 1
    fi

    # The prohibition this replaces said WORKLIST must carry NO merge attribute.
    # It now must carry exactly `docregions`, and `union`/`appendonly` are still
    # refused by name — the reason they were refused has not changed, and a
    # future edit reaching for either should hit this line rather than a comment.
    for f in $REGION_FILES; do
        A="$(git check-attr merge -- "$f" | awk -F': ' '{print $NF}')"
        if [ "$A" != "docregions" ]; then
            echo "setup-merge-drivers RED — $f resolves merge attribute '$A', not" \
                 "'docregions'. 'union' would keep both sides of a REWRITTEN section" \
                 "— the defect that put two contradictory figures in WORKLIST's cold" \
                 "start index — and 'appendonly' would refuse every merge, because no" \
                 "side of these files is ever a byte-prefix extension of the base." >&2
            exit 1
        fi
    done

    python3 ci/merge-docregions.py --check $REGION_FILES || {
        echo "setup-merge-drivers RED — a file carrying merge=docregions no longer declares" \
             "the MERGE-REGION it promises. See ci/merge-docregions.py's header." >&2
        exit 1
    }

    echo "setup-merge-drivers: merge.appendonly -> $DRIVER (docs/DETERMINISM.md confirmed via git check-attr)"
    echo "setup-merge-drivers: merge.docregions -> $RDRIVER ($REGION_FILES confirmed via git check-attr)"
    exit 0
fi

# --- no .git here: the static fallback --------------------------------------
if [ ! -f .gitattributes ] || ! grep -qxF "$DET_LINE" .gitattributes; then
    echo "setup-merge-drivers RED — .gitattributes is missing '$DET_LINE'" >&2
    echo "                          (checked by text, not git check-attr: no .git in this tree)" >&2
    exit 1
fi
for f in $REGION_FILES; do
    if ! grep -qxF "$f merge=docregions" .gitattributes; then
        echo "setup-merge-drivers RED — .gitattributes is missing '$f merge=docregions'" >&2
        echo "                          (checked by text, not git check-attr: no .git in this tree)" >&2
        exit 1
    fi
done
for s in ci/merge-appendonly.py ci/merge-docregions.py; do
    if [ ! -f "$s" ]; then
        echo "setup-merge-drivers RED — .gitattributes names a driver but $s is missing" >&2
        exit 1
    fi
    if ! python3 -c "import sys; compile(open(sys.argv[1]).read(), sys.argv[1], 'exec')" "$s" \
            2>/tmp/setup-merge-drivers.$$.err; then
        echo "setup-merge-drivers RED — $s does not parse:" >&2
        cat /tmp/setup-merge-drivers.$$.err >&2
        rm -f /tmp/setup-merge-drivers.$$.err
        exit 1
    fi
done
rm -f /tmp/setup-merge-drivers.$$.err

# The marker check needs no git and no merge, so unlike `git check-attr` it is
# not skipped here: it is the half of this script that still has teeth in an
# rsynced build copy.
python3 ci/merge-docregions.py --check $REGION_FILES || {
    echo "setup-merge-drivers RED — a file carrying merge=docregions no longer declares" \
         "the MERGE-REGION it promises. See ci/merge-docregions.py's header." >&2
    exit 1
}

echo "setup-merge-drivers: SKIPPED git config/check-attr — no .git in this tree (expected for a" \
     "ci/t0-remote.sh build copy). Confirmed by text instead: .gitattributes carries '$DET_LINE'" \
     "and a merge=docregions line for each of $REGION_FILES, both drivers parse, and every" \
     "declared MERGE-REGION is well formed. The git-level registration is verified where it can" \
     "actually apply — see ci/t0-remote.sh, which calls this script against the real worktree" \
     "before it ships anything."
