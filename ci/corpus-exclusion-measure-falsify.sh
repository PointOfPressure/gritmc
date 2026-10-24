#!/usr/bin/env bash
# corpus-exclusion-measure-falsify.sh — the exclusion reader's own cases.
#
#     ./ci/corpus-exclusion-measure-falsify.sh
#
# `ci/corpus-exclusion-measure.py` is the thing that will be quoted when
# somebody asks whether concurrent recordings interfere. If it cannot see an
# effect, a null from it means nothing, and this repo's most common defect is
# exactly that: a green that could not have gone red.
#
# **CASE 6 IS WHY THE OTHERS MEAN ANYTHING.** It plants a sixty-fold collapse in
# the concurrent arm and requires the reader to report it. Every other case
# here checks that the reader throws away input it should not trust; only case
# 6 checks that it still reports the thing it exists to report.
#
# Boots nothing, needs no tree, no JVM and no lock. Synthetic logs in a
# `mktemp -d`, about a second.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
M="$ROOT/ci/corpus-exclusion-measure.py"
W="$(mktemp -d "${TMPDIR:-/tmp}/excl-measure.XXXXXX")"
trap 'rm -rf "$W"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); printf '  ok    %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# A synthetic run log in the shape the rig produces: every line prefixed with a
# unix timestamp, "subset: N check(s)", then the two markers that bracket the
# reference recording.
mklog() {   # mklog <dir> <tag> <start-epoch> <ref-seconds> <checks>
    local d="$1" tag="$2" t0="$3" dur="$4" n="$5"
    {
        printf '%d.000 subset: %d check(s)\n' "$t0" "$n"
        printf '%d.000 vanilla up on :27100 (%s)\n' "$((t0 + 1))" "$d"
        printf '%d.000 == rustmc on :27101\n' "$((t0 + 1 + dur))"
        printf '%d.000 done\n' "$((t0 + 2 + dur))"
    } > "$d/$tag.log"
}

# A pair: two logs plus the pairs.tsv row the rig writes.
mkpair() {  # mkpair <dir> <pt> <arm> <t0a> <dura> <t0b> <durb> [rca] [rcb]
    local d="$1" pt="$2" arm="$3" t0a="$4" da="$5" t0b="$6" db="$7"
    local rca="${8:-0}" rcb="${9:-0}"
    mklog "$d" "$pt-a" "$t0a" "$da" 8
    mklog "$d" "$pt-b" "$t0b" "$db" 8
    printf '%s\t%s\t%s\t%s\n' "$pt" "$arm" "$rca" "$rcb" >> "$d/pairs.tsv"
}

newdir() { local d="$W/$1"; rm -rf "$d"; mkdir -p "$d"; : > "$d/pairs.tsv"; echo "$d"; }

echo "== corpus-exclusion-measure falsification"

# 1. The happy path: overlapping concurrent pairs and serial pairs, all usable.
D="$(newdir case1)"
mkpair "$D" r1s serial 1000 80 1100 80
mkpair "$D" r2s serial 2000 80 2100 80
mkpair "$D" r1c conc   3000 80 3000 80
mkpair "$D" r2c conc   4000 80 4000 80
OUT="$("$M" "$D" 2>&1)"; RC=$?
if [ "$RC" -eq 0 ] && printf '%s' "$OUT" | grep -c 'serial  N= 4' >/dev/null \
   && printf '%s' "$OUT" | grep -c 'conc    N= 4' >/dev/null; then
    ok "1. four usable pairs, both arms N=4"
else
    bad "1. expected both arms N=4, rc 0"; printf '%s\n' "$OUT" | sed 's/^/        /'
fi

# 2. A concurrent pair whose recording windows do not intersect is DISCARDED.
#    This is the case that stops the rig scoring a fast non-overlap as a fast
#    concurrent pair, which would be a null manufactured out of nothing.
D="$(newdir case2)"
mkpair "$D" r1s serial 1000 80 1100 80
mkpair "$D" r2s serial 2000 80 2100 80
mkpair "$D" r1c conc   3000 80 5000 80      # b starts long after a finished
mkpair "$D" r2c conc   6000 80 6000 80
OUT="$("$M" "$D" 2>&1)"
if printf '%s' "$OUT" | grep -c 'r1c.*did not overlap' >/dev/null \
   && printf '%s' "$OUT" | grep -c 'conc    N= 2' >/dev/null; then
    ok "2. non-overlapping concurrent pair discarded, arm falls to N=2"
else
    bad "2. non-overlapping pair was not discarded"; printf '%s\n' "$OUT" | sed 's/^/        /'
fi

# 3. A serial pair is NOT discarded for failing to overlap — not overlapping is
#    what serial means. The discard rule must be arm-aware or it deletes the
#    control arm.
D="$(newdir case3)"
mkpair "$D" r1s serial 1000 80 5000 80
mkpair "$D" r2s serial 6000 80 9000 80
mkpair "$D" r1c conc   3000 80 3000 80
mkpair "$D" r2c conc   4000 80 4000 80
OUT="$("$M" "$D" 2>&1)"
if printf '%s' "$OUT" | grep -c 'serial  N= 4' >/dev/null; then
    ok "3. serial pairs are not discarded for not overlapping"
else
    bad "3. the discard rule ate the control arm"; printf '%s\n' "$OUT" | sed 's/^/        /'
fi

# 4. A non-zero exit anywhere in a pair discards the pair. A run that refused
#    measured nothing, and its wall time is not a rate.
D="$(newdir case4)"
mkpair "$D" r1s serial 1000 80 1100 80
mkpair "$D" r2s serial 2000 80 2100 80
mkpair "$D" r1c conc   3000 80 3000 80 0 4
mkpair "$D" r2c conc   4000 80 4000 80
OUT="$("$M" "$D" 2>&1)"
if printf '%s' "$OUT" | grep -c 'r1c.*non-zero exit' >/dev/null && printf '%s' "$OUT" | grep -c 'conc    N= 2' >/dev/null; then
    ok "4. a pair with a refusing run is discarded"
else
    bad "4. a refusing run was counted"; printf '%s\n' "$OUT" | sed 's/^/        /'
fi

# 5. Missing window markers discard the pair rather than producing a rate from
#    the whole process lifetime, which would silently include the build.
D="$(newdir case5)"
mkpair "$D" r1s serial 1000 80 1100 80
mkpair "$D" r2s serial 2000 80 2100 80
mkpair "$D" r1c conc   3000 80 3000 80
mkpair "$D" r2c conc   4000 80 4000 80
printf '%d.000 subset: 8 check(s)\n%d.000 nothing else\n' 5000 5001 > "$D/r1c-a.log"
OUT="$("$M" "$D" 2>&1)"
if printf '%s' "$OUT" | grep -c 'r1c.*markers missing' >/dev/null; then
    ok "5. a log without the recording-window markers is discarded"
else
    bad "5. a marker-less log was given a rate"; printf '%s\n' "$OUT" | sed 's/^/        /'
fi

# 6. THE NEGATIVE CONTROL. Plant a sixty-fold collapse in the concurrent arm
#    and require the reader to report it. Without this the five cases above
#    are all satisfied by a reader that reports nothing at all.
D="$(newdir case6)"
mkpair "$D" r1s serial 1000 80 1100 80          # 10 s/scenario
mkpair "$D" r2s serial 2000 80 2100 80
mkpair "$D" r1c conc   3000 4800 3000 4800      # 600 s/scenario
mkpair "$D" r2c conc   20000 4800 20000 4800
OUT="$("$M" "$D" 2>&1)"
RATIO="$(printf '%s' "$OUT" | sed -n 's/.*\/ serial = \([0-9.]*\)x.*/\1/p' | head -1)"
if [ -n "$RATIO" ] && awk "BEGIN{exit !($RATIO > 50)}"; then
    ok "6. NEGATIVE CONTROL — a planted 60x collapse is reported as ${RATIO}x"
else
    bad "6. NEGATIVE CONTROL FAILED — a planted 60x collapse was not reported (ratio='${RATIO}')"
    printf '%s\n' "$OUT" | sed 's/^/        /'
fi

# 7. An arm with fewer than two usable runs is refused, not averaged. One run
#    is not a spread and quoting it as one is how a figure gets withdrawn.
D="$(newdir case7)"
mkpair "$D" r1s serial 1000 80 1100 80
mkpair "$D" r1c conc   3000 80 5000 80          # discarded: no overlap
OUT="$("$M" "$D" 2>&1)"; RC=$?
if [ "$RC" -ne 0 ] && printf '%s' "$OUT" | grep -c 'REFUSING to summarise' >/dev/null; then
    ok "7. an arm with fewer than two usable runs is refused, non-zero"
else
    bad "7. a one-run arm was summarised (rc=$RC)"; printf '%s\n' "$OUT" | sed 's/^/        /'
fi

# 8. No pairs.tsv at all is a refusal, not an empty green.
D="$(newdir case8)"; rm -f "$D/pairs.tsv"
OUT="$("$M" "$D" 2>&1)"; RC=$?
if [ "$RC" -ne 0 ]; then ok "8. a directory with no pairs.tsv is refused"
else bad "8. an empty directory produced a green"; fi

# 9. THE TIMESTAMPER MUST TRACK TIME. Cases 1-8 all feed synthetic logs, so
#    every one of them passes against a rig whose timestamps are worthless.
#    This runs the real timestamper against a writer that emits three lines two
#    seconds apart and requires three DISTINCT stamps.
#
#    It is here because the first version of the rig used
#    `stdbuf -oL awk '{print systime(), $0}'`, and `stdbuf -oL` sets awk's
#    output buffering while doing nothing to its input: mawk reads stdin in
#    blocks, processed the whole run in one burst, and stamped every line with
#    the time of the burst. Measured, on the writer below: awk gave 278, 278,
#    278 where the read loop gives 278, 280, 282. Every recording window would
#    have come out near zero and the arm ratio would have been noise reported
#    with two decimal places.
E="$ROOT/ci/corpus-exclusion-experiment.sh"
SLOW="$W/slow.sh"
cat > "$SLOW" <<'SH'
#!/usr/bin/env bash
echo "vanilla up on :27100 (x)"
sleep 2
echo "middle"
sleep 2
echo "== rustmc on :27101"
SH
chmod +x "$SLOW"
STAMPS="$("$SLOW" | "$E" --stamp | cut -d. -f1)"
NDISTINCT="$(printf '%s\n' "$STAMPS" | sort -u | wc -l)"
SPAN=$(( $(printf '%s\n' "$STAMPS" | tail -1) - $(printf '%s\n' "$STAMPS" | head -1) ))
if [ "$NDISTINCT" -ge 3 ] && [ "$SPAN" -ge 3 ]; then
    ok "9. TIMESTAMPER — three lines 2 s apart get $NDISTINCT distinct stamps spanning ${SPAN}s"
else
    bad "9. TIMESTAMPER IS BUFFERED — $NDISTINCT distinct stamps, span ${SPAN}s (want >=3 and >=3s)"
    printf '%s\n' "$STAMPS" | sed 's/^/        /'
fi

# 10. A DUPLICATE PAIR TAG IS A REFUSAL. Two groups sharing a tag share their
#     log filenames, so one silently overwrites the other and the ensemble is
#     smaller than the row count suggests. The rig produced exactly this on
#     2026-08-18 via a loop variable that was not `local`, and nothing caught
#     it — the run looked healthy and the arms were quietly sharing files.
D="$(newdir case10)"
mkpair "$D" r1s serial 1000 80 1100 80
mkpair "$D" r2s serial 2000 80 2100 80
mkpair "$D" r1c conc   3000 80 3000 80
mkpair "$D" r1c conc   4000 80 4000 80      # same tag again
OUT="$("$M" "$D" 2>&1)"; RC=$?
if [ "$RC" -ne 0 ] && printf '%s' "$OUT" | grep -c 'duplicate pair tag' >/dev/null; then
    ok "10. a duplicated pair tag is refused, non-zero"
else
    bad "10. a duplicated pair tag was accepted (rc=$RC)"
    printf '%s\n' "$OUT" | sed 's/^/        /'
fi

echo
echo "corpus-exclusion-measure falsification: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
