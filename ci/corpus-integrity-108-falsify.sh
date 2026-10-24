#!/usr/bin/env bash
# corpus-integrity-108-falsify.sh — the driver from `ci/corpus-integrity-108.sh`
# never boots a server here. Every case runs it against STUB trees whose
# selfcheck writes a recording and a trace of its own environment, so the
# structure of the experiment can be checked in seconds rather than in five
# hours.
#
#     ./ci/corpus-integrity-108-falsify.sh
#
# The three that matter:
#
#   4  a first invocation that records nothing must STOP the run. The real
#      driver's first smoke test returned 127 from every invocation because
#      `cargo` was not on `PATH`, finished in eleven seconds, and printed a
#      complete-looking block structure with empty arms underneath it;
#   6  every port this rig can pick must be outside `pick_port_pair`'s
#      26000-26999 AND outside `ci/corpus-exclusion-experiment.sh`'s
#      27100-27301, or a production tree or a sibling rig collides with it;
#   8  each tree must get its OWN private `RUSTMC_CORPUS_LOCK_DIR`. If two
#      trees shared one, the concurrent arm would serialise itself and the
#      experiment would measure nothing while looking perfect. If they used the
#      default, this rig would take slots in the production queue.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The driver under test. The injection sweep at the bottom re-invokes THIS
# script with a patched copy, which is what makes every case above falsifiable
# without restructuring any of them into functions.
DRIVER="${INT108_DRIVER:-$ROOT/ci/corpus-integrity-108.sh}"
W="$(mktemp -d /tmp/int108-falsify.XXXXXX)"
trap 'rm -rf "$W"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# A scratch root holding the driver and a corpus-lock.sh stub whose `status`
# output the driver greps for scratch-tree refusal.
mkroot() {  # mkroot <name> [status-text]
    local n="$1" st="${2:-  state: FREE}"
    local R="$W/root-$n"
    mkdir -p "$R/ci"
    cp "$DRIVER" "$R/ci/corpus-integrity-108.sh"
    printf '#!/usr/bin/env bash\n[ "${1:-}" = status ] && cat <<%s\n%s\n%s\nexit 0\n' EOFX "$st" EOFX \
        > "$R/ci/corpus-lock.sh"
    chmod +x "$R/ci/corpus-lock.sh"
    echo "$R"
}

# A stub tree: 108 checks, a fake recorder that answers `recorder-id`, and a
# selfcheck that writes SELFCHECK_RUNS recordings and a line of its own
# environment so the driver's plumbing can be asserted.
mktree() {  # mktree <name> [recorder-id] [mode: ok|barren]
    local n="$1" rid="${2:-rec:aaaaaaaaaaaaaaaa}" mode="${3:-ok}"
    local T="$W/tree-$n"
    mkdir -p "$T/ci/checks/gameplay" "$T/target/release"
    local i
    for i in $(seq 1 108); do : > "$T/ci/checks/gameplay/$(printf '%03d' "$i")-x.toml"; done
    printf '#!/usr/bin/env bash\n[ "${1:-}" = recorder-id ] && echo "%s"\nexit 0\n' "$rid" \
        > "$T/target/release/oracle-runner"
    chmod +x "$T/target/release/oracle-runner"
    cat > "$T/ci/t2-gameplay-selfcheck.sh" <<STUB
#!/usr/bin/env bash
printf '%s\t%s\t%s\t%s\t%s\n' "\$SELFCHECK_WORK" "\$SELFCHECK_PORT_BASE" \\
    "\$RUSTMC_CORPUS_LOCK_DIR" "\$CORPUS_LOCK_ANCHOR_NAME" "\$(date +%s%N)" >> "$W/trace"
mkdir -p "\$SELFCHECK_WORK"
if [ "$mode" = barren ]; then exit 9; fi
for i in \$(seq 1 "\$SELFCHECK_RUNS"); do
    mkdir -p "\$SELFCHECK_WORK/r\$i"
    echo "world data" > "\$SELFCHECK_WORK/r\$i/level.dat"
    echo "{\\"scenarios\\":[\$RANDOM]}" > "\$SELFCHECK_WORK/r\$i.json"
    echo "recorder=$rid" > "\$SELFCHECK_WORK/r\$i.json.prov"
done
sleep 0.3
STUB
    chmod +x "$T/ci/t2-gameplay-selfcheck.sh"
    echo "$T"
}

say() { printf '%s\n' "$*"; }
say "== corpus-integrity-108-falsify"

R="$(mkroot plain)"
A="$(mktree a)"; B="$(mktree b)"; C="$(mktree c)"

# 1. Refusals on the trees themselves.
"$R/ci/corpus-integrity-108.sh" 1 "$W/o1" /nonexistent "$B" >"$W/l1" 2>&1
grep -q "is not a directory" "$W/l1" && ok "1 refuses a tree that is not a directory" \
    || bad "1 accepted a nonexistent tree: $(tail -1 "$W/l1")"

D="$(mktree d)"; rm "$D/target/release/oracle-runner"
"$R/ci/corpus-integrity-108.sh" 1 "$W/o2" "$D" "$B" >"$W/l2" 2>&1
grep -q "no built oracle-runner" "$W/l2" && ok "1 refuses a tree with no oracle-runner" \
    || bad "1 accepted a tree with no oracle-runner"

E="$(mktree e)"; rm "$E/ci/checks/gameplay/"1*.toml
"$R/ci/corpus-integrity-108.sh" 1 "$W/o3" "$E" "$B" >"$W/l3" 2>&1
grep -q "which is not the corpus this measures" "$W/l3" \
    && ok "1 refuses a tree holding a subset rather than the corpus" \
    || bad "1 accepted a tree with fewer than 100 checks"

"$R/ci/corpus-integrity-108.sh" 1 "$W/o4" "$A" "$A" >"$W/l4" 2>&1
grep -q "are the same directory" "$W/l4" && ok "2 refuses the same tree twice" \
    || bad "2 accepted the same tree in two slots"

# 3. A tree the REAL lock knows about is a queued tree, not a scratch one.
RQ="$(mkroot queued "  record:  run pid 1 on box in $A since 2026-08-18T00:00:00+01:00")"
"$RQ/ci/corpus-integrity-108.sh" 1 "$W/o5" "$A" "$B" >"$W/l5" 2>&1
grep -q "it is in the queue, not scratch" "$W/l5" \
    && ok "3 refuses a tree that appears in the real corpus lock's status" \
    || bad "3 ran against a tree that is in the production queue"

# 4. Fail fast when the first invocation records nothing.
BR="$(mktree br "rec:aaaaaaaaaaaaaaaa" barren)"
"$R/ci/corpus-integrity-108.sh" 2 "$W/o6" "$BR" "$B" >"$W/l6" 2>&1
RC=$?
if [ "$RC" -eq 5 ] && grep -q "REFUSING to continue" "$W/l6"; then
    ok "4 a first invocation that recorded nothing stopped the run (rc=5)"
else
    bad "4 first invocation was barren and the run carried on (rc=$RC)"
fi

# 5. A LATER barren invocation is reported, not fatal, and the exit says so.
:> "$W/trace"
A2="$(mktree a2)"; B2="$(mktree b2 "rec:aaaaaaaaaaaaaaaa" barren)"
"$R/ci/corpus-integrity-108.sh" 1 "$W/o7" "$A2" "$B2" >"$W/l7" 2>&1
RC=$?
if [ "$RC" -eq 6 ] && grep -q "produced no recording" "$W/l7"; then
    ok "5 a later barren invocation is named and the run exits 6, keeping what it had"
else
    bad "5 a barren concurrent invocation was not reported (rc=$RC)"
fi
[ -s "$W/o7/rec/serial/b1s-r1.json" ] && ok "5 the recordings taken before it were kept" \
    || bad "5 the good recordings were discarded along with the bad invocation"

# 6. PORTS. Every base this rig can hand out, for any number of trees, must
#    miss production's derived range and the sibling rig's fixed one.
:> "$W/trace"
F="$(mktree f)"
"$R/ci/corpus-integrity-108.sh" 1 "$W/o8" "$A" "$B" "$C" "$F" >"$W/l8" 2>&1
BADPORT=""
while IFS=$'\t' read -r _ pb _ _ _; do
    [ -n "$pb" ] || continue
    # the selfcheck uses PORT_BASE + run - 1, so check the whole span it can use
    for off in 0 1 2 3; do
        p=$((pb + off))
        if [ "$p" -ge 26000 ] && [ "$p" -le 26999 ]; then BADPORT="$BADPORT $p(pick_port_pair)"; fi
        if [ "$p" -ge 27100 ] && [ "$p" -le 27301 ]; then BADPORT="$BADPORT $p(exclusion-rig)"; fi
    done
done < "$W/trace"
[ -z "$BADPORT" ] && ok "6 every port base is clear of 26000-26999 and 27100-27301" \
    || bad "6 ports collide:$BADPORT"

# 7. Arms alternate, and the serial tree rotates so no tree is only ever serial.
"$R/ci/corpus-integrity-108.sh" 3 "$W/o9" "$A" "$B" "$C" >"$W/l9" 2>&1
ORDER="$(grep -oE '^== block [0-9]+ (serial|conc[0-9]+)' "$W/l9" | awk '{print $4}' | tr '\n' ' ')"
[ "$ORDER" = "serial conc3 serial conc3 serial conc3 " ] \
    && ok "7 the arms alternate: $ORDER" || bad "7 arm order was '$ORDER'"
SERIALTREES="$(awk -F'\t' '$2=="serial"{print $4}' "$W/o9/runs.tsv" | sort -u | wc -l)"
[ "$SERIALTREES" -eq 3 ] && ok "7 the serial arm rotated across all 3 trees" \
    || bad "7 the serial arm used $SERIALTREES of 3 trees, so tree is confounded with arm"

# 8. PRIVATE LOCK NAMESPACES, one per tree, none of them the production one.
NS="$(awk -F'\t' '{print $3}' "$W/trace" | sort -u)"
NNS="$(printf '%s\n' "$NS" | wc -l)"
if printf '%s\n' "$NS" | grep -c "^$HOME/.rustmc-corpus-lock$" >/dev/null; then
    bad "8 an invocation used the PRODUCTION lock namespace — it would take a queue slot"
elif [ "$NNS" -ge 3 ]; then
    ok "8 each tree used its own private lock namespace ($NNS distinct, none of them production)"
else
    bad "8 only $NNS distinct lock namespaces for 3 trees — the concurrent arm serialises itself"
fi

# 9. Concurrency is real: the three invocations of a concurrent block overlap.
STARTS="$(awk -F'\t' 'END{print NR}' "$W/trace")"
CONC="$(awk -F'\t' '{print $5}' "$W/trace" | sort -n | awk 'NR>1{d=$1-p; if(d<300000000) c++} {p=$1} END{print c+0}')"
[ "$CONC" -ge 3 ] && ok "9 concurrent invocations started together ($CONC near-simultaneous starts of $STARTS)" \
    || bad "9 no two invocations started within 0.3s of each other, so nothing ran concurrently"

# 10. Recordings are copied out with their stamps; worlds are removed.
[ -s "$W/o9/rec/conc3/b1c0-r1.json" ] && [ -s "$W/o9/rec/conc3/b1c0-r1.json.prov" ] \
    && ok "10 recordings are copied out with their provenance stamps" \
    || bad "10 a recording or its stamp did not reach the outdir"
LEFT="$(find "$A" "$B" "$C" -name level.dat 2>/dev/null | wc -l)"
[ "$LEFT" -eq 0 ] && ok "10 the vanilla worlds were removed after the recordings were taken" \
    || bad "10 $LEFT world(s) left behind — a 108-scenario run leaves hundreds of megabytes each"

# 11. A heterogeneous set of recorders is REPORTED. §17.3's homogeneity control
#     is a claim about the trees, and a claim nobody checks is the class of
#     defect this repo has most of.
G="$(mktree g "rec:bbbbbbbbbbbbbbbb")"
"$R/ci/corpus-integrity-108.sh" 1 "$W/o10" "$A" "$G" >"$W/l10" 2>&1
grep -q "do NOT share one recorder identity" "$W/l10" \
    && ok "11 trees with different recorders are called out, not silently averaged" \
    || bad "11 two different recorder identities passed without a word"

# ===========================================================================
# The injection sweep.
#
# Sixteen green cases prove nothing on their own: this repo's most common defect
# is a check that cannot fail, and it has found fourteen of them in the gameplay
# corpus alone. So each property above is broken on purpose in a copy of the
# driver, and THIS SCRIPT IS RE-RUN AGAINST THE COPY. A break that leaves the
# suite green is a property nobody is actually checking.
#
# Re-invoking the whole file rather than one case is deliberate: it is coarse,
# and it needs no case to be restructured into a function, so there is no
# version of these assertions that runs only in the sweep.
# ===========================================================================
MISSED=0
if [ -z "${INT108_NO_SWEEP:-}" ]; then
    echo
    echo "== injections (each MUST turn this suite red)"
    inject() {  # inject <name> <perl-expr> -> patched path | STALE | COMMENT-ONLY
        local n="$1" e="$2"
        local out="$W/drv.$n.sh"
        cp "$DRIVER" "$out"
        perl -0pi -e "$e" "$out"
        if cmp -s "$DRIVER" "$out"; then echo STALE; return; fi
        # A patch that rewrote only comments has broken nothing, and reporting
        # the resulting green as a MISS accuses correct code of a hole.
        if diff <(grep -vE '^\s*#' "$DRIVER") <(grep -vE '^\s*#' "$out") >/dev/null; then
            echo COMMENT-ONLY; return
        fi
        echo "$out"
    }
    check() {   # check <name> <patched-path>
        local n="$1" d="$2"
        # An empty path would make the re-invocation fall back to the REAL
        # driver via its own default and report a green as a MISS.
        [ -n "$d" ] || { printf '  MISSED  %-34s <-- inject produced nothing\n' "$n"; MISSED=$((MISSED+1)); return; }
        case "$d" in
            STALE)        printf '  MISSED  %-34s <-- the patch did not apply; it is stale, not the code\n' "$n"; MISSED=$((MISSED+1)); return ;;
            COMMENT-ONLY) printf '  MISSED  %-34s <-- the patch hit only comments\n' "$n"; MISSED=$((MISSED+1)); return ;;
        esac
        if INT108_DRIVER="$d" INT108_NO_SWEEP=1 "$0" >/dev/null 2>&1; then
            printf '  MISSED  %-34s <-- the suite stayed green with this broken\n' "$n"; MISSED=$((MISSED+1))
        else
            printf '  CAUGHT  %-34s\n' "$n"
        fi
    }
    # 1. All trees share one lock namespace: the concurrent arm serialises
    #    itself and measures nothing while looking perfect.
    check "shared-lock-namespace" \
        "$(inject sharedns 's/RUSTMC_CORPUS_LOCK_DIR="\$OUT\/lock-\$IDX"/RUSTMC_CORPUS_LOCK_DIR="$OUT\/lock-shared"/')"
    # 2. The production lock namespace: the rig would take real queue slots.
    check "production-lock-namespace" \
        "$(inject prodns 's/RUSTMC_CORPUS_LOCK_DIR="\$OUT\/lock-\$IDX"/RUSTMC_CORPUS_LOCK_DIR="$HOME\/.rustmc-corpus-lock"/')"
    # 3. Ports back inside pick_port_pair's derived range.
    check "ports-inside-26000-26999" "$(inject ports 's/^PORT_FIRST=27600$/PORT_FIRST=26400/m')"
    # 4. The arms stop alternating: one block of each, time of day confounded.
    check "arms-do-not-alternate" \
        "$(inject noalt 's/    SERIAL_TREE=\$\(\( \(b - 1\) % \$\{#TREES\[@\]\} \)\)/    SERIAL_TREE=0/')"
    # 5. A barren invocation stops being noticed.
    check "barren-not-detected" "$(inject nobarren 's/if \[ "\$KEPT" -eq 0 \]; then/if false; then/')"
    # 6. The worlds are left behind: hundreds of megabytes per run.
    check "worlds-not-removed" "$(inject noclean 's/^        rm -rf "\$SW\/r\$i"$/        : keep/m')"
    # 7. A mixed set of recorders passes without a word, so §17.3's homogeneity
    #    control becomes an assertion nobody checked.
    check "recorder-mismatch-silent" "$(inject noreid 's/-ne 1 \]; then/-eq -1 ]; then/')"
    # 8. The stamps stop being copied out, so corpus-floor has to group by
    #    whatever binary is in the tree at read time.
    check "prov-not-copied" "$(inject noprov 's/            cp -f "\$SW\/r\$i.json.prov" "\$OUT\/rec\/\$ARM\/\$TAG-r\$i.json.prov" 2>\/dev\/null \|\| true/            :/')"
fi

echo
echo "=================================================================="
echo "cases: $PASS passed, $FAIL failed"
[ -z "${INT108_NO_SWEEP:-}" ] && echo "injections: $MISSED not caught"
if [ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]; then echo "CORPUS-INTEGRITY-108-FALSIFY GREEN"
else echo "CORPUS-INTEGRITY-108-FALSIFY RED"; fi
echo "=================================================================="
exit $(( FAIL > 0 || MISSED > 0 ))
