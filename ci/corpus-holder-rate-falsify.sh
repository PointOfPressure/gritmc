#!/usr/bin/env bash
# corpus-holder-rate-falsify.sh — the contamination monitor is what licenses
# loading this box while somebody else's merge is recording, so it is checked
# before it is trusted rather than after.
#
#     ./ci/corpus-holder-rate-falsify.sh
#
# The cases that matter:
#
#   1  the schedstat sum must cover ALL of a process's threads. A single
#      `/proc/<pid>/schedstat` describes the main thread only, and the reference
#      is a JVM with dozens, so a one-thread read would report a calm holder
#      whatever was happening to it;
#   2  the holder must be picked, never a waiter. A waiter is blocked on
#      `flock`, accumulates no runqueue delay at all, and would report perfect
#      health for a box that was on fire;
#   6  it must sample the HOLDER's recorder, not any recorder on the box. This
#      rig runs its own recorders in scratch trees at the same time, so "the
#      first `oracle-runner record` in the process table" would sample the
#      experiment and call it the production run.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRIVER="${HOLDERRATE_DRIVER:-$ROOT/ci/corpus-holder-rate.sh}"
W="$(mktemp -d /tmp/holderrate-falsify.XXXXXX)"
# **The stub marker carries this instance's PID.** The sweep re-invokes this
# file four times, and with a shared `pkill -f` pattern one instance's cleanup
# reaps another instance's stub. That is how `share-is-delay-over-cpu` came back
# MISSED: the sub-invocation's fake recorder was killed under it, every sample
# row became the -1 sentinel, and -1 agrees with every formula.
MARK="holder-rate-falsify-proc-$$"
cleanup() { pkill -f "$MARK" 2>/dev/null || true; rm -rf "$W"; }
trap cleanup EXIT

# **The fake trees live under $W, not at a fixed /tmp path.** Two instances of
# this file (the sweep runs four more of it) and any spinner left over from a
# previous run would otherwise share "$HT", and case 7 — "a holder
# with no recorder running is reported" — went red because the PREVIOUS run's
# spinner was still there. A falsifier that fails when run twice is measuring
# the calendar.
HT="$W/HOLDER-TREE"
DT="$W/DECOY-TREE"

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# A scratch root: the script under test, a corpus-lock.sh whose `status` is
# whatever the case needs, and the REAL healthy-baseline (whose extractor this
# one delegates to and whose own falsifier already covers it).
mkroot() {  # mkroot <name> <status-text>
    local n="$1" st="$2"
    local R="$W/root-$n"
    mkdir -p "$R/ci"
    cp "$DRIVER" "$R/ci/corpus-holder-rate.sh"
    cp "$ROOT/ci/corpus-healthy-baseline.sh" "$R/ci/"
    printf '#!/usr/bin/env bash\n[ "${1:-}" = status ] && cat <<%s\n%s\n%s\nexit 0\n' EOFX "$st" EOFX \
        > "$R/ci/corpus-lock.sh"
    chmod +x "$R/ci/corpus-lock.sh" "$R/ci/corpus-holder-rate.sh"
    echo "$R"
}
# A process that looks like a recorder in <tree>, so attribution can be tested
# without a corpus run.
# **No `setsid` here.** A backgrounded `setsid` FORKS, because the shell's job
# is already a process group leader, so `$!` is the setsid that exits
# immediately and the process the case wants has a different pid. That cost a
# run of this file, which reported "no rows written for a live holder with a
# live recorder" about a sampler that was working.
# **And it writes its pid to a file, and is never called inside `$(...)`.** A
# command substitution runs in a subshell AND keeps the pipe open until every
# process writing to it exits, so `FP="$(fake_recorder ...)"` captured the wrong
# pid. Case 6 then reported "it sampled pid X but the holder's recorder is Y"
# about a sampler that had picked correctly, which is the worst kind of red: a
# true instrument accused by a broken harness.
#
# **It SPINS rather than sleeping, at `nice 19`.** A sleeping process
# accumulates neither CPU time nor runqueue delay, so `delay/(cpu+delay)` and
# `delay/cpu` both come out as the -1 sentinel and case 3 cannot tell them
# apart. The `share-is-delay-over-cpu` injection was MISSED for exactly that
# reason: the case was green against a broken formula because both numerators
# were zero.
# The spin body carries its own two-minute deadline, so a fake recorder cannot
# outlive this file however this file dies.
printf '%s\n' 'e=$((SECONDS + 120)); while [ $SECONDS -lt $e ]; do :; done' > "$W/spin.sh"
fake_recorder() {  # fake_recorder <tree> <pidfile>
    nice -n 19 bash -c "exec -a 'oracle-runner record 127.0.0.1:1 $1/ci/checks/gameplay --out $1/x.json $MARK' bash $W/spin.sh" \
        >/dev/null 2>&1 &
    echo $! > "$2"
}

say() { printf '%s\n' "$*"; }
say "== corpus-holder-rate-falsify"

# 1. ALL THREADS. A four-thread spinner must sum to more than its main thread.
python3 - "$W/mt.pid" <<'PY' &
import sys, threading, os, time
open(sys.argv[1], "w").write(str(os.getpid()))
def spin():
    t = time.time() + 12
    while time.time() < t: pass
for _ in range(4): threading.Thread(target=spin, daemon=True).start()
time.sleep(12)
PY
MTCTL=$!
sleep 3
MT="$(cat "$W/mt.pid" 2>/dev/null || true)"
if [ -n "$MT" ] && [ -d "/proc/$MT/task" ]; then
    SUM="$("$DRIVER" --schedstat "$MT" | awk '{print $1}')"
    MAIN="$(awk '{print $1}' "/proc/$MT/schedstat" 2>/dev/null)"
    NT="$(ls "/proc/$MT/task" | wc -l)"
    if [ "$NT" -ge 4 ] && [ "$SUM" -gt "$MAIN" ]; then
        ok "1 schedstat sums all $NT threads ($SUM ns) not just the main one ($MAIN ns)"
    else
        bad "1 schedstat over $NT threads summed $SUM against a main thread of $MAIN"
    fi
else
    bad "1 could not start a multi-threaded process, so the case tested nothing"
fi
wait "$MTCTL" 2>/dev/null || true

# 2. THE HOLDER, NOT A WAITER. Four waiters, one holder, and the oldest waiter
#    is listed first, which is exactly what a careless pattern picks.
STATUS='corpus lock: /home/x/.rustmc-corpus-lock
  state:   HELD
  record:  run pid 1 (anchor 2) on box in '"$HT"' since 2026-08-18T12:00:00+01:00 (16m ago)
  waiting (service order):
    1. run pid 3 in /tmp/WAITER-A, waiting 211m
    2. run pid 4 in /tmp/WAITER-B, waiting 190m
    3. run pid 5 in /tmp/WAITER-C, waiting 36m
    4. run pid 6 in /tmp/WAITER-D, waiting 14m'
R="$(mkroot holder "$STATUS")"
H="$("$R/ci/corpus-holder-rate.sh" --holder)"
[ "$H" = "$HT" ] && ok "2 --holder picked the holder past four waiters" \
    || bad "2 --holder returned '$H', wanted $HT"

# 3. THE SHARE IS delay/(cpu+delay). Driven through a real run against a fake
#    recorder, then checked against the columns it wrote.
mkdir -p "$HT"
fake_recorder "$HT" "$W/fp"; FP="$(cat "$W/fp")"
sleep 1
"$R/ci/corpus-holder-rate.sh" 4 2 "$W/rate.tsv" case3 >/dev/null 2>&1
# NROWS is computed first on purpose: `$(( $(wc -l < "$f") - 1 ))` is a command
# substitution containing a redirect and quotes inside an arithmetic expansion,
# and bash refuses to parse it. The syntax error takes the whole `if` block with
# it, so cases 4, 7 and 8 then ran with an unset $R and reported failures about
# a script that was fine.
NROWS=$(wc -l < "$W/rate.tsv" 2>/dev/null || echo 0)
if [ -s "$W/rate.tsv" ] && [ "$NROWS" -ge 2 ]; then
    ok "3 sampled the holder and wrote $((NROWS - 1)) row(s)"
    # **The sentinel makes this case able to pass vacuously, so it is guarded
    # first.** A row where the sampled process accumulated neither CPU time nor
    # runqueue delay reports -1, and `delay/(cpu+delay)` and `delay/cpu` BOTH
    # report -1 for it. A run whose rows are all sentinel therefore agrees with
    # any formula at all, and the `share-is-delay-over-cpu` injection came back
    # MISSED twice for exactly that reason. If nothing accumulated, this case
    # tested nothing and says so rather than passing.
    LIVEROWS="$(awk -F'\t' 'NR>1 && ($5+$6)>0 {n++} END{print n+0}' "$W/rate.tsv")"
    if [ "$LIVEROWS" -lt 1 ]; then
        bad "3 every row is the -1 sentinel: the stub accumulated no CPU and no runqueue delay,
        so this case cannot discriminate delay/(cpu+delay) from anything else.
        rows: $(awk -F'\t' 'NR>1{printf "%s/%s ", $5, $6}' "$W/rate.tsv")"
    else
        BADROW="$(awk -F'\t' 'NR>1 && ($5+$6)>0 { want=$6/($5+$6); d=want-$8; if (d<0) d=-d; if (d>0.0002) print NR }' "$W/rate.tsv")"
        [ -z "$BADROW" ] && ok "3 delay_share equals delay/(cpu+delay) on all $LIVEROWS live row(s)" \
            || bad "3 delay_share does not match delay/(cpu+delay) on row(s): $BADROW
        values: $(awk -F'\t' 'NR>1{printf "cpu=%s delay=%s share=%s | ", $5, $6, $8}' "$W/rate.tsv")"
    fi
    PIDCOL="$(awk -F'\t' 'NR==2{print $4}' "$W/rate.tsv")"
    [ "$PIDCOL" = "$FP" ] && ok "6 it sampled the HOLDER's recorder (pid $FP), not any recorder on the box" \
        || bad "6 it sampled pid '$PIDCOL' but the holder's recorder is $FP"
else
    bad "3 no rows written for a live holder with a live recorder"
    bad "6 (not reached)"
fi

# 4. A decoy recorder in ANOTHER tree must not be picked. This is case 6's
#    injection-resistant half: with only one recorder on the box, "any recorder"
#    and "the holder's recorder" are the same answer.
mkdir -p "$DT"
fake_recorder "$DT" "$W/dp"; DP="$(cat "$W/dp")"
sleep 1
"$R/ci/corpus-holder-rate.sh" 4 2 "$W/rate2.tsv" case4 >/dev/null 2>&1
PIDCOL2="$(awk -F'\t' 'NR==2{print $4}' "$W/rate2.tsv" 2>/dev/null)"
[ "$PIDCOL2" = "$FP" ] && ok "4 with a decoy recorder in another tree it still picked the holder's" \
    || bad "4 picked '$PIDCOL2' with a decoy present; the holder's is $FP"
kill "$DP" 2>/dev/null

# 5. IT WRITES NOTHING INTO THE HOLDER'S TREE. An instrument that disturbs a run
#    in order to ask whether the run is disturbed is worthless.
find "$HT" -newermt '-10 seconds' -type f 2>/dev/null | grep -c . >/dev/null \
    && bad "5 something under the holder's tree was written during sampling" \
    || ok "5 nothing under the holder's tree was written during sampling"

# 7. A dead recorder stops the sampling rather than being attributed to.
kill "$FP" 2>/dev/null; sleep 1
"$R/ci/corpus-holder-rate.sh" 6 2 "$W/rate3.tsv" case7 >"$W/l7" 2>&1
RC=$?
if [ "$RC" -eq 3 ] && grep -q "has no 'oracle-runner record' running" "$W/l7"; then
    ok "7 a holder between its two recordings is reported, not sampled as zero"
else
    bad "7 with no recorder in the holder's tree: rc=$RC, said $(tail -1 "$W/l7")"
fi

# 9. --follow RE-RESOLVES. The ordinary mode stops when the holder's recorder
#    exits, which is right; following has to survive that, or a six-hour
#    passive sample is a sample of the first twenty minutes.
fake_recorder "$HT" "$W/f1"; F1="$(cat "$W/f1")"
sleep 1
( sleep 6; kill "$F1" 2>/dev/null; sleep 2; fake_recorder "$HT" "$W/f2" ) &
KICK=$!
"$R/ci/corpus-holder-rate.sh" --follow 40 2 "$W/follow.tsv" f >/dev/null 2>&1
wait "$KICK" 2>/dev/null || true
F2="$(cat "$W/f2" 2>/dev/null || echo none)"
NPIDS="$(awk -F'\t' 'NR>1{print $4}' "$W/follow.tsv" 2>/dev/null | sort -u | wc -l)"
if [ "$NPIDS" -ge 2 ]; then
    ok "9 --follow carried on across a holder's recorder dying ($NPIDS distinct pids sampled)"
else
    bad "9 --follow sampled $NPIDS pid(s) across a recorder change; it stopped at the first one"
fi
kill "$F1" "$F2" 2>/dev/null || true

# 10. IT MUST SAY HOW MANY ROWS IT WROTE. The passive sampler lost half an hour
#     of samples on 2026-08-18 and the loss was invisible, because a file with
#     five rows and a file that should have had thirty-five are the same bytes.
#     A claim was built on the fragment and had to be withdrawn
#     (docs/CORPUS-LOCK-HOLD.md §22.4).
fake_recorder "$HT" "$W/f3"; F3="$(cat "$W/f3")"
sleep 1
"$R/ci/corpus-holder-rate.sh" 8 2 "$W/count.tsv" c10 >/dev/null 2>&1
kill "$F3" 2>/dev/null
END_ROW="$(grep '^SAMPLER-END' "$W/count.tsv" 2>/dev/null | head -1)"
DATA_ROWS="$(grep -vc -e '^label' -e '^SAMPLER-END' "$W/count.tsv" 2>/dev/null)"; DATA_ROWS="${DATA_ROWS:-0}"
if [ -z "$END_ROW" ]; then
    bad "10 no SAMPLER-END row: a truncated sample file cannot be told from a quiet box"
else
    CLAIMED="$(printf '%s' "$END_ROW" | awk -F'\t' '{print $3}' | awk '{print $1}')"
    if [ "$CLAIMED" = "$DATA_ROWS" ]; then
        ok "10 SAMPLER-END reports $CLAIMED rows and $DATA_ROWS were written"
    else
        bad "10 SAMPLER-END claims $CLAIMED rows but $DATA_ROWS are in the file"
    fi
fi

# 8. Nobody holding the lock is exit 3 and a word, not a silent empty file.
RF="$(mkroot free '  state:   FREE')"
"$RF/ci/corpus-holder-rate.sh" 4 2 "$W/rate4.tsv" case8 >"$W/l8" 2>&1
RC=$?
[ "$RC" -eq 3 ] && grep -q "nobody holds the corpus lock" "$W/l8" \
    && ok "8 with a free lock it exits 3 and says so" \
    || bad "8 with a free lock: rc=$RC, said $(tail -1 "$W/l8")"

rmdir "$HT" "$DT" 2>/dev/null || true

MISSED=0
if [ -z "${HOLDERRATE_NO_SWEEP:-}" ]; then
    echo
    echo "== injections (each MUST turn this suite red)"
    inject() {
        local n="$1" e="$2"
        local out="$W/drv.$n.sh"
        cp "$DRIVER" "$out"; perl -0pi -e "$e" "$out"; chmod +x "$out"
        if cmp -s "$DRIVER" "$out"; then echo STALE; return; fi
        if diff <(grep -vE '^\s*#' "$DRIVER") <(grep -vE '^\s*#' "$out") >/dev/null; then
            echo COMMENT-ONLY; return; fi
        echo "$out"
    }
    check() {
        local n="$1" d="$2"
        [ -n "$d" ] || { printf '  MISSED  %-30s <-- inject produced nothing\n' "$n"; MISSED=$((MISSED+1)); return; }
        case "$d" in
            STALE)        printf '  MISSED  %-30s <-- stale patch\n' "$n"; MISSED=$((MISSED+1)); return ;;
            COMMENT-ONLY) printf '  MISSED  %-30s <-- comment-only patch\n' "$n"; MISSED=$((MISSED+1)); return ;;
        esac
        if HOLDERRATE_DRIVER="$d" HOLDERRATE_NO_SWEEP=1 "$0" >/dev/null 2>&1; then
            printf '  MISSED  %-30s <-- the suite stayed green with this broken\n' "$n"; MISSED=$((MISSED+1))
        else
            printf '  CAUGHT  %-30s\n' "$n"
        fi
    }
    check "main-thread-only"   "$(inject onethread 's/for t in \/proc\/"\$pid"\/task\/\*\/schedstat; do/for t in \/proc\/"$pid"\/schedstat; do/')"
    check "share-is-delay-over-cpu" "$(inject wrongshare 's/\(t>0\)\? d\/t : -1/(c>0)? d\/c : -1/')"
    check "any-recorder-will-do" "$(inject anyrec 's/grep -qF "\$HOLDER"/grep -qF ""/')"
    check "follow-does-not-refollow" "$(inject nofollow 's/while \[ "\$\(date \+%s\)" -lt "\$F_END" \]; do\n        F_LEFT/while false; do\n        F_LEFT/')"
    check "no-row-count-reported" "$(inject nocount 's/^printf .SAMPLER-END.*$/:/m')"
    check "waiter-instead-of-holder" "$(inject waiter 's/\| "\$ROOT\/ci\/corpus-healthy-baseline.sh" --holder-from-status/| sed -n "s\/^[[:space:]]*[0-9]*\\\\. run pid [0-9]* in \\\\(\\\\\/[^ ,]*\\\\).*\/\\\\1\/p" | head -1/')"
fi

echo
echo "=================================================================="
echo "cases: $PASS passed, $FAIL failed"
[ -z "${HOLDERRATE_NO_SWEEP:-}" ] && echo "injections: $MISSED not caught"
if [ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]; then echo "CORPUS-HOLDER-RATE-FALSIFY GREEN"
else echo "CORPUS-HOLDER-RATE-FALSIFY RED"; fi
echo "=================================================================="
exit $(( FAIL > 0 || MISSED > 0 ))
