#!/usr/bin/env bash
# be-channel-identical.sh — the gate on the block-entity side channel.
#
# WHAT THIS REFUSES.
#
# `docs/DETERMINISM.md` §42 builds a write path out of generation: a feature
# records what it knew at a position, and the emitter reads that back at save
# time. The whole soundness argument for it is one sentence — **there is no
# read path back in** — and the argument is worth nothing as an assertion,
# because a producer that took one extra draw while recording would break it
# without a single line of code saying so.
#
# So it is measured. The same window is generated twice by the same binary,
# once with `RUSTMC_RECORD_BLOCK_ENTITIES=1` and once without, and the two
# region sets must be **identical in their block states**. If recording moves
# a block, the channel is wrong no matter how good the payloads look, and this
# exits non-zero.
#
# WHY THE COMPARISON IS OURS AGAINST OURS, AND WHY THAT IS NOT THE USUAL RULE.
#
# Every other differ call in this repo puts vanilla first, because the sign of
# the number means something. Here both sides are ours and the only admissible
# answer is zero, so there is no sign to get wrong. What there IS to get wrong
# is comparing a run against itself, and that is what the positive control
# below exists for.
#
# THE POSITIVE CONTROL — WITHOUT IT THIS GATE CANNOT FAIL.
#
# Two runs that both had recording off would agree perfectly and prove nothing.
# `ci/t0.sh`'s standing question is "what would have had to happen for this to
# come back red", and the honest answer here has to include "the knob turned".
# So the recording arm is required to have written block entities the
# non-recording arm did not: `region-differ becensus --match mob_spawner`
# must be **zero on the plain arm and non-zero on the recording arm**. A
# window with no recordable block entity in it is refused as VACUOUS rather
# than reported as green — the same verdict `tools/oracle-runner` gives a
# check whose reference produced nothing.
#
# THE WINDOW is small on purpose: centre chunk (61,50) at r=4, which is 81
# chunks and about a minute, and which contains the seed-12345 dungeon whose
# spawner sits at (982,-22,803) — read out of `gen-wide-12345-r16-ow-c` in
# §42.1. A gate that costs an hour is a gate people skip. The r=16 window is
# the one the *figures* come from; this one only has to contain a producer.
#
# NEEDS NO CORPUS LOCK and no vanilla boot: two dumps of our own binary.
#
# Its own falsifier is `ci/be-channel-falsify.sh`, which reconstructs the
# defect this refuses — a recorder that spends a draw — and asserts this
# script goes red on it.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

SEED=${SEED:-12345}
CX=${CX:-61}
CZ=${CZ:-50}
R=${R:-4}
# The dungeon this window exists to contain — chunk (61,50) at seed 12345.
BBOX=${BBOX:-"$((CX - R)) $((CZ - R)) $((CX + R)) $((CZ + R))"}
# Every class a producer in this tree records, not just one. A control that
# watches `mob_spawner` alone goes green while the chest arm is dead, and a
# dead arm moves no block state — so the class list IS the coverage of this
# control and it grows with the arms.
MATCH=${MATCH:-"mob_spawner chest"}
OUT=${OUT:-$ROOT/oracle-work/be-channel-identical}
DUMP="$ROOT/target/release/worldgen-dump"
DIFFER="$ROOT/target/release/region-differ"

for bin in "$DUMP" "$DIFFER"; do
    if [ ! -x "$bin" ]; then
        echo "REFUSED  $bin is not built — cargo build --release -p worldgen-dump -p region-differ" >&2
        exit 2
    fi
done

rm -rf "$OUT"
mkdir -p "$OUT"

# One dump per arm. `RUSTMC_WORKERS=1` so the two arms cannot differ by thread
# scheduling instead of by the thing under test; the parallel path has its own
# identity question and this gate is not it.
arm() {
    local name=$1 record=$2
    echo "=== arm $name (RUSTMC_RECORD_BLOCK_ENTITIES=$record) ==="
    RUSTMC_WORKERS=1 \
    RUSTMC_RECORD_BLOCK_ENTITIES="$record" \
        "$DUMP" full "$SEED" "$R" "$OUT/$name" "$CX" "$CZ" \
        > "$OUT/$name.out" 2> "$OUT/$name.err"
    local rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "FAIL  arm $name exited $rc"
        tail -20 "$OUT/$name.err"
        exit 1
    fi
}

arm plain 0
arm recording 1

# --- the positive control, before the comparison ------------------------------
#
# Counted per arm rather than differenced, so a zero on the recording arm is
# distinguishable from a zero on both.
count() {  # <arm> <class>
    # `worldgen-dump` writes its region files into `<out>/region`, and the
    # differ takes the directory that HOLDS the .mca files. Pointed one level
    # up it reports `chunks=0` and every count is zero — which this gate then
    # correctly called VACUOUS on its first run rather than green, which is
    # the whole reason the positive control is here.
    # shellcheck disable=SC2086
    "$DIFFER" becensus "$OUT/$1/region" --bbox $BBOX --match "$2" 2>/dev/null \
        | grep -oP 'BECENSUS chunks=[0-9]+ block_entities=\K[0-9]+' | head -1
}
vacuous=""
switched=""
for class in $MATCH; do
    PLAIN_N=$(count plain "$class"); PLAIN_N=${PLAIN_N:-0}
    REC_N=$(count recording "$class"); REC_N=${REC_N:-0}
    echo "positive control: $class  plain=$PLAIN_N  recording=$REC_N"
    [ "$REC_N" -eq 0 ] && vacuous="$vacuous $class"
    [ "$PLAIN_N" -ne 0 ] && switched="$switched $class"
done
if [ -n "$vacuous" ]; then
    echo "VACUOUS  the recording arm wrote no block entity of:$vacuous"
    echo "         — this gate measured nothing for those classes. Either their"
    echo "         producer never fired, or the window contains none of them."
    echo "         Not green. Narrow MATCH deliberately if a class is genuinely"
    echo "         absent from this window, and say so where you narrow it."
    exit 3
fi
if [ -n "$switched" ]; then
    echo "REFUSED  the plain arm wrote block entities of:$switched  with recording"
    echo "         off. The switch is not the switch, so an identical pair of"
    echo "         arms would prove nothing."
    exit 1
fi

# --- the gate ----------------------------------------------------------------
# shellcheck disable=SC2086
"$DIFFER" diff "$OUT/plain/region" "$OUT/recording/region" --bbox $BBOX > "$OUT/diff.txt" 2>&1
MIS=$(grep -oP 'mismatches=\K[0-9]+' "$OUT/diff.txt" | head -1)
if [ -z "$MIS" ]; then
    echo "REFUSED  the differ printed no mismatch count — read $OUT/diff.txt"
    tail -5 "$OUT/diff.txt"
    exit 2
fi
echo "block-state mismatches between the two arms: $MIS"

if [ "$MIS" -ne 0 ]; then
    echo "REFUSED  recording changed generation: $MIS block state(s) moved."
    echo "         The side channel has a read path back into generation, or a"
    echo "         producer spends a draw only while recording. docs/DETERMINISM.md §42.5."
    exit 1
fi

echo "GREEN  seed $SEED centre ($CX,$CZ) r=$R: block states byte-identical with"
echo "       recording on and off, and the recording arm wrote block entities of"
echo "       every class in:$MATCH  where the plain arm wrote none."
