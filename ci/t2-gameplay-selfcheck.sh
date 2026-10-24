#!/usr/bin/env bash
# t2-gameplay-selfcheck.sh — is this harness reproducible at all?
#
# Records the whole corpus against vanilla **N times**, on N cold-booted
# servers, and diffs every run against the first with the same comparator
# `ci/t2-gameplay.sh` uses. The expected answer is that every scenario matches
# in every run. Anything else is a fault in the instrument, because the
# reference cannot disagree with itself about its own behaviour.
#
#     ./ci/t2-gameplay-selfcheck.sh
#     SELFCHECK_RUNS=6 ./ci/t2-gameplay-selfcheck.sh   # cheaper, weaker
#
# # Why N is 12 and not 2, which is what it used to be
#
# **A two-run design is a coin flip on the flaps that matter.** Flag a check
# unstable when the N runs are not all identical, model it as a check with two
# outcomes whose rarer one has probability p, and the chance of catching it is
#
#     P(detect) = 1 - p^N - (1-p)^N
#
# because the only way to miss is for all N samples to land on the same side.
# At N = 2 that is 2p(1-p), which peaks at **50%** for an even flap and is
# *worse* for anything lopsided. The corpus has already paid for this: check
# 235 `a-bed-facing-north-puts-its-head-the-other-way` came back 4:2 across six
# runs on 2026-08-03 — a straddled chunk boundary — and a two-run selfcheck
# had a 1-in-2.25 chance of ever noticing.
#
# What N = 12 buys, at the same model:
#
#     even 3:3 flap        (p = 1/2)   99.95%
#     the measured 4:2     (p = 1/3)   99.23%
#     a 3:1 minority       (p = 1/4)   96.83%
#     a 1-in-6 minority    (p = 1/6)   88.78%
#
# So 12 catches the flap this corpus actually produced better than 99 times in
# 100, and the honest limit is the last line: **a one-in-six flap still gets
# through this instrument about one run in nine.** Catching that at 99% needs
# N = 26 (99.13%), which is another fourteen runs — about two and a half hours
# more wall clock — for a fault nobody has yet observed. If one turns up, that is the number to raise it to, and this
# comment is the place the arithmetic lives so nobody has to redo it.
#
# There is no multiple-comparison cost to pay against that: a stable check
# produces N identical traces however large N is, so extra samples only ever
# add power. The whole cost is wall clock — about 11 minutes a run measured on
# the Threadripper 2026-08-03 (97.5 s of that is the corpus's own summed
# `settle_ms`; the rest is 55 cold scenario setups), so N = 12 is a bit over
# two hours and this is a background instrument, not a gate.
#
# # Why this exists
#
# Three separate conclusions about vanilla have now been withdrawn here, and
# every one of them was a harness that was not measuring what it thought:
# a player that had never sent `player_loaded` and so was still at spawn; a
# client that never acknowledged a chunk batch and so held nine chunks; and,
# built on top of the second, a rule excluding the acting player from
# broadcasts that was written into this server's code.
#
# In each case the tell was available and nobody had an instrument pointed at
# it: **the reference disagreed with itself between runs.** `break-block`
# went `match` -> `diverge` with no change to either server, and that was
# noticed only because it happened to be in the corpus on the day. This makes
# that check deliberate and cheap instead of lucky.
#
# It is not in `ci/t0.sh` — it cold-boots a server per run and takes hours.
# Run it after any change to the harness, and before trusting a number that
# rests on a channel nobody has re-measured.
#
# # WHAT TO DO WITH THE RECORDINGS AFTERWARDS — 2026-08-09
#
# The expensive half of this script is the recording, and the recordings it
# leaves in `$SELFCHECK_WORK` are **the best input `ci/corpus-floor.sh` can
# have**: N cold boots of one server taken by one recorder, which is the
# homogeneous ensemble that script's heterogeneity control exists to ask for.
#
#     ./ci/corpus-floor.sh --update oracle-work/gameplay-selfcheck/r[0-9]*.json
#
# **`r[0-9]*.json`, not `r*.json`.** This directory also holds `report-N.json`
# from the aggregation below, and a plain `r*` glob feeds those back in as
# recordings — see `ci/selfcheck-recompare.sh`'s header, where it happened.
# `corpus-floor.sh` refuses a report rather than trusting the caller's glob,
# but the tighter glob is still what to type.
#
# That measurement answers a different question from this script's own. This
# one asks whether the *declared* set of unstable checks is right, pairwise
# against run 1. `corpus-floor.sh` asks how many distinct answers the reference
# gave across all pairs, splits what is vanilla's from what is this harness's,
# and produces the standing floor a corpus score is quoted beside. Neither
# replaces the other and both read the same bytes.
#
# # LAUNCH IT DETACHED. This has now truncated the run twice, identically.
#
#     ssh threadripper 'cd ~/<tree> && setsid nohup ./ci/t2-gameplay-selfcheck.sh \
#         > ~/selfcheck.log 2>&1 < /dev/null &'
#
# At N = 12 this needs about two and a half hours, which is longer than the
# lifetime of most things that can start it. Run over a plain `ssh`, it dies
# when its caller does, and **it dies silently in the worst possible way**: the
# recordings stop mid-run, the pairwise comparisons and the aggregation at the
# bottom of this file never execute at all, and what is left behind is a log
# full of successful `recorded n/55` lines and **no verdict**. A reader sees
# progress and no failure.
#
# Two independent attempts on 2026-08-03 both died in **run 6 at
# `recorded 25/55`** — the same run, the same scenario, to the line. That looks
# exactly like a poisoned scenario and is not one: runs 1 through 5 each walked
# past that same scenario without incident, five clean traversals, so the
# content is exonerated by the run's own log. Both deaths were at roughly 65
# minutes of wall clock, in whichever run happened to be in flight. It is the
# launcher's ceiling, not this script's.
#
# So the designed N had never once executed, and the eight names in
# `ci/checks/gameplay-unstable.toml` were assembled by hand from the five
# traces that survived. **The N-run aggregation had never run on live data at
# any N** — it was only ever replayed against saved reports. Same shape as the
# `ssh -n` that ate `ci/handoff.sh`'s stage 1 for its whole existence: the
# transport killed the stage, the exit code said nothing, and the number was
# asserted rather than observed.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

RUNS="${SELFCHECK_RUNS:-12}"
[ "$RUNS" -ge 2 ] || { echo "SELFCHECK_RUNS must be at least 2" >&2; exit 2; }
# One port per run rather than one reused: a vanilla server that has just been
# stopped leaves the port in TIME_WAIT, and a rebind failure here would look
# like an unstable check.
PORT_BASE="${SELFCHECK_PORT_BASE:-}"
WORK="${SELFCHECK_WORK:-$ROOT/oracle-work/gameplay-selfcheck}"
# The corpus to record. Overridable, and the reason is arithmetic rather than
# convenience.
#
# `ci/selfcheck-recompare.sh` measures a check's reproducibility against
# **saved** recordings in seconds, which is the right tool for anything that
# only changes the comparator. It cannot answer for a check the recordings do
# not contain — it reports "not present" and silently drops it — and the
# recordings on the Threadripper hold 61 scenarios and predate every check
# added since. So a **new** check has no reproducibility figure at all, and
# producing one meant re-recording the whole corpus: 12 minutes a run, five
# hours at the N = 26 the existing recordings were taken at.
#
# That is why `560`, `561` and `562` shipped on 2026-08-05 with the note "a
# 25/25 figure for these checks does not exist and cannot be produced". It
# can now: point this at a directory holding the handful of checks in
# question and the record step costs what those checks cost rather than what
# eighty-six do.
#
#     SELFCHECK_CHECKS=/tmp/mine SELFCHECK_WORK=/tmp/mine-work \
#     SELFCHECK_RUNS=25 ./ci/t2-gameplay-selfcheck.sh
#
# **`SELFCHECK_WORK` moves with it and that is not optional.** The recordings
# under the default work directory are the corpus-wide ones every later
# `selfcheck-recompare.sh` reads; a subset run writing over them would leave
# `r1.json` holding seven scenarios and every future recompare would report
# the other seventy-nine as "not present in the recordings" — which is the
# failure this whole block exists to complain about, manufactured by our own
# hand.
#
# It measures exactly what it did before: the same recorder, the same
# comparator, N cold-booted servers. The denominator is whatever was pointed
# at, so quote the count.
CHECKS="${SELFCHECK_CHECKS:-$ROOT/ci/checks/gameplay}"
[ -d "$CHECKS" ] || { echo "SELFCHECK REFUSED — no corpus at $CHECKS" >&2; exit 4; }
# Pin the world. Without it every run generates a fresh one and the spawn
# point moves with it.
GAMEPLAY_SEED="${GAMEPLAY_SEED:-20260803}"

cleanup() {
    local i
    # Tidies the corpus lock's holder record. NOT what releases the lock — the
    # kernel kills the anchor when this process dies. Guarded because the trap
    # is installed before ci/corpus-lock.sh is sourced.
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
    for i in $(seq 1 "$RUNS"); do
        "$ROOT/ci/vanilla-server.sh" stop "$WORK/r$i" >/dev/null 2>&1 || true
    done
}
trap cleanup EXIT

# Nonzero, not `exit 0`. This block used to print `skipping: no vanilla
# extract` and then exit **successfully**, so a directory with no extracts ran
# no corpus at all and told its caller it had passed — the same shape of
# not-a-gate as `ci/t2-gameplay.sh`'s old preflight, in the file that exists to
# check the reference against itself. A self-check that cannot fail is worse
# than none, because it is what you reach for when you distrust a number.
for f in items.json block_props.json registries.bin flat_preset.json; do
    if [ ! -s "$ROOT/data/vanilla/$f" ]; then
        echo "SELFCHECK REFUSED — no vanilla extract at data/vanilla/$f" >&2
        echo "                    run ./ci/gen-gameplay-tables.sh" >&2
        exit 4
    fi
done

mkdir -p "$WORK"
# One corpus run on this box at a time. This is the heaviest corpus consumer
# there is — it records the corpus against vanilla `$RUNS` times, twelve by
# default, so it holds the box for hours and would otherwise sit on top of a
# merge gate for all of them. See ci/corpus-lock.sh.
. "$ROOT/ci/corpus-lock.sh"
corpus_lock_acquire
# Ports are chosen AFTER the corpus lock, from the tree path, and probed:
# the lock is a pool since 2026-09-24 and a fixed pair shared by every tree is
# the 2026-08-06 collision under a pool. See ci/corpus-ports.sh.
# The runs are sequential and each boots on PORT_BASE + i - 1, so the base is
# derived per tree and every port is still probed by ci/vanilla-server.sh.
. "$ROOT/ci/corpus-ports.sh"
[ -n "$PORT_BASE" ] || PORT_BASE="$(corpus_free_ports 1 selfcheck)" \
    || { echo "SELFCHECK REFUSED — no free port in 28000-28999" >&2; exit 4; }

cargo build -q -p oracle-runner --release
BIN="$ROOT/target/release/oracle-runner"

# The same substrate as `ci/t2-gameplay.sh`, and it has to stay the same: a
# self-check on a quieter world than the real run would pass while the real
# run stayed noisy.
run_one() {
    local dir="$1" port="$2" out="$3"
    rm -rf "$dir"
    # `spawn-radius=0` is substrate, like `difficulty=peaceful`.
# `PlayerList.placeNewPlayer` calls `ServerPlayer.fudgeSpawnLocation`, which
# picks a random position inside the spawn radius on **every join**, so three
# consecutive runs put the reference player at (-4.5, 2.5), (-3.5, -2.5) and
# (3.5, 6.5). Anything whose bytes depend on absolute position — the block
# order inside a `section_blocks_update` does — then differs between two runs
# of the same server with nothing changed. A fixed seed pins the world; this
# pins the join.
VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        "$ROOT/ci/vanilla-server.sh" start "$dir" "$port" "minecraft:flat" "$GAMEPLAY_SEED"
    # 26.2 ids; see ci/t2-gameplay.sh for why the old camelCase ones were a
    # silent no-op for as long as this corpus has existed.
    for g in "advance_time false" "spawn_mobs false" "random_tick_speed 0" "advance_weather false"; do
        "$ROOT/ci/vanilla-server.sh" console "$dir" "gamerule $g"
        sleep 1
        if grep -qE "Incorrect argument|Unknown or incomplete" "$dir/logs/latest.log" 2>/dev/null; then
            echo "FAIL: vanilla refused 'gamerule $g'" >&2; exit 1
        fi
    done
    sleep 1
    "$BIN" record "127.0.0.1:$port" "$CHECKS" --out "$out"
    # Stamp the recording with the recorder that made it, AT THE TIME IT WAS
    # MADE. `ci/corpus-floor.sh` groups an ensemble by recorder to decide
    # whether a disagreement could be a decoder difference, and with no stamp
    # it has to fall back to hashing whatever is in the tree *now* — which
    # groups two recordings taken from one tree at two different commits as
    # one recorder, and that is precisely the confound the grouping exists to
    # catch. The four other fields mirror `ci/t2-gameplay.sh`'s stamp.
    #
    # **It is read out of the binary now, and that changed the string.** It was
    # `cat "$ROOT"/tools/oracle-runner/src/*.rs | sha256sum | cut -c1-16` — the
    # tree's sources, which is the same question one door along: "what recorder
    # is in that directory" rather than "what recorder ran". On a box where
    # `rsync -a` can leave sources older than the artefacts built from them
    # those give different answers and the tree's is the wrong one, exactly as
    # `projection_identity` found. `oracle-runner recorder-id` digests the
    # embedded text of `compare::RECORDER_SOURCES`, so the stamp describes the
    # binary that recorded.
    #
    # `ci/t2-gameplay.sh` writes the same field from the same command as of
    # 2026-08-09, so the two harnesses' recordings are groupable against each
    # other for the first time. Recordings stamped before this carry the old
    # bare-hex form and group apart from the new `rec:` one — which is
    # over-splitting, and over-splitting only ever *shrinks* the homogeneous
    # subset. That is the safe direction: it can make a flap suspect, never
    # admissible.
    {
        printf 'tree=%s\n'     "$ROOT"
        printf 'commit=%s\n'   "$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
        printf 'checks=%s\n'   "$(find "$CHECKS" -type f | sort | xargs md5sum | md5sum | cut -d' ' -f1)"
        printf 'recorder=%s\n' "$("$BIN" recorder-id 2>/dev/null || echo unavailable)"
        printf 'md5=%s\n'      "$(md5sum "$out" | cut -d' ' -f1)"
    } > "$out.prov"
    "$ROOT/ci/vanilla-server.sh" stop "$dir"
}

for i in $(seq 1 "$RUNS"); do
    echo "== vanilla, run $i of $RUNS, on :$((PORT_BASE + i - 1))"
    run_one "$WORK/r$i" "$((PORT_BASE + i - 1))" "$WORK/r$i.json"
done

# No baseline: the ratchet is about this server's progress, and there is no
# subject server here. Every scenario that is not declared unstable should
# simply match, in every run.
#
# Run 1 is the reference for all the others. That is equivalent to "are the N
# traces all identical" — if run 1 agrees with each of runs 2..N then they all
# agree with each other — and it costs N-1 comparisons instead of N(N-1)/2.
# **A FAILING COMPARISON MUST NOT END THE SWEEP.** `set -e` plus a `compare`
# that exits non-zero on an unacceptable verdict used to abort the loop at the
# first one, so the aggregate below was computed over however many pairs
# happened to precede it and reported as if it were the whole thing.
#
# Measured 2026-08-05: a 26-run selfcheck costing four hours fifty-five minutes
# of recording aborted at **report-5**, because
# `540-a-player-that-leaves-the-ground-is-resynced` came back `vacuous` the
# first time both sides took the same branch of a 13/13 vanilla coin flip. Four
# of twenty-five comparisons ran. Every rate the run existed to measure was
# missing, and nothing said so — the log simply stopped, which reads exactly
# like a run that finished.
#
# The recording is the expensive half and it had completed. So the comparisons
# are run to completion and the exit status is carried to the end, which is
# also what makes the recordings reusable by `ci/selfcheck-recompare.sh`
# afterwards instead of needing another five hours.
REPORTS=()
COMPARE_FAILED=0
for i in $(seq 2 "$RUNS"); do
    echo "== run 1 against run $i"
    "$BIN" compare "$CHECKS" --reference "$WORK/r1.json" --subject "$WORK/r$i.json" \
        --json "$WORK/report-$i.json" || COMPARE_FAILED=$((COMPARE_FAILED + 1))
    REPORTS+=("$WORK/report-$i.json")
done
if [ "$COMPARE_FAILED" -gt 0 ]; then
    echo "selfcheck: $COMPARE_FAILED of ${#REPORTS[@]} comparisons reported an" \
         "unacceptable verdict; all of them ran and the aggregate below covers" \
         "every pair"
fi
# The last pairwise report is also written where the old two-run design wrote
# its only one, so anything reading `report.json` still finds a report.
cp "$WORK/report-$RUNS.json" "$WORK/report.json"

# The measured set of unreproducible checks against the declared one.
#
# This used to end at the line above, which printed the numbers and told the
# reader to think about them. It came back 34/37 on its first run and the
# three failures were real and permanent — vanilla draws a mob's spawn yaw
# and a dropped item's position from `Level.getRandom()`, so those checks can
# never match for anybody. A number a human is asked to interpret is not a
# gate; three permanent failures printed every run become wallpaper within a
# week, and the next *new* instability hides behind them.
#
# So the list is declared in `ci/checks/gameplay-unstable.toml` with its
# cause, and this compares against it in **both** directions: a newly
# unstable check is an instrument regression, and one that has become stable
# again means the declared list is stale.
#
# **This gate was checked against the rule it exists to serve**, because a
# gate that cannot fail is not a gate and that has cost this project four
# separate faults. Replayed against the 2026-08-03 selfcheck report with
# three deliberately wrong declared lists:
#
#   an extra declared name (break-block)  -> "NO LONGER UNSTABLE", exit 1
#   a real one removed (water-spreads)    -> "NEWLY UNSTABLE",     exit 1
#   the committed file                    -> green,                exit 0
#
# So it is known to fire on both directions rather than assumed to.
#
# **The N-run aggregation below was replayed the same way** rather than
# inheriting that assurance, because it is different code: the same three
# lists against the 2026-08-03 report supplied as two comparisons gave
# exit 1 / exit 1 / exit 0, with the seven declared names all reported at
# 2/2 comparisons and 48/55 reproduced. A rewrite of a gate is a new gate.
#
# **And again at the real N**, because two comparisons is not eleven and this
# aggregation had never seen eleven — the runs that would have fed it were
# killed by their launcher before they reached it (see the block above). Eleven
# comparisons synthesised from the 2026-08-03 corpus report, every check
# matching except the eight declared names failing in a varying subset:
#
#   the committed file       -> "12 runs, declared 8, measured 8, 47/55
#                               reproduced", per-name splits 6/11 to 10/11,
#                               exit 0
#   water-spreads removed    -> "NEWLY UNSTABLE water-spreads (diverge,
#                               8/11 comparisons)",                exit 1
#   break-block added        -> "NO LONGER UNSTABLE break-block",  exit 1
#
# So the code that will report the number is known to be able to fail at the N
# that will produce it, rather than at a smaller one it has been run at.
#
# With N runs a check is unstable if it failed to reproduce in **any** of the
# N-1 comparisons, and the count of runs it failed in is printed beside it.
# That count is the whole reason for raising N: it is the measured split, and
# it is what tells a 6-in-11 coin flip apart from a 1-in-11 rarity that needs
# a larger N before anyone trusts a green.
echo "== declared instability against measured, over $RUNS runs"
python3 "$ROOT/ci/selfcheck-aggregate.py" "$ROOT/ci/checks/gameplay-unstable.toml" "${REPORTS[@]}"
echo "SELFCHECK GREEN: vanilla reproduces itself everywhere it is declared to"
