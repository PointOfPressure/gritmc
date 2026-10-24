#!/usr/bin/env bash
# order-stage-attribution.sh — which stage is order-dependent?
#
# The brief behind §14 asked which chunk statuses the order dependence appears
# at.  `RUSTMC_DECORATE_ORDER` permutes the decoration stage's traversal and
# nothing else, so the answer *ought* to be "decoration only" — but that is a
# reading of our own source, and ORACLE rule 9 says a reading of code does not
# move a suspect.  It is also exactly the kind of claim that is true until some
# stage quietly caches across chunks, at which point every value test still
# passes and the model of what the knob does is silently wrong.
#
# So: dump the same square through `carvers` under the two orders furthest
# apart in the sweep, and diff.  Anything but zero says the order reaches a
# stage nobody thinks it reaches, and §14's whole account is about the wrong
# stage.
#
# It is also the poison control for the sweep: a run that reported zero because
# `RUSTMC_THROUGH` had silently disabled the traversal would look identical, so
# the same pair is dumped through `decorated` immediately afterwards and must
# come back non-zero on the same command path.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)"
D=./target/release/worldgen-dump
G=./target/release/region-differ
MY=${ORDART_OUT:-$HOME/rp-logs-order/order-work}
mkdir -p "$MY"

for stage in carvers decorated; do
    for order in zx reverse; do
        d="$MY/stage-$stage-$order"
        [ -s "$d/region/r.1.1.mca" ] && { echo "have $stage/$order"; continue; }
        rm -rf "$d"
        RUSTMC_THROUGH="$stage" RUSTMC_DECORATE_ORDER="$order" \
            $D full 20260722 16 "$d" 64 64 > "$d.log" 2>&1
        echo "$stage/$order rc=$?  $(grep -c 'decorate-order:' "$d.log") order lines"
    done
    echo "  $stage: $($G diff "$MY/stage-$stage-zx/region" "$MY/stage-$stage-reverse/region" \
        --bbox 48 48 80 80 | grep -E '^DIFF|^IDENT')"
done
echo "ORDSTAGE DONE"
