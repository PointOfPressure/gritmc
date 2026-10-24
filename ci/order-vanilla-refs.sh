#!/usr/bin/env bash
# order-vanilla-refs.sh — vanilla references generated under DIFFERENT chunk
# orders on the r=16 window, so "is our order dependence larger than vanilla's"
# can be asked with vanilla's order actually varied.
#
# The eight references a..h behind the admissible instrument were all taken at
# GEN_ORDER=rows. They differ from each other only by whatever the server's own
# scheduler did on the day, which is a sample of vanilla's order noise but not a
# controlled variation of its order. AGENT-BRIEF: "a control using only `rows`
# varies which *ascending* row goes first, is blind in the only plane that
# matters, and once wrongly convicted our code."
set -uo pipefail
# OUTPUTS LIVE OUTSIDE $RUSTMC_REMOTE_DIR. `ci/t0-remote.sh --sync` rsyncs the
# working tree with `--delete`, so anything written under the remote dir is
# deleted by the next sync from the laptop -- which happened here on 2026-08-10
# and cost three vanilla references and ten dumps mid-run.
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)"
OUT=${ORDREF_OUT:-$HOME/rp-logs-order/order-refs}
mkdir -p "$OUT"
run() {  # <name> <GEN_ORDER> <port>
    local name="$1" order="$2" port="$3"
    if [ -s "$OUT/$name/r.1.1.mca" ]; then echo "have $name"; return 0; fi
    rm -rf "$OUT/$name"
    GEN_ORDER="$order" ./ci/gen-vanilla-region.sh 20260722 16 "$OUT/$name" \
        "$port" 64 64 overworld > "$OUT/$name.log" 2>&1
    echo "$name ($order) rc=$? $(ls "$OUT/$name" 2>/dev/null | wc -l) region files"
}
run rowsrev-a rows_rev 25941
run rowsrev-b rows_rev 25942
run rows-i    rows     25943
echo "ORDREFS DONE"
