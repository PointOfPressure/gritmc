#!/usr/bin/env bash
# floors-harvest.sh — the whole N campaign for docs/PARITY.md §2 as ONE batched
# serial job.
#
# A floor is a measurement of vanilla's own scheduler jitter, so every
# reference in one ensemble must be harvested under conditions as alike as this
# box allows: SERIAL, same generator, same order, same jar. Load average is
# recorded per reference because the box is running three other lanes' corpus
# recordings and a floor taken under contention is not comparable to one taken
# on a quiet box — that is a hypothesis this campaign can test rather than
# assume.
#
# Idempotent by refusal: an existing reference directory is skipped, never
# regenerated and never written to. Resuming costs only what is not done.
#
# Ports are allocated from 26101 upward, disjoint from every band already used
# on this box (25590, 257xx, 258xx, 2592x-2594x, 2596x-2598x). One port, once.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT=/home/sayer/grit/work-floors
cd "$ROOT" || exit 2
DEST=/home/sayer/rustmc-floors/oracle-work
LOG=/home/sayer/grit/logs/floors-harvest.log
mkdir -p "$(dirname "$LOG")" "$DEST"

# group : label : port : seed : radius : cx : cz : dim
SPEC="
ow31415926r16:a:26101:31415926:16:64:64:overworld:rows
ow31415926r16:b:26102:31415926:16:64:64:overworld:rows
ow31415926r16:c:26103:31415926:16:64:64:overworld:rows
ow31415926r16:d:26104:31415926:16:64:64:overworld:rows
ow31415926r16:e:26105:31415926:16:64:64:overworld:rows
ow31415926r16:f:26106:31415926:16:64:64:overworld:rows
ow31415926r16:g:26107:31415926:16:64:64:overworld:rows
ow31415926r16:h:26108:31415926:16:64:64:overworld:rows
ow31415926r16:rr-a:26109:31415926:16:64:64:overworld:rows_rev
ow31415926r16:rr-b:26110:31415926:16:64:64:overworld:rows_rev
neth31415926c6464:d:26121:31415926:7:64:64:nether:rows
neth31415926c6464:e:26122:31415926:7:64:64:nether:rows
neth31415926c6464:f:26123:31415926:7:64:64:nether:rows
neth31415926c6464:g:26124:31415926:7:64:64:nether:rows
neth31415926c6464:h:26125:31415926:7:64:64:nether:rows
neth20260722c100100:c:26131:20260722:7:100:100:nether:rows
neth20260722c100100:d:26132:20260722:7:100:100:nether:rows
neth20260722c100100:e:26133:20260722:7:100:100:nether:rows
neth20260722c100100:f:26134:20260722:7:100:100:nether:rows
neth20260722c100100:g:26135:20260722:7:100:100:nether:rows
neth20260722c100100:h:26136:20260722:7:100:100:nether:rows
ow20260722c6464r7:a:26141:20260722:7:64:64:overworld:rows
ow20260722c6464r7:b:26142:20260722:7:64:64:overworld:rows
ow20260722c6464r7:c:26143:20260722:7:64:64:overworld:rows
ow20260722c6464r7:d:26144:20260722:7:64:64:overworld:rows
ow20260722c6464r7:e:26145:20260722:7:64:64:overworld:rows
ow20260722c6464r7:f:26146:20260722:7:64:64:overworld:rows
ow20260722c6464r7:g:26147:20260722:7:64:64:overworld:rows
ow20260722c6464r7:h:26148:20260722:7:64:64:overworld:rows
ow31415926c6464r5:a:26151:31415926:5:64:64:overworld:rows
ow31415926c6464r5:b:26152:31415926:5:64:64:overworld:rows
ow31415926c6464r5:c:26153:31415926:5:64:64:overworld:rows
ow31415926c6464r5:d:26154:31415926:5:64:64:overworld:rows
ow31415926c6464r5:e:26155:31415926:5:64:64:overworld:rows
ow31415926c6464r5:f:26156:31415926:5:64:64:overworld:rows
ow31415926c6464r5:g:26157:31415926:5:64:64:overworld:rows
ow31415926c6464r5:h:26158:31415926:5:64:64:overworld:rows
end20260722c8362:a:26161:20260722:7:83:62:end:rows
end20260722c8362:b:26162:20260722:7:83:62:end:rows
end20260722c8362:c:26163:20260722:7:83:62:end:rows
end20260722c8362:d:26164:20260722:7:83:62:end:rows
end20260722c8362:e:26165:20260722:7:83:62:end:rows
end20260722c8362:f:26166:20260722:7:83:62:end:rows
end20260722c8362:g:26167:20260722:7:83:62:end:rows
end20260722c8362:h:26168:20260722:7:83:62:end:rows
"

{
echo "=== FLOORS N CAMPAIGN @ $(date -Is) ==="
echo "tree: $ROOT  $(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null)"
echo "dest: $DEST"
ok=0; skip=0; fail=0
for line in $SPEC; do
    IFS=: read -r grp lab port seed rad cx cz dim ord <<<"$line"
    out="$DEST/floors-$grp-$lab"
    if [ -d "$out" ]; then echo "SKIP  $grp/$lab  exists"; skip=$((skip+1)); continue; fi
    la=$(cut -d' ' -f1 /proc/loadavg)
    t0=$(date +%s)
    echo "--- $grp/$lab port=$port seed=$seed r=$rad c=$cx,$cz dim=$dim ord=$ord load=$la"
    GEN_ORDER="$ord" "$ROOT/ci/gen-vanilla-region.sh" "$seed" "$rad" "$out" "$port" "$cx" "$cz" "$dim" \
        > "$DEST/floors-$grp-$lab.log" 2>&1
    rc=$?
    n=$(ls "$out"/*.mca 2>/dev/null | wc -l)
    echo "    rc=$rc $(( $(date +%s) - t0 ))s regions=$n load_after=$(cut -d' ' -f1 /proc/loadavg) $(head -1 "$out/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED)"
    if [ "$rc" = 0 ] && [ "$n" -gt 0 ]; then ok=$((ok+1)); else fail=$((fail+1)); fi
done
echo "=== CAMPAIGN DONE ok=$ok skip=$skip fail=$fail @ $(date -Is) ==="
} >> "$LOG" 2>&1
