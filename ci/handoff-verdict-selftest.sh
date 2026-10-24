#!/usr/bin/env bash
# ci/handoff-verdict-selftest.sh — prove `ci/handoff.sh` says the right WORD.
#
# `RED` means the gate measured the branch and the branch failed. Anything else
# that stops a gate must not say `RED`, because `RED` sends the next reader to
# the diff. On 2026-08-06 a stage-1 failure — a gate directory that could not be
# prepared, before the branch had even been shipped to the box — printed `RED`
# and cost a wrong diagnosis. `INFRA` (exit 4) exists for that, and this file is
# what stops it rotting back.
#
# WHAT THIS DRIVES. The real `ci/handoff.sh`, unmodified, in a throwaway git
# repo, with `HOME` pointed at that repo so it cannot write into the real gate
# log directory. The stages themselves are stand-ins: a fake `ssh` on PATH and a
# fake `ci/t0-remote.sh`. That is not a weakness of the test, it is the seam
# being tested — `handoff.sh` learns what a stage did from exactly two things,
# its exit status and what it wrote to the log, and a stand-in that produces the
# real status and the real sentence is indistinguishable to it. What this cannot
# test is whether a real stage produces those statuses; that is the job of the
# end-to-end run recorded in the commit message.
#
# THE CONTROL MATTERS AS MUCH AS THE CASES. Scenario `green` must reach `GREEN`
# and scenario `t0-branch-red` must reach `RED`. Without those two, a script
# that printed INFRA unconditionally — or one that fell over in setup — would
# pass every other assertion here. This project's recurring defect is a check
# that could not fail, and a verdict test is an easy place to build one.
#
#     ./ci/handoff-verdict-selftest.sh            # all scenarios
#     ./ci/handoff-verdict-selftest.sh -v         # keep and print the gate log
set -uo pipefail

VERBOSE=""
[ "${1:-}" = "-v" ] && VERBOSE=1

SRC="$(cd "$(dirname "$0")" && pwd)/handoff.sh"
[ -f "$SRC" ] || { echo "no handoff.sh beside this script"; exit 2; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
PASS=0
FAIL=0
LAST_OUT=""

# --- the throwaway repo ------------------------------------------------------
build_repo() {
    local break_branch="${1:-}" branch_name="${2:-under-test}"
    local R="$WORK/repo"
    rm -rf "$R"; mkdir -p "$R/ci"
    # HOME LIVES OUTSIDE THE REPO, AND THAT IS THE SECOND REASON THIS FILE
    # COULD NOT PASS. `ci/handoff.sh` opens by creating `$HOME/rustmc-private/
    # gates` for its log, and `HOME` used to be `$R/home` — inside the fixture
    # checkout. So the gate's own first action made the tree untracked-dirty,
    # and the gate's next action is `REFUSED  working tree dirty — an agent may
    # still be live`. Every scenario refused itself, one line after the
    # `gate-lock.sh` refusal above stopped happening.
    rm -rf "$WORK/home"; mkdir -p "$WORK/home"
    git init -q -b main "$R"
    cp "$SRC" "$R/ci/handoff.sh"
    chmod +x "$R/ci/handoff.sh"

    # THIS FILE HAD STOPPED BEING ABLE TO PASS. Measured 2026-08-16: on
    # unmodified main this selftest reported **0 passed, 17 failed**, every one
    # of them `REFUSED  ci/gate-lock.sh is missing — refusing to gate unlocked`.
    # `ci/handoff.sh` grew a hard refusal on a missing `ci/gate-lock.sh` — which
    # is the correct guard, and its own comment says so — but the throwaway repo
    # here only ever copied `handoff.sh`, so the gate under test refused at line
    # one of every scenario and the seventeen verdicts below were never
    # exercised. A `[ "$FAIL" -eq 0 ] || exit 1` at the bottom of a file that
    # cannot reach zero is this project's own defect class, in its own test.
    #
    # The real library is copied rather than stubbed: it is what the branch
    # would source, and a stub would let the refusal above pass against a lock
    # that does not exist. `run_case` points it at a private lock directory so
    # this can neither queue behind nor wedge a real gate.
    cp "$(dirname "$SRC")/gate-lock.sh" "$R/ci/gate-lock.sh"
    chmod +x "$R/ci/gate-lock.sh"

    # stand-in for the laptop-side driver. Stage 2 (--sync) and stage 4 (--run).
    # Both write into the gate log, which is where handoff.sh reads evidence.
    cat > "$R/ci/t0-remote.sh" <<'EOS'
#!/usr/bin/env bash
case "${1:-}" in
--sync)
    case "${GATE_T_SYNC:-ok}" in
    ok)           echo "t0-remote: rsync to ${RUSTMC_REMOTE:-?} ok"; exit 0 ;;
    rsync-failed) echo "t0-remote: rsync to ${RUSTMC_REMOTE:-?} failed" >&2; exit 2 ;;
    branch-broke) echo "t0-remote: --sync: unknown flag in this branch's copy" >&2; exit 2 ;;
    esac ;;
--run)
    case "${GATE_T_T0:-ok}" in
    ok)         echo "T0 GREEN"; exit 0 ;;
    branch-red) echo "error[E0433]: failed to resolve: use of undeclared crate"
                echo "test result: FAILED. 411 passed; 2 failed"; exit 101 ;;
    transport)  echo "ssh: connect to host t port 22: Connection timed out" >&2
                exit 255 ;;
    esac ;;
esac
echo "usage: t0-remote.sh [--sync|--run]" >&2; exit 2
EOS
    chmod +x "$R/ci/t0-remote.sh"

    ( cd "$R" \
      && git add ci/handoff.sh ci/t0-remote.sh ci/gate-lock.sh \
      && git -c user.name=t -c user.email=t@t commit -qm "gate scripts" \
      && git checkout -q -b "$branch_name" \
      && echo change > file.txt && git add file.txt \
      && git -c user.name=t -c user.email=t@t commit -qm "the branch under test" \
      && git checkout -q main ) >/dev/null

    # A branch that is bad in its own committed source, with no test switch
    # involved: the commit on `under-test` ships a driver that will not parse.
    # The gate has to discover that by running it, exactly as it would with any
    # other branch that broke something.
    if [ -n "$break_branch" ]; then
        ( cd "$R" && git checkout -q "$branch_name" \
          && printf '#!/usr/bin/env bash\nif [ broken\n' > ci/t0-remote.sh \
          && git add ci/t0-remote.sh \
          && git -c user.name=t -c user.email=t@t commit -qm "a genuinely bad commit" \
          && git checkout -q main ) >/dev/null
    fi
    echo "$R"
}

# --- the fake ssh ------------------------------------------------------------
# handoff.sh reaches the box three ways and every one of them is `ssh`:
# `remote_script` (stage 1, script on stdin), `remote` (stages 3, 5, 6) and the
# bare ssh inside t0-remote.sh --run, which is stood in for above. The stage is
# identified from the command string, exactly as the real host would see it.
build_ssh() {
    mkdir -p "$WORK/bin"
    cat > "$WORK/bin/ssh" <<'EOS'
#!/usr/bin/env bash
cmd="${*: -1}"
# Every command string the box would have seen, recorded before it is answered.
# `ci/lanes.sh` caps cargo per lane by having `remote()` splice
# `export CARGO_BUILD_JOBS=N;` in here, and a cap that silently fails to arrive
# is invisible downstream: an uncapped gate is not an error, it is just slower
# and it thrashes its neighbour, which reads as a busy box rather than as a
# dropped flag. This log is what makes its arrival assertable.
printf '%s\n' "$cmd" >> "$CMDLOG"
case "$cmd" in
*"bash -s"*)                                  # stage 1: prepare
    case "${GATE_T_STAGE1:-ok}" in
    transport) echo "ssh: connect to host $2 port 22: No route to host" >&2; exit 255 ;;
    silent)    exit 0 ;;                      # returned 0 having run nothing
    seed-gone) cat >/dev/null
               echo "== stage 1: gate dir owns its own oracle-work and data/vanilla"
               echo "   no shared data at /nonexistent-seed to seed from" >&2
               exit 1 ;;
    ok)        cat >/dev/null
               echo "== stage 1: gate dir owns its own oracle-work and data/vanilla"
               echo "   12 references linked, gameplay is a directory"
               echo "STAGE1 OK"; exit 0 ;;
    esac ;;
*gen-gameplay-tables.sh*)                     # stage 3: this branch's tables
    case "${GATE_T_TABLES:-ok}" in
    ok)         echo "gen-gameplay-tables: 12 tables current"; exit 0 ;;
    branch-red) echo "gen-gameplay-tables: no generator_for() case for signal.json"
                exit 2 ;;
    transport)  echo "ssh: connect to host $2 port 22: No route to host" >&2; exit 255 ;;
    esac ;;
*t1-gate.sh*)                                 # stage 5: T1
    case "${GATE_T_T1:-ok}" in
    ok)         echo "T1-GATE GREEN — 99 test binaries"; exit 0 ;;
    branch-red) echo "T1-GATE RED — 3 unexpected failures"; exit 1 ;;
    transport)  echo "ssh: connect to host $2 port 22: No route to host" >&2; exit 255 ;;
    esac ;;
*gate-corpus-stages.sh*)                      # stages 6+7 in one corpus-lock hold
    # `ci/gate-corpus-stages.sh` prints one `GATE-STAGE-RC <n> <rc>` per stage
    # it ran and handoff reads the verdicts from those lines; a transport
    # failure prints none and the call's own status stands in.
    case "${GATE_T_T2:-ok}" in
    ok)         echo "T2-GAMEPLAY GREEN"; echo "GATE-STAGE-RC 6 0"; exit 0 ;;
    branch-red) echo "  REGRESSED  a-piston-extends-when-powered"; echo "GATE-STAGE-RC 6 1"; exit 1 ;;
    transport)  echo "ssh: connect to host $2 port 22: No route to host" >&2; exit 255 ;;
    esac ;;
*t2-gameplay.sh*)                             # stage 6: T2
    case "${GATE_T_T2:-ok}" in
    ok)         echo "T2-GAMEPLAY GREEN"; exit 0 ;;
    branch-red) echo "  REGRESSED  a-piston-extends-when-powered"; exit 1 ;;
    transport)  echo "ssh: connect to host $2 port 22: No route to host" >&2; exit 255 ;;
    esac ;;
esac
echo "fake ssh: unrecognised command: $cmd" >&2; exit 127
EOS
    # The heredoc is quoted so the stub's own `$GATE_T_*` survive to run time;
    # $WORK therefore has to be baked in afterwards. `env -i` in run_case means
    # an exported variable would not reach the stub either way.
    sed -i "2i CMDLOG=\"$WORK/ssh-cmds.txt\"" "$WORK/bin/ssh"
    chmod +x "$WORK/bin/ssh"
}

# --- one scenario ------------------------------------------------------------
# want_word and want_rc are asserted together on purpose: a caller that reads
# the exit code and a human who reads the line must not be able to disagree.
run_case() {
    local name="$1" want_word="$2" want_rc="$3"; shift 3
    local broken=""
    if [ "${1:-}" = "--bad-branch" ]; then broken=1; shift; fi
    local R; R="$(build_repo "$broken")"
    local out rc
    out="$(cd "$R" && env -i \
        PATH="$WORK/bin:/usr/bin:/bin" HOME="$WORK/home" TERM=dumb \
        RUSTMC_REMOTE=fakehost RUSTMC_REMOTE_DIR=rustmc-selftest-gate \
        RUSTMC_GATE_LOCK_DIR="$WORK/gatelock" \
        "$@" ./ci/handoff.sh under-test --dry-run 2>&1)"
    rc=$?
    LAST_OUT="$out"
    local word; word="$(printf '%s\n' "$out" | head -1 | awk '{print $1}')"
    if [ "$word" = "$want_word" ] && [ "$rc" = "$want_rc" ]; then
        PASS=$((PASS + 1))
        printf '  ok    %-22s -> %-7s exit %s\n' "$name" "$word" "$rc"
    else
        FAIL=$((FAIL + 1))
        printf '  FAIL  %-22s -> %-7s exit %s   (wanted %s / exit %s)\n' \
               "$name" "$word" "$rc" "$want_word" "$want_rc"
        printf '%s\n' "$out" | sed 's/^/          /'
    fi
    if [ -n "$VERBOSE" ]; then
        printf '%s\n' "$out" | sed 's/^/          | /'
        local lg; lg="$(ls -1 "$WORK/home/rustmc-private/gates/"*.log 2>/dev/null | head -1)"
        [ -n "$lg" ] && sed 's/^/          # /' "$lg"
    fi
}

build_ssh

echo
echo "[1] stage 1 cannot run — infrastructure, and no branch code exists on the box yet"
run_case "ssh never connected"  INFRA 4 GATE_T_STAGE1=transport
run_case "seed data missing"    INFRA 4 GATE_T_STAGE1=seed-gone
run_case "stage 1 never ran"    INFRA 4 GATE_T_STAGE1=silent

echo
echo "[2] a later stage could not reach the box — still infrastructure"
run_case "stage 3 transport"    INFRA 4 GATE_T_TABLES=transport
run_case "stage 4 transport"    INFRA 4 GATE_T_T0=transport
run_case "stage 5 transport"    INFRA 4 GATE_T_T1=transport
run_case "stage 6 transport"    INFRA 4 GATE_T_T2=transport
run_case "the rsync failed"     INFRA 4 GATE_T_SYNC=rsync-failed

echo
echo "[3] THE CONTROLS — a branch that is actually bad must still be RED"
run_case "T0 failed"            RED   1 GATE_T_T0=branch-red
run_case "T1 failed"            RED   1 GATE_T_T1=branch-red
run_case "T2 regressed"         RED   1 GATE_T_T2=branch-red
run_case "tables: no generator" RED   1 GATE_T_TABLES=branch-red
run_case "branch broke --sync"  RED   1 GATE_T_SYNC=branch-broke
# no test switch in this one: the branch's own committed driver does not parse
run_case "branch ships bad code" RED  1 --bad-branch

echo
echo "[3b] a RED names the first stage that failed, and a skipped stage says so"
#
# `99` is this file's sentinel for a stage skipped because an earlier one was
# red, and nothing said so. On 2026-08-18 between 09:12 and 10:14 the chain
# produced **113 RED verdicts across 40 branches** against a single environment
# fault, every one reading `t0=1/0 t1=99/0 t2=99/0` — naming T1 and T2 as having
# failed with status 99 on 40 branches whose parity was never measured. It is
# the most-travelled path in the script and it was printing a made-up number in
# two of its five fields.
run_case "T0 red, for the line"  RED 1 GATE_T_T0=branch-red
if printf '%s\n' "$LAST_OUT" | grep -c 'first failure: stage 4 T0' >/dev/null; then
    PASS=$((PASS + 1)); printf '  ok    %-22s -> names the first failing stage\n' "red names its cause"
else
    FAIL=$((FAIL + 1)); printf '  FAIL  %-22s -> does not name which stage failed first\n' "red names its cause"
    printf '%s\n' "$LAST_OUT" | sed 's/^/          /'
fi
if printf '%s\n' "$LAST_OUT" | grep -c 't1=skipped/0 t2=skipped/0' >/dev/null; then
    PASS=$((PASS + 1)); printf '  ok    %-22s -> t1=skipped t2=skipped\n' "skipped is not 99"
else
    FAIL=$((FAIL + 1)); printf '  FAIL  %-22s -> a stage that never ran is reported as exit 99\n' "skipped is not 99"
    printf '%s\n' "$LAST_OUT" | sed 's/^/          /'
fi

echo
echo "[4] THE OTHER CONTROL — the happy path is still reachable"
run_case "everything green"     GREEN 0

echo
echo "[5] the discriminator discriminates: same stage, cause is the only change"
run_case "stage 4, branch bad"  RED   1 GATE_T_T0=branch-red
run_case "stage 4, box gone"    INFRA 4 GATE_T_T0=transport

echo
echo "[6] the per-lane job cap reaches the box, and a bad one never does"
#
# `ci/lanes.sh` runs N gates at once and caps cargo so they do not thrash. The
# cap is set in THIS script's environment and nothing there crosses ssh, so
# `remote()` splices it onto the command string. That splice had to be proven
# somewhere, and it cannot be proven by `ci/lanes-falsify.sh`: that file stands
# in for the whole gate, so it never builds a real remote command at all.
#
# Why the value is validated rather than passed through: it is interpolated
# into a string handed to a remote shell. `CARGO_BUILD_JOBS='6; rm -rf ~'` would
# otherwise be a command, not a setting.
: > "$WORK/ssh-cmds.txt"
run_case "job cap carried"      GREEN   0 CARGO_BUILD_JOBS=6
CAP_TOTAL="$(grep -c . "$WORK/ssh-cmds.txt" 2>/dev/null || true)"; CAP_TOTAL="${CAP_TOTAL:-0}"
CAP_WITH="$(grep -c 'export CARGO_BUILD_JOBS=6; ' "$WORK/ssh-cmds.txt" 2>/dev/null || true)"; CAP_WITH="${CAP_WITH:-0}"
if [ "$CAP_TOTAL" -ge 3 ] && [ "$CAP_WITH" = "$CAP_TOTAL" ]; then
    PASS=$((PASS + 1))
    printf '  ok    %-22s -> %s of %s ssh commands carried it\n' "cap on the wire" "$CAP_WITH" "$CAP_TOTAL"
else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %-22s -> %s of %s ssh commands carried it (wanted all, of at least 3)\n' \
           "cap on the wire" "$CAP_WITH" "$CAP_TOTAL"
    sed 's/^/          | /' "$WORK/ssh-cmds.txt"
fi
run_case "cap is not a number"  REFUSED 2 "CARGO_BUILD_JOBS=6; rm -rf ~"
run_case "cap is zero"          REFUSED 2 CARGO_BUILD_JOBS=0
run_case "no cap set at all"    GREEN   0

echo
echo "[6] a branch name with a slash must get a usable log, not a mid-gate crash"
# `scratch-check/*` is an established branch-naming convention here
# (docs/AGENT-BRIEF.md's own worktree examples, and the real branch this case
# is named for: `scratch-check/refcache-bothways`, found queued behind a live
# gate on 2026-08-17). `$BRANCH` becomes a FILENAME in `ci/handoff.sh`'s own
# `$LOG`, so a `/` in it is read as a directory that was never created —
# every `>>"$LOG"` from stage 1 onward then fails silently under `set -uo
# pipefail` (a failed redirect on `echo` is not what `-e` catches), and the
# gate can die mid-stage having already `git checkout`ed the branch: the same
# shape of outage `lanes-canonical-guard` closed for `ci/lanes.sh` from the
# other direction. This is the control case that could not pass before the
# fix — asserted explicitly below, not just implied by "ok".
: > "$WORK/ssh-cmds.txt"
SLASH_BRANCH="scratch-check/slash-case"
R="$(build_repo "" "$SLASH_BRANCH")"
SLASH_OUT="$(cd "$R" && env -i \
    PATH="$WORK/bin:/usr/bin:/bin" HOME="$WORK/home" TERM=dumb \
    RUSTMC_REMOTE=fakehost RUSTMC_REMOTE_DIR=rustmc-selftest-gate \
    RUSTMC_GATE_LOCK_DIR="$WORK/gatelock" \
    ./ci/handoff.sh "$SLASH_BRANCH" --dry-run 2>&1)"
SLASH_RC=$?
SLASH_WORD="$(printf '%s\n' "$SLASH_OUT" | head -1 | awk '{print $1}')"
if [ "$SLASH_WORD" = GREEN ] && [ "$SLASH_RC" = 0 ]; then
    PASS=$((PASS + 1))
    printf '  ok    %-22s -> %-7s exit %s\n' "slash in branch name" "$SLASH_WORD" "$SLASH_RC"
else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %-22s -> %-7s exit %s   (wanted GREEN / exit 0)\n' \
           "slash in branch name" "$SLASH_WORD" "$SLASH_RC"
    printf '%s\n' "$SLASH_OUT" | sed 's/^/          /'
fi
# The stronger assertion: a REAL log file exists, flat in the gate directory
# (no "gate-scratch-check/" subdirectory), and it is the one this run wrote —
# not just that the verdict line happened to reach stdout despite the log.
if [ -d "$WORK/home/rustmc-private/gates/gate-scratch-check" ]; then
    FAIL=$((FAIL + 1))
    printf '  FAIL  %-22s -> the slash was read as a directory, not sanitised\n' "log stayed flat"
else
    SLASH_LOG="$(ls -1 "$WORK/home/rustmc-private/gates/"gate-scratch-check-slash-case-*.log 2>/dev/null | head -1)"
    if [ -n "$SLASH_LOG" ] && [ -s "$SLASH_LOG" ] && grep -q 'T2-GAMEPLAY GREEN' "$SLASH_LOG"; then
        PASS=$((PASS + 1))
        printf '  ok    %-22s -> %s\n' "log stayed flat" "${SLASH_LOG#"$WORK"/}"
    else
        FAIL=$((FAIL + 1))
        printf '  FAIL  %-22s -> no usable flat log for this branch (found: %s)\n' \
               "log stayed flat" "${SLASH_LOG:-none}"
    fi
fi
if [ -n "$VERBOSE" ]; then
    printf '%s\n' "$SLASH_OUT" | sed 's/^/          | /'
fi

echo "[7] stage timing: every mark is on record, in order, and the summary adds up"
#
# 248 real gate logs and no wall-clock in any of them except the corpus-lock
# waiter heartbeat — every cost figure this project has published was
# reconstructed after the fact. `mark`/`stage_summary` fix that; this proves
# it two ways, both read from the SAME log a human would open rather than
# recomputed independently: the `STAGE-TIME` sequence is monotonic
# non-decreasing, and the printed summary's rows sum to its own TOTAL within
# two seconds. `run_case` only reads the verdict word, so nothing before this
# has ever looked inside the log at all.
run_case "timing: green gate" GREEN 0
TLOG="$(ls -1t "$WORK/home/rustmc-private/gates/"*.log 2>/dev/null | head -1)"
if [ -z "$TLOG" ]; then
    FAIL=$((FAIL + 1))
    printf '  FAIL  %-22s -> no gate log found\n' "timing: marks"
else
    MARKS=$(grep -c '^STAGE-TIME ' "$TLOG")
    MONO=1; PREV=0
    while read -r _ _ ep _; do
        [ "$ep" -ge "$PREV" ] || MONO=0
        PREV=$ep
    done < <(grep '^STAGE-TIME ' "$TLOG")
    if [ "$MARKS" -ge 8 ] && [ "$MONO" -eq 1 ]; then
        PASS=$((PASS + 1))
        printf '  ok    %-22s -> %s marks, monotonic\n' "timing: marks" "$MARKS"
    else
        FAIL=$((FAIL + 1))
        printf '  FAIL  %-22s -> %s marks, monotonic=%s (wanted >=8, non-decreasing)\n' \
               "timing: marks" "$MARKS" "$MONO"
    fi

    # Read back the exact table a human sees, not the raw STAGE-TIME lines --
    # this is what caught the awk bug during development: an uninitialised
    # scalar used as an array subscript is keyed by "" and not "0", so the
    # first row's data silently landed under the wrong key and TOTAL came out
    # as a raw Unix timestamp instead of a duration. Regressing on the
    # PRINTED table, not on the input to the function that produced it, is
    # what a bug in the arithmetic in between can still be caught by.
    TOTAL="$(awk '/^  TOTAL / {v=$2} END {print v}' "$TLOG")"
    PARTSUM="$(awk '/^  [^ ]+ +[0-9]+ +[0-9.]+%$/ && $1 != "TOTAL" && $1 != "stage" {sum += $2} END {print sum + 0}' "$TLOG")"
    DIFF=$(( TOTAL - PARTSUM )); [ "$DIFF" -ge 0 ] || DIFF=$(( -DIFF ))
    if [ -n "$TOTAL" ] && [ "$DIFF" -le 2 ]; then
        PASS=$((PASS + 1))
        printf '  ok    %-22s -> parts sum to %s, TOTAL %s\n' "timing: sums to total" "$PARTSUM" "$TOTAL"
    else
        FAIL=$((FAIL + 1))
        printf '  FAIL  %-22s -> parts sum to %s, TOTAL %s (wanted within 2s)\n' \
               "timing: sums to total" "$PARTSUM" "${TOTAL:-?}"
    fi
fi

echo
echo "[8] stage timing survives the fast-fail path too — an INFRA death still has a time"
#
# The fast-fail path is where slot waste hides, and it is exactly the path a
# naive "mark at the end" implementation would report nothing for: infra()
# exits before stage 2's mark would ever be written. This is a stage-1 death,
# so the assertion is narrow on purpose — a mark for stage 1 and a summary
# block, not a mark for stages that never ran.
run_case "timing: infra death" INFRA 4 GATE_T_STAGE1=transport
TLOG="$(ls -1t "$WORK/home/rustmc-private/gates/"*.log 2>/dev/null | head -1)"
if [ -n "$TLOG" ] && grep -q '^STAGE-TIME 1-prepare ' "$TLOG" && grep -q '^== gate timing summary ==' "$TLOG"; then
    PASS=$((PASS + 1))
    printf '  ok    %-22s -> stage-1 death still marked and summarised\n' "timing: infra has a time"
else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %-22s -> no stage-1 mark or no summary on an INFRA exit\n' "timing: infra has a time"
fi

echo
echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
echo "HANDOFF-VERDICT GREEN"
