#!/usr/bin/env bash
# t0-remote.sh — run ci/t0.sh on the Threadripper against the *working tree*.
#
# The laptop is the user's machine. Nothing compiles or runs here: not the
# gate, not a test, not a one-off `cargo run`. This rsyncs the current working
# tree — uncommitted edits included, which is the point, since the gate has to
# pass *before* the commit — and runs the gate there.
#
# Exit code is the gate's, so the usual shape still works:
#
#     ./ci/t0-remote.sh > /dev/null 2>&1; T=$?
#
# `oracle-work` and `target` are excluded: the remote already has its own
# vanilla extract and its own build cache, and shipping either would cost
# minutes per run for no gain.
#
# **The exclude patterns must not end in a slash.** `oracle-work/` matches
# only *directories*, and in a `git worktree` — which is how a second agent
# gates without shipping the first one's half-written saves — `oracle-work`
# and `data/vanilla` are **symlinks** into the main checkout. rsync then ships
# the symlink, replaces the remote's real directory with it, and because the
# remote's home has the same path as the laptop's, the link points at itself.
# Every worktree on the box symlinks to the one canonical extract, so that
# one slash takes out the vanilla data for every agent at once. It has done
# so, and restoring it cost 200 MB over the wire and a re-extract of the
# structure templates.
#
# # THE TWO HALVES, AND WHY THEY CAN NOW BE ASKED FOR SEPARATELY (2026-08-06)
#
# Shipping the tree and running T0 are one call here because for an agent's
# inner loop they always happen together. For the **merge gate** they must not:
# `ci/handoff.sh` has to generate this branch's own `data/vanilla` tables in
# between them, and welding the two together left it no seam to do that in.
#
# The generators live in the branch's own tree, so they cannot run before the
# rsync; T0's unit tests read the tables they produce, so they cannot run
# after T0. That is the entire ordering constraint, and it has exactly one
# solution: rsync, generate, T0. See the stage comments in `ci/handoff.sh`
# for the defect this closes.
#
# **The no-argument behaviour is unchanged, byte for byte.** Every agent inner
# loop in `docs/AGENT-BRIEF.md` calls this bare and must keep working. An
# unrecognised argument is an error rather than a silent `all`, because a
# caller that means `--sync` and is quietly given `--sync` *plus a T0 run* gets
# a gate whose stages ran in an order nobody chose.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOST="${RUSTMC_REMOTE:-threadripper}"
# `rustmc` is the shared tree: every agent's `oracle-work` entries are symlinked
# out of it and every arm's `data/vanilla` is copied from it. It is 2.3 GB of
# vanilla references that cost hours of wall time to regenerate, and it is NOT a
# git checkout, so nothing in it is recoverable by `git checkout -- .`.
#
# It used to be the default here. On 2026-08-10 an agent called this with
# `RUSTMC_REMOTE_DIR` unexported — one missing `export` in a compound command —
# and rsynced its own branch over the shared tree's source. The exclusions saved
# it: `oracle-work` and `data/vanilla` were untouched and nothing reads that
# tree's source, so nothing was lost. That was luck about which directories the
# rsync happened to cover, not a property of the script.
#
# So an unset variable is now a refusal. Targeting the shared tree deliberately
# still works — set it explicitly — because the refusal is about the *silent*
# default, not about the destination.
. "$(dirname "$0")/lib/require-remote-dir.sh"
DEST="$RUSTMC_REMOTE_DIR"

MODE=all
case "${1:-}" in
    "")      MODE=all ;;
    --sync)  MODE=sync ;;   # ship the tree; run nothing
    --run)   MODE=run ;;    # run T0 on what is already there; ship nothing
    *)       echo "usage: $0 [--sync|--run]" >&2; exit 2 ;;
esac

# THE SIBLING PLUGIN CHECKOUT. 2026-08-06.
#
# `crates/rustmc-server` depends on `grit-plugin-api`, and that crate lives in
# the `grit-plugins` repo beside this one rather than in here — see the note in
# the workspace `Cargo.toml` for why it is a path and not a copy. So this tree
# does not build on its own, and a box that has the tree but not the sibling
# fails with a Cargo error about a missing manifest that says nothing about
# what to do.
#
# It is shipped here rather than left as a thing somebody sets up once,
# deliberately, and the reason is written on `data/vanilla` in
# `docs/AGENT-BRIEF.md`: a shared directory nobody re-checks is how a gate
# ends up measuring whichever branch last touched it. Every tree on the box
# rsyncs the same laptop-side checkout to the same place, so the content is
# identical from every direction and `--delete` removes nothing anyone else is
# using. It lands OUTSIDE $DEST because the path dependency is `../plugins`
# relative to the workspace root, and $DEST is the workspace root.
#
# `GRIT_PLUGINS_DIR` overrides the location for a checkout that is not a
# sibling. A missing one is an error and never a skip: a skip here produces a
# build failure two steps later that reads as a broken branch.
PLUGINS_DIR="${GRIT_PLUGINS_DIR:-$ROOT/../plugins}"

# # `rsync -a` PRESERVES MTIMES AND CARGO DECIDES FRESHNESS BY MTIME. 2026-08-07.
#
# **This is how a green T0 can be a T0 that compiled nothing.** `target/` is
# excluded from both rsyncs, so a gate directory keeps the previous run's build
# cache; `-a` implies `-t`, so a file lands on the box carrying the mtime it had
# on the laptop. Cargo compares each crate's source mtimes against the timestamp
# of the last build's fingerprint: a source that arrives **older** than that
# build is `Fresh`, and `cargo check`, `cargo clippy -D warnings` and
# `cargo test` all report success on a file no compiler has ever read. The
# binary `ci/t2-gameplay.sh` then runs is the previous tree's.
#
# Measured 2026-08-07, on a two-crate replica of this workspace on the box:
#
#     step 1  good tree, shipped and touched, warm target/     -> T0 GREEN
#     step 2  replace a function body with a call to a
#             function that does not exist; set that file's
#             mtime to 2026-08-01
#     step 3  rsync -az --delete --exclude target  (this file,
#             before this change)                              -> shipped, mtime preserved
#     step 4  fmt/clippy/check/test                            -> rc 0 0 0 0, T0 GREEN
#
# It is not a contrived ordering. Every branch whose files were last written
# before the gate directory's most recent build ships them with mtimes older
# than that build, and `git checkout` only refreshes the mtimes of files whose
# content it actually changes. This is the mechanism behind the two occasions
# this project has watched cargo replay cached diagnostics or run a stale
# binary.
#
# The fix is to touch **only what rsync actually transferred**. Touching the
# whole tree would force a full rebuild on every sync; touching the transferred
# set costs exactly what a correct build costs, because those are the files
# whose content changed. `-i` itemises: an 11-character code, a space, then the
# name, and `<f`/`>f` are transferred regular files (`cd` is a directory,
# `*deleting` a removal). The fixed-width `sed` keeps names with spaces intact.
#
# Residual, stated so it is not read as more than it is: a file whose content
# changed while its size and mtime did not is not transferred at all, so it is
# not touched either. `--checksum` would close that and costs a full read of
# 32 MB per call in an agent's inner loop; every edit and every `git checkout`
# moves the mtime, so the quick check is not the hole. The hole was the
# destination keeping a mtime older than a build that had already happened.
ship() {
    local src="$1" dest="$2"; shift 2
    local list rc
    list="$(mktemp)" || return 2
    rsync -azi --delete "$@" "$src" "$HOST:$dest" >"$list"
    rc=$?
    if [ "$rc" -ne 0 ]; then rm -f "$list"; return "$rc"; fi
    sed -n 's/^[<>]f.\{9\} //p' "$list" >"$list.names"
    if [ -s "$list.names" ]; then
        ssh "$HOST" "cd $dest && tr '\\n' '\\0' | xargs -0r touch --" <"$list.names"
        rc=$?
    fi
    rm -f "$list" "$list.names"
    return "$rc"
}

if [ "$MODE" != run ]; then
    # **This is the one place the append-only merge driver can actually be
    # registered.** `ci/t0.sh` calls `ci/setup-merge-drivers.sh` too, for
    # when it happens to run in a real checkout, but every path that
    # actually reaches `ci/t0.sh` in this project ships to a `.git`-less
    # copy first (`--exclude '.git'` a few lines down) — so a `git config`
    # write there has nothing to write to. `$ROOT` here is still the real
    # local worktree, before anything is excluded, which is the only
    # correct place for the git-level registration to happen. This call is
    # a refusal, not a courtesy: a misconfigured driver on `main` is worse
    # than the collision it exists to prevent, because nobody would know to
    # look for it.
    (cd "$ROOT" && ./ci/setup-merge-drivers.sh) || {
        echo "t0-remote: ci/setup-merge-drivers.sh refused in the local worktree $ROOT" >&2
        exit 2
    }

    # **Rule 18 has to be answered HERE, because this is the last line in the
    # pipeline that can still see a commit.** `ci/check-figures.py` rule 18 says
    # a `provenance` must name a commit git resolves; `--exclude .git` a few
    # lines below means the gate cannot ask, so until 2026-09-05 the rule
    # degraded to shape and every gate passed it by not being able to check.
    # Measured on main 1a31e0a56, the identical tree: with history, exit 1 and
    # three `PROVENANCE-NO-COMMIT` reds; rsynced without `.git`, `FIGURES GREEN`
    # and exit 0.
    #
    # So the answer is computed where the answer exists and travels with the
    # tree, exactly as `/.rp-commit` has done since 2026-08-06. Shipping `.git`
    # was the other candidate and is 165 MB — and it would leave every gate
    # directory with a HEAD disagreeing with its own working tree, which is the
    # property `--exclude .git` was chosen for.
    #
    # **Exit 1 is shipped, not refused.** A figure whose provenance resolves
    # nowhere is a ledger fault and belongs in the gate's own log, named, in
    # rule 18's own words — not in a sync that stops before the branch has been
    # looked at. Exit 2 is a refusal because it means no repository or no write,
    # and a stamp that was never written would make every figure red in the gate
    # for a reason that is about this script.
    (cd "$ROOT" && python3 ./ci/figure-provenance.py --snapshot); rc=$?
    if [ "$rc" -ge 2 ]; then
        echo "t0-remote: could not stamp .figure-provenance in $ROOT (exit $rc)" >&2
        echo "           the gate reads it in place of the .git this sync omits;" >&2
        echo "           without it ci/check-figures.py rule 18 reds every figure." >&2
        exit 2
    fi
    if [ ! -f "$PLUGINS_DIR/crates/grit-plugin-api/Cargo.toml" ]; then
        echo "t0-remote: no grit-plugins checkout at $PLUGINS_DIR" >&2
        echo "           this tree needs it to build (crates/rustmc-server -> grit-plugin-api)." >&2
        echo "           git clone https://github.com/PointOfPressure/grit-plugins.git \\" >&2
        echo "               \"$ROOT/../plugins\"   # or set GRIT_PLUGINS_DIR" >&2
        exit 2
    fi
    ship "$PLUGINS_DIR/" "plugins/" --exclude 'target' || {
        echo "t0-remote: rsync of the grit-plugins checkout to $HOST failed" >&2
        exit 2
    }
    ship "$ROOT/" "$DEST/" \
        --exclude 'target' \
        --exclude 'oracle-work' \
        --exclude '.git' \
        --exclude '.gitstore' \
        --exclude 'data/vanilla' || {
        echo "t0-remote: rsync to $HOST failed" >&2
        exit 2
    }
fi

# # THE OBJECT STORE. 2026-08-18.
#
# `--exclude '.git'` a few lines up is correct and is not changing: shipping a
# worktree's `.git` would ship a **pointer file** naming a checkout the box
# does not have, and shipping the common directory would put a repository
# whose HEAD disagrees with the shipped tree under every gate.
#
# But three checks need an object database, and until today they could not get
# one. `ci/check-figures.py` rule 18 resolves a figure's provenance sha,
# `ci/check-cited-audits.py` reads `<sha>:<path>` for each citation, and
# `ci/figure-provenance.py` runs `git log -S` over full history. All three ran
# in a tree with no repository, all three failed for that reason, and
# `ci/check-figures.py` sits at `ci/t0.sh:66` with T0 exiting on first failure
# — so the merge train stopped at 09:12:50 on 2026-08-18 and every branch after
# it reported `t0=1/0` with T1 and T2 never reached.
#
# ## WHY NOT A REPO-FREE DATA PATH
#
# It was the other candidate and it does not survive contact. Each of the three
# asks a question about history: is this sha an object, what did this blob say
# at that sha, which commit first published this claim. Anything a `.git`-less
# tree could carry instead is the check's own previous answer written down, and
# a check that verifies its own cached output is precisely the unfalsifiable
# class this project has found twenty-one times. The history is the evidence; a
# summary of it is a second copy of the verdict.
#
# ## WHAT IS SHIPPED, AND WHAT IT COSTS
#
# Not a copy of `.git` per gate directory. Measured on this box, 2026-08-18:
#
#     rsync -az .git over ssh, cold            612 MB    6.7 s
#     rsync -az .git over ssh, incremental               0.7 s
#     the same content as a reachable pack      47 MB    3.95 s to build
#     T0 GREEN, for scale                                65.2 min  (N=164)
#     T0 RED, for scale                                  3.4 min
#
# 612 MB is 93% unreachable loose objects — git says so itself on every fetch
# — so a `git gc` on the source would take the cold figure to about 47 MB.
# That is the owner's call on a live repository with ten lanes writing to it,
# not this script's, and the design below is why it does not have to be made:
#
# **one store on the box, and a 125-byte pointer in each gate directory.**
# `$DEST/.gitstore` is a bare repository whose `objects/info/alternates` names
# the shared store. Measured: `cat-file`, `show <sha>:<path>` across branches
# and `log -S` over full history all resolve through it. Thirty gate
# directories cost 612 MB once and 125 bytes each, rather than 18 GB.
#
# Sharing an object store is safe in a way sharing `oracle-work` or
# `data/vanilla` is not, and the difference is worth stating because this
# project has been bitten by the general form. Those are mutable directories
# whose contents mean different things on different branches, so a shared one
# silently answers with whoever wrote last. An object database is
# content-addressed and append-only: an object is named by its own hash, so no
# branch can make another read a wrong answer out of it. The only failure it
# admits is an object that is absent, which is loud.
#
# HEAD is written per gate directory, to the commit that directory is gating,
# so `git log` walks THIS branch rather than whatever the shared repository
# last checked out.
#
# `GRIT_SHIP_GIT_STORE=0` turns it off. It is a refusal and not a skip when the
# local tree IS a repository and the store cannot be built: that is an
# environment which could have answered, and a gate that stands down there is
# the defect this whole change is about.
GIT_STORE_SHARED="${GRIT_GIT_STORE:-grit-gitstore}"
if [ "$MODE" != run ] && [ "${GRIT_SHIP_GIT_STORE:-1}" != 0 ]; then
    COMMON="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"
    if [ -n "$COMMON" ] && [ -d "$COMMON/objects" ]; then
        HEADSHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null)"
        # `--delete` is deliberately absent. An object database only grows, and
        # a delete pass here would race a concurrent gate reading it.
        rsync -az "$COMMON/objects" "$COMMON/refs" "$COMMON/packed-refs" \
              "$HOST:$GIT_STORE_SHARED/" 2>/dev/null || {
            echo "t0-remote: could not ship the git object store to $HOST:$GIT_STORE_SHARED" >&2
            echo "           $ROOT is a repository, so this is a refusal and not a skip:" >&2
            echo "           three T0 checks would stand down in a tree that could have answered." >&2
            exit 2
        }
        ssh -n "$HOST" "
            set -e
            mkdir -p '$GIT_STORE_SHARED' '$DEST/.gitstore/objects/info' '$DEST/.gitstore/refs'
            printf '[core]\n\trepositoryformatversion = 0\n\tbare = true\n' > '$DEST/.gitstore/config'
            echo \"\$HOME/$GIT_STORE_SHARED/objects\" > '$DEST/.gitstore/objects/info/alternates'
            echo '$HEADSHA' > '$DEST/.gitstore/HEAD'
        " || {
            echo "t0-remote: could not point $DEST/.gitstore at the shared store" >&2
            exit 2
        }
    elif [ -e "$ROOT/.git" ]; then
        echo "t0-remote: $ROOT has a .git that git cannot read as a repository" >&2
        echo "           refusing rather than shipping a gate that would stand down." >&2
        exit 2
    fi
fi

if [ "$MODE" = sync ]; then
    exit 0
fi

# ssh forwards no environment, so the two T0 knobs a caller may set are passed
# by value: T0_KEEP_GOING (run every stage, list every red; ci/t0-stages.sh) and
# T0_STAGE_LOG (keep the per-stage timing lines in a file on the box).
KG="${T0_KEEP_GOING:-0}"
case "$KG" in 0|1) ;; *) echo "t0-remote: T0_KEEP_GOING must be 0 or 1, not '$KG'" >&2; exit 2 ;; esac
SL="${T0_STAGE_LOG:-}"
case "$SL" in *[!A-Za-z0-9_./~-]*) echo "t0-remote: T0_STAGE_LOG '$SL' has characters this will not quote" >&2; exit 2 ;; esac
ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; cd $DEST && T0_KEEP_GOING=$KG T0_STAGE_LOG=$SL ./ci/t0.sh"
