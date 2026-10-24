#!/usr/bin/env bash
# ci/t0-keepgoing-falsify.sh — red-when-broken evidence for ci/t0-stages.sh.
#
# T0_KEEP_GOING=1 is only worth having if it (a) reports EVERY red, (b) never
# turns a red into a green, and (c) leaves the default run exactly as it was.
# Each of those is a case below, over hermetic fixture T0s built from the red
# SHAPES ci/t0.sh actually uses (`|| { ...; exit 1; }`, a bare command under
# errexit, a subshell, `exit` inside a subshell, `if ... exit`, a command
# substitution, a pipeline under pipefail, `exit` in a loop, `case ... exit`,
# and a `set -u` abort, which no trap can intercept).
#
#   A  all green: both modes exit 0 and print T0 GREEN; no red is listed
#   B  each shape red ALONE, in keep-going: non-zero, no T0 GREEN, exactly
#      that one stage listed, and every stage still ran
#   C  each shape red alone, default mode: exit status and every non-timing
#      output line identical to the same fixture with the library replaced
#      by an empty file — i.e. plain `set -euo pipefail` bash
#   D  every shape red at once, keep-going: all of them listed, non-zero
#   E  timing: one T0-STAGE line per stage; a 0.3 s sleep reads >= 0.3 s wall
#      and ~0 CPU; a stage whose CHILD burns CPU reads > 0.3 s cpu_s
#   F  the real ci/t0.sh sources the library before its first check and calls
#      the verdict guard immediately before it can print T0 GREEN; its real
#      verdict tail, appended to a red fixture, does not print T0 GREEN
#   P  prefetch: a spawned job's stdout, stderr and status are replayed at its
#      call site; it ran in a snapshot; a miss runs inline; leftovers die at
#      exit; T0_PREFETCH=0 is inline; a prefetched red is judged as before
#   T  tree integrity: an unrestored edit is a named T0 RED in both modes;
#      a restored one, a new file and T0's own in-tree log are not
#   L  a second T0 in one tree is refused while the first runs
#   M  MUTANTS: ten broken copies of the library, each of which this script
#      must reject. A falsifier that passes a broken library is itself a check
#      that cannot fail, so this section is not optional.
#
# No build, no box, no lock; ~5 s. Exit 0 green, 1 red.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB="$ROOT/ci/t0-stages.sh"
T0="$ROOT/ci/t0.sh"
MUTANTS=1
QUIET=""
while [ $# -gt 0 ]; do
    case "$1" in
        --lib) LIB="$2"; shift 2 ;;
        --no-mutants) MUTANTS=0; shift ;;
        --quiet) QUIET=1; shift ;;
        *) echo "usage: $0 [--lib FILE] [--no-mutants] [--quiet]" >&2; exit 2 ;;
    esac
done
unset T0_KEEP_GOING T0_STAGE_LOG T0_PREFETCH
WORK="$(mktemp -d "${TMPDIR:-/tmp}/t0kg.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); [ -n "$QUIET" ] || echo "ok   $*"; }
bad()  { FAIL=$((FAIL+1)); echo "FAIL $*"; }

SHAPES=(orgroup bare subshell subexit ifexit cmdsub pipefail loopexit caseexit nounset)
N=${#SHAPES[@]}

# stage_body SHAPE RED -> the stage's code (one stage = comment + code)
stage_body() {
    local s=$1 r=$2
    case "$s" in
        orgroup)  if [ "$r" = 1 ]; then echo 'false || { echo "T0 RED — orgroup" >&2; exit 1; }'
                  else echo 'true || { echo "T0 RED — orgroup" >&2; exit 1; }'; fi ;;
        bare)     if [ "$r" = 1 ]; then echo 'false'; else echo 'true'; fi ;;
        subshell) if [ "$r" = 1 ]; then echo '( cd /; false; echo in-sub-after-false )'
                  else echo '( cd /; true )'; fi ;;
        subexit)  if [ "$r" = 1 ]; then echo '( exit 3 )'; else echo '( exit 0 )'; fi ;;
        ifexit)   printf 'if [ %s = 1 ]; then\n    echo "T0 RED — ifexit" >&2\n    exit 2\nfi\n' "$r" ;;
        cmdsub)   if [ "$r" = 1 ]; then echo 'v=$(false)'; else echo 'v=$(true)'; fi ;;
        pipefail) if [ "$r" = 1 ]; then echo 'false | cat'; else echo 'true | cat'; fi ;;
        loopexit) printf 'for i in 1 2 3; do\n    if [ "$i" = 2 ] && [ %s = 1 ]; then echo "T0 RED — loop" >&2; exit 1; fi\ndone\n' "$r" ;;
        caseexit) printf 'case %s in\n    1) echo "T0 RED — case" >&2; exit 2 ;;\nesac\n' "$r" ;;
        nounset)  if [ "$r" = 1 ]; then echo 'echo "${T0KG_SURELY_UNSET_VAR}"'
                  else echo 'echo "${T0KG_SURELY_UNSET_VAR:-}"'; fi ;;
    esac
}

# mkfix DIR LIBFILE "red shapes..." [tailfile]
mkfix() {
    local d=$1 lib=$2 reds=" $3 " tail=${4:-} s r
    mkdir -p "$d/ci"
    cp "$lib" "$d/ci/t0-stages.sh"
    {
        printf '#!/usr/bin/env bash\nset -euo pipefail\ncd "$(dirname "$0")/.."\n. ci/t0-stages.sh\n'
        for s in "${SHAPES[@]}"; do
            r=0; case "$reds" in *" $s "*) r=1 ;; esac
            printf '\n# stage %s\necho "RAN-%s"\n' "$s" "$s"
            stage_body "$s" "$r"
        done
        # Two GREEN controls a too-eager keep-going would red: plain bash
        # clears errexit inside $( ), and suspends it inside a subshell on the
        # left of ||, so neither of these is a red in a default run.
        printf '\n# control: errexit is cleared in a command substitution\nv=$(false; echo x)\n'
        printf '\n# control: errexit is suspended left of ||\n( false; true ) || { echo "T0 RED — control" >&2; exit 1; }\n'
        if [ -n "${TIMING:-}" ]; then
            printf '\n# a stage that only waits\nsleep 0.3\n'
            printf '\n# a stage whose child burns CPU\nawk '"'"'BEGIN{for(i=0;i<10000000;i++)x+=i}'"'"'\n'
        fi
        if [ -n "$tail" ]; then printf '\n'; cat "$tail"
        else printf '\n# verdict\nt0_stages_verdict_guard\necho "T0 GREEN"\n'; fi
    } > "$d/ci/t0.sh"
    chmod +x "$d/ci/t0.sh"
}

# run DIR MODE -> sets RC, OUT (stdout+stderr)
run() {
    local d=$1 kg=$2
    OUT=$(T0_KEEP_GOING=$kg bash "$d/ci/t0.sh" 2>&1 </dev/null); RC=$?
}
stage_line() { grep -n "^# stage $2\$" "$1/ci/t0.sh" | cut -d: -f1 | awk '{print $1+1}'; }
listed()     { printf '%s\n' "$OUT" | grep -E '^  RED  stage line=' | sed -E 's/.*stage line=([0-9?]+).*/\1/' | sort -n | tr '\n' ' '; }

# --- A ----------------------------------------------------------------------
TIMING=1 mkfix "$WORK/a" "$LIB" ""
for kg in 0 1; do
    run "$WORK/a" $kg
    if [ "$RC" = 0 ] && printf '%s\n' "$OUT" | grep -cx 'T0 GREEN' >/dev/null \
       && ! printf '%s\n' "$OUT" | grep -c 'KEEP-GOING' >/dev/null; then ok "A all-green, keep_going=$kg: rc 0, T0 GREEN, nothing listed"
    else bad "A all-green, keep_going=$kg: rc=$RC"; printf '%s\n' "$OUT" | tail -5; fi
done

# --- B and C ----------------------------------------------------------------
: > "$WORK/empty.sh"
for s in "${SHAPES[@]}"; do
    mkfix "$WORK/b-$s" "$LIB" "$s"
    want="$(stage_line "$WORK/b-$s" "$s") "
    run "$WORK/b-$s" 1
    ran=$(printf '%s\n' "$OUT" | grep -c '^RAN-')
    if [ "$RC" != 0 ] && ! printf '%s\n' "$OUT" | grep -cx 'T0 GREEN' >/dev/null && [ "$(listed)" = "$want" ] \
       && { [ "$ran" = "$N" ]; }; then
        ok "B $s red alone, keep-going: rc=$RC, listed line $want, $ran/$N stages ran"
    else
        bad "B $s red alone, keep-going: rc=$RC listed='$(listed)' want='$want' ran=$ran/$N green=$(printf '%s\n' "$OUT" | grep -cx 'T0 GREEN')"
    fi
    # C: default mode must be plain bash, byte for byte, bar the timing lines
    run "$WORK/b-$s" 0; rc_lib=$RC; out_lib=$(printf '%s\n' "$OUT" | grep -v '^T0-STAGE' | sed "s#$WORK/b-#DIR-#g")
    mkfix "$WORK/c-$s" "$WORK/empty.sh" "$s"
    sed -i 's/^t0_stages_verdict_guard$/:/' "$WORK/c-$s/ci/t0.sh"
    run "$WORK/c-$s" 0; rc_bare=$RC; out_bare=$(printf '%s\n' "$OUT" | sed "s#$WORK/c-#DIR-#g")
    if [ "$rc_lib" = "$rc_bare" ] && [ "$out_lib" = "$out_bare" ] && [ "$rc_lib" != 0 ]; then
        ok "C $s red, default mode: rc $rc_lib and output identical to plain bash"
    else
        bad "C $s red, default mode: rc lib=$rc_lib bare=$rc_bare, output $( [ "$out_lib" = "$out_bare" ] && echo same || echo DIFFERS)"
    fi
done

# --- D ----------------------------------------------------------------------
mkfix "$WORK/d" "$LIB" "${SHAPES[*]}"
run "$WORK/d" 1
want=""; for s in "${SHAPES[@]}"; do want="$want$(stage_line "$WORK/d" "$s") "; done
want=$(printf '%s\n' $want | sort -n | tr '\n' ' ')
if [ "$RC" != 0 ] && [ "$(listed)" = "$want" ] && ! printf '%s\n' "$OUT" | grep -cx 'T0 GREEN' >/dev/null; then
    ok "D all $N shapes red at once: all $N listed, rc=$RC"
else bad "D all red: rc=$RC listed='$(listed)' want='$want'"; fi

# --- E ----------------------------------------------------------------------
run "$WORK/a" 0
nst=$(printf '%s\n' "$OUT" | grep -c '^T0-STAGE line=')
want_st=$(( N + 4 ))   # the shapes, the $( ) control (the ( ) one opens with "(" and merges into it), the wait, the burn, the verdict
if [ "$nst" = "$want_st" ]; then ok "E one T0-STAGE line per stage ($nst)"; else bad "E $nst T0-STAGE lines, want $want_st"; fi
sl=$(printf '%s\n' "$OUT" | grep '^T0-STAGE' | grep 'label="sleep 0.3"')
bl=$(printf '%s\n' "$OUT" | grep '^T0-STAGE' | grep 'label="awk ')
sw=$(sed -E 's/.*wall_s=([0-9.]+).*/\1/' <<<"$sl"); sc=$(sed -E 's/.*cpu_s=([0-9.]+).*/\1/' <<<"$sl")
bc=$(sed -E 's/.*cpu_s=([0-9.]+).*/\1/' <<<"$bl")
if awk -v w="$sw" -v c="$sc" 'BEGIN{exit !(w>=0.3 && c<0.1)}'; then ok "E waiting stage: wall ${sw}s, cpu ${sc}s"
else bad "E waiting stage: wall='$sw' cpu='$sc' (want wall>=0.3, cpu<0.1)"; fi
if awk -v c="$bc" 'BEGIN{exit !(c>0.1)}'; then ok "E child-CPU stage: cpu ${bc}s counted"
else bad "E child-CPU stage: cpu='$bc' (want >0.3; children's CPU is not being counted)"; fi

# --- F ----------------------------------------------------------------------
if [ -f "$T0" ]; then
    src=$(grep -n '^\. ci/t0-stages\.sh$' "$T0" | head -1 | cut -d: -f1)
    first=$(grep -n -E '^(SHELL_BAD|python3|cargo|bash|\./ci)' "$T0" | head -1 | cut -d: -f1)
    if [ -n "$src" ] && [ -n "$first" ] && [ "$src" -lt "$first" ]; then ok "F ci/t0.sh sources the library (line $src) before its first check (line $first)"
    else bad "F ci/t0.sh does not source ci/t0-stages.sh before its first check (src='$src' first='$first')"; fi
    g=$(grep -n -x 't0_stages_verdict_guard' "$T0" | tail -1 | cut -d: -f1)
    gr=$(grep -n 'echo "T0 GREEN"' "$T0" | tail -1 | cut -d: -f1)
    between=$( [ -n "$g" ] && [ -n "$gr" ] && sed -n "$((g+1)),$((gr-1))p" "$T0" | grep -cvE '^\s*(#|$|case |required\))' )
    if [ -n "$g" ] && [ -n "$gr" ] && [ "$g" -lt "$gr" ] && [ "$between" = 0 ]; then ok "F verdict guard (line $g) immediately precedes T0 GREEN (line $gr)"
    else bad "F verdict guard missing or not immediately before T0 GREEN (guard='$g' green='$gr' between='$between')"; fi
    sed -n "${g:-99999},\$p" "$T0" > "$WORK/tail.sh"
    mkfix "$WORK/f" "$LIB" "bare" "$WORK/tail.sh"
    run "$WORK/f" 1
    if [ "$RC" != 0 ] && ! printf '%s\n' "$OUT" | grep -c '^T0 GREEN' >/dev/null; then ok "F real verdict tail after a red, keep-going: rc=$RC and no T0 GREEN"
    else bad "F real verdict tail printed T0 GREEN or exited 0 after a red (rc=$RC)"; fi
else
    bad "F no ci/t0.sh at $T0"
fi

# --- P: prefetch -------------------------------------------------------------
mkpf() {  # dir body-file
    mkdir -p "$1/ci"; cp "$LIB" "$1/ci/t0-stages.sh"
    { printf '#!/usr/bin/env bash\nset -euo pipefail\ncd "$(dirname "$0")/.."\n. ci/t0-stages.sh\n'; cat "$2"; } > "$1/ci/t0.sh"
}
cat > "$WORK/pf-body" <<'PFB'

# spawn
echo before > marker.txt
t0_spawn sh -c 'while [ ! -e GOFILE ]; do sleep 0.05; done; cat marker.txt; echo to-err >&2; echo edited > marker.txt; exit 3'
t0_spawn sh -c 'sleep 30; touch LEAKED'
echo after > marker.txt
touch GOFILE

# judged where it always was
rc=0; t0_prefetched sh -c 'while [ ! -e GOFILE ]; do sleep 0.05; done; cat marker.txt; echo to-err >&2; echo edited > marker.txt; exit 3' > pf.out 2> pf.err || rc=$?
echo "PF-RC=$rc"

# a command never spawned runs inline
t0_prefetched sh -c 'echo inline-ran' > miss.out
PFB
# The job waits for a file OUTSIDE the tree that the fixture touches only after
# it has rewritten marker.txt, so the order is fixed: a job running in the live
# tree would read 'after' (and its edit would land on the tree), never 'before'.
sed -i "s#GOFILE#$WORK/pf-go#g" "$WORK/pf-body"
mkpf "$WORK/pf" "$WORK/pf-body"
( cd "$WORK/pf" && T0_KEEP_GOING=0 bash ci/t0.sh > run.log 2>&1 ); prc=$?
sleep 0.5
if [ "$prc" = 0 ] && grep -qx 'PF-RC=3' "$WORK/pf/run.log" && [ "$(cat "$WORK/pf/pf.out")" = before ] \
   && [ "$(cat "$WORK/pf/pf.err")" = to-err ] && [ "$(cat "$WORK/pf/marker.txt")" = after ] \
   && [ "$(cat "$WORK/pf/miss.out")" = inline-ran ]; then
    ok "P prefetched job: stdout, stderr and rc 3 replayed at the call site; it read and edited its SNAPSHOT (saw 'before', tree still 'after'); a miss runs inline"
else bad "P prefetch replay: rc=$prc $(grep PF-RC "$WORK/pf/run.log") out='$(cat "$WORK/pf/pf.out" 2>/dev/null)' err='$(cat "$WORK/pf/pf.err" 2>/dev/null)' marker='$(cat "$WORK/pf/marker.txt")'"; fi
sleep 1
if [ ! -e "$WORK/pf/LEAKED" ] && ! ls "$WORK/pf/target" 2>/dev/null | grep -c t0-prefetch >/dev/null && ! pgrep -f "sleep 30; touch LEAKED" >/dev/null; then
    ok "P an unconsumed job is killed and its snapshot removed at exit"
else bad "P a prefetched job outlived T0 or left its snapshot"; pkill -f "sleep 30; touch LEAKED"; fi
( cd "$WORK/pf" && rm -f pf.out "$WORK/pf-go" && T0_PREFETCH=0 bash ci/t0.sh > run0.log 2>&1 )
if grep -qx 'PF-RC=3' "$WORK/pf/run0.log" && [ "$(cat "$WORK/pf/pf.out")" = after ]; then
    ok "P T0_PREFETCH=0: the same call site runs inline against the live tree"
else bad "P T0_PREFETCH=0 did not run inline"; fi
printf '\n# red at a bare call site\nt0_spawn false\n\n# judged\nt0_prefetched false\n\n# after\necho REACHED\n' > "$WORK/pf2-body"
mkpf "$WORK/pf2" "$WORK/pf2-body"
( cd "$WORK/pf2" && T0_KEEP_GOING=1 bash ci/t0.sh > run.log 2>&1 ); k=$?
( cd "$WORK/pf2" && bash ci/t0.sh > run0.log 2>&1 ); d=$?
if [ "$k" != 0 ] && grep -q REACHED "$WORK/pf2/run.log" && grep -q '^  RED  stage line=' "$WORK/pf2/run.log" \
   && [ "$d" != 0 ] && ! grep -q REACHED "$WORK/pf2/run0.log"; then
    ok "P a prefetched red at a bare call site: default stops there, keep-going records it and goes on"
else bad "P prefetched red: keep-going rc=$k default rc=$d"; fi

# --- T: tree integrity --------------------------------------------------------
# A stage that edits a file and puts it back is green; one that does not put it
# back is a T0 RED naming the file, in both modes, and never prints T0 GREEN;
# a file the run creates, and T0's own log inside the tree, are not judged.
mktree() {  # dir restore(0|1)
    mkdir -p "$1/ci/checks"; cp "$LIB" "$1/ci/t0-stages.sh"
    printf 'flag = true\n' > "$1/ci/checks/pinned.toml"
    { printf '#!/usr/bin/env bash\nset -euo pipefail\ncd "$(dirname "$0")/.."\n. ci/t0-stages.sh\n'
      printf '\n# a falsifier arm\ncp ci/checks/pinned.toml /tmp/t0kg-bak.$$\nsed -i s/true/false/ ci/checks/pinned.toml\n'
      if [ "$2" = 1 ]; then printf 'cp /tmp/t0kg-bak.$$ ci/checks/pinned.toml\n'; fi
      printf 'rm -f /tmp/t0kg-bak.$$\n'
      printf '\n# a new file is not judged\necho made > made.txt\n'
      printf '\n# verdict\nt0_stages_verdict_guard\necho "T0 GREEN"\n'; } > "$1/ci/t0.sh"
}
mktree "$WORK/t1" 1
( cd "$WORK/t1" && bash ci/t0.sh > own.log 2>&1 ); r0=$?
( cd "$WORK/t1" && T0_KEEP_GOING=1 bash ci/t0.sh > own.log 2>&1 ); r1=$?
if [ "$r0" = 0 ] && [ "$r1" = 0 ] && grep -qx 'T0 GREEN' "$WORK/t1/own.log"; then
    ok "T an edited-and-restored file, a new file and T0's own in-tree log: green in both modes"
else bad "T restoring stage: default rc=$r0 keep-going rc=$r1"; fi
for kg in 0 1; do
    mktree "$WORK/t2-$kg" 0
    ( cd "$WORK/t2-$kg" && T0_KEEP_GOING=$kg bash ci/t0.sh > "$WORK/t2-$kg.log" 2>&1 ); r=$?
    if [ "$r" != 0 ] && ! grep -q '^T0 GREEN' "$WORK/t2-$kg.log" && grep -q 'ci/checks/pinned.toml' "$WORK/t2-$kg.log" \
       && grep -q 'T0 RED — T0 left 1 file' "$WORK/t2-$kg.log"; then
        ok "T a stage that does not restore its edit, keep_going=$kg: T0 RED naming ci/checks/pinned.toml, rc=$r"
    else bad "T unrestored edit, keep_going=$kg: rc=$r green=$(grep -c '^T0 GREEN' "$WORK/t2-$kg.log")"; fi
done

# --- L: one T0 per tree --------------------------------------------------------
mkdir -p "$WORK/l/ci"; cp "$LIB" "$WORK/l/ci/t0-stages.sh"
printf '#!/usr/bin/env bash\nset -euo pipefail\ncd "$(dirname "$0")/.."\n. ci/t0-stages.sh\n\n# slow\nsleep 2\n\n# verdict\nt0_stages_verdict_guard\necho "T0 GREEN"\n' > "$WORK/l/ci/t0.sh"
( cd "$WORK/l" && bash ci/t0.sh > "$WORK/l1.log" 2>&1 ) & lp=$!
sleep 0.7
( cd "$WORK/l" && bash ci/t0.sh > "$WORK/l2.log" 2>&1 ); l2=$?
wait "$lp"; l1=$?
( cd "$WORK/l" && bash ci/t0.sh > "$WORK/l3.log" 2>&1 ); l3=$?
mkdir -p "$WORK/l/target/.t0-running"; echo 999999 > "$WORK/l/target/.t0-running/pid"
( cd "$WORK/l" && bash ci/t0.sh > "$WORK/l4.log" 2>&1 ); l4=$?
if [ "$l1" = 0 ] && [ "$l2" = 2 ] && grep -q 'T0 REFUSED — another T0' "$WORK/l2.log" && [ "$l3" = 0 ] && [ "$l4" = 0 ] \
   && [ ! -e "$WORK/l/target/.t0-running" ]; then
    ok "L a second T0 in the same tree is REFUSED (2) while the first runs; released after; a dead holder is taken over"
else bad "L one-T0-per-tree: first=$l1 second=$l2 after=$l3 stale=$l4"; fi

# --- M ----------------------------------------------------------------------
if [ "$MUTANTS" = 1 ]; then
    # Each mutant is a whole re-run of this suite against a broken library;
    # they are independent (own WORK dir each), so they run concurrently and
    # are judged afterwards, in a fixed order.
    MUTS=()
    mutate() {  # name perl-substitution
        local m="$WORK/mut-$1.sh"
        perl -0pe "$2" "$LIB" > "$m"
        if cmp -s "$m" "$LIB"; then bad "M $1: the mutation did not apply (library changed?)"; return; fi
        MUTS+=("$1")
        ( bash "$0" --lib "$m" --no-mutants --quiet > "$WORK/mut-$1.log" 2>&1; echo $? > "$WORK/mut-$1.rc" ) &
    }
    mutate record-nothing   's/(_t0s_record\(\) \{\n)/$1    return 0\n/'
    mutate guard-disabled   's/(t0_stages_verdict_guard\(\) \{\n)/$1    return 0\n/'
    mutate exit-status-lost 's/            builtin exit "\$rc"\n/            builtin exit 0\n/'
    mutate no-err-trap      's/    trap .\_t0s_err "\$\?" "\$LINENO". ERR\n//'
    mutate err-keeps-errexit 's/(_t0s_record ERR "\$1" "\$2"\n)    set \+e\n/$1/'
    mutate never-rearm      's/    if \[ "\$_T0S_KG" = 1 \]; then set -e; fi\n//'
    mutate child-cpu-lost   's/f\[15\] \+ f\[16\]/0/'
    mutate pf-rc-lost       's/            return "\$rc"\n/            return 0\n/'
    mutate tree-unchecked   's/    if ! changed=\$\(_t0s_tree_changes\); then\n        _T0S_TREE_REPORTED=1/    if false; then\n        _T0S_TREE_REPORTED=1/'
    mutate pf-no-snapshot   's/      cd "\$snap" \|\| rc=255\n//'
    wait
    for m in "${MUTS[@]}"; do
        if [ "$(cat "$WORK/mut-$m.rc" 2>/dev/null)" = 0 ]; then
            bad "M $m: a broken library PASSED this falsifier"
        elif ! grep -q '^T0-KEEPGOING-FALSIFY RED' "$WORK/mut-$m.log"; then
            bad "M $m: the re-run did not reach a verdict: $(tail -1 "$WORK/mut-$m.log")"
        else
            ok "M $m: rejected ($(grep -c '^FAIL' "$WORK/mut-$m.log") case(s) red)"
        fi
    done
fi

echo "T0-KEEPGOING-FALSIFY $([ "$FAIL" = 0 ] && echo GREEN || echo RED) — $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
