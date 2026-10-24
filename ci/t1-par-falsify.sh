#!/usr/bin/env bash
# ci/t1-par-falsify.sh — red-when-broken evidence for ci/t1-par.sh.
#
# A throwaway cargo package in a temp dir (its own target dir, no network):
# a passing lib unit test, a passing integration test, one that panics with a
# marker, one that aborts (killed by a signal), two that rendezvous, and a
# doctest. The same `cargo test --no-fail-fast` is run serially and through
# ci/t1-par.sh, and:
#
#   1  both exit 101 — a failing binary is never green under parallelism
#   2  the multiset of `test result:` lines (timings stripped) is identical,
#      so ci/t1-allowlist.py counts the same binaries/passed/failed/ignored
#   3  the failing test's `---- ... stdout ----` block and its panic marker are
#      in the parallel log, in the same order relative to the result lines
#   4  the aborted binary is reported failed (`error: N targets failed` names
#      both failing binaries)
#   5  every binary ran in the package directory with cargo's environment:
#      a test asserts CARGO_MANIFEST_DIR == cwd and CARGO_PKG_NAME at runtime
#   6  two rendezvous binaries each see the other running (no timing ratio)
#   7  with the failing and aborting tests removed both runs exit 0
#   M  mutants of ci/t1-par.sh that must each be rejected: slot statuses
#      ignored; markers left unreplaced; runner status trusted
#
# Needs cargo; ~40 s on the box, most of it one small compile.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PAR="${T1PAR_UNDER_TEST:-$ROOT/ci/t1-par.sh}"
W=$(mktemp -d "${TMPDIR:-/tmp}/t1pf.XXXXXX"); trap '[ -n "${T1PF_KEEP:-}" ] || rm -rf "$W"' EXIT
PASS=0; FAIL=0
ok() { PASS=$((PASS+1)); echo "ok   $*"; }
bad() { FAIL=$((FAIL+1)); echo "FAIL $*"; }
export CARGO_TARGET_DIR="$W/target" CARGO_NET_OFFLINE=true
unset T1_JOBS

mk() {  # dir with-reds(0|1)
    local d=$1
    mkdir -p "$d/src" "$d/tests"
    cat > "$d/Cargo.toml" <<'EOF'
[package]
name = "t1pf"
version = "0.1.0"
edition = "2021"
[workspace]
EOF
    cat > "$d/src/lib.rs" <<'EOF'
/// ```
/// assert_eq!(t1pf::two(), 2);
/// ```
pub fn two() -> u32 { 2 }
#[cfg(test)]
mod tests { #[test] fn unit_ok() { assert_eq!(super::two(), 2); } }
EOF
    echo '#[test] fn pass_ok() { assert_eq!(t1pf::two(), 2); }' > "$d/tests/pass.rs"
    cat > "$d/tests/env.rs" <<'EOF'
#[test] fn cargo_env_and_cwd() {
    let md = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR at runtime");
    assert_eq!(std::path::PathBuf::from(md).canonicalize().unwrap(), std::env::current_dir().unwrap().canonicalize().unwrap());
    assert_eq!(std::env::var("CARGO_PKG_NAME").unwrap(), "t1pf");
}
EOF
    # The two "sleepers" are a RENDEZVOUS, not a timing: with T1PF_MEET set,
    # each writes its own start marker and waits for the other's. Only two
    # binaries RUNNING AT THE SAME TIME can both see each other; run one after
    # the other, the first gives up (cap 20 s, a bound on a hang and never a
    # comparison) and records that it was alone. The tests pass either way,
    # so the result lines stay comparable; case 6 reads the markers.
    for me in a b; do
        peer=b; [ "$me" = b ] && peer=a
        cat > "$d/tests/slow_$me.rs" <<RS
#[test] fn slow_$me() {
    let Ok(d) = std::env::var("T1PF_MEET") else { return };
    let d = std::path::Path::new(&d);
    std::fs::write(d.join("$me"), "").unwrap();
    let t = std::time::Instant::now();
    while !d.join("$peer").exists() {
        if t.elapsed() > std::time::Duration::from_secs(20) {
            std::fs::write(d.join("$me-alone"), "").unwrap();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::write(d.join("$me-met"), "").unwrap();
}
RS
    done
    if [ "$2" = 1 ]; then
        echo '#[test] fn fails_marker() { println!("T1PF-MARKER-7731"); panic!("t1pf injected red"); }' > "$d/tests/fail.rs"
        echo '#[test] fn aborts() { std::process::abort(); }' > "$d/tests/zabort.rs"
    fi
}
results() { grep -E '^test result:' "$1" | sed -E 's/; finished in [0-9.]+s//' | sort; }

mk "$W/p" 1
( cd "$W/p" && cargo test -q --no-fail-fast --no-run >/dev/null 2>&1 )   # compile once, time the runs only
s=$(date +%s%N); ( cd "$W/p" && cargo test -q --no-fail-fast ) > "$W/serial.log" 2>&1; SRC=$?; sw=$(( ($(date +%s%N)-s)/1000000 ))
mkdir -p "$W/meet"
s=$(date +%s%N); ( cd "$W/p" && T1PF_MEET="$W/meet" T1_JOBS=6 bash "$PAR" test -q --no-fail-fast ) > "$W/par.log" 2>&1; PRC=$?; pw=$(( ($(date +%s%N)-s)/1000000 ))

[ "$SRC" = 101 ] && [ "$PRC" = 101 ] && ok "1 serial and parallel both exit 101 on a red" || bad "1 serial rc=$SRC parallel rc=$PRC (want 101/101)"
if [ -n "$(results "$W/serial.log")" ] && [ "$(results "$W/serial.log")" = "$(results "$W/par.log")" ]; then
    ok "2 identical test-result lines ($(results "$W/par.log" | wc -l) binaries incl. doctests)"
else bad "2 result lines differ:"; diff <(results "$W/serial.log") <(results "$W/par.log") | head; fi
if grep -q '^---- fails_marker stdout ----' "$W/par.log" && grep -q 'T1PF-MARKER-7731' "$W/par.log" && ! grep -q '@@T1PAR-SLOT' "$W/par.log"; then
    ok "3 the failing test's stdout block and marker are in the log; no slot marker left"
else bad "3 failure block missing or a slot marker left unreplaced"; fi
if grep -q '^error: 2 targets failed:' "$W/par.log" && grep -q 'zabort' "$W/par.log" && grep -A3 '^error: 2 targets failed' "$W/par.log" | grep -c '`fail-' >/dev/null; then
    ok "4 the aborted binary counts as failed: error: 2 targets failed (fail, zabort)"
else bad "4 $(grep -A3 'targets failed' "$W/par.log" | tr '\n' ' ')"; fi
if grep -E '^test result: ok\. 1 passed' "$W/par.log" >/dev/null && ! grep -q 'cargo_env_and_cwd.*FAILED\|---- cargo_env_and_cwd stdout' "$W/par.log"; then
    ok "5 CARGO_MANIFEST_DIR == cwd and CARGO_PKG_NAME at runtime, under the runner"
else bad "5 the env/cwd test failed under the runner"; fi
# 6: structural, never a ratio of wall clocks (which went red at load 15 on
# the shard lane's T0 and green on rerun). Walls are printed, not judged.
if [ -e "$W/meet/a-met" ] && [ -e "$W/meet/b-met" ]; then ok "6 slow_a and slow_b each saw the other running: the binaries ran concurrently (serial ${sw} ms, parallel ${pw} ms, not judged)"
else bad "6 the rendezvous binaries did not run at the same time: $(ls "$W/meet" | tr '\n' ' ')"; fi

mk "$W/g" 0
( cd "$W/g" && cargo test -q --no-fail-fast ) > "$W/gs.log" 2>&1; GS=$?
( cd "$W/g" && T1_JOBS=6 bash "$PAR" test -q --no-fail-fast ) > "$W/gp.log" 2>&1; GP=$?
[ "$GS" = 0 ] && [ "$GP" = 0 ] && ok "7 all-green package: both exit 0" || bad "7 green package rc serial=$GS parallel=$GP"

if [ -z "${T1PAR_NO_MUTANTS:-}" ]; then
    mut() {
        local m="$W/mut-$1.sh"
        perl -0pe "$2" "$PAR" > "$m"
        cmp -s "$m" "$PAR" && { bad "M $1: mutation did not apply"; return; }
        cp "$ROOT/ci/t1-par-runner.sh" "$W/t1-par-runner.sh"
        mkdir -p "$W/mci"; cp "$m" "$W/mci/t1-par.sh"; cp "$ROOT/ci/t1-par-runner.sh" "$W/mci/t1-par-runner.sh"
        ( cd "$W/p" && T1_JOBS=6 bash "$W/mci/t1-par.sh" test -q --no-fail-fast ) > "$W/m.log" 2>&1; local rc=$?
        if [ "$rc" = 101 ] && grep -q '^---- fails_marker stdout ----' "$W/m.log" && ! grep -q '@@T1PAR-SLOT' "$W/m.log" \
           && [ "$(results "$W/serial.log")" = "$(results "$W/m.log")" ]; then bad "M $1: a broken t1-par.sh passed"
        else ok "M $1: rejected (rc=$rc)"; fi
    }
    mut slots-ignored   's/if \[ "\$FAILED" -gt 0 \]; then exit 101; fi/:/'
    mut markers-kept    's/\^\@\@T1PAR-SLOT \[0-9\]\+\@\@\$/^NEVER-MATCHES\$/'
    mut runner-trusted  's/if \[ "\$rc" != 0 \]; then\n        FAILED/if false; then\n        FAILED/'
    # And case 6 must go red when the parallelism goes: a runner with ONE slot
    # runs the binaries one at a time, so the rendezvous cannot happen.
    mkdir -p "$W/one"; cp "$PAR" "$W/one/t1-par.sh"
    perl -pe 's/^J="\$\{T1PAR_JOBS:-[0-9]+\}"/J=1/' "$ROOT/ci/t1-par-runner.sh" > "$W/one/t1-par-runner.sh"
    chmod +x "$W/one/t1-par-runner.sh"
    if cmp -s "$W/one/t1-par-runner.sh" "$ROOT/ci/t1-par-runner.sh"; then bad "M one-slot: mutation did not apply"
    else
        rm -rf "$W/meet1"; mkdir -p "$W/meet1"
        ( cd "$W/p" && T1PF_MEET="$W/meet1" T1_JOBS=6 bash "$W/one/t1-par.sh" test -q --no-fail-fast ) > "$W/one.log" 2>&1
        # The mutant must have RUN the rendezvous binaries (a start marker
        # exists) — a mutant that fails to launch anything proves nothing.
        if [ ! -e "$W/meet1/a" ] && [ ! -e "$W/meet1/b" ]; then bad "M one-slot: the mutant never ran the rendezvous binaries"
        elif [ -e "$W/meet1/a-met" ] && [ -e "$W/meet1/b-met" ]; then bad "M one-slot: case 6 passed with parallelism removed"
        else ok "M one-slot: case 6's rendezvous fails with parallelism removed ($(ls "$W/meet1" | tr '\n' ' '))"; fi
    fi
fi
echo "T1-PAR-FALSIFY $([ $FAIL = 0 ] && echo GREEN || echo RED) — $PASS passed, $FAIL failed"
[ $FAIL = 0 ]
