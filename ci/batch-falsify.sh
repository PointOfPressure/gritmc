#!/usr/bin/env bash
# ci/batch-falsify.sh — the red-when-broken evidence for ci/batch.sh.
#
# ===========================================================================
# WHAT THIS PROVES, AND WHY EACH ONE IS HERE
# ===========================================================================
#
# `ci/batch.sh` lands several branches for one gate. Everything about it that
# can go wrong is expensive in a way that is invisible at the time:
#
#   * landing a batch that contains a RED branch puts a broken commit on main
#     behind a green verdict that was never about it;
#   * dirtying `/home/sayer/grit/repo` makes `ci/handoff.sh:418` refuse every
#     subsequent gate — on 2026-08-17 that cost 25 minutes and **861
#     consecutive refusals**, and nothing noticed because nothing looked;
#   * blaming a member for a batch RED it did not cause is a verdict no
#     measurement produced, which is this project's central defect class.
#
# So the four properties the brief names are cases 1, 3, 5 and 9 below, and the
# rest exist because writing them found real defects.
#
# THE STAND-IN GATE IS NOT A LOOKUP ON THE BRANCH NAME. It reads the integration
# tip's own tree and returns RED if a red-listed member's file is present. That
# matters: a stub keyed on the batch branch name could not tell a tip that
# contains its members from an empty one, so `case_two_green_land_both` would
# pass over a batch that landed nothing — the exact green-that-cannot-go-red
# this repo refuses. Injection `tip_content_dropped` is the control for it.
#
# THE FIXTURE'S CANONICAL CHECKOUT IS NOT ON main, because the real one is not
# either: measured 2026-08-17, `/home/sayer/grit/repo` was on
# `refcache-root-invariance` with a live gate in it. A fixture whose clone sat
# on main would make every `--land` case refuse for a reason that has nothing
# to do with what is under test, and case 12 would then be untestable because
# it would be the default.
#
# Usage:  ci/batch-falsify.sh [-v]
#         BATCH_FALSIFY_DRYRUN=1 ci/batch-falsify.sh    # patches only, ~2s
#
# `DRYRUN` applies every injection and runs no case. Run it after touching
# ci/batch.sh: a patch that has stopped matching is a hole in the sweep whether
# or not you have the minutes to run the cases.
# ===========================================================================
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BATCH_REAL="$ROOT/ci/batch.sh"
[ -r "$BATCH_REAL" ] || { echo "no ci/batch.sh beside this script"; exit 2; }

G="git"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/batch-falsify.XXXXXX")"
REMOTE_DIR="rustmc-batch-falsify-$$"
FAKE_PID=""

stop_fake_gate() {
    [ -n "$FAKE_PID" ] || return 0
    local c
    for c in $(pgrep -P "$FAKE_PID" 2>/dev/null); do
        [ "$c" = "$$" ] || kill -9 "$c" 2>/dev/null
    done
    [ "$FAKE_PID" = "$$" ] || kill -9 "$FAKE_PID" 2>/dev/null
    wait "$FAKE_PID" 2>/dev/null
    FAKE_PID=""
}

cleanup() {
    stop_fake_gate
    # PID-resolved, never `pkill -f`: that matches the pattern in its own
    # command line and kills the caller. AGENT-BRIEF, and it cost this project
    # four over-broad matches on 2026-08-17.
    rm -rf "$WORK"
}
trap cleanup EXIT

VERBOSE=""
[ "${1:-}" = "-v" ] && VERBOSE=1
PASS=0; FAIL=0; MISSED=0
declare -a FAILED_CASES=() MISSED_INJECTIONS=()
ok()   { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); FAILED_CASES+=("$*"); printf '  FAIL  %s\n' "$*"; }
note() { [ -n "$VERBOSE" ] && printf '%s\n' "$*" | sed 's/^/        | /'; return 0; }

# ---------------------------------------------------------------------------
# THE FIXTURE
# ---------------------------------------------------------------------------
# origin.git  a bare repo standing in for GitHub
# repo        a clone standing in for /home/sayer/grit/repo, ON A NON-main
#             BRANCH, because the real one is (see the header)
# agent       a worktree of it standing in for the agent's own checkout; this
#             is where ci/batch.sh is installed and run from
#
# Each brN touches ONLY its own `fileN.txt`, so brN and brM cherry-pick onto one
# tip without colliding — which is the case batching exists for. The first
# version of this fixture had every branch append a row to a shared `index.md`,
# copied from ci/lanes-falsify.sh where that is exactly right (it makes a stale
# rebase base show up as a conflict). Here it made EVERY PAIR collide, so nine
# cases failed with one member excluded and it read as a defect in
# ci/batch.sh's cherry-pick. Worth keeping in mind about the real queue: two
# branches appending different lines to one file DO conflict under git's 3-way,
# which is the whole reason `merge=docregions` exists and the reason the
# collision-feasible batch width is small before those drivers land.
#
# Collisions are supplied deliberately by case 9, out of two branches that
# rewrite the same line, rather than as a property of the whole fixture.
F=""
build_fixture() {
    local n="$1"
    F="$WORK/fx"
    rm -rf "$F"; mkdir -p "$F"
    $G init -q --bare -b main "$F/origin.git"
    $G clone -q "$F/origin.git" "$F/repo" 2>/dev/null
    (
        cd "$F/repo"
        $G config user.email f@f; $G config user.name f
        printf '# index\nbase\n' > index.md
        mkdir -p ci && printf 'seed\n' > ci/seed.txt
        $G add index.md ci/seed.txt && $G commit -qm base && $G push -q origin main
        local i
        for i in $(seq 1 "$n"); do
            $G checkout -q -b "br$i" main
            printf 'br%s\n' "$i" > "file$i.txt"
            $G add "file$i.txt" && $G commit -qm "br$i: its own file, colliding with nobody"
            $G push -q origin "br$i"
            $G checkout -q main
        done
        # The canonical checkout sits on something that is NOT main.
        $G checkout -q -b held main
    ) >/dev/null 2>&1
    $G -C "$F/repo" worktree add --detach "$F/agent" main >/dev/null 2>&1
    ( cd "$F/agent" && $G config user.email f@f && $G config user.name f ) >/dev/null 2>&1
}

# ---------------------------------------------------------------------------
# THE STAND-IN GATE
# ---------------------------------------------------------------------------
# Verdict from the TIP'S CONTENT, not from the branch name it was handed:
#   RED  if any file named in red.txt is present in the tree
#   RED  if both files on any line of inter.txt are present  (an INTERACTION
#        red: each member green alone, the pair red together)
#   GREEN otherwise
# `force-exit` overrides with a raw exit code, for the INFRA cases.
# `hook-<n>` runs a command during the n-th gate, which is how the lease and
# shared-checkout cases perturb the world from INSIDE the gate window rather
# than around it.
build_stub() {
    mkdir -p "$WORK/bin"
    cat > "$WORK/bin/stub-gate" <<EOS
#!/usr/bin/env bash
set -uo pipefail
BR="\$1"
W="$WORK"
n=\$(( \$(cat "\$W/gatecount" 2>/dev/null || echo 0) + 1 ))
echo "\$n" > "\$W/gatecount"
printf '%s\t%s\t%s\n' "\$n" "\$BR" "\$(git rev-parse HEAD)" >> "\$W/gates.log"
git ls-tree -r --name-only HEAD > "\$W/tip-\$n.files"

if [ -f "\$W/hook-\$n" ]; then bash "\$W/hook-\$n" >>"\$W/hooks.log" 2>&1; fi
if [ -s "\$W/force-exit" ]; then exit "\$(cat "\$W/force-exit")"; fi

present() { git cat-file -e "HEAD:\$1" 2>/dev/null; }

if [ -s "\$W/red.txt" ]; then
    while IFS= read -r f; do
        [ -n "\$f" ] || continue
        if present "\$f"; then echo "RED      \$BR — stand-in: \$f is in this tip"; exit 1; fi
    done < "\$W/red.txt"
fi
if [ -s "\$W/inter.txt" ]; then
    while read -r a b; do
        [ -n "\$a" ] && [ -n "\$b" ] || continue
        if present "\$a" && present "\$b"; then
            echo "RED      \$BR — stand-in: \$a and \$b are red only together"; exit 1
        fi
    done < "\$W/inter.txt"
fi
echo "GREEN    \$BR — stand-in verdict"
exit 0
EOS
    chmod +x "$WORK/bin/stub-gate"
}

# ---------------------------------------------------------------------------
# RUNNING ONE BATCH
# ---------------------------------------------------------------------------
# The script under test is installed INTO the fixture and run from there,
# because ci/batch.sh derives $ROOT from its own path — running the real file
# against a foreign cwd would have it drive THE REAL REPOSITORY. That is not a
# hypothetical: it is what the first version of ci/lanes-falsify.sh did.
run_batch() {
    local script="$1" root="$2"; shift 2
    : > "$WORK/gates.log"; : > "$WORK/gatecount"
    rm -rf "$WORK/int" "$WORK/gatelogs"
    mkdir -p "$root/ci" "$WORK/gatelogs"
    cp "$script" "$root/ci/batch.sh"; chmod +x "$root/ci/batch.sh"
    # ci/batch.sh consults these two by relative path for the rate guard, so the
    # fixture needs the REAL ones: stubbing them would test a stub's opinion of
    # whether batching pays.
    cp "$ROOT/ci/gate-rate.py" "$ROOT/ci/batch-arith.py" "$root/ci/"
    chmod +x "$root/ci/gate-rate.py" "$root/ci/batch-arith.py"
    # A stand-in for the merge-driver registrar: the real one is not under test
    # here and its absence would make every group refuse for the wrong reason.
    [ -x "$root/ci/setup-merge-drivers.sh" ] || {
        printf '#!/usr/bin/env bash\nexit 0\n' > "$root/ci/setup-merge-drivers.sh"
        chmod +x "$root/ci/setup-merge-drivers.sh"; }
    (
        cd "$root"
        RUSTMC_CANONICAL="$F/repo" \
        RUSTMC_BATCH_GATE_CMD="$WORK/bin/stub-gate %BRANCH%" \
        RUSTMC_BATCH_INT_DIR="$WORK/int" \
        RUSTMC_BATCH_REMOTE_DIR="$REMOTE_DIR" \
        RUSTMC_BATCH_LOGDIR="$WORK/gatelogs" \
        RUSTMC_BATCH_GATE_LOGDIR="$WORK/raterate" \
        RUSTMC_CORPUS_LOCK_DIR="$WORK/corpuslock" \
        RUSTMC_SWEEP_RECORD_CMD="${SWEEP_RECORD_CMD:-}" \
        RUSTMC_SWEEP_LOG="$WORK/sweep.log" \
        RUSTMC_REMOTE_DIR="rustmc-batch-falsify-sweep-$$" \
        bash ./ci/batch.sh "$@"
    ) > "$WORK/out" 2>&1
    echo $? > "$WORK/rc"
    note "$(cat "$WORK/out")"
}

brc()        { cat "$WORK/rc" 2>/dev/null || echo missing; }
# `grep -c` PRINTS 0 AND EXITS 1 on no match, so `grep -c . f || echo 0` emits
# "0\n0" and every numeric comparison against it fails. That one shape broke
# four cases on the first run and read as a defect in ci/batch.sh.
gates_run()  { awk 'END{print NR}' "$WORK/gates.log" 2>/dev/null || echo 0; }
omain()      { $G -C "$F/origin.git" rev-parse main 2>/dev/null; }
ofiles()     { $G -C "$F/origin.git" ls-tree -r --name-only main 2>/dev/null; }
lmain()      { $G -C "$F/repo" rev-parse refs/heads/main 2>/dev/null; }
verdict()    { awk -v b="$1" '$1==b{print $2; exit}' <(sed -n '/^BRANCH  */,$p' "$WORK/out"); }
# `--` is not optional: three assertions here look for a message that begins
# with `--on-red` or `--max-gates`, and without it grep reads the pattern as its
# own option and the case fails with `unrecognised option`.
outhas()     { grep -qF -- "$1" "$WORK/out"; }
shared_dirty() { $G -C "$F/repo" status --porcelain 2>/dev/null | awk 'END{print NR}'; }
shared_head()  { $G -C "$F/repo" rev-parse HEAD 2>/dev/null; }
batch_refs()   { $G -C "$F/repo" for-each-ref --format='%(refname:short)' 'refs/heads/batch-*' 2>/dev/null | awk 'END{print NR}'; }

# Every case ends by asserting the shared checkout is untouched, because that
# is a property of EVERY run and not of one case. A per-case assertion would
# have to be remembered twenty-four times.
assert_shared_untouched() {
    local d h
    d="$(shared_dirty)"; h="$(shared_head)"
    [ "$d" = 0 ] || { echo "        shared checkout dirty: $d entries"; return 1; }
    [ "$h" = "$SHARED_H0" ] || { echo "        shared checkout HEAD moved"; return 1; }
    outhas "SHARED-CHECKOUT  untouched" || {
        echo "        ci/batch.sh did not report SHARED-CHECKOUT untouched"; return 1; }
    return 0
}

# ---------------------------------------------------------------------------
# THE CASES
# ---------------------------------------------------------------------------
SHARED_H0=""
SWEEP_TIP_SRC=""
SWEEP_RECORD_CMD=""

# A FIXTURE THAT CARRIES NEW GAMEPLAY SCENARIOS. br1 and br2 each ADD one
# scenario file, and main has a ledger that registers neither. That is the state
# that makes a recording necessary, and it is the state two agents were stuck in
# this afternoon.
build_scenario_fixture() {
    build_fixture 2
    (
        cd "$F/repo"
        $G checkout -q main
        mkdir -p ci/checks/gameplay
        # THE TIP-SIDE TOOLS MUST BE TRACKED. ci/batch.sh runs ci/sweep-tip.sh
        # and ci/ledger-graft.py FROM THE INTEGRATION WORKTREE, which is a
        # checkout of the tip and gets `reset --hard` + `clean -fd` before every
        # group -- so an untracked copy is deleted before it can run, and the
        # first version of this fixture duly reported "no ci/sweep-tip.sh on the
        # integration tip" for all five sweep cases. In the real repo they are
        # tracked, so tracking them here is the faithful arrangement rather than
        # a workaround.
        cp "${SWEEP_TIP_SRC:-$ROOT/ci/sweep-tip.sh}" ci/sweep-tip.sh
        cp "$ROOT/ci/ledger-graft.py" ci/ledger-graft.py
        cat > ci/corpus-falsifiable.py <<EOCF
#!/usr/bin/env python3
import sys, pathlib
state = pathlib.Path("$WORK/cf-state")
want = state.read_text().strip() if state.exists() else "green"
if want == "green":
    print("corpus-falsifiable: all checks carry evidence")
    sys.exit(0)
led = pathlib.Path("ci/checks/gameplay-falsifiable.toml")
text = led.read_text() if led.exists() else ""
missing = [p.name for p in sorted(pathlib.Path("ci/checks/gameplay").glob("*.toml"))
           if p.name not in text]
for m in missing:
    print(f"UNREGISTERED {m[4:-5]} ({m}) — no red-when-broken evidence")
sys.exit(1 if missing else 0)
EOCF
        chmod +x ci/sweep-tip.sh ci/ledger-graft.py ci/corpus-falsifiable.py
        printf '# ledger\n[meta]\nbattery = ["edit"]\n' > ci/checks/gameplay-falsifiable.toml
        printf 'name = "base"\n' > ci/checks/gameplay/010-base.toml
        printf '[[scenario]]\nname = "base"\nfile = "010-base.toml"\ntier = 2\n' \
            >> ci/checks/gameplay-falsifiable.toml
        $G add ci/checks ci/sweep-tip.sh ci/ledger-graft.py ci/corpus-falsifiable.py
        $G commit -qm "base scenario, its ledger entry, and the tip-side tools"
        $G push -q origin main
        local i
        for i in 1 2; do
            $G checkout -q "br$i"
            $G rebase -q main
            printf 'name = "new%s"\n' "$i" > "ci/checks/gameplay/90$i-new$i.toml"
            $G add ci/checks && $G commit -qm "br$i: a new gameplay scenario"
            $G push -q -f origin "br$i"
        done
        $G checkout -q held
        $G rebase -q main >/dev/null 2>&1 || true
    ) >/dev/null 2>&1
    $G -C "$F/agent" checkout -q --detach main 2>/dev/null
    SHARED_H0="$(shared_head)"
    # The stand-in record phase: what a real ci/t2-gameplay.sh + --sweep would
    # leave behind, which is ledger entries for whatever scenarios the TIP holds.
    # It records that it ran, so a case can assert ONE recording covered N
    # branches rather than N recordings.
    cat > "$WORK/bin/record" <<EOR
#!/usr/bin/env bash
echo "\$(date +%s%N)" >> "$WORK/record-calls"
for f in ci/checks/gameplay/*.toml; do
    b="\$(basename "\$f")"
    grep -qF "\$b" ci/checks/gameplay-falsifiable.toml && continue
    printf '\n[[scenario]]\nname = "%s"\nfile = "%s"\ntier = 2\nkilled_by = ["edit"]\n' \
        "\${b:4:-5}" "\$b" >> ci/checks/gameplay-falsifiable.toml
done
echo green > "$WORK/cf-state"
EOR
    chmod +x "$WORK/bin/record"
    SWEEP_RECORD_CMD="$WORK/bin/record"
    echo red > "$WORK/cf-state"
}

record_calls() { awk 'END{print NR}' "$WORK/record-calls" 2>/dev/null || echo 0; }
origin_file()  { $G -C "$F/origin.git" show "$1:$2" 2>/dev/null; }

# A merge gate in somebody else's tree, in the lock's own on-disk format:
# waiters/<pid> carrying role= and tree= as key=value lines.
plant_merge_gate() {
    mkdir -p "$WORK/corpuslock/waiters"
    printf 'role=gate\npid=%s\ntree=/home/sayer/rustmc-somebody-else\nhost=x\n' "$$" \
        > "$WORK/corpuslock/waiters/$$"
}

# THE RATE THE GUARD READS. ci/batch.sh refuses to land when the measured
# green-at-verdict rate says batching at this width is slower than gating
# serially, so every --land case needs a population that says otherwise. Seeded
# with a healthy one by default; the three rate cases reseed it.
#
# These are real ci/gate-rate.py inputs, not a mocked number: files named
# gate-<branch>-<stamp>.log carrying handoff's own stage sentinels.
seed_rate() {   # seed_rate <n-green> <n-red>
    local g="$1" r="$2" i
    rm -rf "$WORK/raterate"; mkdir -p "$WORK/raterate"
    for i in $(seq 1 "$g"); do
        printf 'T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY GREEN\n' \
            > "$WORK/raterate/gate-g$i-20260817-0000$i.log"
    done
    for i in $(seq 1 "$r"); do
        printf 'T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY RED\n' \
            > "$WORK/raterate/gate-r$i-20260817-1000$i.log"
    done
}

prep() {
    build_fixture "$1"; build_stub
    : > "$WORK/red.txt"; : > "$WORK/inter.txt"; : > "$WORK/force-exit"
    rm -f "$WORK"/hook-* "$WORK/hooks.log"
    seed_rate 30 2          # p = 0.9375 at N = 32 -> BATCH
    echo green > "$WORK/cf-state"
    rm -rf "$WORK/corpuslock"; mkdir -p "$WORK/corpuslock/waiters"
    : > "$WORK/record-calls"
    SWEEP_RECORD_CMD=""
    SHARED_H0="$(shared_head)"
}

# 1. THE BRIEF'S FIRST REQUIREMENT: two distinct green branches must land BOTH.
case_two_green_land_both() {
    prep 2
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    [ "$(brc)" = 0 ]                || { echo "        rc $(brc), want 0"; return 1; }
    [ "$(gates_run)" = 1 ]          || { echo "        $(gates_run) gates, want 1"; return 1; }
    [ "$(omain)" != "$before" ]     || { echo "        origin main did not move"; return 1; }
    ofiles | grep -c '^file1.txt$' >/dev/null  || { echo "        file1.txt not on main"; return 1; }
    ofiles | grep -c '^file2.txt$' >/dev/null  || { echo "        file2.txt not on main"; return 1; }
    [ "$($G -C "$F/origin.git" rev-list --count "$before..main")" = 2 ] || {
        echo "        main did not advance by exactly 2 commits"; return 1; }
    [ "$(verdict br1)" = LANDED ]   || { echo "        br1 verdict $(verdict br1)"; return 1; }
    [ "$(verdict br2)" = LANDED ]   || { echo "        br2 verdict $(verdict br2)"; return 1; }
    [ "$(lmain)" = "$(omain)" ]     || { echo "        local main != origin main"; return 1; }
    assert_shared_untouched
}

# 2. THE BRIEF'S SECOND REQUIREMENT, in its strict form: a batch containing one
#    RED must land NONE of them and leave main untouched.
case_red_batch_lands_none() {
    prep 3
    echo 'file2.txt' > "$WORK/red.txt"
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 3 --on-red none --land br1 br2 br3
    [ "$(brc)" = 1 ]            || { echo "        rc $(brc), want 1"; return 1; }
    [ "$(omain)" = "$before" ]  || { echo "        MAIN MOVED on a red batch"; return 1; }
    [ "$(lmain)" = "$before" ]  || { echo "        local main moved on a red batch"; return 1; }
    outhas "branches landed: 0" || { echo "        did not report 0 landed"; return 1; }
    local b
    for b in br1 br2 br3; do
        [ "$(verdict $b)" = UNJUDGED-BATCHRED ] || {
            echo "        $b verdict $(verdict $b), want UNJUDGED-BATCHRED"; return 1; }
    done
    # And no member is called RED, because none has been measured alone.
    grep -qE '^br[123] +RED$' "$WORK/out" && {
        echo "        a member was condemned without being gated alone"; return 1; }
    assert_shared_untouched
}

# 3. ...and in its working form: the batch lands nothing, the re-gated members
#    are NAMED, and only the ones that pass alone land.
case_red_batch_singles_names_regated() {
    prep 3
    echo 'file2.txt' > "$WORK/red.txt"
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 3 --on-red singles --land br1 br2 br3
    [ "$(brc)" = 1 ]        || { echo "        rc $(brc), want 1"; return 1; }
    [ "$(gates_run)" = 4 ]  || { echo "        $(gates_run) gates, want 4 (1 batch + 3 singles)"; return 1; }
    outhas "--on-red singles: re-gating each of br1 br2 br3 alone" || {
        echo "        did not name the members it re-gated"; return 1; }
    [ "$(verdict br2)" = RED ] || { echo "        br2 verdict $(verdict br2), want RED"; return 1; }
    [ "$(verdict br1)" = LANDED ] || { echo "        br1 verdict $(verdict br1)"; return 1; }
    [ "$(verdict br3)" = LANDED ] || { echo "        br3 verdict $(verdict br3)"; return 1; }
    ofiles | grep -c '^file2.txt$' >/dev/null && { echo "        THE RED BRANCH LANDED"; return 1; }
    ofiles | grep -c '^file1.txt$' >/dev/null || { echo "        br1 did not land"; return 1; }
    ofiles | grep -c '^file3.txt$' >/dev/null || { echo "        br3 did not land"; return 1; }
    [ "$($G -C "$F/origin.git" rev-list --count "$before..main")" = 2 ] || {
        echo "        main advanced by $($G -C "$F/origin.git" rev-list --count "$before..main"), want 2"; return 1; }
    assert_shared_untouched
}

# 4. THE BRIEF'S THIRD REQUIREMENT, as a POSITIVE CONTROL. Every other case
#    asserts the shared checkout is clean; this one dirties it from inside the
#    gate window and proves ci/batch.sh NOTICES. Without this the assertion in
#    all the others is a green that cannot go red.
case_shared_dirty_is_caught() {
    prep 2
    printf 'printf "an agent was mid-edit\\n" >> "%s/repo/index.md"\n' "$F" > "$WORK/hook-1"
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    [ "$(brc)" = 2 ] || { echo "        rc $(brc), want 2 on a dirtied shared checkout"; return 1; }
    outhas "SHARED-CHECKOUT  DIRTIED" || {
        echo "        did not report SHARED-CHECKOUT DIRTIED"; return 1; }
    outhas "861" || { echo "        did not name the failure it is guarding"; return 1; }
    return 0
}

# 5. Refuses to run in the canonical checkout at all — ci/lanes.sh's
#    lanes-canonical-guard, held to by this script too.
case_refuses_root_is_canonical() {
    prep 2
    run_batch "$1" "$F/repo" --width 2 br1 br2
    [ "$(brc)" = 2 ]        || { echo "        rc $(brc), want 2"; return 1; }
    [ "$(gates_run)" = 0 ]  || { echo "        ran $(gates_run) gates in the canonical checkout"; return 1; }
    outhas "canonical checkout" || { echo "        did not name the canonical checkout"; return 1; }
    return 0
}

case_refuses_int_dir_is_root() {
    prep 2
    run_batch "$1" "$F/agent" --width 2 --int-dir "$F/agent" br1 br2
    [ "$(brc)" = 2 ]       || { echo "        rc $(brc), want 2"; return 1; }
    [ "$(gates_run)" = 0 ] || { echo "        ran a gate with int-dir == ROOT"; return 1; }
    assert_shared_untouched
}

case_refuses_int_dir_is_canonical() {
    prep 2
    run_batch "$1" "$F/agent" --width 2 --int-dir "$F/repo" br1 br2
    [ "$(brc)" = 2 ]       || { echo "        rc $(brc), want 2"; return 1; }
    [ "$(gates_run)" = 0 ] || { echo "        ran a gate with int-dir == canonical"; return 1; }
    assert_shared_untouched
}

case_refuses_dirty_root() {
    prep 2
    printf 'half-written\n' >> "$F/agent/index.md"
    run_batch "$1" "$F/agent" --width 2 br1 br2
    [ "$(brc)" = 2 ]       || { echo "        rc $(brc), want 2"; return 1; }
    [ "$(gates_run)" = 0 ] || { echo "        gated out of a dirty ROOT"; return 1; }
    outhas "has modified tracked files" || {
        echo "        did not say the tree had modified tracked files"; return 1; }
    return 0
}

# 9. A COLLISION IS NOT A VERDICT. Two branches rewriting the same line cannot
#    both cherry-pick; the loser is EXCLUDED, the batch still runs, and nothing
#    is reported RED. Collision wants a merge driver, RED wants a code change.
case_collision_excluded_not_red() {
    prep 2
    (
        cd "$F/repo"
        $G checkout -q -b clashA main
        printf '# index\nCLASH A\n' > index.md && $G add index.md && $G commit -qm "clashA"
        $G push -q origin clashA
        $G checkout -q -b clashB main
        printf '# index\nCLASH B\n' > index.md && $G add index.md && $G commit -qm "clashB"
        $G push -q origin clashB
        $G checkout -q held
    ) >/dev/null 2>&1
    run_batch "$1" "$F/agent" --width 2 clashA clashB
    [ "$(brc)" = 0 ] || { echo "        rc $(brc), want 0 — a collision is not a failure"; return 1; }
    outhas "COLLISION, not a verdict" || { echo "        did not call it a collision"; return 1; }
    [ "$(verdict clashB)" = EXCLUDED-COLLISION ] || {
        echo "        clashB verdict $(verdict clashB), want EXCLUDED-COLLISION"; return 1; }
    grep -qE '^clash[AB] +RED$' "$WORK/out" && {
        echo "        a collision was reported as RED"; return 1; }
    assert_shared_untouched
}

# 10. AN INTERACTION RED CONDEMNS NOBODY. The pair is red, each member is green
#     alone, so the fault is the combination and no member may be blamed.
case_interaction_condemns_nobody() {
    prep 2
    printf 'file1.txt file2.txt\n' > "$WORK/inter.txt"
    run_batch "$1" "$F/agent" --width 2 --on-red singles br1 br2
    [ "$(gates_run)" = 3 ] || { echo "        $(gates_run) gates, want 3"; return 1; }
    outhas "INTERACTION" || { echo "        did not report an interaction red"; return 1; }
    outhas "red only in COMBINATION" || { echo "        did not explain the interaction"; return 1; }
    local b
    for b in br1 br2; do
        [ "$(verdict $b)" = GREEN-INTERACTION ] || {
            echo "        $b verdict $(verdict $b), want GREEN-INTERACTION"; return 1; }
    done
    grep -qE '^br[12] +RED$' "$WORK/out" && {
        echo "        a member was condemned for an interaction"; return 1; }
    assert_shared_untouched
}

# 11. --land is the owner's switch: a GREEN batch moves nothing without it.
case_land_off_by_default() {
    prep 2
    local before lbefore; before="$(omain)"; lbefore="$(lmain)"
    run_batch "$1" "$F/agent" --width 2 br1 br2
    [ "$(brc)" = 0 ]           || { echo "        rc $(brc), want 0"; return 1; }
    [ "$(gates_run)" = 1 ]     || { echo "        $(gates_run) gates, want 1"; return 1; }
    [ "$(omain)" = "$before" ] || { echo "        ORIGIN MAIN MOVED without --land"; return 1; }
    [ "$(lmain)" = "$lbefore" ] || { echo "        LOCAL MAIN MOVED without --land"; return 1; }
    outhas "NOT LANDED"        || { echo "        did not say it had not landed"; return 1; }
    outhas "branches landed: 0" || { echo "        reported a landing"; return 1; }
    assert_shared_untouched
}

# 12. Landing is refused while ANY worktree holds main — advancing the ref
#     under it is the 861-refusal bug by another door.
case_land_refused_main_held() {
    prep 2
    $G -C "$F/repo" checkout -q main
    SHARED_H0="$(shared_head)"
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    [ "$(brc)" = 3 ]           || { echo "        rc $(brc), want 3"; return 1; }
    [ "$(omain)" = "$before" ] || { echo "        main moved while a worktree held it"; return 1; }
    outhas "LAND REFUSED  main is checked out at" || {
        echo "        did not refuse for the held checkout"; return 1; }
    assert_shared_untouched
}

# 13. THE LEASE. origin's main moves during the gate; the batch was gated
#     against a base that is no longer main, so it must not land. Note the
#     local remote-tracking ref does NOT know — this is exactly why the check
#     asks the remote with ls-remote rather than trusting refs/remotes.
case_land_refused_lease_lost() {
    prep 2
    # THE INTERLOPER IS A SEPARATE CLONE, and that is the whole point of the
    # case. Pushing from $F/repo would update refs/remotes/origin/main in the
    # SAME repository the batch is reading — a tracking ref shared by all its
    # worktrees — so the cheap `origin/main moved` check would catch it and the
    # ls-remote lease would never be exercised. That is what the first version
    # did, and injection `land_ignores_lease` duly stayed GREEN: the case was
    # measuring a check other than the one it was written for. A real second
    # writer is a different clone, and against one of those the tracking ref is
    # stale and ls-remote is the only thing that can know.
    $G clone -q "$F/origin.git" "$WORK/interloper" 2>/dev/null
    cat > "$WORK/hook-1" <<EOH
set -e
cd "$WORK/interloper"
git config user.email i@i; git config user.name i
printf 'somebody else landed first\n' >> index.md
git add index.md && git commit -qm "interloper"
git push -q origin main
EOH
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    [ "$(brc)" = 3 ] || { echo "        rc $(brc), want 3"; return 1; }
    ofiles | grep -c '^file1.txt$' >/dev/null && { echo "        landed over the interloper"; return 1; }
    outhas "branches landed: 0" || { echo "        reported a landing"; return 1; }
    # THE SPECIFIC MESSAGE, not just "did not land". Git's own non-fast-forward
    # rejection also stops this, so asserting only the outcome makes the lease
    # untestable — the injection stayed GREEN on the first run for exactly that.
    # The property is that the lease refuses BEFORE anything is written: by the
    # time git rejects the push, `git update-ref` has already advanced local
    # main and we are relying on the rollback. Not needing a repair beats
    # having one.
    outhas "LAND REFUSED  the remote's main is" || {
        echo "        refused, but not by the lease — no pre-write refusal"; return 1; }
    outhas "rolled back" && { echo "        needed a rollback; the lease should have refused first"; return 1; }
    return 0
}

# 14. Width splits the queue, and every group is its own gate.
case_width_splits_groups() {
    prep 5
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 2 --land br1 br2 br3 br4 br5
    [ "$(brc)" = 0 ]       || { echo "        rc $(brc), want 0"; return 1; }
    [ "$(gates_run)" = 3 ] || { echo "        $(gates_run) gates, want 3 (2+2+1)"; return 1; }
    [ "$($G -C "$F/origin.git" rev-list --count "$before..main")" = 5 ] || {
        echo "        main advanced by $($G -C "$F/origin.git" rev-list --count "$before..main"), want 5"; return 1; }
    outhas "branches landed: 5" || { echo "        did not report 5 landed"; return 1; }
    assert_shared_untouched
}

# 15. A gate directory a live handoff owns is refused, and the owner is found by
#     reading RUSTMC_REMOTE_DIR out of /proc/<pid>/environ rather than by
#     matching a process name.
case_remote_dir_conflict_refused() {
    prep 2
    mkdir -p "$WORK/fakegate/ci"
    printf '#!/usr/bin/env bash\nsleep 45\n' > "$WORK/fakegate/ci/handoff.sh"
    RUSTMC_REMOTE_DIR="$REMOTE_DIR" bash "$WORK/fakegate/ci/handoff.sh" &
    FAKE_PID=$!
    sleep 0.4
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    stop_fake_gate
    [ "$(gates_run)" = 0 ] || { echo "        gated into a dir a live handoff owns"; return 1; }
    outhas "is owned by a live ci/handoff.sh" || {
        echo "        did not refuse for the remote-dir conflict"; return 1; }
    outhas "/environ" || { echo "        did not say how the owner was resolved"; return 1; }
    assert_shared_untouched
}

# 16/17. An INFRA exit and an UNRECOGNISED exit are not verdicts. This is
#     ci/handoff.sh's own INFRA distinction, carried through the batch: a red
#     sends the reader to a diff, and a diff that had nothing to do with it is
#     the most expensive wrong turn this project makes.
case_gate_infra_is_not_red() {
    prep 2
    echo 4 > "$WORK/force-exit"
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    [ "$(brc)" = 4 ]           || { echo "        rc $(brc), want 4"; return 1; }
    [ "$(omain)" = "$before" ] || { echo "        main moved on an infra gate"; return 1; }
    [ "$(verdict br1)" = UNJUDGED-INFRA ] || {
        echo "        br1 verdict $(verdict br1), want UNJUDGED-INFRA"; return 1; }
    assert_shared_untouched
}

case_unknown_exit_is_not_red() {
    prep 2
    echo 77 > "$WORK/force-exit"
    run_batch "$1" "$F/agent" --width 2 --on-red singles --land br1 br2
    [ "$(brc)" = 4 ] || { echo "        rc $(brc), want 4"; return 1; }
    [ "$(verdict br1)" = UNJUDGED-EXIT77 ] || {
        echo "        br1 verdict $(verdict br1), want UNJUDGED-EXIT77"; return 1; }
    grep -qE '^br[12] +RED$' "$WORK/out" && {
        echo "        an unrecognised exit code condemned a branch"; return 1; }
    assert_shared_untouched
}

# 19. --plan answers the cheap half — will these cherry-pick together — for
#     seconds instead of 80 minutes, and runs no gate at all.
case_plan_runs_no_gate() {
    prep 3
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 3 --plan br1 br2 br3
    [ "$(gates_run)" = 0 ]     || { echo "        --plan ran $(gates_run) gates"; return 1; }
    [ "$(omain)" = "$before" ] || { echo "        --plan moved main"; return 1; }
    outhas "PLAN     group 1: admitted 3" || { echo "        did not report the plan"; return 1; }
    [ "$(verdict br2)" = PLAN-OK ] || { echo "        br2 verdict $(verdict br2)"; return 1; }
    assert_shared_untouched
}

case_max_gates_budget() {
    prep 4
    run_batch "$1" "$F/agent" --width 1 --max-gates 2 --land br1 br2 br3 br4
    [ "$(gates_run)" = 2 ] || { echo "        $(gates_run) gates, want 2"; return 1; }
    outhas "--max-gates 2 reached" || { echo "        did not report the budget stop"; return 1; }
    [ "$(verdict br4)" = UNJUDGED-BUDGET ] || {
        echo "        br4 verdict $(verdict br4), want UNJUDGED-BUDGET"; return 1; }
    assert_shared_untouched
}

# 21. Unpushed local work is NOT batched, and the run says so. AGENT-BRIEF's
#     most expensive documented mistake is work committed and never pushed; a
#     batch that silently gated a local tip would land a commit nothing
#     downstream has.
case_unpushed_local_not_batched() {
    prep 2
    (
        cd "$F/repo" && $G checkout -q br1
        printf 'local only\n' > local-only.txt
        $G add local-only.txt && $G commit -qm "br1: not pushed"
        $G checkout -q held
    ) >/dev/null 2>&1
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    outhas "local commit(s) are NOT in this batch" || {
        echo "        did not report the unpushed commit"; return 1; }
    ofiles | grep -c '^local-only.txt$' >/dev/null && {
        echo "        LANDED AN UNPUSHED COMMIT"; return 1; }
    ofiles | grep -c '^file1.txt$' >/dev/null || { echo "        br1's pushed commit did not land"; return 1; }
    assert_shared_untouched
}

case_missing_branch_skipped() {
    prep 2
    run_batch "$1" "$F/agent" --width 3 --land br1 nosuchbranch br2
    [ "$(verdict nosuchbranch)" = SKIP-MISSING ] || {
        echo "        verdict $(verdict nosuchbranch), want SKIP-MISSING"; return 1; }
    [ "$(gates_run)" = 1 ]  || { echo "        $(gates_run) gates, want 1"; return 1; }
    outhas "branches landed: 2" || { echo "        did not land the two real branches"; return 1; }
    assert_shared_untouched
}

# 23. A base that disagrees with origin/main cannot be landed by either route,
#     because ci/handoff.sh measures BEHIND against LOCAL main and nothing in
#     the chain fetches. Refused before any gate is paid for.
case_refuses_stale_base() {
    prep 2
    (
        cd "$F/repo" && $G checkout -q -B main
        printf 'local advance\n' >> index.md
        $G add index.md && $G commit -qm "local main is ahead of origin"
        $G checkout -q held
    ) >/dev/null 2>&1
    SHARED_H0="$(shared_head)"
    run_batch "$1" "$F/agent" --width 2 --land br1 br2
    [ "$(brc)" = 2 ]       || { echo "        rc $(brc), want 2"; return 1; }
    [ "$(gates_run)" = 0 ] || { echo "        gated a tip that cannot land"; return 1; }
    outhas "!= origin/main" || { echo "        did not name the disagreement"; return 1; }
    assert_shared_untouched
}

case_merge_commit_member_excluded() {
    prep 2
    (
        cd "$F/repo"
        $G checkout -q -b sidebr main
        printf 'side\n' > side.txt && $G add side.txt && $G commit -qm "side"
        $G checkout -q -b mergebr main
        printf 'mine\n' > mine.txt && $G add mine.txt && $G commit -qm "mine"
        $G merge -q --no-ff -m "merge side into mergebr" sidebr
        $G push -q origin mergebr
        $G checkout -q held
    ) >/dev/null 2>&1
    run_batch "$1" "$F/agent" --width 2 --land br1 mergebr
    [ "$(verdict mergebr)" = EXCLUDED-MERGECOMMITS ] || {
        echo "        mergebr verdict $(verdict mergebr), want EXCLUDED-MERGECOMMITS"; return 1; }
    grep -qE '^mergebr +RED$' "$WORK/out" && {
        echo "        a merge commit was reported as RED"; return 1; }
    outhas "branches landed: 1" || { echo "        br1 did not land alone"; return 1; }
    assert_shared_untouched
}

# 25. Batch branches are refs in the SHARED repo. Left behind they accumulate
#     one per group forever in the tree every other agent works in.
case_batch_refs_cleaned_up() {
    prep 4
    run_batch "$1" "$F/agent" --width 2 --land br1 br2 br3 br4
    [ "$(batch_refs)" = 0 ] || { echo "        $(batch_refs) batch-* refs left behind"; return 1; }
    [ -e "$WORK/int" ] && { echo "        integration worktree left behind"; return 1; }
    assert_shared_untouched
}

# 25. GROUPING BY DISJOINT FILE SETS. Two branches that rewrite the same line
#     cannot share a tip; in queue order they are sliced into the same group and
#     one is excluded. --pack puts them in different groups and nothing is lost.
#     Measured worth on the real queue: 8 of 36 collide in queue order.
case_pack_avoids_collisions() {
    prep 1
    (
        cd "$F/repo"
        $G checkout -q -b clashA main
        printf '# index\nCLASH A\n' > index.md && $G add index.md && $G commit -qm clashA
        $G push -q origin clashA
        $G checkout -q -b clashB main
        printf '# index\nCLASH B\n' > index.md && $G add index.md && $G commit -qm clashB
        $G push -q origin clashB
        $G checkout -q held
    ) >/dev/null 2>&1
    # In queue order the two clashers land in one group and one is excluded.
    run_batch "$1" "$F/agent" --width 3 --plan clashA clashB br1
    outhas "COLLISION" || {
        echo "        queue order did not collide, so --pack has nothing to prove here"; return 1; }
    # Packed, they are separated and nothing collides.
    run_batch "$1" "$F/agent" --width 3 --plan --pack clashA clashB br1
    outhas "COLLISION" && { echo "        --pack still collided"; return 1; }
    outhas "--pack: 2 groups by disjoint file sets" || {
        echo "        did not pack into 2 groups"; return 1; }
    outhas "group sizes: 2,1" || { echo "        group sizes not 2,1"; return 1; }
    [ "$(verdict clashB)" = PLAN-OK ] || {
        echo "        clashB verdict $(verdict clashB), want PLAN-OK"; return 1; }
    assert_shared_untouched
}

# 26. ci/handoff.sh's own state checks are asked BEFORE the 80 minutes, and the
#     answer is reported per group. The refusal branch is defence-in-depth
#     against states int_prepare normally prevents — see ci/batch.sh's header —
#     so what is proved here is that the check is evaluated and that its failure
#     path reaches the right verdict (injection precondition_always_fails).
case_plan_reports_preconditions() {
    prep 3
    run_batch "$1" "$F/agent" --width 3 --plan br1 br2 br3
    outhas "preconditions: ok" || {
        echo "        did not report the gate preconditions"; return 1; }
    [ "$(gates_run)" = 0 ] || { echo "        --plan ran a gate"; return 1; }
    assert_shared_untouched
}

# 27. THE RATE GUARD. Batching is a win CONDITIONAL on p, and below p=0.6934 no
#     width beats serial. A batch run that does not look at p could be making the
#     queue slower while reporting a saving — a green that cannot go red. So a
#     low measured rate must REFUSE the land, before any gate is spent.
case_land_refused_on_low_rate() {
    prep 2
    seed_rate 5 20                       # p = 0.20 at N = 25 -> SERIAL
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 3 --land br1 br2
    [ "$(brc)" = 2 ]           || { echo "        rc $(brc), want 2"; return 1; }
    [ "$(gates_run)" = 0 ]     || { echo "        spent $(gates_run) gates at p=0.20"; return 1; }
    [ "$(omain)" = "$before" ] || { echo "        main moved"; return 1; }
    outhas "costs MORE per landed branch than gating" || {
        echo "        did not explain that batching loses at this rate"; return 1; }
    outhas "verdict=SERIAL" || { echo "        did not report the arithmetic's verdict"; return 1; }
    assert_shared_untouched
}

# 28. AND NO RATE IS NOT PERMISSION. Too few gates to call p a rate must refuse
#     too: the absence of the measurement is not evidence that batching helps.
case_land_refused_on_tiny_n() {
    prep 2
    seed_rate 3 1                        # p = 0.75 but N = 4
    run_batch "$1" "$F/agent" --width 3 --land br1 br2
    [ "$(brc)" = 2 ]       || { echo "        rc $(brc), want 2"; return 1; }
    [ "$(gates_run)" = 0 ] || { echo "        spent a gate on an unmeasured rate"; return 1; }
    outhas "too few to call p a rate" || { echo "        did not refuse for N"; return 1; }
    assert_shared_untouched
}

# 29. --accept-rate is the deliberate override and it must say so, because a
#     guard that can be bypassed silently is a guard that will be.
case_accept_rate_overrides() {
    prep 2
    seed_rate 5 20
    local before; before="$(omain)"
    run_batch "$1" "$F/agent" --width 3 --land --accept-rate br1 br2
    [ "$(brc)" = 0 ]        || { echo "        rc $(brc), want 0"; return 1; }
    [ "$(gates_run)" = 1 ]  || { echo "        $(gates_run) gates, want 1"; return 1; }
    [ "$(omain)" != "$before" ] || { echo "        --accept-rate did not land"; return 1; }
    outhas "--accept-rate: landing anyway" || {
        echo "        overrode the guard without saying so"; return 1; }
    assert_shared_untouched
}

# 30. And the guard must not fire when nothing is being landed: --plan and a
#     report-only run measure a batch and move nothing, so a poor rate is
#     information rather than a refusal.
case_rate_guard_silent_without_land() {
    prep 2
    seed_rate 5 20
    run_batch "$1" "$F/agent" --width 3 --plan br1 br2
    [ "$(brc)" = 0 ] || { echo "        rc $(brc), want 0 — --plan lands nothing"; return 1; }
    outhas "costs MORE per landed branch" && {
        echo "        refused a --plan run over the rate"; return 1; }
    assert_shared_untouched
}

# 31. THE CORPUS-LOCK SAVING, WHICH MAY BE THE BIGGER ONE. Two branches each add
#     a gameplay scenario. Neither can pass ci/corpus-falsifiable.py --check
#     until it appears in a recording, and a recording costs one ~40-minute
#     corpus-lock acquisition. On one integration tip, ONE recording registers
#     both — so the assertion is that exactly one recording ran and both
#     scenarios ended up in the tip's ledger.
case_sweep_one_recording_covers_two() {
    prep 2; build_scenario_fixture
    run_batch "$1" "$F/agent" --width 2 --sweep --land br1 br2
    [ "$(record_calls)" = 1 ] || {
        echo "        $(record_calls) recordings, want exactly 1 for 2 branches"; return 1; }
    outhas "2 new scenario(s) across 2 member(s) — ONE recording covers all" || {
        echo "        did not report one recording covering both"; return 1; }
    outhas "ledger committed onto batch-" || { echo "        did not commit the ledger"; return 1; }
    local led; led="$(origin_file main ci/checks/gameplay-falsifiable.toml)"
    printf '%s' "$led" | grep -c '901-new1.toml' >/dev/null || { echo "        br1's entry not on main"; return 1; }
    printf '%s' "$led" | grep -c '902-new2.toml' >/dev/null || { echo "        br2's entry not on main"; return 1; }
    assert_shared_untouched
}

# 32. AND IT MUST NOT TAKE THE LOCK FOR NOTHING. A tree whose scenarios are all
#     registered needs no recording, and 40 minutes spent discovering that is 40
#     minutes out of the queue of somebody who did need it.
case_sweep_skips_when_nothing_unregistered() {
    prep 2; build_scenario_fixture
    echo green > "$WORK/cf-state"          # everything already registered
    run_batch "$1" "$F/agent" --width 2 --sweep --land br1 br2
    [ "$(record_calls)" = 0 ] || {
        echo "        took the lock $(record_calls) time(s) with nothing to sweep"; return 1; }
    outhas "already carries evidence" || { echo "        did not say why it skipped"; return 1; }
    assert_shared_untouched
}

# 33. AND IT MUST NOT QUEUE AGAINST A PRODUCTION MERGE GATE. ci/handoff.sh runs
#     the recorder as role=gate, which jumps the whole queue because it is the
#     only thing that can move main. A measurement that starts alongside it
#     competes for the box; one did today and a production gate waited 25 min.
case_sweep_refuses_under_merge_gate() {
    prep 2; build_scenario_fixture
    plant_merge_gate
    run_batch "$1" "$F/agent" --width 2 --sweep --land br1 br2
    [ "$(record_calls)" = 0 ] || {
        echo "        recorded while a merge gate held the lock"; return 1; }
    outhas "a production merge gate holds or waits the corpus lock" || {
        echo "        did not refuse for the merge gate"; return 1; }
    assert_shared_untouched
}

# 34. A RED BATCH MUST NOT WASTE THE RECORDING. --sweep-back grafts each member's
#     OWN rows onto its OWN branch, so the 40 minutes survives a verdict that had
#     nothing to do with it. And only its own: br1 must not receive br2's
#     evidence, because an entry ties a scenario to the version it was taken
#     against and cross-grafting would be evidence about something else.
case_sweep_back_grafts_own_rows_only() {
    prep 2; build_scenario_fixture
    echo 'file2.txt' > "$WORK/red.txt"      # the batch will be RED
    run_batch "$1" "$F/agent" --width 2 --sweep-back --on-red none --land br1 br2
    [ "$(record_calls)" = 1 ] || { echo "        $(record_calls) recordings, want 1"; return 1; }
    local l1 l2
    l1="$(origin_file br1 ci/checks/gameplay-falsifiable.toml)"
    l2="$(origin_file br2 ci/checks/gameplay-falsifiable.toml)"
    printf '%s' "$l1" | grep -c '901-new1.toml' >/dev/null || { echo "        br1 did not get its own row"; return 1; }
    printf '%s' "$l2" | grep -c '902-new2.toml' >/dev/null || { echo "        br2 did not get its own row"; return 1; }
    printf '%s' "$l1" | grep -c '902-new2.toml' >/dev/null && { echo "        br1 got br2's row"; return 1; }
    printf '%s' "$l2" | grep -c '901-new1.toml' >/dev/null && { echo "        br2 got br1's row"; return 1; }
    outhas "SWEEP-BACK br1" || { echo "        did not report the graft"; return 1; }
    assert_shared_untouched
}

# 35. A branch an agent may be live in is not ours to append to. Held branches
#     are skipped by name, with the reason, and the others are still grafted.
case_sweep_back_skips_held_branch() {
    prep 2; build_scenario_fixture
    $G -C "$F/repo" worktree add "$WORK/holdbr1" br1 >/dev/null 2>&1
    run_batch "$1" "$F/agent" --width 2 --sweep-back --land br1 br2
    outhas "SWEEP-BACK skip br1" || { echo "        did not skip the held branch"; return 1; }
    outhas "an agent may be live" || { echo "        did not say why"; return 1; }
    local l2; l2="$(origin_file br2 ci/checks/gameplay-falsifiable.toml)"
    printf '%s' "$l2" | grep -c '902-new2.toml' >/dev/null || { echo "        br2 was skipped too"; return 1; }
    $G -C "$F/repo" worktree remove --force "$WORK/holdbr1" >/dev/null 2>&1
    return 0
}

CASES="two_green_land_both red_batch_lands_none red_batch_singles_names_regated
sweep_one_recording_covers_two sweep_skips_when_nothing_unregistered
sweep_refuses_under_merge_gate sweep_back_grafts_own_rows_only
sweep_back_skips_held_branch
land_refused_on_low_rate land_refused_on_tiny_n accept_rate_overrides
rate_guard_silent_without_land
pack_avoids_collisions plan_reports_preconditions
shared_dirty_is_caught refuses_root_is_canonical refuses_int_dir_is_root
refuses_int_dir_is_canonical refuses_dirty_root collision_excluded_not_red
interaction_condemns_nobody land_off_by_default land_refused_main_held
land_refused_lease_lost width_splits_groups remote_dir_conflict_refused
gate_infra_is_not_red unknown_exit_is_not_red plan_runs_no_gate
max_gates_budget unpushed_local_not_batched missing_branch_skipped
refuses_stale_base merge_commit_member_excluded batch_refs_cleaned_up"

run_case() { case_"$1" "$2"; }

# ---------------------------------------------------------------------------
# THE INJECTIONS
# ---------------------------------------------------------------------------
# Each is a patch against a COPY of ci/batch.sh plus the one case that must
# catch it. A patch that does not change the file is a hole in the sweep and is
# reported as MISSED, not quietly skipped: a sweep whose patches have stopped
# matching reports all-green while testing nothing.
declare -A CATCHER=(
    [canonical_guard_off]=refuses_root_is_canonical
    [int_dir_guard_off]=refuses_int_dir_is_root
    [dirty_root_ok]=refuses_dirty_root
    [shared_assert_blind]=shared_dirty_is_caught
    [tip_content_dropped]=two_green_land_both
    [land_ignores_lease]=land_refused_lease_lost
    [land_ignores_main_checkout]=land_refused_main_held
    [red_condemns_last_member]=interaction_condemns_nobody
    [unknown_exit_is_red]=unknown_exit_is_not_red
    [lands_without_the_switch]=land_off_by_default
    [collision_is_a_verdict]=collision_excluded_not_red
    [batch_refs_leak]=batch_refs_cleaned_up
    [precondition_always_fails]=two_green_land_both
    [pack_ignores_disjointness]=pack_avoids_collisions
    [rate_guard_off]=land_refused_on_low_rate
    [sweep_back_grafts_everything]=sweep_back_grafts_own_rows_only
    [sweep_back_ignores_held_branch]=sweep_back_skips_held_branch
    [sweeptip_ignores_gate_contention]=sweep_refuses_under_merge_gate
    [sweeptip_always_records]=sweep_skips_when_nothing_unregistered
)

# PATCHING WITHOUT AN ESCAPING LAYER, AND WHY THAT IS NOT FUSSINESS.
#
# The obvious way to write these is `perl -0pi -e 's{\Q...\E}{...}'`, and it
# does not work: inside `\Q...\E` perl still INTERPOLATES, so
# `\Qif [ -n "$held" ]\E` quotes the brackets and then expands `$held` to the
# empty string, and the pattern silently stops matching the file it was written
# against. Backslashing every `$` and `@` fixes that and introduces the next
# one — a `!` or `{`/`}` delimiter collides with `!=` and with the braces in
# the shell code being matched.
#
# Ten of these twelve patches were dead on their first run and two were
# matching the WRONG TEXT, which is worse: a patch that matches something else
# reports PATCH-OK and tests nothing. So the pattern travels as an ordinary
# string in the environment and is `quotemeta`d at run time. There is then no
# escaping layer to get wrong, and the literal below is exactly the literal in
# ci/batch.sh.
#
# The DRY RUN is what found all of this. That is the argument for having one.
# AN AMBIGUOUS ANCHOR IS A PATCH THAT WILL EVENTUALLY HIT THE WRONG TEXT, and
# `s///` silently takes the first match. That is not hypothetical: adding
# `sweep_back` to ci/batch.sh introduced a second `if [ -n "$held" ]; then`
# ABOVE land_tip's, so injection `land_ignores_main_checkout` started patching
# the new one, reported PATCH-OK, and its case stayed green under a break that
# had not been applied. So this refuses a pattern that matches more than once and
# says so, which turns that failure from silent into a MISSED line.
subst() {   # subst <file> <literal-pattern> <replacement>
    SUBST_PAT="$2" SUBST_REP="$3" perl -0pi -e '
        my $p = quotemeta($ENV{SUBST_PAT});
        my $r = $ENV{SUBST_REP};
        my $n = () = /$p/g;
        if ($n > 1) {
            print STDERR "subst: AMBIGUOUS anchor, $n matches — refusing to patch\n";
            exit 0;
        }
        s/$p/$r/s;
    ' "$1"
}
subst_span() {   # subst_span <file> <literal-start> <literal-end> <replacement>
    SUBST_A="$2" SUBST_B="$3" SUBST_REP="$4" perl -0pi -e '
        my $p = quotemeta($ENV{SUBST_A}) . "(?s:.*?)" . quotemeta($ENV{SUBST_B});
        my $r = $ENV{SUBST_REP};
        s/$p/$r/s;
    ' "$1"
}

inject() {
    local name="$1" src="$WORK/inj-$name.sh"
    cp "$BATCH_REAL" "$src"
    case "$name" in
    canonical_guard_off)
        subst "$src" 'if [ -d "$CANONICAL" ] && [ "$ROOT" -ef "$CANONICAL" ]; then' \
                     'if false; then' ;;
    int_dir_guard_off)
        subst "$src" 'if [ -e "$INT_DIR" ] && [ "$INT_DIR" -ef "$ROOT" ]; then' \
                     'if false; then' ;;
    dirty_root_ok)
        subst_span "$src" '[ -z "$(git status --porcelain --untracked-files=no)" ] || {' \
                          "exit 2; }" 'true' ;;
    shared_assert_blind)
        # The shape this guards against is not malice, it is a guard that was
        # written and then stopped being reached.
        subst "$src" 'assert_shared_clean() {' \
"assert_shared_clean() { echo \"SHARED-CHECKOUT  untouched (blinded)\"; return 0; }
assert_shared_clean_unused() {" ;;
    tip_content_dropped)
        # A tip that contains none of its members, gating GREEN. The patch-id
        # check is what must refuse it; without that check this lands nothing
        # while reporting N landings.
        subst "$src" '        ADMITTED+=("$name")' \
                     '        ADMITTED+=("$name"); git -C "$INT_DIR" reset --hard -q "$BASE" >/dev/null 2>&1' ;;
    land_ignores_lease)
        subst "$src" 'if [ "$remote_sha" != "$base" ]; then' 'if false; then' ;;
    land_ignores_main_checkout)
        # Anchored on the line above it: `if [ -n "$held" ]; then` alone also
        # matches sweep_back's held-branch guard, which sits earlier in the file.
        subst "$src" 'held="$(main_held_by | head -1)"
    if [ -n "$held" ]; then' 'held="$(main_held_by | head -1)"
    if false; then' ;;
    red_condemns_last_member)
        # THE UNSOUND INFERENCE, implemented. Re-gate all but the last member and
        # condemn the last for free. It is 0.0513 gates per landed branch cheaper
        # (ci/batch-arith.py) and it is FALSE for an interaction red, which is
        # the case that catches it.
        subst "$src" 'for name in "${members[@]}"; do run_group "$name"; done' \
'for name in "${members[@]:0:$((${#members[@]}-1))}"; do run_group "$name"; done; VERDICT["${members[$((${#members[@]}-1))]}"]="RED"; RED_SEEN=1; RC_FINAL=1' ;;
    unknown_exit_is_red)
        # Anchored on the arm's own comment, not on a fragment of the line
        # inside it: the first version started the span mid-statement and left
        # `VERDICT["$name"]=": ;;` — a patch that "matched" and produced broken
        # bash, which every case would then fail for the wrong reason.
        subst_span "$src" '      *)
        # An exit code this script does not recognise' 'return 4 ;;' \
'      *)
        : ;;' ;;
    lands_without_the_switch)
        subst "$src" '    if [ -z "$LAND" ]; then' '    if false; then' ;;
    collision_is_a_verdict)
        subst "$src" 'EXCLUDED+=("$name:COLLISION")' 'EXCLUDED+=("$name:RED")' ;;
    batch_refs_leak)
        subst "$src" 'git branch -D "$b"' 'true "$b"' ;;
    precondition_always_fails)
        # Proves the precondition check is WIRED into the gating path and that
        # its failure reaches UNJUDGED-PRECONDITION rather than a verdict.
        subst "$src" 'gate_preconditions() {   # branch -> 0 if ci/handoff.sh'"'"'s state checks would pass' \
                     'gate_preconditions() { echo "injected"; return 1; }
gate_preconditions_unused() {' ;;
    sweep_back_grafts_everything)
        # Graft every member's rows to every member: cheaper to write and it
        # attaches evidence to a scenario it was not taken against.
        subst "$src" 'files="$(member_new_scenarios "$name" | tr '"'"'\n'"'"' '"'"' '"'"')"
        [ -n "${files// /}" ] || continue' \
'files="$(for m in "${mine[@]}"; do member_new_scenarios "$m"; done | tr '"'"'\n'"'"' '"'"' '"'"')"
        [ -n "${files// /}" ] || continue' ;;
    sweep_back_ignores_held_branch)
        subst "$src" 'if [ -n "$held" ]; then
            say "SWEEP-BACK skip $name — held by the worktree at $held, an agent may be live"' \
'if false; then
            say "SWEEP-BACK skip $name — held by the worktree at $held, an agent may be live"' ;;
    # --- these two patch ci/sweep-tip.sh, not ci/batch.sh -------------------
    # inject() hands run_batch a batch.sh; for these the batch.sh is untouched
    # and SWEEP_TIP_SRC points the harness at the broken sweep-tip instead.
    # NOTE inject() is called inside $( ), which is a SUBSHELL, so a variable
    # set here would be lost. The path is handed over through a file.
    sweeptip_ignores_gate_contention)
        local st="$WORK/inj-$name-sweep-tip.sh"
        cp "$ROOT/ci/sweep-tip.sh" "$st"
        subst "$st" 'if owner="$(gate_owner)"; then' 'if false; then'
        if cmp -s "$ROOT/ci/sweep-tip.sh" "$st"; then echo "$src"; return 1; fi
        printf '%s' "$st" > "$WORK/sweep-tip-src"
        echo "$src"; return 0 ;;
    sweeptip_always_records)
        local st="$WORK/inj-$name-sweep-tip.sh"
        cp "$ROOT/ci/sweep-tip.sh" "$st"
        subst "$st" 'if [ "$CHECK_RC" -eq 0 ] && [ "${NEED:-0}" -eq 0 ]; then' 'if false; then'
        if cmp -s "$ROOT/ci/sweep-tip.sh" "$st"; then echo "$src"; return 1; fi
        printf '%s' "$st" > "$WORK/sweep-tip-src"
        echo "$src"; return 0 ;;
    rate_guard_off)
        # A batch that lands whatever the rate says. It is the shipped default
        # everywhere else and it is why this guard had to be written.
        subst "$src" 'rate_guard() {' 'rate_guard() { return 0; }
rate_guard_unused() {' ;;
    pack_ignores_disjointness)
        # Packing that packs by nothing: every member fits every group, so
        # --pack degenerates into --width slicing and the collision comes back.
        subst "$src" 'if [ -z "$(comm -12 <(printf '"'"'%s\n'"'"' "${gpaths[$i]}") <(printf '"'"'%s\n'"'"' "${P[$name]}"))" ]; then' \
                     'if true; then' ;;
    *)  echo "unknown injection $name" >&2; return 2 ;;
    esac
    if cmp -s "$BATCH_REAL" "$src"; then echo "$src"; return 1; fi
    echo "$src"; return 0
}

# ---------------------------------------------------------------------------
if [ -n "${BATCH_FALSIFY_DRYRUN:-}" ]; then
    echo "batch-falsify: DRY RUN — applying every patch, running no case"
    rc=0
    for name in "${!CATCHER[@]}"; do
        if inject "$name" >/dev/null; then printf '  PATCH-OK   %s\n' "$name"
        else printf '  PATCH-DEAD %s — no longer matches ci/batch.sh\n' "$name"; rc=1; fi
    done
    [ "$rc" -eq 0 ] && echo "batch-falsify: all ${#CATCHER[@]} patches still match" \
                    || echo "batch-falsify: a patch has stopped matching — the sweep has a hole"
    exit "$rc"
fi

echo "batch-falsify: $(printf '%s' "$CASES" | wc -w) cases, ${#CATCHER[@]} injections"
echo
echo "-- the real ci/batch.sh: every case must PASS ---------------------------"
declare -A BASELINE=()
for c in $CASES; do
    if run_case "$c" "$BATCH_REAL"; then BASELINE[$c]=pass; ok "case $c"
    else BASELINE[$c]=fail; bad "case $c — fails on the UNMODIFIED script"; fi
done

echo
echo "-- injections: the named case must go RED -------------------------------"
for name in "${!CATCHER[@]}"; do
    c="${CATCHER[$name]}"
    SWEEP_TIP_SRC=""          # only the sweeptip_* injections set this
    rm -f "$WORK/sweep-tip-src"
    src="$(inject "$name")" || {
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name — patch did not match ci/batch.sh")
        printf '  MISSED  %s — patch did not match; the sweep has a hole here\n' "$name"; continue; }
    SWEEP_TIP_SRC="$(cat "$WORK/sweep-tip-src" 2>/dev/null || true)"
    if [ "${BASELINE[$c]:-}" != pass ]; then
        printf '  SKIP    %s — its catcher (%s) is already failing\n' "$name" "$c"; continue
    fi
    if run_case "$c" "$src"; then
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name — case $c stayed green")
        printf '  MISSED  %s — case %s stayed GREEN under a deliberate break\n' "$name" "$c"
    else
        ok "injection $name caught by case $c"
    fi
done

echo
printf 'batch-falsify: %d passed, %d failed, %d injections not caught\n' "$PASS" "$FAIL" "$MISSED"
[ "$FAIL" -gt 0 ]   && printf '  failed: %s\n' "${FAILED_CASES[@]}"
[ "$MISSED" -gt 0 ] && printf '  uncaught: %s\n' "${MISSED_INJECTIONS[@]}"
[ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]
