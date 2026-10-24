#!/usr/bin/env bash
# hold-phases-falsify.sh — the hold decomposition's own instrument, tested.
#
# `ci/hold-phases.sh` is always on, in every corpus run and every merge gate on
# the box, and its output is what `docs/CORPUS-LOCK-HOLD.md` will be re-argued
# from. Two failure modes matter and they pull in opposite directions:
#
#   * it must never take a gate down. Every caller runs `set -euo pipefail`, so
#     one unguarded `find | wc -l` or one append to a full disk turns a green
#     gate red — an instrument that can do that is worse than no instrument;
#   * it must never report a number it did not measure. The obvious way to get
#     the first property is to swallow everything, which gets you an instrument
#     that silently reports plausible nonsense. That is the unfalsifiable-check
#     class docs/AGENT-BRIEF.md names first.
#
# The resolution, and what most of these cases assert, is that the two live in
# different halves: the WRITER never fails, and the READER refuses. Case 2 is
# the load-bearing one — a run whose markers are incomplete is absent from the
# report and named as absent, never averaged into it.
#
# Case 4 is the second load-bearing one. The `cargo build` moved out of the
# lock on 2026-08-18, and a reader with the old layout hardcoded would have
# gone on charging the build to the hold, wrong by exactly the amount the
# change was worth. So the reader decides inside-vs-outside from the
# timestamps, and case 4 feeds it both layouts and asserts it says so.
#
# No lock, no jar, no server, no cargo. About 3 seconds.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HP="$ROOT/ci/hold-phases.sh"

W="$(mktemp -d "${TMPDIR:-/tmp}/hold-phases-falsify.XXXXXX")"
trap 'rm -rf "$W"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  ok    %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# Write a synthetic run straight into a phase log: <log> <run> <tree> <role>
# <n> then phase:offset pairs. Offsets are seconds from an arbitrary epoch, so
# every span below has an answer known in advance rather than a measured one.
synth() {
    local log="$1" run="$2" tree="$3" role="$4" n="$5"; shift 5
    local base=1700000000 p off
    for p in "$@"; do
        off="${p##*:}"; p="${p%%:*}"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$(awk -v b=$base -v o="$off" 'BEGIN{printf "%.3f", b+o}')" \
            "iso" "$run" "$tree" "$role" "$p" "$n" "1.00" "-" >> "$log"
    done
}
report() { RUSTMC_HOLD_PHASE_DIR="$1" "$HP" report 2>&1; }

echo "== hold-phases-falsify"
echo

# --- 1. the writer cannot fail its caller -----------------------------------
# An unwritable log directory is the realistic version of a full disk, and the
# caller is a `set -euo pipefail` script exactly like ci/t2-gameplay.sh.
mkdir -p "$W/nowrite" && chmod 500 "$W/nowrite"
cat > "$W/caller.sh" <<EOS
set -euo pipefail
ROOT="$W"
. "$HP"
hold_phase_init "$W/no-such-corpus-dir"
hold_phase acquire-begin
hold_phase acquire-end
hold_phase release
echo REACHED-THE-END
EOS
OUT="$(RUSTMC_HOLD_PHASE_DIR="$W/nowrite/sub" bash "$W/caller.sh" 2>&1)"
if printf '%s' "$OUT" | grep -c 'REACHED-THE-END' >/dev/null; then
    ok "1  an unwritable log leaves a \`set -euo pipefail\` caller running to its last line"
else
    bad "1  the instrument killed its caller: $OUT"
fi
printf '%s' "$OUT" | grep -c 'cannot write' >/dev/null \
    && ok "2  ...and says once, on stderr, that this run will not be measured" \
    || bad "2  it failed silently — a run that is not being measured must say so"
chmod 700 "$W/nowrite"

# --- 3. an incomplete run is refused, not averaged in -----------------------
D="$W/d3"; mkdir -p "$D"
synth "$D/phases.tsv" r-good tree-good run 100 \
      acquire-begin:0 acquire-end:10 ports-done:11 vanilla-up:20 \
      reference-done:80 vanilla-stopped:82 rustmc-up:90 subject-done:150 \
      compare-done:151 release:152
synth "$D/phases.tsv" r-cut tree-cut run 100 \
      acquire-begin:0 acquire-end:10 ports-done:11 vanilla-up:20
R="$(report "$D")"
if printf '%s' "$R" | grep -c 'N=1 run' >/dev/null && printf '%s' "$R" | grep -c 'tree-cut' >/dev/null; then
    ok "3  a run with no \`release\` is excluded from the N and named as excluded"
else
    bad "3  the incomplete run was not excluded or not named: $(printf '%s' "$R" | head -3 | tr '\n' ' ')"
fi

# --- 4. the spans are arithmetic, and the answer is known in advance --------
# reference is vanilla-up..reference-done = 80-20 = 60 s = 1.00 min.
# The whole hold is acquire-end..release = 152-10 = 142 s = 2.37 min.
printf '%s' "$R" | grep -cE 'reference +1 +1\.00' >/dev/null \
    && ok "4  the reference span is the 1.00 min it was constructed to be" \
    || bad "4  reference span wrong: $(printf '%s' "$R" | grep reference | head -1)"
printf '%s' "$R" | grep -cE 'HOLD \(acq\.\.release\) +1 +2\.37' >/dev/null \
    && ok "5  the hold is acquire-end..release and nothing else" \
    || bad "5  hold wrong: $(printf '%s' "$R" | grep HOLD | head -1)"

# --- 6/7. THE LAYOUT IS READ, NOT ASSUMED -----------------------------------
# The same span, once before the acquire and once after it. A reader with the
# layout hardcoded gives the same answer twice; this one must not.
D="$W/d6"; mkdir -p "$D"
synth "$D/phases.tsv" r-out tree-outside run 100 \
      build-begin:0 build-done:60 acquire-begin:60 acquire-end:61 \
      ports-done:62 vanilla-up:70 reference-done:130 vanilla-stopped:132 \
      rustmc-up:140 subject-done:200 compare-done:201 release:202
R6="$(report "$D")"
if printf '%s' "$R6" | sed -n '/OUTSIDE THE HOLD/,$p' | grep -cE 'build +1 +1\.00' >/dev/null; then
    ok "6  a build before the acquire is reported OUTSIDE the hold, at 1.00 min"
else
    bad "6  the build was not reported outside: $(printf '%s' "$R6" | grep -i build | tr '\n' ' ')"
fi
D="$W/d7"; mkdir -p "$D"
synth "$D/phases.tsv" r-in tree-inside run 100 \
      acquire-begin:0 acquire-end:1 build-begin:1 build-done:61 \
      ports-done:62 vanilla-up:70 reference-done:130 vanilla-stopped:132 \
      rustmc-up:140 subject-done:200 compare-done:201 release:202
R7="$(report "$D")"
if printf '%s' "$R7" | sed -n '/INSIDE THE HOLD/,/OUTSIDE/p' | grep -cE 'build +1 +1\.00' >/dev/null; then
    ok "7  the SAME span after the acquire is reported INSIDE it — the reader"
    echo "        reads the layout out of the timestamps rather than assuming one,"
    echo "        which is what makes this report the falsifier for the move."
else
    bad "7  an in-lock build was not reported inside the hold: $(printf '%s' "$R7" | grep -i build | tr '\n' ' ')"
fi

# --- 8. a cache hit does not invent the phases a hit never has --------------
D="$W/d8"; mkdir -p "$D"
synth "$D/phases.tsv" r-hit tree-hit run 100 \
      acquire-begin:0 acquire-end:1 ports-done:2 refcache-hit:2 \
      reference-done:4 rustmc-up:12 subject-done:72 compare-done:73 release:74
R8="$(report "$D")"
if printf '%s' "$R8" | grep -c 'vanilla-boot' >/dev/null; then
    bad "8  a cache hit was credited with a vanilla boot it never did"
else
    ok "8  a hit reports no vanilla-boot and no vanilla-stop — phases it does not have"
fi

# --- 8b. a sharded run is one \`shards\` span, not a reference plus a subject --
# 2026-09-24. Its shards overlap each other, so the reader must not credit it
# with the serial spans it never had, and must report ports-done..subject-done
# (11..131 = 2.00 min) as the one interval its recording is.
D="$W/d8b"; mkdir -p "$D"
synth "$D/phases.tsv" r-shard tree-shard run 100 \
      acquire-begin:0 acquire-end:10 ports-done:11 refcache-miss:11 shards-begin:11 \
      reference-done:100 subject-done:131 compare-done:132 release:133
R8b="$(report "$D")"
if printf '%s' "$R8b" | grep -cE 'shards +1 +2\.00' >/dev/null \
   && ! printf '%s' "$R8b" | grep -cE '^(vanilla-boot|rustmc-boot|reference) ' >/dev/null; then
    ok "8b a sharded run is one 2.00 min \`shards\` span and no serial spans"
else
    bad "8b sharded run misreported: $(printf '%s' "$R8b" | grep -E 'shards|reference|boot' | tr '\n' ' ')"
fi

# --- 9. append atomicity, asserted rather than taken from POSIX -------------
# Sixty trees share one file. Forty concurrent writers, each emitting a
# distinguishable line; every line must arrive whole.
D="$W/d9"; mkdir -p "$D"
for i in $(seq 1 40); do
    ( export RUSTMC_HOLD_PHASE_DIR="$D" ROOT="/home/sayer/rustmc-w$i"
      . "$HP"; hold_phase_init ""; for j in $(seq 1 10); do hold_phase "p$j"; done ) &
done
wait
LINES=$(wc -l < "$D/phases.tsv")
TORN=$(awk -F'\t' 'NF!=9' "$D/phases.tsv" | wc -l)
if [ "$LINES" = 400 ] && [ "$TORN" = 0 ]; then
    ok "9  40 concurrent writers x 10 markers = 400 lines, 0 torn"
else
    bad "9  concurrent append is not atomic: $LINES lines, $TORN torn (expected 400/0)"
fi

# --- 10. the denominator comes from the corpus, not from a constant ---------
D="$W/d10"; mkdir -p "$D" "$W/corpus"
for i in $(seq 1 7); do : > "$W/corpus/s$i.toml"; done
: > "$W/corpus/notes.md"
N="$( export RUSTMC_HOLD_PHASE_DIR="$D" ROOT="$W"
      . "$HP"; hold_phase_init "$W/corpus"; hold_phase x
      awk -F'\t' 'NR==1{print $7}' "$D/phases.tsv" )"
[ "$N" = 7 ] \
    && ok "10 the scenario count is read from the corpus (7 toml, 1 non-toml ignored)" \
    || bad "10 scenario count is $N, expected 7 — the rates would have a wrong denominator"

# --- 11. a malformed line is counted, never silently dropped ---------------
D="$W/d11"; mkdir -p "$D"
synth "$D/phases.tsv" r-ok tree-ok run 100 \
      acquire-begin:0 acquire-end:1 release:100
printf 'garbage with no tabs\n' >> "$D/phases.tsv"
R11="$(report "$D")"
printf '%s' "$R11" | grep -c 'malformed line' >/dev/null \
    && ok "11 a malformed line is reported rather than quietly skipped" \
    || bad "11 a malformed line vanished from the report"

# --- 12. an empty log says so instead of printing an empty table -----------
D="$W/d12"; mkdir -p "$D"; : > "$D/phases.tsv"
report "$D" | grep -cE 'none with a complete|nothing has been measured' >/dev/null \
    && ok "12 an empty log says nothing has been measured" \
    || bad "12 an empty log produced a table"

echo
echo "hold-phases-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
