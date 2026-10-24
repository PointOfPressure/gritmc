#!/usr/bin/env bash
# corpus-overlap-sampler-falsify.sh — the sampler that says how many recordings
# were really live at once.
#
#     ./ci/corpus-overlap-sampler-falsify.sh
#
# `docs/CORPUS-LOCK-HOLD.md` §23 and §24 both rest on the concurrent arm having
# actually been concurrent, and this file is what says so rather than the rig
# that started it. It has already been wrong once in the direction that matters:
# its first version parsed `--live-in` after the mandatory arguments, so the
# tree argument arrived empty, `grep -qF ""` matched every process on the box,
# and it reported **three trees live during a serial block**. A sampler that
# answers "yes" to everything is worse than none, because its answer is the one
# you were hoping for.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRIVER="${OVERLAP_DRIVER:-$ROOT/ci/corpus-overlap-sampler.sh}"
W="$(mktemp -d /tmp/overlap-falsify.XXXXXX)"
MARK="overlap-falsify-proc-$$"
trap 'pkill -f "$MARK" 2>/dev/null || true; rm -rf "$W"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

fake_rec() {   # fake_rec <tree> <pidfile>
    bash -c "exec -a 'oracle-runner record 127.0.0.1:1 $1/ci/checks/gameplay --out $1/x.json $MARK' sleep 40" \
        >/dev/null 2>&1 &
    echo $! > "$2"
}

printf '%s\n' "== corpus-overlap-sampler-falsify"
A="$W/tree-alpha"; B="$W/tree-beta"; C="$W/tree-alpha-extra"
mkdir -p "$A" "$B" "$C"

# 1. A tree with a live recorder reads 1; a tree without reads 0. If this
#    cannot tell them apart nothing below means anything.
fake_rec "$A" "$W/pa"; sleep 1
[ "$("$DRIVER" --live-in "$A")" = 1 ] && ok "1 a tree with a live recorder reads 1" \
    || bad "1 a tree with a live recorder read $("$DRIVER" --live-in "$A")"
[ "$("$DRIVER" --live-in "$B")" = 0 ] && ok "1 a tree with no recorder reads 0 while another tree has one" \
    || bad "1 a tree with no recorder read $("$DRIVER" --live-in "$B")"

# 2. THE EMPTY-ARGUMENT FAILURE, which is the one that actually happened. An
#    empty tree must not match everything.
GOT="$("$DRIVER" --live-in "" 2>/dev/null || echo refused)"
if [ "$GOT" = 1 ]; then
    bad "2 an empty tree argument matched a live recorder — grep -qF '' matches everything"
else
    ok "2 an empty tree argument does not report a live recording (got '$GOT')"
fi

# 3. Substring coincidence is not a match. `tree-alpha-extra` contains
#    `tree-alpha`, so a recorder in the LONGER path must not make the SHORTER
#    one read as live... and the converse must hold too.
fake_rec "$C" "$W/pc"; sleep 1
[ "$("$DRIVER" --live-in "$C")" = 1 ] && ok "3 the longer path reads 1 for its own recorder" \
    || bad "3 the longer path did not see its own recorder"
kill "$(cat "$W/pa")" 2>/dev/null; sleep 1
if [ "$("$DRIVER" --live-in "$A")" = 1 ]; then
    bad "3 $A reads live from a recorder that is only in $C — substring collision"
else
    ok "3 a recorder in a path that CONTAINS this one does not count as this one"
fi

# 4. The tabular mode counts live trees and names them in its header.
fake_rec "$A" "$W/pa2"; sleep 1
"$DRIVER" 3 1 "$W/o.tsv" "$A" "$B" >/dev/null 2>&1
HDR="$(head -1 "$W/o.tsv" 2>/dev/null)"
case "$HDR" in
    *"tree-alpha"*"tree-beta"*) ok "4 the header names the trees in order" ;;
    *) bad "4 header was '$HDR'" ;;
esac
ROW="$(awk -F'\t' 'NR==2{print $2"|"$4"|"$5}' "$W/o.tsv" 2>/dev/null)"
[ "$ROW" = "1|1|0" ] && ok "4 a row with one of two trees live counts 1 and marks the right column" \
    || bad "4 row read '$ROW', wanted '1|1|0'"

# 5. It writes nothing into any tree it samples.
if find "$A" "$B" "$C" -type f 2>/dev/null | grep -c . >/dev/null; then
    bad "5 something was written into a sampled tree"
else
    ok "5 nothing was written into any sampled tree"
fi

MISSED=0
if [ -z "${OVERLAP_NO_SWEEP:-}" ]; then
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
        [ -n "$d" ] || { printf '  MISSED  %-30s <-- inject produced nothing\n' "$n"; MISSED=$((MISSED+1)); return; }
        case "$d" in
            STALE|COMMENT-ONLY) printf '  MISSED  %-30s <-- %s\n' "$n" "$d"; MISSED=$((MISSED+1)); return ;;
        esac
        if OVERLAP_DRIVER="$d" OVERLAP_NO_SWEEP=1 "$0" >/dev/null 2>&1; then
            printf '  MISSED  %-30s <-- the suite stayed green with this broken\n' "$n"; MISSED=$((MISSED+1))
        else
            printf '  CAUGHT  %-30s\n' "$n"
        fi
    }
    # The mode dispatch moves back after the mandatory arguments, which is the
    # regression that produced "three trees live" during a serial block.
    check "live-in-parsed-too-late" \
        "$(inject latemode 's/^if \[ "\$\{1:-\}" = "--live-in" \]; then live_in "\$\{2:\?tree\}"; exit 0; fi$/:/m')"
    # Any recorder counts, not the one in this tree.
    check "any-recorder-counts" "$(inject anyrec 's/grep -qF -- "\$t\/"/grep -q ./')"
    # The tree match stops being anchored to a full path.
    # This one is literally "revert the trailing-slash fix", so case 3 must go
    # red for it. An injection that restores a defect the file was changed to
    # remove is the tightest kind there is.
    check "prefix-match-restored" "$(inject prefix 's/grep -qF -- "\$t\/"/grep -qF -- "$t"/')"
fi

echo
echo "=================================================================="
echo "cases: $PASS passed, $FAIL failed"
[ -z "${OVERLAP_NO_SWEEP:-}" ] && echo "injections: $MISSED not caught"
if [ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]; then echo "CORPUS-OVERLAP-SAMPLER-FALSIFY GREEN"
else echo "CORPUS-OVERLAP-SAMPLER-FALSIFY RED"; fi
echo "=================================================================="
exit $(( FAIL > 0 || MISSED > 0 ))
