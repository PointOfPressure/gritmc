# Sourced guard: refuse a silent default onto the shared tree.
#
# `rustmc` is 2.3 GB of vanilla references that every agent's `oracle-work`
# symlinks point into, and it is not a git checkout — nothing in it comes back
# with `git checkout -- .`. Three scripts defaulted `RUSTMC_REMOTE_DIR` to it.
# On 2026-08-10 one missing `export` in a compound command sent an agent's own
# branch over that tree's source. The rsync exclusions happened to spare
# `oracle-work` and `data/vanilla`, and nothing reads that tree's source, so
# nothing was lost — luck about coverage, not a property of the scripts.
#
# Usage, after `set -u` and before the destination is used:
#
#     . "$(dirname "$0")/lib/require-remote-dir.sh"
#     DEST="$RUSTMC_REMOTE_DIR"
#
# Targeting the shared tree on purpose still works: set the variable to
# `rustmc` explicitly. The refusal is about the silent default, not the
# destination — an unexported variable and a deliberate choice are
# indistinguishable at the point where the damage happens.
if [ -z "${RUSTMC_REMOTE_DIR:-}" ]; then
    cat >&2 <<'REFUSE'
REFUSED  RUSTMC_REMOTE_DIR is unset or empty.

This used to default to ~/rustmc, the shared reference tree. It does not any
more.

    export RUSTMC_REMOTE_DIR=<the name your brief gives you>

Check it with `echo $RUSTMC_REMOTE_DIR` before the first remote call of each
turn — a variable exported in an earlier turn's shell is not set in this one.
If you really do mean the shared tree, say so: RUSTMC_REMOTE_DIR=rustmc
REFUSE
    exit 2
fi
