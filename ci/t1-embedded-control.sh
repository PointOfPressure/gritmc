#!/usr/bin/env bash
# The positive control for a fixture `include_str!` bakes into the binary.
#
# `ci/t1-force.py` moves or edits a fixture and re-runs the test. That works for
# every golden this repo opens at run time, and it is silently useless for the
# two it embeds at COMPILE time:
#
#   crates/rustmc-server/tests/random_tick_pos.rs   include_str!("../golden/random_tick_pos.json")
#   crates/rustmc-server/tests/entity_move.rs       (same shape; already has a control)
#
# `random_tick_pos` reports `ok. 1 passed` with its golden vector **deleted**,
# because the vector is already inside the executable. A removal-only axis would
# have graded it UNVISITED for ever and recorded the reason as "the binary reads
# no fixture", which is the opposite of true: it reads it at build time. §73.4's
# 70 conversions were all removals, so this class was invisible to them.
#
# So the forcing has to happen before the compiler does: perturb, rebuild, run,
# restore, rebuild. The rebuild is the cost and it is why this is a separate
# script rather than another mode in the sweep.
#
# Usage:  ./ci/t1-embedded-control.sh
# Exit:   0 red when perturbed and green when restored | 1 otherwise
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

BIN=random_tick_pos
CRATE=rustmc-server
TARGET=crates/rustmc-server/golden/random_tick_pos.json
LOG="$ROOT/ci/logs/t1-embedded-$BIN.log"
mkdir -p "$(dirname "$LOG")"

fail() { echo "T1-EMBEDDED $*"; exit 1; }

# Leaf 0 of that file is `draws`, which the vectors are generated FROM rather
# than compared against, so it is perturbed at a leaf the assertion reads.
# `--nth` exists for exactly this: see `_numbers` in ci/t1-force.py.
NTH="${1:-40}"

./ci/t1-force.py apply --mode perturb --target "$TARGET" --nth "$NTH" || exit 2
cargo test -q -p "$CRATE" --test "$BIN" > "$LOG" 2>&1
RED=$?
./ci/t1-force.py restore --target "$TARGET" || exit 2

cargo test -q -p "$CRATE" --test "$BIN" > "$LOG.restored" 2>&1
GREEN=$?

if [ "$RED" -eq 0 ]; then
    echo "--- perturbed run, which should have failed ---"
    grep -E '^test result|^---- ' "$LOG" | head -4 | sed 's/^/      /'
    fail "STAYED GREEN with leaf #$NTH of $TARGET perturbed and the binary rebuilt"
fi
[ "$GREEN" -eq 0 ] || fail "DID NOT COME BACK — green is not restored, tree may be dirty"

echo "T1-EMBEDDED $BIN RED perturbed (leaf #$NTH, rebuilt), GREEN restored"
grep -E '^test result' "$LOG" | head -1 | sed 's/^/      armed:    /'
grep -E '^test result' "$LOG.restored" | head -1 | sed 's/^/      restored: /'
