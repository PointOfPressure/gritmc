#!/usr/bin/env bash
# Measurement rig for the village `feature_pool_element` item.
#
# Three verbs, all against the same fixed instrument so every figure this item
# quotes is a delta on one reference set:
#
#   sync            rsync this tree to $RUSTMC_REMOTE_DIR, touch, build release
#   dump  <label>   generate the r=16 window under the default order
#   charge <label> [baseline-isect]
#                   N=8 admissible charge, VANILLA FIRST, against the eight
#                   unmodified references, and the set difference vs a baseline
#   orders <label>  dump all eight distinct orders and build the moved-set, so
#                   the order-stable core can be recomputed after a change
#
# Window: seed 20260722, centre chunk (64,64), r=16, bbox 48 48 80 80
# (blocks x,z 768..1295), full height, 107,053,056 blocks. Threadripper.
#
# `RUSTMC_WORKERS` is kept modest on purpose: the box runs a merge chain and
# other worldgen agents, and nothing here measures wall time.
set -uo pipefail
: "${RUSTMC_REMOTE_DIR:=rustmc-village}"
W=${RUSTMC_WORKERS:-6}
R=~/rp-logs/village
LOCAL=$(cd "$(dirname "$0")/.." && pwd)
REM=\$HOME/$RUSTMC_REMOTE_DIR

case "${1:?verb}" in
sync)
    rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
        --exclude 'data/vanilla' "$LOCAL"/ threadripper:"$RUSTMC_REMOTE_DIR"/ || exit 2
    # rsync -a preserves mtimes and cargo trusts mtimes: without this the
    # release binary can be the previous tree's and the log still names this
    # branch. AGENT-BRIEF, 2026-08-08.
    ssh -n threadripper "cd $REM && find crates tools ci -type f \
        \\( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \\) -print0 | xargs -0 touch"
    ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $REM && \
        cargo build --release -q -p worldgen-dump -p region-differ" || exit 2
    ssh -n threadripper "mkdir -p $R"
    ;;
dump)
    L="${2:?label}"
    ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $REM && \
        rm -rf $R/dump-$L && RUSTMC_WORKERS=$W \
        RUSTMC_VILLAGE_FEATURE_SKIP='${SKIP:-}' ./target/release/worldgen-dump \
        full 20260722 16 $R/dump-$L 64 64 > $R/dump-$L.log 2>&1; echo dump-$L rc=\$?"
    ;;
charge)
    L="${2:?label}"; BASE="${3:-}"
    ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
        D=$REM/target/release/region-differ; \
        for r in a b c d e f g h; do \
            \$D diff \$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-\$r \
                $R/dump-$L/region --bbox 48 48 80 80 --positions > $L-\$r.pos 2>&1; \
            awk 'NF==5 && \$1 ~ /^-?[0-9]+\$/ && \$2 ~ /^-?[0-9]+\$/ && \$3 ~ /^-?[0-9]+\$/ && \$4 ~ /^minecraft:/ {print \$1\"_\"\$2\"_\"\$3\"\\t\"\$4\"\\t\"\$5}' \
                $L-\$r.pos | sort -k1,1 > $L-\$r.tsv; \
        done; \
        cut -f1 $L-a.tsv > $L-isect.txt; \
        for r in b c d e f g h; do cut -f1 $L-\$r.tsv | comm -12 - $L-isect.txt > $L-t && mv $L-t $L-isect.txt; done; \
        join -t\$'\\t' -1 1 -2 1 $L-isect.txt $L-a.tsv > $L-charged.tsv; \
        echo \"=== $L  N=8 admissible charge: \$(wc -l < $L-isect.txt)\"; \
        if [ -n '$BASE' ]; then \
            echo \"  no longer charged: \$(comm -23 $BASE $L-isect.txt | wc -l)\"; \
            echo \"  NEWLY charged    : \$(comm -13 $BASE $L-isect.txt | wc -l)\"; \
        fi"
    ;;
orders)
    L="${2:?label}"
    ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $REM && \
        mkdir -p $R/ord-$L && \
        for o in zx zx_rev zx_zrev reverse spiral spiral_in spawn diag; do \
            d=$R/ord-$L/dump-\$o; rm -rf \$d; \
            RUSTMC_DECORATE_ORDER=\$o RUSTMC_WORKERS=$W \
                RUSTMC_VILLAGE_FEATURE_SKIP='${SKIP:-}' ./target/release/worldgen-dump \
                full 20260722 16 \$d 64 64 > $R/ord-$L/\$o.log 2>&1; \
            $REM/target/release/region-differ diff $R/dump-$L/region \$d/region \
                --bbox 48 48 80 80 --positions > $R/ord-$L/default-vs-\$o.pos 2>&1; \
            echo \"\$o \$(grep -E '^DIFF' $R/ord-$L/default-vs-\$o.pos)\"; \
        done; \
        cd $R/ord-$L && cat default-vs-*.pos | \
            awk 'NF==5 && \$1 ~ /^-?[0-9]+\$/ && \$2 ~ /^-?[0-9]+\$/ && \$3 ~ /^-?[0-9]+\$/ && \$4 ~ /^minecraft:/ {print \$1\"_\"\$2\"_\"\$3}' | \
            sort -u > $R/moved-$L.txt; \
        echo \"moved-set: \$(wc -l < $R/moved-$L.txt)\"; \
        comm -23 $R/$L-isect.txt $R/moved-$L.txt > $R/core-$L.txt; \
        echo \"ORDER-STABLE core: \$(wc -l < $R/core-$L.txt)\""
    ;;
*) echo "unknown verb $1" >&2; exit 2 ;;
esac
