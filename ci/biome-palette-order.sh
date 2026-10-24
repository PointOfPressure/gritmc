#!/usr/bin/env bash
# biome-palette-order.sh — is anything downstream of `BiomePalette` sensitive
# to WHICH integer a biome gets, rather than only to which biome it is?
#
# # The question, and why it needed an instrument
#
# `surface::BiomeId` is a `u16` interned from the datapack's biome directory in
# `BTreeMap` order. Every use of it that anyone could point at is an equality
# compare, so relabelling the biomes ought to be undetectable. A falsification
# sweep on 2026-08-06 (docs/BENCH.md, "the control that failed") rebuilt the
# palette in reverse expecting exactly that, and reported **4 of 4 region files
# changed**. It named no consumer, and a const then called
# `surface::ORDER_IS_LOAD_BEARING` carried the warning for three days.
#
# A warning nobody can act on is worse than none, so this script exists to
# settle it by measurement rather than by reading — ORACLE rule 9. It did:
# the 4-of-4 does not reproduce, the const is now
# `surface::ORDER_IS_NOT_OBSERVABLE`, and this script is the standing gate
# that keeps the answer true. `docs/DETERMINISM.md` §13.
#
# # Why there are two injections and not one
#
# `reverse` changes two things at once: every id's **numeric value** and every
# pair's **relative order**. On its own it therefore cannot name a consumer
# even when it fires. `shift N` changes only the first — it interns N
# unreachable names ahead of the directory, so biome *i* gets `i + N` and every
# `<` between two real biomes still answers exactly what it did.
#
#   shift moves output                  -> a consumer reads the id's VALUE
#                                          (a hash, an arithmetic mix, or an
#                                          index into a table built somewhere
#                                          other than the palette loop)
#   shift identical, reverse moves      -> a consumer reads the ORDER
#                                          (a comparison, a sort, a lowest-id
#                                          tie-break)
#   neither moves                       -> the 2026-08-06 result does not
#                                          reproduce on this window
#
# # One binary, both arms — which the 2026-08-06 method was not
#
# The order is chosen at run time by `RUSTMC_BIOME_PALETTE_ORDER`, so the
# control and the injection are the **same executable**. Nothing here can be a
# build difference, a stale artefact, or the 2026-08-08 rsync-mtime trap where
# cargo reuses the base's binary and the log names your branch. The price is
# one `std::env::var` per dump, read once outside every loop.
#
# # The control that makes the rest mean anything
#
# The first two arms are both `forward`. If they differ, the dumper is not
# deterministic and no other row in the table means anything, so the script
# exits 3 without comparing the injections. "A gate that cannot fail is not a
# gate" — this is the arm that can.
#
# # The arm the 2026-08-06 sweep did not have
#
# That sweep established determinism across three runs of **one** binary and
# then compared **differently built** binaries. Those are not the same claim,
# and the gap between them is big enough to hold a 4-of-4: a rebuild that
# moved output would look exactly like an injection that moved output, and the
# sweep had no arm that could tell them apart.
#
# Set `PALETTE_ORDER_REBUILD_BINARY` to a second `worldgen-dump` built from
# the **same source** and this runs one more `forward` arm with it. It must
# agree with `fwd-a`. If it does not, the finding is not about biome ids at
# all and every other row is void.
#
# Usage (run it on the box; nothing here compiles or runs on a working tree):
#
#     ./ci/biome-palette-order.sh [radius] [centre-x] [centre-z] [seed]
#
# Defaults reproduce the window the 2026-08-06 claim was made on: seed
# 20260722, r=16, centre (64,64), `RUSTMC_WORKERS=1`, four region files.
#
# Exit codes: 0 all injections undetectable (the premise does not reproduce),
# 1 an injection moved output (a consumer exists — the table says which kind),
# 2 setup/usage, 3 a control failed — determinism, positive, digest or
# rebuild — in which case no other row may be read.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

RADIUS="${1:-16}"
CX="${2:-64}"
CZ="${3:-64}"
SEED="${4:-20260722}"
OUT="${PALETTE_ORDER_OUT:-$ROOT/oracle-work/palette-order}"
DUMP="$ROOT/target/release/worldgen-dump"

# `RUSTMC_WORKERS=1` is not incidental. A byte difference under a worker pool
# would have two candidate explanations and this instrument can only afford
# one; the pool is separately proved output-identical in docs/BENCH.md.
export RUSTMC_WORKERS="${RUSTMC_WORKERS:-1}"

if [ ! -x "$DUMP" ]; then
    echo "no worldgen-dump at $DUMP -- cargo build --release -p worldgen-dump" >&2
    exit 2
fi

mkdir -p "$OUT" || exit 2

run_arm() {
    # Three `local`s, not one. Bash expands every argument of the `local`
    # builtin before it runs any of them, so `local a="$1" b="$a"` reads `a`
    # while it is still unset and `set -u` kills the run.
    local name="$1"
    local value="$2"
    local bin="${3:-$DUMP}"
    local dir="$OUT/$name"
    rm -rf "$dir"
    echo "  == $name  RUSTMC_BIOME_PALETTE_ORDER='$value'  $(basename "$bin")" >&2
    RUSTMC_BIOME_PALETTE_ORDER="$value" \
        "$bin" full "$SEED" "$RADIUS" "$dir" "$CX" "$CZ" \
        > "$OUT/$name.log" 2>&1
    local rc=$?
    if [ $rc -ne 0 ]; then
        echo "ARM $name FAILED rc=$rc -- see $OUT/$name.log" >&2
        tail -20 "$OUT/$name.log" >&2
        return 1
    fi
    return 0
}

# `cmp` every region file of an arm against fwd-a. Echoes "<differ> of <total>".
compare() {
    local arm="$1" n=0 d=0 f b
    for f in "$OUT/fwd-a/region"/*.mca; do
        [ -e "$f" ] || continue
        b="$OUT/$arm/region/$(basename "$f")"
        n=$((n + 1))
        cmp -s "$f" "$b" || d=$((d + 1))
    done
    # A missing arm must not read as agreement.
    if [ "$n" -eq 0 ]; then
        echo "0 of 0"
        return
    fi
    echo "$d of $n"
}

echo "seed $SEED  r=$RADIUS  centre ($CX,$CZ)  RUSTMC_WORKERS=$RUSTMC_WORKERS"

run_arm fwd-a  "forward" || exit 2
run_arm fwd-b  "forward" || exit 2

CTL="$(compare fwd-b)"
if [ "${CTL%% *}" != "0" ]; then
    echo "DETERMINISM CONTROL FAILED: forward against forward is $CTL"
    echo "Nothing else in this run means anything. Not comparing the injections."
    exit 3
fi
echo "  control      forward  vs forward   $CTL region files differ"

run_arm rev    "reverse" || exit 2
run_arm shift1 "shift 1" || exit 2
run_arm shift7 "shift 7" || exit 2
run_arm poison "poison"  || exit 2

REV="$(compare rev)"
S1="$(compare shift1)"
S7="$(compare shift7)"
POI="$(compare poison)"

# THE POSITIVE CONTROL. `poison` answers every biome query with id 0, so it
# must move output; if it does not, this knob is not reaching the generating
# path and every clean row above is a measurement of the instrument.
if [ "${POI%% *}" = "0" ]; then
    echo "  poison       MUST have moved       $POI"
    echo "POSITIVE CONTROL FAILED: the palette knob does not reach the generator."
    echo "No row above may be read. Check RUSTMC_BIOME_PALETTE_ORDER is seen by the dump."
    exit 3
fi

# The digests prove the arms built genuinely different palettes — a second,
# independent way for a dead knob to be caught, and the one that still works
# if a window is simply too small to resolve anything.
digest() { grep -m1 '^BIOME_PALETTE ' "$OUT/$1.log" | sed 's/.*digest=//'; }
D_FWD="$(digest fwd-a)"; D_REV="$(digest rev)"; D_S1="$(digest shift1)"
if [ -z "$D_FWD" ] || [ "$D_FWD" = "$D_REV" ] || [ "$D_FWD" = "$D_S1" ]; then
    echo "PALETTE DIGESTS DID NOT MOVE: fwd=$D_FWD rev=$D_REV shift1=$D_S1"
    echo "The arms built the same palette, so nothing above was injected."
    exit 3
fi

echo "  reverse      order + values moved  $REV"
echo "  shift 1      values moved only     $S1"
echo "  shift 7      values moved only     $S7"
echo "  poison       positive control      $POI  (must be non-zero, and is)"
echo "  digests      fwd=$D_FWD rev=$D_REV shift1=$D_S1"

# The rebuild arm — optional, because it needs a second binary that only the
# caller can build. This is the arm the 2026-08-06 sweep lacked.
if [ -n "${PALETTE_ORDER_REBUILD_BINARY:-}" ]; then
    if [ ! -x "$PALETTE_ORDER_REBUILD_BINARY" ]; then
        echo "PALETTE_ORDER_REBUILD_BINARY is not executable" >&2
        exit 2
    fi
    # VACUOUS rather than passed. Measured 2026-08-09: two `cargo build
    # --release` runs of this tree into different target directories produce a
    # **bit-identical** worldgen-dump. Dumping twice with one file and
    # reporting "0 of 4" would be the harness agreeing with itself — the same
    # shape as `tools/oracle-runner`'s vacuous verdict, where the reference
    # produced nothing under the check's own projection. To make this arm say
    # anything, the second binary has to differ: build it with a different
    # codegen setting, e.g. RUSTFLAGS='-C codegen-units=1'.
    if cmp -s "$PALETTE_ORDER_REBUILD_BINARY" "$DUMP"; then
        echo "  rebuild      VACUOUS -- the two binaries are bit-identical"
        echo "               (cargo is reproducible here; this arm measured nothing)"
    else
        run_arm rebuild "forward" "$PALETTE_ORDER_REBUILD_BINARY" || exit 2
        RB="$(compare rebuild)"
        echo "  rebuild      second binary, same source  $RB"
        if [ "${RB%% *}" != "0" ]; then
            echo "REBUILD CONTROL FAILED: two builds of the same source disagree."
            echo "This is not a biome-id finding. Every row above is void."
            exit 3
        fi
    fi
fi

FAIL=0
for r in "$REV" "$S1" "$S7"; do
    [ "${r%% *}" != "0" ] && FAIL=1
done

if [ "$FAIL" -eq 0 ]; then
    echo "PALETTE-ORDER GREEN: no consumer of BiomeId is sensitive to the id it gets."
    exit 0
fi

echo "PALETTE-ORDER RED: a consumer reads the id itself."
if [ "${S1%% *}" != "0" ] || [ "${S7%% *}" != "0" ]; then
    echo "  shift moved output -> the consumer reads the VALUE, not the order."
else
    echo "  shift is clean and reverse is not -> the consumer reads the ORDER."
fi
exit 1
