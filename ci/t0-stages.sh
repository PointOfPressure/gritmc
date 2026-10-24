# ci/t0-stages.sh — sourced by ci/t0.sh, never run. Three things: stage timing
# (always), keep-going (opt-in) and prefetch (on; T0_PREFETCH=0 turns it off).
#
#   1. STAGE TIMING, always on. T0 is cut into STAGES mechanically, with no
#      marker to maintain: a stage starts at every column-0 code line that
#      follows a blank or a column-0 comment line — i.e. at each check's first
#      line after its comment block. A DEBUG trap (top-level shell only; it is
#      not inherited by functions, subshells or command substitutions) notices
#      when execution enters a new stage and closes the previous one.
#      (A line opening with `(` never starts a stage: bash fires no DEBUG
#      trap in the parent for a subshell, so such a stage could not be seen
#      to start. It is timed as part of the stage before it — the label says
#      which — and its reds are still attributed by line.) One
#      machine-readable line per stage is printed at exit:
#
#        T0-STAGE line=<N> wall_s=<s> cpu_s=<s> reds=<n> label="<first line>"
#
#      cpu_s is user+sys of the shell and every child it reaped during the
#      stage (/proc/$$/stat fields 14-17), so a cargo build shows its real
#      CPU and a stage that only waits shows ~0. `T0_STAGE_LOG=<file>` keeps
#      the lines in a file too.
#
#   2. T0_KEEP_GOING=1. Every stage runs; every red is recorded with its line;
#      one summary of all reds is printed at the end; the exit status is
#      non-zero if ANY red was recorded, whatever the last command returned.
#      Mechanism: errexit STAYS ON, so every subshell and command
#      substitution forks with exactly the errexit state it has in a default
#      run (bash: a `( )` subshell inherits -e, a `$( )` has it cleared) and
#      behaves byte-for-byte as it would there. Only the MAIN shell is kept
#      alive: an ERR trap (not inherited: no `set -E`) records the red and
#      turns errexit off, bash re-reads the flag after the trap and does not
#      exit, and the DEBUG trap turns it back on before the next command.
#      `exit` is shadowed by a function that records a non-zero exit and
#      returns; inside a subshell or command substitution it still really
#      exits, ending only that subshell, exactly as before.
#      `set -u` aborts cannot be trapped; the EXIT trap records those as an
#      ABORT red, so an abort is never green either.
#
#      THE ONE PLACE KEEP-GOING COULD TURN RED INTO GREEN is ci/t0.sh's last
#      block, which prints `T0 GREEN` to anyone who reaches it. It calls
#      `t0_stages_verdict_guard` first, which in keep-going mode with any red
#      recorded prints the summary and exits 1 before that line.
#
#   3. PREFETCH (t0_spawn / t0_prefetched): a few long checks start at T0's
#      top, each in its own snapshot of the tree, and are judged at their usual
#      place. Details at the functions below. A prefetched job's CPU lands in
#      whichever stage is running when bash reaps it; the T0-STAGES total is
#      exact.
#
# Default behaviour (T0_KEEP_GOING unset or 0) is unchanged: errexit stays on,
# `exit` is the builtin, and the additions are the T0-STAGE lines at exit and
# the prefetch.
# Falsified by ci/t0-keepgoing-falsify.sh, which runs in T0.

_T0S_SRC="${BASH_SOURCE[1]:-$0}"
[ -f "$_T0S_SRC" ] || _T0S_SRC=ci/t0.sh
_T0S_KG="${T0_KEEP_GOING:-0}"
_T0S_TICK=$(getconf CLK_TCK 2>/dev/null || echo 100)
_T0S_REDS="$(mktemp "${TMPDIR:-/tmp}/t0-reds.XXXXXX")"
_T0S_LINES="${T0_STAGE_LOG:-}"
_T0S_TMPLINES="$(mktemp "${TMPDIR:-/tmp}/t0-stages.XXXXXX")"
_T0S_CUR=""
_T0S_W0=""
_T0S_C0=""
_T0S_START=${EPOCHREALTIME/[.,]/}; _T0S_START=$((10#$_T0S_START))
declare -a _T0S_SEG=() _T0S_LAB=()

# line -> stage start line, and stage start line -> label, in one awk pass.
while IFS=$'\t' read -r _t0s_n _t0s_s _t0s_l; do
    _T0S_SEG[$_t0s_n]=$_t0s_s
    if [ -n "$_t0s_l" ]; then _T0S_LAB[$_t0s_s]=$_t0s_l; fi
done < <(awk '
    BEGIN { seg = 0; sep = 1 }
    {
        code = ($0 ~ /^[^#[:space:]]/) && ($0 !~ /^(fi|done|esac|else|elif|then|do|EOF|[({})])([^[:alnum:]_]|$)/)
        lab = ""
        if (code && sep) {
            seg = NR
            lab = substr($0, 1, 72); gsub(/[\t"\\]/, " ", lab)
        }
        printf "%d\t%d\t%s\n", NR, seg, lab
        sep = ($0 ~ /^[[:space:]]*$/ || $0 ~ /^#/)
    }' "$_T0S_SRC")
unset _t0s_n _t0s_s _t0s_l

_t0s_cpu() {  # centiseconds-ish ticks: self utime+stime + reaped children
    local f
    read -r -a f < "/proc/$$/stat" || { _T0S_CPUV=0; return 0; }
    # field 2 is "(bash)" — no spaces in it, so indices hold
    _T0S_CPUV=$(( f[13] + f[14] + f[15] + f[16] ))
    return 0
}

_t0s_us() { _T0S_USV=${EPOCHREALTIME/[.,]/}; _T0S_USV=$((10#$_T0S_USV)); return 0; }
_t0s_fmt() {  # value divisor decimals -> _T0S_FMTV, integer arithmetic only
    local v=$1 d=$2 p=$3 i f
    i=$(( v / d )); f=$(( (v % d) * (10 ** p) / d ))
    printf -v _T0S_FMTV '%d.%0*d' "$i" "$p" "$f"
    return 0
}

_t0s_close() {
    local w1 c1 wall cpu reds=0
    if [ -z "$_T0S_CUR" ]; then return 0; fi
    _t0s_us; w1=$_T0S_USV
    _t0s_cpu; c1=$_T0S_CPUV
    _t0s_fmt $(( w1 - _T0S_W0 )) 1000000 3; wall=$_T0S_FMTV
    _t0s_fmt $(( (c1 - _T0S_C0) )) "$_T0S_TICK" 2; cpu=$_T0S_FMTV
    if [ -s "$_T0S_REDS" ]; then
        reds=$(awk -v s="$_T0S_CUR" '$4==s' "$_T0S_REDS" | wc -l)
    fi
    printf 'T0-STAGE line=%s wall_s=%s cpu_s=%s reds=%s label="%s"\n' \
        "$_T0S_CUR" "$wall" "$cpu" "$reds" "${_T0S_LAB[$_T0S_CUR]:-}" >> "$_T0S_TMPLINES"
    _T0S_CUR=""
    return 0
}

_t0s_debug() {
    local s
    if [ "$BASH_SUBSHELL" != 0 ]; then return 0; fi
    # keep-going: the ERR trap turned errexit off to survive a red; it goes
    # back on before the next main-shell command, so the next subshell forks
    # with the same -e a default run would give it.
    if [ "$_T0S_KG" = 1 ]; then set -e; fi
    s=${_T0S_SEG[$1]:-}
    if [ -z "$s" ] || [ "$s" = 0 ] || [ "$s" = "$_T0S_CUR" ]; then return 0; fi
    _t0s_close
    _T0S_CUR=$s
    _t0s_us; _T0S_W0=$_T0S_USV
    _t0s_cpu; _T0S_C0=$_T0S_CPUV
    return 0
}

# kind rc line stage — appended to a FILE so a subshell's record survives it.
_t0s_record() {
    local seg=${_T0S_SEG[$3]:-0}
    printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$seg" >> "$_T0S_REDS"
    return 0
}

_t0s_err() {
    # Main shell only. In a subshell (reachable only if someone adds set -E)
    # do nothing and let native errexit end that subshell as it always did.
    if [ "$BASH_SUBSHELL" != 0 ]; then return 0; fi
    _t0s_record ERR "$1" "$2"
    set +e
    return 0
}

_t0s_exit() {
    local code=${1:-$_T0S_LASTRC}
    if [ "$code" = 0 ]; then builtin exit 0; fi
    _t0s_record EXIT "$code" "$2"
    if [ "$BASH_SUBSHELL" != 0 ]; then builtin exit "$code"; fi
    return 0
}

_t0s_summary() {
    local n
    n=$(awk -F'\t' '{print $4}' "$_T0S_REDS" | sort -un | wc -l)
    echo "T0 KEEP-GOING — $n red stage(s), every stage ran:" >&2
    awk -F'\t' '!seen[$4]++ { print $4 "\t" $1 "\t" $2 "\t" $3 }' "$_T0S_REDS" \
    | while IFS=$'\t' read -r seg kind rc line; do
        printf '  RED  stage line=%s (%s rc=%s at line %s) %s\n' \
            "$seg" "$kind" "$rc" "$line" "${_T0S_LAB[$seg]:-}" >&2
    done
    return 0
}

# --- 4. TREE INTEGRITY: T0 must leave every file as it found it ------------
#
# Many T0 stages are falsifiers that edit a tracked file, run a check, and put
# the file back. If one of them does not put it back — killed mid-arm, two T0s
# racing in one directory, a restore that copies the wrong backup — every
# later check reads a doctored tree and the NEXT run inherits it, and nothing
# said so. So at source time a sha1 manifest is taken of every file outside
# target/, oracle-work/, .git/ and __pycache__/, and the verdict guard (both
# modes) re-checks it: a file changed or missing is a T0 RED naming it. Files
# created during the run are not judged. `T0_TREE_CHECK=0` disables it. The
# EXIT trap re-checks too, so a red run still says which files it left dirty.
# ONE T0 PER TREE. Two T0s in one directory run the same edit-and-restore
# falsifiers against the same files at once: each backs up what the other has
# just doctored and "restores" it, which is the one way a falsifier with a
# correct EXIT trap still leaves the tree wrong. A pid lock (a directory, so it
# is atomic and holds no fd a surviving child could keep open) refuses the
# second run with exit 2 and names the first; a lock whose pid is gone is
# taken over.
_T0S_LOCK="target/.t0-running"
mkdir -p target
if ! mkdir "$_T0S_LOCK" 2>/dev/null; then
    _t0s_other=$(cat "$_T0S_LOCK/pid" 2>/dev/null || true)
    if [ -n "$_t0s_other" ] && kill -0 "$_t0s_other" 2>/dev/null; then
        echo "T0 REFUSED — another T0 (pid $_t0s_other) is running in $PWD. Two T0s in one tree race" >&2
        echo "             each other's edit-and-restore falsifiers and can leave tracked files doctored." >&2
        exit 2
    fi
    rm -rf "$_T0S_LOCK"; mkdir "$_T0S_LOCK" || { echo "T0 REFUSED — cannot take $_T0S_LOCK" >&2; exit 2; }
fi
echo $$ > "$_T0S_LOCK/pid"
unset _t0s_other

_T0S_TREE="${T0_TREE_CHECK:-1}"
_T0S_MANIFEST=""
if [ "$_T0S_TREE" = 1 ]; then
    _T0S_MANIFEST="$(mktemp "${TMPDIR:-/tmp}/t0-manifest.XXXXXX")"
    # T0's own stdout/stderr may be a file inside the tree (`./ci/t0.sh > t0.log`);
    # it grows by design, so it is not judged.
    _t0s_own=$(readlink -f /proc/$$/fd/1 /proc/$$/fd/2 2>/dev/null | tr '\n' '|')
    find . \( -path ./target -o -path ./oracle-work -o -path ./.git -o -name __pycache__ \) -prune \
        -o -type f -print0 | sort -z | xargs -0 -r sha1sum \
        | awk -v own="|$_t0s_own" -v pwd="$PWD" '{ p = $0; sub(/^[0-9a-f]+  /, "", p); sub(/^\.\//, "", p)
                                                  if (index(own, "|" pwd "/" p "|") == 0) print }' > "$_T0S_MANIFEST"
    unset _t0s_own
fi
_t0s_tree_changes() {  # prints one line per changed/missing file; returns 0 if none
    local out
    if [ -z "$_T0S_MANIFEST" ] || [ ! -s "$_T0S_MANIFEST" ]; then return 0; fi
    out=$(sha1sum -c --quiet "$_T0S_MANIFEST" 2>/dev/null | sed -n 's/: FAILED.*$//p') || true
    if [ -z "$out" ]; then return 0; fi
    printf '%s\n' "$out"
    return 1
}

# Called by ci/t0.sh immediately before it prints `T0 GREEN`.
t0_stages_verdict_guard() {
    local changed
    if ! changed=$(_t0s_tree_changes); then
        _T0S_TREE_REPORTED=1
        echo "T0 RED — T0 left $(printf '%s\n' "$changed" | wc -l) file(s) different from how it found them; a stage that edits the tree did not restore it:" >&2
        printf '%s\n' "$changed" | sed 's/^/    /' >&2
        if [ "$_T0S_KG" = 1 ]; then _t0s_record TREE 1 "${BASH_LINENO[0]}"; fi
        if [ "$_T0S_KG" != 1 ]; then builtin exit 1; fi
    fi
    if [ "$_T0S_KG" = 1 ] && [ -s "$_T0S_REDS" ]; then
        _t0s_close
        _t0s_summary
        _T0S_SUMMARISED=1
        echo "T0 RED — keep-going: see the stage list above" >&2
        builtin exit 1
    fi
    return 0
}

# --- 3. PREFETCH: long read-only or self-contained checks start at T0's top --
#
#   t0_spawn CMD...        start CMD now, in its OWN copy of the tree
#   t0_prefetched CMD...   at the check's usual place: if an identical CMD was
#                          spawned, wait for it and replay its stdout, its
#                          stderr (to stderr) and its exit status; otherwise
#                          run CMD right here, exactly as before.
#
# Each spawned job runs with its cwd in a private snapshot of the tree taken
# at spawn time (rsync without target/, oracle-work/ and .git; oracle-work is
# linked, it is the read-only vanilla store). That is what makes it safe to
# run a falsifier that edits files and restores them concurrently with the
# rest of T0: it edits its snapshot, not the tree, so neither it nor any other
# check can see the other's half-edited file — which the sequential T0 could
# only promise by ordering. A job must not need target/ or ../plugins; the ones
# ci/t0.sh spawns are listed there with the reason each qualifies.
#
# The call site keeps its own redirections and `|| { ... exit 1; }`, so a red
# is judged exactly where it always was, in both modes. T0_PREFETCH=0 spawns
# nothing and every t0_prefetched runs inline. The one visible difference: a
# job's stderr is replayed after its stdout rather than interleaved.
_T0S_PF="${T0_PREFETCH:-1}"
_T0S_PFDIR=""
declare -a _T0S_PFKEY=() _T0S_PFPID=()
t0_spawn() {
    local key id snap
    if [ "$_T0S_PF" != 1 ]; then return 0; fi
    if [ -z "$_T0S_PFDIR" ]; then
        mkdir -p target
        _T0S_PFDIR=$(mktemp -d "$PWD/target/t0-prefetch.XXXXXX")
    fi
    printf -v key '%q ' "$@"
    id=${#_T0S_PFKEY[@]}
    snap="$_T0S_PFDIR/snap.$id"
    mkdir -p "$snap"
    rsync -a --exclude /target --exclude /oracle-work --exclude /.git ./ "$snap/"
    if [ -e oracle-work ]; then ln -s "$PWD/oracle-work" "$snap/oracle-work"; fi
    # errexit is inherited here; `|| rc=$?` keeps the job's status for the rc
    # file instead of letting it end the subshell before the file is written.
    ( rc=0
      cd "$snap" || rc=255
      if [ "$rc" = 0 ]; then "$@" > "$_T0S_PFDIR/out.$id" 2> "$_T0S_PFDIR/err.$id" </dev/null || rc=$?; fi
      echo "$rc" > "$_T0S_PFDIR/rc.$id" ) &
    _T0S_PFKEY[$id]=$key
    _T0S_PFPID[$id]=$!
    return 0
}
t0_prefetched() {
    local key id rc
    printf -v key '%q ' "$@"
    for id in "${!_T0S_PFKEY[@]}"; do
        if [ "${_T0S_PFKEY[$id]}" = "$key" ]; then
            _T0S_PFKEY[$id]="(consumed)"
            wait "${_T0S_PFPID[$id]}" 2>/dev/null || true
            cat "$_T0S_PFDIR/out.$id"
            cat "$_T0S_PFDIR/err.$id" >&2
            rc=$(cat "$_T0S_PFDIR/rc.$id" 2>/dev/null || echo 255)
            return "$rc"
        fi
    done
    # `|| return` keeps a failure a RETURN status, judged at the call site,
    # rather than an errexit inside this function (where keep-going's ERR trap,
    # not being inherited, could not record it).
    "$@" || return $?
    return 0
}
_t0s_pf_cleanup() {
    local id
    for id in "${!_T0S_PFPID[@]}"; do
        pkill -TERM -P "${_T0S_PFPID[$id]}" 2>/dev/null || true
        kill "${_T0S_PFPID[$id]}" 2>/dev/null || true
    done
    if [ -n "$_T0S_PFDIR" ]; then rm -rf "$_T0S_PFDIR"; fi
    return 0
}

_t0s_on_exit() {
    local rc=$? cur=${_T0S_CUR:-?} changed
    _t0s_pf_cleanup
    if [ -z "${_T0S_TREE_REPORTED:-}" ] && ! changed=$(_t0s_tree_changes); then
        echo "T0: this run left $(printf '%s\n' "$changed" | wc -l) file(s) different from how it found them:" >&2
        printf '%s\n' "$changed" | sed 's/^/    /' >&2
        if [ "$rc" = 0 ]; then rc=1; fi
    fi
    rm -f "$_T0S_MANIFEST"
    if [ "$(cat "$_T0S_LOCK/pid" 2>/dev/null)" = "$$" ]; then rm -rf "$_T0S_LOCK"; fi
    _t0s_close
    local total cpu
    _t0s_us; _t0s_fmt $(( _T0S_USV - _T0S_START )) 1000000 3; total=$_T0S_FMTV
    _t0s_cpu; _t0s_fmt "$_T0S_CPUV" "$_T0S_TICK" 2; cpu=$_T0S_FMTV
    echo "T0-STAGES total_wall_s=$total total_cpu_s=$cpu stages=$(wc -l < "$_T0S_TMPLINES") keep_going=$_T0S_KG rc=$rc" >> "$_T0S_TMPLINES"
    cat "$_T0S_TMPLINES"
    if [ -n "$_T0S_LINES" ]; then cat "$_T0S_TMPLINES" >> "$_T0S_LINES" 2>/dev/null; fi
    if [ "$_T0S_KG" = 1 ]; then
        if [ "$rc" != 0 ] && [ -z "${_T0S_SUMMARISED:-}" ]; then
            printf 'ABORT\t%s\t%s\t%s\n' "$rc" "?" "$cur" >> "$_T0S_REDS"
        fi
        if [ -s "$_T0S_REDS" ]; then
            if [ -z "${_T0S_SUMMARISED:-}" ]; then _t0s_summary; fi
            rm -f "$_T0S_REDS" "$_T0S_TMPLINES"
            if [ "$rc" = 0 ]; then rc=1; fi
            builtin exit "$rc"
        fi
    fi
    rm -f "$_T0S_REDS" "$_T0S_TMPLINES"
    builtin exit "$rc"
}

trap _t0s_on_exit EXIT
trap '_t0s_debug "$LINENO"' DEBUG
if [ "$_T0S_KG" = 1 ]; then
    trap '_t0s_err "$?" "$LINENO"' ERR
    # `$?` at the call site is what a bare `exit` would use; capture it in the
    # alias-free way: the function's first expansion still sees the caller's $?.
    exit() { _T0S_LASTRC=$?; _t0s_exit "${1:-}" "${BASH_LINENO[0]}"; }
fi
