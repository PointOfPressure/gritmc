#!/usr/bin/env bash
# corpus-holder-progress-falsify.sh — the sampler that produced §22's headline.
#
#     ./ci/corpus-holder-progress-falsify.sh
#
# `docs/CORPUS-LOCK-HOLD.md` §22 says a corpus recording in the 50-110 band
# runs at 0.86 of its quiet rate, and the scenarios-per-minute half of that
# came out of this file. §22.2 then reads L5 as failed on the strength of it,
# and §22.5 says the rate is "not a proxy for anything; it is the quantity a
# merge waits on". A number carrying that much weight should not come from an
# instrument nothing has tried to break.
#
# Every case drives it against a log file written at a KNOWN rate by a stub, so
# the answer is arithmetic rather than opinion. No lock, no server, no corpus.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRIVER="${PROGRESS_DRIVER:-$ROOT/ci/corpus-holder-progress.sh}"
W="$(mktemp -d /tmp/progress-falsify.XXXXXX)"
trap 'rm -rf "$W"; pkill -f "progress-falsify-writer-$$" 2>/dev/null || true' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }
say() { printf '%s\n' "$*"; }

# A log that grows at a known rate: one `  recorded n/108` line every <gap>
# seconds, plus decoy lines that must NOT be counted.
start_writer() {   # start_writer <log> <gap> <count>
    local log="$1" gap="$2" n="$3"
    bash -c "
        exec -a 'progress-falsify-writer-$$' bash -c '
            for i in \$(seq 1 $n); do
                printf \"  recorded %d/108  scenario-\$i\n\" \"\$i\" >> \"$log\"
                printf \"some other line mentioning recorded elsewhere\n\" >> \"$log\"
                sleep $gap
            done'
    " >/dev/null 2>&1 &
}

say "== corpus-holder-progress-falsify"

# 1. It counts the harness's progress lines and nothing else. The decoy line
#    above contains the word `recorded` without the two-space prefix, which is
#    what separates a progress line from prose in a t2 log.
L="$W/count.log"
printf '  recorded 1/108  a\n  recorded 2/108  b\nrecorded 3/108 not-a-progress-line\n' > "$L"
printf 'FAIL: 13 scenario(s) unacceptable, recorded above\n' >> "$L"
"$DRIVER" "$L" 4 2 "$W/count.tsv" c1 >/dev/null 2>&1
FIRST="$(awk -F'\t' 'NR==2{print $4}' "$W/count.tsv" 2>/dev/null)"
if [ "$FIRST" = 2 ]; then
    ok "1 counts the two progress lines and ignores the two decoys"
else
    bad "1 counted '$FIRST' progress lines in a file with 2 and 2 decoys"
fi

# 2. THE RATE ARITHMETIC. A log growing at one scenario every 2s must read as
#    30 per minute, within the granularity of a 2s sampling interval.
L2="$W/rate.log"; : > "$L2"
start_writer "$L2" 2 12
sleep 1
"$DRIVER" "$L2" 16 2 "$W/rate.tsv" c2 >/dev/null 2>&1
RATE="$(awk -F'\t' 'NR>1 && $6!="" && $1!="PROGRESS-COMPLETE" && $1!="PROGRESS-INCOMPLETE" {s+=$6; n++} END{if(n) printf "%.1f", s/n}' "$W/rate.tsv")"
if [ -n "$RATE" ] && awk -v r="$RATE" 'BEGIN{exit !(r > 22 && r < 38)}'; then
    ok "2 a log growing at one scenario per 2s reads as $RATE per minute (want ~30)"
else
    bad "2 a log growing at one scenario per 2s read as '${RATE:-nothing}' per minute"
fi

# 3. And the ENDPOINT count agrees with the per-window rates, which is how §22
#    computed its ratio: (last - first) over elapsed.
EP="$(awk -F'\t' '$1=="c2" {if(f=="") {f=$4; t0=$3}; l=$4; t1=$3} END{if(t1>t0) printf "%.1f", (l-f)*60/(t1-t0)}' "$W/rate.tsv")"
if [ -n "$EP" ] && awk -v r="$EP" 'BEGIN{exit !(r > 22 && r < 38)}'; then
    ok "3 the endpoint rate agrees at $EP per minute"
else
    bad "3 endpoint rate was '${EP:-nothing}' per minute"
fi

# 4. COMPLETENESS. A run that took every sample says so.
# The stamp must be present and its claimed count must equal the rows actually
# in the file. Whether the run came out COMPLETE or one sample short is timing
# and is not the property under test; that the number is TRUE is.
STAMP="$(grep -E '^PROGRESS-(COMPLETE|INCOMPLETE)' "$W/rate.tsv" | head -1)"
ACTUAL="$(grep -c -v -e '^label' -e '^PROGRESS-' "$W/rate.tsv" 2>/dev/null)"; ACTUAL="${ACTUAL:-0}"
CLAIM="$(printf '%s' "$STAMP" | awk -F'\t' '{print $3}')"
if [ -z "$STAMP" ]; then
    bad "4 a finished run left no completion stamp at all"
elif [ "$CLAIM" = "$ACTUAL" ]; then
    ok "4 the stamp claims $CLAIM rows and $ACTUAL are in the file"
else
    bad "4 the stamp claims $CLAIM rows but $ACTUAL are in the file"
fi

# 5. And one cut short must exit 8 and say INCOMPLETE, so a short file cannot
#    read as a quiet run. Driven by making the output unwritable, which is what
#    a full disk looks like and needs no process to be killed.
printf 'label\ttime\tepoch\trecorded\tdelta\tper_min\tload1\tworkers\n' > "$W/ro.tsv"
chmod 0444 "$W/ro.tsv"
"$DRIVER" "$L" 4 2 "$W/ro.tsv" c5 > "$W/ro.log" 2>&1
RC=$?
chmod 0644 "$W/ro.tsv" 2>/dev/null
if [ "$RC" -eq 8 ] && grep -q 'PROGRESS-INCOMPLETE' "$W/ro.log"; then
    ok "5 a run whose rows did not land exits 8 and says INCOMPLETE"
else
    bad "5 a run that wrote no rows exited $RC: $(tail -1 "$W/ro.log")"
fi

# 6. An unreadable log is refused up front rather than sampled as zero.
"$DRIVER" "$W/nonexistent.log" 4 2 "$W/x.tsv" c6 > "$W/x.log" 2>&1
RC=$?
[ "$RC" -eq 3 ] && grep -q 'cannot read' "$W/x.log" \
    && ok "6 a log it cannot read is refused with exit 3" \
    || bad "6 an unreadable log gave rc=$RC: $(tail -1 "$W/x.log")"

MISSED=0
if [ -z "${PROGRESS_NO_SWEEP:-}" ]; then
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
        [ -n "$d" ] || { printf '  MISSED  %-28s <-- inject produced nothing\n' "$n"; MISSED=$((MISSED+1)); return; }
        case "$d" in
            STALE|COMMENT-ONLY) printf '  MISSED  %-28s <-- %s\n' "$n" "$d"; MISSED=$((MISSED+1)); return ;;
        esac
        if PROGRESS_DRIVER="$d" PROGRESS_NO_SWEEP=1 "$0" >/dev/null 2>&1; then
            printf '  MISSED  %-28s <-- the suite stayed green with this broken\n' "$n"; MISSED=$((MISSED+1))
        else
            printf '  CAUGHT  %-28s\n' "$n"
        fi
    }
    check "counts-every-line"    "$(inject anyline 's/grep -c .  recorded . "\$LOG"/grep -c "recorded" "$LOG"/')"
    check "rate-ignores-elapsed" "$(inject norate 's/d\*60\.0\/t/d*60.0/')"
    check "no-completeness-check" "$(inject nocomplete 's/if \[ "\$LANDED" -lt "\$WANT" \]; then/if false; then/')"
    check "unreadable-log-accepted" "$(inject noread 's/\[ -r "\$LOG" \] \|\|/[ 1 = 2 ] ||/')"
fi

echo
echo "=================================================================="
echo "cases: $PASS passed, $FAIL failed"
[ -z "${PROGRESS_NO_SWEEP:-}" ] && echo "injections: $MISSED not caught"
if [ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]; then echo "CORPUS-HOLDER-PROGRESS-FALSIFY GREEN"
else echo "CORPUS-HOLDER-PROGRESS-FALSIFY RED"; fi
echo "=================================================================="
exit $(( FAIL > 0 || MISSED > 0 ))
