#!/usr/bin/env bash
# The behaviour axis of T1's positive controls: one arm, one build, one verdict.
#
# `ci/t1-force.py` forces a gate by moving or perturbing the fixture it reads.
# That runs out at the binaries which read no fixture — `golden_rng` carries
# its vectors inline, `plugin_storage` builds its world in memory,
# `golden_chunk_packet`'s round-trip test needs no capture at all. For those the
# only thing that can force a red is a wrong implementation, and
# `rustmc_data::sabotage` is a closed set of them.
#
# Each arm is run in its own invocation. A test binary is already the unit of
# isolation, so unlike the corpus module on `corpus-falsifiability-audit` there
# is nothing to scope by position — but two arms in one run would confound each
# reading with the other exactly as it would there.
#
# **`fired=0` on an armed arm is a verdict, not a warning.** It says the gate
# never reaches the subsystem it claims to cover, so no implementation of it
# could have changed the answer. That is `UNVISITED`, and the
# `T1-SABOTAGE-FIRST` line on stderr is how it is distinguished from a gate
# that ran and did not look.
#
# Usage:  ./ci/t1-sabotage.sh [--build] [arm:binary ...]
#   --build   rebuild with the feature first (needed once per tree)
# Exit:   0 every pair went red | 1 a pair stayed green
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

FEAT="rustmc-data/t1-sabotage"
LOGDIR="$ROOT/ci/logs"
mkdir -p "$LOGDIR"

# The pairs this axis is responsible for. Each is `arm:crate:binary`, and each
# is here because `ci/t1-positive-control.py` could not force that binary by
# fixture — the reason is recorded per binary in
# `ci/checks/t1-positive-controls.toml`.
PAIRS_DEFAULT="
xoroshiro:rustmc-worldgen:golden_rng
legacy-lcg:rustmc-worldgen:golden_rng
palette:rustmc-server:golden_chunk_packet
storage:rustmc-server:plugin_storage
path-accuracy:rustmc-entity:ground_path
path-accuracy:rustmc-entity:navigation
weather-step:rustmc-server:weather_console
"

BUILD=0
if [ "${1:-}" = "--build" ]; then BUILD=1; shift; fi

if [ "$BUILD" -eq 1 ]; then
    echo "== building with --features $FEAT"
    cargo build --workspace --tests --features "$FEAT" -q || exit 2
fi

PAIRS="${*:-}"
[ -n "$PAIRS" ] || PAIRS="$PAIRS_DEFAULT"

PASS=0
FAIL=0
for pair in $PAIRS; do
    arm="${pair%%:*}"
    rest="${pair#*:}"
    crate="${rest%%:*}"
    bin="${rest##*:}"
    log="$LOGDIR/t1-sabotage-$arm-$bin.log"

    # Armed. The feature must be on or the module panics rather than arming
    # nothing — see `rustmc_data::sabotage::armed`.
    #
    # **The firing evidence goes to a FILE, not to the log.** `cargo test`
    # captures a test's output, so the first version of this grepped `$log`
    # for the announcement and reported `fired=0` — this script's spelling of
    # UNVISITED — on an arm that had just turned `golden_rng` from 6 passed to
    # 3 failed. It reported the opposite of what happened, for exactly the
    # reason §73.2 gives for the skip guard `ci/t1-gate.sh` removed.
    fire="$log.fired"
    : > "$fire"
    RUSTMC_T1_SABOTAGE="$arm" RUSTMC_T1_SABOTAGE_LOG="$fire" \
        cargo test -q -p "$crate" --features "$FEAT" --test "$bin" \
        > "$log" 2>&1
    red=$?
    fired=0
    grep -q "T1-SABOTAGE-FIRST $arm" "$fire" && fired=1

    # Unarmed, same build, to show the red is the arm and not the feature.
    cargo test -q -p "$crate" --features "$FEAT" --test "$bin" \
        > "$log.unarmed" 2>&1
    green=$?

    if [ "$red" -ne 0 ] && [ "$green" -eq 0 ] && [ "$fired" -eq 1 ]; then
        echo "ARM $arm -> $bin RED armed, GREEN unarmed, fired"
        PASS=$((PASS + 1))
    else
        printf 'ARM %s -> %s NOT A CONTROL (armed rc=%s, unarmed rc=%s, fired=%s)\n' \
            "$arm" "$bin" "$red" "$green" "$fired"
        [ "$fired" -eq 0 ] && echo "      fired=0 is UNVISITED: this gate never reaches the arm's code"
        grep -E '^(test result|---- )' "$log" | head -4 | sed 's/^/      /'
        FAIL=$((FAIL + 1))
    fi
done

echo "T1-SABOTAGE $PASS forced, $FAIL not"
[ "$FAIL" -eq 0 ] || exit 1
