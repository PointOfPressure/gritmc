#!/usr/bin/env bash
# block-kind-census.sh — a POSITIONAL census, per block kind, twin-checked.
#
# `region-differ census` counts; `region-differ diff` says which positions
# disagree and what they hold. Neither answers the question an attribution
# actually asks, which is: **for this one block kind, exactly which positions
# does vanilla have that we do not, and which do we have that vanilla does
# not** — and are two block kinds' answers the same positions.
#
# A count delta cannot tell those apart. `obsidian -26` and `lava +68` are the
# right size and the right sign to be one lava-to-obsidian conversion, and they
# are also exactly what two unrelated defects forty chunks apart look like.
# Set-differencing the positions settles it in one command. This exists because
# the brief for the y=-55 interface said "one command decides whether this is
# one item or two", and there was no such command.
#
# TWIN-CHECKED, because ORACLE rule 6 applies to a per-kind delta exactly as it
# does to a residual: a position is only charged as vanilla-only if BOTH
# vanilla references have the kind there. The two twins' own disagreement is
# printed beside every figure as that kind's local floor, never subtracted.
#
# VANILLA FIRST is not a convention here, it is the argument order: swap the
# region dirs and every "vanilla-only" row becomes an "ours-only" row.
#
# usage: ci/block-kind-census.sh <ours-region-dir> <label> [kind ...]
#   kinds default to the fluid-interface set. Names are matched EXACTLY
#   against the block id with its properties stripped, so `obsidian` does not
#   also count `crying_obsidian` and `lava` does not count `lava_cauldron`.
set -uo pipefail
OURS="${1:?need our region dir}"
LABEL="${2:?need a label}"
shift 2
KINDS=("$@")
[ ${#KINDS[@]} -eq 0 ] && KINDS=(obsidian magma_block lava water cobblestone stone)

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF_A="${REF_A:-$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-a}"
REF_B="${REF_B:-$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-b}"
BBOX="${BBOX:-48 48 80 80}"
OUT="$HOME/rp-logs/kinds-$LABEL"
mkdir -p "$OUT"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"
D=./target/release/region-differ
[ -x "$D" ] || cargo build --release -p region-differ 2>&1 | tail -2

# One census per region dir per kind. `--match` is a substring filter, so it is
# only a cheap prefilter; the exact-name test is the awk below. Sorted on the
# position triple so `comm` can be used on it, which is the whole point.
pos_for() { # <region-dir> <kind> <outfile>
    $D census "$1" --bbox $BBOX --match "$2" --positions 2>/dev/null |
        awk -v k="minecraft:$2" '$4 == k { print $1, $2, $3 }' | sort > "$3"
}

echo "=== block-kind census: $LABEL @ $(date -Is) ==="
echo "bbox $BBOX   vanilla A $REF_A"
echo "                    B $REF_B"
echo "                 ours $OURS"
echo
printf '%-16s %9s %9s %9s %9s %9s %9s\n' \
    kind vanA vanB ours 'A-only' 'ours-only' 'twinfloor'

for K in "${KINDS[@]}"; do
    pos_for "$REF_A" "$K" "$OUT/$K.a"
    pos_for "$REF_B" "$K" "$OUT/$K.b"
    pos_for "$OURS"  "$K" "$OUT/$K.ours"

    # Both twins agree the kind is here, and we do not: the charged set.
    comm -12 "$OUT/$K.a" "$OUT/$K.b" > "$OUT/$K.van"
    comm -23 "$OUT/$K.van" "$OUT/$K.ours" > "$OUT/$K.vanilla-only"
    # We have it and NEITHER twin does: the other direction, charged the same way.
    cat "$OUT/$K.a" "$OUT/$K.b" | sort -u > "$OUT/$K.either"
    comm -23 "$OUT/$K.ours" "$OUT/$K.either" > "$OUT/$K.ours-only"
    # The twins' own disagreement on this kind. Printed, never subtracted.
    comm -3 "$OUT/$K.a" "$OUT/$K.b" | wc -l > "$OUT/$K.floor"

    printf '%-16s %9d %9d %9d %9d %9d %9d\n' "$K" \
        "$(wc -l < "$OUT/$K.a")" "$(wc -l < "$OUT/$K.b")" \
        "$(wc -l < "$OUT/$K.ours")" \
        "$(wc -l < "$OUT/$K.vanilla-only")" \
        "$(wc -l < "$OUT/$K.ours-only")" \
        "$(cat "$OUT/$K.floor")"
done

# The co-location test the count deltas cannot do. For every ordered pair of
# kinds, how many of vanilla's missing-from-us positions of kind X are held by
# us as kind Y. A one-for-one substitution shows up as a full row here and as
# nothing at all in a count table.
echo
echo "=== co-location: vanilla-only <kind> positions, and what WE hold there ==="
for K in "${KINDS[@]}"; do
    N=$(wc -l < "$OUT/$K.vanilla-only")
    [ "$N" -eq 0 ] && continue
    echo "--- vanilla-only $K: $N positions"
    for J in "${KINDS[@]}"; do
        H=$(comm -12 "$OUT/$K.vanilla-only" "$OUT/$J.ours" | wc -l)
        [ "$H" -gt 0 ] && printf '      %6d  ours has %s\n' "$H" "$J"
    done
    echo "    y-histogram:"
    awk '{print $2}' "$OUT/$K.vanilla-only" | sort -n | uniq -c |
        awk '{printf "      y=%-6s %d\n", $2, $1}'
    echo "    chunks:"
    awk '{printf "%d %d\n", ($1>=0?int($1/16):int(($1-15)/16)), ($3>=0?int($3/16):int(($3-15)/16))}' \
        "$OUT/$K.vanilla-only" | sort | uniq -c | sort -rn | head -8 |
        awk '{printf "      (%s,%s) %s\n", $2, $3, $1}'
done

echo
echo "=== the same, for what WE hold that neither twin does ==="
for K in "${KINDS[@]}"; do
    N=$(wc -l < "$OUT/$K.ours-only")
    [ "$N" -eq 0 ] && continue
    echo "--- ours-only $K: $N positions"
    for J in "${KINDS[@]}"; do
        H=$(comm -12 "$OUT/$K.ours-only" "$OUT/$J.van" | wc -l)
        [ "$H" -gt 0 ] && printf '      %6d  vanilla has %s\n' "$H" "$J"
    done
    echo "    y-histogram (top 12):"
    awk '{print $2}' "$OUT/$K.ours-only" | sort -n | uniq -c | sort -rn | head -12 |
        awk '{printf "      y=%-6s %d\n", $2, $1}'
    echo "    chunks (top 8):"
    awk '{printf "%d %d\n", ($1>=0?int($1/16):int(($1-15)/16)), ($3>=0?int($3/16):int(($3-15)/16))}' \
        "$OUT/$K.ours-only" | sort | uniq -c | sort -rn | head -8 |
        awk '{printf "      (%s,%s) %s\n", $2, $3, $1}'
done
echo "=== positions kept under $OUT ==="
