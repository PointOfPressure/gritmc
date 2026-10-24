#!/usr/bin/env bash
# corpus-exclusion-falsify.sh — two recordings on this box at once must still
# be impossible, after the `cargo build` moved out of the corpus lock.
#
# WHAT THE MOVE PUT AT RISK. Until 2026-08-18 `ci/t2-gameplay.sh` ran its two
# release builds *inside* the hold, so a queued run did nothing at all until it
# owned the box. It now builds before it queues, which means a second run is
# executing real work on the box while a first one records. The lock's whole
# reason for existing is that this must never extend to the recordings
# themselves — three overlapped on 2026-08-06 and produced garbage: throughput
# fell to about one scenario per ten minutes and three runs were abandoned
# between 50/86 and 74/86.
#
# So the property is not "the lock is still called". It is:
#
#     while one run is recording, a second run may build,
#     and may NOT reach a recording.
#
# Both halves are asserted, because only asserting the second would pass for a
# change that never got the concurrency it was made for, and only asserting the
# first would pass for a change that removed the lock.
#
# CASE 7 IS WHY THE OTHER SIX MEAN ANYTHING. A test that starts two runs and
# finds one recording proves nothing unless the same test, with the exclusion
# removed, finds two. Case 7 removes it — `corpus_lock_acquire` is shadowed by
# a stub that returns immediately — and asserts both runs reach the reference
# recording. Without it every assertion here is a green that cannot go red,
# which is this repo's most common defect and the one `docs/AGENT-BRIEF.md`
# names first.
#
# Boots nothing, needs no jar and no JVM. Private lock namespace via
# `RUSTMC_CORPUS_LOCK_DIR`, private phase log via `RUSTMC_HOLD_PHASE_DIR`, two
# scratch trees under `mktemp -d`. About 50 seconds, almost all of it the
# deliberate hold window.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

W="$(mktemp -d "${TMPDIR:-/tmp}/corpus-excl.XXXXXX")"
trap 'rm -rf "$W"' EXIT
export RUSTMC_CORPUS_LOCK_DIR="$W/lock"
export RUSTMC_HOLD_PHASE_DIR="$W/phases"
export CORPUS_LOCK_ANCHOR_NAME="corpus-excl-anchor-$$"
export CORPUS_LOCK_HEARTBEAT=0
mkdir -p "$RUSTMC_CORPUS_LOCK_DIR" "$RUSTMC_HOLD_PHASE_DIR"

# THE FIRST RUN'S HOLD IS ENDED BY THE OBSERVER, NOT BY A CLOCK.
#
# The obvious shape is `sleep N` inside the first run and a smaller `sleep` in
# the test before it looks. Both numbers then have to be guessed against a box
# whose load average moves between 3 and 100 within an hour, and the guess has
# exactly one failure mode: on a loaded box the second run has not finished
# building when the test looks, the test sees no build-done marker, and case 1
# fails for a reason that has nothing to do with the corpus lock. That is not
# hypothetical — T0's `readiness-falsify` was RED at load 11.8 and GREEN at 3.5
# on the same tree and the same binaries, for precisely this reason, and this
# box routinely sits above both.
#
# So there is no duration to choose. The stubbed recording blocks on a sentinel
# file; the test removes it once it has observed everything it needs; the hold
# is exactly as long as the observation takes and no longer, on any box at any
# load. `GL_LIVE_BOUND` below is a backstop against a broken test hanging
# forever — it is never reached, so its value is not a measurement and is not
# claimed to be one.
#
# The same reasoning applies to every "hold the lock for N seconds" constant in
# a lock falsifier on this box, including `ci/gate-lock-falsify.sh`'s.
LIVE_BOUND="${CORPUS_EXCL_LIVE_BOUND:-180}"

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  ok    %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# --- a scratch tree, the `ci/reference-cache-integration.sh` pattern ---------
build_tree() {
    local T="$1" slow="$2"
    rm -rf "$T"; mkdir -p "$T/target/release" "$T/data/vanilla" "$T/crates/fake/src" \
        "$T/tools/oracle-runner/src" "$T/oracle-work/gameplay" \
        "$T/oracle-work/vanilla-26.2/versions/26.2" "$T/bin"
    cp -r "$ROOT/ci" "$T/ci"
    cp "$ROOT"/tools/oracle-runner/src/*.rs "$T/tools/oracle-runner/src/" 2>/dev/null
    printf 'not a real jar\n' > "$T/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
    cat > "$T/crates/fake/src/lib.rs" <<'EOS'
const A: &str = "data/vanilla/items.json";
const B: &str = "data/vanilla/block_props.json";
const C: &str = "data/vanilla/entity_types.json";
EOS
    printf '{"items":{"minecraft:stone":1}}\n'         > "$T/data/vanilla/items.json"
    printf '{"table":[{"id":1,"state":"stone"}]}\n'    > "$T/data/vanilla/block_props.json"
    printf '{"entity_types":{"minecraft:player":0}}\n' > "$T/data/vanilla/entity_types.json"
    ( . "$T/ci/provenance.sh"
      for t in items block_props entity_types; do
          prov_stamp "$T" "$T/data/vanilla/$t.json" ci/gen-gameplay-tables.sh
      done )

    # THE INSTRUMENT. `start` is the first thing in the harness that would
    # touch the reference, so reaching it IS reaching a recording. It stamps
    # the shared timeline with the tree's name and the instant, and — in the
    # slow tree only — holds there, which is what gives the second run a window
    # to be observed in.
    cat > "$T/ci/vanilla-server.sh" <<EOS
#!/usr/bin/env bash
case "\$1" in
    start)
        printf 'start\t%s\t%s\n' "$(basename "$T")" "\$(date +%s.%N)" >> "$W/timeline"
        ${slow:+_n=\$(( $LIVE_BOUND * 20 ))
        while [ -e "$W/hold-open" ] && [ "\$_n" -gt 0 ]; do sleep 0.05; _n=\$((_n-1)); done}
        printf 'stop\t%s\t%s\n' "$(basename "$T")" "\$(date +%s.%N)" >> "$W/timeline"
        echo "STUB: vanilla-server start was reached" >&2
        exit 1 ;;
    alive) exit 0 ;;
    *) exit 0 ;;
esac
EOS
    chmod +x "$T/ci/vanilla-server.sh"
    cat > "$T/target/release/oracle-runner" <<'EOS'
#!/usr/bin/env bash
[ "${1:-}" = "projection-id" ] && { echo "proj:f7fbb29926074663"; exit 0; }
echo "STUB oracle-runner: $*" >&2; exit 1
EOS
    chmod +x "$T/target/release/oracle-runner"
    # `cargo` stubbed to take a measurable, non-zero amount of time. A build
    # that costs nothing could not show that it ran concurrently with anything.
    printf '#!/usr/bin/env bash\nsleep 2\nexit 0\n' > "$T/bin/cargo"; chmod +x "$T/bin/cargo"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$T/target/release/rustmc-server"
    chmod +x "$T/target/release/rustmc-server"
}

run_gate() {  # <tree> <log> [role]
    ( cd "$1" && PATH="$1/bin:$PATH" \
        RUSTMC_CORPUS_LOCK_DIR="$RUSTMC_CORPUS_LOCK_DIR" \
        RUSTMC_HOLD_PHASE_DIR="$RUSTMC_HOLD_PHASE_DIR" \
        CORPUS_LOCK_HEARTBEAT=0 \
        RUSTMC_CORPUS_LOCK_POOL="${POOL:-1}" RUSTMC_CORPUS_SHARDS=1 \
        ${3:+RUSTMC_CORPUS_LOCK_ROLE=$3} \
        timeout 120 ./ci/t2-gameplay.sh ) > "$2" 2>&1
}

# A phase marker's instant for a given tree, from the shared phase log.
phase_at() {  # <tree-basename> <phase>
    awk -F'\t' -v t="$2" '$6==t && $4 ~ /\/'"$1"'$/ {print $1; exit}' \
        "$RUSTMC_HOLD_PHASE_DIR/phases.tsv" 2>/dev/null
}

echo "== corpus-exclusion-falsify: two scratch trees, one private lock"
echo

# ---------------------------------------------------------------------------
# Cases 1-5: the real harness, twice, overlapping.
# ---------------------------------------------------------------------------
: > "$W/timeline"; : > "$W/hold-open"
build_tree "$W/A" slow
build_tree "$W/B" ""

run_gate "$W/A" "$W/A.log" &
APID=$!
# Wait until A is genuinely inside its recording rather than sleeping a guess.
_n=$(( LIVE_BOUND * 10 ))
while ! grep -q '^start' "$W/timeline" 2>/dev/null && [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n-1)); done
# A REFUSAL, NOT A FAILURE, AND THE DIFFERENCE IS THE WHOLE POINT OF THE FILE
# BELOW. If A never reached its recording, this test has learned nothing about
# whether two recordings can overlap — it has learned that its own scratch tree
# would not start. Reporting that as a failed case would put "two corpus runs
# can overlap" in a gate log over a scratch tree that could not boot a stub,
# which is the same defect `ci/check-gate-stages.py`'s `t0_refusals` rule
# catalogues: a check that cannot run reporting a verdict about the code.
# Exit 2 and say REFUSED, so a caller can tell them apart.
grep -q '^start' "$W/timeline" || {
    echo
    echo "CORPUS-EXCLUSION-FALSIFY REFUSED — the scratch tree never reached a recording."
    echo "  This says nothing about the corpus lock. Nothing below ran."
    echo "  tail of A's log:"
    tail -6 "$W/A.log" | sed 's/^/    /'
    exit 2
}

run_gate "$W/B" "$W/B.log" &
BPID=$!

# Wait for the STATE the assertions are about — B having finished its build —
# rather than for a duration somebody guessed would cover it. Bounded, so a B
# that never builds fails the case instead of hanging it.
_n=$(( LIVE_BOUND * 10 ))
while [ -z "$(phase_at B build-done)" ] && [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n-1)); done
BUILD_DONE_B="$(phase_at B build-done)"
STARTS_DURING="$(grep -c '^start' "$W/timeline")"
STATUS="$("$ROOT/ci/corpus-lock.sh" status 2>/dev/null)"

[ -n "$BUILD_DONE_B" ] \
    && ok "1  B finished its release build while A held the lock (the move's point)" \
    || bad "1  B has no build-done marker while A held — the build did not run outside the lock"

[ "$STARTS_DURING" = "1" ] \
    && ok "2  exactly one recording is live while A holds — B is NOT recording" \
    || bad "2  $STARTS_DURING recordings live at once — EXCLUSION IS BROKEN"

if [ -z "$(phase_at B ports-done)" ]; then
    ok "3  B has not picked ports either — it is parked at the acquire, not past it"
else
    bad "3  B got past its acquire while A held the lock"
fi

case "$STATUS" in
    *"WAITING"*|*"waiting"*) ok "4  the lock's own status names B as a waiter" ;;
    *) bad "4  status does not show a waiter: $(echo "$STATUS" | head -3 | tr '\n' ' ')" ;;
esac

# Everything above was observed while A held the lock. Release it.
rm -f "$W/hold-open"
wait "$APID" 2>/dev/null
wait "$BPID" 2>/dev/null

# 5. B must not have been aborted — it waits, then gets its turn. A lock that
#    refused the second run would also produce "one recording at a time" and
#    would be a different, worse protocol.
if [ "$(grep -c '^start' "$W/timeline")" = "2" ]; then
    ok "5  after A released, B took the lock and reached its own recording"
else
    bad "5  B never recorded at all — it was refused rather than queued: $(tail -3 "$W/B.log" | tr '\n' ' ')"
fi

# 6. The two recordings do not overlap by so much as an instant. Case 2 is a
#    count at one moment; this is the interval arithmetic over the whole run,
#    and it is the assertion that would catch an overlap that opened and closed
#    between two of case 2's eyeblinks.
python3 - "$W/timeline" <<'PY'
import sys
# Intervals from the stub's OWN start/stop stamps. Deriving the end from a
# nominal hold length would be assuming the very thing under test.
lines = [l.rstrip("\n").split("\t") for l in open(sys.argv[1])]
starts = {r[1]: float(r[2]) for r in lines if r[0] == "start"}
stops  = {r[1]: float(r[2]) for r in lines if r[0] == "stop"}
iv = sorted((t, stops.get(k, t), k) for k, t in starts.items())
bad = [(a, b) for a, b in zip(iv, iv[1:]) if b[0] < a[1]]
print("  ok    6  the recording intervals are disjoint (%s)" % ", ".join(
    "%s@%.2f" % (r[2], r[0]) for r in iv) if not bad else
    "  FAIL  6  recordings OVERLAP: %s" % bad)
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] && PASS=$((PASS+1)) || FAIL=$((FAIL+1))

# ---------------------------------------------------------------------------
# Case 7: the negative control. Remove the exclusion, keep everything else,
# and the same two runs MUST both record. Without this, cases 2, 3 and 6 are
# greens that cannot go red.
# ---------------------------------------------------------------------------
: > "$W/timeline"; : > "$W/hold-open"
build_tree "$W/A" slow
build_tree "$W/B" ""
for T in "$W/A" "$W/B"; do
    # Shadow the acquire, and nothing else. The file is still sourced, the
    # release still runs, the harness is byte-identical: the ONLY difference
    # between this case and cases 1-6 is that taking the lock does nothing.
    cat >> "$T/ci/corpus-lock.sh" <<'EOS'
corpus_lock_acquire() { _CL_HELD=1; echo "corpus lock: NEGATIVE CONTROL — exclusion disabled"; return 0; }
corpus_lock_release() { return 0; }
corpus_lock_assert_anchored() { return 0; }
EOS
done
run_gate "$W/A" "$W/A2.log" & A2=$!
for _ in $(seq 1 60); do grep -q '^start' "$W/timeline" 2>/dev/null && break; sleep 0.5; done
run_gate "$W/B" "$W/B2.log" & B2=$!
# Same discipline as above: wait for the state, not for a duration. Here the
# state is B having reached a recording, which with the exclusion removed it
# must be able to do while A is still in one.
_n=$(( LIVE_BOUND * 10 ))
while [ "$(grep -c '^start' "$W/timeline")" -lt 2 ] && [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n-1)); done
N_CONC="$(grep -c '^start' "$W/timeline")"
rm -f "$W/hold-open"
wait "$A2" 2>/dev/null; wait "$B2" 2>/dev/null

if [ "$N_CONC" = "2" ]; then
    ok "7  NEGATIVE CONTROL — with the acquire stubbed out, both runs record at"
    echo "        once. Cases 2, 3 and 6 above can therefore fail, and did not."
else
    bad "7  NEGATIVE CONTROL FAILED — only $N_CONC recording(s) started with the
        exclusion removed. Something other than the corpus lock is serialising
        these runs, so cases 2, 3 and 6 prove nothing about the lock."
fi

# ---------------------------------------------------------------------------
# Case 8: the acquire's documented position, derived from the file rather than
# from this comment. `ci/t2-gameplay.sh` lines 227-249 state three properties;
# a future edit that moves the acquire past the port pick, or adds a second
# acquire site, breaks the protocol silently and this is what says so.
# ---------------------------------------------------------------------------
SRC="$ROOT/ci/t2-gameplay.sh"
N_ACQ=$(grep -c '^corpus_lock_acquire$' "$SRC")
L_PRE=$(grep -n '^PREFLIGHT_OK=1$' "$SRC" | cut -d: -f1)
L_ACQ=$(grep -n '^corpus_lock_acquire$' "$SRC" | cut -d: -f1)
# The port PROBE, not the function definition: the probe is the thing that
# must run under the lock, and `pick_port_pair() {` would still match if the
# call moved somewhere else entirely.
L_PORT=$(grep -n 'read -r VAN_PORT OUR_PORT < <(pick_port_pair)' "$SRC" | cut -d: -f1)
L_BUILD=$(grep -n '^cargo build -q -p oracle-runner --release$' "$SRC" | cut -d: -f1)
if [ "$N_ACQ" = 1 ] && [ -n "$L_PRE" ] && [ -n "$L_PORT" ] && [ -n "$L_BUILD" ] \
   && [ "$L_PRE" -lt "$L_ACQ" ] && [ "$L_ACQ" -lt "$L_PORT" ] && [ "$L_BUILD" -lt "$L_ACQ" ]; then
    ok "8  one acquire site, after the preflight, before the port pick, after the build"
else
    bad "8  the acquire's position moved: sites=$N_ACQ preflight=$L_PRE build=$L_BUILD acquire=$L_ACQ ports=$L_PORT"
fi

# ---------------------------------------------------------------------------
# Case 9: the acquire..port-bind gap costs nothing and builds nothing. That
# gap is where a port probed free can be taken by somebody else before it is
# bound, and shortening it was the second reason for the move. Asserted twice
# and deliberately: once on the text, because that is what a future edit
# changes, and once on the clock from the runs above, because a textual check
# cannot see a slow command that is not spelled `cargo`.
# ---------------------------------------------------------------------------
GAP_TXT=$(sed -n "$((L_ACQ + 1)),$((${L_PORT:-0} - 1))p" "$SRC" | grep -vE '^\s*#')
if ! printf '%s\n' "$GAP_TXT" | grep -c 'cargo' >/dev/null; then
    ok "9  no build between the acquire and the port bind (text)"
else
    bad "9  a cargo invocation is back between the acquire and the port bind"
fi

# The same claim, measured. `ports` is acquire-end..ports-done, which is
# exactly this gap, taken from the two real runs cases 1-6 just made.
PORTS_MAX=$(awk -F'\t' '$6=="acquire-end"{a[$3]=$1} $6=="ports-done"{if($3 in a){d=$1-a[$3]; if(d>m)m=d}} END{printf "%.2f", m+0}' \
            "$RUSTMC_HOLD_PHASE_DIR/phases.tsv" 2>/dev/null)
if [ -n "$PORTS_MAX" ] && awk -v v="$PORTS_MAX" 'BEGIN{exit !(v>0 && v<2)}'; then
    ok "10 the gap is ${PORTS_MAX}s of wall clock, measured, over the runs above"
else
    bad "10 acquire..ports-done measured ${PORTS_MAX:-<nothing>}s — expected a
        non-zero time under 2s. Zero means the markers did not record; over 2s
        means real work is back inside the gap."
fi

# ---------------------------------------------------------------------------
# Cases 11-12: the POOL (2026-09-24). Cases 1-10 run at `POOL=1`, which is the
# mutex they were written for. At a pool of two, two trees must be able to
# record at once — or the pool bought nothing — and a THIRD must still be held
# out until one of them finishes, or it is not a bound. Same stubs, same
# observer-ended hold, three trees.
# ---------------------------------------------------------------------------
: > "$W/timeline"; : > "$W/hold-open"
build_tree "$W/A" slow
build_tree "$W/B" slow
build_tree "$W/C" ""
POOL=2 run_gate "$W/A" "$W/A3.log" & A3=$!
_n=$(( LIVE_BOUND * 10 ))
while [ "$(grep -c '^start' "$W/timeline")" -lt 1 ] && [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n-1)); done
POOL=2 run_gate "$W/B" "$W/B3.log" & B3=$!
_n=$(( LIVE_BOUND * 10 ))
while [ "$(grep -c '^start' "$W/timeline")" -lt 2 ] && [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n-1)); done
POOL=2 run_gate "$W/C" "$W/C3.log" & C3=$!
_n=$(( LIVE_BOUND * 10 ))
while [ -z "$(phase_at C build-done)" ] && [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n-1)); done
sleep 2
N_POOL="$(grep -c '^start' "$W/timeline")"
C_PORTS="$(phase_at C ports-done)"
rm -f "$W/hold-open"
wait "$A3" 2>/dev/null; wait "$B3" 2>/dev/null; wait "$C3" 2>/dev/null
if [ "$N_POOL" = "2" ] && [ -z "$C_PORTS" ]; then
    ok "11 pool of 2: two trees recorded at once and the third was held at its acquire"
else
    bad "11 pool of 2: $N_POOL recordings live with two slots held, third past its acquire='${C_PORTS:-no}'"
fi
if [ "$(grep -c '^start' "$W/timeline")" = "3" ]; then
    ok "12 pool of 2: the third tree recorded once a slot came free"
else
    bad "12 pool of 2: the third tree never recorded: $(tail -3 "$W/C3.log" | tr '\n' ' ')"
fi

echo
echo "corpus-exclusion-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || { echo; echo "--- A.log tail ---"; tail -12 "$W/A.log"
                       echo "--- B.log tail ---"; tail -12 "$W/B.log"; exit 1; }
